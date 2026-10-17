use serde::{Deserialize, Serialize};

use crate::adaptive::models::{ContextBucket, CountRhythm, FeatureVector, StateScores};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentLane {
    FocusDuration,
    ShortBreak,
    LongBreak,
    EarlierCadence,
    LaterCadence,
    FocusSupport,
    LongRecovery,
}

impl ExperimentLane {
    /// Selection order is deliberate: bundles precede their scalar alternatives.
    pub const SELECTION_ORDER: [Self; 7] = [
        Self::FocusSupport,
        Self::FocusDuration,
        Self::ShortBreak,
        Self::LongRecovery,
        Self::LongBreak,
        Self::EarlierCadence,
        Self::LaterCadence,
    ];

    pub fn definition(self) -> ExperimentDefinition {
        let (id, parameter, control, control_value, treatment, treatment_value) = match self {
            Self::FocusDuration => (
                "run-focus-duration-40-vs-45-v1",
                "focus_duration_minutes",
                "control_40",
                40.0,
                "focus_45",
                45.0,
            ),
            Self::ShortBreak => (
                "run-short-break-duration-5-vs-7-v1",
                "short_break_minutes",
                "control_5",
                5.0,
                "short_break_7",
                7.0,
            ),
            Self::LongBreak => (
                "run-long-break-duration-10-vs-15-v1",
                "long_break_minutes",
                "control_10",
                10.0,
                "long_break_15",
                15.0,
            ),
            Self::EarlierCadence => (
                "run-long-break-cadence-4-vs-3-v1",
                "long_break_after_focus_count",
                "control_4",
                4.0,
                "cadence_3",
                3.0,
            ),
            Self::LaterCadence => (
                "run-long-break-cadence-4-vs-5-v1",
                "long_break_after_focus_count",
                "control_4",
                4.0,
                "cadence_5",
                5.0,
            ),
            Self::FocusSupport => (
                "run-focus-short-break-support-40-5-vs-45-7-v1",
                "rhythm_bundle",
                "control_40_5",
                0.0,
                "focus_45_short_break_7",
                1.0,
            ),
            Self::LongRecovery => (
                "run-long-recovery-support-15-c4-vs-15-c3-v1",
                "rhythm_bundle",
                "long_break_15_cadence_4",
                0.0,
                "long_break_15_cadence_3",
                1.0,
            ),
        };
        ExperimentDefinition {
            id: id.to_owned(),
            parameter_key: parameter.to_owned(),
            assignment_unit: "run".to_owned(),
            status: "active".to_owned(),
            variants: vec![
                ExperimentVariant {
                    variant_key: control.to_owned(),
                    numeric_value: control_value,
                    is_control: true,
                },
                ExperimentVariant {
                    variant_key: treatment.to_owned(),
                    numeric_value: treatment_value,
                    is_control: false,
                },
            ],
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentDefinition {
    pub id: String,
    pub parameter_key: String,
    pub assignment_unit: String,
    pub status: String,
    pub variants: Vec<ExperimentVariant>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentVariant {
    pub variant_key: String,
    pub numeric_value: f64,
    pub is_control: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentState {
    pub experiment_id: String,
    pub status: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentHistory {
    pub experiment_id: String,
    pub variant_key: String,
    pub context_key: String,
    pub assigned_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentAssignment {
    pub experiment: ExperimentDefinition,
    pub variant: ExperimentVariant,
    pub assignment_seed: String,
    pub assigned_at: String,
    pub selected_rhythm: CountRhythm,
}

macro_rules! numeric_outcomes {
    ($($field:ident),+ $(,)?) => {
        #[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct OutcomeValues { $(pub $field: f64,)+ }
        impl OutcomeValues {
            pub fn scaled(&self, weight: f64) -> Self { Self { $($field: self.$field * weight,)+ } }
            pub fn add(&mut self, other: &Self) { $(self.$field += other.$field;)+ }
        }
    };
}

numeric_outcomes!(
    assignment_count,
    run_observed_count,
    run_completed_count,
    run_stopped_count,
    clean_focus_seconds_sum,
    clean_focus_seconds_square_sum,
    blocked_attempt_count_sum,
    blocked_attempt_count_square_sum,
    break_skipped_count_sum,
    break_skipped_count_square_sum,
    short_break_overtime_seconds_sum,
    short_break_overtime_seconds_square_sum,
    long_break_overtime_seconds_sum,
    long_break_overtime_seconds_square_sum,
    day_observed_count,
    day_started_planned_pomodoro_count_sum,
    day_missed_planned_pomodoro_count_sum,
    day_missed_planned_pomodoro_count_square_sum,
    day_clean_focus_seconds_sum,
    day_blocked_attempt_count_sum,
    day_blocked_attempt_count_square_sum,
    next_day_observed_count,
    next_day_started_run_count,
    next_day_clean_focus_seconds_sum,
    next_day_blocked_attempt_count_sum
);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentOutcome {
    pub experiment_id: String,
    pub variant_key: String,
    pub context_key: Option<String>,
    #[serde(flatten)]
    pub values: OutcomeValues,
}

impl ExperimentOutcome {
    pub fn empty(experiment_id: &str, variant_key: &str, context_key: Option<&str>) -> Self {
        Self {
            experiment_id: experiment_id.to_owned(),
            variant_key: variant_key.to_owned(),
            context_key: context_key.map(str::to_owned),
            values: OutcomeValues::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentAnalysis {
    pub experiment_id: String,
    pub control_variant_key: String,
    pub treatment_variant_key: String,
    pub control_observed_runs: f64,
    pub treatment_observed_runs: f64,
    pub control_completion_rate: f64,
    pub treatment_completion_rate: f64,
    pub control_stop_rate: f64,
    pub treatment_stop_rate: f64,
    pub control_clean_focus_seconds_mean: f64,
    pub treatment_clean_focus_seconds_mean: f64,
    pub control_blocked_attempts_mean: f64,
    pub treatment_blocked_attempts_mean: f64,
    pub control_break_skipped_mean: f64,
    pub treatment_break_skipped_mean: f64,
    pub control_short_break_overtime_seconds_mean: f64,
    pub treatment_short_break_overtime_seconds_mean: f64,
    pub control_long_break_overtime_seconds_mean: f64,
    pub treatment_long_break_overtime_seconds_mean: f64,
    pub control_day_missed_planned_pomodoro_mean: Option<f64>,
    pub treatment_day_missed_planned_pomodoro_mean: Option<f64>,
    pub control_day_blocked_attempts_mean: Option<f64>,
    pub treatment_day_blocked_attempts_mean: Option<f64>,
    pub control_next_day_started_rate: Option<f64>,
    pub treatment_next_day_started_rate: Option<f64>,
    pub analysis_scope: String,
    pub guardrail_breached: bool,
    pub decision: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionInput {
    pub occurred_at: String,
    pub current_rhythm: CountRhythm,
    pub selected_rhythm: CountRhythm,
    pub context: ContextBucket,
    pub features: FeatureVector,
    pub state: StateScores,
    #[serde(default)]
    pub experiment_outcomes: Vec<ExperimentOutcome>,
    #[serde(default)]
    pub experiment_states: Vec<ExperimentState>,
    #[serde(default)]
    pub experiment_assignments: Vec<AssignmentHistory>,
}
