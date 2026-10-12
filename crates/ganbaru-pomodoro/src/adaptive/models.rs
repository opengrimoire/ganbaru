use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Device-local facts for an instant, resolved by the native platform adapter.
/// The seed date matches JavaScript Date.toDateString's English date spelling.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalTimeFact {
    pub epoch_ms: i64,
    pub date_key: String,
    pub date_string: String,
    pub hour: u8,
}

pub type LocalTimeFacts = BTreeMap<i64, LocalTimeFact>;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseInput {
    pub started_at: String,
    pub ended_at: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentInput {
    pub run_id: Option<String>,
    pub rhythm_position: Option<i64>,
    pub phase: String,
    pub planned_start: String,
    pub planned_end: String,
    pub actual_start: Option<String>,
    pub actual_end: Option<String>,
    pub status: String,
    pub end_reason: Option<String>,
    pub pause_log: Vec<PauseInput>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEventInput {
    pub event_type: String,
    pub occurred_at: String,
    pub phase: Option<String>,
    pub reason: Option<String>,
    pub duration_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockEventInput {
    pub occurred_at: String,
    pub phase: Option<String>,
    pub source_type: String,
    pub source_key: String,
    pub decision: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureInput {
    pub segments: Vec<SegmentInput>,
    #[serde(default)]
    pub run_events: Vec<RunEventInput>,
    #[serde(default)]
    pub block_events: Vec<BlockEventInput>,
    #[serde(default)]
    pub data_quality_flags: Vec<String>,
    pub observation_ended_at: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CountRhythm {
    pub kind: CountKind,
    pub focus_duration_minutes: i64,
    pub short_break_minutes: i64,
    pub long_break_minutes: i64,
    pub long_break_after_focus_count: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CountKind {
    Count,
}

impl CountRhythm {
    pub const BASELINE: Self = Self {
        kind: CountKind::Count,
        focus_duration_minutes: 40,
        short_break_minutes: 5,
        long_break_minutes: 10,
        long_break_after_focus_count: 4,
    };

    pub fn from_rhythm(rhythm: &crate::PomodoroRunRhythm) -> Option<Self> {
        match rhythm {
            crate::PomodoroRunRhythm::Count {
                focus_duration_minutes,
                short_break_minutes,
                long_break_minutes,
                long_break_after_focus_count,
            } => Some(Self {
                kind: CountKind::Count,
                focus_duration_minutes: *focus_duration_minutes,
                short_break_minutes: *short_break_minutes,
                long_break_minutes: *long_break_minutes,
                long_break_after_focus_count: *long_break_after_focus_count,
            }),
            crate::PomodoroRunRhythm::Sequence { .. } => None,
        }
    }

    pub fn into_rhythm(self) -> crate::PomodoroRunRhythm {
        crate::PomodoroRunRhythm::Count {
            focus_duration_minutes: self.focus_duration_minutes,
            short_break_minutes: self.short_break_minutes,
            long_break_minutes: self.long_break_minutes,
            long_break_after_focus_count: self.long_break_after_focus_count,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextBucket {
    pub time_of_day: String,
    pub session_position: String,
    pub event_length: String,
    pub workload: String,
    pub energy: String,
    pub environment_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeatureVector {
    pub completed_focus_segments: f64,
    pub interrupted_focus_segments: f64,
    pub focus_failure_count: f64,
    pub late_focus_segment_count: f64,
    pub late_focus_failure_count: f64,
    pub clean_focus_seconds: f64,
    pub planned_focus_seconds: f64,
    pub idle_pause_count: f64,
    pub idle_pause_seconds: f64,
    pub focus_idle_pause_count: f64,
    pub focus_idle_pause_seconds: f64,
    pub early_focus_idle_pause_count: f64,
    pub late_focus_idle_pause_count: f64,
    pub manual_pause_count: f64,
    pub manual_pause_seconds: f64,
    pub suspend_pause_count: f64,
    pub suspend_pause_seconds: f64,
    pub break_started_count: f64,
    pub break_completed_count: f64,
    pub break_skipped_count: f64,
    pub skipped_break_next_focus_success_count: f64,
    pub skipped_break_next_focus_failure_count: f64,
    pub skipped_short_break_next_focus_failure_count: f64,
    pub skipped_long_break_next_focus_failure_count: f64,
    pub short_break_overtime_seconds: f64,
    pub long_break_overtime_seconds: f64,
    pub blocked_attempt_count: f64,
    pub blocked_burst_count: f64,
    pub repeated_blocked_source_attempt_count: f64,
    pub focus_repeated_blocked_source_attempt_count: f64,
    pub break_repeated_blocked_source_attempt_count: f64,
    pub focus_blocked_attempt_count: f64,
    pub early_focus_blocked_attempt_count: f64,
    pub late_focus_blocked_attempt_count: f64,
    pub break_blocked_attempt_count: f64,
    pub break_overtime_blocked_attempt_count: f64,
    pub short_break_overtime_blocked_attempt_count: f64,
    pub long_break_overtime_blocked_attempt_count: f64,
    pub extension_count: f64,
    pub go_to_break_now_count: f64,
    pub early_go_to_break_now_count: f64,
    pub late_go_to_break_now_count: f64,
    pub start_focus_now_count: f64,
    pub start_focus_now_success_count: f64,
    pub start_focus_now_failure_count: f64,
    pub stop_count: f64,
    pub comparable_opportunity_count: f64,
    pub data_quality_flags: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateScores {
    pub readiness: f64,
    pub strain: f64,
    pub recovery_debt: f64,
    pub avoidance_pressure: f64,
    pub momentum: f64,
    pub confidence: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyDecision {
    pub selected_rhythm: CountRhythm,
    pub mode: String,
    pub reason_codes: Vec<String>,
    pub state_scores: StateScores,
}
