use crate::adaptive::experiments::models::ExperimentOutcome;

const PRIOR_RUNS: f64 = 4.0;
const MIN_NEIGHBOR_WEIGHT: f64 = 0.25;

pub fn variant_outcome(
    outcomes: &[ExperimentOutcome],
    experiment: &str,
    variant: &str,
    context: Option<&str>,
) -> ExperimentOutcome {
    let mut result = ExperimentOutcome::empty(experiment, variant, context);
    for outcome in outcomes.iter().filter(|outcome| {
        outcome.experiment_id == experiment
            && outcome.variant_key == variant
            && context.is_none_or(|context| outcome.context_key.as_deref() == Some(context))
    }) {
        result.values.add(&outcome.values);
    }
    result
}

/// Combine the exact context with a bounded prior, never a full external sample.
pub fn pooled_outcomes(
    outcomes: &[ExperimentOutcome],
    context: &str,
    neighbors_only: bool,
) -> Vec<ExperimentOutcome> {
    let mut keys = Vec::<(&str, &str)>::new();
    for outcome in outcomes {
        let pair = (outcome.experiment_id.as_str(), outcome.variant_key.as_str());
        if !keys.contains(&pair) {
            keys.push(pair);
        }
    }
    keys.into_iter()
        .map(|(experiment, variant)| {
            let mut result = variant_outcome(outcomes, experiment, variant, Some(context));
            let mut external = ExperimentOutcome::empty(experiment, variant, None);
            for outcome in outcomes.iter().filter(|outcome| {
                outcome.experiment_id == experiment
                    && outcome.variant_key == variant
                    && outcome.context_key.as_deref() != Some(context)
            }) {
                let weight = if neighbors_only {
                    outcome
                        .context_key
                        .as_deref()
                        .map(|key| neighbor_weight(context, key))
                        .unwrap_or(0.0)
                } else {
                    1.0
                };
                if weight < MIN_NEIGHBOR_WEIGHT {
                    continue;
                }
                external.values.add(&outcome.values.scaled(weight));
            }
            let weight = if external.values.run_observed_count <= 0.0 {
                0.0
            } else {
                (PRIOR_RUNS / external.values.run_observed_count).min(1.0)
            };
            result.values.add(&external.values.scaled(weight));
            result
        })
        .collect()
}

fn neighbor_weight(target: &str, candidate: &str) -> f64 {
    let target: Vec<&str> = target.split(':').take(6).collect();
    let candidate: Vec<&str> = candidate.split(':').take(6).collect();
    if target.len() != 6
        || candidate.len() != 6
        || target
            .iter()
            .chain(&candidate)
            .any(|value| value.is_empty())
        || target[5] != candidate[5]
    {
        return 0.0;
    }
    ordinal_weight(
        target[0],
        candidate[0],
        &["morning", "midday", "afternoon", "evening", "late"],
    ) * ordinal_weight(target[1], candidate[1], &["first", "middle", "late"])
        * ordinal_weight(target[2], candidate[2], &["short", "medium", "long"])
        * ordinal_weight(target[3], candidate[3], &["low", "normal", "high"])
        * if target[4] == candidate[4] {
            1.0
        } else if target[4] == "unknown" || candidate[4] == "unknown" {
            0.8
        } else {
            ordinal_weight(target[4], candidate[4], &["low", "normal", "high"])
        }
}

fn ordinal_weight(target: &str, candidate: &str, buckets: &[&str]) -> f64 {
    if target == candidate {
        return 1.0;
    }
    match (
        buckets.iter().position(|bucket| *bucket == target),
        buckets.iter().position(|bucket| *bucket == candidate),
    ) {
        (Some(left), Some(right)) if left.abs_diff(right) == 1 => 0.6,
        _ => 0.0,
    }
}
