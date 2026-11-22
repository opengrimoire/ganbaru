use super::*;

#[test]
fn event_append_updates_projection_sequence_and_revision_atomically() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let first = append_canonical_event(&pool, content_event("event-1", "Hello "))
            .await
            .unwrap();
        let second = append_canonical_event(&pool, content_event("event-2", "world"))
            .await
            .unwrap();

        assert_eq!(first.event.sequence, 1);
        assert_eq!(first.notification.revision, 2);
        assert_eq!(second.event.sequence, 2);
        assert_eq!(second.notification.revision, 3);
        let thread: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT revision, last_event_sequence, last_projected_sequence, message_count
             FROM chat_threads WHERE id = 'thread-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(thread, (3, 2, 2, 1));
        let text: String = sqlx::query_scalar(
            "SELECT normalized_markdown FROM chat_messages WHERE id = 'assistant-message-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(text, "Hello world");

        let replay = read_canonical_events(&pool, &ChatThreadId::new("thread-1").unwrap(), 0)
            .await
            .unwrap();
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].runtime, first.event.runtime);
        assert_eq!(replay[1].runtime, second.event.runtime);
    });
}

#[test]
fn completed_activity_preserves_streamed_output_title_and_metadata() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(
            &pool,
            canonical_request(
                "event-command-start",
                None,
                CanonicalEvent::ItemStarted(ItemLifecycleEvent {
                    item_id: "command-1".to_string(),
                    kind: CanonicalItemKind::CommandExecution,
                    status: ActivityStatus::Active,
                    title: Some("printf 4".to_string()),
                    detail: None,
                    safe_metadata: Some(VersionedJson {
                        schema_version: 1,
                        value: serde_json::json!({ "toolInput": { "command": "printf 4" } }),
                    }),
                }),
            ),
        )
        .await
        .unwrap();
        let mut output = content_event("event-command-output", "4\n");
        let CanonicalEvent::ContentDelta(delta) = &mut output.runtime.event else {
            unreachable!()
        };
        delta.item_id = "command-1".to_string();
        delta.stream_kind = ContentStreamKind::CommandOutput;
        append_canonical_event(&pool, output).await.unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-command-complete",
                None,
                CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
                    item_id: "command-1".to_string(),
                    kind: CanonicalItemKind::CommandExecution,
                    status: ActivityStatus::Completed,
                    title: None,
                    detail: None,
                    safe_metadata: None,
                }),
            ),
        )
        .await
        .unwrap();

        let (detail, title, metadata): (String, String, String) = sqlx::query_as(
            "SELECT detail, title, safe_metadata_data FROM chat_activities WHERE id = 'command-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(detail, "4\n");
        assert_eq!(title, "printf 4");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&metadata).unwrap(),
            serde_json::json!({ "toolInput": { "command": "printf 4" } })
        );
    });
}

#[test]
fn projections_keep_assistant_phase_and_turn_diff_data() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(
            &pool,
            canonical_request(
                "event-turn-start",
                Some("turn-typed-items"),
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
                "event-final-answer",
                Some("turn-typed-items"),
                CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
                    item_id: "answer-1".to_string(),
                    kind: CanonicalItemKind::AssistantMessage,
                    status: ActivityStatus::Completed,
                    title: Some("Assistant message".to_string()),
                    detail: Some("Final answer".to_string()),
                    safe_metadata: Some(VersionedJson {
                        schema_version: 1,
                        value: serde_json::json!({ "phase": "final_answer" }),
                    }),
                }),
            ),
        )
        .await
        .unwrap();
        let changed_file = ChangedFileSummary {
            relative_path: "src/main.rs".to_string(),
            previous_relative_path: None,
            additions: Some(2),
            deletions: Some(1),
            binary: false,
            status: "modified".to_string(),
        };
        append_canonical_event(
            &pool,
            canonical_request(
                "event-turn-diff",
                Some("turn-typed-items"),
                CanonicalEvent::DiffUpdated(DiffUpdatedEvent {
                    source: "codex".to_string(),
                    files: vec![changed_file.clone()],
                    provider_diff: None,
                }),
            ),
        )
        .await
        .unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-turn-complete",
                Some("turn-typed-items"),
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

        let message = sqlx::query(
            "SELECT normalized_markdown, streaming_state, content_metadata_data
             FROM chat_messages WHERE id = 'answer-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            message.get::<String, _>("normalized_markdown"),
            "Final answer"
        );
        assert_eq!(message.get::<String, _>("streaming_state"), "complete");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &message.get::<String, _>("content_metadata_data")
            )
            .unwrap()["phase"],
            "final_answer"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM chat_activities WHERE id = 'answer-1'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        let page = read_timeline_page(&pool, &ChatThreadId::new("thread-1").unwrap(), None, 50)
            .await
            .unwrap();
        assert_eq!(page.turns[0].changed_files, vec![changed_file]);
    });
}

#[test]
fn terminal_lifecycle_events_expire_open_interaction_requests() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let modes = TurnModeSnapshot {
            safety_mode: SafetyMode::AskForApproval,
            interaction_mode: InteractionMode::Build,
        };

        append_canonical_event(
            &pool,
            canonical_request(
                "event-completed-turn-start",
                Some("turn-completed-request"),
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
        append_canonical_event(
            &pool,
            canonical_request(
                "event-completed-request",
                Some("turn-completed-request"),
                CanonicalEvent::RequestOpened(RequestOpenedEvent {
                    request_id: ProviderRequestId::new("request-completed").unwrap(),
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
        append_canonical_event(
            &pool,
            canonical_request(
                "event-completed-turn-end",
                Some("turn-completed-request"),
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
                "event-aborted-turn-start",
                Some("turn-aborted-request"),
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
        append_canonical_event(
            &pool,
            canonical_request(
                "event-aborted-request",
                Some("turn-aborted-request"),
                CanonicalEvent::UserInputRequested(UserInputRequestedEvent {
                    request_id: ProviderRequestId::new("request-aborted").unwrap(),
                    questions: vec![UserInputQuestion {
                        id: "question-1".to_string(),
                        header: None,
                        question: "Choose an option".to_string(),
                        options: Vec::new(),
                        multiple: false,
                        free_form_allowed: true,
                        required: true,
                    }],
                }),
            ),
        )
        .await
        .unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-aborted-turn-end",
                Some("turn-aborted-request"),
                CanonicalEvent::TurnAborted(TurnAbortedEvent {
                    state: ChatTurnState::Interrupted,
                    reason: "Stopped".to_string(),
                    recoverable: true,
                }),
            ),
        )
        .await
        .unwrap();

        append_canonical_event(
            &pool,
            canonical_request(
                "event-exited-turn-start",
                Some("turn-exited-request"),
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
        append_canonical_event(
            &pool,
            canonical_request(
                "event-exited-request",
                Some("turn-exited-request"),
                CanonicalEvent::RequestOpened(RequestOpenedEvent {
                    request_id: ProviderRequestId::new("request-exited").unwrap(),
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
        append_canonical_event(
            &pool,
            canonical_request(
                "event-session-exited",
                None,
                CanonicalEvent::SessionExited(SessionExitedEvent {
                    session_id: ProviderSessionId::new("session-1").unwrap(),
                    expected: true,
                    exit_code: None,
                    reason: Some("App closed".to_string()),
                }),
            ),
        )
        .await
        .unwrap();

        let requests: Vec<(String, String, bool)> = sqlx::query_as(
            "SELECT id, resolution_state, resolved_at IS NOT NULL
             FROM chat_pending_requests ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            requests,
            vec![
                (
                    "request-aborted".to_string(),
                    "interrupted".to_string(),
                    true
                ),
                ("request-completed".to_string(), "stale".to_string(), true),
                (
                    "request-exited".to_string(),
                    "interrupted".to_string(),
                    true
                ),
            ]
        );
    });
}

#[test]
fn projection_failure_rolls_back_event_and_thread_advance() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let mut request = content_event("event-invalid", "ignored");
        request.runtime.event =
            CanonicalEvent::TurnStarted(ganbaru_chat_contracts::events::TurnStartedEvent {
                provider_turn_id: None,
                state: ganbaru_chat_contracts::models::ChatTurnState::Active,
                modes: ganbaru_chat_contracts::models::TurnModeSnapshot {
                    safety_mode: ganbaru_chat_contracts::models::SafetyMode::AskForApproval,
                    interaction_mode: ganbaru_chat_contracts::models::InteractionMode::Build,
                },
                model_id: None,
                model_options: Vec::new(),
            });

        assert!(append_canonical_event(&pool, request).await.is_err());
        let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        let thread = sqlx::query(
            "SELECT revision, last_event_sequence FROM chat_threads WHERE id = 'thread-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(event_count, 0);
        assert_eq!(thread.get::<i64, _>("revision"), 1);
        assert_eq!(thread.get::<i64, _>("last_event_sequence"), 0);
    });
}

#[test]
fn colliding_provider_item_ids_never_change_another_threads_projections() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        sqlx::query(
            "INSERT INTO chat_threads
                (id, project_id, working_folder_id, title, provider_family_id, provider_instance_id,
                 continuation_group_id, safety_mode, interaction_mode, state,
                 last_activity_at, created_at, updated_at)
             SELECT 'thread-2', project_id, working_folder_id, title, provider_family_id,
                    provider_instance_id, 'continuation-2', safety_mode, interaction_mode, state,
                    last_activity_at, created_at, updated_at
             FROM chat_threads WHERE id = 'thread-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        append_canonical_event(&pool, content_event("event-1", "Private reply"))
            .await
            .unwrap();
        let activity = |event_id: &str, title: &str| {
            canonical_request(
                event_id,
                None,
                CanonicalEvent::ItemStarted(ItemLifecycleEvent {
                    item_id: "command-1".to_string(),
                    kind: CanonicalItemKind::CommandExecution,
                    status: ActivityStatus::Active,
                    title: Some(title.to_string()),
                    detail: None,
                    safe_metadata: None,
                }),
            )
        };
        append_canonical_event(&pool, activity("event-2", "ls"))
            .await
            .unwrap();

        let mut foreign_text = content_event("event-3", " injected");
        foreign_text.runtime.thread_id = ChatThreadId::new("thread-2").unwrap();
        append_canonical_event(&pool, foreign_text).await.unwrap();
        let mut foreign_activity = activity("event-4", "rm -rf");
        foreign_activity.runtime.thread_id = ChatThreadId::new("thread-2").unwrap();
        append_canonical_event(&pool, foreign_activity)
            .await
            .unwrap();

        let message: (String, String) = sqlx::query_as(
            "SELECT thread_id, normalized_markdown FROM chat_messages
             WHERE id = 'assistant-message-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            message,
            ("thread-1".to_string(), "Private reply".to_string())
        );
        let title: String =
            sqlx::query_scalar("SELECT title FROM chat_activities WHERE id = 'command-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(title, "ls");
        let foreign_preview: Option<String> =
            sqlx::query_scalar("SELECT latest_preview FROM chat_threads WHERE id = 'thread-2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_ne!(foreign_preview.as_deref(), Some("Private reply"));
    });
}

#[test]
fn projections_skip_user_echoes_resolve_requests_and_reject_duplicate_events() {
    use ganbaru_chat_contracts::events::RequestResolvedEvent;
    use ganbaru_chat_contracts::models::RequestResolutionState;

    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(
            &pool,
            canonical_request(
                "event-turn-start",
                Some("turn-1"),
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
                "event-user-echo",
                Some("turn-1"),
                CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
                    item_id: "provider-user".to_string(),
                    kind: CanonicalItemKind::UserMessage,
                    status: ActivityStatus::Completed,
                    title: None,
                    detail: Some("Please continue".to_string()),
                    safe_metadata: None,
                }),
            ),
        )
        .await
        .unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-request-open",
                Some("turn-1"),
                CanonicalEvent::RequestOpened(RequestOpenedEvent {
                    request_id: ProviderRequestId::new("request-1").unwrap(),
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
        append_canonical_event(
            &pool,
            canonical_request(
                "event-request-resolve",
                Some("turn-1"),
                CanonicalEvent::RequestResolved(RequestResolvedEvent {
                    request_id: ProviderRequestId::new("request-1").unwrap(),
                    state: RequestResolutionState::Resolved,
                    decision: None,
                }),
            ),
        )
        .await
        .unwrap();
        append_canonical_event(&pool, content_event("event-text", "Once"))
            .await
            .unwrap();
        assert!(
            append_canonical_event(&pool, content_event("event-text", " again"))
                .await
                .is_err()
        );

        let echoes: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_activities WHERE id = 'provider-user'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let user_messages: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE id = 'provider-user'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((echoes, user_messages), (0, 0));
        let request: (String, bool) = sqlx::query_as(
            "SELECT resolution_state, resolved_at IS NOT NULL
             FROM chat_pending_requests WHERE id = 'request-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(request, ("resolved".to_string(), true));
        let text: String = sqlx::query_scalar(
            "SELECT normalized_markdown FROM chat_messages WHERE id = 'assistant-message-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(text, "Once");
        let last_sequence: i64 = sqlx::query_scalar(
            "SELECT last_event_sequence FROM chat_threads WHERE id = 'thread-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(last_sequence, 5);
    });
}

fn turn_started(event_id: &str, turn_id: &str) -> AppendCanonicalEventRequest {
    canonical_request(
        event_id,
        Some(turn_id),
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
    )
}

fn turn_completed(event_id: &str, turn_id: &str, stop_reason: &str) -> AppendCanonicalEventRequest {
    canonical_request(
        event_id,
        Some(turn_id),
        CanonicalEvent::TurnCompleted(TurnCompletedEvent {
            state: ChatTurnState::Completed,
            stop_reason: Some(stop_reason.to_string()),
            usage: None,
            changed_files: Vec::new(),
        }),
    )
}

async fn turn_row(pool: &sqlx::SqlitePool, turn_id: &str) -> (String, Option<String>) {
    sqlx::query_as("SELECT state, stop_reason FROM chat_turns WHERE id = ?")
        .bind(turn_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn thread_states(pool: &sqlx::SqlitePool) -> (String, Option<String>) {
    sqlx::query_as("SELECT state, latest_turn_state FROM chat_threads WHERE id = 'thread-1'")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn turn_started_replay_never_revives_a_settled_turn() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(&pool, turn_started("start-1", "turn-1"))
            .await
            .unwrap();
        assert_eq!(
            thread_states(&pool).await,
            ("active".to_string(), Some("active".to_string()))
        );
        append_canonical_event(&pool, turn_completed("complete-1", "turn-1", "done"))
            .await
            .unwrap();
        append_canonical_event(&pool, turn_started("start-again", "turn-1"))
            .await
            .unwrap();

        assert_eq!(
            turn_row(&pool, "turn-1").await,
            ("completed".to_string(), Some("done".to_string()))
        );
        assert_eq!(
            thread_states(&pool).await,
            ("idle".to_string(), Some("completed".to_string()))
        );
    });
}

#[test]
fn duplicate_turn_settlement_keeps_the_first_outcome() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(&pool, turn_started("start-1", "turn-1"))
            .await
            .unwrap();
        append_canonical_event(&pool, turn_completed("complete-1", "turn-1", "first"))
            .await
            .unwrap();
        append_canonical_event(&pool, turn_completed("complete-2", "turn-1", "second"))
            .await
            .unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "abort-late",
                Some("turn-1"),
                CanonicalEvent::TurnAborted(TurnAbortedEvent {
                    state: ChatTurnState::Interrupted,
                    reason: "late".to_string(),
                    recoverable: true,
                }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(
            turn_row(&pool, "turn-1").await,
            ("completed".to_string(), Some("first".to_string()))
        );
        assert_eq!(
            thread_states(&pool).await,
            ("idle".to_string(), Some("completed".to_string()))
        );
    });
}

#[test]
fn late_settlement_of_an_older_turn_keeps_the_newer_turn_state() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(&pool, turn_started("start-1", "turn-1"))
            .await
            .unwrap();
        append_canonical_event(&pool, turn_completed("complete-1", "turn-1", "done"))
            .await
            .unwrap();
        append_canonical_event(&pool, turn_started("start-2", "turn-2"))
            .await
            .unwrap();
        append_canonical_event(&pool, turn_completed("complete-1-again", "turn-1", "done"))
            .await
            .unwrap();

        assert_eq!(
            thread_states(&pool).await,
            ("active".to_string(), Some("active".to_string()))
        );
    });
}

#[test]
fn turn_events_never_reopen_a_closed_thread() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(&pool, turn_started("start-1", "turn-1"))
            .await
            .unwrap();
        sqlx::query("UPDATE chat_threads SET state = 'closed' WHERE id = 'thread-1'")
            .execute(&pool)
            .await
            .unwrap();
        append_canonical_event(&pool, turn_completed("complete-1", "turn-1", "done"))
            .await
            .unwrap();
        assert_eq!(
            thread_states(&pool).await,
            ("closed".to_string(), Some("completed".to_string()))
        );
        append_canonical_event(&pool, turn_started("start-2", "turn-2"))
            .await
            .unwrap();
        assert_eq!(
            thread_states(&pool).await,
            ("closed".to_string(), Some("active".to_string()))
        );
    });
}

#[test]
fn stored_thread_state_rejects_archived() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let error = sqlx::query("UPDATE chat_threads SET state = 'archived' WHERE id = 'thread-1'")
            .execute(&pool)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("archived_at"), "{error}");
    });
}

#[test]
fn persisted_turn_writer_applies_only_declared_transitions() {
    use ganbaru_chat::repository::state_writes::{
        StateWrite, StateWriteColumns, persist_turn_transition,
    };
    use ganbaru_chat::state::can_transition_turn;
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let mut connection = pool.acquire().await.unwrap();
        let mut ordinal = 0_i64;
        for from in ChatTurnState::ALL {
            for to in ChatTurnState::ALL {
                let turn_id = format!("turn-{}-{}", from.as_str(), to.as_str());
                sqlx::query(
                    "INSERT INTO chat_turns
                        (id, thread_id, ordinal, state, safety_mode, interaction_mode,
                         created_at, updated_at)
                     VALUES (?, 'thread-1', ?, ?, 'ask_for_approval', 'build', ?, ?)",
                )
                .bind(&turn_id)
                .bind(ordinal)
                .bind(from.as_str())
                .bind(NOW)
                .bind(NOW)
                .execute(&mut *connection)
                .await
                .unwrap();
                ordinal += 1;
                let outcome = persist_turn_transition(
                    &mut connection,
                    "thread-1",
                    &turn_id,
                    to,
                    StateWriteColumns::default(),
                )
                .await
                .unwrap();
                let expected = if can_transition_turn(from, to) {
                    StateWrite::Applied
                } else if from == to {
                    StateWrite::AlreadyInState
                } else {
                    StateWrite::Rejected { from }
                };
                assert_eq!(outcome, expected, "turn write {from:?} to {to:?}");
                let stored: String =
                    sqlx::query_scalar("SELECT state FROM chat_turns WHERE id = ?")
                        .bind(&turn_id)
                        .fetch_one(&mut *connection)
                        .await
                        .unwrap();
                let landed = if outcome.is_applied() { to } else { from };
                assert_eq!(stored, landed.as_str(), "stored {from:?} to {to:?}");
            }
        }
        assert_eq!(
            persist_turn_transition(
                &mut connection,
                "thread-other",
                "turn-pending-active",
                ChatTurnState::Active,
                StateWriteColumns::default(),
            )
            .await
            .unwrap(),
            StateWrite::Missing
        );
    });
}

#[test]
fn persisted_thread_writer_applies_only_declared_transitions() {
    use ganbaru_chat::repository::state_writes::{StateWrite, persist_thread_state};
    use ganbaru_chat::state::can_transition_thread;
    use ganbaru_chat_contracts::models::ChatThreadState;
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let mut connection = pool.acquire().await.unwrap();
        for from in ChatThreadState::ALL
            .into_iter()
            .filter(|state| state.is_persisted())
        {
            for to in ChatThreadState::ALL {
                sqlx::query("UPDATE chat_threads SET state = ? WHERE id = 'thread-1'")
                    .bind(from.as_str())
                    .execute(&mut *connection)
                    .await
                    .unwrap();
                let outcome = persist_thread_state(&mut connection, "thread-1", to)
                    .await
                    .unwrap();
                let expected = if can_transition_thread(from, to) {
                    StateWrite::Applied
                } else if from == to {
                    StateWrite::AlreadyInState
                } else {
                    StateWrite::Rejected { from }
                };
                assert_eq!(outcome, expected, "thread write {from:?} to {to:?}");
            }
        }
        assert_eq!(
            persist_thread_state(&mut connection, "thread-missing", ChatThreadState::Idle)
                .await
                .unwrap(),
            StateWrite::Missing
        );
    });
}
