//! Session open requests and continuations: instructions on start and resume, and handoffs when a
//! fresh session would lose the full package.

use super::*;

fn session_open_command() -> SendChatTurnCommand {
    SendChatTurnCommand {
        command: ChatCommandContext {
            client_command_id: ChatCommandId::new("send-session-open").unwrap(),
            expected_thread_revision: Some(1),
        },
        working_folder_id: None,
        scratch_generation_id: None,
        thread_id: Some(ChatThreadId::new("thread-1").unwrap()),
        new_thread_id: None,
        execution_environment_id: None,
        turn_id: ChatTurnId::new("turn-session-open").unwrap(),
        message_id: ChatMessageId::new("message-session-open").unwrap(),
        provider_instance_id: ProviderInstanceId::new("fake-instance").unwrap(),
        provider_managed_model: false,
        model_id: Some(ModelId::new("fake-model").unwrap()),
        model_options: Vec::new(),
        modes: TurnModeSnapshot {
            safety_mode: SafetyMode::AskForApproval,
            interaction_mode: InteractionMode::Build,
        },
        prompt: "Start".to_string(),
        attachment_ids: Vec::new(),
        mentions: Vec::new(),
    }
}

fn session_open_runtime_data(
    provider_thread_id: Option<&str>,
    resume_cursor: Option<VersionedJson>,
) -> ThreadRuntimeData {
    ThreadRuntimeData {
        working_folder_id: None,
        scratch_generation_id: None,
        provider_instance_id: ProviderInstanceId::new("fake-instance").unwrap(),
        continuation_group_id: ContinuationGroupId::new("group-1").unwrap(),
        provider_thread_id: provider_thread_id.map(|id| ProviderThreadId::new(id).unwrap()),
        resume_cursor,
        revision: 1,
        execution_environment_id: None,
    }
}

fn session_open_input<'a>(
    command: &'a SendChatTurnCommand,
    existing: Option<&'a ThreadRuntimeData>,
    continuation_group_id: &'a ContinuationGroupId,
    developer_instructions: Option<&'a str>,
) -> SessionOpenRequestInput<'a> {
    SessionOpenRequestInput {
        thread_id: command.thread_id.as_ref().unwrap(),
        workspace: VerifiedWorkspaceContext {
            working_folder_id: ProjectWorkingFolderId::new("fake-workspace").unwrap(),
            canonical_path: "/fake/workspace".to_string(),
            repository_kind: RepositoryKind::None,
            repository_identity: None,
            project_configuration: ProjectConfigurationAccess::Restricted,
        },
        request: command,
        existing,
        continuation_group_id,
        developer_instructions,
        developer_instructions_delivered: false,
    }
}

#[tokio::test]
async fn session_open_requests_pass_instructions_on_start_and_resume() {
    use crate::chat::tests::fake_driver::{
        FakeDriverControl, RecordingEventSink, fake_driver, operation_context,
    };
    use ganbaru_chat_providers::ProviderEventSink;
    use std::sync::Arc;
    use std::time::Duration;

    let command = session_open_command();
    let group = ContinuationGroupId::new("group-1").unwrap();
    let instructions = "Review the plan before editing.";
    let fresh = session_open_runtime_data(None, None);
    let resumable = session_open_runtime_data(
        Some("provider-thread-1"),
        Some(VersionedJson {
            schema_version: 1,
            value: serde_json::json!({ "cursor": 1 }),
        }),
    );
    let control = Arc::new(FakeDriverControl::default());
    let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingEventSink::new(Arc::clone(&control)));
    let context = operation_context("session-open", Duration::from_secs(1));

    for existing in [None, Some(&fresh)] {
        let SessionOpenRequest::Start(start) = session_open_request(session_open_input(
            &command,
            existing,
            &group,
            Some(instructions),
        ))
        .unwrap() else {
            panic!("a thread without a provider thread starts fresh");
        };
        assert_eq!(start.model_id, command.model_id);
        fake_driver(Arc::clone(&control))
            .start_session(start, Arc::clone(&sink), &context)
            .await
            .unwrap();
    }
    let SessionOpenRequest::Resume(resume) = session_open_request(session_open_input(
        &command,
        Some(&resumable),
        &group,
        Some(instructions),
    ))
    .unwrap() else {
        panic!("a thread with continuation data resumes");
    };
    assert_eq!(resume.provider_thread_id.as_str(), "provider-thread-1");
    assert_eq!(resume.continuation_group_id, group);
    fake_driver(Arc::clone(&control))
        .resume_session(resume, Arc::clone(&sink), &context)
        .await
        .unwrap();
    let SessionOpenRequest::Resume(direct) =
        session_open_request(session_open_input(&command, Some(&resumable), &group, None)).unwrap()
    else {
        panic!("a thread with continuation data resumes");
    };
    fake_driver(Arc::clone(&control))
        .resume_session(direct, sink, &context)
        .await
        .unwrap();

    let expected = Some(instructions.to_string());
    assert_eq!(
        control.opened_instructions(),
        vec![expected.clone(), expected.clone(), expected, None]
    );
}

#[test]
fn session_open_request_refuses_a_provider_thread_without_its_resume_cursor() {
    let command = session_open_command();
    let group = ContinuationGroupId::new("group-1").unwrap();
    let broken = session_open_runtime_data(Some("provider-thread-1"), None);
    let error = session_open_request(session_open_input(
        &command,
        Some(&broken),
        &group,
        Some("Instructions"),
    ))
    .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::ResumeNotFound);
}

/// The continuation launch of the folder run, keeping `full` for a session the provider opens
/// fresh.
fn continuation_folder_launch(
    full: Option<ganbaru_chat::agent_runs::FullPackage>,
) -> ganbaru_chat::agent_runs::RunLaunch {
    use ganbaru_chat::agent_runs::{AttemptReason, ContextDelivery, RunAttempt, RunLaunch};
    let launch = folder_launch();
    RunLaunch {
        attempt: RunAttempt {
            reason: AttemptReason::Continuation,
            context_delivery: ContextDelivery::Delta,
            delivered_context_bytes: 5,
            ..launch.attempt
        },
        full_prompt_on_fresh_session: full,
        ..launch
    }
}

fn full_folder_package() -> ganbaru_chat::agent_runs::FullPackage {
    use ganbaru_chat::agent_runs::{DeliveredSource, DeliveryKind, FullPackage};
    FullPackage {
        prompt: "Full package with every source".to_string(),
        delivered_sources: vec![DeliveredSource {
            source_id: "item:root".to_string(),
            content_sha256: "a".repeat(64),
            kind: DeliveryKind::Package,
        }],
    }
}

fn assignment_origin(
    launch: ganbaru_chat::agent_runs::RunLaunch,
    binding: ganbaru_chat::agent_runs::AgentRunBinding,
) -> TurnOrigin {
    TurnOrigin::Assignment {
        developer_instructions: "Instructions".to_string(),
        run: Box::new(binding),
        launch: Box::new(launch),
    }
}

fn opened_session(provider_thread_id: &str) -> ProviderSessionSnapshot {
    serde_json::from_value(serde_json::json!({
        "sessionId": "fake-session",
        "state": "ready",
        "providerThreadId": provider_thread_id,
        "continuationGroupId": "group-1",
        "resumeCursor": { "schemaVersion": 1, "value": { "cursor": 1 } },
        "effectiveModes": { "safetyMode": "ask_for_approval", "interactionMode": "build" },
        "capabilities": { "entries": [] },
        "startedAt": "2026-10-20T12:00:30Z"
    }))
    .unwrap()
}

/// Stores the delta prompt of the session-open command and an approval the earlier session
/// left open.
async fn seed_continued_turn(pool: &sqlx::SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO chat_messages
            (id, thread_id, turn_id, sequence_anchor, role, normalized_markdown,
             streaming_state, created_at, updated_at)
         VALUES ('message-session-open', 'thread-1', 'turn:working', 0, 'user', 'Delta',
                 'complete', '2026-10-20T12:00:00.000Z', '2026-10-20T12:00:00.000Z');
         INSERT INTO chat_pending_requests
            (id, thread_id, turn_id, provider_request_id, request_kind, safe_display_data,
             allowed_decisions_data, opened_sequence, opened_at)
         VALUES ('request:detached', 'thread-1', 'turn:working', 'native-request', 'approval',
                 '{}', '[]', 1, '2026-10-20T12:00:00.000Z');",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn detached_approval_state(pool: &sqlx::SqlitePool) -> String {
    sqlx::query_scalar(
        "SELECT resolution_state FROM chat_pending_requests WHERE id = 'request:detached'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_silent_fresh_session_hands_the_continuation_off_with_the_full_package() {
    use super::super::coordinator::hand_off_on_fresh_session;
    use ganbaru_chat::agent_runs::{AttemptReason, ContextDelivery, HandoffReason};
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    starting_folder_run(&pool).await;
    seed_continued_turn(&pool).await;
    let binding = working_run_binding(&pool).await;
    let origin = assignment_origin(
        continuation_folder_launch(Some(full_folder_package())),
        binding,
    );
    let existing = session_open_runtime_data(Some("native-1"), None);
    let thread_id = ChatThreadId::new("thread-1").unwrap();

    let (prompt, launch) = hand_off_on_fresh_session(
        &pool,
        &thread_id,
        &session_open_command(),
        &origin,
        Some(&existing),
        &opened_session("native-2"),
    )
    .await
    .unwrap()
    .expect("a provider that opens a new session receives the full package");

    assert_eq!(prompt, full_folder_package().prompt);
    assert_eq!(
        launch.attempt.reason,
        AttemptReason::Handoff(HandoffReason::NativeSessionLost)
    );
    assert_eq!(launch.attempt.context_delivery, ContextDelivery::Full);
    assert_eq!(
        launch.attempt.delivered_sources,
        full_folder_package().delivered_sources
    );
    assert_eq!(launch.full_prompt_on_fresh_session, None);
    let stored: (String, String, String, i64) = sqlx::query_as(
        "SELECT attempt_reason, handoff_reason, context_delivery, delivered_context_bytes
         FROM chat_agent_runs WHERE id = 'run:working'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        (
            "handoff".to_string(),
            "native_session_lost".to_string(),
            "full".to_string(),
            i64::try_from(prompt.len()).unwrap()
        )
    );
    let message: String = sqlx::query_scalar(
        "SELECT normalized_markdown FROM chat_messages WHERE id = 'message-session-open'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        message, prompt,
        "the timeline shows what the provider received"
    );
    assert_eq!(detached_approval_state(&pool).await, "interrupted");
}

#[tokio::test]
async fn a_resumed_session_or_a_continuation_without_a_full_package_never_hands_off_silently() {
    use super::super::coordinator::hand_off_on_fresh_session;
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    starting_folder_run(&pool).await;
    seed_continued_turn(&pool).await;
    let binding = working_run_binding(&pool).await;
    let existing = session_open_runtime_data(Some("native-1"), None);
    let thread_id = ChatThreadId::new("thread-1").unwrap();
    let command = session_open_command();

    let resumed = assignment_origin(
        continuation_folder_launch(Some(full_folder_package())),
        binding.clone(),
    );
    let same = hand_off_on_fresh_session(
        &pool,
        &thread_id,
        &command,
        &resumed,
        Some(&existing),
        &opened_session("native-1"),
    )
    .await
    .unwrap();
    assert_eq!(same, None, "a resumed session keeps the delta");
    let direct = hand_off_on_fresh_session(
        &pool,
        &thread_id,
        &command,
        &TurnOrigin::Direct,
        Some(&existing),
        &opened_session("native-2"),
    )
    .await
    .unwrap();
    assert_eq!(direct, None, "a direct send has no package to resend");

    let without_fallback = assignment_origin(continuation_folder_launch(None), binding);
    let refused = hand_off_on_fresh_session(
        &pool,
        &thread_id,
        &command,
        &without_fallback,
        Some(&existing),
        &opened_session("native-2"),
    )
    .await
    .expect_err("a delta never reaches a session that did not receive the full package");
    assert_eq!(refused.code, ChatErrorCode::ResumeNotFound);

    let reason: String =
        sqlx::query_scalar("SELECT attempt_reason FROM chat_agent_runs WHERE id = 'run:working'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        reason, "fresh",
        "the stored attempt of the run is unchanged"
    );
    assert_eq!(detached_approval_state(&pool).await, "open");
}

#[tokio::test]
async fn a_continuation_whose_session_was_not_found_asks_for_a_handoff() {
    use ganbaru_chat::agent_runs::{LaunchSettlement, settle_launch};
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    starting_folder_run(&pool).await;
    seed_continued_turn(&pool).await;
    let binding = working_run_binding(&pool).await;
    let lost = ChatError::new(ChatErrorCode::ResumeNotFound, "Session not found", true);

    let settlement = settle_launch(
        &pool,
        &binding,
        &continuation_folder_launch(Some(full_folder_package())),
        Some(&lost),
        &UtcTimestamp::new("2026-10-20T12:01:00.000Z".to_string()).unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(settlement, LaunchSettlement::HandoffRequired);
    assert_eq!(
        folder_assignment_and_run(&pool).await,
        (
            "queued".to_string(),
            "failed".to_string(),
            "claimed".to_string()
        )
    );
    assert_eq!(detached_approval_state(&pool).await, "interrupted");
}

#[tokio::test]
async fn only_a_continuation_resumes_with_its_instructions_delivered() {
    use ganbaru_chat::agent_runs::{AttemptReason, HandoffReason, RunAttempt, RunLaunch};
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    starting_folder_run(&pool).await;
    let binding = working_run_binding(&pool).await;
    let with_reason = |reason| {
        let launch = folder_launch();
        assignment_origin(
            RunLaunch {
                attempt: RunAttempt {
                    reason,
                    ..launch.attempt
                },
                ..launch
            },
            binding.clone(),
        )
    };

    assert!(
        assignment_origin(continuation_folder_launch(None), binding.clone())
            .instructions_delivered_to_native_session()
    );
    for reason in [
        AttemptReason::Fresh,
        AttemptReason::Retry,
        AttemptReason::Handoff(HandoffReason::KeyChanged),
        AttemptReason::Handoff(HandoffReason::AuthorityRevoked),
        AttemptReason::Handoff(HandoffReason::NativeSessionLost),
    ] {
        assert!(
            !with_reason(reason).instructions_delivered_to_native_session(),
            "{reason:?} opens a native session that has not received the instructions"
        );
    }
    assert!(!TurnOrigin::Direct.instructions_delivered_to_native_session());
}

#[test]
fn a_continuation_resume_reports_its_instructions_delivered() {
    let command = session_open_command();
    let group = ContinuationGroupId::new("group-1").unwrap();
    let resumable = session_open_runtime_data(
        Some("provider-thread-1"),
        Some(VersionedJson {
            schema_version: 1,
            value: serde_json::json!({ "cursor": 1 }),
        }),
    );
    let instructions = Some("Review the plan before editing.");
    let SessionOpenRequest::Resume(continued) = session_open_request(SessionOpenRequestInput {
        developer_instructions_delivered: true,
        ..session_open_input(&command, Some(&resumable), &group, instructions)
    })
    .unwrap() else {
        panic!("a thread with continuation data resumes");
    };
    assert!(continued.developer_instructions_delivered);
    assert_eq!(
        continued.developer_instructions.as_deref(),
        instructions,
        "the instructions still travel so a provider can tell a change apart"
    );
    let SessionOpenRequest::Resume(other) = session_open_request(session_open_input(
        &command,
        Some(&resumable),
        &group,
        instructions,
    ))
    .unwrap() else {
        panic!("a thread with continuation data resumes");
    };
    assert!(!other.developer_instructions_delivered);
}
