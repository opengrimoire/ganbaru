use super::helpers::migrated_memory_pool;
use sqlx::AssertSqlSafe;

#[test]
fn schema_creates_normalized_notes_database_tables() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for table in [
            "notes_databases",
            "notes_data_sources",
            "notes_database_views",
            "notes_data_source_relation_links",
            "notes_data_source_rollup_cache",
            "notes_data_source_templates",
            "notes_data_source_template_blocks",
        ] {
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?")
                    .bind(table)
                    .fetch_optional(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, Some(1), "{table} should exist");
        }

        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, title, properties)
             VALUES ('page-a', 'workspace', 'Inbox', '{}')",
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
                payload,
                plain_text,
                sort_order
             )
             VALUES (
                'database-a',
                'page-a',
                'page_id',
                'page-a',
                'child_database',
                '{\"title\":\"Tasks\"}',
                'Tasks',
                1000
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_databases (
                id,
                parent_type,
                parent_page_id,
                title,
                title_rich_text,
                description
             )
             VALUES ('database-a', 'page_id', 'page-a', 'Tasks', '[]', '[]')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_data_sources (
                id,
                database_id,
                title,
                title_rich_text,
                description,
                properties
             )
             VALUES ('source-a', 'database-a', 'Tasks', '[]', '[]', '{}')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_database_views (
                id,
                database_id,
                data_source_id,
                name,
                type,
                sorts
             )
             VALUES ('view-a', 'database-a', 'source-a', 'Table', 'table', '[]')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(
            sqlx::query(
                "INSERT INTO notes_database_views (
                id,
                database_id,
                data_source_id,
                name,
                type,
                sorts
             )
             VALUES ('view-b', 'database-a', 'source-a', 'Bad', 'kanbanish', '[]')",
            )
            .execute(&pool)
            .await
            .is_err()
        );
    });
}

#[test]
fn schema_enforces_notes_folder_placement_invariants() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let folder_table: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'notes_folders'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(folder_table, Some(1));
        let folder_column: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM pragma_table_info('notes_pages') WHERE name = 'folder_id'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(folder_column, Some(1));

        sqlx::query("INSERT INTO project_groups (id, name, order_key) VALUES ('folder-group', 'Folders', 'a0')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, order_key)
             VALUES ('folder-project-a', 'folder-group', 'A', 'a0'),
                    ('folder-project-b', 'folder-group', 'B', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_folders (id, project_id, name)
             VALUES ('folder-a', 'folder-project-a', 'A'),
                    ('folder-b', 'folder-project-b', 'B')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "INSERT INTO notes_folders (id, project_id, parent_folder_id, name)
             VALUES ('folder-cross', 'folder-project-a', 'folder-b', 'Cross project')",
            )
            .execute(&pool)
            .await
            .is_err()
        );

        sqlx::query(
            "INSERT INTO notes_folders (id, project_id, parent_folder_id, name)
             VALUES ('folder-child', 'folder-project-a', 'folder-a', 'Child')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "UPDATE notes_folders SET parent_folder_id = 'folder-child' WHERE id = 'folder-a'",
            )
            .execute(&pool)
            .await
            .is_err()
        );

        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, folder_id, title, properties)
             VALUES (
                 'folder-page',
                 'workspace',
                 'folder-a',
                 'Folder page',
                 json_object('__ganbaru_project_id', 'folder-project-a')
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "INSERT INTO notes_pages (id, parent_type, folder_id, title, properties)
             VALUES (
                 'wrong-project-page',
                 'workspace',
                 'folder-a',
                 'Wrong project',
                 json_object('__ganbaru_project_id', 'folder-project-b')
             )",
            )
            .execute(&pool)
            .await
            .is_err()
        );
        assert!(
            sqlx::query(
                "INSERT INTO notes_pages (
                id, parent_type, parent_page_id, folder_id, title, properties
             ) VALUES (
                 'nested-folder-page',
                 'page_id',
                 'folder-page',
                 'folder-a',
                 'Invalid nested folder page',
                 json_object('__ganbaru_project_id', 'folder-project-a')
             )",
            )
            .execute(&pool)
            .await
            .is_err()
        );
        assert!(
            sqlx::query(
                "UPDATE notes_pages
             SET properties = json_object('__ganbaru_project_id', 'folder-project-b')
             WHERE id = 'folder-page'",
            )
            .execute(&pool)
            .await
            .is_err()
        );
    });
}

#[test]
fn schema_validates_project_notes_default_open_mode() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, order_key)
             VALUES ('group-notes-mode', 'Notes mode', 'lucide:folder', 'a0')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, order_key)
             VALUES ('project-notes-mode', 'group-notes-mode', 'Notes mode', 'lucide:folder', 'a0')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let default_mode: Option<String> = sqlx::query_scalar(
            "SELECT notes_default_open_mode FROM projects WHERE id = 'project-notes-mode'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(default_mode, None);

        sqlx::query(
            "UPDATE projects SET notes_default_open_mode = 'side' WHERE id = 'project-notes-mode'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(sqlx::query(
            "UPDATE projects SET notes_default_open_mode = 'invalid' WHERE id = 'project-notes-mode'",
        )
        .execute(&pool)
        .await
        .is_err());
    });
}

#[test]
fn schema_creates_notes_local_user_identity() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let id: String = sqlx::query_scalar(
            "SELECT id FROM notes_local_users ORDER BY created_time ASC, id ASC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_ne!(id, "local-user");
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_info('notes_local_users') ORDER BY cid",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(columns, ["id", "created_time"]);

        let created_by_column: Option<i64> = sqlx::query_scalar(
            "SELECT 1
             FROM pragma_table_info('notes_page_history_snapshots')
             WHERE name = 'created_by'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(created_by_column, Some(1));
    });
}

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
            "INSERT INTO notes_pages (id, parent_type, title, properties)
             VALUES ('page-a', 'workspace', 'Inbox', '{}')",
            "INSERT INTO notes_comment_threads (id, page_id, parent_type, parent_page_id)
             VALUES ('thread-a', 'page-a', 'page_id', 'page-a')",
            "INSERT INTO notes_comments (id, thread_id, rich_text, created_by)
             VALUES ('comment-a', 'thread-a', '[]', 'person-a')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        let insert_reaction = |emoji: &'static str, created_by: &'static str| {
            sqlx::query(
                "INSERT INTO notes_comment_reactions (comment_id, created_by, display_name, emoji)
                 VALUES ('comment-a', ?, '{\"type\":\"user\",\"resolved_name\":\"You\"}', ?)",
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
            "INSERT INTO notes_pages (id, parent_type, title, properties)
             VALUES ('page-a', 'workspace', 'Inbox', '{}')",
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
                id, page_id, parent_type, parent_page_id, type, payload, plain_text, sort_order
             )
             VALUES (
                'block-a', 'page-a', 'page_id', 'page-a', 'paragraph',
                json_object('rich_text', json_array()), '', 1
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
            "INSERT INTO notes_pages (id, parent_type, title, properties)
             VALUES ('page-a', 'workspace', 'Inbox', '{}')",
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
                payload,
                plain_text,
                sort_order
             )
             VALUES (
                'block-a',
                'page-a',
                'page_id',
                'page-a',
                'paragraph',
                json_object('rich_text', json_array()),
                'Original',
                1
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

#[test]
fn schema_creates_notes_page_icon_assets() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        sqlx::query(
            "INSERT INTO notes_page_icon_assets
                (id, asset_path, original_name, content_type, byte_size, sha256)
             VALUES (
                'asset-1',
                'notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png',
                'focus.png',
                'image/png',
                42,
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let asset_path: String = sqlx::query_scalar(
            "SELECT asset_path FROM notes_page_icon_assets WHERE id = 'asset-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            asset_path,
            "notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
        );

        for (id, asset_path, content_type, byte_size, sha256) in [
            (
                "bad-prefix",
                "project-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                "image/png",
                1,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            (
                "bad-nested",
                "notes/page-icons/nested/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                "image/png",
                1,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            (
                "bad-type",
                "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.gif",
                "image/gif",
                1,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            (
                "bad-size",
                "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                "image/png",
                0,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            (
                "bad-hash",
                "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                "image/png",
                1,
                "short",
            ),
        ] {
            let inserted = sqlx::query(
                "INSERT INTO notes_page_icon_assets
                    (id, asset_path, content_type, byte_size, sha256)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(asset_path)
            .bind(content_type)
            .bind(byte_size)
            .bind(sha256)
            .execute(&pool)
            .await;
            assert!(inserted.is_err());
        }
    });
}

#[test]
fn schema_creates_notes_page_cover_assets() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        sqlx::query(
            "INSERT INTO notes_page_cover_assets
                (id, asset_path, original_name, content_type, byte_size, sha256)
             VALUES (
                'asset-1',
                'notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png',
                'cover.png',
                'image/png',
                42,
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let asset_path: String = sqlx::query_scalar(
            "SELECT asset_path FROM notes_page_cover_assets WHERE id = 'asset-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            asset_path,
            "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
        );

        for (id, asset_path, content_type, byte_size, sha256) in [
            (
                "bad-prefix",
                "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                "image/png",
                1,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            (
                "bad-nested",
                "notes/page-covers/nested/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                "image/png",
                1,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            (
                "bad-type",
                "notes/page-covers/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.gif",
                "image/gif",
                1,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            (
                "bad-size",
                "notes/page-covers/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                "image/png",
                0,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            (
                "bad-hash",
                "notes/page-covers/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                "image/png",
                1,
                "short",
            ),
        ] {
            let inserted = sqlx::query(
                "INSERT INTO notes_page_cover_assets
                    (id, asset_path, content_type, byte_size, sha256)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(asset_path)
            .bind(content_type)
            .bind(byte_size)
            .bind(sha256)
            .execute(&pool)
            .await;
            assert!(inserted.is_err());
        }
    });
}

#[test]
fn schema_creates_notes_assets_and_references() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, title)
             VALUES ('page-1', 'workspace', 'Assets')",
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
                payload,
                plain_text,
                sort_order
             )
             VALUES (
                'block-1',
                'page-1',
                'page_id',
                'page-1',
                'paragraph',
                '{\"paragraph\":{\"rich_text\":[]}}',
                '',
                1000
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
                'notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png',
                'notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png',
                'image',
                'local_upload',
                'focus.png',
                'image/png',
                42,
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
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
                sha256,
                storage_state,
                missing_at
             )
             VALUES (
                'notes/files/report.pdf',
                'notes/files/report.pdf',
                'pdf',
                'imported',
                'report.pdf',
                'application/pdf',
                128,
                'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                'missing',
                '2026-07-02T12:00:00.000Z'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO notes_asset_references (asset_id, owner_type, owner_id, page_id, role)
             VALUES (
                'notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png',
                'page',
                'page-1',
                'page-1',
                'page_icon'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_asset_references (
                asset_id,
                owner_type,
                owner_id,
                page_id,
                block_id,
                role
             )
             VALUES (
                'notes/files/report.pdf',
                'block',
                'block-1',
                'page-1',
                'block-1',
                'block_file'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let reference_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_asset_references")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(reference_count, 2);

        for (
            id,
            asset_path,
            kind,
            source_type,
            content_type,
            byte_size,
            sha256,
            storage_state,
            missing_at,
        ) in [
            (
                "bad-path",
                "notes/files/nested/file.pdf",
                "pdf",
                "imported",
                "application/pdf",
                1,
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "available",
                None::<&str>,
            ),
            (
                "bad-icon-type",
                "notes/page-icons/icon.svg",
                "image",
                "local_upload",
                "image/svg+xml",
                1,
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "available",
                None::<&str>,
            ),
            (
                "bad-content-type",
                "notes/files/file.bin",
                "file",
                "local_upload",
                "not-a-mime",
                1,
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "available",
                None::<&str>,
            ),
            (
                "bad-size",
                "notes/files/file.bin",
                "file",
                "local_upload",
                "application/octet-stream",
                0,
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "available",
                None::<&str>,
            ),
            (
                "bad-hash",
                "notes/files/file.bin",
                "file",
                "local_upload",
                "application/octet-stream",
                1,
                "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
                "available",
                None::<&str>,
            ),
            (
                "bad-missing-state",
                "notes/files/file.bin",
                "file",
                "local_upload",
                "application/octet-stream",
                1,
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "missing",
                None::<&str>,
            ),
        ] {
            let inserted = sqlx::query(
                "INSERT INTO notes_assets (
                    id,
                    asset_path,
                    kind,
                    source_type,
                    content_type,
                    byte_size,
                    sha256,
                    storage_state,
                    missing_at
                 )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(asset_path)
            .bind(kind)
            .bind(source_type)
            .bind(content_type)
            .bind(byte_size)
            .bind(sha256)
            .bind(storage_state)
            .bind(missing_at)
            .execute(&pool)
            .await;
            assert!(inserted.is_err(), "{id} should fail");
        }

        let invalid_reference = sqlx::query(
            "INSERT INTO notes_asset_references (asset_id, owner_type, owner_id, role)
             VALUES (
                'notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png',
                'page',
                'page-1',
                'page_icon'
             )",
        )
        .execute(&pool)
        .await;
        assert!(invalid_reference.is_err());
    });
}

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
                payload,
                plain_text,
                sort_order
             )
             VALUES (
                'block-1',
                'page-1',
                'page_id',
                'page-1',
                'paragraph',
                '{\"paragraph\":{\"rich_text\":[]}}',
                'Mention target',
                1000
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
                payload,
                plain_text,
                sort_order
             )
             VALUES (
                'block-1',
                'page-1',
                'page_id',
                'page-1',
                'paragraph',
                '{\"paragraph\":{\"rich_text\":[]}}',
                'Task mention',
                1000
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
                'asset-1',
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
                'asset-1',
                'asset-1',
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
                'asset-1',
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

#[test]
fn schema_creates_strict_project_notes_history_storage() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let default_retention: i64 = sqlx::query_scalar(
            "SELECT retention_days FROM notes_page_history_settings WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(default_retention, 30);

        sqlx::query(
            "INSERT INTO project_groups (id, name, order_key) VALUES ('group-1', 'Group', 'a0')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, order_key)
             VALUES ('project-1', 'group-1', 'Learning', 'a0')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let inherited: Option<i64> = sqlx::query_scalar(
            "SELECT notes_history_retention_days FROM projects WHERE id = 'project-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(inherited, None);

        for days in [0_i64, 7, 30, 90, 180, 365] {
            sqlx::query(
                "UPDATE projects SET notes_history_retention_days = ? WHERE id = 'project-1'",
            )
            .bind(days)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query("UPDATE notes_page_history_settings SET retention_days = ? WHERE id = 1")
                .bind(days)
                .execute(&pool)
                .await
                .unwrap();
        }
        assert!(
            sqlx::query(
                "UPDATE projects SET notes_history_retention_days = 14 WHERE id = 'project-1'",
            )
            .execute(&pool)
            .await
            .is_err()
        );
        assert!(
            sqlx::query(
                "UPDATE notes_page_history_settings SET retention_days = NULL WHERE id = 1",
            )
            .execute(&pool)
            .await
            .is_err()
        );
        assert!(sqlx::query(
            "UPDATE notes_page_history_settings SET retention_days = 366 WHERE id = 1",
        )
        .execute(&pool)
        .await
        .is_err());

        for table in [
            "notes_history_bundles",
            "notes_history_bundle_chunks",
            "notes_project_history_versions",
            "notes_project_history_bundle_references",
            "notes_project_history_asset_pins",
            "notes_project_history_dirty",
            "notes_history_maintenance_state",
        ] {
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = ?",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(exists, 1, "missing table {table}");
        }

        let deadline_index_exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'index'
               AND name = 'idx_notes_project_history_dirty_deadlines'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(deadline_index_exists, 1);
    });
}

#[test]
fn notes_block_types_accept_six_heading_levels_in_blocks_and_templates() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO notes_pages (id, parent_type, title, properties) VALUES ('page', 'workspace', 'Headings', '{}');
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, payload, has_children)
               VALUES ('parent', 'page', 'page_id', 'page', 'heading_4', '{\"rich_text\":[],\"is_toggleable\":true}', 1);
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_block_id, type, payload)
               VALUES ('child', 'page', 'block_id', 'parent', 'paragraph', '{\"rich_text\":[]}');
             INSERT INTO notes_page_templates (id, name) VALUES ('template', 'Headings');
             INSERT INTO notes_page_template_blocks (template_id, id, parent_type, type, payload, has_children)
               VALUES ('template', 'parent', 'template', 'heading_4', '{\"rich_text\":[],\"is_toggleable\":true}', 1);
             INSERT INTO notes_page_template_blocks (template_id, id, parent_type, parent_block_id, type, payload)
               VALUES ('template', 'child', 'block_id', 'parent', 'paragraph', '{\"rich_text\":[]}');",
        ).execute(&pool).await.unwrap();
        for table in ["notes_blocks", "notes_page_template_blocks"] {
            for heading in ["heading_5", "heading_6"] {
                sqlx::query(AssertSqlSafe(format!(
                    "UPDATE {table} SET type = ? WHERE id = 'parent'"
                )))
                .bind(heading)
                .execute(&pool)
                .await
                .unwrap();
            }
            assert!(
                sqlx::query(AssertSqlSafe(format!(
                    "UPDATE {table} SET type = 'heading_7' WHERE id = 'parent'"
                )))
                .execute(&pool)
                .await
                .is_err()
            );
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
                .fetch_one(&pool)
                .await
                .unwrap(),
            "ok"
        );
    });
}
