use super::*;
use crate::models::UtcTimestamp;
use crate::project_defaults::{default_channel_ids, ensure_project_chat_defaults};
use sqlx::SqlitePool;

const NOW: &str = "2026-11-12T09:00:00.000Z";
const LATER: &str = "2026-11-12T09:05:00.000Z";
const PROJECT_ID: &str = "project-delegation-outcome";
const PARENT_TEAMMATE: &str = "participant:delegation-lead";
const CHILD_TEAMMATE: &str = "participant:delegation-helper";
const PARENT: &str = "assignment:delegation-parent";
const CHILD: &str = "assignment:delegation-child";

async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

/// Seeds a channel where the lead's working parent assignment delegated a child to the helper
/// in the child's own reply thread, with the parent in `parent_state`.
async fn seed_delegation(pool: &SqlitePool, parent_state: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-delegation', 'D', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('{PROJECT_ID}', 'group-delegation', 'Delegation', 'a0');"
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
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('{PARENT_TEAMMATE}', 'ai_teammate', 'Lead', '{NOW}', '{NOW}'),
                ('{CHILD_TEAMMATE}', 'ai_teammate', 'Helper', '{NOW}', '{NOW}');
         INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
         VALUES ('{PARENT_TEAMMATE}', 'Lead', '{NOW}', '{NOW}'),
                ('{CHILD_TEAMMATE}', 'Helper', '{NOW}', '{NOW}');
         INSERT INTO chat_conversation_items (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:parent-root', '{conversation}', 'message', 'c000', '{NOW}'),
                ('item:child-root', '{conversation}', 'message', 'c001', '{NOW}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('item:parent-root', 'participant:local-owner', '{NOW}'),
                ('item:child-root', '{PARENT_TEAMMATE}', '{NOW}');
         INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:parent-root', 'item:parent-root', 'Ship the release', '{NOW}'),
                ('revision:child-root', 'item:child-root', '@Helper write notes', '{NOW}');
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:' || substr(item_id, 6);
         INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES ('reply:parent', '{conversation}', 'item:parent-root', '{NOW}', '{NOW}', '{NOW}'),
                ('reply:child', '{conversation}', 'item:child-root', '{NOW}', '{NOW}', '{NOW}');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, teammate_id, triggering_message_item_id, state,
             created_at, updated_at)
         VALUES ('{PARENT}', 'reply:parent', '{PARENT_TEAMMATE}', 'item:parent-root',
                 '{parent_state}', '{NOW}', '{NOW}');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, teammate_id, triggering_message_item_id, state,
             parent_assignment_id, delegation_depth, delegation_objective,
             delegation_expected_result, delegation_folder_ceiling, created_at, updated_at)
         VALUES ('{CHILD}', 'reply:child', '{CHILD_TEAMMATE}', 'item:child-root', 'working',
                 '{PARENT}', 1, 'Write notes', 'A summary', '[]', '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
}

/// Settles the child as `state` and posts its `update_kind` message with `body` in its thread.
async fn settle_child_with_update(pool: &SqlitePool, state: &str, update_kind: &str, body: &str) {
    let conversation = default_channel_ids(PROJECT_ID).unwrap().0;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES ('item:child-result', '{conversation}', 'reply:child', 'message', 'c002', '{NOW}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('item:child-result', '{CHILD_TEAMMATE}', '{NOW}');
         INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:child-result', 'item:child-result', '{body}', '{NOW}');
         UPDATE chat_communication_messages SET current_revision_id = 'revision:child-result'
         WHERE item_id = 'item:child-result';
         INSERT INTO chat_work_semantic_updates
            (item_id, assignment_id, update_kind, payload_data, created_at)
         VALUES ('item:child-result', '{CHILD}', '{update_kind}', '{{}}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
    set_child_state(pool, state, None).await;
}

async fn set_child_state(pool: &SqlitePool, state: &str, reason: Option<&str>) {
    sqlx::query("UPDATE chat_work_assignments SET state = ?, state_reason = ? WHERE id = ?")
        .bind(state)
        .bind(reason)
        .bind(CHILD)
        .execute(pool)
        .await
        .unwrap();
}

async fn post(pool: &SqlitePool) -> Option<DelegationOutcome> {
    let mut connection = pool.acquire().await.unwrap();
    post_delegation_outcome(&mut connection, CHILD, LATER)
        .await
        .unwrap()
}

/// Every outcome message in the parent's thread, as `(author, markdown, update kind, payload)`.
async fn outcome_messages(pool: &SqlitePool) -> Vec<(String, String, String, String)> {
    sqlx::query_as(
        "SELECT message.author_participant_id, revision.normalized_markdown,
                semantic.update_kind, semantic.assignment_id
         FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         JOIN chat_work_semantic_updates semantic ON semantic.item_id = item.id
         WHERE item.reply_thread_id = 'reply:parent'",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn parent_inputs(pool: &SqlitePool) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT message_item_id, routing_kind, delivery_state
         FROM chat_work_assignment_inputs WHERE assignment_id = ? ORDER BY ordinal",
    )
    .bind(PARENT)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn parent_thread_revision(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT revision FROM chat_reply_threads WHERE id = 'reply:parent'")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_completed_child_posts_one_settled_update_and_a_continuation_of_its_working_parent() {
    let pool = migrated_pool().await;
    seed_delegation(&pool, "working").await;
    settle_child_with_update(&pool, "completed", "result", "Notes are drafted").await;

    let outcome = post(&pool).await.expect("outcome");
    let item_id = outcome_item_id(CHILD).unwrap();
    assert_eq!(outcome.parent_assignment_id.as_str(), PARENT);
    assert_eq!(outcome.message_item_id, item_id);
    assert!(outcome.input_queued);
    assert_eq!(
        outcome_messages(&pool).await,
        vec![(
            CHILD_TEAMMATE.to_owned(),
            "Notes are drafted".to_owned(),
            "delegation".to_owned(),
            PARENT.to_owned(),
        )]
    );
    let payload: String = sqlx::query_scalar(
        "SELECT json_extract(payload_data, '$.payload') FROM chat_work_semantic_updates
         WHERE item_id = ?",
    )
    .bind(&item_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(
        payload,
        json!({
            "phase": "settled",
            "childAssignmentId": CHILD,
            "parentAssignmentId": PARENT,
            "state": "completed",
        })
    );
    assert_eq!(
        parent_inputs(&pool).await,
        vec![(
            item_id,
            "queued_continuation".to_owned(),
            "pending".to_owned()
        )]
    );

    let revision = parent_thread_revision(&pool).await;
    assert_eq!(post(&pool).await, None);
    assert_eq!(outcome_messages(&pool).await.len(), 1);
    assert_eq!(parent_inputs(&pool).await.len(), 1);
    assert_eq!(parent_thread_revision(&pool).await, revision);
}

#[tokio::test]
async fn a_settled_parent_gets_the_update_without_an_input() {
    let pool = migrated_pool().await;
    seed_delegation(&pool, "ready_for_review").await;
    settle_child_with_update(&pool, "ready_for_review", "review", "Two files changed").await;

    let outcome = post(&pool).await.expect("outcome");
    assert!(!outcome.input_queued);
    assert_eq!(outcome_messages(&pool).await.len(), 1);
    assert!(parent_inputs(&pool).await.is_empty());
}

#[tokio::test]
async fn an_outcome_without_a_result_carries_the_state_reason_or_a_fallback() {
    let pool = migrated_pool().await;
    seed_delegation(&pool, "working").await;
    set_child_state(&pool, "cancelled", Some("parent_authority_revoked")).await;
    post(&pool).await.expect("outcome");
    assert_eq!(
        outcome_messages(&pool).await[0].1,
        "parent_authority_revoked"
    );

    let pool = migrated_pool().await;
    seed_delegation(&pool, "working").await;
    set_child_state(&pool, "failed", None).await;
    post(&pool).await.expect("outcome");
    assert_eq!(outcome_messages(&pool).await[0].1, "Delegated work failed.");
}

#[tokio::test]
async fn the_outcome_body_is_bounded_to_the_report_limit() {
    let pool = migrated_pool().await;
    seed_delegation(&pool, "working").await;
    let long = "é".repeat(MAX_REPORT_BODY_BYTES);
    settle_child_with_update(&pool, "completed", "result", &long).await;
    post(&pool).await.expect("outcome");
    let body = &outcome_messages(&pool).await[0].1;
    assert!(body.len() <= MAX_REPORT_BODY_BYTES);
    assert!(body.len() > MAX_REPORT_BODY_BYTES - 2);
}

#[tokio::test]
async fn unsettled_undelegated_and_moved_children_post_nothing() {
    let pool = migrated_pool().await;
    seed_delegation(&pool, "working").await;
    assert_eq!(post(&pool).await, None);
    for state in ["queued", "waiting_for_answer", "waiting_for_approval"] {
        set_child_state(&pool, state, None).await;
        assert_eq!(post(&pool).await, None);
    }
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(
        post_delegation_outcome(&mut connection, PARENT, LATER)
            .await
            .unwrap(),
        None
    );
    drop(connection);

    set_child_state(&pool, "cancelled", Some("rerouted")).await;
    sqlx::query(
        "INSERT INTO chat_work_assignments
            (id, reply_thread_id, teammate_id, triggering_message_item_id, state,
             previous_assignment_id, parent_assignment_id, delegation_depth,
             delegation_objective, delegation_expected_result, delegation_folder_ceiling,
             created_at, updated_at)
         VALUES ('assignment:delegation-successor', 'reply:child', ?, 'item:child-root',
                 'queued', ?, ?, 1, 'Write notes', 'A summary', '[]', ?, ?)",
    )
    .bind(CHILD_TEAMMATE)
    .bind(CHILD)
    .bind(PARENT)
    .bind(NOW)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(post(&pool).await, None);
    assert!(outcome_messages(&pool).await.is_empty());
}
