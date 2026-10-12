//! Bounded candidate transformations preserve recovery and experiment guardrails.

use crate::adaptive::decision::{AdaptiveDecision, AdaptiveDecisionInput, decide_run_start};
use crate::adaptive::models::{CountRhythm, LocalTimeFacts};
use crate::adaptive::replay::models::BoundedCandidate;

/// Default offline replay candidates, in deterministic tie-breaking order.
pub fn default_candidates() -> Vec<BoundedCandidate> {
    vec![
        BoundedCandidate {
            id: "focus-growth-with-short-break-support".to_owned(),
            focus_duration_delta_minutes: Some(5.0),
            short_break_delta_minutes: Some(2.0),
            ..BoundedCandidate::default()
        },
        BoundedCandidate {
            id: "long-recovery-support".to_owned(),
            long_break_delta_minutes: Some(5.0),
            long_break_after_focus_count_delta: Some(-1.0),
            ..BoundedCandidate::default()
        },
        BoundedCandidate {
            id: "capacity-cadence-growth".to_owned(),
            focus_duration_delta_minutes: Some(5.0),
            long_break_after_focus_count_delta: Some(1.0),
            ..BoundedCandidate::default()
        },
    ]
}

/// Apply a bounded candidate only to a decision mode it is allowed to alter.
pub fn decide_candidate(
    input: &AdaptiveDecisionInput,
    candidate: &BoundedCandidate,
    facts: &LocalTimeFacts,
) -> Result<AdaptiveDecision, String> {
    let mut decision = decide_run_start(input, facts)?;
    let allowed = candidate.allowed_decision_modes.as_ref().map_or_else(
        || {
            matches!(
                decision.decision_mode.as_str(),
                "hold" | "exploit" | "explore"
            )
        },
        |modes| modes.contains(&decision.decision_mode),
    );
    if !allowed {
        return Ok(decision);
    }
    let selected = adjusted_rhythm(decision.selected_rhythm, candidate);
    if selected == decision.selected_rhythm {
        return Ok(decision);
    }
    decision.selected_rhythm = selected;
    decision.decision_mode = "explore".to_owned();
    decision
        .reason_codes
        .retain(|reason| reason != "hold_current_rhythm");
    if !decision
        .reason_codes
        .iter()
        .any(|reason| reason == "replay_candidate")
    {
        decision.reason_codes.push("replay_candidate".to_owned());
    }
    decision.candidate_id = Some(candidate.id.clone());
    decision.experiment_update = None;
    decision.experiment_assignment = None;
    Ok(decision)
}

pub(super) fn parameter_keys(candidate: &BoundedCandidate) -> Vec<String> {
    adjustments(candidate)
        .into_iter()
        .filter(|(_, exact, delta, _, _)| {
            exact.is_some_and(f64::is_finite)
                || delta.is_some_and(|value| value.is_finite() && value != 0.0)
        })
        .map(|(key, ..)| key.to_owned())
        .collect()
}

pub(super) fn components(candidate: &BoundedCandidate) -> Vec<BoundedCandidate> {
    parameter_keys(candidate)
        .into_iter()
        .map(|key| {
            let mut component = BoundedCandidate {
                id: format!("{}:{key}", candidate.id),
                allowed_decision_modes: candidate.allowed_decision_modes.clone(),
                ..BoundedCandidate::default()
            };
            match key.as_str() {
                "focus_duration_minutes" => {
                    component.focus_duration_minutes = candidate.focus_duration_minutes;
                    component.focus_duration_delta_minutes = candidate.focus_duration_delta_minutes;
                }
                "short_break_minutes" => {
                    component.short_break_minutes = candidate.short_break_minutes;
                    component.short_break_delta_minutes = candidate.short_break_delta_minutes;
                }
                "long_break_minutes" => {
                    component.long_break_minutes = candidate.long_break_minutes;
                    component.long_break_delta_minutes = candidate.long_break_delta_minutes;
                }
                "long_break_after_focus_count" => {
                    component.long_break_after_focus_count = candidate.long_break_after_focus_count;
                    component.long_break_after_focus_count_delta =
                        candidate.long_break_after_focus_count_delta;
                }
                _ => unreachable!("Candidate parameter keys are internal constants"),
            }
            component
        })
        .collect()
}

type Adjustment = (&'static str, Option<f64>, Option<f64>, i64, i64);
fn adjustments(candidate: &BoundedCandidate) -> [Adjustment; 4] {
    [
        (
            "focus_duration_minutes",
            candidate.focus_duration_minutes,
            candidate.focus_duration_delta_minutes,
            15,
            60,
        ),
        (
            "short_break_minutes",
            candidate.short_break_minutes,
            candidate.short_break_delta_minutes,
            3,
            12,
        ),
        (
            "long_break_minutes",
            candidate.long_break_minutes,
            candidate.long_break_delta_minutes,
            10,
            30,
        ),
        (
            "long_break_after_focus_count",
            candidate.long_break_after_focus_count,
            candidate.long_break_after_focus_count_delta,
            2,
            5,
        ),
    ]
}
fn adjusted_rhythm(rhythm: CountRhythm, candidate: &BoundedCandidate) -> CountRhythm {
    let current = [
        rhythm.focus_duration_minutes,
        rhythm.short_break_minutes,
        rhythm.long_break_minutes,
        rhythm.long_break_after_focus_count,
    ];
    let values = adjustments(candidate)
        .into_iter()
        .zip(current)
        .map(|((_, exact, delta, min, max), value)| {
            let raw = exact
                .filter(|v| v.is_finite())
                .unwrap_or(value as f64 + delta.filter(|v| v.is_finite()).unwrap_or(0.0));
            // Round like JavaScript Math.round, so negative halves round toward positive infinity.
            ((raw + 0.5).floor().clamp(min as f64, max as f64)) as i64
        })
        .collect::<Vec<_>>();
    CountRhythm {
        focus_duration_minutes: values[0],
        short_break_minutes: values[1],
        long_break_minutes: values[2],
        long_break_after_focus_count: values[3],
        ..rhythm
    }
}
