//! Notes derived indexes: the search projection, the backlink index, and link facts.

use super::helpers::migrated_memory_pool;

#[test]
fn schema_creates_notes_search_fts_projection() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, title)
             VALUES ('page-1', 'workspace', 'Searchable')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_search_index (
                id,
                source_type,
                page_id,
                title,
                body,
                metadata,
                source_last_edited_time
             )
             VALUES (
                'page:page-1',
                'page',
                'page-1',
                'Searchable',
                'Alpha project',
                'local metadata',
                '2026-07-02T12:00:00.000Z'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_search_fts (index_id, title, body, metadata)
             VALUES ('page:page-1', 'Searchable', 'Alpha project', 'local metadata')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_search_index_state (key, value)
             VALUES ('source_fingerprint', 'test')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let matched: String = sqlx::query_scalar(
            "SELECT index_id
             FROM notes_search_fts
             WHERE notes_search_fts MATCH 'alpha'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(matched, "page:page-1");

        let state: String =
            sqlx::query_scalar("SELECT value FROM notes_search_index_state WHERE key = ?")
                .bind("source_fingerprint")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "test");
    });
}

#[test]
fn schema_creates_notes_backlink_index() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, title)
             VALUES ('page-1', 'workspace', 'Source')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_blocks (
                id,
                page_id,
                parent_type,
                parent_page_id,
                type,
                attrs,
                plain_text,
                order_key
             )
             VALUES (
                'block-1',
                'page-1',
                'page_id',
                'page-1',
                'paragraph',
                '{\"paragraph\":{\"rich_text\":[]}}',
                'Mention target',
                'a0'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_backlink_index (
                id,
                target_type,
                target_id,
                source_type,
                source_page_id,
                source_block_id,
                reference_type,
                snippet,
                created_time,
                last_edited_time
             )
             VALUES (
                'block:block-1:page:target-page:page_mention',
                'page',
                'target-page',
                'block',
                'page-1',
                'block-1',
                'page_mention',
                'Mention target',
                '2026-07-02T12:00:00.000Z',
                '2026-07-02T12:00:00.000Z'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_backlink_index (
                id,
                target_type,
                target_id,
                target_object_type,
                source_type,
                source_page_id,
                source_block_id,
                reference_type,
                snippet,
                created_time,
                last_edited_time
             )
             VALUES (
                'block:block-1:local_object:task-1:local_object_mention',
                'local_object',
                'task-1',
                'project_task',
                'block',
                'page-1',
                'block-1',
                'local_object_mention',
                'Mention target',
                '2026-07-02T12:00:00.000Z',
                '2026-07-02T12:00:00.000Z'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_backlink_index_state (key, value)
             VALUES ('source_fingerprint', 'test')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let target_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_backlink_index
             WHERE target_type = 'page' AND target_id = 'target-page'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(target_count, 1);

        let invalid_local_object = sqlx::query(
            "INSERT INTO notes_backlink_index (
                id,
                target_type,
                target_id,
                source_type,
                source_page_id,
                source_block_id,
                reference_type,
                created_time,
                last_edited_time
             )
             VALUES (
                'invalid-local-object',
                'local_object',
                'task-2',
                'block',
                'page-1',
                'block-1',
                'local_object_mention',
                '2026-07-02T12:00:00.000Z',
                '2026-07-02T12:00:00.000Z'
             )",
        )
        .execute(&pool)
        .await;
        assert!(invalid_local_object.is_err());
    });
}

#[test]
fn schema_creates_notes_link_facts() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, title)
             VALUES ('page-1', 'workspace', 'Source')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_blocks (
                id,
                page_id,
                parent_type,
                parent_page_id,
                type,
                attrs,
                plain_text,
                order_key
             )
             VALUES (
                'block-1',
                'page-1',
                'page_id',
                'page-1',
                'paragraph',
                '{\"paragraph\":{\"rich_text\":[]}}',
                'Task mention',
                'a0'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_assets (
                id,
                asset_path,
                kind,
                source_type,
                original_name,
                content_type,
                byte_size,
                sha256
             )
             VALUES (
                'notes/files:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                'notes/files/asset-1.png',
                'image',
                'local_upload',
                'asset-1.png',
                'image/png',
                42,
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_link_facts (
                id,
                source_object_type,
                source_object_id,
                source_page_id,
                source_block_id,
                target_object_type,
                target_object_id,
                link_type,
                snippet,
                created_time,
                last_edited_time
             )
             VALUES (
                'fact-local-object',
                'block',
                'block-1',
                'page-1',
                'block-1',
                'project_task',
                'task-1',
                'local_object_mention',
                'Task mention',
                '2026-07-02T12:00:00.000Z',
                '2026-07-02T12:00:00.000Z'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_link_facts (
                id,
                source_object_type,
                source_object_id,
                source_page_id,
                source_block_id,
                target_object_type,
                target_object_id,
                target_asset_id,
                link_type,
                snippet,
                created_time,
                last_edited_time
             )
             VALUES (
                'fact-file',
                'block',
                'block-1',
                'page-1',
                'block-1',
                'file',
                'notes/files:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                'notes/files:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                'block_file',
                'diagram.png',
                '2026-07-02T12:00:00.000Z',
                '2026-07-02T12:00:00.000Z'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_link_facts_state (key, value)
             VALUES ('source_fingerprint', 'test')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let task_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_link_facts
             WHERE target_object_type = 'project_task'
               AND target_object_id = 'task-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(task_count, 1);

        let invalid_file = sqlx::query(
            "INSERT INTO notes_link_facts (
                id,
                source_object_type,
                source_object_id,
                source_page_id,
                source_block_id,
                target_object_type,
                target_object_id,
                link_type,
                snippet,
                created_time,
                last_edited_time
             )
             VALUES (
                'invalid-file',
                'block',
                'block-1',
                'page-1',
                'block-1',
                'file',
                'notes/files:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                'block_file',
                'diagram.png',
                '2026-07-02T12:00:00.000Z',
                '2026-07-02T12:00:00.000Z'
             )",
        )
        .execute(&pool)
        .await;
        assert!(invalid_file.is_err());
    });
}
