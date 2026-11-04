use super::*;

#[test]
fn startup_recovery_interrupts_only_turns_without_proven_resumability() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(
            &pool,
            canonical_request(
                "event-orphan-start",
                Some("turn-orphan"),
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
        append_canonical_event(
            &pool,
            canonical_request(
                "event-orphan-request",
                Some("turn-orphan"),
                CanonicalEvent::RequestOpened(RequestOpenedEvent {
                    request_id: ProviderRequestId::new("request-orphan").unwrap(),
                    kind: CanonicalRequestKind::CommandExecution,
                    title: "Approve command".to_string(),
                    detail: None,
                    allowed_decisions: Vec::new(),
                    safe_payload: VersionedJson {
                        schema_version: 1,
                        value: serde_json::json!({}),
                    },
                    subject: None,
                    session_grants: Vec::new(),
                }),
            ),
        )
        .await
        .unwrap();
        let mut resumable = HashSet::new();
        resumable.insert(ChatThreadId::new("thread-1").unwrap());
        assert_eq!(
            recover_orphaned_turns(&pool, &resumable, &UtcTimestamp::new(NOW).unwrap())
                .await
                .unwrap(),
            0
        );
        let request_state: String = sqlx::query_scalar(
            "SELECT resolution_state FROM chat_pending_requests WHERE id = 'request-orphan'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(request_state, "open");
        assert_eq!(
            recover_orphaned_turns(&pool, &HashSet::new(), &UtcTimestamp::new(NOW).unwrap())
                .await
                .unwrap(),
            1
        );
        let state: String =
            sqlx::query_scalar("SELECT state FROM chat_turns WHERE id = 'turn-orphan'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "interrupted");
        let event_type: String =
            sqlx::query_scalar("SELECT event_type FROM chat_events ORDER BY sequence DESC LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(event_type, "turn_aborted");
        let request_state: String = sqlx::query_scalar(
            "SELECT resolution_state FROM chat_pending_requests WHERE id = 'request-orphan'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(request_state, "interrupted");

        sqlx::query(
            "UPDATE chat_pending_requests
             SET resolution_state = 'open', resolved_at = NULL
             WHERE id = 'request-orphan'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            recover_orphaned_turns(&pool, &HashSet::new(), &UtcTimestamp::new(NOW).unwrap())
                .await
                .unwrap(),
            0
        );
        let recovered_request_state: String = sqlx::query_scalar(
            "SELECT resolution_state FROM chat_pending_requests WHERE id = 'request-orphan'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(recovered_request_state, "interrupted");
    });
}
