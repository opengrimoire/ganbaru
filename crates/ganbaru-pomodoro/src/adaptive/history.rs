//! Explicit conversion from bounded canonical reads into policy evidence.

use crate::PomodoroAdaptiveHistoryRead;
use crate::adaptive::decision::{AdaptiveHistory, ContextState};
use crate::adaptive::experiments::models::{
    AssignmentHistory, ExperimentOutcome, ExperimentState, OutcomeValues,
};
use crate::adaptive::models::{
    BlockEventInput, PauseInput, RunEventInput, SegmentInput, StateScores,
};

impl From<PomodoroAdaptiveHistoryRead> for AdaptiveHistory {
    fn from(history: PomodoroAdaptiveHistoryRead) -> Self {
        Self {
            segments: history
                .segments
                .into_iter()
                .map(|value| SegmentInput {
                    run_id: Some(value.run_id),
                    rhythm_position: Some(value.rhythm_position),
                    phase: value.phase,
                    planned_start: value.planned_start,
                    planned_end: value.planned_end,
                    actual_start: value.actual_start,
                    actual_end: value.actual_end,
                    status: value.status,
                    end_reason: value.end_reason,
                    pause_log: value
                        .pause_log
                        .into_iter()
                        .map(|pause| PauseInput {
                            started_at: pause.started_at,
                            ended_at: pause.ended_at,
                            reason: pause.reason,
                        })
                        .collect(),
                })
                .collect(),
            run_events: history
                .run_events
                .into_iter()
                .map(|value| RunEventInput {
                    event_type: value.event_type,
                    occurred_at: value.occurred_at,
                    phase: value.phase,
                    reason: value.reason,
                    duration_seconds: value.duration_seconds,
                })
                .collect(),
            block_events: history
                .block_events
                .into_iter()
                .map(|value| BlockEventInput {
                    occurred_at: value.occurred_at,
                    phase: value.phase,
                    source_type: value.source_type,
                    source_key: value.source_key,
                    decision: value.decision,
                })
                .collect(),
            previous_states: history
                .previous_states
                .into_iter()
                .map(|value| ContextState {
                    context_key: value.context_key,
                    scores: StateScores {
                        readiness: value.readiness,
                        strain: value.strain,
                        recovery_debt: value.recovery_debt,
                        avoidance_pressure: value.avoidance_pressure,
                        momentum: value.momentum,
                        confidence: value.confidence,
                    },
                })
                .collect(),
            experiment_states: history
                .experiment_states
                .into_iter()
                .map(|value| ExperimentState {
                    experiment_id: value.experiment_id,
                    status: value.status,
                    started_at: value.started_at,
                    ended_at: value.ended_at,
                })
                .collect(),
            experiment_assignments: history
                .experiment_assignments
                .into_iter()
                .map(|value| AssignmentHistory {
                    experiment_id: value.experiment_id,
                    variant_key: value.variant_key,
                    context_key: value.context_key,
                    assigned_at: value.assigned_at,
                })
                .collect(),
            experiment_outcomes: history
                .experiment_outcomes
                .into_iter()
                .map(|value| ExperimentOutcome {
                    experiment_id: value.experiment_id,
                    variant_key: value.variant_key,
                    context_key: Some(value.context_key),
                    values: OutcomeValues {
                        assignment_count: value.assignment_count as f64,
                        run_observed_count: value.run_observed_count as f64,
                        run_completed_count: value.run_completed_count as f64,
                        run_stopped_count: value.run_stopped_count as f64,
                        clean_focus_seconds_sum: value.clean_focus_seconds_sum,
                        clean_focus_seconds_square_sum: value.clean_focus_seconds_square_sum,
                        blocked_attempt_count_sum: value.blocked_attempt_count_sum,
                        blocked_attempt_count_square_sum: value.blocked_attempt_count_square_sum,
                        break_skipped_count_sum: value.break_skipped_count_sum,
                        break_skipped_count_square_sum: value.break_skipped_count_square_sum,
                        short_break_overtime_seconds_sum: value.short_break_overtime_seconds_sum,
                        short_break_overtime_seconds_square_sum: value
                            .short_break_overtime_seconds_square_sum,
                        long_break_overtime_seconds_sum: value.long_break_overtime_seconds_sum,
                        long_break_overtime_seconds_square_sum: value
                            .long_break_overtime_seconds_square_sum,
                        day_observed_count: value.day_observed_count as f64,
                        day_started_planned_pomodoro_count_sum: value
                            .day_started_planned_pomodoro_count_sum,
                        day_missed_planned_pomodoro_count_sum: value
                            .day_missed_planned_pomodoro_count_sum,
                        day_missed_planned_pomodoro_count_square_sum: value
                            .day_missed_planned_pomodoro_count_square_sum,
                        day_clean_focus_seconds_sum: value.day_clean_focus_seconds_sum,
                        day_blocked_attempt_count_sum: value.day_blocked_attempt_count_sum,
                        day_blocked_attempt_count_square_sum: value
                            .day_blocked_attempt_count_square_sum,
                        next_day_observed_count: value.next_day_observed_count as f64,
                        next_day_started_run_count: value.next_day_started_run_count as f64,
                        next_day_clean_focus_seconds_sum: value.next_day_clean_focus_seconds_sum,
                        next_day_blocked_attempt_count_sum: value
                            .next_day_blocked_attempt_count_sum,
                    },
                })
                .collect(),
        }
    }
}
