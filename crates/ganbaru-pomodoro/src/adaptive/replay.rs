//! Native bounded replay evaluation over immutable, canonical evidence.

mod candidates;
pub(crate) mod dataset;
mod gates;
pub mod models;
pub mod scoring;

use std::collections::{BTreeMap, BTreeSet};

use crate::adaptive::experiments::selection::experiment_context_key;
use crate::adaptive::models::LocalTimeFacts;
use candidates::{components, parameter_keys};
use gates::{candidate_status, context_gate, interaction, observed_gate, policy_gate};
use models::*;
use scoring::{compare_scores, score_results};

pub use candidates::{decide_candidate, default_candidates};

const MAX_OPPORTUNITIES: usize = 50;
const MAX_CANDIDATES: usize = 16;

/// Evaluate candidate exposure, comparable contexts and component interactions.
/// Inputs and native timezone facts are supplied from one SQLite snapshot.
pub fn evaluate(
    opportunities: &[ReplayOpportunity],
    candidates: &[BoundedCandidate],
    outcomes: &[ObservedOutcome],
    options: &GateOptions,
    facts: &LocalTimeFacts,
) -> Result<CandidateWorkflow, String> {
    if opportunities.len() > MAX_OPPORTUNITIES || candidates.len() > MAX_CANDIDATES {
        return Err("Adaptive replay exceeds its opportunity or candidate limit".to_owned());
    }
    unique_ids(
        opportunities.iter().map(|entry| entry.id.as_str()),
        "opportunity",
    )?;
    unique_ids(
        candidates.iter().map(|entry| entry.id.as_str()),
        "candidate",
    )?;
    let evaluations = candidates
        .iter()
        .map(|candidate| evaluate_candidate(opportunities, candidate, outcomes, facts))
        .collect::<Result<Vec<_>, _>>()?;
    let context_reports = context_reports(&evaluations, outcomes)?;
    let gates = candidates
        .iter()
        .map(|candidate| {
            policy_gate(
                &candidate.id,
                context_reports
                    .iter()
                    .filter_map(|report| {
                        report
                            .evaluations
                            .iter()
                            .find(|entry| entry.evaluation.candidate_id == candidate.id)
                            .map(|entry| {
                                context_gate(
                                    &candidate.id,
                                    &report.context_key,
                                    &entry.evaluation.outcome_score,
                                    options,
                                )
                            })
                    })
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    let observed_outcome_gates = candidates
        .iter()
        .map(|candidate| observed_gate(&candidate.id, options))
        .collect::<Vec<_>>();
    let mut interactions = Vec::new();
    for (candidate, combined) in candidates.iter().zip(&evaluations) {
        let keys = parameter_keys(candidate);
        if keys.len() < 2 {
            continue;
        }
        let component_evaluations = components(candidate)
            .iter()
            .map(|component| evaluate_candidate(opportunities, component, outcomes, facts))
            .collect::<Result<Vec<_>, _>>()?;
        interactions.push(interaction(
            &candidate.id,
            keys,
            combined.clone(),
            component_evaluations,
            options,
        ));
    }
    let reviews = evaluations
        .iter()
        .zip(&gates)
        .zip(&observed_outcome_gates)
        .map(|((evaluation, gate), observed)| {
            let interaction = interactions
                .iter()
                .find(|entry| entry.candidate_id == evaluation.candidate_id)
                .cloned();
            CandidateReview {
                candidate_id: evaluation.candidate_id.clone(),
                status: candidate_status(gate, observed, interaction.as_ref()),
                evaluation: evaluation.clone(),
                gate: gate.clone(),
                observed_outcome_gate: observed.clone(),
                interaction,
            }
        })
        .collect::<Vec<_>>();
    let ids = |status| {
        reviews
            .iter()
            .filter(|entry| entry.status == status)
            .map(|entry| entry.candidate_id.clone())
            .collect()
    };
    Ok(CandidateWorkflow {
        usable_candidate_ids: ids(CandidateStatus::Usable),
        unsafe_candidate_ids: ids(CandidateStatus::Unsafe),
        inconclusive_candidate_ids: ids(CandidateStatus::Inconclusive),
        evaluations,
        context_reports,
        gates,
        observed_outcome_gates,
        interactions,
        reviews,
    })
}

/// Select the lowest-burden usable candidate for an explicitly accepted context.
/// Equal scores retain configured candidate order.
pub fn select_for_context<'a>(
    workflow: &'a CandidateWorkflow,
    context: &str,
) -> Option<&'a CandidateReview> {
    workflow
        .reviews
        .iter()
        .filter(|review| {
            review.status == CandidateStatus::Usable
                && review
                    .gate
                    .context_gates
                    .iter()
                    .any(|gate| gate.context_key == context && gate.status == GateStatus::Pass)
        })
        .min_by(|a, b| compare_scores(&a.evaluation.outcome_score, &b.evaluation.outcome_score))
}

fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>, kind: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if id.is_empty() || !seen.insert(id) {
            return Err(format!(
                "Invalid or duplicate adaptive replay {kind} id: {id}"
            ));
        }
    }
    Ok(())
}

fn evaluate_candidate(
    opportunities: &[ReplayOpportunity],
    candidate: &BoundedCandidate,
    outcomes: &[ObservedOutcome],
    facts: &LocalTimeFacts,
) -> Result<PolicyEvaluation, String> {
    let results = opportunities
        .iter()
        .map(|opportunity| {
            Ok(ReplayResult {
                opportunity_id: opportunity.id.clone(),
                label: opportunity.label.clone().filter(|label| !label.is_empty()),
                decision: decide_candidate(&opportunity.input, candidate, facts)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    policy_evaluation(&candidate.id, results, outcomes)
}

fn policy_evaluation(
    candidate: &str,
    results: Vec<ReplayResult>,
    outcomes: &[ObservedOutcome],
) -> Result<PolicyEvaluation, String> {
    let summary = summarize_results(&results);
    let outcome_score = score_results(&results, outcomes, Attribution::SelectedRhythmMatch)?;
    Ok(PolicyEvaluation {
        candidate_id: candidate.to_owned(),
        results,
        summary,
        outcome_score,
    })
}

fn context_reports(
    evaluations: &[PolicyEvaluation],
    outcomes: &[ObservedOutcome],
) -> Result<Vec<ContextReport>, String> {
    let mut order = Vec::new();
    let mut contexts: BTreeMap<String, BTreeMap<String, Vec<ReplayResult>>> = BTreeMap::new();
    for evaluation in evaluations {
        for result in &evaluation.results {
            let key = experiment_context_key(&result.decision.context);
            if !contexts.contains_key(&key) {
                order.push(key.clone());
            }
            contexts
                .entry(key)
                .or_default()
                .entry(evaluation.candidate_id.clone())
                .or_default()
                .push(result.clone());
        }
    }
    order
        .into_iter()
        .map(|key| {
            let results_by_candidate = &contexts[&key];
            let context_evaluations = evaluations
                .iter()
                .map(|entry| {
                    Ok(ContextEvaluation {
                        context_key: key.clone(),
                        evaluation: policy_evaluation(
                            &entry.candidate_id,
                            results_by_candidate
                                .get(&entry.candidate_id)
                                .cloned()
                                .unwrap_or_default(),
                            outcomes,
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(ContextReport {
                context_key: key,
                evaluations: context_evaluations,
            })
        })
        .collect()
}

fn summarize_results(results: &[ReplayResult]) -> ReplaySummary {
    let mut summary = ReplaySummary {
        total_opportunities: results.len(),
        ..ReplaySummary::default()
    };
    for result in results {
        let decision = &result.decision;
        *summary
            .decision_mode_counts
            .entry(decision.decision_mode.clone())
            .or_default() += 1;
        for reason in &decision.reason_codes {
            *summary
                .reason_code_counts
                .entry(reason.clone())
                .or_default() += 1;
        }
        if let Some(assignment) = &decision.experiment_assignment {
            *summary
                .experiment_assignment_counts
                .entry(assignment.experiment.id.clone())
                .or_default() += 1;
        }
        let current = decision.current_rhythm;
        let selected = decision.selected_rhythm;
        for (changed, key) in [
            (
                current.focus_duration_minutes != selected.focus_duration_minutes,
                "focus_duration_minutes",
            ),
            (
                current.short_break_minutes != selected.short_break_minutes,
                "short_break_minutes",
            ),
            (
                current.long_break_minutes != selected.long_break_minutes,
                "long_break_minutes",
            ),
            (
                current.long_break_after_focus_count != selected.long_break_after_focus_count,
                "long_break_after_focus_count",
            ),
        ] {
            if changed {
                *summary
                    .changed_value_counts
                    .entry(key.to_owned())
                    .or_default() += 1;
            }
        }
    }
    summary
}
