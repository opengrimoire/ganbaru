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

/// Every legal Focus mode change. `Stopped` and `Expired` hold no open run; every other
/// mode belongs to exactly one open run, so a run start leaves a closed mode and any open
/// mode can close. A closed mode may also close again: closing requires an open run in
/// SQLite, so that move only happens when the state row disagrees with committed execution,
/// and recovery must still close the run. `Running` to `Running` is a phase change inside
/// the same run, and `Running` to a pause mode also restores a pause across a Calendar
/// reconfiguration.
pub(super) const FOCUS_MODE_TRANSITIONS: &[(FocusMode, FocusMode)] = &[
    (FocusMode::Stopped, FocusMode::Running),
    (FocusMode::Stopped, FocusMode::Stopped),
    (FocusMode::Stopped, FocusMode::Expired),
    (FocusMode::Expired, FocusMode::Running),
    (FocusMode::Expired, FocusMode::Stopped),
    (FocusMode::Expired, FocusMode::Expired),
    (FocusMode::Running, FocusMode::Running),
    (FocusMode::Running, FocusMode::ManualPause),
    (FocusMode::Running, FocusMode::IdlePause),
    (FocusMode::Running, FocusMode::Suspended),
    (FocusMode::Running, FocusMode::ReturnWait),
    (FocusMode::Running, FocusMode::Stopped),
    (FocusMode::Running, FocusMode::Expired),
    (FocusMode::ManualPause, FocusMode::Running),
    (FocusMode::ManualPause, FocusMode::ReturnWait),
    (FocusMode::ManualPause, FocusMode::Stopped),
    (FocusMode::ManualPause, FocusMode::Expired),
    (FocusMode::IdlePause, FocusMode::Running),
    (FocusMode::IdlePause, FocusMode::IdleFailed),
    (FocusMode::IdlePause, FocusMode::ReturnWait),
    (FocusMode::IdlePause, FocusMode::Stopped),
    (FocusMode::IdlePause, FocusMode::Expired),
    (FocusMode::IdleFailed, FocusMode::Running),
    (FocusMode::IdleFailed, FocusMode::Stopped),
    (FocusMode::IdleFailed, FocusMode::Expired),
    (FocusMode::Suspended, FocusMode::Running),
    (FocusMode::Suspended, FocusMode::ReturnWait),
    (FocusMode::Suspended, FocusMode::Stopped),
    (FocusMode::Suspended, FocusMode::Expired),
    (FocusMode::ReturnWait, FocusMode::Running),
    (FocusMode::ReturnWait, FocusMode::Stopped),
    (FocusMode::ReturnWait, FocusMode::Expired),
];

impl FocusMode {
    /// Whether [`FOCUS_MODE_TRANSITIONS`] allows moving from this mode to `next`.
    pub(super) fn can_transition_to(self, next: Self) -> bool {
        FOCUS_MODE_TRANSITIONS.contains(&(self, next))
    }
}

/// The execution mode with the facts that exist only while Focus is in it.
///
/// Serialized with its mode name under `mode` and its facts beside it, so the wire snapshot
/// keeps `mode` as a plain discriminant. Entering a mode replaces the whole value, which
/// drops the previous mode's idle, suspend, or return evidence.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "mode",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FocusModeState {
    #[default]
    Stopped,
    Running,
    ManualPause {
        paused_prompts_dismissed: bool,
    },
    IdlePause {
        idle_started_at_ms: i64,
        idle_detected_at_ms: i64,
        /// Visibility acknowledged by the current controller; unseen warnings cannot
        /// trigger idle failure.
        idle_overlay_visible_at_ms: Option<i64>,
        paused_prompts_dismissed: bool,
    },
    IdleFailed {
        idle_started_at_ms: i64,
        idle_detected_at_ms: i64,
        focus_failed_at_ms: i64,
        paused_prompts_dismissed: bool,
    },
    Suspended {
        suspend_started_at_ms: i64,
        suspend_returned_at_ms: i64,
        paused_prompts_dismissed: bool,
    },
    ReturnWait {
        return_started_at_ms: i64,
        paused_prompts_dismissed: bool,
    },
    Expired,
}

impl FocusModeState {
    /// The mode without its facts, as used by the transition table and native consumers.
    pub fn kind(&self) -> FocusMode {
        match self {
            Self::Stopped => FocusMode::Stopped,
            Self::Running => FocusMode::Running,
            Self::ManualPause { .. } => FocusMode::ManualPause,
            Self::IdlePause { .. } => FocusMode::IdlePause,
            Self::IdleFailed { .. } => FocusMode::IdleFailed,
            Self::Suspended { .. } => FocusMode::Suspended,
            Self::ReturnWait { .. } => FocusMode::ReturnWait,
            Self::Expired => FocusMode::Expired,
        }
    }

    /// Whether a paused-state prompt was dismissed; modes without prompts have none.
    pub fn paused_prompts_dismissed(&self) -> bool {
        match self {
            Self::ManualPause {
                paused_prompts_dismissed,
            }
            | Self::IdlePause {
                paused_prompts_dismissed,
                ..
            }
            | Self::IdleFailed {
                paused_prompts_dismissed,
                ..
            }
            | Self::Suspended {
                paused_prompts_dismissed,
                ..
            }
            | Self::ReturnWait {
                paused_prompts_dismissed,
                ..
            } => *paused_prompts_dismissed,
            Self::Stopped | Self::Running | Self::Expired => false,
        }
    }

    /// The dismissal flag of a mode that shows paused prompts, or `None` for one that shows none.
    pub(super) fn paused_prompts_dismissed_mut(&mut self) -> Option<&mut bool> {
        match self {
            Self::ManualPause {
                paused_prompts_dismissed,
            }
            | Self::IdlePause {
                paused_prompts_dismissed,
                ..
            }
            | Self::IdleFailed {
                paused_prompts_dismissed,
                ..
            }
            | Self::Suspended {
                paused_prompts_dismissed,
                ..
            }
            | Self::ReturnWait {
                paused_prompts_dismissed,
                ..
            } => Some(paused_prompts_dismissed),
            Self::Stopped | Self::Running | Self::Expired => None,
        }
    }

    /// When the current idle episode began, while idle paused or after it failed the phase.
    pub fn idle_started_at_ms(&self) -> Option<i64> {
        match self {
            Self::IdlePause {
                idle_started_at_ms, ..
            }
            | Self::IdleFailed {
                idle_started_at_ms, ..
            } => Some(*idle_started_at_ms),
            _ => None,
        }
    }

    /// When native execution detected the current idle episode.
    pub fn idle_detected_at_ms(&self) -> Option<i64> {
        match self {
            Self::IdlePause {
                idle_detected_at_ms,
                ..
            }
            | Self::IdleFailed {
                idle_detected_at_ms,
                ..
            } => Some(*idle_detected_at_ms),
            _ => None,
        }
    }

    /// When the live controller showed the idle warning, only while idle paused.
    pub fn idle_overlay_visible_at_ms(&self) -> Option<i64> {
        match self {
            Self::IdlePause {
                idle_overlay_visible_at_ms,
                ..
            } => *idle_overlay_visible_at_ms,
            _ => None,
        }
    }

    /// When an elapsed phase began waiting for the user to return, only while waiting.
    pub fn return_started_at_ms(&self) -> Option<i64> {
        match self {
            Self::ReturnWait {
                return_started_at_ms,
                ..
            } => Some(*return_started_at_ms),
            _ => None,
        }
    }

    /// The start of the interruption this mode waits on, if it records one.
    pub fn wait_started_at_ms(&self) -> Option<i64> {
        match self {
            Self::IdlePause {
                idle_started_at_ms, ..
            }
            | Self::IdleFailed {
                idle_started_at_ms, ..
            } => Some(*idle_started_at_ms),
            Self::Suspended {
                suspend_started_at_ms,
                ..
            } => Some(*suspend_started_at_ms),
            Self::ReturnWait {
                return_started_at_ms,
                ..
            } => Some(*return_started_at_ms),
            Self::Stopped | Self::Running | Self::ManualPause { .. } | Self::Expired => None,
        }
    }

    /// The pause a Calendar reconfiguration restores in the run that replaces this one,
    /// with the pause reason it records. Idle and suspend evidence restarts at `now_ms`,
    /// because the new run's phase starts then; acknowledgements carry over.
    pub(super) fn carried_into_reconfigured_run(
        &self,
        now_ms: i64,
    ) -> Option<(&'static str, FocusModeState)> {
        match *self {
            Self::ManualPause {
                paused_prompts_dismissed,
            } => Some((
                "manual",
                Self::ManualPause {
                    paused_prompts_dismissed,
                },
            )),
            Self::IdlePause {
                idle_detected_at_ms,
                idle_overlay_visible_at_ms,
                paused_prompts_dismissed,
                ..
            } => Some((
                "idle",
                Self::IdlePause {
                    idle_started_at_ms: now_ms,
                    idle_detected_at_ms,
                    idle_overlay_visible_at_ms,
                    paused_prompts_dismissed,
                },
            )),
            Self::Suspended {
                suspend_returned_at_ms,
                paused_prompts_dismissed,
                ..
            } => Some((
                "suspend",
                Self::Suspended {
                    suspend_started_at_ms: now_ms,
                    suspend_returned_at_ms: suspend_returned_at_ms.max(now_ms),
                    paused_prompts_dismissed,
                },
            )),
            Self::Stopped
            | Self::Running
            | Self::IdleFailed { .. }
            | Self::ReturnWait { .. }
            | Self::Expired => None,
        }
    }
}

impl PartialEq<FocusMode> for FocusModeState {
    fn eq(&self, other: &FocusMode) -> bool {
        self.kind() == *other
    }
}

/// Lifecycle of one accepted phase, matching the `pomodoro_segments.status` CHECK.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusSegmentStatus {
    Active,
    Completed,
    Interrupted,
}

impl FocusSegmentStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Interrupted => "interrupted",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, FocusExecutionError> {
        match value {
            "active" => Ok(Self::Active),
            "completed" => Ok(Self::Completed),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(execution_error(
                FocusErrorCode::InvalidState,
                "Invalid canonical Focus segment status",
            )),
        }
    }
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
    /// Stable device id whose activity row each transition writes; `None` writes no activity,
    /// as in a vault without a device identity.
    pub device_id: Option<String>,
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
/// Vault identity and generation prevent effects from crossing vault replacements.
#[derive(Clone, Debug)]
pub struct CommittedFocusEffect {
    pub vault_id: String,
    pub vault_generation: u64,
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
///
/// The mode and its facts are one nested value under `mode`, so a stored row cannot hold
/// idle, suspend, or return evidence for a mode it is not in. An empty object is a stopped
/// state, which is how the baseline seeds the row.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ExecutionState {
    pub run_id: Option<String>,
    pub segment_id: Option<String>,
    pub mode: FocusModeState,
    pub calendar_configuration: Option<FocusConfiguration>,
    pub skip_next_break: bool,
    pub focus_extension_used: bool,
    pub break_extension_ms: i64,
    pub dismissed_occurrence_id: Option<String>,
    pub automatic_admission_suppressed: bool,
    pub idle_timeout_override_set: bool,
    pub idle_timeout_override_minutes: Option<i64>,
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
    pub status: FocusSegmentStatus,
    pub end_reason: Option<String>,
    pub pauses: Vec<FocusPauseSnapshot>,
}

/// A committed operation result. Visual countdown interpolation cannot mutate it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusExecutionSnapshot {
    pub revision: i64,
    pub observed_at_ms: i64,
    /// The mode name on the wire as `mode`, with that mode's facts beside it.
    #[serde(flatten)]
    pub mode: FocusModeState,
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
    pub activity_source_unavailable: bool,
    pub effective_idle_timeout_minutes: Option<i64>,
    /// Why this transition skipped adaptive evidence, for the native owner's log.
    /// It stays off the wire and out of stored receipts.
    #[serde(skip)]
    pub adaptive_degraded: Option<String>,
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
