use super::*;
use crate::coordination::reports::HostReportRefusal;
use crate::coordination::reports::tests::{
    AGENT, ASSIGNMENT, NOW, THREAD, call, migrated_pool, seed,
};
use crate::scheduling::{QuietPolicy, RecurrenceRule, WeekdaySet};
use sqlx::SqlitePool;

const FIRST_AT: &str = "2026-11-13T08:00:00.000Z";

fn proposal(message: &str) -> ScheduleProposal {
    ScheduleProposal {
        message: message.to_string(),
        first_at: FIRST_AT.to_string(),
        recurrence: Some(RecurrenceSpec {
            rule: RecurrenceRule::FixedTime {
                local_minute: 480,
                weekdays: WeekdaySet::from_mask(0b001_1111).unwrap(),
            },
            time_zone: "Europe/Madrid".to_string(),
            quiet_policy: QuietPolicy::Defer,
        }),
        reason: "You asked for a daily standup nudge".to_string(),
    }
}

async fn propose(
    pool: &SqlitePool,
    request: &str,
    proposal: &ScheduleProposal,
    validate: impl FnOnce(&ScheduleProposal) -> ChatResult<()>,
) -> ChatResult<HostCallOutcome> {
    let mut connection = pool.acquire().await.unwrap();
    record_host_schedule_proposal(&mut connection, call(request), proposal, validate, NOW).await
}

fn accept(_proposal: &ScheduleProposal) -> ChatResult<()> {
    Ok(())
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_proposal_posts_a_agent_card_in_the_reply_thread_and_schedules_nothing() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    let proposed = proposal("Post the standup summary");
    assert_eq!(
        propose(&pool, "request-1", &proposed, accept)
            .await
            .unwrap(),
        HostCallOutcome::Posted
    );
    let item_id = HostUpdateIds::derive(PROPOSAL_ID_PART, call("request-1"))
        .unwrap()
        .item_id;
    let mut connection = pool.acquire().await.unwrap();
    let stored = read_schedule_proposal(&mut connection, &item_id)
        .await
        .unwrap();
    drop(connection);
    assert_eq!(stored.proposal, proposed);
    assert_eq!(stored.reply_thread_id, THREAD);
    assert_eq!(stored.agent_id, AGENT);
    assert_eq!(stored.agent_name.as_deref(), Some("Helper"));
    assert_eq!(
        count(&pool, "SELECT count(*) FROM chat_scheduled_messages").await,
        0
    );
    let (author, markdown): (String, String) = sqlx::query_as(
        "SELECT message.author_participant_id, revision.normalized_markdown
         FROM chat_communication_messages message
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE message.item_id = ?",
    )
    .bind(&item_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(author, AGENT);
    assert_eq!(markdown, "Post the standup summary");
    let source: String = sqlx::query_scalar(
        "SELECT json_extract(payload_data, '$.payload.source')
         FROM chat_work_semantic_updates WHERE item_id = ?",
    )
    .bind(&item_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(source, HOST_TOOL_SOURCE);
}

#[tokio::test]
async fn an_identical_replay_skips_validation_and_a_changed_request_conflicts() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    let proposed = proposal("Post the standup summary");
    propose(&pool, "request-1", &proposed, accept)
        .await
        .unwrap();
    let expired = |_: &ScheduleProposal| Err(ScheduleProposalRefusal::TimeInvalid.into_error());
    assert_eq!(
        propose(&pool, "request-1", &proposed, expired)
            .await
            .unwrap(),
        HostCallOutcome::Replayed
    );
    let error = propose(&pool, "request-1", &proposal("Something else"), accept)
        .await
        .unwrap_err();
    assert_eq!(
        HostReportRefusal::from_error(&error),
        Some(HostReportRefusal::RequestConflict)
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM chat_work_semantic_updates WHERE update_kind = 'schedule_proposal'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn validation_refusals_write_nothing() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    let error = propose(&pool, "request-1", &proposal("Nudge"), |_| {
        Err(ScheduleProposalRefusal::RecurrenceInvalid.into_error())
    })
    .await
    .unwrap_err();
    assert_eq!(
        ScheduleProposalRefusal::from_error(&error),
        Some(ScheduleProposalRefusal::RecurrenceInvalid)
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM chat_work_semantic_updates").await,
        0
    );
}

#[tokio::test]
async fn work_waiting_for_an_answer_may_propose_but_ended_runs_may_not() {
    let pool = migrated_pool().await;
    seed(&pool, "waiting_for_answer", "working").await;
    assert_eq!(
        propose(&pool, "request-1", &proposal("Nudge"), accept)
            .await
            .unwrap(),
        HostCallOutcome::Posted
    );
    let ended = migrated_pool().await;
    seed(&ended, "working", "completed").await;
    let error = propose(&ended, "request-1", &proposal("Nudge"), accept)
        .await
        .unwrap_err();
    assert_eq!(
        HostReportRefusal::from_error(&error),
        Some(HostReportRefusal::AssignmentInactive)
    );
}

#[test]
fn proposal_text_is_trimmed_and_bounded_in_characters() {
    let mut padded = proposal("  Nudge  ");
    padded.reason = "  Because  ".to_string();
    let normalized = normalize_proposal_text(padded).unwrap();
    assert_eq!(normalized.message, "Nudge");
    assert_eq!(normalized.reason, "Because");
    assert_eq!(
        normalize_proposal_text(proposal("   ")),
        Err(ScheduleProposalRefusal::TooLarge)
    );
    let longest = "é".repeat(MAX_PROPOSED_MESSAGE_CHARS);
    assert!(normalize_proposal_text(proposal(&longest)).is_ok());
    let too_long = "é".repeat(MAX_PROPOSED_MESSAGE_CHARS + 1);
    assert_eq!(
        normalize_proposal_text(proposal(&too_long)),
        Err(ScheduleProposalRefusal::TooLarge)
    );
    let mut long_reason = proposal("Nudge");
    long_reason.reason = "r".repeat(MAX_PROPOSAL_REASON_CHARS + 1);
    assert_eq!(
        normalize_proposal_text(long_reason),
        Err(ScheduleProposalRefusal::TooLarge)
    );
}

#[tokio::test]
async fn reading_refuses_other_kinds_malformed_payloads_and_removed_agents() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    propose(&pool, "request-1", &proposal("Nudge"), accept)
        .await
        .unwrap();
    let item_id = HostUpdateIds::derive(PROPOSAL_ID_PART, call("request-1"))
        .unwrap()
        .item_id;
    let mut connection = pool.acquire().await.unwrap();
    let missing = read_schedule_proposal(&mut connection, "item:root")
        .await
        .unwrap_err();
    assert_eq!(missing.code, ChatErrorCode::NotFound);
    sqlx::query("UPDATE chat_participants SET archived_at = ? WHERE id = ?")
        .bind(NOW)
        .bind(AGENT)
        .execute(&mut *connection)
        .await
        .unwrap();
    let archived = read_schedule_proposal(&mut connection, &item_id)
        .await
        .unwrap();
    assert_eq!(archived.agent_name, None);
    // A replicated payload is not validated per kind, so a peer can deliver a malformed one.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         UPDATE chat_work_semantic_updates
         SET payload_data = json_set(payload_data, '$.payload.firstAt', 12)
         WHERE item_id = '{item_id}';
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;"
    )))
    .execute(&mut *connection)
    .await
    .unwrap();
    let malformed = read_schedule_proposal(&mut connection, &item_id)
        .await
        .unwrap_err();
    assert_eq!(malformed.code, ChatErrorCode::Validation);
    drop(connection);
    assert_eq!(
        count(
            &pool,
            &format!("SELECT count(*) FROM chat_work_semantic_updates WHERE assignment_id = '{ASSIGNMENT}'")
        )
        .await,
        1
    );
}

#[test]
fn refusal_codes_round_trip_through_errors() {
    for refusal in ScheduleProposalRefusal::ALL {
        assert_eq!(
            ScheduleProposalRefusal::from_error(&refusal.into_error()),
            Some(refusal)
        );
    }
    assert_eq!(
        ScheduleProposalRefusal::from_error(&HostReportRefusal::RequestConflict.into_error()),
        None
    );
}
