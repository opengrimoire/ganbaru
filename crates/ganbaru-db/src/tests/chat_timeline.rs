use super::helpers::{load_project_chat_graph, migrated_memory_pool};

const MARKDOWN_BYTE_LIMIT: usize = 131_072;
const RICH_CONTENT_BYTE_LIMIT: usize = 196_608;

const INSERT_REVISION: &str = "INSERT INTO chat_communication_message_revisions
    (id, message_item_id, normalized_markdown, rich_content_data, created_at)
    VALUES (?, 'item-root', ?, ?, '2026-10-23T00:00:00.000Z')";

/// Rich content whose serialized form is exactly `bytes` long.
fn rich_content_of(bytes: usize) -> String {
    let envelope = r#"{"text":""}"#;
    format!(r#"{{"text":"{}"}}"#, "x".repeat(bytes - envelope.len()))
}

#[test]
fn message_revisions_hold_text_up_to_the_byte_bounds() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let empty = "{}".to_string();

        for (id, markdown, rich) in [
            (
                "revision-largest",
                "x".repeat(MARKDOWN_BYTE_LIMIT),
                rich_content_of(RICH_CONTENT_BYTE_LIMIT),
            ),
            (
                "revision-markdown",
                "x".repeat(MARKDOWN_BYTE_LIMIT + 1),
                empty,
            ),
            (
                "revision-rich",
                String::new(),
                rich_content_of(RICH_CONTENT_BYTE_LIMIT + 1),
            ),
        ] {
            let result = sqlx::query(INSERT_REVISION)
                .bind(id)
                .bind(markdown)
                .bind(rich)
                .execute(&pool)
                .await;
            assert_eq!(result.is_ok(), id == "revision-largest", "{id}: {result:?}");
        }
    });
}

#[test]
fn attachment_references_queue_a_capture_on_their_revision() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        sqlx::raw_sql(
            "DELETE FROM sync_capture;
             INSERT INTO chat_communication_attachment_references
                (message_revision_id, ordinal, attachment_id, working_folder_id, kind,
                 original_display_name, mime_type, byte_size, sha256)
             VALUES ('message-revision-root', 0, 'attachment-managed', 'folder-managed', 'image',
                     'screen.png', 'image/png', 4,
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');",
        )
        .execute(&pool)
        .await
        .unwrap();

        let captures: Vec<(String, i64)> =
            sqlx::query_as("SELECT row_key, mask FROM sync_capture WHERE table_id = 373")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(captures.len(), 1, "one capture per revision: {captures:?}");
        assert_eq!(captures[0].0, "message-revision-root");
        assert_ne!(
            captures[0].1 & 2,
            0,
            "the capture names the attachments group"
        );
    });
}

const INSERT_TIMELINE_MESSAGE: &str = "INSERT INTO chat_messages
    (id, thread_id, turn_id, sequence_anchor, role, normalized_markdown, streaming_state,
     created_at, updated_at)
    VALUES (?, 'thread-managed', 'turn-1', 1, 'assistant', 'Hello', 'streaming',
            '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z')";

async fn timeline_clock(pool: &sqlx::SqlitePool, thread_id: &str) -> Option<i64> {
    sqlx::query_scalar("SELECT last_change FROM chat_timeline_change_clocks WHERE thread_id = ?")
        .bind(thread_id)
        .fetch_optional(pool)
        .await
        .unwrap()
}

async fn message_change(pool: &sqlx::SqlitePool, id: &str) -> i64 {
    sqlx::query_scalar("SELECT change_sequence FROM chat_messages WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn chat_timeline_rows_advance_the_thread_change_clock_once_per_write() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let before = timeline_clock(&pool, "thread-managed").await.unwrap();
        sqlx::query(INSERT_TIMELINE_MESSAGE)
            .bind("message-1")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(message_change(&pool, "message-1").await, before + 1);
        assert_eq!(
            timeline_clock(&pool, "thread-managed").await,
            Some(before + 1)
        );

        sqlx::query("UPDATE chat_messages SET normalized_markdown = 'Hello there' WHERE id = ?")
            .bind("message-1")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(message_change(&pool, "message-1").await, before + 2);
        assert_eq!(
            timeline_clock(&pool, "thread-managed").await,
            Some(before + 2)
        );

        sqlx::query("UPDATE chat_timeline_change_clocks SET last_change = last_change + 1")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE chat_messages SET normalized_markdown = 'Hi', change_sequence = ? WHERE id = ?",
        )
        .bind(before + 3)
        .bind("message-1")
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(message_change(&pool, "message-1").await, before + 3);
        assert_eq!(
            timeline_clock(&pool, "thread-managed").await,
            Some(before + 3)
        );

        sqlx::query(
            "UPDATE chat_turns SET invalidated_at = '2026-10-24T00:00:00.000Z' WHERE id = 'turn-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let turn_change: i64 =
            sqlx::query_scalar("SELECT change_sequence FROM chat_turns WHERE id = 'turn-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(turn_change, before + 4);
    });
}

#[test]
fn chat_timeline_deletes_record_removals_except_with_their_thread() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        sqlx::query(INSERT_TIMELINE_MESSAGE)
            .bind("message-1")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM chat_messages WHERE id = 'message-1'")
            .execute(&pool)
            .await
            .unwrap();
        let clock = timeline_clock(&pool, "thread-managed").await.unwrap();
        let removal: (i64, String) = sqlx::query_as(
            "SELECT change_sequence, item_id FROM chat_timeline_removals WHERE thread_id = 'thread-managed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(removal, (clock, "message-1".to_string()));

        sqlx::query(INSERT_TIMELINE_MESSAGE)
            .bind("message-2")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM chat_threads WHERE id = 'thread-managed'")
            .execute(&pool)
            .await
            .unwrap();
        let removals: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_timeline_removals")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(removals, 0);
        assert_eq!(timeline_clock(&pool, "thread-managed").await, None);
    });
}

#[test]
fn chat_timeline_triggers_stay_silent_while_sync_applies() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let before = timeline_clock(&pool, "thread-managed").await;
        sqlx::query("UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(INSERT_TIMELINE_MESSAGE)
            .bind("message-1")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(message_change(&pool, "message-1").await, 0);
        assert_eq!(timeline_clock(&pool, "thread-managed").await, before);
    });
}
