use serde::{Deserialize, Serialize};

use super::super::PomodoroRunRhythm;

/// One user action, retried with the same identity after an uncertain response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FocusCommand {
    pub command_id: String,
    pub expected_revision: i64,
    pub intent: FocusIntent,
}

/// User meaning rather than writable run, segment, or adaptive policy payloads.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FocusIntent {
    StartScheduled { occurrence_id: Option<String> },
    Pause,
    Resume,
    Stop,
    Advance,
    SkipBreak,
    SetSkipNextBreak { enabled: bool },
    ExtendFocus { seconds: i64 },
    ExtendBreak { seconds: i64 },
    ResolveIdle { resume: bool },
    ResolveSuspend { resume: bool },
    SetIdleTimeout { minutes: Option<i64> },
    SetAutomaticAdmissionSuppressed { suppressed: bool },
    DismissPausedPrompts,
}

/// Current execution mode. Waiting for return has no active accepted phase.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusMode {
    #[default]
    Stopped,
    Running,
    ManualPause,
    IdlePause,
    IdleFailed,
    Suspended,
    ReturnWait,
    Expired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusPhase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl FocusPhase {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ShortBreak => "short_break",
            Self::LongBreak => "long_break",
        }
    }
}

/// A validated rhythm and idle setting, independent of mutable editor drafts.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FocusConfiguration {
    pub rhythm: PomodoroRunRhythm,
    pub rhythm_source: String,
    pub preset_key: Option<String>,
    pub idle_timeout_minutes: Option<i64>,
}

/// Trusted result of resolving an occurrence in the execution transaction.
/// This type is deliberately not deserializable as a public command argument.
#[derive(Clone, Debug)]
pub struct FocusCommitment {
    pub event_id: String,
    pub occurrence_id: String,
    pub event_date: String,
    pub title: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub configuration: FocusConfiguration,
    pub calendar_revision: String,
}

/// Platform behavior is explicit in recovery and native deadline decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusPlatform {
    Desktop,
    Android,
}

/// A fresh native observation. Missing idle time cannot authorize admission.
#[derive(Clone, Copy, Debug)]
pub struct FocusActivityObservation {
    pub observed_at_ms: i64,
    pub idle_ms: Option<i64>,
    pub webcam_in_use: bool,
}

/// Visible idle grace uses monotonic elapsed time supplied by the native owner.
pub const FOCUS_IDLE_FAILURE_GRACE_MS: u64 = 60_000;

/// Native-only observations are separate from deserializable user intents.
#[derive(Clone, Debug)]
pub enum FocusObservation {
    Recover,
    Deadline,
    Activity(FocusActivityObservation),
    /// Presentation can acknowledge one matching idle episode, never choose its clock.
    IdleOverlayVisible {
        run_id: String,
        segment_id: String,
        detected_at_ms: i64,
    },
    IdleGraceElapsed {
        run_id: String,
        segment_id: String,
        visible_at_ms: i64,
        elapsed_ms: u64,
    },
    Suspend {
        started_at_ms: i64,
        returned_at_ms: i64,
    },
    CalendarChanged,
    AutomaticAdmission(FocusActivityObservation),
    ForegroundChanged {
        foreground: bool,
    },
    Heartbeat,
}

/// Clock and platform facts supplied by the native application owner.
pub struct FocusExecutionContext {
    pub now_ms: i64,
    pub platform: FocusPlatform,
    pub foreground: bool,
    pub commitment: Option<FocusCommitment>,
    pub local_time: Option<std::sync::Arc<dyn FocusLocalTimeResolver>>,
    pub planned_blocks: Vec<super::super::PomodoroAdaptivePlannedBlockWrite>,
}

/// Native platform adapter for historical local dates. No WebView data is accepted.
/// Implementations must bound requests and return an error when the device zone
/// cannot be resolved instead of substituting UTC for local-day evidence.
pub trait FocusLocalTimeResolver: Send + Sync {
    fn resolve(
        &self,
        instants: std::collections::BTreeSet<i64>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        super::super::adaptive::models::LocalTimeFacts,
                        FocusExecutionError,
                    >,
                > + Send
                + '_,
        >,
    >;
}

/// Post-commit native automation input. A renewed lease can retain its revision;
/// consumers deduplicate transitions while updating the bounded validity window.
/// Vault identity and generation prevent effects from crossing vault handoffs.
#[derive(Clone, Debug)]
pub struct CommittedFocusEffect {
    pub vault_id: String,
    pub vault_generation: u64,
    /// Shared vault ownership fence, distinct from this Focus owner's generation.
    pub ownership_generation: u64,
    pub execution_revision: i64,
    pub run_id: Option<String>,
    pub segment_id: Option<String>,
    pub event_id: Option<String>,
    pub occurrence_id: Option<String>,
    pub event_title: Option<String>,
    pub phase: Option<FocusPhase>,
    pub mode: FocusMode,
    pub phase_deadline_ms: Option<i64>,
    pub event_deadline_ms: Option<i64>,
    pub remaining_ms: i64,
    pub valid_until_ms: i64,
}

impl CommittedFocusEffect {
    /// Consumers share the vault ownership fence, while their local runtime
    /// generations remain independent of the Focus publication generation.
    pub fn matches_vault_authority(&self, vault_id: &str, ownership_generation: u64) -> bool {
        self.vault_id == vault_id && self.ownership_generation == ownership_generation
    }

    /// An owner heartbeat cannot extend an accepted phase or Calendar deadline.
    /// A paused phase retains its work allowance but still expires with its run.
    pub fn effective_valid_until_ms(&self) -> i64 {
        self.valid_until_ms
            .min(self.event_deadline_ms.unwrap_or(i64::MAX))
            .min(if self.mode == FocusMode::Running {
                self.phase_deadline_ms.unwrap_or(0)
            } else {
                i64::MAX
            })
    }
}

/// Durable control facts that are not themselves evidence of an executed phase.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ExecutionState {
    pub run_id: Option<String>,
    pub segment_id: Option<String>,
    pub mode: FocusMode,
    pub calendar_configuration: Option<FocusConfiguration>,
    pub skip_next_break: bool,
    pub focus_extension_used: bool,
    pub break_extension_ms: i64,
    pub dismissed_occurrence_id: Option<String>,
    pub automatic_admission_suppressed: bool,
    pub paused_prompts_dismissed: bool,
    pub idle_timeout_override_set: bool,
    pub idle_timeout_override_minutes: Option<i64>,
    pub idle_started_at_ms: Option<i64>,
    pub idle_detected_at_ms: Option<i64>,
    pub idle_overlay_visible_at_ms: Option<i64>,
    pub focus_failed_at_ms: Option<i64>,
    pub suspend_started_at_ms: Option<i64>,
    pub suspend_returned_at_ms: Option<i64>,
    pub return_started_at_ms: Option<i64>,
    pub activity_source_unavailable: bool,
    pub last_transition_at_ms: i64,
}

/// Canonical run context retained by UI and native automation consumers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusRunSnapshot {
    pub id: String,
    pub event_id: Option<String>,
    pub occurrence_id: String,
    pub event_date: String,
    pub title: Option<String>,
    pub started_at_ms: i64,
    pub planned_start_ms: i64,
    pub planned_end_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub inherited_focus_ms: i64,
    pub inherited_phase_ms: i64,
    pub configuration: FocusConfiguration,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusPauseSnapshot {
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub reason: String,
}

/// Only a phase that actually started can appear in this DTO.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusSegmentSnapshot {
    pub id: String,
    pub run_id: String,
    pub event_id: Option<String>,
    pub event_date: String,
    pub phase: FocusPhase,
    pub rhythm_position: i64,
    pub planned_start_ms: i64,
    pub planned_end_ms: i64,
    pub actual_start_ms: i64,
    pub actual_end_ms: Option<i64>,
    pub chosen_duration_ms: i64,
    pub status: String,
    pub end_reason: Option<String>,
    pub pauses: Vec<FocusPauseSnapshot>,
}

/// A committed operation result. Visual countdown interpolation cannot mutate it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusExecutionSnapshot {
    pub revision: i64,
    pub observed_at_ms: i64,
    pub mode: FocusMode,
    pub run: Option<FocusRunSnapshot>,
    pub segment: Option<FocusSegmentSnapshot>,
    pub changed_segments: Vec<FocusSegmentSnapshot>,
    pub phase_deadline_ms: Option<i64>,
    pub remaining_ms: i64,
    pub elapsed_ms: i64,
    pub completed_focus_count: i64,
    pub skip_next_break: bool,
    pub focus_extension_used: bool,
    pub break_extension_ms: i64,
    pub dismissed_occurrence_id: Option<String>,
    pub automatic_admission_suppressed: bool,
    pub paused_prompts_dismissed: bool,
    pub idle_started_at_ms: Option<i64>,
    pub idle_detected_at_ms: Option<i64>,
    /// Visibility acknowledged by the current controller; unseen warnings cannot trigger idle failure.
    pub idle_overlay_visible_at_ms: Option<i64>,
    pub focus_failed_at_ms: Option<i64>,
    pub suspend_started_at_ms: Option<i64>,
    pub suspend_returned_at_ms: Option<i64>,
    pub return_started_at_ms: Option<i64>,
    pub activity_source_unavailable: bool,
    pub effective_idle_timeout_minutes: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusErrorCode {
    InvalidIntent,
    StaleRevision,
    CommandIdentityConflict,
    InvalidState,
    IneligibleCommitment,
    Persistence,
    Unavailable,
    Busy,
    StaleGeneration,
    ReadOnly,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusExecutionError {
    pub code: FocusErrorCode,
    pub message: String,
    pub current_revision: Option<i64>,
}

impl std::fmt::Display for FocusExecutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FocusExecutionError {}

impl From<String> for FocusExecutionError {
    fn from(message: String) -> Self {
        Self {
            code: FocusErrorCode::Persistence,
            message,
            current_revision: None,
        }
    }
}

pub(super) fn execution_error(
    code: FocusErrorCode,
    message: impl Into<String>,
) -> FocusExecutionError {
    FocusExecutionError {
        code,
        message: message.into(),
        current_revision: None,
    }
}
