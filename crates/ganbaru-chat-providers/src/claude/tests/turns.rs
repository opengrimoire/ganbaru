//! Claude non-streamed text, plan identity, and failed result tests.

use super::streams::{active_state, normalize_lines, normalizer};
use crate::claude::normalizer::ClaudeRouteState;
use crate::claude::session::{claims_routed_plan, route_control_request};
use crate::events::{CanonicalEvent, CanonicalRuntimeEvent};
use crate::models::*;
use serde_json::{Value, json};

const STREAM_SCOPES: &str = include_str!("../fixtures/stream-scopes.jsonl");

fn assistant_texts(events: &[CanonicalRuntimeEvent]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|event| match &event.event {
            CanonicalEvent::ContentDelta(delta)
                if delta.stream_kind == ContentStreamKind::AssistantText =>
            {
                Some((delta.item_id.clone(), delta.delta.clone()))
            }
            _ => None,
        })
        .collect()
}

fn normalize(state: &mut ClaudeRouteState, values: Vec<Value>) -> Vec<CanonicalRuntimeEvent> {
    let normalizer = normalizer();
    values
        .into_iter()
        .flat_map(|value| normalizer.normalize_message(state, value).unwrap())
        .collect()
}

fn assistant(message_id: Option<&str>, model: &str, content: Value) -> Value {
    let mut message = json!({ "role": "assistant", "model": model, "content": content });
    if let Some(id) = message_id {
        message["id"] = json!(id);
    }
    json!({ "type": "assistant", "parent_tool_use_id": null, "message": message })
}

fn plan_message(tool_use_id: &str) -> Value {
    assistant(
        Some("msg_plan"),
        "claude-sonnet-4-5",
        json!([{ "type": "tool_use", "id": tool_use_id, "name": "ExitPlanMode",
            "input": { "plan": "## Plan\n\n1. Step." } }]),
    )
}

fn plan_request(tool_use_id: Option<&str>) -> Value {
    let mut request = json!({
        "subtype": "can_use_tool",
        "tool_name": "ExitPlanMode",
        "input": { "plan": "## Plan\n\n1. Step." }
    });
    if let Some(id) = tool_use_id {
        request["tool_use_id"] = json!(id);
    }
    request
}

fn plan_ids(events: &[CanonicalRuntimeEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match &event.event {
            CanonicalEvent::ProposedPlanCompleted(plan) => Some(plan.plan_id.clone()),
            _ => None,
        })
        .collect()
}

fn failed_result(extra: Value) -> Value {
    let mut result = json!({
        "type": "result", "subtype": "error_during_execution", "is_error": true,
        "session_id": "11111111-1111-4111-8111-111111111111", "errors": []
    });
    for (key, value) in extra.as_object().unwrap() {
        result[key] = value.clone();
    }
    result
}

fn abort_reason(events: &[CanonicalRuntimeEvent]) -> (ChatTurnState, String) {
    events
        .iter()
        .find_map(|event| match &event.event {
            CanonicalEvent::TurnAborted(aborted) => Some((aborted.state, aborted.reason.clone())),
            _ => None,
        })
        .expect("a failed result aborts the turn")
}

#[test]
fn streamed_text_is_not_repeated_by_its_assistant_frames() {
    let normalizer = normalizer();
    let mut state = active_state();
    let events = normalize_lines(&normalizer, &mut state, STREAM_SCOPES);
    let texts = assistant_texts(&events);
    assert_eq!(
        texts
            .iter()
            .filter(|(_, text)| text.as_str() == "Final answer")
            .count(),
        1
    );
    assert!(
        texts.iter().all(|(_, text)| text != "Root text"),
        "the streamed deltas are the only copy"
    );
}

#[test]
fn synthetic_notices_and_unstreamed_frames_appear_once_per_block() {
    let mut state = active_state();
    let events = normalize(
        &mut state,
        vec![
            assistant(
                None,
                "<synthetic>",
                json!([{ "type": "text", "text": "API Error: overloaded" }]),
            ),
            assistant(
                Some("msg_plain"),
                "claude-sonnet-4-5",
                json!([{ "type": "text", "text": "First block" }]),
            ),
            assistant(
                Some("msg_plain"),
                "claude-sonnet-4-5",
                json!([{ "type": "tool_use", "id": "toolu_read", "name": "Read", "input": {} }]),
            ),
            assistant(
                Some("msg_plain"),
                "claude-sonnet-4-5",
                json!([{ "type": "text", "text": "Second block" }, { "type": "text", "text": "" }]),
            ),
        ],
    );
    let texts = assistant_texts(&events);
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert_eq!(texts[0].1, "API Error: overloaded");
    assert!(texts[0].0.starts_with("session-1:assistant:"));
    assert_eq!(
        texts[1],
        (
            "session-1:msg_plain:0".to_string(),
            "First block".to_string()
        )
    );
    assert_eq!(
        texts[2],
        (
            "session-1:msg_plain:1".to_string(),
            "Second block".to_string()
        )
    );
    for (item_id, _) in &texts {
        let completed = events.iter().any(|event| {
            matches!(&event.event, CanonicalEvent::ItemCompleted(item)
                if &item.item_id == item_id && item.kind == CanonicalItemKind::AssistantMessage)
        });
        assert!(completed, "{item_id} is a completed assistant item");
    }
}

#[test]
fn successful_result_text_is_never_a_second_message() {
    let normalizer = normalizer();
    let mut state = active_state();
    let events = normalize_lines(&normalizer, &mut state, STREAM_SCOPES);
    let last_result = events
        .iter()
        .rposition(|event| matches!(event.event, CanonicalEvent::TurnCompleted(_)))
        .unwrap();
    assert!(
        assistant_texts(&events[last_result..]).is_empty(),
        "the result repeats the final assistant text"
    );
}

#[test]
fn exit_plan_mode_projects_one_plan_when_the_message_arrives_first() {
    let mut state = active_state();
    let events = normalize(&mut state, vec![plan_message("toolu_plan")]);
    assert_eq!(plan_ids(&events), ["toolu_plan"]);

    let routed = route_control_request(
        "control-1",
        plan_request(Some("toolu_plan")).as_object().unwrap(),
    )
    .unwrap();
    assert!(
        routed.immediate_response.is_some(),
        "the deny is always sent"
    );
    assert!(!claims_routed_plan(&mut state, &routed.event, "control-1"));
}

#[test]
fn exit_plan_mode_projects_one_plan_when_the_request_arrives_first() {
    let mut state = active_state();
    let routed = route_control_request(
        "control-1",
        plan_request(Some("toolu_plan")).as_object().unwrap(),
    )
    .unwrap();
    let CanonicalEvent::ProposedPlanCompleted(plan) = &routed.event else {
        panic!("expected a proposed plan");
    };
    assert_eq!(plan.plan_id, "toolu_plan");
    assert!(claims_routed_plan(&mut state, &routed.event, "control-1"));

    let events = normalize(&mut state, vec![plan_message("toolu_plan")]);
    assert!(plan_ids(&events).is_empty());
    let next = normalize(&mut state, vec![plan_message("toolu_other_plan")]);
    assert_eq!(plan_ids(&next), ["toolu_other_plan"]);
}

#[test]
fn exit_plan_mode_request_without_a_tool_use_id_keeps_its_request_id() {
    let mut state = active_state();
    let routed =
        route_control_request("control-7", plan_request(None).as_object().unwrap()).unwrap();
    let CanonicalEvent::ProposedPlanCompleted(plan) = &routed.event else {
        panic!("expected a proposed plan");
    };
    assert_eq!(plan.plan_id, "control-7");
    assert!(claims_routed_plan(&mut state, &routed.event, "control-7"));
}

#[test]
fn exit_plan_mode_without_a_tool_use_id_projects_one_plan_in_either_order() {
    let mut message_first = active_state();
    let events = normalize(&mut message_first, vec![plan_message("toolu_plan")]);
    assert_eq!(plan_ids(&events), ["toolu_plan"]);
    let routed =
        route_control_request("control-7", plan_request(None).as_object().unwrap()).unwrap();
    assert!(
        routed.immediate_response.is_some(),
        "the deny is always sent"
    );
    assert!(!claims_routed_plan(
        &mut message_first,
        &routed.event,
        "control-7"
    ));

    let mut request_first = active_state();
    let routed =
        route_control_request("control-8", plan_request(None).as_object().unwrap()).unwrap();
    assert!(claims_routed_plan(
        &mut request_first,
        &routed.event,
        "control-8"
    ));
    let events = normalize(&mut request_first, vec![plan_message("toolu_plan")]);
    assert!(plan_ids(&events).is_empty());
    let next = normalize(&mut request_first, vec![plan_message("toolu_next_plan")]);
    assert_eq!(
        plan_ids(&next),
        ["toolu_next_plan"],
        "one request pairs with one message"
    );
}

#[test]
fn exit_plan_mode_without_a_tool_use_id_keeps_a_different_plan() {
    let mut state = active_state();
    normalize(&mut state, vec![plan_message("toolu_plan")]);
    let mut request = plan_request(None);
    request["input"]["plan"] = json!("## Revised plan");
    let routed = route_control_request("control-9", request.as_object().unwrap()).unwrap();
    assert!(claims_routed_plan(&mut state, &routed.event, "control-9"));
}

#[test]
fn failed_results_keep_the_most_specific_reason() {
    let mut state = active_state();
    let with_text = normalize(
        &mut state,
        vec![failed_result(
            json!({ "result": "Credit balance is too low" }),
        )],
    );
    assert_eq!(
        abort_reason(&with_text),
        (
            ChatTurnState::Failed,
            "Credit balance is too low".to_string()
        )
    );

    state.active_chat_turn_id = Some(ChatTurnId::new("turn-2".to_string()).unwrap());
    let with_error = normalize(
        &mut state,
        vec![failed_result(json!({
            "errors": ["Prompt is too long"], "result": "Ignored text"
        }))],
    );
    assert_eq!(abort_reason(&with_error).1, "Prompt is too long");

    state.active_chat_turn_id = Some(ChatTurnId::new("turn-3".to_string()).unwrap());
    let bare = normalize(&mut state, vec![failed_result(json!({ "result": " " }))]);
    assert_eq!(
        abort_reason(&bare).1,
        "Claude turn failed: error_during_execution"
    );
}

#[test]
fn result_text_mentioning_an_interrupt_does_not_mark_the_turn_interrupted() {
    let mut state = active_state();
    let events = normalize(
        &mut state,
        vec![failed_result(
            json!({ "result": "Tests failed after I interrupted the watcher" }),
        )],
    );
    assert_eq!(abort_reason(&events).0, ChatTurnState::Failed);
}

#[test]
fn a_result_completes_text_the_turn_left_open() {
    let mut state = active_state();
    let events = normalize(
        &mut state,
        vec![
            json!({ "type": "stream_event", "event": {
                "type": "message_start", "message": { "id": "msg_open" } } }),
            json!({ "type": "stream_event", "event": {
                "type": "content_block_start", "index": 0,
                "content_block": { "type": "text", "text": "" } } }),
            failed_result(json!({ "result": "Stream ended" })),
        ],
    );
    let completion = events
        .iter()
        .position(|event| {
            matches!(&event.event, CanonicalEvent::ItemCompleted(item)
                if item.item_id == "session-1:msg_open:0")
        })
        .expect("the open block completes");
    let aborted = events
        .iter()
        .position(|event| matches!(event.event, CanonicalEvent::TurnAborted(_)))
        .unwrap();
    assert!(completion < aborted);
    assert_eq!(
        events[completion].turn_id.as_ref().map(ChatTurnId::as_str),
        Some("turn-1")
    );
}
