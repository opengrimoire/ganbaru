//! Validated Pomodoro write requests accepted from command adapters.

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroRunWrite {
    pub(crate) id: String,
    pub(crate) event_id: String,
    pub(crate) event_date: String,
    pub(crate) planned_start: String,
    pub(crate) planned_end: String,
    pub(crate) started_at: String,
    pub(crate) rhythm: PomodoroRunRhythm,
    pub(crate) rhythm_source: String,
    pub(crate) preset_key: Option<String>,
    pub(crate) idle_timeout_minutes: Option<i64>,
    pub(crate) event_title_snapshot: Option<String>,
    pub(crate) inherited_focus_minutes: i64,
    pub(crate) inherited_rhythm_position: i64,
    pub(crate) inherited_from_run_id: Option<String>,
    pub(crate) start_trigger: String,
    pub(crate) adaptive_snapshot: Option<PomodoroRunAdaptiveSnapshotWrite>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroRunAdaptiveSnapshotWrite {
    pub(crate) policy_id: String,
    pub(crate) policy_version: i64,
    pub(crate) model_version: i64,
    pub(crate) context_snapshot: PomodoroAdaptiveContextSnapshotWrite,
    pub(crate) decision: PomodoroAdaptiveDecisionWrite,
    #[serde(default)]
    pub(crate) planned_blocks: Vec<PomodoroAdaptivePlannedBlockWrite>,
    #[serde(default)]
    pub(crate) experiment_updates: Vec<PomodoroAdaptiveExperimentWrite>,
    pub(crate) experiment_assignments: Vec<PomodoroAdaptiveExperimentAssignmentWrite>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveDecisionEnvelopeWrite {
    pub(crate) policy_id: String,
    pub(crate) policy_version: i64,
    pub(crate) model_version: i64,
    pub(crate) context_snapshot: PomodoroAdaptiveContextSnapshotWrite,
    pub(crate) decision: PomodoroAdaptiveDecisionWrite,
    #[serde(default)]
    pub(crate) experiment_updates: Vec<PomodoroAdaptiveExperimentWrite>,
    pub(crate) experiment_assignments: Vec<PomodoroAdaptiveExperimentAssignmentWrite>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveContextSnapshotWrite {
    pub(crate) id: String,
    pub(crate) run_id: String,
    pub(crate) segment_id: Option<String>,
    pub(crate) local_started_at: String,
    pub(crate) time_of_day: String,
    pub(crate) session_position: String,
    pub(crate) event_length: String,
    pub(crate) workload: String,
    pub(crate) energy: String,
    pub(crate) environment_id: Option<String>,
    pub(crate) features: Vec<PomodoroAdaptiveFeatureWrite>,
    pub(crate) data_quality_flags: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveFeatureWrite {
    pub(crate) feature_key: String,
    pub(crate) numeric_value: Option<f64>,
    pub(crate) categorical_value: Option<String>,
    pub(crate) boolean_value: Option<bool>,
    pub(crate) missing: bool,
    pub(crate) source_kind: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveDecisionWrite {
    pub(crate) id: String,
    pub(crate) policy_id: String,
    pub(crate) run_id: String,
    pub(crate) segment_id: Option<String>,
    pub(crate) context_snapshot_id: String,
    pub(crate) opportunity_kind: String,
    #[serde(default)]
    pub(crate) candidate_id: Option<String>,
    pub(crate) decision_mode: String,
    pub(crate) policy_version: i64,
    pub(crate) model_version: i64,
    pub(crate) occurred_at: String,
    pub(crate) values: Vec<PomodoroAdaptiveDecisionValueWrite>,
    pub(crate) reason_codes: Vec<String>,
    pub(crate) state_scores: PomodoroAdaptiveStateScoresWrite,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveDecisionValueWrite {
    pub(crate) value_key: String,
    pub(crate) previous_numeric_value: Option<f64>,
    pub(crate) selected_numeric_value: f64,
    pub(crate) value_unit: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveExperimentAssignmentWrite {
    pub(crate) experiment: PomodoroAdaptiveExperimentWrite,
    pub(crate) assignment: PomodoroAdaptiveAssignmentWrite,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptivePlannedBlockWrite {
    pub event_date: String,
    pub event_id: Option<String>,
    pub original_event_id: String,
    pub planned_start: String,
    pub planned_end: String,
    pub source_kind: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveExperimentWrite {
    pub(crate) id: String,
    pub(crate) policy_id: String,
    pub(crate) parameter_key: String,
    pub(crate) assignment_unit: String,
    pub(crate) status: String,
    pub(crate) started_at: Option<String>,
    pub(crate) ended_at: Option<String>,
    pub(crate) variants: Vec<PomodoroAdaptiveExperimentVariantWrite>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveExperimentVariantWrite {
    pub(crate) variant_key: String,
    pub(crate) numeric_value: f64,
    pub(crate) is_control: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveAssignmentWrite {
    pub(crate) id: String,
    pub(crate) experiment_id: String,
    pub(crate) variant_key: String,
    pub(crate) run_id: String,
    pub(crate) segment_id: Option<String>,
    pub(crate) context_snapshot_id: String,
    pub(crate) assignment_seed: String,
    pub(crate) assigned_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveStateScoresWrite {
    pub(crate) readiness: f64,
    pub(crate) strain: f64,
    pub(crate) recovery_debt: f64,
    pub(crate) avoidance_pressure: f64,
    pub(crate) momentum: f64,
    pub(crate) confidence: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PomodoroRunRhythm {
    Count {
        focus_duration_minutes: i64,
        short_break_minutes: i64,
        long_break_minutes: i64,
        long_break_after_focus_count: i64,
    },
    Sequence {
        steps: Vec<PomodoroRunSequenceStep>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroRunSequenceStep {
    pub focus_duration_minutes: i64,
    pub break_phase: String,
    pub break_duration_minutes: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroRunClosure {
    pub(crate) run_id: String,
    pub(crate) ended_at: String,
    pub(crate) end_reason: String,
    pub(crate) segment_status: String,
    pub(crate) segment_end_reason: String,
    pub(crate) event_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroSegmentWrite {
    pub(crate) id: String,
    pub(crate) event_id: String,
    pub(crate) event_date: String,
    pub(crate) run_id: String,
    pub(crate) rhythm_position: i64,
    pub(crate) phase: String,
    pub(crate) planned_start: String,
    pub(crate) planned_end: String,
    pub(crate) actual_start: Option<String>,
    pub(crate) actual_end: Option<String>,
    pub(crate) pauses: Vec<PomodoroPauseWrite>,
    pub(crate) status: String,
    pub(crate) end_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroPauseWrite {
    pub(crate) started_at: String,
    pub(crate) ended_at: Option<String>,
    pub(crate) reason: String,
}
