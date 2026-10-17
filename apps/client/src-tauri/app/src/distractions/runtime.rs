//! One native desktop observation owner, independent of WebView lifecycle.

mod policy;

use super::authorization::configured_close_authorization;
use super::*;
use ganbaru_distractions::accounting;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, mpsc, oneshot};

const OBSERVATION_INTERVAL: Duration = Duration::from_secs(5);
const MAX_ACTION_AGE: Duration = Duration::from_secs(5);
const FREEZE_TIMEOUT: Duration = Duration::from_secs(5);
const LIMIT_CLOSE_INTERVAL: Duration = Duration::from_secs(60);
const MAX_CLOSES_PER_OBSERVATION: usize = 8;
const MAX_PENDING_INTERVALS: usize = 4_000;
const MAX_CLOSE_KEYS: usize = 4_096;
const MAX_NOTIFICATION_TIMES: usize = 5;

#[derive(Default)]
struct Frame {
    generation: u64,
    active: bool,
    stopped: bool,
}

struct Control {
    frame: Mutex<Frame>,
    generation: AtomicU64,
    wake: Notify,
}

impl Control {
    fn current(&self, generation: u64) -> Result<std::sync::MutexGuard<'_, Frame>, String> {
        let frame = self
            .frame
            .lock()
            .map_err(|_| "Distractions runtime control is unavailable")?;
        if !frame.active || frame.generation != generation {
            return Err("Distractions observation was invalidated".into());
        }
        Ok(frame)
    }
}

struct Freeze {
    generation: u64,
    reply: oneshot::Sender<Result<(), String>>,
}

struct RuntimeState {
    control: Arc<Control>,
    sender: mpsc::Sender<Freeze>,
    foreground: Arc<Mutex<(Instant, DistractionsForegroundDesktopAppStatus)>>,
    projection: Mutex<Option<(u64, Instant, limits_read::UsageProjection)>>,
}

struct Context {
    generation: u64,
    vault_id: String,
    root_path: PathBuf,
    ownership: vault::ownership::VaultOwnershipStatus,
    config: Value,
    limits: limits::LimitsConfig,
    observation: accounting::Observation,
    foreground: DistractionsForegroundDesktopAppStatus,
    phase: Option<DistractionsRuntimeState>,
    phase_matches: Vec<DistractionsRunningDesktopAppMatch>,
    open_matches: Vec<DistractionsRunningDesktopAppMatch>,
}

impl Context {
    fn validate_binding(&self, app: &tauri::AppHandle) -> Result<(), String> {
        if vault::active_vault_id(app)? != self.vault_id
            || vault::active_vault_path(app)? != self.root_path
            || app
                .state::<vault::ownership::VaultOwnershipManager>()
                .status(&self.vault_id)?
                != self.ownership
        {
            return Err("Distractions vault context changed".into());
        }
        Ok(())
    }

    /// Guard process-closing actions, which must act on a recent foreground and process scan.
    fn validate(&self, app: &tauri::AppHandle) -> Result<(), String> {
        if Instant::now().saturating_duration_since(self.observation.monotonic) > MAX_ACTION_AGE {
            return Err("Distractions observation is stale".into());
        }
        self.validate_configuration(app)
    }

    /// Guard projection publication, which stays valid while the vault and rules are unchanged.
    fn validate_configuration(&self, app: &tauri::AppHandle) -> Result<(), String> {
        self.validate_binding(app)?;
        let config = limits_read::config_at(&self.root_path.join(VAULT_CONFIG_FILE))?;
        if config.get("distractions") != self.config.get("distractions") {
            return Err("Distractions configuration changed".into());
        }
        Ok(())
    }
}

/// Install exactly one serialized owner with bounded control requests and blocking work.
pub(crate) fn setup(app: &tauri::AppHandle) {
    if app.try_state::<RuntimeState>().is_some() {
        return;
    }
    let control = Arc::new(Control {
        frame: Mutex::new(Frame {
            generation: 1,
            active: true,
            stopped: false,
        }),
        generation: AtomicU64::new(1),
        wake: Notify::new(),
    });
    let foreground = Arc::new(Mutex::new((
        Instant::now(),
        unavailable_foreground_desktop_app_status("native observation has not run"),
    )));
    let (sender, mut receiver) = mpsc::channel::<Freeze>(4);
    app.manage(RuntimeState {
        control: control.clone(),
        sender,
        foreground: foreground.clone(),
        projection: Mutex::new(None),
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut owner = Owner {
            app,
            control,
            foreground,
            accounting: accounting::Accounting::default(),
            binding: None,
            session: None,
            pending: None,
            attempts: HashMap::new(),
            notifications: Vec::new(),
            last_error: None,
        };
        loop {
            tokio::select! {
                biased;
                request = receiver.recv() => {
                    let Some(request) = request else { break };
                    owner.accounting.clear(); owner.binding = None;
                    let result = owner.flush_pending().await;
                    let current = owner.control.generation.load(Ordering::Acquire);
                    let result = if current != request.generation { Err("Distractions freeze was superseded".into()) } else { result };
                    let _ = request.reply.send(result);
                }
                _ = owner.control.wake.notified() => { owner.tick().await; }
                _ = tokio::time::sleep(OBSERVATION_INTERVAL) => { owner.tick().await; }
            }
        }
    });
}

/// UI reads cannot race the accounting worker for query admission or trigger publication.
pub(super) fn usage_projection<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<limits_read::UsageProjection, String> {
    let runtime = app
        .try_state::<RuntimeState>()
        .ok_or("native Distractions runtime is unavailable")?;
    let generation = runtime.control.generation.load(Ordering::Acquire);
    let _guard = runtime.control.current(generation)?;
    let cached = runtime
        .projection
        .lock()
        .map_err(|_| "native usage projection is unavailable")?;
    let (published_generation, published_at, projection) = cached
        .as_ref()
        .ok_or("native usage observation is not ready")?;
    if *published_generation != generation
        || published_at.elapsed() > OBSERVATION_INTERVAL + MAX_ACTION_AGE
    {
        return Err("native usage projection is stale".into());
    }
    Ok(projection.clone())
}

/// Capture the publication generation before any asynchronous snapshot work begins.
pub(crate) fn capture_publication_generation<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<u64> {
    app.try_state::<RuntimeState>()
        .map(|runtime| runtime.control.generation.load(Ordering::Acquire))
}

/// Check a synchronous caller's current publication authority.
pub(crate) fn publication_token<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<u64>, String> {
    let Some(runtime) = app.try_state::<RuntimeState>() else {
        return Ok(None);
    };
    let generation = runtime.control.generation.load(Ordering::Acquire);
    drop(runtime.control.current(generation)?);
    Ok(Some(generation))
}

/// Serialize file publication with revocation so old reads cannot republish after shutdown.
pub(super) fn publish<R: Runtime, T>(
    app: &tauri::AppHandle<R>,
    expected: Option<u64>,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let Some(runtime) = app.try_state::<RuntimeState>() else {
        return action();
    };
    let generation = expected.ok_or("Distractions publication predates runtime initialization")?;
    let _guard = runtime.control.current(generation)?;
    action()
}

fn invalidate<R: Runtime>(app: &tauri::AppHandle<R>, stop: bool) -> Result<u64, String> {
    let Some(runtime) = app.try_state::<RuntimeState>() else {
        state_files::clear_enforcement_state_files(&state_path(app)?, &limit_state_path(app)?)?;
        return Ok(0);
    };
    let mut frame = runtime
        .control
        .frame
        .lock()
        .map_err(|_| "Distractions runtime control is unavailable")?;
    frame.active = false;
    frame.stopped |= stop;
    frame.generation = frame.generation.wrapping_add(1);
    runtime
        .control
        .generation
        .store(frame.generation, Ordering::Release);
    state_files::clear_enforcement_state_files(&state_path(app)?, &limit_state_path(app)?)?;
    if let Ok(mut foreground) = runtime.foreground.lock() {
        *foreground = (
            Instant::now(),
            unavailable_foreground_desktop_app_status("native observation was invalidated"),
        );
    }
    runtime.control.wake.notify_one();
    Ok(frame.generation)
}

/// Cancel destructive work and file publication before an irreversible process exit.
pub(crate) fn stop_and_clear<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    invalidate(app, true).map(|_| ())
}

/// Preserve already captured interval batches before a handoff closes the vault database.
pub(crate) async fn stop_for_vault_handoff<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let Some(runtime) = app.try_state::<RuntimeState>() else {
        return Ok(());
    };
    let generation = invalidate(app, false)?;
    let (reply, result) = oneshot::channel();
    tokio::time::timeout(FREEZE_TIMEOUT, async {
        runtime
            .sender
            .send(Freeze { generation, reply })
            .await
            .map_err(|_| "Distractions owner stopped".to_string())?;
        result
            .await
            .map_err(|_| "Distractions owner did not acknowledge freeze".to_string())?
    })
    .await
    .map_err(|_| {
        "Distractions observation has not drained; vault handoff was cancelled".to_string()
    })?
}

/// Reactivate only after vault activation or handoff rollback has finished.
pub(crate) fn resume_after_vault_handoff<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let Some(runtime) = app.try_state::<RuntimeState>() else {
        return Ok(());
    };
    let mut frame = runtime
        .control
        .frame
        .lock()
        .map_err(|_| "Distractions runtime control is unavailable")?;
    if frame.stopped {
        return Ok(());
    }
    frame.generation = frame.generation.wrapping_add(1);
    frame.active = true;
    runtime
        .control
        .generation
        .store(frame.generation, Ordering::Release);
    state_files::clear_enforcement_state_files(&state_path(app)?, &limit_state_path(app)?)?;
    runtime.control.wake.notify_one();
    Ok(())
}

/// Serialize persisted rule edits with the final close boundary and revoke old observations.
pub(crate) fn commit_configuration<R: Runtime>(
    app: &tauri::AppHandle<R>,
    write: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let Some(runtime) = app.try_state::<RuntimeState>() else {
        return write();
    };
    let mut frame = runtime
        .control
        .frame
        .lock()
        .map_err(|_| "Distractions runtime control is unavailable")?;
    let result = write();
    frame.generation = frame.generation.wrapping_add(1);
    runtime
        .control
        .generation
        .store(frame.generation, Ordering::Release);
    runtime.control.wake.notify_one();
    result
}

pub(super) fn foreground_projection<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> DistractionsForegroundDesktopAppStatus {
    let Some(runtime) = app.try_state::<RuntimeState>() else {
        return unavailable_foreground_desktop_app_status("native runtime is unavailable");
    };
    let generation = runtime.control.generation.load(Ordering::Acquire);
    let Ok(_guard) = runtime.control.current(generation) else {
        return unavailable_foreground_desktop_app_status("native runtime is frozen");
    };
    let Ok((observed, status)) = runtime.foreground.lock().map(|value| value.clone()) else {
        return unavailable_foreground_desktop_app_status(
            "native observation state is unavailable",
        );
    };
    if observed.elapsed() > OBSERVATION_INTERVAL + MAX_ACTION_AGE {
        return unavailable_foreground_desktop_app_status("native foreground observation is stale");
    }
    status
}

struct Pending {
    vault_id: String,
    device_id: String,
    rows: Vec<DistractionsUsageSampleRow>,
}
struct Owner {
    app: tauri::AppHandle,
    control: Arc<Control>,
    foreground: Arc<Mutex<(Instant, DistractionsForegroundDesktopAppStatus)>>,
    accounting: accounting::Accounting,
    binding: Option<(u64, String, u64)>,
    session: Option<String>,
    pending: Option<Pending>,
    attempts: HashMap<String, Instant>,
    notifications: Vec<Instant>,
    last_error: Option<String>,
}

impl Owner {
    async fn flush_pending(&mut self) -> Result<(), String> {
        if let Some(pending) = &self.pending {
            crate::distractions::linked::enqueue_native_samples(
                &self.app,
                &pending.vault_id,
                &pending.device_id,
                &pending.rows,
            )
            .await?;
        }
        self.pending = None;
        Ok(())
    }

    async fn tick(&mut self) {
        let result = self.observe().await;
        if let Err(error) = result {
            self.accounting.clear();
            if self.last_error.as_ref() != Some(&error) {
                eprintln!("native Distractions runtime: {error}");
            }
            self.last_error = Some(error);
        } else {
            self.last_error = None;
        }
    }

    async fn observe(&mut self) -> Result<(), String> {
        self.flush_pending().await?;
        let generation = self.control.generation.load(Ordering::Acquire);
        if self.control.current(generation).is_err() {
            self.accounting.clear();
            return Ok(());
        }
        let app = self.app.clone();
        let control = self.control.clone();
        let context = tauri::async_runtime::spawn_blocking(move || {
            let Some(root_path) = vault::available_active_vault_path(&app)? else {
                return Ok(None);
            };
            let vault_id = vault::active_vault_id(&app)?;
            let ownership = app
                .state::<vault::ownership::VaultOwnershipManager>()
                .status(&vault_id)?;
            let config = limits_read::config_at(&root_path.join(VAULT_CONFIG_FILE))?;
            let limits = limits::parse_config(&config)?;
            let phase = read_fresh_runtime_state(&state_path(&app)?, now_utc());
            let phase_rules = policy::phase_rules(&config, phase.as_ref())?;
            let usage_rules = policy::usage_rules(&limits);
            let monotonic = Instant::now();
            let wall_ms = now_epoch_ms();
            let foreground = if usage_rules.is_empty() {
                unavailable_foreground_desktop_app_status("no desktop usage source is configured")
            } else {
                foreground_desktop_app_status()
            };
            let cancelled = || {
                control.generation.load(Ordering::Acquire) != generation
                    || monotonic.elapsed() > MAX_ACTION_AGE
            };
            let phase_matches = list_blocked_desktop_app_matches(phase_rules, cancelled);
            let open_matches = if !foreground.available
                && foreground
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.to_ascii_lowercase().contains("wayland"))
            {
                list_blocked_desktop_app_matches(usage_rules, cancelled)
            } else {
                Vec::new()
            };
            let sources = if let Some(source) = policy::foreground_source(&limits, &foreground) {
                vec![source]
            } else {
                policy::open_sources(&open_matches)
            };
            let observation = accounting::Observation {
                wall_ms,
                monotonic,
                sources,
                zone: ganbaru_civil_time::system_zone()?,
            };
            let context = Context {
                generation,
                vault_id,
                root_path,
                ownership,
                config,
                limits,
                observation,
                foreground,
                phase,
                phase_matches,
                open_matches,
            };
            drop(control.current(generation)?);
            context.validate(&app)?;
            Ok::<_, String>(Some(context))
        })
        .await
        .map_err(|error| format!("desktop observation worker: {error}"))??;
        let Some(context) = context else {
            self.accounting.clear();
            self.binding = None;
            return Ok(());
        };
        if self.session.is_none() {
            self.session = Some(crate::distractions::linked::new_accounting_session()?);
        }
        let binding = (
            generation,
            context.vault_id.clone(),
            context.ownership.generation,
        );
        if self.binding.as_ref() != Some(&binding) {
            self.accounting.clear();
            self.attempts.clear();
            self.binding = Some(binding);
        }
        let identity = format!(
            "{}|{}|{}",
            context.vault_id,
            context.ownership.device_id,
            self.session
                .as_deref()
                .ok_or("desktop interval session is unavailable")?
        );
        let rows = self
            .accounting
            .observe(context.observation.clone(), &identity)?;
        if rows.len() > MAX_PENDING_INTERVALS {
            return Err("desktop intervals exceed the pending limit".into());
        }
        if !rows.is_empty() {
            self.pending = Some(Pending {
                vault_id: context.vault_id.clone(),
                device_id: context.ownership.device_id.clone(),
                rows,
            });
            self.flush_pending().await?;
        }
        if let Ok(mut foreground) = self.foreground.lock() {
            *foreground = (context.observation.monotonic, context.foreground.clone());
        }
        let projection = limits_read::derive_usage_projection(self.app.clone()).await?;
        // A slow SQLite read must not discard accepted totals; close actions recheck their age.
        context.validate_configuration(&self.app)?;
        publish(&self.app, Some(generation), || {
            let runtime = self.app.state::<RuntimeState>();
            let mut cached = runtime
                .projection
                .lock()
                .map_err(|_| "native usage projection is unavailable")?;
            *cached = Some((generation, Instant::now(), projection.clone()));
            Ok(())
        })?;
        self.enforce(context, projection.totals).await
    }

    async fn enforce(
        &mut self,
        context: Context,
        totals: Vec<limits::BudgetTotal>,
    ) -> Result<(), String> {
        self.attempts
            .retain(|_, attempted| attempted.elapsed() < LIMIT_CLOSE_INTERVAL);
        let mut targets = context
            .phase_matches
            .iter()
            .cloned()
            .map(|observed| (observed, None))
            .collect::<Vec<_>>();
        for observed in &context.open_matches {
            let source = accounting::UsageSource {
                key: app_name_key(&observed.process_name),
                label: observed.app_name.clone(),
            };
            if let Some((identity, name)) =
                policy::exhausted_rule(&context.limits, &totals, &source)
            {
                let key = format!("{:?}|{}", identity, source.key);
                if self.allow_limit_attempt(key) {
                    let mut observed = observed.clone();
                    observed.rule_identity = identity;
                    targets.push((observed, Some(name)));
                }
            }
        }
        let foreground_limit = policy::foreground_source(&context.limits, &context.foreground)
            .and_then(|source| {
                policy::exhausted_rule(&context.limits, &totals, &source).map(|rule| (source, rule))
            })
            .filter(|(source, (identity, _))| {
                self.allow_limit_attempt(format!("{:?}|{}", identity, source.key))
            });
        let context = Arc::new(context);
        let mut closed = Vec::new();
        for (observed, limit_name) in targets.into_iter().take(MAX_CLOSES_PER_OBSERVATION) {
            let app = self.app.clone();
            let control = self.control.clone();
            let context = context.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                let mut guard = None;
                let signalled = process_control::close_desktop_process_checked(
                    &app,
                    DistractionsCloseDesktopAppRequest {
                        process_id: observed.process_id,
                        process_name: observed.process_name.clone(),
                        process_identity: observed.process_identity.clone(),
                        rule_identity: observed.rule_identity.clone(),
                    },
                    || {
                        guard = None;
                        guard = Some(control.current(context.generation)?);
                        context.validate(&app)
                    },
                )?;
                Ok::<_, String>(signalled.then_some((observed, limit_name)))
            })
            .await
            .map_err(|error| format!("desktop close worker: {error}"))?;
            match result {
                Ok(Some(value)) => closed.push(value),
                Ok(None) => {}
                Err(error) => {
                    eprintln!("native Distractions close declined: {error}");
                    break;
                }
            }
        }
        if let Some((source, (identity, limit_name))) = foreground_limit {
            let app = self.app.clone();
            let control = self.control.clone();
            let context = context.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                let status = &context.foreground;
                let expected = DistractionsForegroundDesktopAppExpectation {
                    app_name: status.app_name.clone(),
                    process_name: status.process_name.clone(),
                    process_id: status.process_id,
                    process_identity: status.process_identity.clone(),
                    match_names: status.match_names.clone(),
                };
                let mut guard = None;
                close_current_foreground_desktop_app(expected, &mut |current| {
                    guard = None;
                    guard = Some(control.current(context.generation)?);
                    context.validate(&app)?;
                    let authorization = load_close_authorization(&app, &identity)?;
                    validate_names_authorized(
                        foreground_status_match_names(current),
                        &authorization,
                    )
                })?;
                Ok::<_, String>((source.label, limit_name))
            })
            .await
            .map_err(|error| format!("foreground close worker: {error}"))?;
            match result {
                Ok((label, limit_name)) => {
                    self.notify(label, Some(limit_name));
                }
                Err(error) => eprintln!("native Distractions foreground close declined: {error}"),
            }
        }
        for (observed, limit_name) in closed {
            if context.ownership.can_write {
                let _permit = self
                    .app
                    .state::<vault::ownership::VaultOwnershipManager>()
                    .acquire_managed_write(&context.vault_id)?;
                context.validate_binding(&self.app)?;
                let pool = crate::db::connect_sqlite(
                    self.app.clone(),
                    format!("sqlite:{}", vault::APP_SQLITE_FILE),
                )
                .await?;
                context.validate_binding(&self.app)?;
                let event =
                    usage::normalize_desktop_block_event(DistractionsDesktopBlockEventInput {
                        app_name: observed.app_name.clone(),
                        process_name: Some(observed.process_name),
                        process_id: Some(observed.process_id),
                    })?;
                usage::insert_desktop_block_event(
                    &pool,
                    event,
                    context.phase.as_ref(),
                    &now_utc().to_rfc3339_opts(SecondsFormat::Millis, true),
                )
                .await?;
            }
            self.notify(observed.app_name, limit_name);
        }
        Ok(())
    }

    fn allow_limit_attempt(&mut self, key: String) -> bool {
        if self.attempts.contains_key(&key) || self.attempts.len() >= MAX_CLOSE_KEYS {
            return false;
        }
        self.attempts.insert(key, Instant::now());
        true
    }

    fn notify(&mut self, label: String, limit: Option<String>) {
        self.notifications
            .retain(|time| time.elapsed() < LIMIT_CLOSE_INTERVAL);
        if self.notifications.len() >= MAX_NOTIFICATION_TIMES {
            return;
        }
        self.notifications.push(Instant::now());
        if let Some(limit) = limit {
            crate::notifications::desktop::show_distractions_desktop_limit_notification(
                self.app.clone(),
                label,
                limit,
                self.app.state(),
            );
        } else {
            crate::notifications::desktop::show_distractions_desktop_block_notification(
                self.app.clone(),
                label,
                self.app.state(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoked_generations_cannot_publish_or_close_after_resume() {
        let control = Control {
            frame: Mutex::new(Frame {
                generation: 1,
                active: true,
                stopped: false,
            }),
            generation: AtomicU64::new(1),
            wake: Notify::new(),
        };
        assert!(control.current(1).is_ok());
        {
            let mut frame = control.frame.lock().unwrap();
            frame.active = false;
            frame.generation = 2;
        }
        assert!(control.current(1).is_err());
        assert!(control.current(2).is_err());
        {
            let mut frame = control.frame.lock().unwrap();
            frame.active = true;
            frame.generation = 3;
        }
        assert!(control.current(1).is_err());
        assert!(control.current(3).is_ok());
    }
}
