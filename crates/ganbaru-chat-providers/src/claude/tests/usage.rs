//! Claude token usage and context fill tests.

use super::streams::{active_state, normalize_lines, normalizer};
use crate::claude::normalizer::ClaudeRouteState;
use crate::events::*;
use crate::models::*;
use serde_json::{Value, json};

const STREAM_SCOPES: &str = include_str!("../fixtures/stream-scopes.jsonl");

fn usage_events(events: &[CanonicalRuntimeEvent]) -> Vec<&ThreadUsageUpdatedEvent> {
    events
        .iter()
        .filter_map(|event| match &event.event {
            CanonicalEvent::ThreadUsageUpdated(usage) => Some(usage),
            _ => None,
        })
        .collect()
}

fn completed_usage(events: &[CanonicalRuntimeEvent]) -> Option<&ThreadUsageUpdatedEvent> {
    events.iter().find_map(|event| match &event.event {
        CanonicalEvent::TurnCompleted(completed) => completed.usage.as_ref(),
        _ => None,
    })
}

fn result(model_usage: Value) -> Value {
    json!({
        "type": "result", "subtype": "success", "is_error": false, "result": "Done",
        "usage": { "input_tokens": 7, "cache_creation_input_tokens": 3,
            "cache_read_input_tokens": 90, "output_tokens": 5 },
        "modelUsage": model_usage
    })
}

fn assistant(parent: Value, message: Value) -> Value {
    json!({ "type": "assistant", "parent_tool_use_id": parent, "message": message })
}

fn root_start(id: &str, model: &str, usage: Value) -> Value {
    json!({
        "type": "stream_event", "parent_tool_use_id": null,
        "event": { "type": "message_start", "message": { "id": id, "model": model, "usage": usage } }
    })
}

fn run(state: &mut ClaudeRouteState, messages: Vec<Value>) -> Vec<CanonicalRuntimeEvent> {
    let normalizer = normalizer();
    messages
        .into_iter()
        .flat_map(|message| normalizer.normalize_message(state, message).unwrap())
        .collect()
}

#[test]
fn context_fill_is_the_latest_root_request_against_the_main_model_window() {
    let normalizer = normalizer();
    let mut state = active_state();
    let events = normalize_lines(&normalizer, &mut state, STREAM_SCOPES);
    let reported = usage_events(&events);
    assert_eq!(reported.len(), 1);
    let usage = reported[0];
    assert_eq!(
        usage.context_tokens,
        Some(30 + 60 + 1200 + 12),
        "msg_root_3 input and cache counts from message_start, output from message_delta"
    );
    assert_eq!(
        usage.context_limit,
        Some(200_000),
        "the subagent model's larger window is ignored"
    );
    assert_eq!(usage.input_tokens, Some(60));
    assert_eq!(usage.output_tokens, Some(64));
    assert_eq!(usage.cached_input_tokens, Some(3300));
    assert_eq!(usage.cache_write_input_tokens, Some(210));
    assert_eq!(usage.cost.as_ref().map(|cost| cost.amount), Some(0.02));
    assert_eq!(completed_usage(&events), Some(usage));
}

#[test]
fn an_aliased_route_model_still_selects_the_resolved_main_model_window() {
    let mut state = active_state();
    state.model_id = Some(ModelId::new("sonnet".to_string()).unwrap());
    let events = run(
        &mut state,
        vec![
            root_start(
                "msg_1",
                "claude-sonnet-4-5",
                json!({ "input_tokens": 4, "output_tokens": 1 }),
            ),
            result(json!({
                "claude-sonnet-4-5": { "contextWindow": 200000 },
                "claude-haiku-4-5": { "contextWindow": 1000000 }
            })),
        ],
    );
    assert_eq!(usage_events(&events)[0].context_limit, Some(200_000));
}

#[test]
fn the_route_model_or_the_largest_window_applies_when_no_request_named_a_model() {
    let mut state = active_state();
    state.model_id = Some(ModelId::new("claude-opus-4-1".to_string()).unwrap());
    let windows = json!({
        "claude-opus-4-1": { "contextWindow": 200000 },
        "claude-haiku-4-5": { "contextWindow": 1000000 }
    });
    let events = run(&mut state, vec![result(windows.clone())]);
    assert_eq!(usage_events(&events)[0].context_limit, Some(200_000));

    let mut state = active_state();
    let events = run(&mut state, vec![result(windows)]);
    let usage = usage_events(&events)[0];
    assert_eq!(usage.context_limit, Some(1_000_000));
    assert_eq!(
        usage.context_tokens, None,
        "a turn without root requests reports no fill"
    );
}

#[test]
fn an_unstreamed_root_assistant_message_supplies_the_request_usage() {
    let mut state = active_state();
    let events = run(
        &mut state,
        vec![
            assistant(
                Value::Null,
                json!({ "id": "msg_plain", "model": "claude-sonnet-4-5", "content": [],
                    "usage": { "input_tokens": 8, "cache_read_input_tokens": 400,
                        "output_tokens": 20 } }),
            ),
            result(json!({})),
        ],
    );
    assert_eq!(usage_events(&events)[0].context_tokens, Some(428));
}

#[test]
fn assistant_usage_fills_a_stream_that_reported_no_input_and_keeps_the_larger_output() {
    let mut state = active_state();
    let events = run(
        &mut state,
        vec![
            root_start("msg_1", "claude-sonnet-4-5", json!({ "output_tokens": 1 })),
            json!({ "type": "stream_event", "parent_tool_use_id": null,
                "event": { "type": "message_delta", "usage": { "output_tokens": 30 } } }),
            assistant(
                Value::Null,
                json!({ "id": "msg_1", "model": "claude-sonnet-4-5", "content": [],
                    "usage": { "input_tokens": 5, "cache_read_input_tokens": 100,
                        "output_tokens": 2 } }),
            ),
            result(json!({})),
        ],
    );
    assert_eq!(usage_events(&events)[0].context_tokens, Some(5 + 100 + 30));
}

#[test]
fn a_streamed_request_keeps_its_stream_usage_over_its_assistant_frames() {
    let mut state = active_state();
    let events = run(
        &mut state,
        vec![
            root_start(
                "msg_1",
                "claude-sonnet-4-5",
                json!({ "input_tokens": 10, "output_tokens": 1 }),
            ),
            json!({ "type": "stream_event", "parent_tool_use_id": null,
                "event": { "type": "message_delta",
                    "usage": { "input_tokens": null, "cache_read_input_tokens": 0, "output_tokens": 6 } } }),
            assistant(
                Value::Null,
                json!({ "id": "msg_1", "model": "claude-sonnet-4-5", "content": [],
                    "usage": { "input_tokens": 999, "output_tokens": 999 } }),
            ),
            result(json!({})),
        ],
    );
    assert_eq!(
        usage_events(&events)[0].context_tokens,
        Some(16),
        "null delta fields keep the start values and the assistant frame does not override"
    );
}

#[test]
fn subagent_and_synthetic_usage_never_move_the_context_fill() {
    let mut state = active_state();
    let events = run(
        &mut state,
        vec![
            root_start(
                "msg_root",
                "claude-sonnet-4-5",
                json!({ "input_tokens": 50, "output_tokens": 5 }),
            ),
            json!({ "type": "stream_event", "parent_tool_use_id": "toolu_task",
                "event": { "type": "message_start", "message": { "id": "msg_sub",
                    "model": "claude-haiku-4-5", "usage": { "input_tokens": 90000 } } } }),
            json!({ "type": "stream_event", "parent_tool_use_id": "toolu_task",
                "event": { "type": "message_delta", "usage": { "output_tokens": 70000 } } }),
            assistant(
                json!("toolu_task"),
                json!({ "id": "msg_sub", "model": "claude-haiku-4-5", "content": [],
                    "usage": { "input_tokens": 90000, "output_tokens": 70000 } }),
            ),
            assistant(
                Value::Null,
                json!({ "model": "<synthetic>", "content": [],
                    "usage": { "input_tokens": 0, "output_tokens": 0 } }),
            ),
            result(json!({
                "claude-sonnet-4-5": { "contextWindow": 200000 },
                "claude-haiku-4-5": { "contextWindow": 1000000 }
            })),
        ],
    );
    let usage = usage_events(&events)[0];
    assert_eq!(usage.context_tokens, Some(55));
    assert_eq!(usage.context_limit, Some(200_000));
}

#[test]
fn each_turn_reports_only_its_own_requests() {
    let mut state = active_state();
    let first = run(
        &mut state,
        vec![
            root_start("msg_1", "claude-sonnet-4-5", json!({ "input_tokens": 40 })),
            result(json!({})),
        ],
    );
    assert_eq!(usage_events(&first)[0].context_tokens, Some(40));
    state.active_chat_turn_id = Some(ChatTurnId::new("turn-2".to_string()).unwrap());
    let second = run(&mut state, vec![result(json!({}))]);
    assert_eq!(usage_events(&second)[0].context_tokens, None);
}
