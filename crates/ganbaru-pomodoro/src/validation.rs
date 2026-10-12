use std::collections::HashSet;

use super::time::iso_seconds_between;
use super::*;

const MAX_FOCUS_DURATION_MINUTES: i64 = 120;
const MAX_SHORT_BREAK_DURATION_MINUTES: i64 = 30;
const MAX_LONG_BREAK_DURATION_MINUTES: i64 = 60;

pub(super) fn validate_run_adaptive_snapshot_for_segment(
    run: &PomodoroRunWrite,
    segment: &PomodoroSegmentWrite,
) -> Result<(), String> {
    let Some(snapshot) = &run.adaptive_snapshot else {
        return Ok(());
    };
    require_non_empty(&snapshot.policy_id, "adaptive_snapshot.policy_id")?;
    validate_positive_version(snapshot.policy_version, "adaptive_snapshot.policy_version")?;
    validate_positive_version(snapshot.model_version, "adaptive_snapshot.model_version")?;

    let context = &snapshot.context_snapshot;
    require_non_empty(&context.id, "adaptive_snapshot.context_snapshot.id")?;
    require_non_empty(&context.run_id, "adaptive_snapshot.context_snapshot.run_id")?;
    if context.run_id != run.id {
        return Err("adaptive context run_id must match run id".to_string());
    }
    if let Some(segment_id) = &context.segment_id {
        if segment_id != &segment.id {
            return Err("adaptive context segment_id must match initial segment id".to_string());
        }
    }
    require_non_empty(
        &context.local_started_at,
        "adaptive_snapshot.context_snapshot.local_started_at",
    )?;
    validate_adaptive_time_of_day(&context.time_of_day)?;
    validate_adaptive_session_position(&context.session_position)?;
    validate_adaptive_event_length(&context.event_length)?;
    validate_adaptive_workload(&context.workload)?;
    validate_adaptive_energy(&context.energy)?;
    validate_adaptive_features(&context.features)?;
    validate_adaptive_data_quality_flags(&context.data_quality_flags)?;

    let decision = &snapshot.decision;
    require_non_empty(&decision.id, "adaptive_snapshot.decision.id")?;
    require_non_empty(&decision.policy_id, "adaptive_snapshot.decision.policy_id")?;
    require_non_empty(&decision.run_id, "adaptive_snapshot.decision.run_id")?;
    if decision.policy_id != snapshot.policy_id {
        return Err("adaptive decision policy_id must match snapshot policy_id".to_string());
    }
    if decision.run_id != run.id {
        return Err("adaptive decision run_id must match run id".to_string());
    }
    if let Some(segment_id) = &decision.segment_id {
        if segment_id != &segment.id {
            return Err("adaptive decision segment_id must match initial segment id".to_string());
        }
    }
    if decision.context_snapshot_id != context.id {
        return Err(
            "adaptive decision context_snapshot_id must match context snapshot id".to_string(),
        );
    }
    validate_optional_adaptive_candidate_id(&decision.candidate_id)?;
    validate_adaptive_opportunity_kind(&decision.opportunity_kind)?;
    validate_adaptive_decision_mode(&decision.decision_mode)?;
    validate_positive_version(
        decision.policy_version,
        "adaptive_snapshot.decision.policy_version",
    )?;
    validate_positive_version(
        decision.model_version,
        "adaptive_snapshot.decision.model_version",
    )?;
    if decision.policy_version != snapshot.policy_version {
        return Err(
            "adaptive decision policy_version must match snapshot policy_version".to_string(),
        );
    }
    if decision.model_version != snapshot.model_version {
        return Err(
            "adaptive decision model_version must match snapshot model_version".to_string(),
        );
    }
    require_non_empty(
        &decision.occurred_at,
        "adaptive_snapshot.decision.occurred_at",
    )?;
    validate_adaptive_decision_values(&decision.values)?;
    validate_adaptive_reason_codes(&decision.reason_codes)?;
    validate_adaptive_state_scores(&decision.state_scores)?;
    validate_adaptive_planned_blocks(&snapshot.planned_blocks, run)?;
    validate_adaptive_experiment_updates_for_policy(
        &snapshot.experiment_updates,
        &snapshot.policy_id,
    )?;
    validate_adaptive_experiment_assignments_for_context(
        &snapshot.experiment_assignments,
        &snapshot.policy_id,
        &context.id,
        &run.id,
        Some(&segment.id),
    )
}

pub(super) fn validate_adaptive_decision_envelope_for_segment(
    envelope: &PomodoroAdaptiveDecisionEnvelopeWrite,
    segment: &PomodoroSegmentWrite,
) -> Result<(), String> {
    require_non_empty(&envelope.policy_id, "adaptive_decision.policy_id")?;
    validate_positive_version(envelope.policy_version, "adaptive_decision.policy_version")?;
    validate_positive_version(envelope.model_version, "adaptive_decision.model_version")?;

    let context = &envelope.context_snapshot;
    require_non_empty(&context.id, "adaptive_decision.context_snapshot.id")?;
    require_non_empty(&context.run_id, "adaptive_decision.context_snapshot.run_id")?;
    if context.run_id != segment.run_id {
        return Err("adaptive context run_id must match segment run_id".to_string());
    }
    if let Some(segment_id) = &context.segment_id {
        if segment_id != &segment.id {
            return Err("adaptive context segment_id must match segment id".to_string());
        }
    }
    require_non_empty(
        &context.local_started_at,
        "adaptive_decision.context_snapshot.local_started_at",
    )?;
    validate_adaptive_time_of_day(&context.time_of_day)?;
    validate_adaptive_session_position(&context.session_position)?;
    validate_adaptive_event_length(&context.event_length)?;
    validate_adaptive_workload(&context.workload)?;
    validate_adaptive_energy(&context.energy)?;
    validate_adaptive_features(&context.features)?;
    validate_adaptive_data_quality_flags(&context.data_quality_flags)?;

    let decision = &envelope.decision;
    require_non_empty(&decision.id, "adaptive_decision.decision.id")?;
    require_non_empty(&decision.policy_id, "adaptive_decision.decision.policy_id")?;
    require_non_empty(&decision.run_id, "adaptive_decision.decision.run_id")?;
    if decision.policy_id != envelope.policy_id {
        return Err("adaptive decision policy_id must match envelope policy_id".to_string());
    }
    if decision.run_id != segment.run_id {
        return Err("adaptive decision run_id must match segment run_id".to_string());
    }
    if let Some(segment_id) = &decision.segment_id {
        if segment_id != &segment.id {
            return Err("adaptive decision segment_id must match segment id".to_string());
        }
    }
    if decision.context_snapshot_id != context.id {
        return Err(
            "adaptive decision context_snapshot_id must match context snapshot id".to_string(),
        );
    }
    validate_optional_adaptive_candidate_id(&decision.candidate_id)?;
    validate_adaptive_opportunity_kind(&decision.opportunity_kind)?;
    match decision.opportunity_kind.as_str() {
        "focus_start" if segment.phase == "focus" => {}
        "break_start" if segment.phase == "short_break" || segment.phase == "long_break" => {}
        _ => {
            return Err(
                "adaptive boundary opportunity must match inserted segment phase".to_string(),
            );
        }
    }
    validate_adaptive_decision_mode(&decision.decision_mode)?;
    validate_positive_version(
        decision.policy_version,
        "adaptive_decision.decision.policy_version",
    )?;
    validate_positive_version(
        decision.model_version,
        "adaptive_decision.decision.model_version",
    )?;
    if decision.policy_version != envelope.policy_version {
        return Err(
            "adaptive decision policy_version must match envelope policy_version".to_string(),
        );
    }
    if decision.model_version != envelope.model_version {
        return Err(
            "adaptive decision model_version must match envelope model_version".to_string(),
        );
    }
    require_non_empty(
        &decision.occurred_at,
        "adaptive_decision.decision.occurred_at",
    )?;
    validate_adaptive_decision_values(&decision.values)?;
    validate_adaptive_reason_codes(&decision.reason_codes)?;
    validate_adaptive_state_scores(&decision.state_scores)?;
    validate_adaptive_experiment_updates_for_policy(
        &envelope.experiment_updates,
        &envelope.policy_id,
    )?;
    validate_adaptive_experiment_assignments_for_context(
        &envelope.experiment_assignments,
        &envelope.policy_id,
        &context.id,
        &segment.run_id,
        Some(&segment.id),
    )
}

fn validate_adaptive_experiment_updates_for_policy(
    experiments: &[PomodoroAdaptiveExperimentWrite],
    policy_id: &str,
) -> Result<(), String> {
    let mut seen = HashSet::new();
    for experiment in experiments {
        validate_adaptive_experiment(experiment, policy_id)?;
        if !seen.insert(experiment.id.as_str()) {
            return Err("adaptive experiment updated more than once".to_string());
        }
    }
    Ok(())
}

fn validate_adaptive_experiment_assignments_for_context(
    assignments: &[PomodoroAdaptiveExperimentAssignmentWrite],
    policy_id: &str,
    context_snapshot_id: &str,
    run_id: &str,
    segment_id: Option<&str>,
) -> Result<(), String> {
    let mut seen = HashSet::new();
    for envelope in assignments {
        validate_adaptive_experiment_assignment(
            envelope,
            policy_id,
            context_snapshot_id,
            run_id,
            segment_id,
        )?;
        if !seen.insert(envelope.assignment.experiment_id.as_str()) {
            return Err("adaptive experiment assigned more than once".to_string());
        }
    }
    Ok(())
}

fn validate_adaptive_experiment_assignment(
    envelope: &PomodoroAdaptiveExperimentAssignmentWrite,
    policy_id: &str,
    context_snapshot_id: &str,
    run_id: &str,
    segment_id: Option<&str>,
) -> Result<(), String> {
    let experiment = &envelope.experiment;
    let assignment = &envelope.assignment;
    validate_adaptive_experiment(experiment, policy_id)?;
    require_non_empty(&assignment.id, "adaptive_assignment.id")?;
    require_non_empty(
        &assignment.experiment_id,
        "adaptive_assignment.experiment_id",
    )?;
    require_non_empty(&assignment.variant_key, "adaptive_assignment.variant_key")?;
    require_non_empty(&assignment.run_id, "adaptive_assignment.run_id")?;
    require_non_empty(
        &assignment.context_snapshot_id,
        "adaptive_assignment.context_snapshot_id",
    )?;
    require_non_empty(
        &assignment.assignment_seed,
        "adaptive_assignment.assignment_seed",
    )?;
    require_non_empty(&assignment.assigned_at, "adaptive_assignment.assigned_at")?;
    if assignment.experiment_id != experiment.id {
        return Err("adaptive assignment experiment_id must match experiment id".to_string());
    }
    if assignment.run_id != run_id {
        return Err("adaptive assignment run_id must match run id".to_string());
    }
    if assignment.context_snapshot_id != context_snapshot_id {
        return Err(
            "adaptive assignment context_snapshot_id must match context snapshot id".to_string(),
        );
    }
    if assignment.segment_id.as_deref() != segment_id {
        return Err("adaptive assignment segment_id must match segment id".to_string());
    }
    if !experiment
        .variants
        .iter()
        .any(|variant| variant.variant_key == assignment.variant_key)
    {
        return Err("adaptive assignment variant_key must exist in experiment".to_string());
    }
    Ok(())
}

fn validate_adaptive_experiment(
    experiment: &PomodoroAdaptiveExperimentWrite,
    policy_id: &str,
) -> Result<(), String> {
    require_non_empty(&experiment.id, "adaptive_experiment.id")?;
    require_non_empty(&experiment.policy_id, "adaptive_experiment.policy_id")?;
    if experiment.policy_id != policy_id {
        return Err("adaptive experiment policy_id must match policy id".to_string());
    }
    validate_adaptive_experiment_parameter_key(&experiment.parameter_key)?;
    validate_adaptive_assignment_unit(&experiment.assignment_unit)?;
    validate_adaptive_experiment_status(&experiment.status)?;
    if let Some(started_at) = &experiment.started_at {
        require_non_empty(started_at, "adaptive_experiment.started_at")?;
    }
    if let Some(ended_at) = &experiment.ended_at {
        require_non_empty(ended_at, "adaptive_experiment.ended_at")?;
    }
    let terminal_status = experiment.status == "completed" || experiment.status == "abandoned";
    if terminal_status && experiment.ended_at.is_none() {
        return Err("terminal adaptive experiment status needs ended_at".to_string());
    }
    if !terminal_status && experiment.ended_at.is_some() {
        return Err("non-terminal adaptive experiment status cannot have ended_at".to_string());
    }
    if experiment.variants.len() < 2 {
        return Err("adaptive experiment must include at least two variants".to_string());
    }
    let mut control_count = 0;
    let mut variant_keys = HashSet::new();
    for variant in &experiment.variants {
        require_non_empty(
            &variant.variant_key,
            "adaptive_experiment_variant.variant_key",
        )?;
        if !variant.numeric_value.is_finite() {
            return Err("adaptive experiment variant numeric_value must be finite".to_string());
        }
        if variant.is_control {
            control_count += 1;
        }
        if !variant_keys.insert(variant.variant_key.as_str()) {
            return Err("adaptive experiment variants must be unique".to_string());
        }
    }
    if control_count != 1 {
        return Err("adaptive experiment must include exactly one control variant".to_string());
    }
    Ok(())
}

pub(super) fn validate_phase(phase: &str) -> Result<(), String> {
    match phase {
        "focus" | "short_break" | "long_break" => Ok(()),
        _ => Err(format!("invalid pomodoro segment phase: {phase}")),
    }
}

pub(super) fn validate_run_rhythm(rhythm: &PomodoroRunRhythm) -> Result<(), String> {
    match rhythm {
        PomodoroRunRhythm::Count {
            focus_duration_minutes,
            short_break_minutes,
            long_break_minutes,
            long_break_after_focus_count,
        } => {
            validate_config_minutes(
                *focus_duration_minutes,
                MAX_FOCUS_DURATION_MINUTES,
                "focus_duration_minutes",
            )?;
            validate_config_minutes(
                *short_break_minutes,
                MAX_SHORT_BREAK_DURATION_MINUTES,
                "short_break_minutes",
            )?;
            validate_config_minutes(
                *long_break_minutes,
                MAX_LONG_BREAK_DURATION_MINUTES,
                "long_break_minutes",
            )?;
            validate_rhythm_position_count(
                *long_break_after_focus_count,
                "long_break_after_focus_count",
            )
        }
        PomodoroRunRhythm::Sequence { steps } => {
            if steps.is_empty() {
                return Err("sequence rhythm must include at least one step".to_string());
            }
            validate_rhythm_position_count(steps.len() as i64, "sequence step count")?;
            for (index, step) in steps.iter().enumerate() {
                validate_config_minutes(
                    step.focus_duration_minutes,
                    MAX_FOCUS_DURATION_MINUTES,
                    &format!("sequence step {index} focus_duration_minutes"),
                )?;
                validate_phase(&step.break_phase)?;
                validate_config_minutes(
                    step.break_duration_minutes,
                    if step.break_phase == "long_break" {
                        MAX_LONG_BREAK_DURATION_MINUTES
                    } else {
                        MAX_SHORT_BREAK_DURATION_MINUTES
                    },
                    &format!("sequence step {index} break_duration_minutes"),
                )?;
            }
            Ok(())
        }
    }
}

fn validate_rhythm_position_count(value: i64, field: &str) -> Result<(), String> {
    if !(1..=12).contains(&value) {
        Err(format!("{field} must be between 1 and 12"))
    } else {
        Ok(())
    }
}

pub(super) fn validate_event_type(event_type: &str) -> Result<(), String> {
    match event_type {
        "start" | "phase_start" | "phase_complete" | "pause_start" | "pause_end"
        | "idle_detected" | "focus_failed" | "suspend_detected" | "skip_break" | "extend_focus"
        | "go_to_break_now" | "start_focus_now" | "reconfigure" | "block_transition" | "stop"
        | "complete" | "crash_recovery" => Ok(()),
        _ => Err(format!("invalid pomodoro run event type: {event_type}")),
    }
}

fn validate_positive_version(value: i64, field: &str) -> Result<(), String> {
    if value <= 0 {
        Err(format!("{field} must be positive"))
    } else {
        Ok(())
    }
}

fn validate_adaptive_time_of_day(value: &str) -> Result<(), String> {
    match value {
        "morning" | "midday" | "afternoon" | "evening" | "late" => Ok(()),
        _ => Err(format!("invalid adaptive time_of_day: {value}")),
    }
}

fn validate_adaptive_session_position(value: &str) -> Result<(), String> {
    match value {
        "first" | "middle" | "late" => Ok(()),
        _ => Err(format!("invalid adaptive session_position: {value}")),
    }
}

fn validate_adaptive_event_length(value: &str) -> Result<(), String> {
    match value {
        "short" | "medium" | "long" => Ok(()),
        _ => Err(format!("invalid adaptive event_length: {value}")),
    }
}

fn validate_adaptive_workload(value: &str) -> Result<(), String> {
    match value {
        "low" | "normal" | "high" => Ok(()),
        _ => Err(format!("invalid adaptive workload: {value}")),
    }
}

fn validate_adaptive_energy(value: &str) -> Result<(), String> {
    match value {
        "low" | "normal" | "high" | "unknown" => Ok(()),
        _ => Err(format!("invalid adaptive energy: {value}")),
    }
}

fn validate_adaptive_opportunity_kind(value: &str) -> Result<(), String> {
    match value {
        "run_start" | "focus_start" | "break_start" | "focus_tick" | "break_overtime"
        | "block_event" | "idle_failure" | "run_outcome" => Ok(()),
        _ => Err(format!("invalid adaptive opportunity_kind: {value}")),
    }
}

fn validate_adaptive_decision_mode(value: &str) -> Result<(), String> {
    match value {
        "fallback" | "hold" | "recovery" | "guardrail" | "exploit" | "explore" => Ok(()),
        _ => Err(format!("invalid adaptive decision_mode: {value}")),
    }
}

fn validate_adaptive_feature_source(value: &str) -> Result<(), String> {
    match value {
        "pomodoro" | "distractions" | "calendar" | "diary" | "project" | "environment"
        | "device" => Ok(()),
        _ => Err(format!("invalid adaptive feature source_kind: {value}")),
    }
}

fn validate_adaptive_planned_block_source(value: &str) -> Result<(), String> {
    match value {
        "live_event" | "archived_event" | "scheduler_snapshot" => Ok(()),
        _ => Err(format!(
            "invalid adaptive planned block source_kind: {value}"
        )),
    }
}

fn validate_adaptive_data_quality_flag(value: &str) -> Result<(), String> {
    match value {
        "extension_unavailable"
        | "desktop_tracking_unavailable"
        | "diary_missing"
        | "idle_detection_disabled"
        | "crash_recovered"
        | "calendar_clipped" => Ok(()),
        _ => Err(format!("invalid adaptive data quality flag: {value}")),
    }
}

fn validate_adaptive_parameter_key(value: &str) -> Result<(), String> {
    match value {
        "focus_duration_minutes"
        | "short_break_minutes"
        | "long_break_minutes"
        | "long_break_after_focus_count" => Ok(()),
        _ => Err(format!("invalid adaptive value_key: {value}")),
    }
}

fn validate_adaptive_experiment_parameter_key(value: &str) -> Result<(), String> {
    match value {
        "rhythm_bundle" => Ok(()),
        _ => validate_adaptive_parameter_key(value)
            .map_err(|_| format!("invalid adaptive experiment parameter_key: {value}")),
    }
}

fn validate_adaptive_value_unit(value: &str) -> Result<(), String> {
    match value {
        "minutes" | "count" => Ok(()),
        _ => Err(format!("invalid adaptive value_unit: {value}")),
    }
}

fn validate_adaptive_assignment_unit(value: &str) -> Result<(), String> {
    match value {
        "phase" | "run" | "day" | "context" => Ok(()),
        _ => Err(format!("invalid adaptive assignment_unit: {value}")),
    }
}

fn validate_adaptive_experiment_status(value: &str) -> Result<(), String> {
    match value {
        "draft" | "active" | "paused" | "completed" | "abandoned" => Ok(()),
        _ => Err(format!("invalid adaptive experiment status: {value}")),
    }
}

fn validate_adaptive_reason_code(value: &str) -> Result<(), String> {
    match value {
        "no_history"
        | "low_confidence"
        | "missing_extension_data"
        | "missing_diary_data"
        | "high_strain"
        | "high_avoidance_pressure"
        | "high_recovery_debt"
        | "clean_momentum"
        | "break_return_drift"
        | "break_transition_pressure"
        | "skipped_break_recovery"
        | "focus_idle_pressure"
        | "repeated_blocked_source_pressure"
        | "capacity_rebuild"
        | "experiment_assignment"
        | "experiment_guardrail"
        | "guardrail_recovery"
        | "replay_candidate"
        | "hold_current_rhythm" => Ok(()),
        _ => Err(format!("invalid adaptive reason_code: {value}")),
    }
}

fn validate_optional_adaptive_candidate_id(value: &Option<String>) -> Result<(), String> {
    if let Some(candidate_id) = value {
        require_non_empty(candidate_id, "adaptive decision candidate_id")?;
    }
    Ok(())
}

fn validate_adaptive_features(features: &[PomodoroAdaptiveFeatureWrite]) -> Result<(), String> {
    let mut keys = HashSet::new();
    for feature in features {
        require_non_empty(&feature.feature_key, "adaptive feature feature_key")?;
        if !keys.insert(feature.feature_key.as_str()) {
            return Err(format!(
                "duplicate adaptive feature: {}",
                feature.feature_key
            ));
        }
        if let Some(value) = feature.numeric_value {
            validate_finite(value, "adaptive feature numeric_value")?;
        }
        if !feature.missing
            && feature.numeric_value.is_none()
            && feature.categorical_value.is_none()
            && feature.boolean_value.is_none()
        {
            return Err(format!(
                "adaptive feature {} needs a value",
                feature.feature_key
            ));
        }
        validate_adaptive_feature_source(&feature.source_kind)?;
    }
    Ok(())
}

fn validate_adaptive_data_quality_flags(flags: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for flag in flags {
        validate_adaptive_data_quality_flag(flag)?;
        if !seen.insert(flag.as_str()) {
            return Err(format!("duplicate adaptive data quality flag: {flag}"));
        }
    }
    Ok(())
}

fn validate_adaptive_planned_blocks(
    blocks: &[PomodoroAdaptivePlannedBlockWrite],
    run: &PomodoroRunWrite,
) -> Result<(), String> {
    if blocks.len() > MAX_ADAPTIVE_PLANNED_BLOCKS_PER_SNAPSHOT {
        return Err(format!(
            "adaptive planned block snapshot exceeds {} rows",
            MAX_ADAPTIVE_PLANNED_BLOCKS_PER_SNAPSHOT
        ));
    }
    let mut seen = HashSet::new();
    for block in blocks {
        require_non_empty(&block.event_date, "adaptive planned block event_date")?;
        if block.event_date != run.event_date {
            return Err("adaptive planned block event_date must match run event_date".to_string());
        }
        if let Some(event_id) = &block.event_id {
            require_non_empty(event_id, "adaptive planned block event_id")?;
        }
        require_non_empty(
            &block.original_event_id,
            "adaptive planned block original_event_id",
        )?;
        require_non_empty(&block.planned_start, "adaptive planned block planned_start")?;
        require_non_empty(&block.planned_end, "adaptive planned block planned_end")?;
        let Some(duration_seconds) = iso_seconds_between(&block.planned_start, &block.planned_end)
        else {
            return Err("adaptive planned block timestamps must be valid instants".to_string());
        };
        if duration_seconds <= 0 {
            return Err(
                "adaptive planned block planned_end must be after planned_start".to_string(),
            );
        }
        validate_adaptive_planned_block_source(&block.source_kind)?;
        let key = format!("{}|{}", block.original_event_id, block.planned_start);
        if !seen.insert(key) {
            return Err(format!(
                "duplicate adaptive planned block: {} at {}",
                block.original_event_id, block.planned_start
            ));
        }
    }
    Ok(())
}

fn validate_adaptive_decision_values(
    values: &[PomodoroAdaptiveDecisionValueWrite],
) -> Result<(), String> {
    if values.is_empty() {
        return Err("adaptive decision must include selected values".to_string());
    }
    let mut keys = HashSet::new();
    for value in values {
        validate_adaptive_parameter_key(&value.value_key)?;
        if !keys.insert(value.value_key.as_str()) {
            return Err(format!(
                "duplicate adaptive decision value: {}",
                value.value_key
            ));
        }
        if let Some(previous) = value.previous_numeric_value {
            validate_finite(previous, "adaptive decision previous_numeric_value")?;
        }
        validate_finite(
            value.selected_numeric_value,
            "adaptive decision selected_numeric_value",
        )?;
        validate_adaptive_value_unit(&value.value_unit)?;
    }
    Ok(())
}

fn validate_adaptive_reason_codes(reason_codes: &[String]) -> Result<(), String> {
    if reason_codes.is_empty() {
        return Err("adaptive decision must include at least one reason_code".to_string());
    }
    let mut seen = HashSet::new();
    for reason_code in reason_codes {
        validate_adaptive_reason_code(reason_code)?;
        if !seen.insert(reason_code.as_str()) {
            return Err(format!("duplicate adaptive reason_code: {reason_code}"));
        }
    }
    Ok(())
}

fn validate_adaptive_state_scores(scores: &PomodoroAdaptiveStateScoresWrite) -> Result<(), String> {
    validate_score(scores.readiness, "adaptive state readiness")?;
    validate_score(scores.strain, "adaptive state strain")?;
    validate_score(scores.recovery_debt, "adaptive state recovery_debt")?;
    validate_score(
        scores.avoidance_pressure,
        "adaptive state avoidance_pressure",
    )?;
    validate_score(scores.momentum, "adaptive state momentum")?;
    validate_score(scores.confidence, "adaptive state confidence")
}

fn validate_score(value: f64, field: &str) -> Result<(), String> {
    validate_finite(value, field)?;
    if !(0.0..=1.0).contains(&value) {
        Err(format!("{field} must be between 0 and 1"))
    } else {
        Ok(())
    }
}

fn validate_finite(value: f64, field: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{field} must be finite"))
    }
}

pub(super) fn validate_pause(pause: &PomodoroPauseWrite) -> Result<(), String> {
    require_non_empty(&pause.started_at, "pause.started_at")?;
    validate_pause_reason(&pause.reason)
}

pub(super) fn validate_pause_reason(reason: &str) -> Result<(), String> {
    match reason {
        "idle" | "manual" | "suspend" => Ok(()),
        _ => Err(format!("invalid pomodoro pause reason: {reason}")),
    }
}

fn validate_config_minutes(value: i64, maximum: i64, field: &str) -> Result<(), String> {
    if !(1..=maximum).contains(&value) {
        Err(format!("{field} must be between 1 and {maximum}"))
    } else {
        Ok(())
    }
}

pub(super) fn canonical_event_id(value: &str) -> Result<&str, String> {
    require_non_empty(value, "event_id")?;
    let id = value.split_once("::").map_or(value, |(parent, _)| parent);
    require_non_empty(id, "event_id")?;
    Ok(id)
}

pub(super) fn require_non_empty(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} cannot be empty"))
    } else {
        Ok(())
    }
}
