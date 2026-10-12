use crate::adaptive::models::{FeatureVector, StateScores};

const STATE_DECAY: f64 = 0.55;
const HIGH_PRESSURE_THRESHOLD: f64 = 0.58;

/// Convert canonical behavior into bounded adaptive state scores.
pub fn derive_adaptive_state(
    features: &FeatureVector,
    previous: Option<StateScores>,
) -> StateScores {
    let current = current_state(features);
    let Some(previous) = previous else {
        return current;
    };
    StateScores {
        readiness: blend(previous.readiness, current.readiness),
        strain: blend(previous.strain, current.strain),
        recovery_debt: blend(previous.recovery_debt, current.recovery_debt),
        avoidance_pressure: blend(previous.avoidance_pressure, current.avoidance_pressure),
        momentum: blend(previous.momentum, current.momentum),
        confidence: current
            .confidence
            .max(blend(previous.confidence, current.confidence)),
    }
}

/// Reason order is stable because persisted explanations and parity fixtures depend on it.
pub fn reason_codes_for_state(state: StateScores) -> Vec<String> {
    let mut reasons = Vec::new();
    if state.strain >= HIGH_PRESSURE_THRESHOLD {
        reasons.push("high_strain".to_owned());
    }
    if state.avoidance_pressure >= HIGH_PRESSURE_THRESHOLD {
        reasons.push("high_avoidance_pressure".to_owned());
    }
    if state.recovery_debt >= HIGH_PRESSURE_THRESHOLD {
        reasons.push("high_recovery_debt".to_owned());
    }
    if state.momentum >= 0.68 {
        reasons.push("clean_momentum".to_owned());
    }
    if reasons.is_empty() {
        reasons.push("hold_current_rhythm".to_owned());
    }
    reasons
}

fn current_state(f: &FeatureVector) -> StateScores {
    let focus_attempts = (f.completed_focus_segments + f.interrupted_focus_segments).max(1.0);
    let break_attempts = (f.break_started_count + f.break_skipped_count).max(1.0);
    let clean_completion_rate = f.completed_focus_segments / focus_attempts;
    let failure_rate = f.focus_failure_count / focus_attempts;
    let late_failure_rate = f.late_focus_failure_count / f.late_focus_segment_count.max(1.0);
    let failure_intensity = normalized(f.focus_failure_count, 3.0);
    let late_failure_pressure = normalized(f.late_focus_failure_count, 2.0);
    let idle_pressure = normalized(f.idle_pause_count + f.idle_pause_seconds / 600.0, 4.0);
    let focus_idle_pressure = normalized(
        f.focus_idle_pause_count + f.focus_idle_pause_seconds / 600.0,
        4.0,
    );
    let early_focus_idle_pressure = normalized(f.early_focus_idle_pause_count, 2.0);
    let late_focus_idle_pressure = normalized(f.late_focus_idle_pause_count, 2.0);
    let repeated_blocked_source_pressure = normalized(
        f.repeated_blocked_source_attempt_count + f.focus_repeated_blocked_source_attempt_count,
        8.0,
    );
    let early_focus_block_pressure = normalized(f.early_focus_blocked_attempt_count, 6.0);
    let late_focus_block_pressure = normalized(f.late_focus_blocked_attempt_count, 4.0);
    let go_to_break_now_pressure = normalized(f.go_to_break_now_count, 3.0);
    let early_go_to_break_pressure = normalized(f.early_go_to_break_now_count, 2.0);
    let late_go_to_break_pressure = normalized(f.late_go_to_break_now_count, 2.0);
    let early_break_success_pressure = normalized(f.start_focus_now_success_count, 3.0);
    let early_break_failure_pressure = normalized(f.start_focus_now_failure_count, 2.0);
    let break_overtime_block_pressure = normalized(f.break_overtime_blocked_attempt_count, 3.0);
    let skipped_break_failure_pressure = normalized(
        f.skipped_break_next_focus_failure_count + f.skipped_long_break_next_focus_failure_count,
        3.0,
    );
    let manual_pause_pressure =
        normalized(f.manual_pause_count + f.manual_pause_seconds / 900.0, 4.0);
    let blocked_pressure = normalized(
        f.focus_blocked_attempt_count + f.blocked_burst_count * 2.0,
        8.0,
    );
    let break_overtime_pressure = normalized(
        f.short_break_overtime_seconds / 180.0 + f.long_break_overtime_seconds / 300.0,
        4.0,
    );
    let skipped_break_pressure = normalized(f.break_skipped_count, 3.0);
    let break_drift = normalized(f.short_break_overtime_seconds / 120.0, 3.0);
    let stop_pressure = normalized(f.stop_count, 2.0);
    let confidence_penalty: f64 = f
        .data_quality_flags
        .iter()
        .map(|flag| match flag.as_str() {
            "extension_unavailable" => 0.18,
            "desktop_tracking_unavailable" => 0.06,
            "diary_missing" => 0.04,
            "idle_detection_disabled" => 0.08,
            "crash_recovered" => 0.14,
            _ => 0.04,
        })
        .sum();
    let strain = clamp01(
        failure_rate * 0.24
            + failure_intensity * 0.34
            + late_failure_pressure * 0.1
            + idle_pressure * 0.22
            + focus_idle_pressure * 0.08
            + early_focus_idle_pressure * 0.1
            + late_focus_idle_pressure * 0.08
            + late_focus_block_pressure * 0.08
            + go_to_break_now_pressure * 0.1
            + early_go_to_break_pressure * 0.12
            + late_go_to_break_pressure * 0.08
            + early_break_failure_pressure * 0.08
            + skipped_break_failure_pressure * 0.1
            + manual_pause_pressure * 0.12
            + blocked_pressure * 0.18
            + stop_pressure * 0.08,
    );
    let avoidance_pressure = clamp01(
        blocked_pressure * 0.72
            + repeated_blocked_source_pressure * 0.22
            + early_focus_block_pressure * 0.16
            + normalized(f.break_blocked_attempt_count, 6.0) * 0.16
            + break_overtime_block_pressure * 0.2
            + stop_pressure * 0.12,
    );
    let recovery_debt = clamp01(
        skipped_break_pressure * 0.32
            + break_overtime_pressure * 0.26
            + failure_rate * 0.22
            + late_failure_rate * 0.12
            + focus_idle_pressure * 0.06
            + late_focus_idle_pressure * 0.12
            + late_focus_block_pressure * 0.12
            + go_to_break_now_pressure * 0.08
            + early_go_to_break_pressure * 0.08
            + late_go_to_break_pressure * 0.12
            + early_break_failure_pressure * 0.12
            + break_overtime_block_pressure * 0.14
            + skipped_break_failure_pressure * 0.2
            + break_drift * 0.12
            + stop_pressure * 0.08,
    );
    let momentum = clamp01(
        clean_completion_rate * 0.52
            + normalized(f.clean_focus_seconds / 60.0, 120.0) * 0.2
            + early_break_success_pressure * 0.06
            + (1.0 - blocked_pressure).max(0.0) * 0.14
            + (1.0 - break_overtime_pressure).max(0.0) * 0.14
            - recovery_debt * 0.25,
    );
    let readiness = clamp01(
        momentum * 0.55
            + clean_completion_rate * 0.25
            + normalized(f.extension_count, 3.0) * 0.1
            + early_break_success_pressure * 0.08
            - strain * 0.28
            - recovery_debt * 0.22,
    );
    let confidence = clamp01(
        normalized(f.comparable_opportunity_count, 12.0) * 0.68
            + normalized(focus_attempts, 8.0) * 0.22
            + normalized(break_attempts, 6.0) * 0.1
            - confidence_penalty,
    );
    StateScores {
        readiness,
        strain,
        recovery_debt,
        avoidance_pressure,
        momentum,
        confidence,
    }
}

fn blend(previous: f64, current: f64) -> f64 {
    clamp01(previous * STATE_DECAY + current * (1.0 - STATE_DECAY))
}
fn normalized(value: f64, scale: f64) -> f64 {
    if scale <= 0.0 {
        0.0
    } else {
        clamp01(value / scale)
    }
}
pub(super) fn clamp01(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
