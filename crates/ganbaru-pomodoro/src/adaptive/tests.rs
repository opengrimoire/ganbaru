mod decision;
mod experiments;
mod replay;

use serde::Deserialize;
use sqlx::types::Json;

use crate::adaptive::features::{derive_context_bucket, extract_adaptive_features};
use crate::adaptive::models::{
    ContextBucket, CountRhythm, FeatureInput, FeatureVector, PolicyDecision, StateScores,
};
use crate::adaptive::policy::select_adaptive_rhythm;
use crate::adaptive::state::derive_adaptive_state;
use crate::adaptive::statistics::{self, MeanVarianceEstimate, RateEstimate};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BaseFixture {
    features: Vec<FeatureCase>,
    policies: Vec<PolicyCase>,
    states: Vec<StateCase>,
    local_time: Vec<LocalTimeCase>,
    rates: Vec<RateCase>,
    means: Vec<MeanCase>,
}

#[derive(Deserialize)]
struct FeatureCase {
    name: String,
    input: FeatureInput,
    expected: FeatureVector,
}

#[derive(Deserialize)]
struct PolicyCase {
    name: String,
    input: PolicyInput,
    expected: PolicyDecision,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyInput {
    current_rhythm: CountRhythm,
    features: FeatureVector,
    state: StateScores,
    context: ContextBucket,
}

#[derive(Deserialize)]
struct StateCase {
    features: FeatureVector,
    previous: Option<StateScores>,
    expected: StateScores,
}

#[derive(Deserialize)]
struct LocalTimeCase {
    zone: String,
    facts: Vec<LocalFactCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalFactCase {
    epoch_ms: i64,
    date_key: String,
    date_string: String,
    hour: u8,
    context: ContextBucket,
}

#[derive(Deserialize)]
struct RateCase {
    successes: f64,
    trials: f64,
    expected: RateEstimate,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MeanCase {
    total: f64,
    square_total: f64,
    count: f64,
    expected: MeanVarianceEstimate,
}

pub(super) fn near(actual: f64, expected: f64, label: &str) {
    let tolerance = 1e-12 * expected.abs().max(1.0);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{label}: actual {actual}, expected {expected}"
    );
}

pub(super) fn state_near(actual: StateScores, expected: StateScores) {
    near(actual.readiness, expected.readiness, "readiness");
    near(actual.strain, expected.strain, "strain");
    near(
        actual.recovery_debt,
        expected.recovery_debt,
        "recovery debt",
    );
    near(
        actual.avoidance_pressure,
        expected.avoidance_pressure,
        "avoidance pressure",
    );
    near(actual.momentum, expected.momentum, "momentum");
    near(actual.confidence, expected.confidence, "confidence");
}

/// Deserialize a checked-in parity fixture through the SQLx JSON codec instead of a
/// separate fixture codec.
pub(super) async fn fixture<T: serde::de::DeserializeOwned + Send + Unpin + 'static>(
    source: &str,
) -> T {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let result: Json<T> = sqlx::query_scalar("SELECT ?")
        .bind(source)
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    result.0
}

#[test]
fn matches_shared_typescript_feature_state_policy_and_statistics_fixtures() {
    let fixture: BaseFixture = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fixture(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/adaptive-base-parity.json"
        ))));
    assert_eq!(
        (
            fixture.features.len(),
            fixture.policies.len(),
            fixture.states.len()
        ),
        (11, 26, 74)
    );
    for case in fixture.features {
        assert_eq!(
            extract_adaptive_features(&case.input),
            case.expected,
            "feature case: {}",
            case.name
        );
    }
    for case in fixture.policies {
        let actual = select_adaptive_rhythm(
            case.input.current_rhythm,
            &case.input.features,
            case.input.state,
            &case.input.context,
        );
        assert_eq!(
            actual.selected_rhythm, case.expected.selected_rhythm,
            "policy rhythm: {}",
            case.name
        );
        assert_eq!(
            actual.mode, case.expected.mode,
            "policy mode: {}",
            case.name
        );
        assert_eq!(
            actual.reason_codes, case.expected.reason_codes,
            "policy reasons: {}",
            case.name
        );
        state_near(actual.state_scores, case.expected.state_scores);
    }
    for case in fixture.states {
        state_near(
            derive_adaptive_state(&case.features, case.previous),
            case.expected,
        );
    }
    for case in fixture.local_time {
        for fact in case.facts {
            assert!(
                fact.epoch_ms > 0 && fact.date_key.len() == 10 && fact.date_string.len() == 15,
                "{}",
                case.zone
            );
            assert_eq!(
                derive_context_bucket(fact.hour, 60.0, 1, 0.0, None, None),
                fact.context,
                "{} at {}",
                case.zone,
                fact.epoch_ms
            );
        }
    }
    for case in fixture.rates {
        let actual = statistics::estimate_rate(
            case.successes,
            case.trials,
            statistics::DEFAULT_CONFIDENCE_Z,
        );
        near(actual.successes, case.expected.successes, "rate successes");
        near(actual.trials, case.expected.trials, "rate trials");
        near(actual.point, case.expected.point, "rate point");
        near(
            actual.lower_bound,
            case.expected.lower_bound,
            "rate lower bound",
        );
        near(
            actual.upper_bound,
            case.expected.upper_bound,
            "rate upper bound",
        );
        near(
            actual.standard_error,
            case.expected.standard_error,
            "rate standard error",
        );
    }
    for case in fixture.means {
        let actual = statistics::estimate_mean_with_variance(
            case.total,
            case.square_total,
            case.count,
            statistics::DEFAULT_CONFIDENCE_Z,
        );
        near(actual.total, case.expected.total, "mean total");
        near(actual.count, case.expected.count, "mean count");
        near(actual.point, case.expected.point, "mean point");
        near(
            actual.square_total,
            case.expected.square_total,
            "mean square total",
        );
        near(
            actual.sample_variance,
            case.expected.sample_variance,
            "mean sample variance",
        );
        near(
            actual.standard_error,
            case.expected.standard_error,
            "mean standard error",
        );
        near(
            actual.lower_bound,
            case.expected.lower_bound,
            "mean lower bound",
        );
        near(
            actual.upper_bound,
            case.expected.upper_bound,
            "mean upper bound",
        );
    }
}
