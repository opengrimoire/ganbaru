use super::helpers::migrated_memory_pool;

#[test]
fn schema_creates_normalized_quick_notes_storage() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for table in [
            "quick_note_tags",
            "quick_notes",
            "quick_note_text_runs",
            "quick_note_images",
            "quick_notes_search_fts",
        ] {
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name = ?")
                    .bind(table)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, 1, "missing {table}");
        }
        let invalid_color = sqlx::query(
            "INSERT INTO quick_notes (id, color, order_key) VALUES ('bad-color', 32, 'a0')",
        )
        .execute(&pool)
        .await;
        assert!(invalid_color.is_err());
        sqlx::query("INSERT INTO quick_notes (id, order_key) VALUES ('default-color', 'a0')")
            .execute(&pool)
            .await
            .unwrap();
        let default_color: i64 =
            sqlx::query_scalar("SELECT color FROM quick_notes WHERE id = 'default-color'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(default_color, 30);
        let null_color =
            sqlx::query("UPDATE quick_notes SET color = NULL WHERE id = 'default-color'")
                .execute(&pool)
                .await;
        assert!(null_color.is_err());
        let missing_order = sqlx::query("INSERT INTO quick_notes (id) VALUES ('unordered')")
            .execute(&pool)
            .await;
        assert!(missing_order.is_err());
        let invalid_run = sqlx::query(
            "INSERT INTO quick_note_text_runs (note_id, sort_order, content) VALUES ('missing', 0, 'text')",
        )
        .execute(&pool)
        .await;
        assert!(invalid_run.is_err());
        sqlx::query(
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag', 'Work', 'a0')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let duplicate_name = sqlx::query(
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-2', 'work', 'a1')",
        )
        .execute(&pool)
        .await;
        assert!(duplicate_name.is_err());
        let missing_tag_order =
            sqlx::query("INSERT INTO quick_note_tags (id, name) VALUES ('tag-3', 'Later')")
                .execute(&pool)
                .await;
        assert!(missing_tag_order.is_err());
    });
}

#[test]
fn image_rows_reference_content_addressed_files_and_cascade_with_their_note() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query("INSERT INTO quick_notes (id, order_key) VALUES ('note', 'a0')")
            .execute(&pool)
            .await
            .unwrap();
        let digest = "0123456789abcdef".repeat(4);
        let insert = |sha256: String, content_type: &'static str, byte_size: i64| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO quick_note_images
                         (note_id, sort_order, sha256, content_type, byte_size, width, height)
                     VALUES ('note', (SELECT COUNT(*) FROM quick_note_images), ?, ?, ?, 640, 480)",
                )
                .bind(sha256)
                .bind(content_type)
                .bind(byte_size)
                .execute(&pool)
                .await
            }
        };
        assert!(
            insert(digest.to_uppercase(), "image/png", 10)
                .await
                .is_err()
        );
        assert!(
            insert(digest[1..].to_string(), "image/png", 10)
                .await
                .is_err()
        );
        assert!(insert(digest.clone(), "image/bmp", 10).await.is_err());
        assert!(insert(digest.clone(), "image/png", 0).await.is_err());
        insert(digest.clone(), "image/gif", 10).await.unwrap();

        sqlx::query("DELETE FROM quick_notes WHERE id = 'note'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM quick_note_images")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    });
}

#[test]
fn tags_have_no_count_limit() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for index in 0..12 {
            sqlx::query("INSERT INTO quick_note_tags (id, name, order_key) VALUES (?, ?, 'a0')")
                .bind(format!("tag-{index}"))
                .bind(format!("Tag {index}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM quick_note_tags")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 12);
    });
}

#[test]
fn notes_and_tags_capture_their_columns_index_search_and_clear_deleted_tags() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-a', 'Work', 'a0');
             INSERT INTO quick_notes (id, title, body_plain_text, tag_id, order_key)
             VALUES ('note-1', 'Title one', 'alpha', 'tag-a', 'a0'),
                    ('note-2', 'Title two', 'charlie', NULL, 'a1');",
        )
        .execute(&pool)
        .await
        .unwrap();

        let matched: String = sqlx::query_scalar(
            "SELECT note_id FROM quick_notes_search_fts WHERE quick_notes_search_fts MATCH 'charlie'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(matched, "note-2");

        let captures: Vec<(i64, String, i64, i64)> = sqlx::query_as(
            "SELECT table_id, row_key, mask, created FROM sync_capture ORDER BY table_id, row_key",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(captures.len(), 3);
        assert!(captures.iter().all(|(table_id, _, mask, created)| {
            *created == 1 && *mask == if *table_id == 1 { 31 } else { 511 }
        }));

        sqlx::query("DELETE FROM quick_note_tags WHERE id = 'tag-a'")
            .execute(&pool)
            .await
            .unwrap();
        let tag: Option<String> =
            sqlx::query_scalar("SELECT tag_id FROM quick_notes WHERE id = 'note-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(tag, None);
        let dangling = sqlx::query("UPDATE quick_notes SET tag_id = 'missing' WHERE id = 'note-2'")
            .execute(&pool)
            .await;
        assert!(dangling.is_err());
    });
}
