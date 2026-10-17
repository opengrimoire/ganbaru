//! Durable focus history, validation, and recovery independent of Tauri and the WebView.
//!
//! Callers supply an authorized local SQLite pool. Notification schedules are reminders,
//! never evidence of an accepted run or authority to start a later focus interval.

pub mod adaptive;
pub mod admission;
mod execution;
pub use execution::*;
mod reads;
pub use reads::*;
#[cfg(test)]
mod tests;
mod time;
mod validation;
mod writes;
pub use writes::requests::*;

use serde::{Deserialize, Serialize};

#[cfg(test)]
use reads::{load_adaptive_history_from_pool, load_adaptive_replay_dataset_from_pool};
#[cfg(test)]
use validation::{
    canonical_event_id, validate_adaptive_decision_envelope_for_segment, validate_event_type,
    validate_pause_reason, validate_phase, validate_run_rhythm,
};
#[cfg(test)]
use writes::{
    RunEventInsert, close_run_tx, insert_adaptive_decision_envelope_tx, insert_run_event_tx,
    insert_run_tx, insert_segment_tx, record_matured_adaptive_day_outcomes_tx,
    record_matured_adaptive_next_day_outcomes_tx,
};

const MAX_ADAPTIVE_PLANNED_BLOCKS_PER_SNAPSHOT: usize = 512;

#[derive(Serialize)]
pub struct PomodoroSegmentRead {
    id: String,
    event_id: String,
    event_date: String,
    run_id: String,
    rhythm_position: i64,
    phase: String,
    planned_start: String,
    planned_end: String,
    actual_start: Option<String>,
    actual_end: Option<String>,
    status: String,
    pauses: Vec<PomodoroPauseWrite>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveHistoryRead {
    segments: Vec<PomodoroAdaptiveHistorySegmentRead>,
    run_events: Vec<PomodoroAdaptiveHistoryRunEventRead>,
    block_events: Vec<PomodoroAdaptiveHistoryBlockEventRead>,
    previous_states: Vec<PomodoroAdaptiveHistoryContextStateRead>,
    experiment_states: Vec<PomodoroAdaptiveHistoryExperimentStateRead>,
    experiment_outcomes: Vec<PomodoroAdaptiveHistoryExperimentOutcomeRead>,
    experiment_assignments: Vec<PomodoroAdaptiveHistoryExperimentAssignmentRead>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveHistorySegmentRead {
    run_id: String,
    rhythm_position: i64,
    phase: String,
    planned_start: String,
    planned_end: String,
    actual_start: Option<String>,
    actual_end: Option<String>,
    status: String,
    end_reason: Option<String>,
    pause_log: Vec<PomodoroPauseWrite>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveHistoryRunEventRead {
    event_type: String,
    occurred_at: String,
    phase: Option<String>,
    reason: Option<String>,
    duration_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveHistoryBlockEventRead {
    occurred_at: String,
    phase: Option<String>,
    source_type: String,
    source_key: String,
    decision: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveHistoryContextStateRead {
    context_key: String,
    readiness: f64,
    strain: f64,
    recovery_debt: f64,
    avoidance_pressure: f64,
    momentum: f64,
    confidence: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveHistoryExperimentStateRead {
    experiment_id: String,
    status: String,
    started_at: Option<String>,
    ended_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveHistoryExperimentAssignmentRead {
    experiment_id: String,
    variant_key: String,
    context_key: String,
    assigned_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveHistoryExperimentOutcomeRead {
    experiment_id: String,
    variant_key: String,
    context_key: String,
    assignment_count: i64,
    run_observed_count: i64,
    run_completed_count: i64,
    run_stopped_count: i64,
    clean_focus_seconds_sum: f64,
    clean_focus_seconds_square_sum: f64,
    blocked_attempt_count_sum: f64,
    blocked_attempt_count_square_sum: f64,
    break_skipped_count_sum: f64,
    break_skipped_count_square_sum: f64,
    short_break_overtime_seconds_sum: f64,
    short_break_overtime_seconds_square_sum: f64,
    long_break_overtime_seconds_sum: f64,
    long_break_overtime_seconds_square_sum: f64,
    day_observed_count: i64,
    day_started_planned_pomodoro_count_sum: f64,
    day_missed_planned_pomodoro_count_sum: f64,
    day_missed_planned_pomodoro_count_square_sum: f64,
    day_clean_focus_seconds_sum: f64,
    day_blocked_attempt_count_sum: f64,
    day_blocked_attempt_count_square_sum: f64,
    next_day_observed_count: i64,
    next_day_started_run_count: i64,
    next_day_clean_focus_seconds_sum: f64,
    next_day_blocked_attempt_count_sum: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveReplayDatasetRead {
    opportunities: Vec<PomodoroAdaptiveReplayRunStartOpportunityRead>,
    outcomes: Vec<PomodoroAdaptiveReplayOutcomeRowRead>,
    histories: Vec<PomodoroAdaptiveReplayOpportunityHistoryRead>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveReplayRunStartOpportunityRead {
    id: String,
    run_id: String,
    started_at: String,
    planned_start: String,
    planned_end: String,
    candidate_id: Option<String>,
    current_rhythm: PomodoroRunRhythm,
    selected_rhythm: PomodoroRunRhythm,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveReplayOutcomeRowRead {
    opportunity_id: String,
    outcome_window: String,
    outcome_key: String,
    numeric_value: Option<f64>,
    boolean_value: Option<bool>,
    categorical_value: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroAdaptiveReplayOpportunityHistoryRead {
    opportunity_id: String,
    history: PomodoroAdaptiveHistoryRead,
}
