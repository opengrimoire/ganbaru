use chrono::DateTime;

use crate::adaptive::experiments::analysis::analyze_experiment;
use crate::adaptive::experiments::models::{
    ExperimentAssignment, ExperimentLane, ExperimentState, SelectionInput,
};
use crate::adaptive::models::{ContextBucket, CountRhythm, LocalTimeFacts};

const DAY_MS: i64 = 86_400_000;
const COOLDOWN_MS: i64 = 14 * DAY_MS;
const EXPLORATION_WINDOW_MS: i64 = 7 * DAY_MS;
const WEEKLY_CONTEXT_BUDGET: usize = 2;
const MODERATE_CONFIDENCE: f64 = 0.5;
const LOW_RISK: f64 = 0.35;
const HIGH_RISK: f64 = 0.58;
const HIGH_MOMENTUM: f64 = 0.68;
const LONG_BREAK_DRIFT_SECONDS: f64 = 300.0;

/// Select a bounded run assignment using native device-local facts for its seed.
/// Missing local facts fail the operation rather than silently changing its day.
pub fn select_run_start_assignment(
    input: &SelectionInput,
    local_time: &LocalTimeFacts,
) -> Result<Option<ExperimentAssignment>, String> {
    let occurred_at_ms = parse_timestamp_ms(&input.occurred_at)
        .ok_or_else(|| "Adaptive experiment occurrence time is invalid".to_owned())?;
    let context_key = experiment_context_key(&input.context);
    let recent: Vec<_> = input
        .experiment_assignments
        .iter()
        .filter_map(|assignment| {
            let assigned_at_ms = parse_timestamp_ms(&assignment.assigned_at)?;
            (assignment.context_key == context_key
                && assigned_at_ms >= occurred_at_ms.saturating_sub(EXPLORATION_WINDOW_MS)
                && assigned_at_ms < occurred_at_ms)
                .then_some((assignment, assigned_at_ms))
        })
        .collect();
    if recent.len() >= WEEKLY_CONTEXT_BUDGET {
        return Ok(None);
    }
    // Keep the first assignment when timestamps are equal.
    let latest = recent
        .iter()
        .fold(None, |latest: Option<&(_, i64)>, entry| {
            if latest.is_none_or(|previous| entry.1 > previous.1) {
                Some(entry)
            } else {
                latest
            }
        });
    for lane in ExperimentLane::SELECTION_ORDER {
        if !is_eligible(lane, input) {
            continue;
        }
        let definition = lane.definition();
        if experiment_cooldown_state(&input.experiment_states, &definition.id, &input.occurred_at)
            .is_some()
            || matches!(
                analyze_experiment(lane, &input.experiment_outcomes, Some(&context_key))
                    .decision
                    .as_str(),
                "prefer_control" | "prefer_treatment"
            )
        {
            continue;
        }
        let rejected_component = match lane {
            ExperimentLane::FocusSupport => {
                scalar_rejected(ExperimentLane::FocusDuration, input, &context_key)
                    || scalar_rejected(ExperimentLane::ShortBreak, input, &context_key)
            }
            ExperimentLane::LongRecovery => {
                scalar_rejected(ExperimentLane::LongBreak, input, &context_key)
                    || scalar_rejected(ExperimentLane::EarlierCadence, input, &context_key)
            }
            _ => false,
        };
        if rejected_component
            || latest.is_some_and(|latest| latest.0.experiment_id != definition.id)
        {
            continue;
        }
        let local = local_time.get(&occurred_at_ms).ok_or_else(|| {
            "Native local-time facts are missing for adaptive assignment".to_owned()
        })?;
        let seed = format!(
            "{}|{}|{}|{}",
            definition.id, context_key, local.date_string, input.occurred_at
        );
        let variant =
            definition.variants[stable_variant_index(&seed, definition.variants.len())].clone();
        let mut rhythm = input.selected_rhythm;
        match lane {
            ExperimentLane::FocusDuration => {
                rhythm.focus_duration_minutes = variant.numeric_value as i64
            }
            ExperimentLane::ShortBreak => rhythm.short_break_minutes = variant.numeric_value as i64,
            ExperimentLane::LongBreak => rhythm.long_break_minutes = variant.numeric_value as i64,
            ExperimentLane::EarlierCadence | ExperimentLane::LaterCadence => {
                rhythm.long_break_after_focus_count = variant.numeric_value as i64
            }
            ExperimentLane::FocusSupport => {
                rhythm = CountRhythm::BASELINE;
                if !variant.is_control {
                    rhythm.focus_duration_minutes = 45;
                    rhythm.short_break_minutes = 7;
                }
            }
            ExperimentLane::LongRecovery => {
                if !variant.is_control {
                    rhythm = CountRhythm {
                        long_break_minutes: 15,
                        long_break_after_focus_count: 3,
                        ..CountRhythm::BASELINE
                    };
                }
            }
        }
        return Ok(Some(ExperimentAssignment {
            experiment: definition,
            variant,
            assignment_seed: seed,
            assigned_at: input.occurred_at.clone(),
            selected_rhythm: rhythm,
        }));
    }
    Ok(None)
}

/// Return the latest terminal experiment state inside the inclusive cooldown.
pub fn experiment_cooldown_state<'a>(
    states: &'a [ExperimentState],
    experiment_id: &str,
    occurred_at: &str,
) -> Option<&'a ExperimentState> {
    let occurred_at_ms = parse_timestamp_ms(occurred_at)?;
    let mut latest = None;
    let mut latest_ms = i64::MIN;
    for state in states {
        if state.experiment_id != experiment_id
            || !matches!(state.status.as_str(), "completed" | "abandoned")
        {
            continue;
        }
        let Some(ended_at_ms) = state.ended_at.as_deref().and_then(parse_timestamp_ms) else {
            continue;
        };
        if ended_at_ms > occurred_at_ms || occurred_at_ms.saturating_sub(ended_at_ms) > COOLDOWN_MS
        {
            continue;
        }
        if ended_at_ms > latest_ms {
            latest = Some(state);
            latest_ms = ended_at_ms;
        }
    }
    latest
}

/// Reproduce JavaScript's UTF-16 FNV-1a hash with wrapping 32-bit multiplication.
pub fn stable_variant_index(seed: &str, variant_count: usize) -> usize {
    if variant_count == 0 {
        return 0;
    }
    let hash = seed.encode_utf16().fold(2_166_136_261_u32, |hash, unit| {
        (hash ^ u32::from(unit)).wrapping_mul(16_777_619)
    });
    (u64::from(hash) % variant_count as u64) as usize
}

/// Stable context spelling is shared by state lookup, experiment evidence and replay.
pub fn experiment_context_key(context: &ContextBucket) -> String {
    [
        &context.time_of_day,
        &context.session_position,
        &context.event_length,
        &context.workload,
        &context.energy,
        context.environment_id.as_deref().unwrap_or("none"),
    ]
    .join(":")
}

fn parse_timestamp_ms(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.timestamp_millis())
}

fn scalar_rejected(lane: ExperimentLane, input: &SelectionInput, context: &str) -> bool {
    experiment_cooldown_state(
        &input.experiment_states,
        &lane.definition().id,
        &input.occurred_at,
    )
    .is_some_and(|state| state.status == "abandoned")
        || analyze_experiment(lane, &input.experiment_outcomes, Some(context)).decision
            == "prefer_control"
}

fn is_eligible(lane: ExperimentLane, input: &SelectionInput) -> bool {
    let features = &input.features;
    let state = &input.state;
    if input.current_rhythm != CountRhythm::BASELINE
        || features
            .data_quality_flags
            .iter()
            .any(|flag| flag == "extension_unavailable")
        || state.confidence < MODERATE_CONFIDENCE
    {
        return false;
    }
    let risk_threshold = if lane == ExperimentLane::EarlierCadence {
        HIGH_RISK
    } else {
        LOW_RISK
    };
    if state.strain >= risk_threshold
        || state.recovery_debt >= risk_threshold
        || state.avoidance_pressure >= risk_threshold
    {
        return false;
    }
    let mut expected = CountRhythm::BASELINE;
    match lane {
        ExperimentLane::FocusDuration | ExperimentLane::FocusSupport => {
            expected.focus_duration_minutes = 45
        }
        ExperimentLane::ShortBreak => expected.short_break_minutes = 7,
        ExperimentLane::LongBreak | ExperimentLane::LongRecovery => {
            expected.long_break_minutes = 15
        }
        ExperimentLane::EarlierCadence => expected.long_break_after_focus_count = 3,
        ExperimentLane::LaterCadence => expected.long_break_after_focus_count = 5,
    }
    if input.selected_rhythm != expected {
        return false;
    }
    let minimum_opportunities = match lane {
        ExperimentLane::FocusSupport
        | ExperimentLane::LongRecovery
        | ExperimentLane::LaterCadence => 24.0,
        _ => 8.0,
    };
    if features.comparable_opportunity_count < minimum_opportunities {
        return false;
    }
    match lane {
        ExperimentLane::FocusDuration => true,
        ExperimentLane::FocusSupport => {
            features.completed_focus_segments >= 8.0
                && features.break_completed_count >= 8.0
                && features.focus_failure_count <= 0.0
                && features.interrupted_focus_segments <= 0.0
                && features.break_skipped_count <= 0.0
        }
        ExperimentLane::ShortBreak => features.short_break_overtime_seconds >= 120.0,
        ExperimentLane::LongBreak => {
            features.long_break_overtime_seconds >= LONG_BREAK_DRIFT_SECONDS
                && features.long_break_overtime_blocked_attempt_count <= 0.0
        }
        ExperimentLane::LongRecovery => {
            features.long_break_overtime_seconds >= LONG_BREAK_DRIFT_SECONDS
                && features.long_break_overtime_blocked_attempt_count <= 0.0
                && features.completed_focus_segments >= 8.0
                && features.break_completed_count >= 4.0
                && features.break_skipped_count <= 0.0
                && features.focus_failure_count <= 0.0
                && features.interrupted_focus_segments <= 0.0
        }
        ExperimentLane::EarlierCadence => {
            features.late_focus_segment_count >= 2.0
                && (features.late_focus_failure_count >= 1.0
                    || features.late_focus_blocked_attempt_count >= 3.0)
        }
        ExperimentLane::LaterCadence => {
            features.completed_focus_segments >= 12.0
                && features.clean_focus_seconds >= features.planned_focus_seconds
                && features.interrupted_focus_segments <= 0.0
                && features.focus_failure_count <= 0.0
                && features.break_skipped_count <= 0.0
                && features.break_completed_count >= 10.0
                && features.blocked_attempt_count <= 0.0
                && features.short_break_overtime_seconds <= 0.0
                && features.long_break_overtime_seconds <= 0.0
                && state.momentum >= HIGH_MOMENTUM
        }
    }
}
