use serde::Deserialize;

use crate::adaptive::decision::{
    AdaptiveDecision, AdaptiveDecisionInput, decide_boundary, decide_run_start,
};
use crate::adaptive::models::{LocalTimeFact, LocalTimeFacts, StateScores};
use crate::adaptive::snapshots::{SnapshotIds, decision_envelope, run_start_snapshot};
use crate::adaptive::tests::{fixture, state_near};
use crate::{
    PomodoroAdaptiveDecisionEnvelopeWrite, PomodoroAdaptiveStateScoresWrite,
    PomodoroRunAdaptiveSnapshotWrite,
};

#[derive(Deserialize)]
struct DecisionCase {
    name: String,
    input: AdaptiveDecisionInput,
    facts: Vec<LocalTimeFact>,
    expected: AdaptiveDecision,
    snapshot: Option<PomodoroRunAdaptiveSnapshotWrite>,
    boundary: AdaptiveDecision,
    envelope: Option<PomodoroAdaptiveDecisionEnvelopeWrite>,
}

fn ids() -> SnapshotIds {
    SnapshotIds {
        run: "run".to_owned(),
        segment: "segment".to_owned(),
        context: "00000000-0000-4000-8000-000000000001".to_owned(),
        decision: "00000000-0000-4000-8000-000000000002".to_owned(),
        assignment: "00000000-0000-4000-8000-000000000003".to_owned(),
    }
}

fn scores(value: &PomodoroAdaptiveStateScoresWrite) -> StateScores {
    StateScores {
        readiness: value.readiness,
        strain: value.strain,
        recovery_debt: value.recovery_debt,
        avoidance_pressure: value.avoidance_pressure,
        momentum: value.momentum,
        confidence: value.confidence,
    }
}

fn normalize_decision(
    mut actual: AdaptiveDecision,
    expected: &AdaptiveDecision,
) -> AdaptiveDecision {
    state_near(actual.state_scores, expected.state_scores);
    actual.state_scores = expected.state_scores;
    actual
}

#[test]
fn matches_complete_typescript_decisions_and_persisted_snapshots_for_all_seven_lanes() {
    let cases: Vec<DecisionCase> = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fixture(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/adaptive-decision-parity.json"
        ))));
    assert_eq!(cases.len(), 153);
    let mut assignments = 0;
    let mut guardrails = 0;
    let mut updates = 0;
    let mut snapshots = 0;
    for case in cases {
        let facts: LocalTimeFacts = case
            .facts
            .into_iter()
            .map(|fact| (fact.epoch_ms, fact))
            .collect();
        let actual = decide_run_start(&case.input, &facts).unwrap();
        assignments += usize::from(actual.experiment_assignment.is_some());
        guardrails += usize::from(actual.decision_mode == "guardrail");
        updates += usize::from(actual.experiment_update.is_some());
        assert_eq!(
            normalize_decision(actual.clone(), &case.expected),
            case.expected,
            "{}",
            case.name
        );
        if let Some(expected) = case.snapshot {
            snapshots += 1;
            let mut snapshot = run_start_snapshot(&actual, ids(), Vec::new());
            state_near(
                scores(&snapshot.decision.state_scores),
                scores(&expected.decision.state_scores),
            );
            snapshot.decision.state_scores = expected.decision.state_scores.clone();
            assert_eq!(snapshot, expected, "run snapshot: {}", case.name);
        }
        let actual = decide_boundary(&case.input, &facts).unwrap();
        assert_eq!(
            normalize_decision(actual.clone(), &case.boundary),
            case.boundary,
            "boundary: {}",
            case.name
        );
        if let Some(expected) = case.envelope {
            let mut envelope = decision_envelope(&actual, ids(), "focus_start");
            state_near(
                scores(&envelope.decision.state_scores),
                scores(&expected.decision.state_scores),
            );
            envelope.decision.state_scores = expected.decision.state_scores.clone();
            assert_eq!(envelope, expected, "boundary snapshot: {}", case.name);
        }
    }
    assert!(assignments > 0 && guardrails > 0 && updates > 0);
    assert_eq!(snapshots, 9);
}
