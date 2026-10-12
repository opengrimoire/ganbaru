//! Native persistence envelopes for adaptive features, values, and decisions.

use crate::adaptive::decision::{AdaptiveDecision, ExperimentUpdate};
use crate::adaptive::experiments::models::ExperimentAssignment;
use crate::adaptive::models::{CountRhythm, FeatureVector};
use crate::*;

/// Identities are allocated by the accepted transaction, never by the WebView.
pub(crate) struct SnapshotIds {
    pub run: String,
    pub segment: String,
    pub context: String,
    pub decision: String,
    pub assignment: String,
}

pub(crate) fn run_start_snapshot(
    decision: &AdaptiveDecision,
    ids: SnapshotIds,
    planned_blocks: Vec<PomodoroAdaptivePlannedBlockWrite>,
) -> PomodoroRunAdaptiveSnapshotWrite {
    let assignments = decision
        .experiment_assignment
        .as_ref()
        .map(|assignment| assignment_write(assignment, &ids, &decision.policy_id))
        .into_iter()
        .collect();
    let envelope = decision_envelope(decision, ids, "run_start");
    PomodoroRunAdaptiveSnapshotWrite {
        policy_id: envelope.policy_id,
        policy_version: envelope.policy_version,
        model_version: envelope.model_version,
        context_snapshot: envelope.context_snapshot,
        decision: envelope.decision,
        planned_blocks,
        experiment_updates: envelope.experiment_updates,
        experiment_assignments: assignments,
    }
}

pub(crate) fn decision_envelope(
    decision: &AdaptiveDecision,
    ids: SnapshotIds,
    opportunity: &str,
) -> PomodoroAdaptiveDecisionEnvelopeWrite {
    let context = &decision.context;
    let scores = decision.state_scores;
    PomodoroAdaptiveDecisionEnvelopeWrite {
        policy_id: decision.policy_id.clone(),
        policy_version: decision.policy_version,
        model_version: decision.model_version,
        context_snapshot: PomodoroAdaptiveContextSnapshotWrite {
            id: ids.context.clone(),
            run_id: ids.run.clone(),
            segment_id: Some(ids.segment.clone()),
            local_started_at: decision.occurred_at.clone(),
            time_of_day: context.time_of_day.clone(),
            session_position: context.session_position.clone(),
            event_length: context.event_length.clone(),
            workload: context.workload.clone(),
            energy: context.energy.clone(),
            environment_id: context.environment_id.clone(),
            features: feature_writes(&decision.features),
            data_quality_flags: decision.features.data_quality_flags.clone(),
        },
        decision: PomodoroAdaptiveDecisionWrite {
            id: ids.decision,
            policy_id: decision.policy_id.clone(),
            run_id: ids.run,
            segment_id: Some(ids.segment),
            context_snapshot_id: ids.context,
            opportunity_kind: opportunity.to_owned(),
            candidate_id: decision.candidate_id.clone(),
            decision_mode: decision.decision_mode.clone(),
            policy_version: decision.policy_version,
            model_version: decision.model_version,
            occurred_at: decision.occurred_at.clone(),
            values: value_writes(decision.current_rhythm, decision.selected_rhythm),
            reason_codes: decision.reason_codes.clone(),
            state_scores: PomodoroAdaptiveStateScoresWrite {
                readiness: scores.readiness,
                strain: scores.strain,
                recovery_debt: scores.recovery_debt,
                avoidance_pressure: scores.avoidance_pressure,
                momentum: scores.momentum,
                confidence: scores.confidence,
            },
        },
        experiment_updates: decision
            .experiment_update
            .as_ref()
            .map(experiment_write)
            .into_iter()
            .collect(),
        experiment_assignments: Vec::new(),
    }
}

fn experiment_write(update: &ExperimentUpdate) -> PomodoroAdaptiveExperimentWrite {
    PomodoroAdaptiveExperimentWrite {
        id: update.id.clone(),
        policy_id: update.policy_id.clone(),
        parameter_key: update.parameter_key.clone(),
        assignment_unit: update.assignment_unit.clone(),
        status: update.status.clone(),
        started_at: update.started_at.clone(),
        ended_at: update.ended_at.clone(),
        variants: update
            .variants
            .iter()
            .map(|variant| PomodoroAdaptiveExperimentVariantWrite {
                variant_key: variant.variant_key.clone(),
                numeric_value: variant.numeric_value,
                is_control: variant.is_control,
            })
            .collect(),
    }
}

fn assignment_write(
    assignment: &ExperimentAssignment,
    ids: &SnapshotIds,
    policy_id: &str,
) -> PomodoroAdaptiveExperimentAssignmentWrite {
    let definition = &assignment.experiment;
    PomodoroAdaptiveExperimentAssignmentWrite {
        experiment: PomodoroAdaptiveExperimentWrite {
            id: definition.id.clone(),
            policy_id: policy_id.to_owned(),
            parameter_key: definition.parameter_key.clone(),
            assignment_unit: definition.assignment_unit.clone(),
            status: definition.status.clone(),
            started_at: Some(assignment.assigned_at.clone()),
            ended_at: matches!(definition.status.as_str(), "completed" | "abandoned")
                .then(|| assignment.assigned_at.clone()),
            variants: definition
                .variants
                .iter()
                .map(|variant| PomodoroAdaptiveExperimentVariantWrite {
                    variant_key: variant.variant_key.clone(),
                    numeric_value: variant.numeric_value,
                    is_control: variant.is_control,
                })
                .collect(),
        },
        assignment: PomodoroAdaptiveAssignmentWrite {
            id: ids.assignment.clone(),
            experiment_id: definition.id.clone(),
            variant_key: assignment.variant.variant_key.clone(),
            run_id: ids.run.clone(),
            segment_id: Some(ids.segment.clone()),
            context_snapshot_id: ids.context.clone(),
            assignment_seed: assignment.assignment_seed.clone(),
            assigned_at: assignment.assigned_at.clone(),
        },
    }
}

fn value_writes(
    previous: CountRhythm,
    selected: CountRhythm,
) -> Vec<PomodoroAdaptiveDecisionValueWrite> {
    [
        (
            "focus_duration_minutes",
            previous.focus_duration_minutes,
            selected.focus_duration_minutes,
            "minutes",
        ),
        (
            "short_break_minutes",
            previous.short_break_minutes,
            selected.short_break_minutes,
            "minutes",
        ),
        (
            "long_break_minutes",
            previous.long_break_minutes,
            selected.long_break_minutes,
            "minutes",
        ),
        (
            "long_break_after_focus_count",
            previous.long_break_after_focus_count,
            selected.long_break_after_focus_count,
            "count",
        ),
    ]
    .into_iter()
    .map(
        |(key, prior, value, unit)| PomodoroAdaptiveDecisionValueWrite {
            value_key: key.to_owned(),
            previous_numeric_value: Some(prior as f64),
            selected_numeric_value: value as f64,
            value_unit: unit.to_owned(),
        },
    )
    .collect()
}

fn feature_writes(features: &FeatureVector) -> Vec<PomodoroAdaptiveFeatureWrite> {
    let pomodoro = [
        (
            "completed_focus_segments",
            features.completed_focus_segments,
        ),
        (
            "interrupted_focus_segments",
            features.interrupted_focus_segments,
        ),
        ("focus_failure_count", features.focus_failure_count),
        (
            "late_focus_segment_count",
            features.late_focus_segment_count,
        ),
        (
            "late_focus_failure_count",
            features.late_focus_failure_count,
        ),
        ("clean_focus_seconds", features.clean_focus_seconds),
        ("planned_focus_seconds", features.planned_focus_seconds),
        ("idle_pause_count", features.idle_pause_count),
        ("idle_pause_seconds", features.idle_pause_seconds),
        ("focus_idle_pause_count", features.focus_idle_pause_count),
        (
            "focus_idle_pause_seconds",
            features.focus_idle_pause_seconds,
        ),
        (
            "early_focus_idle_pause_count",
            features.early_focus_idle_pause_count,
        ),
        (
            "late_focus_idle_pause_count",
            features.late_focus_idle_pause_count,
        ),
        ("manual_pause_count", features.manual_pause_count),
        ("manual_pause_seconds", features.manual_pause_seconds),
        ("suspend_pause_count", features.suspend_pause_count),
        ("suspend_pause_seconds", features.suspend_pause_seconds),
        ("break_started_count", features.break_started_count),
        ("break_completed_count", features.break_completed_count),
        ("break_skipped_count", features.break_skipped_count),
        (
            "skipped_break_next_focus_success_count",
            features.skipped_break_next_focus_success_count,
        ),
        (
            "skipped_break_next_focus_failure_count",
            features.skipped_break_next_focus_failure_count,
        ),
        (
            "skipped_short_break_next_focus_failure_count",
            features.skipped_short_break_next_focus_failure_count,
        ),
        (
            "skipped_long_break_next_focus_failure_count",
            features.skipped_long_break_next_focus_failure_count,
        ),
        (
            "short_break_overtime_seconds",
            features.short_break_overtime_seconds,
        ),
        (
            "long_break_overtime_seconds",
            features.long_break_overtime_seconds,
        ),
    ];
    let blocking = [
        ("blocked_attempt_count", features.blocked_attempt_count),
        ("blocked_burst_count", features.blocked_burst_count),
        (
            "repeated_blocked_source_attempt_count",
            features.repeated_blocked_source_attempt_count,
        ),
        (
            "focus_repeated_blocked_source_attempt_count",
            features.focus_repeated_blocked_source_attempt_count,
        ),
        (
            "break_repeated_blocked_source_attempt_count",
            features.break_repeated_blocked_source_attempt_count,
        ),
        (
            "focus_blocked_attempt_count",
            features.focus_blocked_attempt_count,
        ),
        (
            "early_focus_blocked_attempt_count",
            features.early_focus_blocked_attempt_count,
        ),
        (
            "late_focus_blocked_attempt_count",
            features.late_focus_blocked_attempt_count,
        ),
        (
            "break_blocked_attempt_count",
            features.break_blocked_attempt_count,
        ),
        (
            "break_overtime_blocked_attempt_count",
            features.break_overtime_blocked_attempt_count,
        ),
        (
            "short_break_overtime_blocked_attempt_count",
            features.short_break_overtime_blocked_attempt_count,
        ),
        (
            "long_break_overtime_blocked_attempt_count",
            features.long_break_overtime_blocked_attempt_count,
        ),
    ];
    let controls = [
        ("extension_count", features.extension_count),
        ("go_to_break_now_count", features.go_to_break_now_count),
        (
            "early_go_to_break_now_count",
            features.early_go_to_break_now_count,
        ),
        (
            "late_go_to_break_now_count",
            features.late_go_to_break_now_count,
        ),
        ("start_focus_now_count", features.start_focus_now_count),
        (
            "start_focus_now_success_count",
            features.start_focus_now_success_count,
        ),
        (
            "start_focus_now_failure_count",
            features.start_focus_now_failure_count,
        ),
        ("stop_count", features.stop_count),
        (
            "comparable_opportunity_count",
            features.comparable_opportunity_count,
        ),
    ];
    pomodoro
        .into_iter()
        .map(|(key, value)| numeric_feature(key, value, "pomodoro"))
        .chain(
            blocking
                .into_iter()
                .map(|(key, value)| numeric_feature(key, value, "distractions")),
        )
        .chain(
            controls
                .into_iter()
                .map(|(key, value)| numeric_feature(key, value, "pomodoro")),
        )
        .collect()
}

fn numeric_feature(key: &str, value: f64, source: &str) -> PomodoroAdaptiveFeatureWrite {
    PomodoroAdaptiveFeatureWrite {
        feature_key: key.to_owned(),
        numeric_value: Some(value),
        categorical_value: None,
        boolean_value: None,
        missing: false,
        source_kind: source.to_owned(),
    }
}
