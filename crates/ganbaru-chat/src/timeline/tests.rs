use super::*;
use crate::models::UtcTimestamp;
use crate::project_defaults::{default_channel_ids, ensure_project_chat_defaults};
use serde_json::json;
use sqlx::SqlitePool;

const NOW: &str = "2026-10-27T09:00:00.000Z";
const PROJECT_ID: &str = "project-timeline";

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

/// Seeds a project channel with a root message and its reply thread, and returns the
/// conversation id.
async fn seed_channel(pool: &SqlitePool) -> String {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-timeline', 'Timeline', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('{PROJECT_ID}', 'group-timeline', 'Timeline', 'a0');"
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
        "INSERT INTO chat_conversation_items (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:root', '{conversation}', 'message', 'c000', '{NOW}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('item:root', 'participant:local-owner', '{NOW}');
         INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES ('reply:timeline', '{conversation}', 'item:root', '{NOW}', '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
    conversation
}

async fn insert_item(
    pool: &SqlitePool,
    id: &str,
    conversation: &str,
    reply: Option<&str>,
    key: &str,
) {
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES (?, ?, ?, 'work_update', ?, ?)",
    )
    .bind(id)
    .bind(conversation)
    .bind(reply)
    .bind(key)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn order_key_after_starts_empty_scopes_and_grows_past_the_last_key() {
    let first = order_key_after(None).unwrap();
    let second = order_key_after(Some(&first)).unwrap();
    let third = order_key_after(Some(&second)).unwrap();
    assert!(first < second && second < third, "{first} {second} {third}");
    assert!(order_key_after(Some("not a key")).is_err());
}

#[tokio::test]
async fn next_order_key_follows_its_own_scope_and_breaks_equal_key_ties_by_id() {
    let pool = migrated_pool().await;
    let conversation = seed_channel(&pool).await;
    // Two devices appended concurrently after the root message and picked the same key.
    insert_item(&pool, "item:b", &conversation, None, "c001").await;
    insert_item(&pool, "item:a", &conversation, None, "c001").await;
    insert_item(
        &pool,
        "item:reply",
        &conversation,
        Some("reply:timeline"),
        "c000",
    )
    .await;

    let mut connection = pool.acquire().await.unwrap();
    let root_last = last_order_key(&mut connection, &conversation, None)
        .await
        .unwrap();
    assert_eq!(root_last.as_deref(), Some("c001"));
    let root_next = next_order_key(&mut connection, &conversation, None)
        .await
        .unwrap();
    assert!(root_next.as_str() > "c001", "{root_next}");
    let reply_next = next_order_key(&mut connection, &conversation, Some("reply:timeline"))
        .await
        .unwrap();
    assert_eq!(reply_next, order_key_after(Some("c000")).unwrap());
    let empty_next = next_order_key(&mut connection, &conversation, Some("reply:missing"))
        .await
        .unwrap();
    assert_eq!(empty_next, order_key_after(None).unwrap());
    drop(connection);

    let root_order: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM chat_conversation_items
             WHERE conversation_id = ? AND reply_thread_id IS NULL ORDER BY order_key, id",
    )
    .bind(&conversation)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(root_order, ["item:root", "item:a", "item:b"]);
}

#[tokio::test]
async fn revision_display_numbers_follow_creation_time_then_id() {
    let pool = migrated_pool().await;
    seed_channel(&pool).await;
    sqlx::raw_sql(
        "INSERT INTO chat_communication_message_revisions
                (id, message_item_id, normalized_markdown, created_at)
             VALUES ('revision:z-first', 'item:root', 'One', '2026-10-27T09:00:00.000Z'),
                    ('revision:b-tied', 'item:root', 'Two', '2026-10-27T09:00:01.000Z'),
                    ('revision:a-tied', 'item:root', 'Three', '2026-10-27T09:00:01.000Z');",
    )
    .execute(&pool)
    .await
    .unwrap();
    let numbers: Vec<(String, i64)> = sqlx::query_as(
        "SELECT id, revision FROM chat_communication_message_revisions_numbered
             ORDER BY revision",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        numbers,
        [
            ("revision:z-first".to_owned(), 1),
            ("revision:a-tied".to_owned(), 2),
            ("revision:b-tied".to_owned(), 3),
        ]
    );
}

#[test]
fn truncation_stops_on_a_character_boundary_inside_a_multibyte_character() {
    let text = "aé€😀";
    assert_eq!(truncate_at_char_boundary(text, 64), (text, false));
    assert_eq!(truncate_at_char_boundary(text, 2), ("a", true));
    assert_eq!(truncate_at_char_boundary(text, 5), ("aé", true));
    assert_eq!(truncate_at_char_boundary(text, 9), ("aé€", true));
    assert_eq!(truncate_at_char_boundary(text, 0), ("", true));
}

#[test]
fn projected_revisions_fit_the_schema_bounds_and_mark_what_was_cut() {
    let small =
        bounded_projected_revision("Done", json!({"type": "agent_update", "payload": {}})).unwrap();
    assert_eq!(small.normalized_markdown, "Done");
    assert!(!small.rich_content_data.contains(TRUNCATED_RICH_CONTENT_KEY));

    let long = "€".repeat(MAX_MESSAGE_MARKDOWN_BYTES / 3 + 1);
    let cut =
        bounded_projected_revision(&long, json!({"type": "agent_update", "payload": {}})).unwrap();
    assert!(cut.normalized_markdown.len() <= MAX_MESSAGE_MARKDOWN_BYTES);
    assert!(cut.normalized_markdown.len() > MAX_MESSAGE_MARKDOWN_BYTES - 3);
    assert!(long.starts_with(&cut.normalized_markdown));
    let marked: Value = serde_json::from_str(&cut.rich_content_data).unwrap();
    assert_eq!(marked[TRUNCATED_RICH_CONTENT_KEY], true);

    let payload = "x".repeat(MAX_RICH_CONTENT_BYTES);
    let dropped = bounded_projected_revision(
        "Done",
        json!({"type": "agent_update", "agentRunId": "run-1", "payload": {"output": payload}}),
    )
    .unwrap();
    assert!(dropped.rich_content_data.len() <= MAX_RICH_CONTENT_BYTES);
    let dropped: Value = serde_json::from_str(&dropped.rich_content_data).unwrap();
    assert_eq!(dropped["payload"], Value::Null);
    assert_eq!(dropped["agentRunId"], "run-1");
    assert_eq!(dropped[TRUNCATED_RICH_CONTENT_KEY], true);
}

#[tokio::test]
async fn the_schema_refuses_revisions_over_the_byte_bounds() {
    let pool = migrated_pool().await;
    seed_channel(&pool).await;
    let insert = |id: &'static str, markdown: String, rich: String| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO chat_communication_message_revisions
                        (id, message_item_id, normalized_markdown, rich_content_data, created_at)
                     VALUES (?, 'item:root', ?, ?, ?)",
            )
            .bind(id)
            .bind(markdown)
            .bind(rich)
            .bind(NOW)
            .execute(&pool)
            .await
        }
    };
    let at_bound = "é".repeat(MAX_MESSAGE_MARKDOWN_BYTES / 2);
    insert("revision:at-bound", at_bound, "{}".to_owned())
        .await
        .unwrap();
    let over_bound = format!("{}a", "é".repeat(MAX_MESSAGE_MARKDOWN_BYTES / 2));
    assert!(
        insert("revision:over", over_bound, "{}".to_owned())
            .await
            .is_err()
    );
    let rich = format!("{{\"text\":\"{}\"}}", "x".repeat(MAX_RICH_CONTENT_BYTES));
    assert!(
        insert("revision:rich", "Hi".to_owned(), rich)
            .await
            .is_err()
    );
}
