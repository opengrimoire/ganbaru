use super::*;

#[test]
fn canonical_events_rebuild_equivalent_projections() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let modes = TurnModeSnapshot {
            safety_mode: SafetyMode::AskForApproval,
            interaction_mode: InteractionMode::Build,
        };
        append_canonical_event(
            &pool,
            canonical_request(
                "event-turn-start",
                Some("turn-1"),
                CanonicalEvent::TurnStarted(TurnStartedEvent {
                    provider_turn_id: None,
                    state: ChatTurnState::Active,
                    modes,
                    model_id: None,
                    model_options: Vec::new(),
                }),
            ),
        )
        .await
        .unwrap();
        let mut message = content_event("event-message", "Rebuild me");
        message.runtime.turn_id = Some(ChatTurnId::new("turn-1").unwrap());
        append_canonical_event(&pool, message).await.unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-activity",
                Some("turn-1"),
                CanonicalEvent::ItemStarted(ItemLifecycleEvent {
                    item_id: "activity-1".to_string(),
                    kind: CanonicalItemKind::CommandExecution,
                    status: ActivityStatus::Active,
                    title: Some("Compile".to_string()),
                    detail: None,
                    safe_metadata: Some(VersionedJson {
                        schema_version: 12,
                        value: serde_json::json!({ "future": true }),
                    }),
                }),
            ),
        )
        .await
        .unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-turn-complete",
                Some("turn-1"),
                CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                    state: ChatTurnState::Completed,
                    stop_reason: None,
                    usage: None,
                    changed_files: Vec::new(),
                }),
            ),
        )
        .await
        .unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-request",
                Some("turn-1"),
                CanonicalEvent::RequestOpened(RequestOpenedEvent {
                    request_id: ProviderRequestId::new("request-1").unwrap(),
                    kind: CanonicalRequestKind::CommandExecution,
                    title: "Approve".to_string(),
                    detail: None,
                    allowed_decisions: Vec::new(),
                    safe_payload: VersionedJson {
                        schema_version: 44,
                        value: serde_json::json!({ "future": "preserved" }),
                    },
                    subject: None,
                    session_grants: Vec::new(),
                }),
            ),
        )
        .await
        .unwrap();
        let before: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT 'message', id, normalized_markdown FROM chat_messages
             UNION ALL SELECT 'activity', id, safe_metadata_data FROM chat_activities
             UNION ALL SELECT 'turn', id, state FROM chat_turns
             UNION ALL SELECT 'request', id, safe_display_data FROM chat_pending_requests
             ORDER BY 1, 2",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rebuild_thread_projections(&pool, &ChatThreadId::new("thread-1").unwrap())
                .await
                .unwrap(),
            5
        );
        let after: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT 'message', id, normalized_markdown FROM chat_messages
             UNION ALL SELECT 'activity', id, safe_metadata_data FROM chat_activities
             UNION ALL SELECT 'turn', id, state FROM chat_turns
             UNION ALL SELECT 'request', id, safe_display_data FROM chat_pending_requests
             ORDER BY 1, 2",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(after, before);
    });
}

#[test]
fn reverted_turn_events_stay_invalid_after_projection_rebuild() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        sqlx::query(
            "UPDATE chat_threads SET provider_thread_id = 'provider-thread',
                    resume_cursor_schema_version = 1,
                    resume_cursor_data = '{\"threadId\":\"provider-thread\"}'
             WHERE id = 'thread-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-invalid-turn-start",
                Some("turn-invalid"),
                CanonicalEvent::TurnStarted(TurnStartedEvent {
                    provider_turn_id: None,
                    state: ChatTurnState::Active,
                    modes: TurnModeSnapshot {
                        safety_mode: SafetyMode::AskForApproval,
                        interaction_mode: InteractionMode::Build,
                    },
                    model_id: None,
                    model_options: Vec::new(),
                }),
            ),
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chat_turns SET user_message_id = 'user-invalid' WHERE id = 'turn-invalid'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_messages
                (id, thread_id, turn_id, sequence_anchor, role, normalized_markdown,
                 streaming_state, created_at, updated_at)
             VALUES ('user-invalid', 'thread-1', 'turn-invalid', 0, 'user',
                     'reverted prompt', 'complete', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let mut assistant = content_event("event-invalid-assistant", "reverted answer");
        assistant.runtime.turn_id = Some(ChatTurnId::new("turn-invalid").unwrap());
        append_canonical_event(&pool, assistant).await.unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-invalid-turn-complete",
                Some("turn-invalid"),
                CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                    state: ChatTurnState::Completed,
                    stop_reason: None,
                    usage: None,
                    changed_files: Vec::new(),
                }),
            ),
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chat_events SET invalidated_at = ?, invalidation_reason = 'checkpoint_restore'
             WHERE thread_id = 'thread-1' AND turn_id = 'turn-invalid'",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chat_turns SET invalidated_at = ?, invalidation_reason = 'checkpoint_restore'
             WHERE id = 'turn-invalid'",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-thread-reverted",
                None,
                CanonicalEvent::ThreadReverted(ThreadRevertedEvent {
                    checkpoint_id: ChatCheckpointId::new("checkpoint-target").unwrap(),
                    reverted_turn_ids: vec![ChatTurnId::new("turn-invalid").unwrap()],
                    provider_history_action: "fork_required".to_string(),
                }),
            ),
        )
        .await
        .unwrap();

        let events = read_canonical_events(&pool, &ChatThreadId::new("thread-1").unwrap(), 0)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].runtime.event,
            CanonicalEvent::ThreadReverted(_)
        ));
        assert_eq!(
            rebuild_thread_projections(&pool, &ChatThreadId::new("thread-1").unwrap())
                .await
                .unwrap(),
            4
        );
        let invalidated_turns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_turns
             WHERE id = 'turn-invalid' AND invalidated_at IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let raw_user_messages: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE id = 'user-invalid'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let assistant_messages: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE role = 'assistant'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let thread = sqlx::query(
            "SELECT provider_thread_id, resume_cursor_data, message_count, latest_preview
             FROM chat_threads WHERE id = 'thread-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(invalidated_turns, 1);
        assert_eq!(raw_user_messages, 1);
        assert_eq!(assistant_messages, 0);
        assert_eq!(thread.get::<Option<String>, _>("provider_thread_id"), None);
        assert_eq!(thread.get::<Option<String>, _>("resume_cursor_data"), None);
        assert_eq!(thread.get::<i64, _>("message_count"), 0);
        assert_eq!(thread.get::<Option<String>, _>("latest_preview"), None);
    });
}

#[test]
fn activity_projection_keeps_public_reasoning_summary_separate_from_raw_reasoning() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(
            &pool,
            canonical_request(
                "event-reasoning-start",
                None,
                CanonicalEvent::ItemStarted(ItemLifecycleEvent {
                    item_id: "reasoning-1".to_string(),
                    kind: CanonicalItemKind::Reasoning,
                    status: ActivityStatus::Active,
                    title: Some("Reasoning".to_string()),
                    detail: None,
                    safe_metadata: None,
                }),
            ),
        )
        .await
        .unwrap();
        for (event_id, stream_kind, delta) in [
            (
                "event-reasoning-private",
                ContentStreamKind::ReasoningText,
                "Private provider reasoning",
            ),
            (
                "event-reasoning-summary",
                ContentStreamKind::ReasoningSummary,
                "Checking attachment rendering",
            ),
        ] {
            let mut request = content_event(event_id, delta);
            let CanonicalEvent::ContentDelta(content) = &mut request.runtime.event else {
                unreachable!()
            };
            content.item_id = "reasoning-1".to_string();
            content.stream_kind = stream_kind;
            append_canonical_event(&pool, request).await.unwrap();
        }
        append_canonical_event(
            &pool,
            canonical_request(
                "event-reasoning-complete",
                None,
                CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
                    item_id: "reasoning-1".to_string(),
                    kind: CanonicalItemKind::Reasoning,
                    status: ActivityStatus::Completed,
                    title: Some("Reasoning".to_string()),
                    detail: None,
                    safe_metadata: None,
                }),
            ),
        )
        .await
        .unwrap();

        let row =
            sqlx::query("SELECT item_kind, detail FROM chat_activities WHERE id = 'reasoning-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.get::<String, _>("item_kind"), "reasoning_summary");
        assert_eq!(
            row.get::<String, _>("detail"),
            "Checking attachment rendering"
        );
    });
}
