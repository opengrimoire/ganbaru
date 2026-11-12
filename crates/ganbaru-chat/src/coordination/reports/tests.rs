use super::*;
use crate::models::UtcTimestamp;
use crate::project_defaults::{default_channel_ids, ensure_project_chat_defaults};
use sqlx::SqlitePool;

pub(crate) const NOW: &str = "2026-11-12T09:00:00.000Z";
const LATER: &str = "2026-11-12T09:05:00.000Z";
pub(crate) const PROJECT_ID: &str = "project-host-reports";
pub(crate) const TEAMMATE: &str = "participant:reports-helper";
const OWNER: &str = "participant:local-owner";
pub(crate) const ASSIGNMENT: &str = "assignment:reports";
pub(crate) const RUN: &str = "run:reports-1";
const TURN: &str = "turn:reports-1";
pub(crate) const THREAD: &str = "reply:reports";

/// A migrated in-memory pool. Foreign keys stay off, and the run is seeded as a sync apply, so
/// a run needs none of the policy, authorization, and provider rows its dispatch would create.
pub(crate) async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=OFF")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

/// Seeds an assignment of the helper in `assignment_state` whose run is in `run_state`.
pub(crate) async fn seed(pool: &SqlitePool, assignment_state: &str, run_state: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-reports', 'R', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('{PROJECT_ID}', 'group-reports', 'Reports', 'a0');"
    )))
    .execute(pool)
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    ensure_project_chat_defaults(
        &mut connection,
        PROJECT_ID,
        &UtcTimestamp::new(NOW.to_string()).unwrap(),
    )
    .await
    .unwrap();
    drop(connection);
    let conversation = default_channel_ids(PROJECT_ID).unwrap().0;
    let digest = "a".repeat(64);
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('{TEAMMATE}', 'ai_teammate', 'Helper', '{NOW}', '{NOW}');
         INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
         VALUES ('{TEAMMATE}', 'Helper', '{NOW}', '{NOW}');
         INSERT INTO chat_conversation_items (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:root', '{conversation}', 'message', 'c000', '{NOW}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('item:root', '{OWNER}', '{NOW}');
         INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:root', 'item:root', '@Helper tidy the docs', '{NOW}');
         UPDATE chat_communication_messages SET current_revision_id = 'revision:root'
         WHERE item_id = 'item:root';
         INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES ('{THREAD}', '{conversation}', 'item:root', '{NOW}', '{NOW}', '{NOW}');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, teammate_id, triggering_message_item_id, state,
             created_at, updated_at)
         VALUES ('{ASSIGNMENT}', '{THREAD}', '{TEAMMATE}', 'item:root', '{assignment_state}',
                 '{NOW}', '{NOW}');
         UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         INSERT INTO chat_agent_runs
            (id, assignment_id, project_id, teammate_policy_revision_id,
             authorization_revision_id, authorization_scope_digest, provider_turn_id,
             provider_thread_id, state, run_ordinal, created_at, updated_at)
         VALUES ('{RUN}', '{ASSIGNMENT}', '{PROJECT_ID}', 'policy:reports', 'authorization:reports',
                 '{digest}', '{TURN}', 'thread:reports', '{run_state}', 1, '{NOW}', '{NOW}');
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;"
    )))
    .execute(pool)
    .await
    .unwrap();
}

/// Posts a message by `author` in the reply thread and records it as a pending input of
/// `routing_kind`.
async fn post_input(pool: &SqlitePool, item_id: &str, author: &str, routing_kind: &str) {
    let conversation = default_channel_ids(PROJECT_ID).unwrap().0;
    let mut connection = pool.acquire().await.unwrap();
    let order_key = next_order_key(&mut connection, &conversation, Some(THREAD))
        .await
        .unwrap();
    let ordinal: i64 =
        sqlx::query_scalar("SELECT coalesce(max(ordinal), 0) + 1 FROM chat_work_assignment_inputs")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    drop(connection);
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES ('{item_id}', '{conversation}', '{THREAD}', 'message', '{order_key}', '{LATER}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('{item_id}', '{author}', '{LATER}');
         INSERT INTO chat_work_assignment_inputs
            (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
         VALUES ('input:{item_id}', '{ASSIGNMENT}', '{item_id}',
                 {ordinal},
                 '{routing_kind}', '{LATER}');"
    )))
    .execute(pool)
    .await
    .unwrap();
}

pub(crate) fn call(client_request_id: &str) -> HostCall<'_> {
    HostCall {
        run_id: RUN,
        assignment_id: ASSIGNMENT,
        client_request_id,
    }
}

async fn ask(pool: &SqlitePool, request: &str, question: &str) -> ChatResult<HostCallOutcome> {
    let mut connection = pool.acquire().await.unwrap();
    record_host_question(
        &mut connection,
        call(request),
        question,
        &["Keep it".to_owned(), "Drop it".to_owned()],
        NOW,
    )
    .await
}

async fn report(
    pool: &SqlitePool,
    request: &str,
    kind: ReportKind,
    body: &str,
) -> ChatResult<HostCallOutcome> {
    let mut connection = pool.acquire().await.unwrap();
    record_host_report(&mut connection, call(request), kind, body, NOW).await
}

async fn states(pool: &SqlitePool) -> (String, String) {
    sqlx::query_as(
        "SELECT assignment.state, run.state FROM chat_work_assignments assignment
         JOIN chat_agent_runs run ON run.assignment_id = assignment.id
         WHERE assignment.id = ?",
    )
    .bind(ASSIGNMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Every teammate update in the reply thread, as `(update kind, markdown)`.
async fn updates(pool: &SqlitePool) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT semantic.update_kind, revision.normalized_markdown
         FROM chat_work_semantic_updates semantic
         JOIN chat_conversation_items item ON item.id = semantic.item_id
         JOIN chat_communication_messages message ON message.item_id = semantic.item_id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE item.reply_thread_id = ? ORDER BY item.order_key",
    )
    .bind(THREAD)
    .fetch_all(pool)
    .await
    .unwrap()
}

fn refusal(result: ChatResult<HostCallOutcome>) -> Option<HostReportRefusal> {
    HostReportRefusal::from_error(&result.unwrap_err())
}

#[test]
fn report_kinds_and_refusals_round_trip_their_wire_names() {
    for kind in ReportKind::ALL {
        assert_eq!(ReportKind::from_name(kind.as_str()), Some(kind));
    }
    assert_eq!(ReportKind::from_name("note_proposal"), None);
    for refusal in HostReportRefusal::ALL {
        assert_eq!(
            HostReportRefusal::from_error(&refusal.into_error()),
            Some(refusal)
        );
    }
    let other = ChatError::new(ChatErrorCode::Validation, "report_empty_body", false);
    assert_eq!(HostReportRefusal::from_error(&other), None);
}

#[test]
fn report_bodies_are_trimmed_and_bounded() {
    assert_eq!(validate_report_body("  Plan  "), Ok("Plan"));
    assert_eq!(
        validate_report_body(" \n "),
        Err(HostReportRefusal::EmptyBody)
    );
    let limit = "a".repeat(MAX_REPORT_BODY_BYTES);
    assert_eq!(validate_report_body(&limit), Ok(limit.as_str()));
    assert_eq!(
        validate_report_body(&format!("{limit}a")),
        Err(HostReportRefusal::BodyTooLarge)
    );
}

#[tokio::test]
async fn a_question_waits_for_an_answer_and_replays_only_the_same_input() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;

    assert_eq!(
        ask(&pool, "ask-1", "Keep the old page?").await.unwrap(),
        HostCallOutcome::Posted
    );
    assert_eq!(
        states(&pool).await,
        ("waiting_for_answer".to_owned(), "working".to_owned())
    );
    assert_eq!(
        updates(&pool).await,
        vec![(
            "question".to_owned(),
            "Keep the old page?\n\n- Keep it\n- Drop it".to_owned()
        )]
    );
    let mut connection = pool.acquire().await.unwrap();
    assert!(run_asked_host_question(&mut connection, RUN).await.unwrap());
    drop(connection);

    assert_eq!(
        ask(&pool, "ask-1", "Keep the old page?").await.unwrap(),
        HostCallOutcome::Replayed
    );
    assert_eq!(
        refusal(ask(&pool, "ask-1", "Remove the old page?").await),
        Some(HostReportRefusal::RequestConflict)
    );
    assert_eq!(
        refusal(ask(&pool, "ask-2", "Another question?").await),
        Some(HostReportRefusal::AssignmentInactive),
        "only working work can ask"
    );
    assert_eq!(updates(&pool).await.len(), 1);
}

#[tokio::test]
async fn a_settled_run_can_neither_ask_nor_report() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "completed").await;
    assert_eq!(
        refusal(ask(&pool, "ask-1", "Keep it?").await),
        Some(HostReportRefusal::AssignmentInactive)
    );
    assert_eq!(
        refusal(report(&pool, "report-1", ReportKind::Plan, "Plan").await),
        Some(HostReportRefusal::AssignmentInactive)
    );
    assert!(updates(&pool).await.is_empty());
}

#[tokio::test]
async fn reports_post_while_working_or_waiting_and_replay_by_request_id() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    report(&pool, "report-1", ReportKind::Plan, "1. Read 2. Write")
        .await
        .unwrap();
    ask(&pool, "ask-1", "Keep the old page?").await.unwrap();
    report(&pool, "report-2", ReportKind::Replan, "Wait for the answer")
        .await
        .unwrap();
    assert_eq!(
        report(&pool, "report-1", ReportKind::Plan, "1. Read 2. Write")
            .await
            .unwrap(),
        HostCallOutcome::Replayed
    );
    assert_eq!(
        refusal(report(&pool, "report-1", ReportKind::Result, "1. Read 2. Write").await),
        Some(HostReportRefusal::RequestConflict)
    );
    assert_eq!(
        refusal(report(&pool, "report-1", ReportKind::Plan, "Other plan").await),
        Some(HostReportRefusal::RequestConflict)
    );
    let kinds: Vec<String> = updates(&pool)
        .await
        .into_iter()
        .map(|(kind, _)| kind)
        .collect();
    assert_eq!(kinds, ["plan", "question", "replan"]);

    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(run_result_report(&mut connection, RUN).await.unwrap(), None);
    drop(connection);
    report(&pool, "report-3", ReportKind::Result, "Docs tidied")
        .await
        .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(
        run_result_report(&mut connection, RUN).await.unwrap(),
        Some("Docs tidied".to_owned())
    );
}

#[tokio::test]
async fn an_answer_to_a_working_run_resumes_the_assignment() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    ask(&pool, "ask-1", "Keep the old page?").await.unwrap();
    post_input(&pool, "item:teammate-note", TEAMMATE, "steer").await;
    post_input(&pool, "item:answer", OWNER, "steer").await;

    let mut connection = pool.acquire().await.unwrap();
    assert!(
        !resume_answered_question(&mut connection, ASSIGNMENT, "item:teammate-note", LATER)
            .await
            .unwrap(),
        "a teammate message is not an answer"
    );
    assert!(
        resume_answered_question(&mut connection, ASSIGNMENT, "item:answer", LATER)
            .await
            .unwrap()
    );
    drop(connection);
    assert_eq!(
        states(&pool).await,
        ("working".to_owned(), "working".to_owned())
    );
}

#[tokio::test]
async fn only_a_person_answer_after_the_asking_run_ended_awaits_dispatch() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    ask(&pool, "ask-1", "Keep the old page?").await.unwrap();
    let awaiting = |pool: SqlitePool| async move {
        let mut connection = pool.acquire().await.unwrap();
        answered_question_awaiting_dispatch(&mut connection, ASSIGNMENT)
            .await
            .unwrap()
    };

    post_input(&pool, "item:early", OWNER, "steer").await;
    assert!(!awaiting(pool.clone()).await, "the asking run still works");

    sqlx::query("UPDATE chat_agent_runs SET state = 'completed' WHERE id = ?")
        .bind(RUN)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE chat_work_assignment_inputs SET delivery_state = 'delivered'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(!awaiting(pool.clone()).await, "nobody answered yet");

    post_input(&pool, "item:teammate-note", TEAMMATE, "queued_continuation").await;
    assert!(!awaiting(pool.clone()).await, "a teammate does not answer");

    post_input(&pool, "item:answer", OWNER, "queued_continuation").await;
    assert!(awaiting(pool.clone()).await);
}

/// Posts a message with `markdown` by `author` as a pending input of `routing_kind`.
async fn post_message(
    pool: &SqlitePool,
    item_id: &str,
    author: &str,
    routing_kind: &str,
    markdown: &str,
) {
    post_input(pool, item_id, author, routing_kind).await;
    sqlx::query(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown, created_at)
         VALUES (?1 || ':revision', ?1, ?2, ?3)",
    )
    .bind(item_id)
    .bind(markdown)
    .bind(LATER)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE chat_communication_messages SET current_revision_id = ?1 || ':revision'
         WHERE item_id = ?1",
    )
    .bind(item_id)
    .execute(pool)
    .await
    .unwrap();
}

/// Seeds an assignment whose completed run asked a question, with a completed dispatch job.
async fn seed_asked_and_completed(pool: &SqlitePool) {
    seed(pool, "working", "working").await;
    ask(pool, "ask-1", "Keep the old page?").await.unwrap();
    sqlx::query("UPDATE chat_agent_runs SET state = 'completed' WHERE id = ?")
        .bind(RUN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO chat_assignment_dispatch_jobs
            (id, assignment_id, state, attempt_count, available_at, claimed_at, claim_token,
             created_at, updated_at)
         VALUES ('job:reports', ?1, 'completed', 1, ?2, ?2, 'token', ?2, ?2)",
    )
    .bind(ASSIGNMENT)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

async fn requeue(pool: &SqlitePool, device_id: &str) -> bool {
    let mut connection = pool.acquire().await.unwrap();
    requeue_answered_assignment(&mut connection, ASSIGNMENT, device_id, LATER)
        .await
        .unwrap()
}

async fn job_state(pool: &SqlitePool) -> String {
    sqlx::query_scalar("SELECT state FROM chat_assignment_dispatch_jobs WHERE assignment_id = ?")
        .bind(ASSIGNMENT)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Sets the executing device to the SQL literal `device`. The column is fixed once written, so
/// the fixture writes it as a sync apply.
async fn set_executing_device(pool: &SqlitePool, device: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         UPDATE chat_work_assignments SET executing_device_id = {device};
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;"
    )))
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn an_answered_question_requeues_the_assignment_once() {
    let pool = migrated_pool().await;
    seed_asked_and_completed(&pool).await;
    assert!(!requeue(&pool, "device:this").await, "nobody answered yet");

    post_message(
        &pool,
        "item:answer",
        OWNER,
        "queued_continuation",
        "Keep it",
    )
    .await;
    assert!(requeue(&pool, "device:this").await);
    assert_eq!(states(&pool).await.0, "queued");
    assert_eq!(job_state(&pool).await, "queued");
    assert!(
        !requeue(&pool, "device:this").await,
        "a requeued assignment is not requeued again"
    );
}

#[tokio::test]
async fn an_answered_question_stays_waiting_on_another_device_or_without_a_finished_job() {
    let pool = migrated_pool().await;
    seed_asked_and_completed(&pool).await;
    post_message(
        &pool,
        "item:answer",
        OWNER,
        "queued_continuation",
        "Keep it",
    )
    .await;
    set_executing_device(&pool, "'device:other'").await;
    assert!(!requeue(&pool, "device:this").await);
    assert_eq!(states(&pool).await.0, "waiting_for_answer");

    set_executing_device(&pool, "NULL").await;
    sqlx::query("UPDATE chat_assignment_dispatch_jobs SET state = 'claimed'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        !requeue(&pool, "device:this").await,
        "a dispatch is in flight"
    );
    assert_eq!(states(&pool).await.0, "waiting_for_answer");
}

#[tokio::test]
async fn the_answered_question_holds_the_question_and_only_person_answers_in_order() {
    let pool = migrated_pool().await;
    seed_asked_and_completed(&pool).await;
    let read = |pool: SqlitePool| async move {
        let mut connection = pool.acquire().await.unwrap();
        read_answered_question(&mut connection, ASSIGNMENT)
            .await
            .unwrap()
    };
    assert_eq!(read(pool.clone()).await, None, "nobody answered yet");

    post_message(&pool, "item:note", TEAMMATE, "queued_continuation", "Noted").await;
    post_message(&pool, "item:first", OWNER, "queued_continuation", "Keep it").await;
    post_message(&pool, "item:second", OWNER, "steer", "And link it").await;
    let answered = read(pool.clone()).await.expect("answered");
    assert_eq!(answered.asking_run_id, RUN);
    assert!(answered.question.markdown.starts_with("Keep the old page?"));
    let answers = answered
        .answers
        .iter()
        .map(|answer| (answer.item_id.as_str(), answer.markdown.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        answers,
        [("item:first", "Keep it"), ("item:second", "And link it")]
    );
}

#[cfg(feature = "native-runtime")]
mod projection {
    use super::*;
    use crate::coordination::projection::project_provider_event_in_transaction;
    use crate::events::{CanonicalEvent, CanonicalRuntimeEvent, TurnCompletedEvent};
    use crate::models::{
        ChatEventId, ChatThreadId, ChatTurnId, ChatTurnState, ProviderFamilyId, ProviderInstanceId,
    };

    /// Records the provider turn of the run, with `changed_files` as its changed file summary.
    async fn seed_turn(pool: &SqlitePool, changed_files: &str) {
        sqlx::query(
            "INSERT INTO chat_turns
                (id, thread_id, ordinal, state, safety_mode, interaction_mode,
                 changed_file_summary_schema_version, changed_file_summary_data,
                 created_at, updated_at)
             VALUES (?, 'thread:reports', 1, 'completed', 'ask_for_approval', 'build', 1, ?, ?, ?)",
        )
        .bind(TURN)
        .bind(changed_files)
        .bind(NOW)
        .bind(NOW)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn complete_turn(pool: &SqlitePool) {
        let runtime = CanonicalRuntimeEvent {
            schema_version: 1,
            event_id: ChatEventId::new("event:reports-completed".to_owned()).unwrap(),
            provider_family_id: ProviderFamilyId::new("codex".to_owned()).unwrap(),
            provider_instance_id: ProviderInstanceId::new("codex-local".to_owned()).unwrap(),
            thread_id: ChatThreadId::new("thread:reports".to_owned()).unwrap(),
            created_at: UtcTimestamp::new(LATER.to_owned()).unwrap(),
            turn_id: Some(ChatTurnId::new(TURN.to_owned()).unwrap()),
            provider_turn_id: None,
            provider_item_id: None,
            provider_request_id: None,
            provider_task_id: None,
            provider_reference: None,
            event: CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                state: ChatTurnState::Completed,
                stop_reason: None,
                usage: None,
                changed_files: Vec::new(),
            }),
            redacted_diagnostic: None,
        };
        let mut transaction = pool.begin().await.unwrap();
        project_provider_event_in_transaction(&mut transaction, &runtime)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }

    const ONE_CHANGED_FILE: &str = r#"[{"relativePath":"docs/a.md","previousRelativePath":null,"additions":1,"deletions":0,"binary":false,"status":"modified"}]"#;

    #[tokio::test]
    async fn projection_of_an_asking_turn_settles_only_the_run() {
        let pool = migrated_pool().await;
        seed(&pool, "working", "working").await;
        seed_turn(&pool, ONE_CHANGED_FILE).await;
        ask(&pool, "ask-1", "Keep the old page?").await.unwrap();

        complete_turn(&pool).await;

        assert_eq!(
            states(&pool).await,
            ("waiting_for_answer".to_owned(), "completed".to_owned())
        );
        let kinds: Vec<String> = updates(&pool)
            .await
            .into_iter()
            .map(|(kind, _)| kind)
            .collect();
        assert_eq!(kinds, ["question"]);
    }

    #[tokio::test]
    async fn projection_after_a_result_report_goes_to_review_without_an_inferred_result() {
        let pool = migrated_pool().await;
        seed(&pool, "working", "working").await;
        seed_turn(&pool, "[]").await;
        report(&pool, "report-1", ReportKind::Result, "Docs tidied")
            .await
            .unwrap();

        complete_turn(&pool).await;

        assert_eq!(
            states(&pool).await,
            ("ready_for_review".to_owned(), "completed".to_owned())
        );
        assert_eq!(
            updates(&pool).await,
            vec![("result".to_owned(), "Docs tidied".to_owned())]
        );
    }

    #[tokio::test]
    async fn projection_after_a_result_report_with_changed_files_posts_the_review() {
        let pool = migrated_pool().await;
        seed(&pool, "working", "working").await;
        seed_turn(&pool, ONE_CHANGED_FILE).await;
        report(&pool, "report-1", ReportKind::Result, "Docs tidied")
            .await
            .unwrap();

        complete_turn(&pool).await;

        assert_eq!(
            states(&pool).await,
            ("ready_for_review".to_owned(), "completed".to_owned())
        );
        assert_eq!(
            updates(&pool).await,
            vec![
                ("result".to_owned(), "Docs tidied".to_owned()),
                ("review".to_owned(), "Docs tidied".to_owned()),
            ]
        );
        let changed: String = sqlx::query_scalar(
            "SELECT json_extract(payload_data, '$.payload.changedFiles[0].relativePath')
             FROM chat_work_semantic_updates WHERE update_kind = 'review'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(changed, "docs/a.md");
    }

    #[tokio::test]
    async fn projection_without_a_report_keeps_inferring_the_result() {
        let pool = migrated_pool().await;
        seed(&pool, "working", "working").await;
        seed_turn(&pool, "[]").await;
        report(&pool, "report-1", ReportKind::Plan, "1. Read")
            .await
            .unwrap();

        complete_turn(&pool).await;

        assert_eq!(
            states(&pool).await,
            ("completed".to_owned(), "completed".to_owned())
        );
        assert_eq!(
            updates(&pool).await,
            vec![
                ("plan".to_owned(), "1. Read".to_owned()),
                ("result".to_owned(), "Work completed.".to_owned()),
            ]
        );
    }

    fn usage_report(input: u64, output: u64) -> crate::events::ThreadUsageUpdatedEvent {
        crate::events::ThreadUsageUpdatedEvent {
            input_tokens: Some(input),
            output_tokens: Some(output),
            cached_input_tokens: Some(0),
            cache_write_input_tokens: Some(0),
            context_tokens: None,
            context_limit: None,
            cost: None,
            count_scope: Some(crate::events::UsageCountScope::Turn),
        }
    }

    fn runtime_event(event_id: &str, event: CanonicalEvent) -> CanonicalRuntimeEvent {
        CanonicalRuntimeEvent {
            schema_version: 1,
            event_id: ChatEventId::new(event_id.to_owned()).unwrap(),
            provider_family_id: ProviderFamilyId::new("claude".to_owned()).unwrap(),
            provider_instance_id: ProviderInstanceId::new("claude-local".to_owned()).unwrap(),
            thread_id: ChatThreadId::new("thread:reports".to_owned()).unwrap(),
            created_at: UtcTimestamp::new(LATER.to_owned()).unwrap(),
            turn_id: Some(ChatTurnId::new(TURN.to_owned()).unwrap()),
            provider_turn_id: None,
            provider_item_id: None,
            provider_request_id: None,
            provider_task_id: None,
            provider_reference: None,
            event,
            redacted_diagnostic: None,
        }
    }

    async fn counted_tokens(pool: &SqlitePool) -> Option<i64> {
        sqlx::query_scalar("SELECT counted_tokens FROM chat_agent_run_usage WHERE run_id = ?")
            .bind(RUN)
            .fetch_optional(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn usage_event_updates_run_counters_in_one_transaction() {
        let pool = migrated_pool().await;
        seed(&pool, "working", "working").await;
        let usage = runtime_event(
            "event:reports-usage",
            CanonicalEvent::ThreadUsageUpdated(usage_report(30, 12)),
        );

        let mut transaction = pool.begin().await.unwrap();
        let organizational = project_provider_event_in_transaction(&mut transaction, &usage)
            .await
            .unwrap();
        assert!(!organizational, "usage changes no organizational state");
        transaction.rollback().await.unwrap();
        assert_eq!(counted_tokens(&pool).await, None, "the counters roll back");

        let mut transaction = pool.begin().await.unwrap();
        project_provider_event_in_transaction(&mut transaction, &usage)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(counted_tokens(&pool).await, Some(42));
        assert_eq!(states(&pool).await.1, "working");
    }

    #[tokio::test]
    async fn usage_on_the_completed_turn_counts_once_after_its_report() {
        let pool = migrated_pool().await;
        seed(&pool, "working", "working").await;
        seed_turn(&pool, "[]").await;
        let usage = runtime_event(
            "event:reports-usage",
            CanonicalEvent::ThreadUsageUpdated(usage_report(30, 12)),
        );
        let completed = runtime_event(
            "event:reports-completed",
            CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                state: ChatTurnState::Completed,
                stop_reason: None,
                usage: Some(usage_report(30, 12)),
                changed_files: Vec::new(),
            }),
        );
        for runtime in [&usage, &completed] {
            let mut transaction = pool.begin().await.unwrap();
            project_provider_event_in_transaction(&mut transaction, runtime)
                .await
                .unwrap();
            transaction.commit().await.unwrap();
        }
        assert_eq!(counted_tokens(&pool).await, Some(42));
        assert_eq!(states(&pool).await.1, "completed");
    }

    #[tokio::test]
    async fn usage_event_for_settled_run_is_ignored() {
        let pool = migrated_pool().await;
        seed(&pool, "failed", "failed").await;
        let mut transaction = pool.begin().await.unwrap();
        let organizational = project_provider_event_in_transaction(
            &mut transaction,
            &runtime_event(
                "event:reports-late-usage",
                CanonicalEvent::ThreadUsageUpdated(usage_report(30, 12)),
            ),
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert!(!organizational);
        assert_eq!(counted_tokens(&pool).await, None);
    }
}
