//! Serialized native Focus owner. Semantic state is published only after commit.

mod authority;
mod calendar;
pub(crate) mod calendar_edit;
pub(crate) use calendar_edit::{calendar_review_context, commit_calendar_edit};
mod clock;
mod delivery;
mod effects;
mod execution;
use crate::vault::runtime_lifecycle as lifecycle;
mod local_time;
#[cfg(any(mobile, test))]
mod mobile;
#[cfg(desktop)]
mod presentation;
mod subscriptions;
#[cfg(desktop)]
pub(crate) use authority::overlay_context_is_current;
pub(crate) use authority::{
    allows_existing_lease, current_effect, effect_is_current, presentation_is_current,
};
#[cfg(not(target_os = "ios"))]
pub(crate) use authority::{capture_music_delivery, music_delivery_is_current};
#[cfg(mobile)]
use mobile::FocusNotificationCopy as NotificationCopy;
#[cfg(desktop)]
use presentation::DesktopNotificationCopy as NotificationCopy;
pub(crate) use subscriptions::FocusNotice;

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ganbaru_pomodoro::*;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::{Manager, Runtime, ipc::Channel};
use tokio::sync::{Notify, mpsc, oneshot, watch};

const COMMAND_CAPACITY: usize = 32;
const HEARTBEAT_INTERVAL_MS: i64 = 15_000;
const ACTIVITY_RETRY_INTERVAL_MS: i64 = 15_000;
const ACTIVE_TICK_INTERVAL_MS: i64 = 1000;
const PAUSED_PRESENTATION_INTERVAL_MS: i64 = 180;
const ERROR_RETRY_INTERVAL_MS: i64 = 5000;
const FREEZE_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// The Android ongoing notification action that resumes a manual pause.
#[cfg(target_os = "android")]
const NOTIFICATION_RESUME_ACTION: &str = "resume";
pub(crate) const EFFECT_LEASE_MS: i64 = 15_000;
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FocusRequest {
    vault_id: String,
    vault_generation: u64,
    command: FocusCommand,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FocusProjection {
    vault_id: Option<String>,
    vault_generation: u64,
    snapshot: Option<FocusExecutionSnapshot>,
    error: Option<String>,
}

/// The immutable action result is distinct from the latest presentation state.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FocusCommandResult {
    projection: FocusProjection,
    receipt: FocusExecutionSnapshot,
}

struct Reply {
    projection: FocusProjection,
    receipt: Option<FocusExecutionSnapshot>,
}

impl From<FocusProjection> for Reply {
    fn from(projection: FocusProjection) -> Self {
        Self {
            projection,
            receipt: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ForegroundObservation {
    #[cfg(target_os = "android")]
    sequence: u64,
    foreground: bool,
    observed_at_ms: i64,
}

#[derive(Clone)]
struct FocusRuntimeState {
    sender: mpsc::Sender<Message>,
    invalidation: Arc<Notify>,
    calendar_dirty: Arc<AtomicBool>,
    lifecycle: lifecycle::LifecycleControl,
    foreground: watch::Sender<ForegroundObservation>,
    authority: watch::Sender<Option<authority::EffectAuthority>>,
    #[cfg(desktop)]
    controls: watch::Sender<Option<FocusNativeContext>>,
}

/// Native controls retain the accepted context that was displayed to the user.
#[derive(Clone, Debug)]
#[cfg(desktop)]
pub(crate) struct FocusNativeContext {
    vault_id: String,
    pub(crate) vault_generation: u64,
    pub(crate) revision: i64,
    pub(crate) mode: FocusMode,
    pub(crate) phase: Option<FocusPhase>,
    pub(crate) run_id: Option<String>,
    pub(crate) segment_id: Option<String>,
    pub(crate) idle_detected_at_ms: Option<i64>,
}

#[cfg(desktop)]
impl FocusNativeContext {
    /// Bind publication to the exact snapshot supplying the visible tray state.
    pub(crate) fn matches_snapshot(
        &self,
        generation: u64,
        snapshot: &FocusExecutionSnapshot,
    ) -> bool {
        self.revision == snapshot.revision && self.matches_phase(generation, snapshot)
    }

    /// Heartbeats can refresh revisions without changing the displayed action's target.
    fn matches_phase(&self, generation: u64, snapshot: &FocusExecutionSnapshot) -> bool {
        self.vault_generation == generation
            && self.mode == snapshot.mode.kind()
            && self.phase == snapshot.segment.as_ref().map(|segment| segment.phase)
            && self.run_id == snapshot.run.as_ref().map(|run| run.id.clone())
            && self.segment_id == snapshot.segment.as_ref().map(|segment| segment.id.clone())
            && self.idle_detected_at_ms == snapshot.mode.idle_detected_at_ms()
    }
}

#[cfg(all(test, desktop))]
mod tests;

/// Paint acknowledgement is scoped to one native idle episode, without a caller clock.
#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct IdleOverlayScope {
    vault_generation: u64,
    run_id: String,
    segment_id: String,
    idle_detected_at_ms: i64,
}

enum Request {
    CalendarReview,
    CalendarDismissUndo {
        vault_id: String,
        generation: u64,
        delete_command_id: String,
    },
    CalendarEdit {
        request: Box<ganbaru_calendar::events::commit::CommitRequest>,
        response: oneshot::Sender<
            Result<
                ganbaru_calendar::events::commit::CommitReply,
                ganbaru_calendar::events::commit::CommitFailure,
            >,
        >,
    },
    #[cfg(not(target_os = "ios"))]
    NotificationCopy(NotificationCopy),
    Command(FocusRequest),
    #[cfg(desktop)]
    IdleOverlayVisible {
        scope: IdleOverlayScope,
        window_label: String,
    },
    #[cfg(desktop)]
    Native {
        intent: FocusIntent,
        context: FocusNativeContext,
    },
    /// Resume pressed on the Android ongoing notification for the run and phase it displayed.
    #[cfg(target_os = "android")]
    NotificationResume {
        run_id: String,
        segment_id: String,
    },
    Snapshot,
    Freeze {
        revision: u64,
    },
    Resume {
        revision: u64,
    },
    Subscribe {
        id: String,
        window_label: String,
        channel: Channel<FocusNotice>,
    },
    Renew {
        id: String,
        window_label: String,
    },
    Unsubscribe {
        id: String,
        window_label: String,
    },
}
impl Request {
    fn is_presentation(&self) -> bool {
        match self {
            Self::CalendarReview
            | Self::CalendarDismissUndo { .. }
            | Self::Subscribe { .. }
            | Self::Renew { .. }
            | Self::Unsubscribe { .. } => true,
            #[cfg(not(target_os = "ios"))]
            Self::NotificationCopy(_) => true,
            _ => false,
        }
    }
}

struct Message {
    request: Request,
    response: Option<oneshot::Sender<Result<Reply, FocusExecutionError>>>,
}

struct Owner {
    app: tauri::AppHandle,
    pool: Option<SqlitePool>,
    projection: FocusProjection,
    /// The last logged reason a transition skipped adaptive evidence.
    adaptive_degraded: Option<String>,
    recovered_vault: Option<(String, u64)>,
    foreground: bool,
    /// Cached stable device id for the activity row.
    device_id: Option<String>,
    access: VaultAccess,
    next_heartbeat_ms: i64,
    next_activity_ms: i64,
    next_boundary_ms: Option<i64>,
    /// Whether the last canonical Calendar resolution held a current commitment.
    /// Unknown starts as true so the first tick still probes activity.
    calendar_commitment: bool,
    next_retry_ms: Option<i64>,
    last_clock: Option<clock::ClockObservation>,
    calendar_undo: Option<calendar_edit::UndoSlot>,
    idle_grace: clock::IdleGraceClock,
    #[cfg(any(desktop, target_os = "android"))]
    native_command_sequence: u64,
    effects: delivery::DeliveryHandle,
    subscriptions: subscriptions::Subscriptions<Channel<FocusNotice>>,
}

/// Whether the owner may touch the active vault. A freeze detaches it; a resume stays
/// `Resuming` until effects are quiescent and the lifecycle completes that revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VaultAccess {
    Attached,
    Frozen,
    Resuming { revision: u64 },
}

impl VaultAccess {
    fn is_frozen(self) -> bool {
        !matches!(self, Self::Attached)
    }

    fn resume_revision(self) -> Option<u64> {
        match self {
            Self::Resuming { revision } => Some(revision),
            Self::Attached | Self::Frozen => None,
        }
    }
}

fn error(code: FocusErrorCode, message: impl Into<String>) -> FocusExecutionError {
    FocusExecutionError {
        code,
        message: message.into(),
        current_revision: None,
    }
}

fn now_ms() -> Result<i64, FocusExecutionError> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    i64::try_from(value).map_err(|_| {
        error(
            FocusErrorCode::Unavailable,
            "Native clock is outside the supported Focus range",
        )
    })
}

fn platform() -> FocusPlatform {
    if cfg!(mobile) {
        FocusPlatform::Android
    } else {
        FocusPlatform::Desktop
    }
}

/// Install a bounded owner independently of the main WebView's lifetime.
pub(crate) fn setup(app: &tauri::AppHandle) {
    if app.try_state::<FocusRuntimeState>().is_some() {
        return;
    }
    let (sender, mut receiver) = mpsc::channel::<Message>(COMMAND_CAPACITY);
    let (foreground, mut foreground_receiver) = watch::channel(ForegroundObservation {
        #[cfg(target_os = "android")]
        sequence: 0,
        foreground: platform() == FocusPlatform::Desktop,
        observed_at_ms: 0,
    });
    let invalidation = Arc::new(Notify::new());
    let calendar_dirty = Arc::new(AtomicBool::new(true));
    let lifecycle = lifecycle::LifecycleControl::new();
    let mut lifecycle_receiver = lifecycle.subscribe();
    app.manage(FocusRuntimeState {
        sender,
        invalidation: Arc::clone(&invalidation),
        calendar_dirty: Arc::clone(&calendar_dirty),
        lifecycle,
        foreground,
        authority: watch::channel(None).0,
        #[cfg(desktop)]
        controls: watch::channel(None).0,
    });
    #[cfg(target_os = "android")]
    {
        let authority_app = app.clone();
        let mut nonce_bytes = [0_u8; size_of::<i64>()];
        if let Err(error) = rustls::crypto::ring::default_provider()
            .secure_random
            .fill(&mut nonce_bytes)
        {
            eprintln!("Generate native Android Focus process identity: {error:?}");
            return;
        }
        let process_nonce = (i64::from_le_bytes(nonce_bytes) & i64::MAX).max(1);
        if let Err(error) = ganbaru_mobile_notifications::set_focus_authority_checker(
            process_nonce,
            move |generation, revision| {
                presentation_is_current(&authority_app, generation, revision)
            },
        ) {
            eprintln!("Install native Android Focus callback authority: {error}");
            return;
        }
    }
    let delivery = delivery::DeliveryHandle::start(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut owner = Owner {
            app,
            pool: None,
            projection: FocusProjection {
                vault_id: None,
                vault_generation: 0,
                snapshot: None,
                error: None,
            },
            adaptive_degraded: None,
            recovered_vault: None,
            foreground: platform() == FocusPlatform::Desktop,
            device_id: None,
            access: VaultAccess::Attached,
            next_heartbeat_ms: 0,
            next_activity_ms: 0,
            next_boundary_ms: None,
            calendar_commitment: true,
            next_retry_ms: None,
            last_clock: None,
            calendar_undo: None,
            idle_grace: clock::IdleGraceClock::default(),
            #[cfg(any(desktop, target_os = "android"))]
            native_command_sequence: 0,
            effects: delivery,
            subscriptions: subscriptions::Subscriptions::default(),
        };
        if let Err(error) = owner.attach_platform_lifecycle().await {
            owner.record_failure(error);
        }
        let mut next_wake = tokio::time::Instant::now();
        let mut delivery_feedback_open = true;
        loop {
            owner.prune_calendar_undo();
            owner.subscriptions.prune(Instant::now());
            // Presentation traffic can introduce an earlier wake, but cannot
            // repeatedly reset a pending execution tick into the future.
            next_wake =
                clock::earlier_wake(next_wake.into_std(), Instant::now(), owner.next_delay())
                    .into();
            if let Some(undo) = &owner.calendar_undo {
                next_wake = next_wake.min(tokio::time::Instant::from_std(undo.deadline()));
            }
            tokio::select! {
                biased;
                _ = lifecycle_receiver.changed() => {
                    let lifecycle_request = *lifecycle_receiver.borrow_and_update();
                    if lifecycle_request.intent == lifecycle::LifecycleIntent::Resume
                        && let Err(error) = owner.handle(Request::Resume { revision: lifecycle_request.revision }).await
                    {
                        owner.record_failure(error);
                    }
                }
                _ = tokio::time::sleep_until(next_wake) => {
                    if let Err(error) = owner.tick(calendar_dirty.swap(false, Ordering::AcqRel)).await { owner.record_failure(error); }
                    next_wake = tokio::time::Instant::now() + owner.next_delay();
                }
                Some(message) = receiver.recv() => {
                    let presentation = message.request.is_presentation();
                    let result = owner.handle(message.request).await;
                    if !presentation && let Err(error) = &result
                        && matches!(error.code, FocusErrorCode::Persistence | FocusErrorCode::Unavailable | FocusErrorCode::ReadOnly) {
                        owner.record_failure(error.clone());
                    }
                    if let Some(response) = message.response { let _ = response.send(result); }
                }
                changed = owner.effects.feedback.changed(), if delivery_feedback_open => {
                    if changed.is_err() { delivery_feedback_open = false; }
                    owner.sync_delivery_feedback();
                    if owner.access.resume_revision().is_some() && owner.effects.is_quiescent() {
                        next_wake = tokio::time::Instant::now();
                    }
                }
                changed = foreground_receiver.changed() => {
                    if changed.is_err() { break; }
                    let observation = *foreground_receiver.borrow_and_update();
                    owner.foreground = observation.foreground;
                    if let Err(error) = owner.observe(FocusObservation::ForegroundChanged { foreground: observation.foreground }, observation.observed_at_ms).await { owner.record_failure(error); }
                }
                _ = invalidation.notified() => {
                    if !owner.frozen()
                        && !owner.freeze_requested()
                        && calendar_dirty.swap(false, Ordering::AcqRel)
                    {
                        owner.effects.invalidate_preferences();
                        let result = async { owner.ensure_context(true).await?; owner.observe(FocusObservation::CalendarChanged, now_ms()?).await }.await;
                        if let Err(error) = result { owner.record_failure(error); }
                    }
                }
                else => break,
            }
        }
    });
}

async fn request<R: Runtime>(
    app: &tauri::AppHandle<R>,
    request: Request,
) -> Result<Reply, FocusExecutionError> {
    let state = app.try_state::<FocusRuntimeState>().ok_or_else(|| {
        error(
            FocusErrorCode::Unavailable,
            "Native Focus owner is not initialized",
        )
    })?;
    let (response, receiver) = oneshot::channel();
    state
        .sender
        .try_send(Message {
            request,
            response: Some(response),
        })
        .map_err(|_| {
            error(
                FocusErrorCode::Busy,
                "Native Focus command queue is full or closed",
            )
        })?;
    receiver.await.map_err(|_| {
        error(
            FocusErrorCode::Unavailable,
            "Native Focus owner stopped before returning its result",
        )
    })?
}

#[tauri::command]
pub(crate) async fn focus_command(
    app: tauri::AppHandle,
    request: FocusRequest,
) -> Result<FocusCommandResult, FocusExecutionError> {
    if request.vault_id.is_empty()
        || request.vault_id.len() > 128
        || request.command.command_id.len() > 128
        || matches!(&request.command.intent, FocusIntent::StartScheduled { occurrence_id: Some(id) } if id.len() > 1024)
    {
        return Err(error(
            FocusErrorCode::InvalidIntent,
            "Focus command identity exceeds its limit",
        ));
    }
    let reply = self::request(&app, Request::Command(request)).await?;
    let receipt = reply.receipt.ok_or_else(|| {
        error(
            FocusErrorCode::Unavailable,
            "Native Focus command returned no receipt",
        )
    })?;
    Ok(FocusCommandResult {
        projection: reply.projection,
        receipt,
    })
}

#[tauri::command]
pub(crate) async fn focus_snapshot(
    app: tauri::AppHandle,
) -> Result<FocusProjection, FocusExecutionError> {
    request(&app, Request::Snapshot)
        .await
        .map(|reply| reply.projection)
}

/// A Focus phase in progress on this device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FocusPhaseActivity {
    /// Unix milliseconds the phase ends while its timer runs, or `None` while it is paused,
    /// suspended, or waiting for the person to return.
    pub(crate) deadline_ms: Option<i64>,
}

/// The Focus phase in progress in `snapshot`, if any. Breaks, a stopped or expired run, and an
/// idle failure are not a Focus phase in progress.
pub(crate) fn focus_phase_activity_of(
    snapshot: Option<&FocusExecutionSnapshot>,
) -> Option<FocusPhaseActivity> {
    let snapshot = snapshot?;
    if snapshot.segment.as_ref()?.phase != FocusPhase::Focus {
        return None;
    }
    match snapshot.mode.kind() {
        FocusMode::Running => Some(FocusPhaseActivity {
            deadline_ms: snapshot.phase_deadline_ms,
        }),
        FocusMode::ManualPause
        | FocusMode::IdlePause
        | FocusMode::Suspended
        | FocusMode::ReturnWait => Some(FocusPhaseActivity { deadline_ms: None }),
        FocusMode::Stopped | FocusMode::IdleFailed | FocusMode::Expired => None,
    }
}

/// The Focus phase in progress on this device, read from the native Focus owner. A device whose
/// Focus owner is not running, as before a vault opens, has none.
pub(crate) async fn focus_phase_activity<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Option<FocusPhaseActivity> {
    app.try_state::<FocusRuntimeState>()?;
    match request(app, Request::Snapshot).await {
        Ok(reply) => focus_phase_activity_of(reply.projection.snapshot.as_ref()),
        Err(error) => {
            eprintln!(
                "Focus state is unavailable for quiet periods: {}",
                error.message
            );
            None
        }
    }
}

/// The native overlay and its main-window fallback can report a painted warning.
#[cfg(desktop)]
#[tauri::command]
pub(crate) async fn focus_idle_overlay_visible(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    scope: IdleOverlayScope,
) -> Result<(), FocusExecutionError> {
    if !matches!(
        window.label(),
        crate::pomodoro::overlay::POMODORO_OVERLAY_MAIN_LABEL | "main"
    ) || scope.vault_generation == 0
        || scope.run_id.is_empty()
        || scope.run_id.len() > 128
        || scope.segment_id.is_empty()
        || scope.segment_id.len() > 128
        || scope.idle_detected_at_ms < 0
    {
        return Err(error(
            FocusErrorCode::InvalidIntent,
            "Idle visibility acknowledgement has no valid native overlay scope",
        ));
    }
    request(
        &app,
        Request::IdleOverlayVisible {
            scope,
            window_label: window.label().into(),
        },
    )
    .await
    .map(|_| ())
}

/// Frontend localization supplies copy only; the native owner supplies phase state.
#[tauri::command]
pub(crate) async fn focus_notification_copy(
    app: tauri::AppHandle,
    copy: NotificationCopy,
) -> Result<(), FocusExecutionError> {
    copy.validate()?;
    #[cfg(not(target_os = "ios"))]
    {
        request(&app, Request::NotificationCopy(copy))
            .await
            .map(|_| ())
    }
    #[cfg(target_os = "ios")]
    {
        let _ = app;
        Err(error(
            FocusErrorCode::Unavailable,
            "Native Focus notifications are not implemented on iOS",
        ))
    }
}

/// Bind presentation to the invoking native window rather than caller input.
#[tauri::command]
pub(crate) async fn focus_subscribe(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    subscription_id: String,
    channel: Channel<FocusNotice>,
) -> Result<(), FocusExecutionError> {
    request(
        &app,
        Request::Subscribe {
            id: subscription_id,
            window_label: window.label().into(),
            channel,
        },
    )
    .await
    .map(|_| ())
}

/// Renew one bounded stream and read the current native projection.
#[tauri::command]
pub(crate) async fn focus_renew_subscription(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    subscription_id: String,
) -> Result<FocusProjection, FocusExecutionError> {
    request(
        &app,
        Request::Renew {
            id: subscription_id,
            window_label: window.label().into(),
        },
    )
    .await
    .map(|reply| reply.projection)
}

/// A window can release only its own presentation stream.
#[tauri::command]
pub(crate) async fn focus_unsubscribe(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    subscription_id: String,
) -> Result<(), FocusExecutionError> {
    request(
        &app,
        Request::Unsubscribe {
            id: subscription_id,
            window_label: window.label().into(),
        },
    )
    .await
    .map(|_| ())
}

/// Capture the displayed native revision before creating a control or notification.
#[cfg(desktop)]
pub(crate) fn capture_native_context<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<FocusNativeContext, String> {
    let state = app
        .try_state::<FocusRuntimeState>()
        .ok_or("Native Focus owner is unavailable")?;
    if state.lifecycle.is_revoked() {
        return Err("Native Focus controls are revoked while the vault is changing".to_owned());
    }
    state
        .controls
        .borrow()
        .clone()
        .ok_or_else(|| "Native Focus has no accepted control context".into())
}

/// Stale native button actions use the same revision and generation gates as IPC.
#[cfg(desktop)]
pub(crate) fn native_control_in_context<R: Runtime>(
    app: &tauri::AppHandle<R>,
    intent: FocusIntent,
    context: FocusNativeContext,
) -> Result<(), String> {
    let state = app
        .try_state::<FocusRuntimeState>()
        .ok_or_else(|| "Native Focus owner is unavailable".to_owned())?;
    state
        .sender
        .try_send(Message {
            request: Request::Native { intent, context },
            response: None,
        })
        .map_err(|error| format!("Enqueue native Focus control: {error}"))
}

/// Calendar invalidations coalesce instead of accumulating work in the command queue.
pub(crate) fn invalidate_calendar<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(state) = app.try_state::<FocusRuntimeState>() {
        state.calendar_dirty.store(true, Ordering::Release);
        state.invalidation.notify_one();
    }
}

/// Drain accepted work and release the cached pool before vault quiescence.
pub(crate) async fn stop_for_vault_handoff<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let Some(state) = app.try_state::<FocusRuntimeState>() else {
        return Ok(());
    };
    let revision = state
        .lifecycle
        .request(lifecycle::LifecycleIntent::Freeze)
        .map_err(|error| error.to_string())?;
    revoke_requested_authority(&state);
    tokio::time::timeout(
        FREEZE_REQUEST_TIMEOUT,
        request(app, Request::Freeze { revision }),
    )
    .await
    .map_err(|_| "Native Focus owner did not acknowledge vault quiescence in time".to_owned())?
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn revoke_requested_authority(state: &FocusRuntimeState) {
    state.authority.send_replace(None);
    #[cfg(desktop)]
    state.controls.send_replace(None);
}

/// Reload only the current authorized vault after the connection/write gates open.
pub(crate) fn resume_after_vault_handoff<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let Some(state) = app.try_state::<FocusRuntimeState>() else {
        return Ok(());
    };
    if state.sender.is_closed() {
        return Err("Resume native Focus owner: command owner stopped".to_owned());
    }
    state
        .lifecycle
        .request(lifecycle::LifecycleIntent::Resume)
        .map_err(|error| error.to_string())?;
    Ok(())
}

impl Owner {
    fn frozen(&self) -> bool {
        self.access.is_frozen()
    }

    async fn handle(&mut self, request: Request) -> Result<Reply, FocusExecutionError> {
        let request = match request {
            Request::CalendarDismissUndo {
                vault_id,
                generation,
                delete_command_id,
            } => {
                self.dismiss_calendar_undo(&vault_id, generation, &delete_command_id);
                return Ok(self.projection.clone().into());
            }
            Request::CalendarReview => {
                if self.frozen() || self.freeze_requested() {
                    return Err(error(
                        FocusErrorCode::Unavailable,
                        "Calendar preview is suspended while the vault changes",
                    ));
                }
                // Preview authorizes its own read snapshot. This request only
                // captures cached state and must not run recovery or vault IO.
                return Ok(self.projection.clone().into());
            }
            Request::CalendarEdit { request, response } => {
                let result = self.calendar_edit(*request).await;
                let _ = response.send(result);
                return Ok(self.projection.clone().into());
            }
            request => request,
        };
        match request {
            #[cfg(not(target_os = "ios"))]
            Request::NotificationCopy(copy) => {
                self.effects.configure_copy(copy)?;
                // Reconcile canonical Calendar and execution before publishing
                // language changes, rather than renewing a cached phase here.
                invalidate_calendar(&self.app);
                return Ok(self.projection.clone().into());
            }
            Request::Subscribe {
                id,
                window_label,
                channel,
            } => {
                self.subscriptions
                    .subscribe(id, window_label, channel, Instant::now())?;
                self.subscriptions.publish(&self.projection, Instant::now());
                return Ok(self.projection.clone().into());
            }
            Request::Renew { id, window_label } => {
                self.subscriptions
                    .renew(&id, &window_label, Instant::now())?;
                return Ok(self.projection.clone().into());
            }
            Request::Unsubscribe { id, window_label } => {
                self.subscriptions.unsubscribe(&id, &window_label);
                return Ok(self.projection.clone().into());
            }
            Request::Freeze { revision } => {
                if !self
                    .app
                    .state::<FocusRuntimeState>()
                    .lifecycle
                    .matches(revision, lifecycle::LifecycleIntent::Freeze)
                {
                    return Err(error(
                        FocusErrorCode::Busy,
                        "Native Focus freeze request was superseded",
                    ));
                }
                self.access = VaultAccess::Frozen;
                self.recovered_vault = None;
                self.clear_effect_authority();
                self.idle_grace = clock::IdleGraceClock::default();
                self.pool = None;
                self.projection.vault_generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
                self.projection.snapshot = None;
                #[cfg(desktop)]
                self.app
                    .state::<FocusRuntimeState>()
                    .controls
                    .send_replace(None);
                self.subscriptions.publish(&self.projection, Instant::now());
                self.effects.freeze().await?;
                if !self
                    .app
                    .state::<FocusRuntimeState>()
                    .lifecycle
                    .matches(revision, lifecycle::LifecycleIntent::Freeze)
                {
                    return Err(error(
                        FocusErrorCode::Busy,
                        "Native Focus freeze acknowledgement was superseded",
                    ));
                }
                return Ok(self.projection.clone().into());
            }
            Request::Resume { revision } => {
                if !self
                    .app
                    .state::<FocusRuntimeState>()
                    .lifecycle
                    .matches(revision, lifecycle::LifecycleIntent::Resume)
                {
                    return Ok(self.projection.clone().into());
                }
                if !self.frozen() {
                    self.clear_effect_authority();
                    self.recovered_vault = None;
                    self.idle_grace = clock::IdleGraceClock::default();
                    self.projection.vault_generation =
                        NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
                    self.projection.snapshot = None;
                    #[cfg(desktop)]
                    self.app
                        .state::<FocusRuntimeState>()
                        .controls
                        .send_replace(None);
                    self.subscriptions.publish(&self.projection, Instant::now());
                }
                self.effects.begin_freeze()?;
                self.access = VaultAccess::Resuming { revision };
                self.pool = None;
                if !self.effects.is_quiescent()
                    || !self
                        .app
                        .state::<FocusRuntimeState>()
                        .lifecycle
                        .complete_resume(revision)
                {
                    return Ok(self.projection.clone().into());
                }
                self.access = VaultAccess::Attached;
            }
            _ if self.frozen() || self.freeze_requested() => {
                return Err(error(
                    FocusErrorCode::Unavailable,
                    "Focus is suspended while this vault is changing",
                ));
            }
            _ => {}
        }
        // Accepted retries must be checked before startup recovery resolves any
        // current Calendar or timezone input. New actions recover after preflight.
        self.ensure_context(!matches!(&request, Request::Command(_)))
            .await?;
        let observed_now = now_ms().map(|now| self.logical_now(now));
        self.refresh_foreground();
        match request {
            #[cfg(desktop)]
            Request::IdleOverlayVisible {
                scope,
                window_label,
            } => {
                if scope.vault_generation != self.projection.vault_generation {
                    return Err(error(
                        FocusErrorCode::StaleGeneration,
                        "Idle overlay belongs to a previous vault generation",
                    ));
                }
                if window_label == "main" && self.projection.error.is_none() {
                    return Err(error(
                        FocusErrorCode::InvalidIntent,
                        "The main window has no active idle-overlay fallback",
                    ));
                }
                self.observe(
                    FocusObservation::IdleOverlayVisible {
                        run_id: scope.run_id,
                        segment_id: scope.segment_id,
                        detected_at_ms: scope.idle_detected_at_ms,
                    },
                    observed_now?,
                )
                .await?;
            }
            Request::Command(request) => {
                if self.projection.vault_id.as_deref() != Some(&request.vault_id) {
                    return Err(error(
                        FocusErrorCode::StaleGeneration,
                        "Focus action belongs to a previous vault generation",
                    ));
                }
                let receipt = self
                    .execute(
                        &request.command,
                        observed_now,
                        Some(request.vault_generation),
                    )
                    .await?;
                return Ok(Reply {
                    projection: self.projection.clone(),
                    receipt: Some(receipt),
                });
            }
            #[cfg(desktop)]
            Request::Native { intent, context } => {
                if self.projection.vault_id.as_ref() != Some(&context.vault_id) {
                    return Err(error(
                        FocusErrorCode::StaleGeneration,
                        "Native Focus control belongs to a previous vault",
                    ));
                }
                let snapshot = self.projection.snapshot.as_ref().ok_or_else(|| {
                    error(
                        FocusErrorCode::Unavailable,
                        "Native Focus control has no current execution",
                    )
                })?;
                if !context.matches_phase(self.projection.vault_generation, snapshot) {
                    return Err(error(
                        FocusErrorCode::StaleRevision,
                        "Native Focus control belongs to a superseded run, phase, or mode",
                    ));
                }
                let revision = snapshot.revision;
                self.native_command_sequence =
                    self.native_command_sequence.checked_add(1).ok_or_else(|| {
                        error(
                            FocusErrorCode::Unavailable,
                            "Native Focus command sequence is exhausted",
                        )
                    })?;
                let command = FocusCommand {
                    command_id: format!(
                        "native-focus-{}-{}-{}",
                        context.vault_generation, revision, self.native_command_sequence
                    ),
                    expected_revision: revision,
                    intent,
                };
                self.execute(&command, observed_now, Some(context.vault_generation))
                    .await?;
            }
            #[cfg(target_os = "android")]
            Request::NotificationResume { run_id, segment_id } => {
                let snapshot = self.projection.snapshot.as_ref().ok_or_else(|| {
                    error(
                        FocusErrorCode::Unavailable,
                        "Notification Resume has no current execution",
                    )
                })?;
                let displayed_phase = snapshot.mode == FocusMode::ManualPause
                    && snapshot.run.as_ref().is_some_and(|run| run.id == run_id)
                    && snapshot
                        .segment
                        .as_ref()
                        .is_some_and(|segment| segment.id == segment_id);
                if !displayed_phase {
                    return Err(error(
                        FocusErrorCode::StaleRevision,
                        "Notification Resume belongs to a superseded run, phase, or mode",
                    ));
                }
                let revision = snapshot.revision;
                let generation = self.projection.vault_generation;
                self.native_command_sequence =
                    self.native_command_sequence.checked_add(1).ok_or_else(|| {
                        error(
                            FocusErrorCode::Unavailable,
                            "Native Focus command sequence is exhausted",
                        )
                    })?;
                let command = FocusCommand {
                    command_id: format!(
                        "android-notification-focus-{generation}-{revision}-{}",
                        self.native_command_sequence
                    ),
                    expected_revision: revision,
                    intent: FocusIntent::Resume,
                };
                self.execute(&command, observed_now, Some(generation))
                    .await?;
            }
            Request::Snapshot | Request::Resume { .. } => {
                let now = observed_now?;
                self.observe(FocusObservation::CalendarChanged, now).await?;
                let pool = self.pool.as_ref().ok_or_else(|| {
                    error(FocusErrorCode::Unavailable, "Focus vault is unavailable")
                })?;
                self.projection.snapshot = Some(focus_read_execution_snapshot(pool, now).await?);
                self.verify_authority()?;
            }
            Request::Freeze { .. } => {}
            Request::CalendarEdit { .. }
            | Request::CalendarReview
            | Request::CalendarDismissUndo { .. } => {
                unreachable!("Calendar requests are dispatched before Focus execution")
            }
            Request::Subscribe { .. } | Request::Renew { .. } | Request::Unsubscribe { .. } => {
                unreachable!("presentation request handled before execution")
            }
            #[cfg(not(target_os = "ios"))]
            Request::NotificationCopy(_) => {
                unreachable!("notification copy handled before execution")
            }
        }
        Ok(self.projection.clone().into())
    }

    async fn ensure_context(&mut self, recover: bool) -> Result<(), FocusExecutionError> {
        if self.frozen() {
            return Err(error(FocusErrorCode::Unavailable, "Focus vault is frozen"));
        }
        let app = self.app.clone();
        let vault_id =
            tauri::async_runtime::spawn_blocking(move || crate::vault::active_vault_id(&app))
                .await
                .map_err(|error| error.to_string())??;
        let attached =
            self.projection.vault_id.as_deref() == Some(&vault_id) && self.pool.is_some();
        if !attached {
            self.clear_effect_authority();
            self.recovered_vault = None;
            self.pool = None;
            self.idle_grace = clock::IdleGraceClock::default();
            self.effects.freeze().await?;
            self.projection.vault_generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
            self.projection.vault_id = Some(vault_id.clone());
            self.projection.snapshot = None;
            #[cfg(desktop)]
            self.app
                .state::<FocusRuntimeState>()
                .controls
                .send_replace(None);
            self.projection.error = None;
            self.subscriptions.publish(&self.projection, Instant::now());
            self.pool = Some(crate::db::connect_vault(&self.app).await?);
            self.verify_authority()?;
        }
        let generation = self.projection.vault_generation;
        if recover && self.recovered_vault.as_ref() != Some(&(vault_id.clone(), generation)) {
            let now = now_ms()?;
            let pool = self
                .pool
                .as_ref()
                .ok_or_else(|| error(FocusErrorCode::Unavailable, "Focus vault is unavailable"))?;
            self.projection.snapshot = Some(focus_read_execution_snapshot(pool, now).await?);
            self.observe(FocusObservation::Recover, now).await?;
            self.recovered_vault = Some((vault_id, generation));
        }
        if !attached {
            self.next_activity_ms = 0;
            self.next_heartbeat_ms = 0;
        }
        Ok(())
    }

    fn verify_authority(&self) -> Result<(), FocusExecutionError> {
        let vault_id = crate::vault::active_vault_id(&self.app)?;
        if self.projection.vault_id.as_deref() != Some(&vault_id) {
            return Err(error(
                FocusErrorCode::StaleGeneration,
                "Active vault changed during Focus execution",
            ));
        }
        Ok(())
    }

    /// The stable device id that keys this device's Focus activity row, read once per owner.
    fn device_id(&mut self) -> Result<String, String> {
        if let Some(device_id) = &self.device_id {
            return Ok(device_id.clone());
        }
        let device_id = crate::vault::ensure_device_id(&self.app)
            .map_err(|error| format!("Read the Focus device identity: {error}"))?;
        self.device_id = Some(device_id.clone());
        Ok(device_id)
    }

    fn logical_now(&self, observed_at_ms: i64) -> i64 {
        observed_at_ms.max(
            self.projection
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.observed_at_ms)
                .unwrap_or(0),
        )
    }

    fn refresh_foreground(&mut self) {
        self.foreground = self
            .app
            .state::<FocusRuntimeState>()
            .foreground
            .borrow()
            .foreground;
    }

    fn freeze_requested(&self) -> bool {
        self.app.state::<FocusRuntimeState>().lifecycle.is_revoked()
    }

    fn next_delay(&self) -> Duration {
        let expiry = self
            .subscriptions
            .next_expiry()
            .map(|expiry| expiry.saturating_duration_since(Instant::now()));
        let delay = self.execution_delay();
        expiry.map_or(delay, |expiry| delay.min(expiry))
    }

    fn execution_delay(&self) -> Duration {
        let Ok(now) = now_ms() else {
            return Duration::from_millis(ERROR_RETRY_INTERVAL_MS as u64);
        };
        if self.frozen() || self.freeze_requested() {
            return if self.access.resume_revision().is_some() {
                Duration::from_millis(ERROR_RETRY_INTERVAL_MS as u64)
            } else {
                Duration::from_secs(60 * 60)
            };
        }
        if let Some(retry) = self.next_retry_ms.filter(|retry| *retry > now) {
            return Duration::from_millis(retry.saturating_sub(now) as u64);
        }
        if self.pool.is_none() {
            return Duration::from_millis(ERROR_RETRY_INTERVAL_MS as u64);
        }
        let running = self
            .projection
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.mode == FocusMode::Running);
        let mut next = self.next_boundary_ms.unwrap_or(i64::MAX);
        next = next.min(self.next_heartbeat_ms).min(self.next_activity_ms);
        if let Some(snapshot) = &self.projection.snapshot {
            next = next.min(snapshot.phase_deadline_ms.unwrap_or(i64::MAX));
            if let Some(run) = snapshot
                .run
                .as_ref()
                .filter(|run| run.ended_at_ms.is_none())
            {
                next = next.min(run.planned_end_ms);
            }
        }
        if running {
            next = next.min(now + ACTIVE_TICK_INTERVAL_MS);
        } else if self
            .projection
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.mode == FocusMode::ManualPause)
        {
            next = next.min(now.saturating_add(PAUSED_PRESENTATION_INTERVAL_MS));
        }
        let delay = Duration::from_millis(next.saturating_sub(now).clamp(1, 60_000) as u64);
        self.idle_grace
            .remaining(Instant::now())
            .map_or(delay, |grace| delay.min(grace))
    }

    async fn tick(&mut self, calendar_dirty: bool) -> Result<(), FocusExecutionError> {
        if self.frozen() {
            if let Some(revision) = self.access.resume_revision()
                && self.effects.is_quiescent()
                && self
                    .app
                    .state::<FocusRuntimeState>()
                    .lifecycle
                    .complete_resume(revision)
            {
                self.access = VaultAccess::Attached;
            } else {
                return Ok(());
            }
        }
        if self.freeze_requested() {
            return Ok(());
        }
        if calendar_dirty {
            self.effects.invalidate_preferences();
        }
        self.ensure_context(true).await?;
        let wall_now = now_ms()?;
        let now = self.logical_now(wall_now);
        let current_clock = clock::ClockObservation {
            wall_ms: wall_now,
            monotonic: Instant::now(),
        };
        let discontinuity = self
            .last_clock
            .as_ref()
            .and_then(|previous| clock::discontinuity(previous, &current_clock));
        self.last_clock = Some(current_clock);
        if let Some((start, end)) = discontinuity.filter(|_| {
            platform() == FocusPlatform::Desktop
                && self
                    .projection
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.mode == FocusMode::Running)
        }) {
            self.observe(
                FocusObservation::Suspend {
                    started_at_ms: start,
                    returned_at_ms: end,
                },
                now.max(end),
            )
            .await?;
        }
        let mut heartbeat_due = now >= self.next_heartbeat_ms;
        if calendar_dirty
            || self
                .next_boundary_ms
                .is_some_and(|boundary| now >= boundary)
        {
            // A heartbeat reconciles Calendar as well, so a due heartbeat replaces the
            // separate Calendar observation instead of opening a second transaction.
            if heartbeat_due {
                self.observe(FocusObservation::Heartbeat, now).await?;
                self.next_heartbeat_ms = now + HEARTBEAT_INTERVAL_MS;
                heartbeat_due = false;
            } else {
                self.observe(FocusObservation::CalendarChanged, now).await?;
            }
        }
        let due = self.projection.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .run
                .as_ref()
                .is_some_and(|run| run.ended_at_ms.is_none() && now >= run.planned_end_ms)
                || snapshot
                    .phase_deadline_ms
                    .is_some_and(|deadline| now >= deadline)
        });
        if due {
            self.observe(FocusObservation::Deadline, now).await?;
        }
        if let Some(observation) = self.projection.snapshot.as_ref().and_then(|snapshot| {
            self.idle_grace.elapsed_observation(
                self.projection.vault_generation,
                snapshot,
                Instant::now(),
            )
        }) {
            self.observe(observation, now_ms()?).await?;
        }
        if heartbeat_due {
            self.observe(FocusObservation::Heartbeat, now).await?;
            self.next_heartbeat_ms = now + HEARTBEAT_INTERVAL_MS;
        }
        if now >= self.next_activity_ms {
            let active = self.projection.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .run
                    .as_ref()
                    .is_some_and(|run| run.ended_at_ms.is_none())
            });
            // Automatic admission ignores activity without a current commitment, and
            // the heartbeat already reconciled expiry and Calendar state this tick.
            if !active && !self.calendar_commitment {
                self.next_activity_ms = now + ACTIVITY_RETRY_INTERVAL_MS;
                self.deliver_effects(now);
                return Ok(());
            }
            let activity = self.read_activity().await?;
            self.observe(
                if active {
                    FocusObservation::Activity(activity)
                } else {
                    FocusObservation::AutomaticAdmission(activity)
                },
                now_ms()?,
            )
            .await?;
            self.next_activity_ms = now + ACTIVITY_RETRY_INTERVAL_MS;
        }
        self.deliver_effects(now);
        Ok(())
    }

    #[cfg(desktop)]
    async fn read_activity(&self) -> Result<FocusActivityObservation, FocusExecutionError> {
        tauri::async_runtime::spawn_blocking(|| {
            let observed_at_ms = now_ms()?;
            let status = crate::pomodoro::idle::focus_idle_status();
            Ok(FocusActivityObservation {
                observed_at_ms,
                idle_ms: status.idle_ms.and_then(|value| i64::try_from(value).ok()),
                webcam_in_use: status.webcam_in_use,
            })
        })
        .await
        .map_err(|error| error.to_string())?
    }

    #[cfg(mobile)]
    async fn read_activity(&self) -> Result<FocusActivityObservation, FocusExecutionError> {
        Ok(FocusActivityObservation {
            observed_at_ms: now_ms()?,
            idle_ms: None,
            webcam_in_use: false,
        })
    }

    #[cfg(target_os = "android")]
    async fn attach_platform_lifecycle(&self) -> Result<(), FocusExecutionError> {
        use ganbaru_mobile_notifications::MobileNotificationsExt;
        let app = self.app.clone();
        let state = self.app.state::<FocusRuntimeState>().inner().clone();
        let sender = state.sender.clone();
        let channel = tauri::ipc::Channel::new(move |body| {
            let observation: ganbaru_mobile_notifications::NativeFocusLifecycle =
                body.deserialize()?;
            let previous = *state.foreground.borrow();
            if observation.sequence >= previous.sequence {
                state.foreground.send_replace(ForegroundObservation {
                    sequence: observation.sequence,
                    foreground: observation.foreground,
                    observed_at_ms: observation.observed_at_ms,
                });
            }
            Ok(())
        });
        let actions = tauri::ipc::Channel::new(move |body| {
            let action: ganbaru_mobile_notifications::NativeFocusNotificationAction =
                body.deserialize()?;
            if action.action != NOTIFICATION_RESUME_ACTION {
                eprintln!("Ignored unknown Android Focus notification action");
                return Ok(());
            }
            if let Err(error) = sender.try_send(Message {
                request: Request::NotificationResume {
                    run_id: action.run_id,
                    segment_id: action.phase_id,
                },
                response: None,
            }) {
                eprintln!("Enqueue Android Focus notification Resume: {error}");
            }
            Ok(())
        });
        tauri::async_runtime::spawn_blocking(move || {
            let notifications = app.mobile_notifications();
            notifications.attach_focus_lifecycle(channel)?;
            notifications.attach_focus_actions(actions)
        })
        .await
        .map_err(|error| error.to_string())??;
        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    async fn attach_platform_lifecycle(&self) -> Result<(), FocusExecutionError> {
        Ok(())
    }
}
