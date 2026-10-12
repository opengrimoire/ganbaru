use super::*;

#[test]
fn redacted_fixture_normalizes_lifecycle_content_plan_and_unknown_events() {
    let normalizer = CodexEventNormalizer::new(
        identifier("codex-instance-1", ProviderInstanceId::new),
        identifier("chat-thread-1", ChatThreadId::new),
        identifier("session-1", ProviderSessionId::new),
    );
    let mut state = CodexRouteState::new(
        modes(SafetyMode::AskForApproval, InteractionMode::Build),
        Some(identifier("gpt-5.4", ModelId::new)),
    );
    state.active_chat_turn_id = Some(identifier("chat-turn-1", ChatTurnId::new));
    let mut events = Vec::new();
    for line in include_str!("../fixtures/notifications.jsonl").lines() {
        let envelope: Value = serde_json::from_str(line).unwrap();
        events.extend(
            normalizer
                .normalize_notification(
                    &mut state,
                    envelope["method"].as_str().unwrap(),
                    envelope["params"].clone(),
                )
                .unwrap(),
        );
    }

    let CanonicalEvent::ThreadStarted(thread_started) = &events[0].event else {
        panic!("expected thread start");
    };
    assert_eq!(
        thread_started.provider_thread_id.as_str(),
        "provider-thread-1"
    );
    assert!(thread_started.title.is_none());
    assert!(events.iter().any(|event| matches!(
        &event.event,
        CanonicalEvent::ContentDelta(delta)
            if delta.item_id == "provider-item-1"
                && delta.delta == "Hello from Codex"
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        CanonicalEvent::ContentDelta(delta)
            if delta.content_index == 2
                && delta.stream_kind == ContentStreamKind::ReasoningSummary
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        CanonicalEvent::PlanUpdated(plan) if plan.steps.len() == 2
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        CanonicalEvent::Unknown(unknown)
            if unknown.source_type == "future/redactedEvent"
    )));
    assert!(matches!(
        events.last().unwrap().event,
        CanonicalEvent::TurnCompleted(_)
    ));
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("ganbaru-codex-fixture-secret")
    );
}

#[test]
fn item_lifecycles_preserve_command_message_and_file_change_data() {
    let normalizer = CodexEventNormalizer::new(
        identifier("codex-instance-1", ProviderInstanceId::new),
        identifier("chat-thread-1", ChatThreadId::new),
        identifier("session-1", ProviderSessionId::new),
    );
    let mut state = CodexRouteState::new(
        modes(SafetyMode::AskForApproval, InteractionMode::Build),
        Some(identifier("gpt-5.4", ModelId::new)),
    );
    state.active_chat_turn_id = Some(identifier("chat-turn-1", ChatTurnId::new));

    normalizer
        .normalize_notification(
            &mut state,
            "turn/started",
            json!({
                "threadId": "provider-thread-1",
                "turn": { "id": "provider-turn-1", "status": "inProgress" }
            }),
        )
        .unwrap();
    let assistant_events = normalizer
        .normalize_notification(
            &mut state,
            "item/completed",
            json!({
                "threadId": "provider-thread-1",
                "turnId": "provider-turn-1",
                "item": {
                    "id": "answer-1",
                    "type": "agentMessage",
                    "text": "Finished",
                    "phase": "final_answer"
                }
            }),
        )
        .unwrap();
    let command_events = normalizer
        .normalize_notification(
            &mut state,
            "item/completed",
            json!({
                "threadId": "provider-thread-1",
                "turnId": "provider-turn-1",
                "item": {
                    "id": "command-1",
                    "type": "commandExecution",
                    "command": "printf 4",
                    "cwd": "/workspace",
                    "status": "completed",
                    "aggregatedOutput": "4\n",
                    "exitCode": 0,
                    "durationMs": 24
                }
            }),
        )
        .unwrap();
    let file_events = normalizer
        .normalize_notification(
            &mut state,
            "item/completed",
            json!({
                "threadId": "provider-thread-1",
                "turnId": "provider-turn-1",
                "item": {
                    "id": "file-1",
                    "type": "fileChange",
                    "status": "completed",
                    "changes": [{
                        "path": "src/main.rs",
                        "kind": "update",
                        "diff": "@@ -1 +1,2 @@\n-old\n+new\n+line"
                    }]
                }
            }),
        )
        .unwrap();
    let turn_events = normalizer
        .normalize_notification(
            &mut state,
            "turn/completed",
            json!({
                "threadId": "provider-thread-1",
                "turn": { "id": "provider-turn-1", "status": "completed" }
            }),
        )
        .unwrap();

    let CanonicalEvent::ItemCompleted(assistant) = &assistant_events[0].event else {
        panic!("expected assistant lifecycle");
    };
    assert_eq!(assistant.detail.as_deref(), Some("Finished"));
    assert_eq!(
        assistant.safe_metadata.as_ref().unwrap().value["phase"],
        "final_answer"
    );
    let CanonicalEvent::ItemCompleted(command) = &command_events[0].event else {
        panic!("expected command lifecycle");
    };
    assert_eq!(command.title.as_deref(), Some("printf 4"));
    assert_eq!(command.detail.as_deref(), Some("4\n"));
    assert_eq!(
        command.safe_metadata.as_ref().unwrap().value["cwd"],
        "/workspace"
    );
    assert_eq!(command.safe_metadata.as_ref().unwrap().value["exitCode"], 0);
    assert_eq!(
        command.safe_metadata.as_ref().unwrap().value["durationMs"],
        24
    );
    assert!(file_events.iter().any(|event| matches!(
        &event.event,
        CanonicalEvent::DiffUpdated(diff)
            if diff.files.len() == 1
                && diff.files[0].relative_path == "src/main.rs"
                && diff.files[0].additions == Some(2)
                && diff.files[0].deletions == Some(1)
    )));
    let CanonicalEvent::TurnCompleted(completed) = &turn_events[0].event else {
        panic!("expected turn completion");
    };
    assert_eq!(completed.changed_files.len(), 1);
    assert_eq!(completed.changed_files[0].relative_path, "src/main.rs");
}

#[test]
fn context_compaction_is_a_thread_level_activity() {
    let normalizer = CodexEventNormalizer::new(
        identifier("codex-instance-1", ProviderInstanceId::new),
        identifier("chat-thread-1", ChatThreadId::new),
        identifier("session-1", ProviderSessionId::new),
    );
    let mut state = CodexRouteState::new(
        modes(SafetyMode::AskForApproval, InteractionMode::Build),
        Some(identifier("gpt-5.4", ModelId::new)),
    );
    state.active_chat_turn_id = Some(identifier("compact-turn-1", ChatTurnId::new));
    state.active_provider_turn_id = Some("provider-turn-1".to_string());

    let events = normalizer
        .normalize_notification(
            &mut state,
            "item/completed",
            json!({
                "threadId": "provider-thread-1",
                "turnId": "provider-turn-1",
                "item": {
                    "id": "compaction-1",
                    "type": "contextCompaction",
                    "status": "completed"
                }
            }),
        )
        .unwrap();

    assert_eq!(events.len(), 1);
    assert!(events[0].turn_id.is_none());
    assert!(matches!(
        &events[0].event,
        CanonicalEvent::ItemCompleted(item)
            if item.kind == CanonicalItemKind::ContextCompaction
    ));
}
