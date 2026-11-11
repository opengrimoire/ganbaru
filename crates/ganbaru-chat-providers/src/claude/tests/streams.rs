//! Claude stream scope, item identity, and resume cursor tests.

use super::route_state;
use crate::claude::normalizer::{ClaudeEventNormalizer, ClaudeRouteState};
use crate::events::{CanonicalEvent, CanonicalRuntimeEvent};
use crate::models::*;
use serde_json::{Value, json};

const STREAM_SCOPES: &str = include_str!("../fixtures/stream-scopes.jsonl");

pub(super) fn normalizer() -> ClaudeEventNormalizer {
    ClaudeEventNormalizer::new(
        ProviderInstanceId::new("claude-instance-1".to_string()).unwrap(),
        ChatThreadId::new("thread-1".to_string()).unwrap(),
        ProviderSessionId::new("session-1".to_string()).unwrap(),
    )
}

pub(super) fn active_state() -> ClaudeRouteState {
    let mut state = route_state();
    state.active_chat_turn_id = Some(ChatTurnId::new("turn-1".to_string()).unwrap());
    state
}

/// Normalizes every non-control line of a JSONL transcript.
pub(super) fn normalize_lines(
    normalizer: &ClaudeEventNormalizer,
    state: &mut ClaudeRouteState,
    transcript: &str,
) -> Vec<CanonicalRuntimeEvent> {
    let mut events = Vec::new();
    for line in transcript.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).unwrap();
        if value.get("type").and_then(Value::as_str) == Some("control_request") {
            continue;
        }
        events.extend(normalizer.normalize_message(state, value).unwrap());
    }
    events
}

fn text_for(events: &[CanonicalRuntimeEvent], item_id: &str) -> String {
    events
        .iter()
        .filter_map(|event| match &event.event {
            CanonicalEvent::ContentDelta(delta) if delta.item_id == item_id => {
                Some(delta.delta.as_str())
            }
            _ => None,
        })
        .collect()
}

fn lifecycle_count(events: &[CanonicalRuntimeEvent], item_id: &str) -> (usize, usize) {
    events
        .iter()
        .fold((0, 0), |(started, completed), event| match &event.event {
            CanonicalEvent::ItemStarted(item) if item.item_id == item_id => {
                (started + 1, completed)
            }
            CanonicalEvent::ItemCompleted(item) if item.item_id == item_id => {
                (started, completed + 1)
            }
            _ => (started, completed),
        })
}

fn mentions_item(event: &CanonicalRuntimeEvent, item_id: &str) -> bool {
    match &event.event {
        CanonicalEvent::ContentDelta(delta) => delta.item_id == item_id,
        CanonicalEvent::ItemStarted(item)
        | CanonicalEvent::ItemUpdated(item)
        | CanonicalEvent::ItemCompleted(item) => item.item_id == item_id,
        _ => false,
    }
}

#[test]
fn subagent_frames_at_the_same_index_leave_root_text_intact() {
    let normalizer = normalizer();
    let mut state = active_state();
    let events = normalize_lines(&normalizer, &mut state, STREAM_SCOPES);

    assert_eq!(text_for(&events, "session-1:msg_root_1:0"), "Root text");
    assert_eq!(text_for(&events, "session-1:msg_root_3:0"), "Final answer");
    assert!(events.iter().all(|event| !matches!(
        &event.event,
        CanonicalEvent::ContentDelta(delta) if delta.stream_kind == ContentStreamKind::AssistantText
            && delta.delta.contains("Subagent")
    )));
    assert!(
        events
            .iter()
            .all(|event| !mentions_item(event, "toolu_sub_bash")),
        "subagent tool results must not open or close root items"
    );
    assert!(
        events
            .iter()
            .all(|event| !matches!(event.event, CanonicalEvent::PlanUpdated(_))),
        "a subagent checklist must not replace the root plan"
    );
}

#[test]
fn every_streamed_block_keeps_one_item_id_from_start_to_completion() {
    let normalizer = normalizer();
    let mut state = active_state();
    let events = normalize_lines(&normalizer, &mut state, STREAM_SCOPES);

    for item_id in [
        "session-1:msg_root_1:0",
        "session-1:msg_root_2:0",
        "session-1:msg_root_3:0",
    ] {
        assert_eq!(lifecycle_count(&events, item_id), (1, 1), "{item_id}");
    }
    assert_eq!(lifecycle_count(&events, "toolu_task"), (1, 1));
    let abandoned_completion = events
        .iter()
        .position(|event| {
            matches!(&event.event, CanonicalEvent::ItemCompleted(item)
                if item.item_id == "session-1:msg_root_2:0")
        })
        .unwrap();
    let next_start = events
        .iter()
        .position(|event| {
            matches!(&event.event, CanonicalEvent::ItemStarted(item)
                if item.item_id == "session-1:msg_root_3:0")
        })
        .unwrap();
    assert!(
        abandoned_completion < next_start,
        "a new root message closes the blocks the previous one left open"
    );
}

#[test]
fn resume_cursor_follows_only_root_model_messages() {
    let normalizer = normalizer();
    let mut state = active_state();
    let events = normalize_lines(&normalizer, &mut state, STREAM_SCOPES);
    assert_eq!(
        state.resume_cursor.last_assistant_uuid.as_deref(),
        Some("aaaaaaaa-0000-4000-8000-000000000003")
    );
    let cursors = events
        .iter()
        .filter_map(|event| match &event.event {
            CanonicalEvent::ThreadMetadataUpdated(update) => update
                .resume_cursor
                .as_ref()
                .and_then(|cursor| cursor.value.get("lastAssistantUuid"))
                .and_then(Value::as_str),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!cursors.contains(&"bbbbbbbb-0000-4000-8000-000000000001"));

    normalizer
        .normalize_message(
            &mut state,
            json!({
                "type": "assistant",
                "uuid": "cccccccc-0000-4000-8000-000000000001",
                "parent_tool_use_id": null,
                "message": {
                    "role": "assistant",
                    "model": "<synthetic>",
                    "content": [{ "type": "text", "text": "No response requested." }]
                }
            }),
        )
        .unwrap();
    assert_eq!(
        state.resume_cursor.last_assistant_uuid.as_deref(),
        Some("aaaaaaaa-0000-4000-8000-000000000003"),
        "a synthetic notice is not a model turn to resume at"
    );
}

#[test]
fn blocks_without_a_message_start_keep_separate_fallback_ids() {
    let normalizer = normalizer();
    let mut state = active_state();
    let block = |kind: &str, index: u64| {
        json!({ "type": "stream_event", "event": {
            "type": kind, "index": index, "content_block": { "type": "text", "text": "" },
            "delta": { "type": "text_delta", "text": "chunk" }
        }})
    };
    let mut events = Vec::new();
    for value in [
        block("content_block_start", 0),
        block("content_block_delta", 0),
        block("content_block_stop", 0),
        block("content_block_start", 0),
        block("content_block_stop", 0),
    ] {
        events.extend(normalizer.normalize_message(&mut state, value).unwrap());
    }
    let started = events
        .iter()
        .filter_map(|event| match &event.event {
            CanonicalEvent::ItemStarted(item) => Some(item.item_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(started.len(), 2);
    assert_ne!(started[0], started[1]);
    assert!(
        started
            .iter()
            .all(|id| id.starts_with("session-1:assistant:"))
    );
}

#[test]
fn oversized_or_control_message_ids_fall_back_to_session_counters() {
    let normalizer = normalizer();
    let mut state = active_state();
    let long_id = "m".repeat(1_000);
    for message_id in [long_id.as_str(), "msg\nbroken", ""] {
        let mut events = normalizer
            .normalize_message(
                &mut state,
                json!({ "type": "stream_event", "event": {
                    "type": "message_start", "message": { "id": message_id }
                }}),
            )
            .unwrap();
        events.extend(
            normalizer
                .normalize_message(
                    &mut state,
                    json!({ "type": "stream_event", "event": {
                        "type": "content_block_start", "index": 0,
                        "content_block": { "type": "text", "text": "" }
                    }}),
                )
                .unwrap(),
        );
        let CanonicalEvent::ItemStarted(item) = &events.last().unwrap().event else {
            panic!("expected a started text item");
        };
        assert!(
            item.item_id.starts_with("session-1:assistant:"),
            "{message_id:?}"
        );
    }
}
