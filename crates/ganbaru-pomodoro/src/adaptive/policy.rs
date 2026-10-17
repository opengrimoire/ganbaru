use crate::adaptive::models::{
    ContextBucket, CountRhythm, FeatureVector, PolicyDecision, StateScores,
};
use crate::adaptive::state::reason_codes_for_state;

const LOW_CONFIDENCE: f64 = 0.25;
const MODERATE_CONFIDENCE: f64 = 0.5;
const HIGH_PRESSURE: f64 = 0.58;
const LOW_RISK: f64 = 0.35;
const HIGH_MOMENTUM: f64 = 0.68;
const SHORT_BREAK_DRIFT_SECONDS: f64 = 120.0;
const LONG_BREAK_DRIFT_SECONDS: f64 = 300.0;
const FOCUS_STEP: i64 = 5;
const FOCUS_MINIMUM: i64 = 15;
const NORMAL_FOCUS_MINIMUM: i64 = 25;
const FOCUS_MAXIMUM: i64 = 60;
const LONG_BREAK_STEP: i64 = 5;
const LONG_BREAK_MINIMUM: i64 = 10;
const LONG_BREAK_MAXIMUM: i64 = 30;
const CADENCE_MINIMUM: i64 = 2;
const CADENCE_MAXIMUM: i64 = 5;

/// Choose the deterministic base rhythm before experiment and replay evidence.
/// Branch order is policy meaning.
pub fn select_adaptive_rhythm(
    current: CountRhythm,
    features: &FeatureVector,
    state: StateScores,
    _context: &ContextBucket,
) -> PolicyDecision {
    let mut selected = current;
    let mut reasons = reason_codes_for_state(state);
    if features
        .data_quality_flags
        .iter()
        .any(|flag| flag == "extension_unavailable")
    {
        push_unique(&mut reasons, "missing_extension_data");
    }
    if features
        .data_quality_flags
        .iter()
        .any(|flag| flag == "diary_missing")
    {
        push_unique(&mut reasons, "missing_diary_data");
    }
    if state.confidence < LOW_CONFIDENCE {
        push_unique(
            &mut reasons,
            if features.comparable_opportunity_count == 0.0 {
                "no_history"
            } else {
                "low_confidence"
            },
        );
        return decision(selected, "fallback", reasons, state);
    }
    if state.recovery_debt >= HIGH_PRESSURE
        || (state.strain >= HIGH_PRESSURE && features.focus_failure_count > 0.0)
    {
        selected.focus_duration_minutes = decrease_focus(
            selected.focus_duration_minutes,
            if features.focus_failure_count > 0.0 {
                FOCUS_MINIMUM
            } else {
                NORMAL_FOCUS_MINIMUM
            },
        );
        support_recovery(&mut selected);
        push_unique(&mut reasons, "guardrail_recovery");
        return decision(selected, "recovery", reasons, state);
    }
    if features.late_focus_segment_count >= 2.0 && features.late_focus_failure_count >= 2.0 {
        support_recovery(&mut selected);
        push_unique(&mut reasons, "guardrail_recovery");
        return decision(selected, "guardrail", reasons, state);
    }
    if features.skipped_long_break_next_focus_failure_count >= 1.0
        || features.skipped_break_next_focus_failure_count >= 2.0
    {
        support_recovery(&mut selected);
        push_unique(&mut reasons, "skipped_break_recovery");
        push_unique(&mut reasons, "guardrail_recovery");
        return decision(selected, "guardrail", reasons, state);
    }
    if features.late_focus_segment_count >= 2.0
        && (features.late_focus_failure_count == 1.0
            || (features.late_focus_blocked_attempt_count >= 3.0
                && features.late_focus_blocked_attempt_count
                    > features.early_focus_blocked_attempt_count
                && state.recovery_debt < LOW_RISK))
        && state.strain < HIGH_PRESSURE
        && state.recovery_debt < HIGH_PRESSURE
        && state.avoidance_pressure < HIGH_PRESSURE
    {
        selected.long_break_after_focus_count =
            (selected.long_break_after_focus_count - 1).clamp(CADENCE_MINIMUM, CADENCE_MAXIMUM);
        push_unique(&mut reasons, "guardrail_recovery");
        return decision(selected, "explore", reasons, state);
    }
    if features.early_focus_idle_pause_count >= 2.0
        && state.strain >= LOW_RISK
        && state.avoidance_pressure < HIGH_PRESSURE
    {
        selected.focus_duration_minutes =
            decrease_focus(selected.focus_duration_minutes, NORMAL_FOCUS_MINIMUM);
        push_unique(&mut reasons, "focus_idle_pressure");
        push_unique(&mut reasons, "guardrail_recovery");
        return decision(selected, "guardrail", reasons, state);
    }
    if features.late_focus_idle_pause_count >= 2.0 && state.recovery_debt >= LOW_RISK {
        selected.focus_duration_minutes =
            decrease_focus(selected.focus_duration_minutes, NORMAL_FOCUS_MINIMUM);
        push_unique(&mut reasons, "focus_idle_pressure");
        push_unique(&mut reasons, "guardrail_recovery");
        return decision(selected, "guardrail", reasons, state);
    }
    if features.early_go_to_break_now_count >= 2.0
        && state.strain >= LOW_RISK
        && state.avoidance_pressure < HIGH_PRESSURE
    {
        selected.focus_duration_minutes =
            decrease_focus(selected.focus_duration_minutes, NORMAL_FOCUS_MINIMUM);
        push_unique(&mut reasons, "guardrail_recovery");
        return decision(selected, "guardrail", reasons, state);
    }
    if features.late_go_to_break_now_count >= 2.0 && state.recovery_debt >= LOW_RISK {
        selected.focus_duration_minutes =
            decrease_focus(selected.focus_duration_minutes, NORMAL_FOCUS_MINIMUM);
        push_unique(&mut reasons, "guardrail_recovery");
        return decision(selected, "guardrail", reasons, state);
    }
    if features.late_focus_blocked_attempt_count >= 3.0
        && features.late_focus_blocked_attempt_count > features.early_focus_blocked_attempt_count
        && state.recovery_debt >= LOW_RISK
    {
        support_recovery(&mut selected);
        push_unique(&mut reasons, "guardrail_recovery");
        return decision(selected, "guardrail", reasons, state);
    }
    if state.avoidance_pressure >= HIGH_PRESSURE
        && state.strain < LOW_RISK
        && state.recovery_debt < LOW_RISK
        && features.focus_failure_count == 0.0
        && features.interrupted_focus_segments == 0.0
    {
        push_unique(&mut reasons, "hold_current_rhythm");
        return decision(selected, "hold", reasons, state);
    }
    if features.focus_repeated_blocked_source_attempt_count >= 3.0
        && state.strain < LOW_RISK
        && state.recovery_debt < LOW_RISK
        && features.focus_failure_count == 0.0
        && features.interrupted_focus_segments == 0.0
    {
        push_unique(&mut reasons, "repeated_blocked_source_pressure");
        push_unique(&mut reasons, "hold_current_rhythm");
        return decision(selected, "hold", reasons, state);
    }
    if state.strain >= HIGH_PRESSURE || state.avoidance_pressure >= HIGH_PRESSURE {
        selected.focus_duration_minutes =
            decrease_focus(selected.focus_duration_minutes, NORMAL_FOCUS_MINIMUM);
        if state.avoidance_pressure >= HIGH_PRESSURE && features.blocked_burst_count > 0.0 {
            selected.long_break_after_focus_count =
                (selected.long_break_after_focus_count - 1).clamp(CADENCE_MINIMUM, CADENCE_MAXIMUM);
        }
        return decision(selected, "guardrail", reasons, state);
    }
    if features.short_break_overtime_seconds >= SHORT_BREAK_DRIFT_SECONDS
        && features.short_break_overtime_blocked_attempt_count > 0.0
    {
        push_unique(&mut reasons, "break_transition_pressure");
        push_unique(&mut reasons, "hold_current_rhythm");
        return decision(selected, "hold", reasons, state);
    }
    if features.short_break_overtime_seconds >= SHORT_BREAK_DRIFT_SECONDS
        && state.avoidance_pressure < LOW_RISK
    {
        selected.short_break_minutes = (selected.short_break_minutes + 2).clamp(3, 12);
        push_unique(&mut reasons, "break_return_drift");
        return decision(selected, "explore", reasons, state);
    }
    if features.long_break_overtime_seconds >= LONG_BREAK_DRIFT_SECONDS
        && features.long_break_overtime_blocked_attempt_count == 0.0
        && state.avoidance_pressure < LOW_RISK
    {
        selected.long_break_minutes = (selected.long_break_minutes + LONG_BREAK_STEP)
            .clamp(LONG_BREAK_MINIMUM, LONG_BREAK_MAXIMUM);
        push_unique(&mut reasons, "break_return_drift");
        return decision(selected, "explore", reasons, state);
    }
    let ready_for_growth = state.confidence >= MODERATE_CONFIDENCE
        && state.momentum >= HIGH_MOMENTUM
        && state.strain < LOW_RISK
        && state.avoidance_pressure < LOW_RISK
        && state.recovery_debt < LOW_RISK;
    if ready_for_growth
        && features.completed_focus_segments >= 12.0
        && features.clean_focus_seconds >= features.planned_focus_seconds
        && features.interrupted_focus_segments == 0.0
        && features.focus_failure_count == 0.0
        && features.break_skipped_count == 0.0
        && features.break_completed_count >= 10.0
        && features.blocked_attempt_count == 0.0
        && features.short_break_overtime_seconds == 0.0
        && features.long_break_overtime_seconds == 0.0
        && features.comparable_opportunity_count >= 24.0
    {
        selected.long_break_after_focus_count =
            (selected.long_break_after_focus_count + 1).clamp(CADENCE_MINIMUM, CADENCE_MAXIMUM);
        push_unique(&mut reasons, "capacity_rebuild");
        return decision(selected, "explore", reasons, state);
    }
    if ready_for_growth {
        selected.focus_duration_minutes =
            (selected.focus_duration_minutes + FOCUS_STEP).clamp(FOCUS_MINIMUM, FOCUS_MAXIMUM);
        push_unique(&mut reasons, "capacity_rebuild");
        return decision(selected, "exploit", reasons, state);
    }
    push_unique(&mut reasons, "hold_current_rhythm");
    decision(selected, "hold", reasons, state)
}

fn support_recovery(selected: &mut CountRhythm) {
    selected.long_break_minutes = (selected.long_break_minutes + LONG_BREAK_STEP)
        .clamp(LONG_BREAK_MINIMUM, LONG_BREAK_MAXIMUM);
    selected.long_break_after_focus_count =
        (selected.long_break_after_focus_count - 1).clamp(CADENCE_MINIMUM, CADENCE_MAXIMUM);
}
fn decrease_focus(value: i64, minimum: i64) -> i64 {
    (value - FOCUS_STEP).clamp(minimum, FOCUS_MAXIMUM)
}
fn decision(
    selected_rhythm: CountRhythm,
    mode: &str,
    reason_codes: Vec<String>,
    state_scores: StateScores,
) -> PolicyDecision {
    PolicyDecision {
        selected_rhythm,
        mode: mode.to_owned(),
        reason_codes,
        state_scores,
    }
}
pub(super) fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}
