use super::*;
use crate::coordination::reports::HostReportRefusal;
use crate::coordination::reports::tests::{
    ASSIGNMENT, NOW, OWNER, RUN, THREAD, call, migrated_pool, post_input, seed, states,
};
use sqlx::SqlitePool;

fn request() -> PushRequest {
    PushRequest {
        working_folder_id: "folder:reports".to_string(),
        execution_environment_id: None,
        target: GitPushTarget {
            local_branch: "topic".to_string(),
            remote: "origin".to_string(),
            destination_branch: "topic".to_string(),
            push_url: "git@example.invalid:org/repo.git".to_string(),
            object_id: "a".repeat(40),
        },
        git_config_digest: "b".repeat(64),
    }
}

async fn ask_push(pool: &SqlitePool, client_request_id: &str) -> ChatResult<HostCallOutcome> {
    let mut connection = pool.acquire().await.unwrap();
    record_host_push_request(
        &mut connection,
        call(client_request_id),
        "Push the docs fix",
        &request(),
        NOW,
    )
    .await
}

fn item_id(client_request_id: &str) -> String {
    HostUpdateIds::derive(PUSH_REQUEST_ID_PART, call(client_request_id))
        .unwrap()
        .item_id
}

async fn read(pool: &SqlitePool, item_id: &str) -> ChatResult<StoredPushRequest> {
    let mut connection = pool.acquire().await.unwrap();
    read_push_request(&mut connection, item_id).await
}

/// Runs `sql` as a sync apply, so state guards that only hold for local writes stay out.
async fn applied(pool: &SqlitePool, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         {sql};
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;"
    )))
    .execute(pool)
    .await
    .unwrap();
}

async fn set_run_state(pool: &SqlitePool, state: &str) {
    applied(
        pool,
        &format!("UPDATE chat_agent_runs SET state = '{state}' WHERE id = '{RUN}'"),
    )
    .await;
}

#[test]
fn refusals_round_trip_their_codes_and_carry_a_reason() {
    for refusal in PushRequestRefusal::ALL {
        let error = refusal.into_error();
        assert_eq!(PushRequestRefusal::from_error(&error), Some(refusal));
        assert_eq!(error.details.unwrap()["reason"], refusal.code());
    }
}

#[test]
fn summaries_are_trimmed_and_bounded() {
    assert_eq!(normalize_push_summary("  Push it \n"), Ok("Push it"));
    assert_eq!(
        normalize_push_summary(" \n"),
        Err(PushRequestRefusal::SummaryInvalid)
    );
    assert_eq!(
        normalize_push_summary(&"é".repeat(MAX_PUSH_SUMMARY_CHARS)).map(str::len),
        Ok(MAX_PUSH_SUMMARY_CHARS * 2)
    );
    assert_eq!(
        normalize_push_summary(&"a".repeat(MAX_PUSH_SUMMARY_CHARS + 1)),
        Err(PushRequestRefusal::SummaryInvalid)
    );
}

#[test]
fn a_request_with_an_unsafe_field_is_malformed() {
    assert!(request().is_well_formed());
    let cases: [fn(&mut PushRequest); 8] = [
        |request| request.target.object_id = "HEAD".to_string(),
        |request| request.target.destination_branch = "+main".to_string(),
        |request| request.target.local_branch = "a..b".to_string(),
        |request| request.target.remote = "--upload-pack=x".to_string(),
        |request| request.target.push_url = "https://u:p@example.invalid/r".to_string(),
        |request| request.git_config_digest = "B".repeat(64),
        |request| request.working_folder_id = String::new(),
        |request| request.execution_environment_id = Some("a b".to_string()),
    ];
    for change in cases {
        let mut changed = request();
        change(&mut changed);
        assert!(!changed.is_well_formed(), "{changed:?}");
    }
}

#[tokio::test]
async fn a_push_request_is_a_question_that_waits_for_the_person() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    assert_eq!(
        ask_push(&pool, "request-1").await.unwrap(),
        HostCallOutcome::Posted
    );
    assert_eq!(states(&pool).await.0, "waiting_for_answer");
    let item_id = item_id("request-1");
    let stored = read(&pool, &item_id).await.unwrap();
    assert_eq!(stored.request, request());
    assert_eq!(stored.summary, "Push the docs fix");
    assert_eq!(stored.state, PushRequestState::Pending);
    assert_eq!(stored.reply_thread_id, THREAD);
    assert_eq!(stored.assignment_id, ASSIGNMENT);
    assert_eq!(
        stored.authorization_revision_id.as_deref(),
        Some("authorization:reports")
    );
    assert_eq!(stored.teammate_name.as_deref(), Some("Helper"));

    let mut connection = pool.acquire().await.unwrap();
    assert!(
        crate::coordination::reports::run_asked_host_question(&mut connection, RUN)
            .await
            .unwrap()
    );
    drop(connection);

    set_run_state(&pool, "completed").await;
    assert_eq!(
        read(&pool, &item_id).await.unwrap().state,
        PushRequestState::Open
    );
    assert_eq!(
        ask_push(&pool, "request-1").await.unwrap(),
        HostCallOutcome::Replayed
    );
}

#[tokio::test]
async fn a_request_closes_once_a_person_replies_or_the_run_does_not_complete() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    ask_push(&pool, "request-1").await.unwrap();
    let item_id = item_id("request-1");
    set_run_state(&pool, "failed").await;
    assert_eq!(
        read(&pool, &item_id).await.unwrap().state,
        PushRequestState::Closed
    );
    set_run_state(&pool, "completed").await;
    post_input(&pool, "item:answer", OWNER, "queued_continuation").await;
    let closed = read(&pool, &item_id).await.unwrap();
    assert_eq!(closed.state, PushRequestState::Closed);
    assert_eq!(closed.state.require_open(), Err(PushRequestRefusal::Closed));
}

#[tokio::test]
async fn a_newer_question_supersedes_a_push_request() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    ask_push(&pool, "request-1").await.unwrap();
    applied(
        &pool,
        &format!("UPDATE chat_work_assignments SET state = 'working' WHERE id = '{ASSIGNMENT}'"),
    )
    .await;
    let mut connection = pool.acquire().await.unwrap();
    crate::coordination::reports::record_host_question(
        &mut connection,
        call("request-2"),
        "Which branch?",
        &[],
        NOW,
    )
    .await
    .unwrap();
    drop(connection);
    set_run_state(&pool, "completed").await;
    assert_eq!(
        read(&pool, &item_id("request-1")).await.unwrap().state,
        PushRequestState::Closed
    );
}

#[tokio::test]
async fn a_request_needs_a_working_assignment_and_a_well_formed_target() {
    let pool = migrated_pool().await;
    seed(&pool, "waiting_for_answer", "working").await;
    let refused = ask_push(&pool, "request-1").await.unwrap_err();
    assert_eq!(
        HostReportRefusal::from_error(&refused),
        Some(HostReportRefusal::AssignmentInactive)
    );

    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    let mut malformed = request();
    malformed.target.object_id = "main".to_string();
    let mut connection = pool.acquire().await.unwrap();
    let refused =
        record_host_push_request(&mut connection, call("request-1"), "Push", &malformed, NOW)
            .await
            .unwrap_err();
    assert_eq!(refused.code, ChatErrorCode::Validation);
    drop(connection);
    assert_eq!(states(&pool).await.0, "working");
}

#[tokio::test]
async fn an_ordinary_question_or_a_tampered_payload_is_not_a_push_request() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    let mut connection = pool.acquire().await.unwrap();
    crate::coordination::reports::record_host_question(
        &mut connection,
        call("request-1"),
        "Which branch?",
        &[],
        NOW,
    )
    .await
    .unwrap();
    drop(connection);
    let question = HostUpdateIds::derive("ask", call("request-1"))
        .unwrap()
        .item_id;
    assert_eq!(
        read(&pool, &question).await.unwrap_err().code,
        ChatErrorCode::NotFound
    );

    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    ask_push(&pool, "request-1").await.unwrap();
    let item_id = item_id("request-1");
    applied(
        &pool,
        &format!(
            "UPDATE chat_work_semantic_updates
             SET payload_data = json_set(payload_data, '$.payload.push.pushUrl', 'https://u:p@h/r')
             WHERE item_id = '{item_id}'"
        ),
    )
    .await;
    assert_eq!(
        read(&pool, &item_id).await.unwrap_err().code,
        ChatErrorCode::Validation
    );
}

#[tokio::test]
async fn the_state_read_follows_the_run_and_marks_a_confirmed_request_pushed() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    ask_push(&pool, "request-1").await.unwrap();
    let item_id = item_id("request-1");
    let state = |item_id: String| {
        let pool = pool.clone();
        async move {
            let mut connection = pool.acquire().await.unwrap();
            read_push_request_state(&mut connection, &item_id)
                .await
                .unwrap()
        }
    };
    assert_eq!(state(item_id.clone()).await, ChatPushRequestState::Waiting);
    set_run_state(&pool, "completed").await;
    assert_eq!(state(item_id.clone()).await, ChatPushRequestState::Open);
    assert_eq!(
        state("item:unknown".to_string()).await,
        ChatPushRequestState::Closed
    );

    let reference_id = push_confirmation_id(PUSH_CONFIRMATION_REFERENCE_PREFIX, &item_id).unwrap();
    assert_eq!(
        reference_id,
        push_confirmation_id(PUSH_CONFIRMATION_REFERENCE_PREFIX, &item_id).unwrap()
    );
    assert_ne!(
        reference_id,
        push_confirmation_id(PUSH_CONFIRMATION_REFERENCE_PREFIX, "item:other").unwrap()
    );
    applied(
        &pool,
        &format!(
            "INSERT INTO chat_message_references
               (id, message_revision_id, reference_kind, label_snapshot, plain_text_projection,
                start_offset, end_offset, ordinal, created_at)
             VALUES ('{reference_id}', 'revision:confirmation', 'participant', 'Helper',
                     '@Helper', 0, 7, 0, '{NOW}')"
        ),
    )
    .await;
    post_input(&pool, "item:answer", OWNER, "queued_continuation").await;
    assert_eq!(state(item_id).await, ChatPushRequestState::Pushed);
}
