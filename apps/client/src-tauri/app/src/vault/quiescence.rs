//! Source-side write exclusion and blocker checks for vault handoff.

use super::ownership::VaultOwnershipManager;
use crate::db;
use std::sync::{Arc, LazyLock};
use tauri::{Manager, Runtime};

static VAULT_TRANSITION_GATE: LazyLock<Arc<tokio::sync::Mutex<()>>> =
    LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(())));

/// Excludes another selection, snapshot, or replacement until cleanup finishes.
#[derive(Clone)]
pub(crate) struct VaultTransition {
    _serial: Arc<tokio::sync::OwnedMutexGuard<()>>,
}

/// Rejects concurrent vault transitions without accumulating waiting operations.
pub(crate) fn reserve_vault_transition() -> Result<VaultTransition, String> {
    reserve_transition(&VAULT_TRANSITION_GATE)
}

fn reserve_transition(gate: &Arc<tokio::sync::Mutex<()>>) -> Result<VaultTransition, String> {
    let serial = gate.clone().try_lock_owned().map_err(|_| {
        "Another vault operation is in progress; retry after it finishes".to_string()
    })?;
    Ok(VaultTransition {
        _serial: Arc::new(serial),
    })
}

/// Releases runtime freezes after the connection gate and managed write fence drop.
struct ResumeRuntimes {
    resume: Option<Box<dyn Fn() -> Result<(), String> + Send + Sync>>,
    _transition: VaultTransition,
}

impl Drop for ResumeRuntimes {
    fn drop(&mut self) {
        if let Err(error) = self.resume_now() {
            eprintln!("resume vault runtimes: {error}");
        }
    }
}

impl ResumeRuntimes {
    fn resume_now(&mut self) -> Result<(), String> {
        self.resume.take().map_or(Ok(()), |resume| resume())
    }
}

async fn freeze_runtimes<R: Runtime>(
    app: &tauri::AppHandle<R>,
    transition: VaultTransition,
) -> Result<ResumeRuntimes, String> {
    let resume_app = app.clone();
    let resume = ResumeRuntimes {
        resume: Some(Box::new(move || super::resume_native_runtimes(&resume_app))),
        _transition: transition,
    };
    crate::sync::stop_for_vault_handoff(app).await?;
    crate::pomodoro::stop_for_vault_handoff(app).await?;
    crate::music::session::stop_for_vault_handoff(app).await?;
    #[cfg(desktop)]
    crate::distractions::runtime::stop_for_vault_handoff(app).await?;
    #[cfg(target_os = "android")]
    crate::distractions::android::runtime::stop_for_vault_handoff(app).await?;
    Ok(resume)
}

/// Keeps resume after the managed fence in every cancellation and failure path.
struct FrozenWrites {
    managed_write_fence: super::ownership::ManagedVaultWriteFence,
    resume_runtimes: ResumeRuntimes,
}

async fn freeze_and_fence<R: Runtime>(
    app: &tauri::AppHandle<R>,
    transition: VaultTransition,
) -> Result<FrozenWrites, String> {
    let resume_runtimes = freeze_runtimes(app, transition).await?;
    let fence_app = app.clone();
    // The worker owns reactivation until the blocking writer drain actually finishes.
    tauri::async_runtime::spawn_blocking(move || {
        let managed_write_fence = fence_app
            .state::<VaultOwnershipManager>()
            .fence_managed_writes()?;
        Ok(FrozenWrites {
            managed_write_fence,
            resume_runtimes,
        })
    })
    .await
    .map_err(|error| format!("vault writer drain worker failed: {error}"))?
}

/// Holds the SQLite connection gate while a source vault is frozen.
pub(crate) struct SourceQuiescence {
    state: Option<SourceQuiescenceState>,
    rollback_on_drop: bool,
}

/// Cleanup retains the transition and fences until durable rollback finishes.
struct SourceQuiescenceState {
    rollback: Option<Box<dyn FnOnce() -> Result<(), String> + Send>>,
    database_guard: Option<db::VaultExclusiveGuard>,
    frozen: FrozenWrites,
}

impl SourceQuiescenceState {
    fn finish(mut self) -> Result<(), String> {
        let rollback = self.rollback.take().map_or(Ok(()), |rollback| rollback());
        drop(self.database_guard.take());
        let FrozenWrites {
            managed_write_fence,
            mut resume_runtimes,
        } = self.frozen;
        drop(managed_write_fence);
        let resume = resume_runtimes.resume_now();
        match (rollback, resume) {
            (Ok(()), result) | (result, Ok(())) => result,
            (Err(rollback), Err(resume)) => {
                Err(format!("{rollback}; resume vault runtimes: {resume}"))
            }
        }
    }
}

impl Drop for SourceQuiescence {
    fn drop(&mut self) {
        if let Some(mut state) = self.state.take() {
            if !self.rollback_on_drop {
                state.rollback = None;
            }
            // Cancellation cannot release fences ahead of the file-backed rollback.
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = state.finish() {
                    eprintln!("cancel outgoing vault preparation: {error}");
                }
            });
        }
    }
}

/// Holds the central write boundaries while creating an ownership-neutral snapshot.
pub(crate) struct SnapshotQuiescence {
    _database_guard: db::VaultExclusiveGuard,
    _managed_write_fence: super::ownership::ManagedVaultWriteFence,
    _resume_runtimes: ResumeRuntimes,
}

impl SnapshotQuiescence {
    /// Releases both write gates and reports reactivation failure to the caller.
    pub(crate) fn finish(self) -> Result<(), String> {
        let Self {
            _database_guard,
            _managed_write_fence,
            mut _resume_runtimes,
        } = self;
        drop(_database_guard);
        drop(_managed_write_fence);
        _resume_runtimes.resume_now()
    }
}

impl SourceQuiescence {
    /// Resume an unchanged source after failure before generation commit.
    pub(crate) async fn abort(mut self) -> Result<(), String> {
        let state = self
            .state
            .take()
            .expect("source preparation retains its cleanup");
        tauri::async_runtime::spawn_blocking(move || state.finish())
            .await
            .map_err(|error| format!("outgoing preparation cleanup worker failed: {error}"))?
    }

    /// Retain pre-commit ownership only after the resumable transfer is stored.
    pub(crate) fn retain_outgoing(mut self) {
        self.retain_for_retry();
    }

    /// Keep the fences for a prepared coordinator transfer while retaining explicit abort.
    pub(crate) fn retain_for_retry(&mut self) {
        self.rollback_on_drop = false;
    }
}

/// Include a rollback failure without losing the original preparation error.
pub(crate) async fn abort_source_preparation(
    quiescence: Option<SourceQuiescence>,
    error: String,
) -> String {
    if let Some(quiescence) = quiescence
        && let Err(rollback) = quiescence.abort().await
    {
        return format!("{error}; outgoing preparation rollback failed: {rollback}");
    }
    error
}

/// Check blockers, stop playback, fence new connections, and drain SQLite work.
pub(crate) async fn begin_source_quiescence<R: Runtime>(
    app: &tauri::AppHandle<R>,
    expected_generation: u64,
    transfer_id: String,
    receiver_device_id: String,
) -> Result<SourceQuiescence, String> {
    let transition = reserve_vault_transition()?;
    let vault_id = super::active_vault_id(app)?;
    check_pomodoro_blocker(app).await?;
    check_chat_blocker(app)?;
    let sync_plan = crate::sync::quiesced_sync(app);

    let frozen = freeze_and_fence(app, transition).await?;

    let ownership_app = app.clone();
    let mut quiescence = tauri::async_runtime::spawn_blocking(move || {
        ownership_app
            .state::<VaultOwnershipManager>()
            .begin_outgoing(
                &vault_id,
                expected_generation,
                transfer_id.clone(),
                receiver_device_id,
            )?;
        Ok::<_, String>(SourceQuiescence {
            rollback_on_drop: true,
            state: Some(SourceQuiescenceState {
                rollback: Some(Box::new(move || {
                    ownership_app
                        .state::<VaultOwnershipManager>()
                        .abort_outgoing(&vault_id, &transfer_id)
                })),
                database_guard: None,
                frozen,
            }),
        })
    })
    .await
    .map_err(|error| format!("outgoing preparation worker failed: {error}"))??;
    quiescence
        .state
        .as_mut()
        .expect("source preparation retains its cleanup")
        .database_guard = Some(db::begin_vault_exclusive().await);

    let result = async {
        check_chat_blocker(app)?;
        db::close_all_sqlite_pools_for_restore(app).await?;
        crate::sync::seal_quiesced(app, sync_plan).await;
        check_drained_pomodoro_blocker(app).await?;
        check_chat_blocker(app)
    }
    .await;
    if let Err(error) = result {
        if let Err(rollback) = quiescence.abort().await {
            return Err(format!(
                "{error}; outgoing preparation rollback failed: {rollback}"
            ));
        }
        return Err(error);
    }

    Ok(quiescence)
}

/// Briefly excludes central writers while a read-only replica snapshot is created.
pub(crate) async fn begin_snapshot_quiescence<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<SnapshotQuiescence, String> {
    begin_reserved_quiescence(app, reserve_vault_transition()?).await
}

/// Drains native owners before excluding writes or closing the active database.
pub(crate) async fn begin_reserved_quiescence<R: Runtime>(
    app: &tauri::AppHandle<R>,
    transition: VaultTransition,
) -> Result<SnapshotQuiescence, String> {
    let sync_plan = crate::sync::quiesced_sync(app);
    let frozen = freeze_and_fence(app, transition).await?;
    let database_guard = db::begin_vault_exclusive().await;
    db::close_all_sqlite_pools_for_restore(app).await?;
    crate::sync::seal_quiesced(app, sync_plan).await;
    Ok(SnapshotQuiescence {
        _database_guard: database_guard,
        _managed_write_fence: frozen.managed_write_fence,
        _resume_runtimes: frozen.resume_runtimes,
    })
}

async fn check_drained_pomodoro_blocker<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let database_path = super::active_vault_path(app)?.join(super::APP_SQLITE_FILE);
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path_read_only(database_path).await?;
    let result = require_no_active_pomodoro(&pool).await;
    registry.close_all().await?;
    result
}

async fn check_pomodoro_blocker<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let pool =
        db::connect_sqlite(app.clone(), format!("sqlite:{}", super::APP_SQLITE_FILE)).await?;
    require_no_active_pomodoro(&pool).await
}

async fn require_no_active_pomodoro(pool: &sqlx::SqlitePool) -> Result<(), String> {
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pomodoro_runs WHERE ended_at IS NULL LIMIT 1)",
    )
    .fetch_one(pool)
    .await
    .map_err(|error| format!("check active Pomodoro session: {error}"))?;
    if active {
        Err("Finish or stop the active Pomodoro session before moving this vault".to_string())
    } else {
        Ok(())
    }
}

#[cfg(desktop)]
fn check_chat_blocker<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let (_, active_turns) = app
        .state::<ganbaru_chat::runtime::ChatRuntimeRegistry>()
        .process_counts()
        .map_err(|error| format!("check active Chat work: {}", error.message))?;
    if active_turns > 0 {
        Err("Wait for active Chat work to finish before moving this vault".to_string())
    } else {
        Ok(())
    }
}

#[cfg(mobile)]
fn check_chat_blocker<R: Runtime>(_app: &tauri::AppHandle<R>) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    struct SourceFixture {
        ownership: Arc<VaultOwnershipManager>,
        transition_gate: Arc<tokio::sync::Mutex<()>>,
        database_gate: Arc<tokio::sync::RwLock<()>>,
        path: std::path::PathBuf,
    }

    impl SourceFixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ganbaru-source-quiescence-{}-{}.json",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let ownership = Arc::new(VaultOwnershipManager::default());
            ownership
                .initialize_from_path(path.clone(), "desktop".into())
                .unwrap();
            ownership.status("vault").unwrap();
            ownership
                .begin_outgoing("vault", 0, "transfer".into(), "phone".into())
                .unwrap();
            Self {
                ownership,
                transition_gate: Arc::new(tokio::sync::Mutex::new(())),
                database_gate: Arc::new(tokio::sync::RwLock::new(())),
                path,
            }
        }

        fn quiescence(&self, retain_outgoing: bool) -> SourceQuiescence {
            let transition = reserve_transition(&self.transition_gate).unwrap();
            let ownership = self.ownership.clone();
            let resume_ownership = ownership.clone();
            let transition_gate = self.transition_gate.clone();
            SourceQuiescence {
                rollback_on_drop: true,
                state: Some(SourceQuiescenceState {
                    rollback: Some(Box::new(move || {
                        assert!(ownership.fence_managed_writes().is_err());
                        ownership.abort_outgoing("vault", "transfer")
                    })),
                    database_guard: None,
                    frozen: FrozenWrites {
                        managed_write_fence: self.ownership.fence_managed_writes().unwrap(),
                        resume_runtimes: ResumeRuntimes {
                            resume: Some(Box::new(move || {
                                assert!(resume_ownership.fence_managed_writes().is_ok());
                                assert!(reserve_transition(&transition_gate).is_err());
                                assert_eq!(
                                    resume_ownership.status("vault")?.can_write,
                                    !retain_outgoing
                                );
                                Ok(())
                            })),
                            _transition: transition,
                        },
                    },
                }),
            }
        }

        async fn wait_for_cleanup(&self) {
            let serial = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                self.transition_gate.lock(),
            )
            .await
            .unwrap();
            drop(serial);
        }
    }

    impl Drop for SourceFixture {
        fn drop(&mut self) {
            std::fs::remove_file(&self.path).unwrap();
        }
    }

    #[tokio::test]
    async fn cancellation_while_waiting_for_database_drain_rolls_back_before_reactivation() {
        let fixture = SourceFixture::new();
        let database_reader = fixture.database_gate.clone().read_owned().await;
        let mut quiescence = fixture.quiescence(false);
        let gate = fixture.database_gate.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let operation = tokio::spawn(async move {
            started.send(()).unwrap();
            quiescence.state.as_mut().unwrap().database_guard = Some(gate.write_owned().await);
            quiescence
        });
        ready.await.unwrap();
        operation.abort();
        match operation.await {
            Err(error) => assert!(error.is_cancelled()),
            Ok(_) => panic!("source preparation completed while its database gate was held"),
        }
        drop(database_reader);
        fixture.wait_for_cleanup().await;
        assert!(fixture.ownership.status("vault").unwrap().can_write);
        assert!(reserve_transition(&fixture.transition_gate).is_ok());
    }

    #[tokio::test]
    async fn cancellation_after_database_drain_releases_all_fences_after_durable_rollback() {
        let fixture = SourceFixture::new();
        let mut quiescence = fixture.quiescence(false);
        quiescence.state.as_mut().unwrap().database_guard =
            Some(fixture.database_gate.clone().write_owned().await);
        drop(quiescence);
        fixture.wait_for_cleanup().await;
        assert!(fixture.ownership.status("vault").unwrap().can_write);
        assert!(fixture.database_gate.try_write().is_ok());
        assert!(fixture.ownership.fence_managed_writes().is_ok());
    }

    #[tokio::test]
    async fn stored_transfer_retains_preparation_and_explicit_abort_restores_owner() {
        let retained = SourceFixture::new();
        retained.quiescence(true).retain_outgoing();
        retained.wait_for_cleanup().await;
        assert!(!retained.ownership.status("vault").unwrap().can_write);
        assert!(matches!(
            retained.ownership.status("vault").unwrap().transfer_phase,
            super::super::ownership::TransferPhase::PreparingOutgoing { .. }
        ));

        let aborted = SourceFixture::new();
        let mut quiescence = aborted.quiescence(false);
        quiescence.retain_for_retry();
        quiescence.abort().await.unwrap();
        assert!(aborted.ownership.status("vault").unwrap().can_write);
    }

    #[tokio::test]
    async fn rollback_failure_is_reported_and_does_not_repeat_or_skip_reactivation() {
        let fixture = SourceFixture::new();
        let mut quiescence = fixture.quiescence(true);
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let rollback_attempts = attempts.clone();
        quiescence.state.as_mut().unwrap().rollback = Some(Box::new(move || {
            rollback_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err("ownership persistence failed".into())
        }));
        assert_eq!(
            quiescence.abort().await.unwrap_err(),
            "ownership persistence failed"
        );
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(fixture.ownership.fence_managed_writes().is_ok());
        assert!(reserve_transition(&fixture.transition_gate).is_ok());
    }

    #[test]
    fn concurrent_vault_operations_are_rejected_until_retained_cleanup_finishes() {
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let transition = reserve_transition(&gate).unwrap();
        let cleanup = transition.clone();
        assert!(reserve_transition(&gate).is_err());
        drop(transition);
        assert!(reserve_transition(&gate).is_err());
        drop(cleanup);
        assert!(reserve_transition(&gate).is_ok());
    }

    #[tokio::test]
    async fn snapshot_releases_database_and_write_fences_before_resuming_under_serial_guard() {
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let transition = reserve_transition(&gate).unwrap();
        let database_gate = Arc::new(tokio::sync::RwLock::new(()));
        let database_guard = database_gate.clone().write_owned().await;
        let ownership = Arc::new(VaultOwnershipManager::default());
        let managed_write_fence = ownership.fence_managed_writes().unwrap();
        let resumed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let callback_gate = gate.clone();
        let callback_database = database_gate.clone();
        let callback_ownership = ownership.clone();
        let callback_resumed = resumed.clone();
        let quiescence = SnapshotQuiescence {
            _database_guard: database_guard,
            _managed_write_fence: managed_write_fence,
            _resume_runtimes: ResumeRuntimes {
                resume: Some(Box::new(move || {
                    assert!(callback_database.try_write().is_ok());
                    assert!(callback_ownership.fence_managed_writes().is_ok());
                    assert!(reserve_transition(&callback_gate).is_err());
                    callback_resumed.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                })),
                _transition: transition,
            },
        };
        drop(quiescence);
        assert!(resumed.load(std::sync::atomic::Ordering::SeqCst));
        assert!(reserve_transition(&gate).is_ok());
    }

    #[test]
    fn cancelled_writer_drain_releases_fence_before_reactivation() {
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let transition = reserve_transition(&gate).unwrap();
        let ownership = Arc::new(VaultOwnershipManager::default());
        let managed_write_fence = ownership.fence_managed_writes().unwrap();
        let callback_ownership = ownership.clone();
        let callback_gate = gate.clone();
        let frozen = FrozenWrites {
            managed_write_fence,
            resume_runtimes: ResumeRuntimes {
                resume: Some(Box::new(move || {
                    assert!(callback_ownership.fence_managed_writes().is_ok());
                    assert!(reserve_transition(&callback_gate).is_err());
                    Ok(())
                })),
                _transition: transition,
            },
        };
        drop(frozen);
        assert!(reserve_transition(&gate).is_ok());
    }

    #[tokio::test]
    async fn explicit_finish_reports_reactivation_failure_without_repeating_it_on_drop() {
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let transition = reserve_transition(&gate).unwrap();
        let database_gate = Arc::new(tokio::sync::RwLock::new(()));
        let database_guard = database_gate.clone().write_owned().await;
        let ownership = Arc::new(VaultOwnershipManager::default());
        let managed_write_fence = ownership.fence_managed_writes().unwrap();
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let callback_attempts = attempts.clone();
        let quiescence = SnapshotQuiescence {
            _database_guard: database_guard,
            _managed_write_fence: managed_write_fence,
            _resume_runtimes: ResumeRuntimes {
                resume: Some(Box::new(move || {
                    assert!(database_gate.try_write().is_ok());
                    assert!(ownership.fence_managed_writes().is_ok());
                    callback_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err("reactivation unavailable".to_string())
                })),
                _transition: transition,
            },
        };
        assert_eq!(quiescence.finish().unwrap_err(), "reactivation unavailable");
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(reserve_transition(&gate).is_ok());
    }

    #[tokio::test]
    async fn pomodoro_blocker_tracks_only_open_runs() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE pomodoro_runs (ended_at TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        require_no_active_pomodoro(&pool).await.unwrap();

        sqlx::query("INSERT INTO pomodoro_runs (ended_at) VALUES (NULL)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(require_no_active_pomodoro(&pool).await.is_err());

        sqlx::query("UPDATE pomodoro_runs SET ended_at = '2026-09-13T00:00:00Z'")
            .execute(&pool)
            .await
            .unwrap();
        require_no_active_pomodoro(&pool).await.unwrap();
        pool.close().await;
    }
}
