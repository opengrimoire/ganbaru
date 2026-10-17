//! Observed outcome attribution and conservative burden ranking for replay.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::adaptive::models::FeatureVector;
use crate::adaptive::replay::models::{
    Attribution, CandidateObservedScore, ObservedOutcome, OutcomeScore, ReplayResult,
};

const SHORT_BREAK_DRIFT_SECONDS: f64 = 120.0;
const LONG_BREAK_DRIFT_SECONDS: f64 = 300.0;
const CLEAN_FOCUS_LOSS_SECONDS: f64 = 240.0;

/// Join outcomes by opportunity and score only the requested attribution.
/// Duplicate outcomes are rejected because their attribution is ambiguous.
pub fn score_results(
    results: &[ReplayResult],
    outcomes: &[ObservedOutcome],
    attribution: Attribution,
) -> Result<OutcomeScore, String> {
    let mut by_id = BTreeMap::new();
    for outcome in outcomes {
        if by_id.insert(&outcome.opportunity_id, outcome).is_some() {
            return Err(format!(
                "Duplicate adaptive replay outcome for opportunity {}",
                outcome.opportunity_id
            ));
        }
    }
    let mut score = OutcomeScore {
        attribution,
        ..OutcomeScore::default()
    };
    for result in results {
        let Some(outcome) = by_id.get(&result.opportunity_id) else {
            continue;
        };
        score.total_joined_outcomes += 1;
        let matches = outcome
            .observed_rhythm
            .map(|rhythm| rhythm == result.decision.selected_rhythm);
        match matches {
            Some(true) => score.matched_selected_rhythm_count += 1,
            Some(false) => score.mismatched_selected_rhythm_count += 1,
            None => score.unknown_selected_rhythm_count += 1,
        }
        if attribution == Attribution::SelectedRhythmMatch && matches != Some(true) {
            continue;
        }
        add_features(&mut score, &outcome.features);
    }
    derive_rates(&mut score);
    Ok(score)
}

/// Score actual candidate exposure without counterfactual attribution.
pub fn score_observed(outcomes: &[ObservedOutcome]) -> OutcomeScore {
    let mut score = OutcomeScore {
        total_joined_outcomes: outcomes.len(),
        unknown_selected_rhythm_count: outcomes.len(),
        ..OutcomeScore::default()
    };
    for outcome in outcomes {
        add_features(&mut score, &outcome.features);
    }
    derive_rates(&mut score);
    score
}

/// Group actual exposure in first-observed candidate order.
pub fn score_by_candidate(
    outcomes: &[ObservedOutcome],
    candidate_ids: &BTreeMap<String, Option<String>>,
) -> Vec<CandidateObservedScore> {
    let mut groups: Vec<(String, Vec<ObservedOutcome>)> = Vec::new();
    for outcome in outcomes {
        let Some(Some(candidate)) = candidate_ids.get(&outcome.opportunity_id) else {
            continue;
        };
        if candidate.is_empty() {
            continue;
        }
        if let Some((_, values)) = groups.iter_mut().find(|(id, _)| id == candidate) {
            values.push(outcome.clone());
        } else {
            groups.push((candidate.clone(), vec![outcome.clone()]));
        }
    }
    groups
        .into_iter()
        .map(|(candidate_id, values)| CandidateObservedScore {
            candidate_id,
            opportunity_ids: values
                .iter()
                .map(|value| value.opportunity_id.clone())
                .collect(),
            outcome_score: score_observed(&values),
        })
        .collect()
}

/// Order lower burden before sample size, clean focus and completion.
pub fn compare_scores(a: &OutcomeScore, b: &OutcomeScore) -> Ordering {
    ascending(a.guardrail_breach_rate, b.guardrail_breach_rate)
        .then_with(|| b.scored_outcome_count.cmp(&a.scored_outcome_count))
        .then_with(|| descending(a.clean_focus_ratio, b.clean_focus_ratio))
        .then_with(|| descending(a.completion_rate, b.completion_rate))
        .then_with(|| ascending(a.blocked_attempts_mean, b.blocked_attempts_mean))
        .then_with(|| ascending(a.break_skipped_mean, b.break_skipped_mean))
        .then_with(|| {
            ascending(
                a.short_break_overtime_seconds_mean,
                b.short_break_overtime_seconds_mean,
            )
        })
        .then_with(|| {
            ascending(
                a.long_break_overtime_seconds_mean,
                b.long_break_overtime_seconds_mean,
            )
        })
}

fn ascending(a: Option<f64>, b: Option<f64>) -> Ordering {
    a.unwrap_or(f64::INFINITY)
        .total_cmp(&b.unwrap_or(f64::INFINITY))
}
fn descending(a: Option<f64>, b: Option<f64>) -> Ordering {
    b.unwrap_or(f64::NEG_INFINITY)
        .total_cmp(&a.unwrap_or(f64::NEG_INFINITY))
}

fn add_features(score: &mut OutcomeScore, features: &FeatureVector) {
    score.scored_outcome_count += 1;
    score.completed_focus_segments += features.completed_focus_segments;
    score.interrupted_focus_segments += features.interrupted_focus_segments;
    score.focus_failure_count += features.focus_failure_count;
    score.stop_count += features.stop_count;
    score.clean_focus_seconds += features.clean_focus_seconds;
    score.planned_focus_seconds += features.planned_focus_seconds;
    score.blocked_attempt_count += features.blocked_attempt_count;
    score.break_skipped_count += features.break_skipped_count;
    score.short_break_overtime_seconds += features.short_break_overtime_seconds;
    score.long_break_overtime_seconds += features.long_break_overtime_seconds;
    let reasons = [
        (features.focus_failure_count > 0.0, "focus_failure"),
        (
            features.interrupted_focus_segments > 0.0,
            "interrupted_focus",
        ),
        (features.stop_count > 0.0, "stopped_run"),
        (features.blocked_attempt_count > 0.0, "blocked_attempts"),
        (features.break_skipped_count > 0.0, "skipped_break"),
        (
            features.short_break_overtime_seconds >= SHORT_BREAK_DRIFT_SECONDS,
            "short_break_overtime",
        ),
        (
            features.long_break_overtime_seconds >= LONG_BREAK_DRIFT_SECONDS,
            "long_break_overtime",
        ),
        (
            features.planned_focus_seconds > 0.0
                && features.clean_focus_seconds
                    < features.planned_focus_seconds - CLEAN_FOCUS_LOSS_SECONDS,
            "clean_focus_loss",
        ),
    ];
    if reasons.iter().any(|(present, _)| *present) {
        score.guardrail_breach_count += 1;
    }
    for (_, reason) in reasons.into_iter().filter(|(present, _)| *present) {
        *score
            .guardrail_reason_counts
            .entry(reason.to_owned())
            .or_default() += 1;
    }
}

fn ratio(numerator: f64, denominator: f64) -> Option<f64> {
    (denominator > 0.0).then(|| numerator / denominator)
}
fn derive_rates(score: &mut OutcomeScore) {
    let count = score.scored_outcome_count as f64;
    score.completion_rate = ratio(
        score.completed_focus_segments,
        score.completed_focus_segments + score.interrupted_focus_segments,
    );
    score.clean_focus_ratio = ratio(score.clean_focus_seconds, score.planned_focus_seconds);
    score.blocked_attempts_mean = ratio(score.blocked_attempt_count, count);
    score.break_skipped_mean = ratio(score.break_skipped_count, count);
    score.short_break_overtime_seconds_mean = ratio(score.short_break_overtime_seconds, count);
    score.long_break_overtime_seconds_mean = ratio(score.long_break_overtime_seconds, count);
    score.guardrail_breach_rate = ratio(score.guardrail_breach_count as f64, count);
}
