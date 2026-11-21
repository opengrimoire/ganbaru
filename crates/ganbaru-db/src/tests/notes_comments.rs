//! Notes discussions: comment read state, reactions, notification preferences, mention
//! notifications, and suggestions.

use super::helpers::migrated_memory_pool;
use sqlx::AssertSqlSafe;

#[test]
fn schema_creates_notes_comment_thread_read_state() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for table in ["notes_comment_thread_reads", "notes_local_users"] {
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?")
                    .bind(table)
                    .fetch_optional(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, Some(1), "{table} should exist");
        }

        let user_id_fk: Option<i64> = sqlx::query_scalar(
            "SELECT 1
             FROM pragma_foreign_key_list('notes_comment_thread_reads')
             WHERE \"table\" = 'notes_local_users'
               AND \"from\" = 'user_id'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(user_id_fk, Some(1));
    });
}

#[test]
fn schema_creates_notes_comment_reactions_and_notification_preferences() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let user_id: String =
            sqlx::query_scalar("SELECT id FROM notes_local_users ORDER BY id LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        for statement in [
            "INSERT INTO notes_pages (id, parent_type, title)
             VALUES ('page-a', 'workspace', 'Inbox')",
            "INSERT INTO notes_comment_threads (id, page_id, parent_type, parent_page_id)
             VALUES ('thread-a', 'page-a', 'page_id', 'page-a')",
            "INSERT INTO notes_comments (id, thread_id, rich_text, created_by)
             VALUES ('comment-a', 'thread-a', '[]', 'person-a')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        let insert_reaction = |emoji: &'static str, created_by: &'static str| {
            sqlx::query(
                "INSERT INTO notes_comment_reactions (comment_id, created_by, display_name, emoji, sync_key)
                 VALUES ('comment-a', ?1, '{\"type\":\"user\",\"resolved_name\":\"You\"}', ?2, 'reaction:' || ?1 || ':' || ?2)",
            )
            .bind(created_by)
            .bind(emoji)
        };
        insert_reaction("👍", "person-a")
            .execute(&pool)
            .await
            .unwrap();
        insert_reaction("🎉", "person-a")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            insert_reaction("👍", "person-a")
                .execute(&pool)
                .await
                .is_err(),
            "one person reacts once per emoji"
        );
        assert!(
            insert_reaction(" ", "person-a")
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            insert_reaction("12345678901234567", "person-a")
                .execute(&pool)
                .await
                .is_err()
        );

        sqlx::query(
            "INSERT INTO notes_comment_thread_mutes (thread_id, user_id) VALUES ('thread-a', ?)",
        )
        .bind(&user_id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "INSERT INTO notes_comment_thread_mutes (thread_id, user_id)
                 VALUES ('thread-a', 'missing-user')",
            )
            .execute(&pool)
            .await
            .is_err()
        );

        let insert_level = |level: &'static str| {
            sqlx::query(
                "INSERT OR REPLACE INTO notes_page_notification_settings (page_id, user_id, level)
                 VALUES ('page-a', ?, ?)",
            )
            .bind(user_id.clone())
            .bind(level)
        };
        for level in ["all", "mentions", "none"] {
            insert_level(level).execute(&pool).await.unwrap();
        }
        assert!(insert_level("invite").execute(&pool).await.is_err());

        sqlx::query("DELETE FROM notes_comments WHERE id = 'comment-a'")
            .execute(&pool)
            .await
            .unwrap();
        let reactions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_comment_reactions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(reactions, 0, "reactions follow their comment");

        sqlx::query("DELETE FROM notes_pages WHERE id = 'page-a'")
            .execute(&pool)
            .await
            .unwrap();
        for table in [
            "notes_comment_thread_mutes",
            "notes_page_notification_settings",
        ] {
            let remaining: i64 =
                sqlx::query_scalar(AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(remaining, 0, "{table} follows its page");
        }
    });
}

#[test]
fn schema_creates_notes_mention_notifications() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?")
                .bind("notes_mention_notifications")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert_eq!(exists, Some(1));

        let index_count: i64 = sqlx::query_scalar(
            "SELECT count(*)
             FROM sqlite_schema
             WHERE type = 'index'
               AND tbl_name = 'notes_mention_notifications'
               AND name LIKE 'idx_notes_mention_notifications_%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(index_count, 3);

        let page_fk: Option<i64> = sqlx::query_scalar(
            "SELECT 1
             FROM pragma_foreign_key_list('notes_mention_notifications')
             WHERE \"table\" = 'notes_pages'
               AND \"from\" = 'page_id'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(page_fk, Some(1));

        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, title)
             VALUES ('page-a', 'workspace', 'Inbox')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "INSERT INTO notes_mention_notifications (
                id,
                source_type,
                source_id,
                page_id,
                kind,
                target_type,
                status,
                fingerprint
             )
             VALUES (
                'notification-a',
                'block',
                'block-a',
                'page-a',
                'reminder',
                'date',
                'stale',
                'fingerprint-a'
             )",
            )
            .execute(&pool)
            .await
            .is_err()
        );
        sqlx::query(
            "INSERT INTO notes_blocks (
                id, page_id, parent_type, parent_page_id, type, attrs, plain_text, order_key
             )
             VALUES (
                'block-a', 'page-a', 'page_id', 'page-a', 'paragraph',
                json_object('rich_text', json_array()), '', 'a0'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (offset, accepted) in [(0, true), (40_320, true), (-1, false), (40_321, false)] {
            let inserted = sqlx::query(
                "INSERT INTO notes_mention_notifications (
                    id, source_type, source_id, page_id, kind, target_type, fingerprint, block_id, trigger_at,
                    remind_minutes_before
                 )
                 VALUES (?, 'block', 'block-a', 'page-a', 'reminder', 'date', ?, 'block-a', '2026-07-05', ?)",
            )
            .bind(format!("offset-{offset}"))
            .bind(format!("fingerprint-{offset}"))
            .bind(offset)
            .execute(&pool)
            .await;
            assert_eq!(inserted.is_ok(), accepted, "offset {offset}");
        }

        // Property reminders come from their page and name their property; they carry no block or comment.
        for (id, source_id, block_id, kind, target_id, accepted) in [
            ("property-a", "page-a", None, "reminder", Some("due"), true),
            (
                "property-other-page",
                "page-b",
                None,
                "reminder",
                Some("due"),
                false,
            ),
            (
                "property-block",
                "page-a",
                Some("block-a"),
                "reminder",
                Some("due"),
                false,
            ),
            (
                "property-mention",
                "page-a",
                None,
                "user_mention",
                Some("user"),
                false,
            ),
            ("property-unnamed", "page-a", None, "reminder", None, false),
        ] {
            let inserted = sqlx::query(
                "INSERT INTO notes_mention_notifications (
                    id, source_type, source_id, page_id, block_id, kind, target_type, target_id, fingerprint,
                    trigger_at
                 )
                 VALUES (?, 'page_property', ?, 'page-a', ?, ?, 'date', ?, ?, '2026-07-05')",
            )
            .bind(id)
            .bind(source_id)
            .bind(block_id)
            .bind(kind)
            .bind(target_id)
            .bind(format!("fingerprint-{id}"))
            .execute(&pool)
            .await;
            assert_eq!(inserted.is_ok(), accepted, "{id}");
        }
    });
}

#[test]
fn schema_creates_notes_suggestions() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?")
                .bind("notes_suggestions")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert_eq!(exists, Some(1));

        for (column, action) in [
            ("created_by", "CASCADE"),
            ("accepted_by", "SET NULL"),
            ("rejected_by", "SET NULL"),
        ] {
            let user_fk: Option<String> = sqlx::query_scalar(
                "SELECT on_delete
                 FROM pragma_foreign_key_list('notes_suggestions')
                 WHERE \"table\" = 'notes_local_users'
                   AND \"from\" = ?",
            )
            .bind(column)
            .fetch_optional(&pool)
            .await
            .unwrap();
            assert_eq!(
                user_fk.as_deref(),
                Some(action),
                "{column} should reference notes_local_users"
            );
        }

        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, title)
             VALUES ('page-a', 'workspace', 'Inbox')",
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
                'block-a',
                'page-a',
                'page_id',
                'page-a',
                'paragraph',
                json_object('rich_text', json_array()),
                'Original',
                'a0'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let user_id: String = sqlx::query_scalar(
            "SELECT id FROM notes_local_users ORDER BY created_time ASC, id ASC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "INSERT INTO notes_suggestions (
                id,
                page_id,
                block_id,
                created_by,
                display_name,
                range_start,
                range_end,
                original_text,
                proposed_text,
                accepted_at,
                accepted_by
             )
             VALUES (
                'suggestion-a',
                'page-a',
                'block-a',
                ?,
                json_object('type', 'user', 'resolved_name', 'You'),
                0,
                8,
                'Original',
                'Changed',
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                ?
             )",
            )
            .bind(&user_id)
            .bind(&user_id)
            .execute(&pool)
            .await
            .is_err()
        );
    });
}
