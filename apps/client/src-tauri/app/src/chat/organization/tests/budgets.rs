//! The budget a post chooses for the work it starts, the teammate default, how continuations and
//! retries keep their limits, and setting and clearing a budget.

use super::super::budgets::{
    BUDGET_NEEDS_NEW_WORK, BUDGET_ON_ACTIVE_WORK, BUDGET_WITHOUT_TEAMMATE, clear_assignment_budget,
    set_assignment_budget,
};
use super::routing::{ROUTED_ASSIGNMENT_ID, post_request, reply_request, review_work_in};
use super::*;

const BUDGET_TIME: &str = "2026-08-04T17:05:00.000Z";

/// A stored budget: token limit, working seconds limit, who set it, and the used tokens.
type StoredBudget = (Option<i64>, Option<i64>, Option<String>, i64);

fn limits(
    token_limit: Option<u64>,
    working_seconds_limit: Option<u64>,
) -> ChatAssignmentBudgetInput {
    ChatAssignmentBudgetInput {
        token_limit,
        working_seconds_limit,
    }
}

/// The stored budget of `assignment_id`, if it has one.
async fn stored_budget(pool: &SqlitePool, assignment_id: &str) -> Option<StoredBudget> {
    sqlx::query_as(
        "SELECT token_limit, working_seconds_limit, set_by_participant_id, used_counted_tokens
         FROM chat_assignment_budgets WHERE assignment_id = ?",
    )
    .bind(assignment_id)
    .fetch_optional(pool)
    .await
    .unwrap()
}

/// Runs `sql` as an applied sync change would, past the capture and local write triggers.
async fn apply_as_synced(pool: &SqlitePool, sql: &str) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

/// Gives the review agent's policy a default token limit.
async fn set_review_default(pool: &SqlitePool, token_limit: i64) {
    save_policy_with_default_token_limit(
        pool,
        "participant:review-agent",
        "policy:review",
        Some(token_limit),
    )
    .await;
}

/// Gives `assignment_id` a budget of `token_limit` with `used` tokens already counted.
async fn seed_budget(pool: &SqlitePool, assignment_id: &str, token_limit: i64, used: i64) {
    let id = ganbaru_chat::ids::derived_id(
        ganbaru_sync_contracts::chat_usage::ASSIGNMENT_BUDGET_KEY_PREFIX,
        &[assignment_id],
    )
    .unwrap();
    apply_as_synced(
        pool,
        &format!(
            "INSERT INTO chat_assignment_budgets
                (id, assignment_id, token_limit, set_by_participant_id, set_at,
                 used_counted_tokens)
             VALUES ('{id}', '{assignment_id}', {token_limit}, 'participant:local-owner',
                     '{BUDGET_TIME}', {used})"
        ),
    )
    .await;
}

fn assert_budget_refusal(error: &ChatError, reason: &str) {
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert_eq!(error.field.as_deref(), Some("budget"));
    assert_eq!(error.details.as_deref(), Some(&json!({ "reason": reason })));
}

#[test]
fn budget_on_active_work_is_refused() {
    tauri::async_runtime::block_on(async {
        for state in ["queued", "working", "waiting_for_answer"] {
            let pool = review_work_in(state).await;
            let mut request = reply_request("command:budget", None);
            request.budget = Some(limits(Some(20_000), None));

            let Err(error) = post_request(&pool, &request).await else {
                panic!("a budget on {state} work posted");
            };

            assert_budget_refusal(&error, BUDGET_ON_ACTIVE_WORK);
            let budgets: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_assignment_budgets")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(budgets, 0, "{state} work gets no budget");
        }
    });
}

#[test]
fn a_new_assignment_stores_its_chosen_budget_with_who_set_it() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("completed").await;
        set_review_default(&pool, 50_000).await;
        let mut request = reply_request("command:budget", None);
        request.budget = Some(limits(Some(20_000), Some(600)));

        let posted = post_request(&pool, &request).await.unwrap();

        let assignment_id = posted.result.assignment.expect("the post starts work").id;
        assert_eq!(
            stored_budget(&pool, assignment_id.as_str()).await,
            Some((
                Some(20_000),
                Some(600),
                Some(LOCAL_PARTICIPANT_ID.to_owned()),
                0
            )),
            "the chosen budget wins over the default"
        );
    });
}

#[test]
fn default_budget_applies_to_new_assignment() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        set_review_default(&pool, 30_000).await;

        let posted = post_request(&pool, &reply_request("command:default", None))
            .await
            .unwrap();

        let assignment_id = posted.result.assignment.expect("the post starts work").id;
        assert_eq!(
            stored_budget(&pool, assignment_id.as_str()).await,
            Some((Some(30_000), None, None, 0)),
            "a default budget names no one who set it"
        );
    });
}

#[test]
fn a_new_assignment_without_a_budget_or_default_runs_without_one() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;

        let posted = post_request(&pool, &reply_request("command:unbudgeted", None))
            .await
            .unwrap();

        let assignment_id = posted.result.assignment.expect("the post starts work").id;
        assert_eq!(stored_budget(&pool, assignment_id.as_str()).await, None);
    });
}

#[test]
fn a_continuation_keeps_its_budget_limits_on_a_fresh_row() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("completed").await;
        set_review_default(&pool, 50_000).await;
        seed_budget(&pool, ROUTED_ASSIGNMENT_ID, 20_000, 12_000).await;

        let posted = post_request(&pool, &reply_request("command:continue", None))
            .await
            .unwrap();

        let assignment_id = posted.result.assignment.expect("the post starts work").id;
        assert_ne!(assignment_id.as_str(), ROUTED_ASSIGNMENT_ID);
        assert_eq!(
            stored_budget(&pool, assignment_id.as_str()).await,
            Some((Some(20_000), None, Some(LOCAL_PARTICIPANT_ID.to_owned()), 0)),
            "the continuation keeps the limits and starts counting again"
        );
    });
}

#[test]
fn retry_gets_fresh_budget_row() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&[NEW_OWNER]);
        let new_owner = PostingDevice::this_device(NEW_OWNER, &writers);
        let now = UtcTimestamp::new(BUDGET_TIME).unwrap();
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "failed").await;
        seed_budget(&pool, "assignment:old", 20_000, 20_000).await;
        let failed = read_assignment(&pool, &ChatWorkAssignmentId::new("assignment:old").unwrap())
            .await
            .unwrap();

        let retried =
            assignments::persist_retry(&pool, &failed.id, failed.revision, new_owner, &now)
                .await
                .unwrap();

        assert_ne!(retried, failed.id);
        assert_eq!(
            stored_budget(&pool, retried.as_str()).await,
            Some((Some(20_000), None, Some(LOCAL_PARTICIPANT_ID.to_owned()), 0))
        );
        assert_eq!(
            stored_budget(&pool, "assignment:old")
                .await
                .map(|budget| budget.3),
            Some(20_000),
            "the stranded work keeps what it used"
        );
    });
}

#[test]
fn a_same_device_retry_keeps_its_budget_row() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("failed").await;
        seed_budget(&pool, ROUTED_ASSIGNMENT_ID, 20_000, 8_000).await;
        let failed = read_assignment(
            &pool,
            &ChatWorkAssignmentId::new(ROUTED_ASSIGNMENT_ID).unwrap(),
        )
        .await
        .unwrap();
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);

        let retried = assignments::persist_retry(
            &pool,
            &failed.id,
            failed.revision,
            device,
            &UtcTimestamp::new(BUDGET_TIME).unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(retried, failed.id);
        assert_eq!(
            stored_budget(&pool, ROUTED_ASSIGNMENT_ID).await,
            Some((
                Some(20_000),
                None,
                Some(LOCAL_PARTICIPANT_ID.to_owned()),
                8_000
            )),
            "used amounts only grow, so the requeued work keeps counting on its row"
        );
    });
}

#[test]
fn a_budget_set_keeps_the_used_amount_and_a_repeat_set_changes_nothing() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("failed").await;
        seed_budget(&pool, ROUTED_ASSIGNMENT_ID, 20_000, 20_000).await;
        let assignment_id = ChatWorkAssignmentId::new(ROUTED_ASSIGNMENT_ID).unwrap();
        let first = UtcTimestamp::new("2026-08-04T17:06:00.000Z").unwrap();
        let raised = limits(Some(40_000), Some(1_200));

        let read = set_assignment_budget(&pool, &assignment_id, &raised, &first)
            .await
            .unwrap();

        assert_eq!(read.token_limit, Some(40_000));
        assert_eq!(read.working_seconds_limit, Some(1_200));
        assert_eq!(read.used_counted_tokens, 20_000);
        assert_eq!(
            read.set_by_participant_id
                .as_ref()
                .map(ChatParticipantId::as_str),
            Some(LOCAL_PARTICIPANT_ID)
        );
        assert_eq!(read.set_at, first);

        let repeated = set_assignment_budget(
            &pool,
            &assignment_id,
            &raised,
            &UtcTimestamp::new("2026-08-04T17:07:00.000Z").unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(repeated.set_at, first, "the same limits capture nothing");
    });
}

#[test]
fn a_cleared_budget_can_be_set_again() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("completed").await;
        let assignment_id = ChatWorkAssignmentId::new(ROUTED_ASSIGNMENT_ID).unwrap();
        let now = UtcTimestamp::new(BUDGET_TIME).unwrap();
        set_assignment_budget(&pool, &assignment_id, &limits(Some(20_000), None), &now)
            .await
            .unwrap();

        clear_assignment_budget(&pool, &assignment_id)
            .await
            .unwrap();
        assert_eq!(stored_budget(&pool, ROUTED_ASSIGNMENT_ID).await, None);

        set_assignment_budget(&pool, &assignment_id, &limits(None, Some(900)), &now)
            .await
            .unwrap();
        assert_eq!(
            stored_budget(&pool, ROUTED_ASSIGNMENT_ID).await,
            Some((None, Some(900), Some(LOCAL_PARTICIPANT_ID.to_owned()), 0))
        );
    });
}

#[test]
fn a_budget_for_missing_work_or_without_a_limit_is_refused() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("completed").await;
        let now = UtcTimestamp::new(BUDGET_TIME).unwrap();
        let missing = ChatWorkAssignmentId::new("assignment:missing").unwrap();

        let error = set_assignment_budget(&pool, &missing, &limits(Some(20_000), None), &now)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::NotFound);
        let error = clear_assignment_budget(&pool, &missing).await.unwrap_err();
        assert_eq!(error.code, ChatErrorCode::NotFound);

        let routed = ChatWorkAssignmentId::new(ROUTED_ASSIGNMENT_ID).unwrap();
        let error = set_assignment_budget(&pool, &routed, &limits(None, None), &now)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(stored_budget(&pool, ROUTED_ASSIGNMENT_ID).await, None);
    });
}

#[test]
fn a_scheduled_message_cannot_choose_a_budget() {
    let mut request = message("Later");
    request.budget = Some(limits(Some(20_000), None));
    let error = super::super::scheduling::refuse_scheduled_post_choices(&request).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert_eq!(error.field.as_deref(), Some("budget"));
}

#[test]
fn a_budget_without_a_teammate_is_refused() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("completed").await;
        let mut request = message("Plain channel note");
        request.client_command_id = ChatCommandId::new("command:no-teammate").unwrap();
        request.channel_id = ChatChannelId::new("channel:review").unwrap();
        request.budget = Some(limits(Some(20_000), None));

        let Err(error) = post_request(&pool, &request).await else {
            panic!("a budget reached no teammate and posted");
        };

        assert_budget_refusal(&error, BUDGET_WITHOUT_TEAMMATE);
    });
}

#[test]
fn a_budget_on_a_mention_that_records_no_work_is_refused() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        set_review_agent_home_device(&pool, None).await;
        let desk_writers = chat_writers_of(&["device:desk"]);
        let mut request = reply_request("command:mention-only", None);
        request.budget = Some(limits(Some(20_000), None));

        let Err(error) = persist_post(
            &pool,
            &request,
            None,
            PostingDevice::this_device("device:phone", &desk_writers),
        )
        .await
        else {
            panic!("a budget on a mention that starts no work posted");
        };

        assert_budget_refusal(&error, BUDGET_NEEDS_NEW_WORK);
    });
}
