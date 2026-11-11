//! Effort and Fast mode changes inside a running Claude session.

use super::init_gate::{GateHarness, internal_status, turn_request, written_subtypes};
use crate::claude::protocol::*;
use crate::models::*;
use serde_json::{Value, json};

fn effort(value: &str) -> ModelOptionSelection {
    ModelOptionSelection {
        key: "effort".to_string(),
        value: ModelOptionValue::Choice(value.to_string()),
    }
}

fn fast_mode(value: bool) -> ModelOptionSelection {
    ModelOptionSelection {
        key: "fastMode".to_string(),
        value: ModelOptionValue::Boolean(value),
    }
}

fn written_settings(lines: &[Value]) -> Vec<Value> {
    lines
        .iter()
        .filter(|line| {
            line.pointer("/request/subtype").and_then(Value::as_str) == Some("apply_flag_settings")
        })
        .filter_map(|line| line.pointer("/request/settings").cloned())
        .collect()
}

/// Starts a direct session with `start_options`, sends one turn with `turn_options`, and
/// returns every line the driver wrote up to the user message.
async fn first_turn(
    start_options: Vec<ModelOptionSelection>,
    turn_options: Vec<ModelOptionSelection>,
) -> (ProviderSessionId, ProviderSessionId, Vec<Value>) {
    let mut harness = GateHarness::new(false, internal_status("connected"));
    let mut start = harness.start_request();
    start.model_options = start_options;
    let session = harness.start_with(start).await.unwrap();
    let mut turn = turn_request(&session.session_id, "turn-1");
    turn.model_options = turn_options;
    let receipt = harness.send(turn).await.unwrap();
    assert_eq!(receipt.state, ChatTurnState::Active);
    let mut seen = Vec::new();
    harness.claude().wait_for("user", &mut seen).await;
    let started_in = harness
        .events()
        .iter()
        .filter_map(|event| match &event.event {
            crate::events::CanonicalEvent::SessionStarted(started) => {
                Some(started.session_id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(started_in.len(), 1, "the session never restarts");
    (session.session_id, started_in[0].clone(), seen)
}

#[test]
fn apply_flag_settings_changes_effort_without_restart() {
    crate::test_block_on(async {
        let (session_id, started_id, seen) =
            first_turn(vec![effort("high"), fast_mode(false)], vec![effort("low")]).await;
        assert_eq!(session_id, started_id);
        assert_eq!(
            written_subtypes(&seen),
            ["initialize", "set_permission_mode", "apply_flag_settings"]
        );
        assert_eq!(
            written_settings(&seen),
            [json!({ "effortLevel": "low", "fastMode": false })],
            "the unchanged Fast mode keeps its live value"
        );
    });
}

#[test]
fn apply_flag_settings_changes_fast_mode_and_keeps_effort() {
    crate::test_block_on(async {
        let (_, _, seen) = first_turn(vec![effort("max")], vec![fast_mode(true)]).await;
        assert_eq!(
            written_settings(&seen),
            [json!({ "effortLevel": "max", "fastMode": true })]
        );
        let (_, _, seen) = first_turn(Vec::new(), vec![fast_mode(true)]).await;
        assert_eq!(
            written_settings(&seen),
            [json!({ "effortLevel": null, "fastMode": true })]
        );
    });
}

#[test]
fn unchanged_options_send_no_flag_settings() {
    crate::test_block_on(async {
        for turn_options in [Vec::new(), vec![effort("high"), fast_mode(false)]] {
            let (_, _, seen) =
                first_turn(vec![effort("high"), fast_mode(false)], turn_options).await;
            assert_eq!(
                written_subtypes(&seen),
                ["initialize", "set_permission_mode"]
            );
        }
    });
}

#[test]
fn apply_flag_settings_payload_checks_effort() {
    assert_eq!(
        apply_flag_settings_payload(Some("xhigh"), None).unwrap(),
        json!({ "settings": { "effortLevel": "xhigh", "fastMode": null } })
    );
    for level in CLAUDE_EFFORT_LEVELS {
        assert!(apply_flag_settings_payload(Some(level), Some(true)).is_ok());
    }
    for invalid in ["", "HIGH", "ultra"] {
        let error = apply_flag_settings_payload(Some(invalid), None).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation, "{invalid:?}");
    }
}
