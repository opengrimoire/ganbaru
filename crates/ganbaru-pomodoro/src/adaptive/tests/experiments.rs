use chrono::DateTime;
use serde::Deserialize;

use crate::adaptive::experiments::analysis::analyze_experiment;
use crate::adaptive::experiments::models::{
    ExperimentAnalysis, ExperimentAssignment, ExperimentLane, ExperimentOutcome, ExperimentState,
    SelectionInput,
};
use crate::adaptive::experiments::selection::{
    experiment_cooldown_state, select_run_start_assignment, stable_variant_index,
};
use crate::adaptive::models::{LocalTimeFact, LocalTimeFacts};

#[derive(Deserialize)]
struct Fixture {
    selection: Vec<SelectionCase>,
    analysis: Vec<AnalysisCase>,
    cooldown: Vec<CooldownCase>,
    hashes: Vec<HashCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectionCase {
    name: String,
    input: SelectionInput,
    local_date_string: String,
    expected: Option<ExperimentAssignment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisCase {
    name: String,
    lane: ExperimentLane,
    outcomes: Vec<ExperimentOutcome>,
    context_key: String,
    expected: ExperimentAnalysis,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CooldownCase {
    states: Vec<ExperimentState>,
    experiment_id: String,
    occurred_at: String,
    expected: Option<ExperimentState>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HashCase {
    seed: String,
    variant_count: usize,
    expected: usize,
}

#[test]
fn matches_shared_seven_lane_experiment_selection_analysis_and_hash_fixtures() {
    let fixture: Fixture = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(crate::adaptive::tests::fixture(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/adaptive-experiment-parity.json"
        ))));
    assert_eq!(
        (
            fixture.selection.len(),
            fixture.analysis.len(),
            fixture.hashes.len()
        ),
        (133, 112, 36)
    );
    for case in fixture.selection {
        let epoch_ms = DateTime::parse_from_rfc3339(&case.input.occurred_at)
            .unwrap()
            .timestamp_millis();
        let facts = LocalTimeFacts::from([(
            epoch_ms,
            LocalTimeFact {
                epoch_ms,
                date_key: "2026-06-10".to_owned(),
                date_string: case.local_date_string,
                hour: 3,
            },
        )]);
        assert_eq!(
            select_run_start_assignment(&case.input, &facts).unwrap(),
            case.expected,
            "{}",
            case.name
        );
    }
    for case in fixture.analysis {
        let actual = analyze_experiment(case.lane, &case.outcomes, Some(&case.context_key));
        compare_analysis(actual, case.expected, &case.name);
    }
    for case in fixture.cooldown {
        assert_eq!(
            experiment_cooldown_state(&case.states, &case.experiment_id, &case.occurred_at),
            case.expected.as_ref()
        );
    }
    for case in fixture.hashes {
        assert_eq!(
            stable_variant_index(&case.seed, case.variant_count),
            case.expected,
            "seed {}",
            case.seed
        );
    }
}

fn compare_analysis(actual: ExperimentAnalysis, expected: ExperimentAnalysis, name: &str) {
    assert_eq!(
        (
            &actual.experiment_id,
            &actual.control_variant_key,
            &actual.treatment_variant_key,
            &actual.analysis_scope,
            actual.guardrail_breached,
            &actual.decision
        ),
        (
            &expected.experiment_id,
            &expected.control_variant_key,
            &expected.treatment_variant_key,
            &expected.analysis_scope,
            expected.guardrail_breached,
            &expected.decision
        ),
        "{name}"
    );
    macro_rules! means { ($($field:ident),+ $(,)?) => { $(crate::adaptive::tests::near(actual.$field, expected.$field, &format!("{name}: {}", stringify!($field)));)+ }; }
    means!(
        control_observed_runs,
        treatment_observed_runs,
        control_completion_rate,
        treatment_completion_rate,
        control_stop_rate,
        treatment_stop_rate,
        control_clean_focus_seconds_mean,
        treatment_clean_focus_seconds_mean,
        control_blocked_attempts_mean,
        treatment_blocked_attempts_mean,
        control_break_skipped_mean,
        treatment_break_skipped_mean,
        control_short_break_overtime_seconds_mean,
        treatment_short_break_overtime_seconds_mean,
        control_long_break_overtime_seconds_mean,
        treatment_long_break_overtime_seconds_mean
    );
    macro_rules! optional_means { ($($field:ident),+ $(,)?) => { $(match (actual.$field, expected.$field) {
        (Some(actual), Some(expected)) => crate::adaptive::tests::near(actual, expected, &format!("{name}: {}", stringify!($field))),
        (actual, expected) => assert_eq!(actual, expected, "{name}: {}", stringify!($field)),
    })+ }; }
    optional_means!(
        control_day_missed_planned_pomodoro_mean,
        treatment_day_missed_planned_pomodoro_mean,
        control_day_blocked_attempts_mean,
        treatment_day_blocked_attempts_mean,
        control_next_day_started_rate,
        treatment_next_day_started_rate
    );
}
