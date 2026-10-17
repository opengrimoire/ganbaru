//! Context, observed exposure and component interaction gates for replay.

use crate::adaptive::replay::models::*;
use crate::adaptive::replay::scoring::compare_scores;

const MIN_EVIDENCE: f64 = 4.0;
const MAX_BREACH_RATE: f64 = 0.25;
const MAX_INTERACTION_BREACH_INCREASE: f64 = 0.15;
const MAX_INTERACTION_RATIO_DROP: f64 = 0.1;

pub(super) fn context_gate(
    candidate: &str,
    context: &str,
    score: &OutcomeScore,
    options: &GateOptions,
) -> ContextGate {
    let minimum = minimum_evidence(options.min_matched_outcomes_per_context);
    let maximum = clamp_ratio(options.max_guardrail_breach_rate, MAX_BREACH_RATE);
    let reasons = gate_reasons(
        score,
        minimum,
        maximum,
        "insufficient_matched_outcomes",
        "guardrail_breach_rate",
    );
    ContextGate {
        candidate_id: candidate.to_owned(),
        context_key: context.to_owned(),
        status: gate_status(score, minimum, maximum),
        reasons,
        scored_outcome_count: score.scored_outcome_count,
        min_matched_outcomes_per_context: minimum,
        guardrail_breach_rate: score.guardrail_breach_rate,
        max_guardrail_breach_rate: maximum,
    }
}

pub(super) fn policy_gate(candidate: &str, contexts: Vec<ContextGate>) -> PolicyGate {
    let mut reasons = Vec::new();
    for gate in &contexts {
        for reason in &gate.reasons {
            if !reasons.contains(reason) {
                reasons.push(reason.clone());
            }
        }
    }
    let status = if contexts.iter().any(|gate| gate.status == GateStatus::Fail) {
        GateStatus::Fail
    } else if contexts
        .iter()
        .any(|gate| gate.status == GateStatus::InsufficientEvidence)
    {
        GateStatus::InsufficientEvidence
    } else {
        GateStatus::Pass
    };
    PolicyGate {
        candidate_id: candidate.to_owned(),
        status,
        reasons,
        context_gates: contexts,
    }
}

pub(super) fn observed_gate(candidate: &str, options: &GateOptions) -> ObservedGate {
    let empty = OutcomeScore::default();
    // The last score wins when a candidate repeats.
    let score = options
        .observed_candidate_outcome_scores
        .iter()
        .rev()
        .find(|entry| entry.candidate_id == candidate)
        .map(|entry| &entry.outcome_score)
        .unwrap_or(&empty);
    let minimum = minimum_evidence(options.min_observed_candidate_outcomes);
    let maximum = clamp_ratio(
        options.max_observed_candidate_guardrail_breach_rate,
        MAX_BREACH_RATE,
    );
    ObservedGate {
        candidate_id: candidate.to_owned(),
        status: gate_status(score, minimum, maximum),
        reasons: gate_reasons(
            score,
            minimum,
            maximum,
            "insufficient_observed_candidate_outcomes",
            "observed_candidate_guardrail_breach_rate",
        ),
        scored_outcome_count: score.scored_outcome_count,
        min_observed_candidate_outcomes: minimum,
        guardrail_breach_rate: score.guardrail_breach_rate,
        max_observed_candidate_guardrail_breach_rate: maximum,
    }
}

pub(super) fn interaction(
    candidate: &str,
    parameter_keys: Vec<String>,
    combined: PolicyEvaluation,
    components: Vec<PolicyEvaluation>,
    options: &GateOptions,
) -> Interaction {
    let minimum = minimum_evidence(options.min_interaction_matched_outcomes);
    let best = components
        .iter()
        .filter(|entry| entry.outcome_score.scored_outcome_count >= minimum)
        .min_by(|a, b| compare_scores(&a.outcome_score, &b.outcome_score))
        .cloned();
    let mut reasons = Vec::new();
    if combined.outcome_score.scored_outcome_count < minimum {
        reasons.push("insufficient_combined_evidence".to_owned());
    }
    if best.is_none() {
        reasons.push("insufficient_component_evidence".to_owned());
    }
    let difference = |a: Option<f64>, b: Option<f64>| a.zip(b).map(|(a, b)| a - b);
    let guardrail_increase = best.as_ref().and_then(|best| {
        difference(
            combined.outcome_score.guardrail_breach_rate,
            best.outcome_score.guardrail_breach_rate,
        )
    });
    let clean_focus_drop = best.as_ref().and_then(|best| {
        difference(
            best.outcome_score.clean_focus_ratio,
            combined.outcome_score.clean_focus_ratio,
        )
    });
    let completion_drop = best.as_ref().and_then(|best| {
        difference(
            best.outcome_score.completion_rate,
            combined.outcome_score.completion_rate,
        )
    });
    let status = if !reasons.is_empty() {
        InteractionStatus::InsufficientEvidence
    } else {
        for (value, maximum, reason) in [
            (
                guardrail_increase,
                clamp_ratio(
                    options.max_interaction_guardrail_breach_rate_increase,
                    MAX_INTERACTION_BREACH_INCREASE,
                ),
                "guardrail_exceeds_component",
            ),
            (
                clean_focus_drop,
                clamp_ratio(
                    options.max_interaction_clean_focus_ratio_drop,
                    MAX_INTERACTION_RATIO_DROP,
                ),
                "clean_focus_under_component",
            ),
            (
                completion_drop,
                clamp_ratio(
                    options.max_interaction_completion_rate_drop,
                    MAX_INTERACTION_RATIO_DROP,
                ),
                "completion_under_component",
            ),
        ] {
            if value.is_some_and(|value| value > maximum) {
                reasons.push(reason.to_owned());
            }
        }
        if !reasons.is_empty() {
            InteractionStatus::Antagonistic
        } else if best.as_ref().is_some_and(|best| {
            compare_scores(&combined.outcome_score, &best.outcome_score).is_lt()
        }) {
            InteractionStatus::Favorable
        } else {
            InteractionStatus::Neutral
        }
    };
    Interaction {
        candidate_id: candidate.to_owned(),
        parameter_keys,
        component_candidate_ids: components
            .iter()
            .map(|entry| entry.candidate_id.clone())
            .collect(),
        status,
        reasons,
        combined_evaluation: combined,
        component_evaluations: components,
        best_component_evaluation: best,
        guardrail_breach_rate_increase: guardrail_increase,
        clean_focus_ratio_drop: clean_focus_drop,
        completion_rate_drop: completion_drop,
    }
}

pub(super) fn candidate_status(
    gate: &PolicyGate,
    observed: &ObservedGate,
    interaction: Option<&Interaction>,
) -> CandidateStatus {
    if gate.status == GateStatus::Fail
        || observed.status == GateStatus::Fail
        || interaction.is_some_and(|entry| entry.status == InteractionStatus::Antagonistic)
    {
        CandidateStatus::Unsafe
    } else if gate.status == GateStatus::Pass {
        CandidateStatus::Usable
    } else {
        CandidateStatus::Inconclusive
    }
}

fn minimum_evidence(value: Option<f64>) -> usize {
    (value
        .filter(|value| value.is_finite())
        .unwrap_or(MIN_EVIDENCE)
        + 0.5)
        .floor()
        .max(1.0) as usize
}
fn clamp_ratio(value: Option<f64>, default: f64) -> f64 {
    let value = value.unwrap_or(default);
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        MAX_BREACH_RATE
    }
}
fn gate_status(score: &OutcomeScore, minimum: usize, maximum: f64) -> GateStatus {
    if score.scored_outcome_count < minimum {
        GateStatus::InsufficientEvidence
    } else if score
        .guardrail_breach_rate
        .is_some_and(|rate| rate > maximum)
    {
        GateStatus::Fail
    } else {
        GateStatus::Pass
    }
}
fn gate_reasons(
    score: &OutcomeScore,
    minimum: usize,
    maximum: f64,
    insufficient: &str,
    breach: &str,
) -> Vec<String> {
    match gate_status(score, minimum, maximum) {
        GateStatus::Pass => Vec::new(),
        GateStatus::InsufficientEvidence => vec![insufficient.to_owned()],
        GateStatus::Fail => vec![breach.to_owned()],
    }
}
