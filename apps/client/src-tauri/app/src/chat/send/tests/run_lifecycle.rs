//! Run settlement and launch: checkpoint changes, completion review, late provider events, and
//! launches that race a settled, cancelled, or missing run.

use super::*;

fn changed_file(relative_path: &str) -> ChangedFileSummary {
    ChangedFileSummary {
        relative_path: relative_path.to_string(),
        previous_relative_path: None,
        additions: Some(1),
        deletions: Some(0),
        binary: false,
        status: "modified".to_string(),
    }
}

#[test]
fn checkpoint_changes_count_when_the_provider_omits_or_crowds_them_out() {
    let mut omitted = Vec::new();
    merge_checkpoint_changed_files(&mut omitted, vec![changed_file("src/main.rs")]);
    assert_eq!(omitted, vec![changed_file("src/main.rs")]);

    let mut reported = vec![ChangedFileSummary {
        additions: Some(40),
        ..changed_file("src/main.rs")
    }];
    reported.extend((0..600).map(|index| changed_file(&format!("noise/{index}.txt"))));
    merge_checkpoint_changed_files(&mut reported, vec![changed_file("src/main.rs")]);
    assert_eq!(reported.len(), 512);
    assert_eq!(reported[0], changed_file("src/main.rs"));
    assert_eq!(
        reported
            .iter()
            .filter(|file| file.relative_path == "src/main.rs")
            .count(),
        1
    );
}

async fn complete_working_turn(pool: &sqlx::SqlitePool, recorded_diff: bool) -> (String, String) {
    use crate::chat::tests::repository::canonical_request;
    use ganbaru_chat::repository::events::append_canonical_event;
    use ganbaru_chat_contracts::events::TurnCompletedEvent;
    if recorded_diff {
        append_canonical_event(
            pool,
            canonical_request(
                "event:diff",
                Some("turn:working"),
                CanonicalEvent::DiffUpdated(DiffUpdatedEvent {
                    source: "checkpoint".to_string(),
                    files: vec![changed_file("src/main.rs")],
                    provider_diff: None,
                }),
            ),
        )
        .await
        .unwrap();
    }
    append_canonical_event(
        pool,
        canonical_request(
            "event:completed",
            Some("turn:working"),
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
    sqlx::query_as(
        "SELECT assignment.state, run.state
         FROM chat_work_assignments assignment
         JOIN chat_agent_runs run ON run.assignment_id = assignment.id
         WHERE run.id = 'run:working'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn completion_requests_review_from_the_recorded_turn_diff_not_the_completion_report() {
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    assert_eq!(
        complete_working_turn(&pool, true).await,
        ("ready_for_review".to_string(), "completed".to_string())
    );

    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    assert_eq!(
        complete_working_turn(&pool, false).await,
        ("completed".to_string(), "completed".to_string())
    );
}

#[tokio::test]
async fn late_provider_events_cannot_reopen_or_rewrite_a_settled_run() {
    use crate::chat::tests::repository::canonical_request;
    use ganbaru_chat::repository::events::append_canonical_event;
    use ganbaru_chat_contracts::events::{
        RequestOpenedEvent, TurnAbortedEvent, TurnCompletedEvent,
    };
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    sqlx::raw_sql(
        "UPDATE chat_agent_runs SET state = 'cancelled', settled_at = '2026-10-20T12:01:00.000Z'
         WHERE id = 'run:working';
         UPDATE chat_work_assignments SET state = 'cancelled', settled_at = '2026-10-20T12:01:00.000Z'
         WHERE id = (SELECT assignment_id FROM chat_agent_runs WHERE id = 'run:working');",
    )
    .execute(&pool)
    .await
    .unwrap();
    let late_events = [
        CanonicalEvent::RequestOpened(RequestOpenedEvent {
            request_id: ProviderRequestId::new("request-late").unwrap(),
            kind: CanonicalRequestKind::CommandExecution,
            title: "Approve".to_string(),
            detail: None,
            allowed_decisions: Vec::new(),
            safe_payload: VersionedJson {
                schema_version: 1,
                value: serde_json::json!({}),
            },
            subject: None,
            session_grants: Vec::new(),
        }),
        CanonicalEvent::TurnCompleted(TurnCompletedEvent {
            state: ChatTurnState::Completed,
            stop_reason: None,
            usage: None,
            changed_files: Vec::new(),
        }),
        CanonicalEvent::TurnAborted(TurnAbortedEvent {
            state: ChatTurnState::Failed,
            reason: "Provider exited".to_string(),
            recoverable: false,
        }),
    ];
    for (index, event) in late_events.into_iter().enumerate() {
        append_canonical_event(
            &pool,
            canonical_request(&format!("event:late-{index}"), Some("turn:working"), event),
        )
        .await
        .unwrap();
    }

    let states: (String, String, String) = sqlx::query_as(
        "SELECT assignment.state, run.state, run.settled_at
         FROM chat_work_assignments assignment
         JOIN chat_agent_runs run ON run.assignment_id = assignment.id
         WHERE run.id = 'run:working'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        states,
        (
            "cancelled".to_string(),
            "cancelled".to_string(),
            "2026-10-20T12:01:00.000Z".to_string()
        )
    );
    let projected: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_conversation_items WHERE id LIKE 'organizational-item:%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(projected, 0, "a settled run projects no late messages");

    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    sqlx::raw_sql(
        "UPDATE chat_agent_runs SET state = 'cancelled', settled_at = '2026-10-20T12:01:00.000Z'
         WHERE id = 'run:working';",
    )
    .execute(&pool)
    .await
    .unwrap();
    let assignment_before: String = sqlx::query_scalar(
        "SELECT assignment.state FROM chat_work_assignments assignment
         JOIN chat_agent_runs run ON run.assignment_id = assignment.id
         WHERE run.id = 'run:working'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    append_canonical_event(
        &pool,
        canonical_request(
            "event:late-after-revocation",
            Some("turn:working"),
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
    let states: (String, String, String) = sqlx::query_as(
        "SELECT assignment.state, run.state, run.settled_at
         FROM chat_work_assignments assignment
         JOIN chat_agent_runs run ON run.assignment_id = assignment.id
         WHERE run.id = 'run:working'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        states,
        (
            assignment_before,
            "cancelled".to_string(),
            "2026-10-20T12:01:00.000Z".to_string()
        ),
        "a run cancelled by revocation stays cancelled while its assignment is still active"
    );
}

#[tokio::test]
async fn provider_events_of_a_settled_assignment_project_no_messages_and_never_reopen_its_run() {
    use crate::chat::tests::repository::canonical_request;
    use ganbaru_chat::repository::events::append_canonical_event;
    use ganbaru_chat_contracts::events::RequestOpenedEvent;
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    // Another device cancelled the assignment while this device's run still works.
    sqlx::query(
        "UPDATE chat_work_assignments SET state = 'cancelled', settled_at = '2026-10-20T12:01:00.000Z'
         WHERE id = (SELECT assignment_id FROM chat_agent_runs WHERE id = 'run:working')",
    )
    .execute(&pool)
    .await
    .unwrap();
    append_canonical_event(
        &pool,
        canonical_request(
            "event:approval",
            Some("turn:working"),
            CanonicalEvent::RequestOpened(RequestOpenedEvent {
                request_id: ProviderRequestId::new("request-settled").unwrap(),
                kind: CanonicalRequestKind::CommandExecution,
                title: "Approve".to_string(),
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
    let run_state: String =
        sqlx::query_scalar("SELECT state FROM chat_agent_runs WHERE id = 'run:working'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        run_state, "working",
        "the run never waits for a settled assignment"
    );

    assert_eq!(
        complete_working_turn(&pool, true).await,
        ("cancelled".to_string(), "completed".to_string()),
        "the run records that its turn ended while the assignment stays settled"
    );
    let projected: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_conversation_items WHERE id LIKE 'organizational-item:%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        projected, 0,
        "a settled assignment shows no late approval or review"
    );
}

/// Inserts the starting run `run_id` of the folder assignment under `launch`.
async fn insert_folder_run(
    pool: &sqlx::SqlitePool,
    run_id: &str,
    launch: &ganbaru_chat::agent_runs::RunLaunch,
) -> ChatResult<()> {
    let binding = ganbaru_chat::agent_runs::AgentRunBinding {
        run_id: ChatAgentRunId::new(run_id).unwrap(),
        run_ordinal: 3,
        ..working_run_binding(pool).await
    };
    let turn_id = format!("turn:{run_id}");
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO chat_turns
            (id, thread_id, ordinal, state, safety_mode, interaction_mode, created_at, updated_at)
         SELECT ?, 'thread-1', max(ordinal) + 1, 'pending', 'ask_for_approval', 'build',
                '2026-10-20T12:02:00.000Z', '2026-10-20T12:02:00.000Z'
         FROM chat_turns WHERE thread_id = 'thread-1'",
    )
    .bind(&turn_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    ganbaru_chat::agent_runs::insert_starting_run(
        &mut transaction,
        ganbaru_chat::agent_runs::StartingAgentRun {
            binding: &binding,
            launch,
            provider_turn_id: &ChatTurnId::new(turn_id.clone()).unwrap(),
            provider_thread_id: &ChatThreadId::new("thread-1").unwrap(),
            now: &UtcTimestamp::new("2026-10-20T12:02:00.000Z".to_string()).unwrap(),
        },
    )
    .await?;
    transaction.commit().await.unwrap();
    Ok(())
}

#[tokio::test]
async fn a_starting_run_records_its_attempt_under_the_claim_that_launches_it() {
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    starting_folder_run(&pool).await;
    sqlx::query("UPDATE chat_agent_runs SET state = 'failed' WHERE id = 'run:working'")
        .execute(&pool)
        .await
        .unwrap();
    insert_folder_run(&pool, "run:handoff", &folder_launch())
        .await
        .unwrap();

    let stored: (String, String, String, String, String, i64) = sqlx::query_as(
        "SELECT attempt_reason, handoff_reason, instructions_sha256, model_selection_sha256,
                context_delivery, delivered_context_bytes
         FROM chat_agent_runs WHERE id = 'run:handoff'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        (
            "handoff".to_string(),
            "key_changed".to_string(),
            "c".repeat(64),
            "d".repeat(64),
            "full".to_string(),
            512
        )
    );

    let stale = ganbaru_chat::agent_runs::RunLaunch {
        claim_token: "claim:stale".to_string(),
        ..folder_launch()
    };
    let refused = insert_folder_run(&pool, "run:stale", &stale)
        .await
        .expect_err("a lost claim starts no run");
    assert_eq!(refused.code, ChatErrorCode::Conflict);
    let runs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chat_agent_runs WHERE id = 'run:stale'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(runs, 0);
}

async fn cancel_folder_assignment(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "UPDATE chat_work_assignments
         SET state = 'cancelled', state_reason = 'Cancelled on another device',
             settled_at = '2026-10-20T12:01:00.000Z'
         WHERE id = 'assignment:folders'",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn a_launch_aborts_when_its_assignment_settled_before_the_run_started() {
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    cancel_folder_assignment(&pool).await;
    let binding = ganbaru_chat::agent_runs::AgentRunBinding {
        run_id: ChatAgentRunId::new("run:late").unwrap(),
        run_ordinal: 3,
        ..working_run_binding(&pool).await
    };
    let mut transaction = pool.begin().await.unwrap();
    let refused = ganbaru_chat::agent_runs::insert_starting_run(
        &mut transaction,
        ganbaru_chat::agent_runs::StartingAgentRun {
            binding: &binding,
            launch: &folder_launch(),
            provider_turn_id: &ChatTurnId::new("turn:queued").unwrap(),
            provider_thread_id: &ChatThreadId::new("thread-1").unwrap(),
            now: &UtcTimestamp::new("2026-10-20T12:02:00.000Z".to_string()).unwrap(),
        },
    )
    .await
    .expect_err("a settled assignment starts no run");
    transaction.rollback().await.unwrap();

    assert_eq!(refused.code, ChatErrorCode::InvalidStateTransition);
    let runs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chat_agent_runs WHERE id = 'run:late'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(runs, 0);
}

#[tokio::test]
async fn a_launch_that_settles_after_a_remote_cancel_keeps_the_assignment_cancelled() {
    use ganbaru_chat::agent_runs::{LaunchSettlement, settle_launch};
    let now = UtcTimestamp::new("2026-10-20T12:02:00.000Z".to_string()).unwrap();
    let launch_error = ChatError::new(ChatErrorCode::DriverUnavailable, "Provider exited", true);
    for error in [None, Some(&launch_error)] {
        let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
        starting_folder_run(&pool).await;
        cancel_folder_assignment(&pool).await;
        let binding = working_run_binding(&pool).await;

        let settlement = settle_launch(&pool, &binding, &folder_launch(), error, &now)
            .await
            .unwrap();

        assert_eq!(settlement, LaunchSettlement::Superseded);
        assert_eq!(
            folder_assignment_and_run(&pool).await,
            (
                "cancelled".to_string(),
                "cancelled".to_string(),
                "cancelled".to_string()
            )
        );
        let failure_messages: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_conversation_items WHERE id LIKE 'organizational-item:%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(failure_messages, 0);
    }

    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    starting_folder_run(&pool).await;
    let binding = working_run_binding(&pool).await;
    let settlement = settle_launch(&pool, &binding, &folder_launch(), None, &now)
        .await
        .unwrap();
    assert_eq!(settlement, LaunchSettlement::Recorded);
    assert_eq!(
        folder_assignment_and_run(&pool).await,
        (
            "working".to_string(),
            "working".to_string(),
            "completed".to_string()
        )
    );
}

#[tokio::test]
async fn a_launch_cannot_rewrite_a_run_that_revocation_already_cancelled() {
    use ganbaru_chat::agent_runs::{LaunchSettlement, settle_launch};
    let now = UtcTimestamp::new("2026-10-20T12:02:00.000Z".to_string()).unwrap();
    let launch_error = ChatError::new(ChatErrorCode::DriverUnavailable, "Provider exited", true);
    for error in [None, Some(&launch_error)] {
        let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
        starting_folder_run(&pool).await;
        sqlx::query(
            "UPDATE chat_agent_runs
             SET state = 'cancelled', settled_at = '2026-10-20T12:01:00.000Z'
             WHERE id = 'run:working'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let binding = working_run_binding(&pool).await;
        let revision_before: i64 = sqlx::query_scalar(
            "SELECT revision FROM chat_work_assignments WHERE id = 'assignment:folders'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let settlement = settle_launch(&pool, &binding, &folder_launch(), error, &now)
            .await
            .unwrap();

        assert_eq!(settlement, LaunchSettlement::RunSettled);
        let rows: (String, i64, String, Option<String>, String, Option<String>) = sqlx::query_as(
            "SELECT assignment.state, assignment.revision, run.state, run.settled_at,
                        job.state, job.last_error
                 FROM chat_work_assignments assignment
                 JOIN chat_agent_runs run ON run.assignment_id = assignment.id
                 JOIN chat_assignment_dispatch_jobs job ON job.assignment_id = assignment.id
                 WHERE run.id = 'run:working'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let (assignment_state, assignment_revision, run_state, settled_at, job_state, last_error) =
            rows;
        assert_eq!(assignment_state, "queued");
        assert_eq!(run_state, "cancelled");
        assert_eq!(settled_at.as_deref(), Some("2026-10-20T12:01:00.000Z"));
        assert_eq!(job_state, "claimed");
        assert_eq!(last_error, None);
        assert_eq!(assignment_revision, revision_before);
        let failure_messages: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_conversation_items WHERE id LIKE 'organizational-item:%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(failure_messages, 0);
    }
}

#[tokio::test]
async fn a_launch_of_a_missing_run_stops_the_session_and_writes_nothing() {
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    starting_folder_run(&pool).await;
    let binding = ganbaru_chat::agent_runs::AgentRunBinding {
        run_id: ChatAgentRunId::new("run:missing").unwrap(),
        ..working_run_binding(&pool).await
    };
    let now = UtcTimestamp::new("2026-10-20T12:02:00.000Z".to_string()).unwrap();

    let settlement =
        ganbaru_chat::agent_runs::settle_launch(&pool, &binding, &folder_launch(), None, &now)
            .await
            .unwrap();

    assert_eq!(
        settlement,
        ganbaru_chat::agent_runs::LaunchSettlement::RunSettled
    );
    assert_eq!(
        folder_assignment_and_run(&pool).await,
        (
            "queued".to_string(),
            "starting".to_string(),
            "claimed".to_string()
        )
    );
}

#[tokio::test]
async fn provider_events_after_a_remote_cancel_cannot_reopen_the_assignment() {
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    cancel_folder_assignment(&pool).await;

    let (assignment_state, _) = complete_working_turn(&pool, true).await;

    assert_eq!(assignment_state, "cancelled");
}
