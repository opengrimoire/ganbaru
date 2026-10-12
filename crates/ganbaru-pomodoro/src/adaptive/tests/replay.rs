use serde::Deserialize;

use crate::adaptive::models::{LocalTimeFact, LocalTimeFacts};
use crate::adaptive::replay::models::{
    BoundedCandidate, CandidateWorkflow, GateOptions, Interaction, ObservedOutcome,
    PolicyEvaluation, ReplayOpportunity,
};
use crate::adaptive::replay::{evaluate, select_for_context};
use crate::adaptive::tests::{fixture, state_near};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    opportunities: Vec<ReplayOpportunity>,
    candidates: Vec<BoundedCandidate>,
    outcomes: Vec<ObservedOutcome>,
    options: GateOptions,
    facts: Vec<LocalTimeFact>,
    expected: CandidateWorkflow,
    context_key: String,
    selected: Option<String>,
}

#[test]
fn replay_gate_workflows_match_shared_fixtures() {
    let cases: Vec<Case> = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fixture(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/adaptive-replay-parity.json"
        ))));
    assert_eq!(cases.len(), 30);
    for case in cases {
        let facts: LocalTimeFacts = case
            .facts
            .into_iter()
            .map(|fact| (fact.epoch_ms, fact))
            .collect();
        let mut result = evaluate(
            &case.opportunities,
            &case.candidates,
            &case.outcomes,
            &case.options,
            &facts,
        )
        .unwrap_or_else(|error| panic!("{}: {error}", case.name));
        normalize(&mut result, &case.expected);
        assert_eq!(result, case.expected, "{}", case.name);
        assert_eq!(
            select_for_context(&result, &case.context_key)
                .map(|review| review.candidate_id.clone()),
            case.selected,
            "{}",
            case.name
        );
    }
}

fn normalize(actual: &mut CandidateWorkflow, expected: &CandidateWorkflow) {
    for (a, b) in actual.evaluations.iter_mut().zip(&expected.evaluations) {
        normalize_evaluation(a, b);
    }
    for (a, b) in actual
        .context_reports
        .iter_mut()
        .zip(&expected.context_reports)
    {
        for (a, b) in a.evaluations.iter_mut().zip(&b.evaluations) {
            normalize_evaluation(&mut a.evaluation, &b.evaluation);
        }
    }
    for (a, b) in actual.interactions.iter_mut().zip(&expected.interactions) {
        normalize_interaction(a, b);
    }
    for (a, b) in actual.reviews.iter_mut().zip(&expected.reviews) {
        normalize_evaluation(&mut a.evaluation, &b.evaluation);
        if let (Some(a), Some(b)) = (&mut a.interaction, &b.interaction) {
            normalize_interaction(a, b);
        }
    }
}

fn normalize_interaction(actual: &mut Interaction, expected: &Interaction) {
    normalize_evaluation(
        &mut actual.combined_evaluation,
        &expected.combined_evaluation,
    );
    for (a, b) in actual
        .component_evaluations
        .iter_mut()
        .zip(&expected.component_evaluations)
    {
        normalize_evaluation(a, b);
    }
    if let (Some(a), Some(b)) = (
        &mut actual.best_component_evaluation,
        &expected.best_component_evaluation,
    ) {
        normalize_evaluation(a, b);
    }
}

fn normalize_evaluation(actual: &mut PolicyEvaluation, expected: &PolicyEvaluation) {
    for (a, b) in actual.results.iter_mut().zip(&expected.results) {
        state_near(a.decision.state_scores, b.decision.state_scores);
        a.decision.state_scores = b.decision.state_scores;
    }
}
