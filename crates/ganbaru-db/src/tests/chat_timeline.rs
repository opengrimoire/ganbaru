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
