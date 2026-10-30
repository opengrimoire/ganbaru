//! Write exclusion and blocker checks for vault snapshots and replacements.

use super::write_fence::VaultWriteFence;
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
    #[cfg(desktop)]
    app.state::<crate::chat::terminal::ChatTerminalRegistry>()
        .suspend_creation()
        .map_err(|error| format!("pause Chat terminals: {}", error.message))?;
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
    managed_write_fence: super::write_fence::ManagedVaultWriteFence,
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
            .state::<VaultWriteFence>()
            .fence_managed_writes()?;
        Ok(FrozenWrites {
            managed_write_fence,
            resume_runtimes,
        })
    })
    .await
    .map_err(|error| format!("vault writer drain worker failed: {error}"))?
}

/// Holds the central write boundaries while creating an vault snapshot.
pub(crate) struct SnapshotQuiescence {
    _database_guard: db::VaultExclusiveGuard,
    _managed_write_fence: super::write_fence::ManagedVaultWriteFence,
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

/// Briefly excludes central writers while a vault snapshot is created.
pub(crate) async fn begin_snapshot_quiescence<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<SnapshotQuiescence, String> {
    begin_reserved_quiescence(app, reserve_vault_transition()?, RunningTerminals::Continue).await
}

/// Whether shells already running in the active vault may outlive the transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RunningTerminals {
    /// The vault stays intact on disk, so existing shells keep running; only new ones pause.
    Continue,
    /// The vault contents are replaced, so a running shell blocks the transition.
    Block,
}

/// Drains native owners before excluding writes or closing the active database.
pub(crate) async fn begin_reserved_quiescence<R: Runtime>(
    app: &tauri::AppHandle<R>,
    transition: VaultTransition,
    running_terminals: RunningTerminals,
) -> Result<SnapshotQuiescence, String> {
    let blocks_terminals = running_terminals == RunningTerminals::Block;
    if blocks_terminals {
        check_terminal_blocker(app)?;
    }
    let sync_plan = crate::sync::quiesced_sync(app);
    let frozen = freeze_and_fence(app, transition).await?;
    if blocks_terminals {
        // Creation is paused now, so this count cannot grow before the database closes.
        check_terminal_blocker(app)?;
    }
    let database_guard = db::begin_vault_exclusive().await;
    db::close_all_sqlite_pools_for_restore(app).await?;
    crate::sync::seal_quiesced(app, sync_plan).await;
    Ok(SnapshotQuiescence {
        _database_guard: database_guard,
        _managed_write_fence: frozen.managed_write_fence,
        _resume_runtimes: frozen.resume_runtimes,
    })
}

#[cfg(desktop)]
fn check_terminal_blocker<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let running = app
        .state::<crate::chat::terminal::ChatTerminalRegistry>()
        .live_count()
        .map_err(|error| format!("check running Chat terminals: {}", error.message))?;
    if running > 0 {
        Err("Close running Chat terminals before moving or replacing this vault".to_string())
    } else {
        Ok(())
    }
}

#[cfg(mobile)]
fn check_terminal_blocker<R: Runtime>(_app: &tauri::AppHandle<R>) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let fence = Arc::new(VaultWriteFence::default());
        let managed_write_fence = fence.fence_managed_writes().unwrap();
        let resumed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let callback_gate = gate.clone();
        let callback_database = database_gate.clone();
        let callback_fence = fence.clone();
        let callback_resumed = resumed.clone();
        let quiescence = SnapshotQuiescence {
            _database_guard: database_guard,
            _managed_write_fence: managed_write_fence,
            _resume_runtimes: ResumeRuntimes {
                resume: Some(Box::new(move || {
                    assert!(callback_database.try_write().is_ok());
                    assert!(callback_fence.fence_managed_writes().is_ok());
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
        let fence = Arc::new(VaultWriteFence::default());
        let managed_write_fence = fence.fence_managed_writes().unwrap();
        let callback_fence = fence.clone();
        let callback_gate = gate.clone();
        let frozen = FrozenWrites {
            managed_write_fence,
            resume_runtimes: ResumeRuntimes {
                resume: Some(Box::new(move || {
                    assert!(callback_fence.fence_managed_writes().is_ok());
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
        let fence = Arc::new(VaultWriteFence::default());
        let managed_write_fence = fence.fence_managed_writes().unwrap();
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let callback_attempts = attempts.clone();
        let quiescence = SnapshotQuiescence {
            _database_guard: database_guard,
            _managed_write_fence: managed_write_fence,
            _resume_runtimes: ResumeRuntimes {
                resume: Some(Box::new(move || {
                    assert!(database_gate.try_write().is_ok());
                    assert!(fence.fence_managed_writes().is_ok());
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
}
