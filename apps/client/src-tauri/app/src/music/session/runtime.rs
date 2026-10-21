//! Serialized application owner for queue intent, observations, and committed effects.

use super::{
    backend,
    models::*,
    persistence,
    policy::{SessionPolicy, Transition},
    queue,
    subscriptions::{SessionFrame, SessionNotice, Subscriptions},
};
use crate::vault::runtime_lifecycle::{LifecycleControl, LifecycleIntent};
use ganbaru_music_library::{MusicLibraryError, MusicLibraryResult};
use sqlx::SqlitePool;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, Runtime, ipc::Channel};
use tokio::sync::{mpsc, oneshot};
#[cfg(desktop)]
mod completion;
mod context;
#[cfg(desktop)]
mod context_soundscape;
mod focus;
#[cfg(test)]
mod tests;

const MESSAGE_CAPACITY: usize = 64;
const OBSERVATION_INTERVAL: Duration = Duration::from_millis(250);
const CHECKPOINT_INTERVAL_MS: i64 = 5_000;
const FREEZE_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
static SESSION_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub(crate) struct MusicSessionState {
    sender: mpsc::Sender<Message>,
    lifecycle: LifecycleControl,
}

enum Request {
    Start(SessionStart),
    Command(SessionCommand),
    Snapshot,
    Observe {
        observation: SessionObservation,
        browser: Option<(String, String)>,
    },
    Subscribe {
        window: String,
        id: String,
        channel: Channel<SessionNotice>,
    },
    Acknowledge {
        window: String,
        id: String,
        sequence: u64,
    },
    ReadFrame {
        window: String,
        id: String,
        sequence: u64,
        reply: oneshot::Sender<MusicLibraryResult<SessionFrame>>,
    },
    Unsubscribe {
        window: String,
        id: String,
    },
    Host {
        window: String,
        id: String,
        available: bool,
    },
    Control(SessionIntent),
    Focus(ganbaru_pomodoro::CommittedFocusEffect),
    #[cfg(target_os = "android")]
    BackendUnavailable {
        session_id: String,
        generation: u64,
        reason: AndroidInterruption,
    },
    #[cfg(desktop)]
    Completion {
        scope: (u64, i64),
        sound: crate::sound_effects::AppSound,
    },
    ResumeVault {
        revision: u64,
    },
    Freeze {
        revision: u64,
    },
}

struct Message {
    request: Request,
    expected_vault: Option<String>,
    reply: Option<oneshot::Sender<MusicLibraryResult<SessionProjection>>>,
}

struct Owner {
    app: tauri::AppHandle,
    state: SessionPolicy,
    pool: Option<SqlitePool>,
    vault_id: Option<String>,
    vault_generation: u64,
    device_id: String,
    roots: BTreeMap<String, String>,
    subscriptions: Subscriptions,
    checkpoint_at: i64,
    frozen: bool,
    backend_error_pending: Option<SessionObservation>,
    #[cfg(not(target_os = "ios"))]
    native_drain_generation: Option<u64>,
    focus: Option<focus::FocusLease>,
    #[cfg(desktop)]
    calendar: context::CalendarActivationState,
    lifecycle: LifecycleControl,
    #[cfg(desktop)]
    completion_duck: Option<completion::CompletionDuck>,
    /// Music volume setting last pushed to the soundscape engine, which plays relative to it.
    #[cfg(desktop)]
    soundscape_master: Option<f64>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

/// Fence manual background starts and recovery against the current native vault lifecycle.
#[cfg(desktop)]
pub(crate) fn background_output_guard(
    app: &tauri::AppHandle,
) -> Result<Box<dyn Fn() -> bool + Send>, String> {
    let runtime = app
        .try_state::<MusicSessionState>()
        .ok_or_else(|| "The native Music owner is unavailable".to_string())?;
    let lifecycle = runtime.lifecycle.clone();
    let current = lifecycle.current();
    if current.intent != LifecycleIntent::Active {
        return Err("Background playback is quiesced while the vault is changing".into());
    }
    Ok(Box::new(move || {
        lifecycle.matches(current.revision, LifecycleIntent::Active)
    }))
}

fn new_policy() -> SessionPolicy {
    let nonce = SESSION_NONCE.fetch_add(1, Ordering::Relaxed);
    let now = now_ms() as u64;
    SessionPolicy::new(
        format!("music-{now:x}-{nonce:x}"),
        now ^ nonce.wrapping_add(1),
    )
}

/// Resolves only native device bindings. These values never enter a checkpoint.
fn root_bindings(app: &tauri::AppHandle) -> MusicLibraryResult<BTreeMap<String, String>> {
    let vault_id = crate::vault::active_vault_id(app)
        .map_err(|error| MusicLibraryError::runtime("resolve music session vault", error))?;
    let state = crate::vault::read_app_state(app)
        .map_err(|error| MusicLibraryError::runtime("read music session bindings", error))?;
    Ok(state
        .music_root_bindings
        .get(&vault_id)
        .cloned()
        .unwrap_or_default())
}

/// Uses the same configured host for browser delivery and initial platform bridge capture.
pub(super) fn primary_window_label<R: Runtime>(app: &tauri::AppHandle<R>) -> String {
    app.config()
        .app
        .windows
        .first()
        .map(|window| window.label.clone())
        .unwrap_or_else(|| tauri::utils::config::WindowConfig::default().label)
}

/// Installs the single native owner. Recovery waits until an active vault is available.
pub(crate) fn setup(app: &tauri::AppHandle) {
    if app.try_state::<MusicSessionState>().is_some() {
        return;
    }
    let (sender, mut receiver) = mpsc::channel::<Message>(MESSAGE_CAPACITY);
    let lifecycle = LifecycleControl::new();
    let mut lifecycle_receiver = lifecycle.subscribe();
    app.manage(MusicSessionState {
        sender,
        lifecycle: lifecycle.clone(),
    });
    let primary_window = primary_window_label(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut owner = Owner {
            app,
            state: new_policy(),
            pool: None,
            vault_id: None,
            vault_generation: 0,
            device_id: String::new(),
            roots: BTreeMap::new(),
            subscriptions: Subscriptions::new(primary_window),
            checkpoint_at: 0,
            frozen: false,
            backend_error_pending: None,
            #[cfg(not(target_os = "ios"))]
            native_drain_generation: None,
            focus: None,
            #[cfg(desktop)]
            calendar: context::CalendarActivationState::default(),
            lifecycle,
            #[cfg(desktop)]
            completion_duck: None,
            #[cfg(desktop)]
            soundscape_master: None,
        };
        let mut interval = tokio::time::interval(OBSERVATION_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                changed = lifecycle_receiver.changed() => {
                    if changed.is_err() { break; }
                    let pending = *lifecycle_receiver.borrow_and_update();
                    if pending.intent == LifecycleIntent::Resume
                        && let Err(error) = owner.handle(Request::ResumeVault { revision: pending.revision }, None).await
                    {
                        owner.failed(error.to_string()).await;
                    }
                }
                Some(message) = receiver.recv() => {
                    let result = owner.handle(message.request, message.expected_vault).await;
                    if let Some(reply) = message.reply { let _ = reply.send(result); }
                    else if let Err(error) = result { eprintln!("music session: {error}"); }
                }
                _ = interval.tick() => {
                    if let Err(error) = owner.tick().await { owner.failed(error.to_string()).await; }
                }
                else => break,
            }
        }
    });
}

async fn request<R: Runtime>(
    app: &tauri::AppHandle<R>,
    request: Request,
) -> MusicLibraryResult<SessionProjection> {
    let expected_vault = if matches!(
        request,
        Request::Freeze { .. }
            | Request::ResumeVault { .. }
            | Request::Acknowledge { .. }
            | Request::Unsubscribe { .. }
    ) {
        None
    } else {
        let app = app.clone();
        Some(
            tauri::async_runtime::spawn_blocking(move || crate::vault::active_vault_id(&app))
                .await
                .map_err(|error| MusicLibraryError::runtime("capture music request vault", error))?
                .map_err(|error| {
                    MusicLibraryError::runtime("capture music request vault", error)
                })?,
        )
    };
    let runtime = app.try_state::<MusicSessionState>().ok_or_else(|| {
        MusicLibraryError::runtime("music session", "native owner is not initialized")
    })?;
    let (reply, response) = oneshot::channel();
    runtime
        .sender
        .try_send(Message {
            request,
            expected_vault,
            reply: Some(reply),
        })
        .map_err(|error| MusicLibraryError::runtime("enqueue music intent", error))?;
    response
        .await
        .map_err(|error| MusicLibraryError::runtime("receive music result", error))?
}

/// Hardware and tray controls enter the same serialized owner without frontend listeners.
pub(crate) fn dispatch_control(
    app: &tauri::AppHandle,
    intent: SessionIntent,
) -> Result<(), String> {
    let runtime = app
        .try_state::<MusicSessionState>()
        .ok_or_else(|| "Music owner is not initialized".to_string())?;
    runtime
        .sender
        .try_send(Message {
            request: Request::Control(intent),
            expected_vault: None,
            reply: None,
        })
        .map_err(|error| format!("enqueue music control: {error}"))
}

/// Stops the owner before a vault pool can close or another device can become writer.
pub(crate) async fn stop_for_vault_handoff<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let Some(runtime) = app.try_state::<MusicSessionState>() else {
        return Ok(());
    };
    freeze_owner(&runtime, FREEZE_REQUEST_TIMEOUT).await
}

async fn freeze_owner(runtime: &MusicSessionState, timeout: Duration) -> Result<(), String> {
    let revision = runtime.lifecycle.request(LifecycleIntent::Freeze)?;
    let (reply, response) = oneshot::channel();
    runtime
        .sender
        .try_send(Message {
            request: Request::Freeze { revision },
            expected_vault: None,
            reply: Some(reply),
        })
        .map_err(|error| format!("enqueue Music quiescence: {error}"))?;
    tokio::time::timeout(timeout, response)
        .await
        .map_err(|_| "Native Music owner did not acknowledge vault quiescence in time".to_string())?
        .map_err(|error| format!("receive Music quiescence: {error}"))?
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn resume_owner(runtime: &MusicSessionState) -> Result<(), String> {
    if runtime.sender.is_closed() {
        return Err("Resume native Music owner: command owner stopped".into());
    }
    runtime
        .lifecycle
        .request(LifecycleIntent::Resume)
        .map(|_| ())
}

/// Re-enables playback only after the vault coordinator finishes activation or recovery.
pub(crate) fn resume_after_vault_handoff<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let Some(runtime) = app.try_state::<MusicSessionState>() else {
        return Ok(());
    };
    resume_owner(&runtime)
}

/// Enqueues accepted Focus state; duplicate revisions only renew its bounded lease.
pub(crate) fn reconcile_committed_focus(
    app: &tauri::AppHandle,
    effect: ganbaru_pomodoro::CommittedFocusEffect,
) -> Result<(), String> {
    let runtime = app
        .try_state::<MusicSessionState>()
        .ok_or_else(|| "Music owner is not initialized".to_string())?;
    runtime
        .sender
        .try_send(Message {
            request: Request::Focus(effect),
            expected_vault: None,
            reply: None,
        })
        .map_err(|error| format!("enqueue committed Focus music: {error}"))
}

/// Service loss is an interruption, never a synthetic manual command or failed-track advance.
#[cfg(target_os = "android")]
pub(crate) fn native_backend_unavailable(
    app: &tauri::AppHandle,
    session_id: String,
    generation: u64,
    reason: AndroidInterruption,
) -> Result<(), String> {
    let runtime = app
        .try_state::<MusicSessionState>()
        .ok_or_else(|| "Music owner is not initialized".to_string())?;
    runtime
        .sender
        .try_send(Message {
            request: Request::BackendUnavailable {
                session_id,
                generation,
                reason,
            },
            expected_vault: None,
            reply: None,
        })
        .map_err(|error| format!("enqueue Android Music interruption: {error}"))
}

/// Attenuates the current Music generation and queues a scoped native completion sound.
#[cfg(desktop)]
pub(crate) async fn play_focus_completion(
    app: &tauri::AppHandle,
    scope: (u64, i64),
    sound: crate::sound_effects::AppSound,
) -> Result<(), String> {
    tokio::time::timeout(
        Duration::from_secs(5),
        request(app, Request::Completion { scope, sound }),
    )
    .await
    .map_err(|_| "Native Music did not acknowledge Focus completion audio in time".to_string())?
    .map(|_| ())
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn music_session_host(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    subscription_id: String,
    available: bool,
) -> MusicLibraryResult<()> {
    queue::validate_action_id(&subscription_id)?;
    request(
        &app,
        Request::Host {
            window: window.label().to_string(),
            id: subscription_id,
            available,
        },
    )
    .await
    .map(|_| ())
}

/// Registers one bounded projection stream for the calling WebView.
#[tauri::command]
pub(crate) async fn music_session_subscribe(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    subscription_id: String,
    channel: Channel<SessionNotice>,
) -> MusicLibraryResult<()> {
    queue::validate_action_id(&subscription_id)?;
    request(
        &app,
        Request::Subscribe {
            window: window.label().to_string(),
            id: subscription_id,
            channel,
        },
    )
    .await
    .map(|_| ())
}

/// Reads only a retained frame belonging to the calling WebView.
#[tauri::command]
pub(crate) async fn music_session_read_frame(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    subscription_id: String,
    sequence: u64,
) -> MusicLibraryResult<SessionFrame> {
    queue::validate_action_id(&subscription_id)?;
    let runtime = app.try_state::<MusicSessionState>().ok_or_else(|| {
        MusicLibraryError::runtime("music session", "native owner is not initialized")
    })?;
    let (reply, response) = oneshot::channel();
    runtime
        .sender
        .try_send(Message {
            request: Request::ReadFrame {
                window: window.label().to_string(),
                id: subscription_id,
                sequence,
                reply,
            },
            expected_vault: None,
            reply: None,
        })
        .map_err(|error| MusicLibraryError::runtime("enqueue music frame read", error))?;
    response
        .await
        .map_err(|error| MusicLibraryError::runtime("receive music frame", error))?
}

/// Repeated acknowledgements cannot release a later in-flight frame.
#[tauri::command]
pub(crate) async fn music_session_acknowledge(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    subscription_id: String,
    sequence: u64,
) -> MusicLibraryResult<()> {
    queue::validate_action_id(&subscription_id)?;
    request(
        &app,
        Request::Acknowledge {
            window: window.label().to_string(),
            id: subscription_id,
            sequence,
        },
    )
    .await
    .map(|_| ())
}

/// Drops only the calling WebView's matching subscription, even during vault quiescence.
#[tauri::command]
pub(crate) async fn music_session_unsubscribe(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    subscription_id: String,
) -> MusicLibraryResult<()> {
    queue::validate_action_id(&subscription_id)?;
    request(
        &app,
        Request::Unsubscribe {
            window: window.label().to_string(),
            id: subscription_id,
        },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub(crate) async fn music_session_start(
    app: tauri::AppHandle,
    request: SessionStart,
) -> MusicLibraryResult<SessionProjection> {
    queue::validate_action_id(&request.action_id)?;
    self::request(&app, Request::Start(request)).await
}

#[tauri::command]
pub(crate) async fn music_session_command(
    app: tauri::AppHandle,
    request: SessionCommand,
) -> MusicLibraryResult<SessionProjection> {
    queue::validate_action_id(&request.action_id)?;
    if matches!(
        request.intent,
        SessionIntent::Observe { .. } | SessionIntent::BrowserHost { .. }
    ) {
        return Err(MusicLibraryError::validation(
            "intent",
            "use the bounded observation adapter",
        ));
    }
    self::request(&app, Request::Command(request)).await
}

#[tauri::command]
pub(crate) async fn music_session_snapshot(
    app: tauri::AppHandle,
) -> MusicLibraryResult<SessionProjection> {
    request(&app, Request::Snapshot).await
}

#[tauri::command]
pub(crate) async fn music_session_observe(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    subscription_id: String,
    observation: SessionObservation,
) -> MusicLibraryResult<()> {
    queue::validate_action_id(&subscription_id)?;
    if observation.source_identity.len() > 16_384
        || observation
            .error
            .as_ref()
            .is_some_and(|error| error.len() > 4_096)
    {
        return Err(MusicLibraryError::validation(
            "observation",
            "media observation exceeds its size limit",
        ));
    }
    request(
        &app,
        Request::Observe {
            observation,
            browser: Some((window.label().to_string(), subscription_id)),
        },
    )
    .await
    .map(|_| ())
}

#[cfg(target_os = "android")]
pub(super) fn native_observation(
    app: &tauri::AppHandle,
    observation: SessionObservation,
) -> Result<(), String> {
    let runtime = app
        .try_state::<MusicSessionState>()
        .ok_or_else(|| "Music owner is not initialized".to_string())?;
    runtime
        .sender
        .try_send(Message {
            request: Request::Observe {
                observation,
                browser: None,
            },
            expected_vault: None,
            reply: None,
        })
        .map_err(|error| format!("enqueue Android music observation: {error}"))
}

impl Owner {
    async fn ensure_context(&mut self) -> MusicLibraryResult<()> {
        let app = self.app.clone();
        let (vault_id, vault_generation, device_id, roots) =
            tauri::async_runtime::spawn_blocking(move || {
                let vault_id = crate::vault::active_vault_id(&app)
                    .map_err(|error| MusicLibraryError::runtime("resolve music vault", error))?;
                let device_id = crate::vault::ensure_device_id(&app)
                    .map_err(|error| MusicLibraryError::runtime("resolve music device", error))?;
                let ownership = app.state::<crate::vault::ownership::VaultOwnershipManager>();
                let status = ownership.status(&vault_id).map_err(|error| {
                    MusicLibraryError::runtime("read music vault ownership", error)
                })?;
                if !status.can_write {
                    return Err(MusicLibraryError::conflict(
                        "Music playback requires the active writable vault",
                    ));
                }
                Ok::<_, MusicLibraryError>((
                    vault_id,
                    status.generation,
                    device_id,
                    root_bindings(&app)?,
                ))
            })
            .await
            .map_err(|error| MusicLibraryError::runtime("read music device state", error))??;
        self.roots = roots;
        if self.frozen {
            return Err(MusicLibraryError::conflict(
                "Music is quiesced while this vault is handed off",
            ));
        }
        if self.vault_id.as_deref() == Some(&vault_id)
            && self.vault_generation == vault_generation
            && self.pool.is_some()
        {
            return Ok(());
        }
        if self.state.current.is_some() {
            self.apply_effect(SessionEffect::Stop {
                generation: self.state.generation + 1,
            })
            .await
            .map_err(|error| MusicLibraryError::runtime("stop prior music vault", error))?;
        }
        self.subscriptions.invalidate_context();
        // Only the first context of this process is a new launch; later contexts reopen
        // a vault this process already played, such as after a handoff or a vault switch.
        let selection = if self.vault_id.is_none() {
            persistence::RestoreSelection::Next
        } else {
            persistence::RestoreSelection::Saved
        };
        self.pool = Some(
            crate::db::connect_vault(&self.app)
                .await
                .map_err(|error| MusicLibraryError::runtime("connect music session", error))?,
        );
        self.vault_id = Some(vault_id);
        self.vault_generation = vault_generation;
        self.device_id = device_id;
        self.frozen = false;
        let generation = self.state.generation.saturating_add(1);
        self.state = new_policy();
        self.state.generation = generation;
        self.focus = None;
        #[cfg(desktop)]
        {
            self.calendar = context::CalendarActivationState::default();
        }
        #[cfg(desktop)]
        {
            self.completion_duck = None;
        }
        #[cfg(target_os = "android")]
        backend::attach(&self.app)
            .await
            .map_err(|error| MusicLibraryError::runtime("attach Android music service", error))?;
        let pool = self.pool.as_ref().expect("session pool initialized");
        if let Some(saved) = persistence::load_checkpoint(pool, &self.device_id).await?
            && let Some(definition) = &saved.queue
        {
            let mut restored = self.state.clone();
            let mut transaction = pool.begin().await.map_err(|error| {
                MusicLibraryError::database("read recovered music queue", error)
            })?;
            queue::load_queue(
                &mut transaction,
                &self.roots,
                definition,
                &mut restored,
                now_ms(),
            )
            .await?;
            transaction.commit().await.map_err(|error| {
                MusicLibraryError::database("finish recovered music queue", error)
            })?;
            let transition = persistence::restore(&mut restored, &saved, selection, now_ms());
            self.commit(restored, transition, None, true).await?;
        }
        self.publish(true)?;
        Ok(())
    }

    async fn handle(
        &mut self,
        request: Request,
        expected_vault: Option<String>,
    ) -> MusicLibraryResult<SessionProjection> {
        let request = match request {
            Request::ReadFrame {
                window,
                id,
                sequence,
                reply,
            } => {
                let result = self.subscriptions.read(&window, &id, sequence, now_ms());
                let _ = reply.send(result);
                return Ok(self.state.projection(false, now_ms()));
            }
            Request::Acknowledge {
                window,
                id,
                sequence,
            } => {
                self.subscriptions
                    .acknowledge(&window, &id, sequence, now_ms())?;
                self.subscriptions.flush(now_ms());
                return Ok(self.state.projection(false, now_ms()));
            }
            Request::Unsubscribe { window, id } => {
                self.subscriptions.unsubscribe(&window, &id);
                if self.state.browser_host
                    && !self.subscriptions.browser_available(now_ms())
                    && self.pool.is_some()
                    && !self.frozen
                {
                    self.intent(SessionIntent::BrowserHost { available: false }, None)
                        .await?;
                }
                return Ok(self.state.projection(false, now_ms()));
            }
            request => request,
        };
        if let Request::ResumeVault { revision } = request {
            if !self.lifecycle.complete_resume(revision) {
                return Ok(self.state.projection(false, now_ms()));
            }
            self.frozen = false;
            self.pool = None;
            self.focus = None;
            return Ok(self.state.projection(false, now_ms()));
        }
        let include_queue = matches!(
            &request,
            Request::Snapshot
                | Request::Start(_)
                | Request::Command(SessionCommand {
                    intent: SessionIntent::Refresh
                        | SessionIntent::RetryContext
                        | SessionIntent::RestoreReview { .. },
                    ..
                })
        );
        if let Request::Freeze { revision } = request {
            if !self.lifecycle.matches(revision, LifecycleIntent::Freeze) {
                return Err(MusicLibraryError::conflict(
                    "Music quiescence was superseded",
                ));
            }
            if self.pool.is_some() {
                let mut next = self.state.clone();
                let transition = next.apply(SessionIntent::Pause, now_ms());
                self.commit(next, transition, None, true).await?;
                self.apply_effect(SessionEffect::Stop {
                    generation: self.state.generation + 1,
                })
                .await
                .map_err(|error| MusicLibraryError::runtime("stop handoff music", error))?;
            }
            #[cfg(desktop)]
            self.stop_context_soundscape().await?;
            self.pool = None;
            self.frozen = true;
            #[cfg(desktop)]
            {
                self.completion_duck = None;
            }
            self.subscriptions.invalidate_context();
            return Ok(self.state.projection(true, now_ms()));
        }
        if self.frozen || self.lifecycle.is_revoked() {
            return Err(MusicLibraryError::conflict(
                "Music is suspended while this vault is changing",
            ));
        }
        // Browser host lease renewals arrive every second. When they already belong to the
        // established vault context, revalidating device state and bindings adds nothing.
        let renews_established_host = matches!(request, Request::Host { .. })
            && self.pool.is_some()
            && expected_vault.is_some()
            && expected_vault == self.vault_id;
        if !renews_established_host {
            self.ensure_context().await?;
        }
        if expected_vault.is_some() && expected_vault != self.vault_id {
            return Err(MusicLibraryError::conflict(
                "Music request belongs to the previous vault",
            ));
        }
        if self.state.browser_host && !self.subscriptions.browser_available(now_ms()) {
            self.intent(SessionIntent::BrowserHost { available: false }, None)
                .await?;
        }
        match request {
            Request::Subscribe {
                window,
                id,
                channel,
            } => {
                self.subscriptions
                    .subscribe(window, id, channel, now_ms())?;
                if self.state.browser_host && !self.subscriptions.browser_available(now_ms()) {
                    self.intent(SessionIntent::BrowserHost { available: false }, None)
                        .await?;
                }
                self.publish(true)?;
            }
            Request::Host {
                window,
                id,
                available,
            } => {
                let available = self
                    .subscriptions
                    .renew_host(&window, &id, available, now_ms())?;
                if available != self.state.browser_host {
                    self.intent(SessionIntent::BrowserHost { available }, None)
                        .await?;
                }
            }
            Request::Start(start) => self.start(start).await?,
            Request::Command(command) => {
                let hash = persistence::request_hash(&command)?;
                // Receipt lookup precedes generation validation so lost replies remain retryable.
                let mut connection = self
                    .pool
                    .as_ref()
                    .expect("initialized pool")
                    .acquire()
                    .await
                    .map_err(|error| {
                        MusicLibraryError::database("read music action receipt", error)
                    })?;
                if persistence::receipt(&mut connection, &self.device_id, &command.action_id, &hash)
                    .await?
                {
                    return Ok(self.state.projection(true, now_ms()));
                }
                drop(connection);
                if command
                    .session_id
                    .as_ref()
                    .is_some_and(|id| id != &self.state.session_id)
                {
                    return Err(MusicLibraryError::stale(
                        "music session",
                        command.session_id.as_deref().unwrap_or_default(),
                    ));
                }
                self.intent(command.intent, Some((&command.action_id, &hash)))
                    .await?;
            }
            Request::Control(intent) => self.intent(intent, None).await?,
            Request::Focus(effect) => self.reconcile_focus(effect).await?,
            #[cfg(target_os = "android")]
            Request::BackendUnavailable {
                session_id,
                generation,
                reason,
            } => {
                if self.state.session_id == session_id
                    && self.state.generation == generation
                    && self
                        .state
                        .current_entry()
                        .is_some_and(|entry| entry.backend == SessionBackend::NativeAudio)
                {
                    let mut next = self.state.clone();
                    let transition = next.interrupt_backend();
                    next.error = Some(reason.message().into());
                    self.commit(next, transition, None, true).await?;
                }
            }
            #[cfg(desktop)]
            Request::Completion { scope, sound } => self.begin_completion(scope, sound).await?,
            Request::Observe {
                observation,
                browser,
            } => {
                if browser.is_some()
                    && self
                        .state
                        .current_entry()
                        .is_none_or(|entry| entry.backend != SessionBackend::Browser)
                {
                    return Ok(self.state.projection(false, now_ms()));
                }
                if let Some((window, id)) = browser
                    && !self.subscriptions.is_host(&window, &id, now_ms())
                {
                    return Err(MusicLibraryError::conflict(
                        "Music observation is not from the active browser host",
                    ));
                }
                self.observation(observation).await?;
            }
            Request::Snapshot
            | Request::Freeze { .. }
            | Request::ResumeVault { .. }
            | Request::Acknowledge { .. }
            | Request::ReadFrame { .. }
            | Request::Unsubscribe { .. } => (),
        }
        Ok(self.state.projection(include_queue, now_ms()))
    }

    async fn start(&mut self, start: SessionStart) -> MusicLibraryResult<()> {
        #[cfg(target_os = "android")]
        backend::attach(&self.app).await.map_err(|error| {
            MusicLibraryError::runtime("reconnect Android Music service", error)
        })?;
        let _permit = self.write_permit().await?;
        if !start.volume.is_finite()
            || !(0.0..=1.0).contains(&start.volume)
            || !start.rate.is_finite()
            || !(0.25..=2.0).contains(&start.rate)
        {
            return Err(MusicLibraryError::validation(
                "settings",
                "volume or rate is outside its range",
            ));
        }
        let hash = persistence::request_hash(&start)?;
        let mut next = if matches!(start.queue, SessionQueueIntent::ReviewItem { .. }) {
            self.state.clone()
        } else {
            new_policy()
        };
        next.generation = self.state.generation + 1;
        next.revision = self.state.revision;
        next.queue_revision = self.state.queue_revision;
        next.browser_host = self.state.browser_host;
        next.online = self.state.online;
        next.order = start.order;
        next.volume = start.volume;
        next.muted = start.muted;
        next.rate = start.rate;
        next.manual_revision = self.state.manual_revision + 1;
        next.current = None;
        next.playlist_id = None;
        next.history.clear();
        next.shuffle.clear();
        next.failed.clear();
        let pool = self.pool.as_ref().expect("initialized pool").clone();
        let mut transaction = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|error| MusicLibraryError::database("read new music queue", error))?;
        if persistence::receipt(&mut transaction, &self.device_id, &start.action_id, &hash).await? {
            return Ok(());
        }
        let selected = queue::load_queue(
            &mut transaction,
            &self.roots,
            &start.queue,
            &mut next,
            now_ms(),
        )
        .await?;
        let avoid = if let SessionQueueIntent::SavedPlaylist { avoid_item_id, .. } = &start.queue {
            avoid_item_id.as_deref()
        } else {
            None
        };
        let mut transition = next.initial(selected.as_deref(), avoid, start.autoplay, now_ms());
        if start.resume
            && self.state.definition == next.definition
            && self.state.current == next.current
        {
            let seek = next.apply(
                SessionIntent::Seek {
                    position_ms: self.state.position_ms,
                },
                now_ms(),
            );
            for effect in &mut transition.effects {
                if let SessionEffect::Load { position_ms, .. } = effect {
                    *position_ms = next.position_ms;
                }
            }
            // The load already carries the clamped resume position.
            drop(seek);
        }
        if self.state.owner != SessionOwner::Review {
            let mut prior = self.state.clone();
            transition
                .listening
                .extend(prior.apply(SessionIntent::Stop, now_ms()).listening);
        }
        persistence::commit_in_transaction(
            &mut transaction,
            &self.device_id,
            &next,
            &transition,
            Some((&start.action_id, &hash)),
            now_ms(),
        )
        .await?;
        transaction
            .commit()
            .await
            .map_err(|error| MusicLibraryError::database("commit new music queue", error))?;
        self.install(next, transition).await
    }

    async fn intent(
        &mut self,
        intent: SessionIntent,
        action: Option<(&str, &str)>,
    ) -> MusicLibraryResult<()> {
        if matches!(intent, SessionIntent::RetryContext) {
            return self.retry_context(action).await;
        }
        if matches!(intent, SessionIntent::Refresh) {
            return self.refresh(action).await;
        }
        let mut next = self.state.clone();
        if matches!(intent, SessionIntent::BrowserHost { available: true }) {
            if self.state.browser_host {
                return Ok(());
            }
        }
        if !matches!(
            intent,
            SessionIntent::Online { .. }
                | SessionIntent::BrowserHost { .. }
                | SessionIntent::Refresh
                | SessionIntent::Observe { .. }
        ) {
            next.manual_revision += 1;
            if next.owner != SessionOwner::Review {
                next.owner = SessionOwner::Manual;
            }
            if let Some(context) = &mut next.context {
                context.state = "overridden".into();
            }
        }
        #[cfg(target_os = "android")]
        let reconnect = (matches!(intent, SessionIntent::Play)
            || (matches!(intent, SessionIntent::Toggle)
                && !matches!(
                    self.state.status,
                    SessionStatus::Playing | SessionStatus::Loading
                )))
            && self
                .state
                .current_entry()
                .is_some_and(|entry| entry.backend == SessionBackend::NativeAudio)
            && backend::restart_for_play(&self.app)
                .await
                .map_err(|error| {
                    MusicLibraryError::runtime("restart Android Music service", error)
                })?;
        #[cfg(desktop)]
        let reconnect = false;
        #[cfg(not(target_os = "ios"))]
        let mut prepared = if reconnect
            || (matches!(
                self.state.issue,
                Some(SessionIssue::Interrupted | SessionIssue::Restored)
            ) && matches!(intent, SessionIntent::Play | SessionIntent::Toggle)
                && self
                    .state
                    .current_entry()
                    .is_some_and(|entry| entry.backend == SessionBackend::NativeAudio))
        {
            next.reconnect_backend()
        } else {
            Transition::default()
        };
        let transition = next.apply(intent, now_ms());
        #[cfg(not(target_os = "ios"))]
        let transition = {
            prepared.changed |= transition.changed;
            prepared.effects.extend(transition.effects);
            prepared.listening.extend(transition.listening);
            prepared
        };
        self.commit(next, transition, action, true).await
    }

    async fn refresh(&mut self, action: Option<(&str, &str)>) -> MusicLibraryResult<()> {
        let Some(definition) = self.state.definition.clone() else {
            return Ok(());
        };
        if matches!(definition, SessionQueueIntent::Sources { .. }) {
            return Ok(());
        }
        let _permit = self.write_permit().await?;
        let mut next = self.state.clone();
        let pool = self.pool.as_ref().expect("initialized pool").clone();
        let mut transaction = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|error| MusicLibraryError::database("refresh music queue", error))?;
        if let Some((id, hash)) = action
            && persistence::receipt(&mut transaction, &self.device_id, id, hash).await?
        {
            return Ok(());
        }
        if let Err(error) = queue::load_queue(
            &mut transaction,
            &self.roots,
            &definition,
            &mut next,
            now_ms(),
        )
        .await
        {
            if matches!(definition, SessionQueueIntent::SavedPlaylist { .. })
                && error.code == ganbaru_music::error::MusicLibraryErrorCode::NotFound
            {
                let detached = SessionQueueIntent::LibraryItems {
                    item_ids: self
                        .state
                        .queue
                        .iter()
                        .filter_map(|entry| entry.item_id.clone())
                        .collect(),
                    selected_item_id: self
                        .state
                        .current_entry()
                        .and_then(|entry| entry.item_id.clone()),
                    name: self.state.queue_name.clone(),
                };
                queue::load_queue(
                    &mut transaction,
                    &self.roots,
                    &detached,
                    &mut next,
                    now_ms(),
                )
                .await?;
                next.context = None;
                next.owner = SessionOwner::Manual;
            } else {
                return Err(error);
            }
        }
        let transition = next.reconcile_queue(&self.state, now_ms());
        persistence::commit_in_transaction(
            &mut transaction,
            &self.device_id,
            &next,
            &transition,
            action,
            now_ms(),
        )
        .await?;
        transaction
            .commit()
            .await
            .map_err(|error| MusicLibraryError::database("finish music queue refresh", error))?;
        self.install(next, transition).await
    }

    async fn observation(&mut self, observation: SessionObservation) -> MusicLibraryResult<()> {
        let prior_status = self.state.status;
        let mut next = self.state.clone();
        let transition = next.apply(SessionIntent::Observe { observation }, now_ms());
        if !transition.changed {
            return Ok(());
        }
        let durable = !transition.listening.is_empty()
            || !transition.effects.is_empty()
            || next.status != prior_status
            || now_ms() - self.checkpoint_at >= CHECKPOINT_INTERVAL_MS;
        self.commit(next, transition, None, durable).await
    }

    async fn commit(
        &mut self,
        next: SessionPolicy,
        transition: Transition,
        action: Option<(&str, &str)>,
        durable: bool,
    ) -> MusicLibraryResult<()> {
        // Non-durable observations carry no effects and write nothing, so the
        // serialized owner does not need the vault write permit for them.
        let _permit = if durable {
            Some(self.write_permit().await?)
        } else {
            None
        };
        if durable {
            if !persistence::commit(
                self.pool.as_ref().expect("initialized pool"),
                &self.device_id,
                &next,
                &transition,
                action,
                now_ms(),
            )
            .await?
            {
                return Ok(());
            }
            self.checkpoint_at = now_ms();
        }
        self.install(next, transition).await
    }

    async fn install(
        &mut self,
        next: SessionPolicy,
        transition: Transition,
    ) -> MusicLibraryResult<()> {
        let queue_changed = next.queue_revision != self.state.queue_revision
            || next.session_id != self.state.session_id;
        #[cfg(desktop)]
        let prior_status = self.state.status;
        self.state = next;
        #[cfg(desktop)]
        if self
            .completion_duck
            .as_ref()
            .is_some_and(|duck| !duck.matches(&self.state))
        {
            self.release_completion().await?;
        }
        for effect in transition.effects {
            if let Err(error) = self.apply_effect(effect).await {
                self.state.status = SessionStatus::Error;
                self.state.issue = Some(SessionIssue::SourceFailure);
                let source_failure = error.is_source_failure();
                self.state.error = Some(error.to_string());
                if self
                    .state
                    .current_entry()
                    .is_some_and(|entry| entry.backend == SessionBackend::Browser)
                {
                    self.state.browser_host = false;
                    self.state.status = SessionStatus::Paused;
                    self.state.autoplay_requested = false;
                    self.state.issue = Some(SessionIssue::BrowserHostUnavailable);
                    self.subscriptions
                        .effect(
                            SessionEffect::Stop {
                                generation: self.state.generation,
                            },
                            now_ms(),
                        )
                        .map_err(|error| {
                            MusicLibraryError::runtime("stop unavailable browser Music", error)
                        })?;
                    #[cfg(not(target_os = "ios"))]
                    {
                        // Switching to a browser source can fail while stopping
                        // the prior native decoder. Retain that native drain too.
                        self.native_drain_generation = Some(self.state.generation);
                    }
                    break;
                }
                #[cfg(desktop)]
                {
                    if source_failure {
                        self.backend_error_pending =
                            self.state.current_entry().map(|entry| SessionObservation {
                                session_id: self.state.session_id.clone(),
                                generation: self.state.generation,
                                sequence: self.state.last_sequence.saturating_add(1),
                                source_identity: entry.source.identity.clone(),
                                status: SessionStatus::Error,
                                position_ms: self.state.position_ms,
                                duration_ms: self.state.duration_ms,
                                error: self.state.error.clone(),
                            });
                    }
                }
                #[cfg(not(target_os = "ios"))]
                if !source_failure {
                    let error = self.state.error.take();
                    // A lost execution result does not prove the decoder is
                    // stopped. Revoke old observations and retain one Stop to
                    // drain before any later playback effect can be delivered.
                    self.state.interrupt_backend();
                    self.state.error = error;
                    self.native_drain_generation = Some(self.state.generation);
                }
                #[cfg(target_os = "ios")]
                {
                    self.state.status = SessionStatus::Paused;
                    self.state.issue = Some(SessionIssue::Interrupted);
                }
                break;
            }
        }
        #[cfg(desktop)]
        self.follow_music_with_soundscape(prior_status).await;
        self.publish(queue_changed)?;
        Ok(())
    }

    async fn write_permit(
        &self,
    ) -> MusicLibraryResult<crate::vault::ownership::ManagedVaultWritePermit> {
        let app = self.app.clone();
        let vault_id = self
            .vault_id
            .clone()
            .ok_or_else(|| MusicLibraryError::conflict("Music vault is not active"))?;
        let generation = self.vault_generation;
        tauri::async_runtime::spawn_blocking(move || {
            let ownership = app.state::<crate::vault::ownership::VaultOwnershipManager>();
            let permit = ownership
                .acquire_managed_write(&vault_id)
                .map_err(|error| MusicLibraryError::runtime("authorize music transition", error))?;
            let active_id = crate::vault::active_vault_id(&app)
                .map_err(|error| MusicLibraryError::runtime("verify music vault", error))?;
            let status = ownership
                .status(&vault_id)
                .map_err(|error| MusicLibraryError::runtime("verify music generation", error))?;
            if active_id != vault_id || status.generation != generation {
                return Err(MusicLibraryError::conflict(
                    "Music vault ownership changed before the transition",
                ));
            }
            Ok(permit)
        })
        .await
        .map_err(|error| MusicLibraryError::runtime("authorize music transition", error))?
    }

    async fn apply_effect(&mut self, effect: SessionEffect) -> Result<(), backend::Failure> {
        #[cfg(desktop)]
        let effect = self.attenuate_completion(effect);
        if self.lifecycle.is_revoked()
            && !matches!(
                effect,
                SessionEffect::Pause { .. } | SessionEffect::Stop { .. }
            )
        {
            return Err("Music playback delivery is revoked while the vault is changing".into());
        }
        #[cfg(not(target_os = "ios"))]
        self.drain_native_delivery().await?;
        let browser = self
            .state
            .current_entry()
            .is_some_and(|entry| entry.backend == SessionBackend::Browser);
        #[cfg(not(target_os = "ios"))]
        let authority = if browser && matches!(effect, SessionEffect::Load { .. }) {
            self.native_delivery_authority(&SessionEffect::Stop {
                generation: backend::effect_generation(&effect),
            })?
        } else {
            self.native_delivery_authority(&effect)?
        };
        if browser {
            backend::apply(
                &self.app,
                Some(SessionBackend::Browser),
                effect.clone(),
                #[cfg(not(target_os = "ios"))]
                authority,
            )
            .await?;
            if !self.subscriptions.browser_available(now_ms())
                && !matches!(
                    effect,
                    SessionEffect::Pause { .. } | SessionEffect::Stop { .. }
                )
            {
                // Intent and settings remain canonical while the browser cannot consume them.
                return Ok(());
            }
            self.subscriptions
                .effect(effect, now_ms())
                .map_err(|error| backend::Failure::Interrupted(error.to_string()))
        } else {
            if matches!(
                effect,
                SessionEffect::Load { .. } | SessionEffect::Stop { .. }
            ) {
                self.subscriptions
                    .effect(
                        SessionEffect::Stop {
                            generation: backend::effect_generation(&effect),
                        },
                        now_ms(),
                    )
                    .map_err(|error| error.to_string())?;
            }
            backend::apply(
                &self.app,
                self.state.current_entry().map(|entry| entry.backend),
                effect,
                #[cfg(not(target_os = "ios"))]
                authority,
            )
            .await
        }
    }

    /// An uncertain SDK result must stop before a later effect can reopen playback.
    #[cfg(not(target_os = "ios"))]
    async fn drain_native_delivery(&mut self) -> Result<(), backend::Failure> {
        let Some(generation) = self.native_drain_generation else {
            return Ok(());
        };
        let effect = SessionEffect::Stop { generation };
        let authority = self.native_delivery_authority(&effect)?;
        backend::apply(
            &self.app,
            Some(SessionBackend::NativeAudio),
            effect,
            authority,
        )
        .await?;
        self.native_drain_generation = None;
        Ok(())
    }

    /// A queued SDK effect retains its admission identity through actual use.
    #[cfg(not(target_os = "ios"))]
    fn native_delivery_authority(
        &self,
        effect: &SessionEffect,
    ) -> Result<std::sync::Arc<dyn Fn() -> bool + Send + Sync>, String> {
        let draining = matches!(
            effect,
            SessionEffect::Pause { .. } | SessionEffect::Stop { .. }
        ) || matches!(effect, SessionEffect::Settings { volume, .. }
            if *volume == 0.0 || !self.state.autoplay_requested);
        let lifecycle = self.lifecycle.clone();
        let admitted = lifecycle.current();
        let app = self.app.clone();
        let vault_id = self.vault_id.clone();
        let generation = self.vault_generation;
        let focus = if !draining && self.state.owner == SessionOwner::Pomodoro {
            let (proof, deadline) = crate::pomodoro::native_runtime::capture_music_delivery(&app)
                .ok_or("Committed Focus music authority is unavailable")?;
            let key = format!(
                "{}:{}:{}",
                proof.run_id.as_deref().unwrap_or_default(),
                proof.segment_id.as_deref().unwrap_or_default(),
                proof.event_id.as_deref().unwrap_or_default()
            );
            if self
                .state
                .context
                .as_ref()
                .is_none_or(|context| context.activation_key != key)
            {
                return Err("Committed Focus music phase changed before delivery".into());
            }
            Some((proof, deadline))
        } else {
            None
        };
        Ok(std::sync::Arc::new(move || {
            if !lifecycle.matches(admitted.revision, admitted.intent)
                || (!draining && lifecycle.is_revoked())
            {
                return false;
            }
            if draining {
                return true;
            }
            let Some(vault_id) = &vault_id else {
                return false;
            };
            let Some(ownership) = app.try_state::<crate::vault::ownership::VaultOwnershipManager>()
            else {
                return false;
            };
            let owner_current = ownership
                .cached_status(vault_id)
                .ok()
                .flatten()
                .is_some_and(|status| status.can_write && status.generation == generation);
            owner_current
                && focus.as_ref().is_none_or(|(proof, deadline)| {
                    crate::pomodoro::native_runtime::music_delivery_is_current(
                        &app, proof, *deadline,
                    )
                })
                && lifecycle.matches(admitted.revision, admitted.intent)
        }))
    }

    fn publish(&mut self, include_queue: bool) -> MusicLibraryResult<()> {
        let snapshot = self.state.projection(include_queue, now_ms());
        #[cfg(desktop)]
        {
            crate::music::media_controls::publish_session(&snapshot).map_err(|error| {
                MusicLibraryError::runtime("publish native media controls", error)
            })?;
            crate::tray::publish_music_session(&self.app, &snapshot)
                .map_err(|error| MusicLibraryError::runtime("publish Music tray", error))?;
            self.push_soundscape_master();
        }
        self.subscriptions.publish(snapshot);
        self.subscriptions.flush(now_ms());
        Ok(())
    }

    /// Keeps the soundscape at its preset level relative to the Music volume setting, which applies
    /// even while Music is paused or empty; mute silences both.
    #[cfg(desktop)]
    fn push_soundscape_master(&mut self) {
        let master = if self.state.muted {
            0.0
        } else {
            self.state.volume
        };
        if self.soundscape_master != Some(master) {
            self.soundscape_master = Some(master);
            crate::music::soundscape::set_master(&self.app, master);
        }
    }

    /// Reads an on-by-default Music preference from the vault configuration.
    async fn music_preference(&self, key: &'static str) -> MusicLibraryResult<bool> {
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let raw = crate::vault::vault_read_config(app)
                .map_err(|error| MusicLibraryError::runtime("read music preference", error))?;
            let config: serde_json::Value = serde_json::from_str(&raw)
                .map_err(|error| MusicLibraryError::runtime("decode music preference", error))?;
            Ok(config
                .get("preferences")
                .and_then(|preferences| preferences.get(key))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true))
        })
        .await
        .map_err(|error| MusicLibraryError::runtime("read music preference", error))?
    }

    /// Pauses the soundscape with Music when the listener wants that, and resumes one that a
    /// Music pause paused. Music running out, failing, or stopping is not a pause.
    ///
    /// The Music state is already installed here, so an unreadable preference leaves the
    /// soundscape alone instead of failing the Music change.
    #[cfg(desktop)]
    async fn follow_music_with_soundscape(&self, prior: SessionStatus) {
        let playing = |status| matches!(status, SessionStatus::Playing | SessionStatus::Loading);
        if playing(prior) && self.state.status == SessionStatus::Paused {
            match self.music_preference("musicPauseSoundscapeWithMusic").await {
                Ok(true) => crate::music::soundscape::pause_with_music(&self.app),
                Ok(false) => {}
                Err(error) => eprintln!("soundscape follows Music: {error}"),
            }
        } else if prior == SessionStatus::Paused && playing(self.state.status) {
            // A vault change quiesces background output; nothing resumes until it is active again.
            if let Ok(guard) = background_output_guard(&self.app) {
                crate::music::soundscape::resume_with_music(&self.app, guard);
            }
        }
    }

    async fn tick(&mut self) -> MusicLibraryResult<()> {
        self.subscriptions.expire(now_ms());
        self.subscriptions.flush(now_ms());
        if self.frozen || self.lifecycle.is_revoked() {
            return Ok(());
        }
        #[cfg(not(target_os = "ios"))]
        if let Err(error) = self.drain_native_delivery().await {
            // Transport recovery is not a persistence failure or a failed track.
            self.state.error = Some(format!("Stop uncertain native Music delivery: {error}"));
            self.publish(false)?;
            return Ok(());
        }
        if self.pool.is_none() {
            #[cfg(mobile)]
            return Ok(());
            #[cfg(desktop)]
            {
                if !self.calendar.admit_context_load() {
                    return Ok(());
                }
                self.ensure_context().await?;
            }
        }
        #[cfg(desktop)]
        self.poll_completion().await?;
        self.expire_focus().await?;
        #[cfg(desktop)]
        {
            match self.reconcile_calendar_assignment(None).await {
                Ok(()) => {}
                Err(error) => {
                    self.state.context_error =
                        Some(super::policy::ContextFailure::Calendar(error.to_string()));
                    self.publish(false)?;
                }
            }
        }
        #[cfg(target_os = "android")]
        if self
            .state
            .current_entry()
            .is_some_and(|entry| entry.backend == SessionBackend::NativeAudio)
            && matches!(
                self.state.status,
                SessionStatus::Playing | SessionStatus::Loading | SessionStatus::Ready
            )
            && !backend::is_active().await.map_err(|error| {
                MusicLibraryError::runtime("observe Android Music service", error)
            })?
        {
            let mut next = self.state.clone();
            let transition = next.interrupt_backend();
            self.commit(next, transition, None, true).await?;
        }
        if let Some(observation) = self.backend_error_pending.take() {
            self.observation(observation).await?;
        }
        if self.state.issue == Some(SessionIssue::NoEligibleItems)
            && self.state.autoplay_requested
            && !self.state.eligible(now_ms()).is_empty()
        {
            let mut next = self.state.clone();
            let transition = next.apply(SessionIntent::Play, now_ms());
            self.commit(next, transition, None, true).await?;
        }
        if self.state.browser_host && !self.subscriptions.browser_available(now_ms()) {
            self.intent(SessionIntent::BrowserHost { available: false }, None)
                .await?;
        }
        if self
            .state
            .current_entry()
            .is_some_and(|entry| entry.backend == SessionBackend::NativeAudio)
            && matches!(
                self.state.status,
                SessionStatus::Playing | SessionStatus::Loading | SessionStatus::Ready
            )
        {
            match backend::observe(
                &self.app,
                &self.state.session_id,
                self.state.generation,
                self.state.last_sequence + 1,
            )
            .await
            {
                Ok(Some(observation)) => self.observation(observation).await?,
                Ok(None) => (),
                Err(error) => {
                    #[cfg(not(target_os = "ios"))]
                    {
                        self.state.interrupt_backend();
                        self.native_drain_generation = Some(self.state.generation);
                    }
                    self.state.error = Some(format!("Observe native playback: {error}"));
                    self.publish(false)?;
                }
            }
        }
        Ok(())
    }

    async fn failed(&mut self, error: String) {
        self.state.issue = Some(SessionIssue::PersistenceFailure);
        self.state.error = Some(error);
        self.state.status = SessionStatus::Paused;
        self.state.autoplay_requested = false;
        if let Err(error) = self
            .apply_effect(SessionEffect::Pause {
                generation: self.state.generation,
            })
            .await
        {
            eprintln!("pause failed music session: {error}");
        }
        if let Err(error) = self.publish(false) {
            eprintln!("publish failed music session: {error}");
        }
    }
}
