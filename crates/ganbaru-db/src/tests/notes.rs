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
            "notes_relation_edges",
            "notes_page_property_values",
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
                'database-a',
                'page-a',
                'page_id',
                'page-a',
                'child_database',
                '{\"title\":\"Tasks\"}',
                'Tasks',
                'a0'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_databases (
                id,
                block_id,
                parent_type,
                parent_page_id,
                title,
                title_rich_text,
                description
             )
             VALUES ('database-a', 'database-a', 'page_id', 'page-a', 'Tasks', '[]', '[]')",
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
                description
             )
             VALUES ('source-a', 'database-a', 'Tasks', '[]', '[]')",
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
fn schema_keeps_notes_page_project_and_trash_owner_in_columns() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let notes_triggers: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_schema
             WHERE type = 'trigger' AND tbl_name LIKE 'notes_%' AND name NOT LIKE 'sync\\_%' ESCAPE '\\'
             ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            notes_triggers,
            [
                "notes_pages_local_nesting_insert",
                "notes_pages_local_nesting_update",
                "notes_pages_park_child_page_blocks",
            ],
            "Notes rules are checked by writes, except moved-out child_page block parking \
             through foreign key cascades and local nested page pairing"
        );

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
             VALUES ('folder-a', 'folder-project-a', 'A')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, folder_id, project_id, title)
             VALUES ('folder-page', 'workspace', 'folder-a', 'folder-project-a', 'Folder page'),
                    ('loose-page', 'workspace', NULL, 'folder-project-b', 'Loose page'),
                    ('unassigned-page', 'workspace', NULL, NULL, 'Unassigned page')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "INSERT INTO notes_pages (id, parent_type, project_id, title)
                 VALUES ('missing-project-page', 'workspace', 'missing-project', 'Missing')",
            )
            .execute(&pool)
            .await
            .is_err(),
            "a page project must name an existing project"
        );
        for blank_owner in ["", "   "] {
            assert!(
                sqlx::query("UPDATE notes_pages SET trash_owner = ? WHERE id = 'loose-page'")
                    .bind(blank_owner)
                    .execute(&pool)
                    .await
                    .is_err(),
                "a page trash owner may not be blank"
            );
        }

        sqlx::query("DELETE FROM projects WHERE id = 'folder-project-a'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: Vec<(String, Option<String>, Option<String>)> =
            sqlx::query_as("SELECT id, project_id, folder_id FROM notes_pages ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            remaining,
            [
                ("folder-page".to_string(), None, None),
                (
                    "loose-page".to_string(),
                    Some("folder-project-b".to_string()),
                    None
                ),
                ("unassigned-page".to_string(), None, None),
            ],
            "deleting a project keeps its pages as unassigned workspace pages"
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
                '',
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
                'notes/page-icons:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
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
                'notes/files:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
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
                'notes/page-icons:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
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
                'notes/files:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
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
            (
                "bad-id",
                "notes/page-covers/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.png",
                "image",
                "local_upload",
                "image/png",
                1,
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "available",
                None::<&str>,
            ),
        ] {
            let class = asset_path
                .rsplit_once('/')
                .map_or(asset_path, |(class, _)| class);
            let asset_id = if id == "bad-id" {
                format!("notes/files:{sha256}")
            } else {
                format!("{class}:{sha256}")
            };
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
            .bind(asset_id)
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
                'notes/page-icons:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
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
            "INSERT INTO notes_pages (id, parent_type, title) VALUES ('page', 'workspace', 'Headings');
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, attrs, order_key)
               VALUES ('parent', 'page', 'page_id', 'page', 'heading_4', '{\"is_toggleable\":true}', 'a0');
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_block_id, type, text, order_key)
               VALUES ('child', 'page', 'block_id', 'parent', 'paragraph', '{\"rich_text\":[]}', 'a0');
             INSERT INTO notes_page_templates (id, name) VALUES ('template', 'Headings');
             INSERT INTO notes_page_template_blocks (template_id, id, ordinal, parent_type, type, payload, has_children)
               VALUES ('template', 'parent', 0, 'template', 'heading_4', '{\"rich_text\":[],\"is_toggleable\":true}', 1);
             INSERT INTO notes_page_template_blocks (template_id, id, ordinal, parent_type, parent_block_id, type, payload)
               VALUES ('template', 'child', 1, 'block_id', 'parent', 'paragraph', '{\"rich_text\":[]}');",
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
            // Template blocks are owned sync rows whose types Notes and the sync adapter check.
            if table == "notes_blocks" {
                assert!(
                    sqlx::query(AssertSqlSafe(format!(
                        "UPDATE {table} SET type = 'heading_7' WHERE id = 'parent'"
                    )))
                    .execute(&pool)
                    .await
                    .is_err()
                );
            }
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

/// A migrated block: id, type, attrs, text, trash owner, open, and editing lock.
type MigratedFixtureBlock = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

/// Last migration before the Notes structure preparation.
const BEFORE_NOTES_SYNC_STRUCTURE: i64 = 20261024045821;

/// The Notes structure preparation migration.
pub(super) const NOTES_SYNC_STRUCTURE: i64 = 20261024075934;

/// Last migration before Notes structure replication.
const BEFORE_NOTES_STRUCTURE_REPLICATION: i64 = 20261025051154;

/// The migration that makes Notes structure replicated.
const NOTES_STRUCTURE_REPLICATION: i64 = 20261025091420;

/// Last migration before Notes databases replication.
const BEFORE_NOTES_DATABASES_REPLICATION: i64 = 20261025183055;

/// The migration that makes Notes databases and templates replicated.
const NOTES_DATABASES_REPLICATION: i64 = 20261025212528;

/// Every block type the Notes schema accepts.
const FIXTURE_BLOCK_TYPES: [&str; 37] = [
    "paragraph",
    "heading_1",
    "heading_2",
    "heading_3",
    "heading_4",
    "heading_5",
    "heading_6",
    "bulleted_list_item",
    "numbered_list_item",
    "to_do",
    "toggle",
    "callout",
    "quote",
    "child_page",
    "child_database",
    "breadcrumb",
    "table_of_contents",
    "column_list",
    "column",
    "table",
    "table_row",
    "tab",
    "image",
    "video",
    "audio",
    "file",
    "pdf",
    "bookmark",
    "link_preview",
    "synced_block",
    "template",
    "button",
    "embed",
    "equation",
    "divider",
    "code",
    "unsupported",
];

/// Page property values of every Notes property type.
pub(super) fn fixture_page_properties() -> serde_json::Value {
    serde_json::json!({
        "title": { "type": "title", "title": [{ "type": "text", "text": { "content": "Fixture" }, "plain_text": "Fixture" }] },
        "Notes": { "type": "rich_text", "rich_text": [{ "type": "text", "text": { "content": "Body" }, "plain_text": "Body" }] },
        "Estimate": { "type": "number", "number": 3.5 },
        "Stage": { "type": "select", "select": { "id": "s1", "name": "Doing", "color": "blue" } },
        "Tags": { "type": "multi_select", "multi_select": [{ "id": "t1", "name": "Home", "color": "red" }] },
        "Status": { "type": "status", "status": { "id": "st", "name": "Done", "color": "green" } },
        "Due": { "type": "date", "date": { "start": "2026-10-24", "end": null, "time_zone": null } },
        "Owner": { "type": "people", "people": [{ "object": "user", "id": "user-1" }] },
        "Files": { "type": "files", "files": [{ "name": "a.pdf", "type": "file", "file": { "url": "assets/notes/files/a.pdf" } }] },
        "Done": { "type": "checkbox", "checkbox": true },
        "Link": { "type": "url", "url": "https://example.com" },
        "Mail": { "type": "email", "email": "a@example.com" },
        "Phone": { "type": "phone_number", "phone_number": "+1 555 0100" },
        "Score": { "type": "formula", "formula": { "type": "number", "number": 7 } },
        "Related": { "type": "relation", "relation": [{ "id": "page-2" }], "has_more": false },
        "Total": { "type": "rollup", "rollup": { "type": "number", "number": 2, "function": "count" } },
        "Created": { "type": "created_time", "created_time": "2026-10-01T00:00:00.000Z" },
        "Creator": { "type": "created_by", "created_by": { "object": "user", "id": "user-1" } },
        "Edited": { "type": "last_edited_time", "last_edited_time": "2026-10-02T00:00:00.000Z" },
        "Editor": { "type": "last_edited_by", "last_edited_by": { "object": "user", "id": "user-1" } },
        "Number": { "type": "unique_id", "unique_id": { "prefix": "TASK", "number": 4 } },
        "Place": { "type": "place", "place": { "name": "Office", "lat": 1.5, "lon": 2.5 } },
        "Verified": { "type": "verification", "verification": { "state": "verified" } },
        "Action": { "type": "button", "button": {} }
    })
}

/// A previous-shape block payload carrying text keys, attributes, and view state for `block_type`.
fn fixture_block_payload(block_type: &str) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "rich_text": [{ "type": "text", "text": { "content": block_type }, "plain_text": block_type }],
        "caption": [],
        "color": "gray_background",
        "url": format!("https://example.com/{block_type}")
    });
    match block_type {
        "toggle" => payload["ganbaru_open"] = serde_json::json!(true),
        "child_database" => payload["editing_locked"] = serde_json::json!(true),
        "child_page" => payload["title"] = serde_json::json!("Stale title"),
        "table_row" => payload["cells"] = serde_json::json!([[], []]),
        _ => {}
    }
    payload
}

#[test]
fn notes_sync_structure_migration_keeps_every_block_and_property_type() {
    super::block_on(async {
        let pool = super::helpers::memory_pool_migrated_through(BEFORE_NOTES_SYNC_STRUCTURE).await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, order_key) VALUES ('group', 'Group', 'a0');
             INSERT INTO projects (id, group_id, name, order_key) VALUES ('project', 'group', 'P', 'a0');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut properties = fixture_page_properties();
        properties["__ganbaru_project_id"] = serde_json::json!("project");
        properties["__ganbaru_trash_owner"] = serde_json::json!("owner-page");
        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, title, properties)
             VALUES ('page', 'workspace', 'Fixture', ?)",
        )
        .bind(properties.to_string())
        .execute(&pool)
        .await
        .unwrap();
        let block_count = FIXTURE_BLOCK_TYPES.len();
        for (index, block_type) in FIXTURE_BLOCK_TYPES.iter().enumerate() {
            let mut payload = fixture_block_payload(block_type);
            if index == 0 {
                payload["__ganbaru_trash_owner"] = serde_json::json!("owner-block");
            }
            // Reverse order so the order keys must follow `sort_order`, not the ids.
            let sort_order = (block_count - index) as f64 * 10.0;
            sqlx::query(
                "INSERT INTO notes_blocks (
                    id, page_id, parent_type, parent_page_id, in_trash, type, payload, plain_text,
                    sort_order
                 ) VALUES (printf('block-%02d', ?), 'page', 'page_id', 'page', ?, ?, ?, ?, ?)",
            )
            .bind(index as i64)
            .bind(i64::from(index == 0))
            .bind(block_type)
            .bind(payload.to_string())
            .bind(block_type)
            .bind(sort_order)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO notes_blocks (
                id, page_id, parent_type, parent_block_id, type, payload, plain_text, sort_order
             ) VALUES ('nested', 'page', 'block_id', 'block-10', 'paragraph', '{\"rich_text\":[]}', '', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // `block-13` is the fixture's child_page block; `orphan` names no block. The nested page
        // carries a stale project property, which the migration must not keep in the column.
        sqlx::raw_sql(
            "INSERT INTO notes_pages (id, parent_type, parent_page_id, title, properties)
             VALUES ('block-13', 'page_id', 'page', 'Nested', '{\"__ganbaru_project_id\":\"project\"}'),
                    ('orphan', 'page_id', 'page', 'Orphan', '{}');",
        )
        .execute(&pool)
        .await
        .unwrap();

        super::helpers::migrate_through(&pool, NOTES_SYNC_STRUCTURE)
            .await
            .unwrap();

        let placements: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, parent_type, child_block_id, project_id FROM notes_pages
             WHERE id IN ('block-13', 'orphan') ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            placements,
            vec![
                (
                    "block-13".to_string(),
                    "page_id".to_string(),
                    Some("block-13".to_string()),
                    None
                ),
                ("orphan".to_string(), "workspace".to_string(), None, None),
            ]
        );

        let (project_id, trash_owner, stored_properties): (Option<String>, Option<String>, String) =
            sqlx::query_as(
                "SELECT project_id, trash_owner, properties FROM notes_pages WHERE id = 'page'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(project_id.as_deref(), Some("project"));
        assert_eq!(trash_owner.as_deref(), Some("owner-page"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored_properties).unwrap(),
            fixture_page_properties()
        );

        let rows: Vec<MigratedFixtureBlock> = sqlx::query_as(
            "SELECT block.id, block.type, block.attrs, block.text, block.trash_owner,
                        view.open, view.editing_locked
                 FROM notes_blocks AS block
                 LEFT JOIN notes_block_view_state AS view ON view.block_id = block.id
                 WHERE block.parent_page_id = 'page'
                 ORDER BY block.order_key, block.id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), block_count);
        for (position, (id, block_type, attrs, text, block_trash_owner, open, locked)) in
            rows.into_iter().enumerate()
        {
            let index = block_count - 1 - position;
            assert_eq!(id, format!("block-{index:02}"), "order follows sort_order");
            assert_eq!(block_type, FIXTURE_BLOCK_TYPES[index]);
            let mut expected = fixture_block_payload(&block_type);
            let expected_open = expected
                .as_object_mut()
                .unwrap()
                .remove("ganbaru_open")
                .map(|value| i64::from(value == true));
            let expected_locked = expected
                .as_object_mut()
                .unwrap()
                .remove("editing_locked")
                .map(|value| i64::from(value == true));
            expected.as_object_mut().unwrap().remove("title");
            let mut stored: serde_json::Value = serde_json::from_str(&attrs).unwrap();
            let text: serde_json::Value = serde_json::from_str(&text).unwrap();
            for (key, value) in text.as_object().unwrap() {
                assert!(
                    ["rich_text", "cells", "caption"].contains(&key.as_str()),
                    "{key}"
                );
                stored[key] = value.clone();
            }
            assert_eq!(stored, expected, "{block_type}");
            assert_eq!(open, expected_open, "{block_type}");
            assert_eq!(locked, expected_locked, "{block_type}");
            let expected_owner = (index == 0).then(|| "owner-block".to_string());
            assert_eq!(block_trash_owner, expected_owner, "{block_type}");
        }
        let nested: (String, String) = sqlx::query_as(
            "SELECT parent_block_id, order_key FROM notes_blocks WHERE id = 'nested'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(nested, ("block-10".to_string(), "c000".to_string()));
        let violations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(violations, 0);
    });
}

#[test]
fn notes_sync_structure_migration_merges_duplicate_assets_into_the_available_row() {
    super::block_on(async {
        let pool = super::helpers::memory_pool_migrated_through(BEFORE_NOTES_SYNC_STRUCTURE).await;
        let sha256 = "a".repeat(64);
        sqlx::query(
            "INSERT INTO notes_assets (
                id, asset_path, kind, source_type, original_name, content_type, byte_size, sha256,
                storage_state, missing_at, created_at
             ) VALUES
                ('missing-old', 'notes/files/old.pdf', 'pdf', 'local_upload', NULL,
                 'application/pdf', 10, ?1, 'missing', '2026-02-01T00:00:00.000Z',
                 '2026-01-01T00:00:00.000Z'),
                ('available-new', 'notes/files/new.pdf', 'pdf', 'local_upload', 'Report.pdf',
                 'application/pdf', 10, ?1, 'available', NULL, '2026-03-01T00:00:00.000Z')",
        )
        .bind(&sha256)
        .execute(&pool)
        .await
        .unwrap();
        let properties = serde_json::json!({
            "title": { "type": "title", "title": [] },
            "Files": { "type": "files", "files": [
                { "name": "old.pdf", "type": "file", "file": { "url": "notes/files/old.pdf" } }
            ] }
        });
        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, title, properties)
             VALUES ('page', 'workspace', 'Assets', ?)",
        )
        .bind(properties.to_string())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_asset_references (asset_id, owner_type, owner_id, page_id, role)
             VALUES ('missing-old', 'page', 'page', 'page', 'page_cover')",
        )
        .execute(&pool)
        .await
        .unwrap();

        super::helpers::migrate_through(&pool, NOTES_SYNC_STRUCTURE)
            .await
            .unwrap();

        let merged_id = format!("notes/files:{sha256}");
        let assets: Vec<(String, String, String, Option<String>)> =
            sqlx::query_as("SELECT id, asset_path, storage_state, original_name FROM notes_assets")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            assets,
            vec![(
                merged_id.clone(),
                "notes/files/new.pdf".to_string(),
                "available".to_string(),
                Some("Report.pdf".to_string()),
            )]
        );
        let reference: String = sqlx::query_scalar(
            "SELECT asset_id FROM notes_asset_references WHERE page_id = 'page'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reference, merged_id);
        let stored: String =
            sqlx::query_scalar("SELECT properties FROM notes_pages WHERE id = 'page'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let stored: serde_json::Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(
            stored["Files"]["files"][0]["file"]["url"],
            "notes/files/new.pdf"
        );
    });
}

#[test]
fn fresh_vault_notes_blocks_store_split_content_and_order_keys() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('notes_blocks') ORDER BY cid")
                .fetch_all(&pool)
                .await
                .unwrap();
        for present in ["attrs", "text", "order_key", "trash_owner"] {
            assert!(columns.iter().any(|column| column == present), "{present}");
        }
        for absent in ["payload", "sort_order", "has_children"] {
            assert!(!columns.iter().any(|column| column == absent), "{absent}");
        }
        let page_columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('notes_pages')")
                .fetch_all(&pool)
                .await
                .unwrap();
        for present in ["project_id", "trash_owner"] {
            assert!(
                page_columns.iter().any(|column| column == present),
                "{present}"
            );
        }
        let notes_triggers: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger' AND tbl_name IN ('notes_blocks', 'notes_pages', 'notes_folders') AND name NOT LIKE 'sync\\_%' ESCAPE '\\' AND name NOT LIKE 'notes\\_pages\\_local\\_nesting\\_%' ESCAPE '\\' AND name <> 'notes_pages_park_child_page_blocks'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(notes_triggers, 0);

        sqlx::raw_sql(
            "INSERT INTO notes_pages (id, parent_type, title) VALUES ('page', 'workspace', 'P');
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, order_key)
               VALUES ('block', 'page', 'page_id', 'page', 'toggle', 'a0');",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (column, value) in [("attrs", "[]"), ("text", "\"x\""), ("order_key", "")] {
            let rejected = sqlx::query(AssertSqlSafe(format!(
                "UPDATE notes_blocks SET {column} = ? WHERE id = 'block'"
            )))
            .bind(value)
            .execute(&pool)
            .await;
            assert!(rejected.is_err(), "{column} accepted {value}");
        }
        assert!(
            sqlx::query("INSERT INTO notes_block_view_state (block_id) VALUES ('block')")
                .execute(&pool)
                .await
                .is_err(),
            "an empty view state row is refused"
        );
        sqlx::query("INSERT INTO notes_block_view_state (block_id, open) VALUES ('block', 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM notes_pages WHERE id = 'page'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM notes_blocks) + (SELECT COUNT(*) FROM notes_block_view_state)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0);
    });
}

#[test]
fn fresh_vault_nested_pages_follow_their_child_page_block() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, order_key) VALUES ('group', 'G', 'a0');
             INSERT INTO projects (id, group_id, name, order_key) VALUES ('project', 'group', 'P', 'a0');
             INSERT INTO notes_folders (id, project_id, name) VALUES ('folder', 'project', 'F');
             INSERT INTO notes_pages (id, parent_type, title) VALUES ('parent', 'workspace', 'P');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, parent_page_id, child_block_id, title)
             VALUES ('child', 'page_id', 'parent', 'child', 'C')",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, order_key)
             VALUES ('child', 'parent', 'page_id', 'parent', 'child_page', 'a0')",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit()
            .await
            .expect("the block reference is checked at commit");

        // Local writes keep the parent columns paired with `child_block_id`; remote placement
        // is paired by the Notes structure repair.
        for statement in [
            "UPDATE notes_pages SET child_block_id = NULL WHERE id = 'child'",
            "UPDATE notes_pages SET parent_type = 'workspace', parent_page_id = NULL WHERE id = 'child'",
            "UPDATE notes_pages SET child_block_id = 'parent' WHERE id = 'child'",
            "UPDATE notes_pages SET child_block_id = 'parent' WHERE id = 'parent'",
            "UPDATE notes_pages SET folder_id = 'folder' WHERE id = 'child'",
        ] {
            assert!(
                sqlx::raw_sql(AssertSqlSafe(statement))
                    .execute(&pool)
                    .await
                    .is_err(),
                "{statement}"
            );
        }
        assert!(
            sqlx::query(
                "INSERT INTO notes_pages (id, parent_type, parent_page_id, child_block_id, title)
                 VALUES ('loose', 'page_id', 'parent', 'loose', 'L')",
            )
            .execute(&pool)
            .await
            .is_err(),
            "a nested page needs its child_page block"
        );
        sqlx::raw_sql(
            "UPDATE sync_apply_state SET applying = 1;
             UPDATE notes_pages SET child_block_id = NULL WHERE id = 'child';
             UPDATE notes_pages SET child_block_id = 'child' WHERE id = 'child';
             UPDATE sync_apply_state SET applying = 0;",
        )
        .execute(&pool)
        .await
        .expect("apply writes placement before the hook derives the parent columns");

        sqlx::query("DELETE FROM notes_blocks WHERE id = 'child'")
            .execute(&pool)
            .await
            .unwrap();
        let pages: Vec<String> = sqlx::query_scalar("SELECT id FROM notes_pages ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(pages, vec!["parent".to_string()]);
    });
}

/// Columns of `notes_pages` the replication migration copies.
const REPLICATED_PAGE_COLUMNS: &str = "id, parent_type, parent_page_id, parent_block_id, \
    parent_data_source_id, child_block_id, project_id, title, icon, cover, in_trash, trash_owner, \
    archived, source_provider, source_object_id, source_workspace_id, source_last_edited_time, \
    url, public_url, created_time, last_edited_time, trashed_time, folder_id, parent_row_page_id";

/// Columns of `notes_blocks` the replication migration copies.
const REPLICATED_BLOCK_COLUMNS: &str = "id, page_id, parent_type, parent_page_id, \
    parent_block_id, in_trash, trash_owner, type, attrs, text, plain_text, order_key, \
    source_provider, source_object_id, source_last_edited_time, created_time, last_edited_time";

/// Every row of `table` as one JSON array of `columns` per row, ordered by id.
async fn row_values(pool: &sqlx::SqlitePool, table: &str, columns: &str) -> Vec<String> {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT json_array({columns}) FROM {table} ORDER BY id"
    )))
    .fetch_all(pool)
    .await
    .unwrap()
}

#[test]
fn notes_structure_replication_keeps_seeded_structure_and_adds_sync_state() {
    super::block_on(async {
        let pool =
            super::helpers::memory_pool_migrated_through(BEFORE_NOTES_STRUCTURE_REPLICATION).await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, order_key) VALUES ('group', 'G', 'a0');
             INSERT INTO projects (id, group_id, name, order_key) VALUES ('project', 'group', 'P', 'a0');
             INSERT INTO notes_folders (id, project_id, name) VALUES ('folder', 'project', 'F');
             INSERT INTO notes_pages (id, parent_type, title, folder_id, created_time)
                 VALUES ('parent', 'workspace', 'Parent', 'folder', '2026-10-01T00:00:00Z');
             INSERT INTO notes_pages (id, parent_type, title, project_id, icon, in_trash, trash_owner, trashed_time, url, public_url)
                 VALUES ('trashed', 'workspace', 'Trashed', 'project', '{\"type\":\"emoji\",\"emoji\":\"x\"}', 1, 'trashed',
                     '2026-10-02T00:00:00Z', 'https://example.test/a', 'https://example.test/b');
             INSERT INTO notes_page_aliases (id, page_id, alias, normalized_alias)
                 VALUES ('home', 'parent', 'Home', 'home');
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, plain_text, order_key)
                 VALUES ('root', 'parent', 'page_id', 'parent', 'paragraph', 'root text', 'a0');
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_block_id, type, plain_text, order_key)
                 VALUES ('nested', 'parent', 'block_id', 'root', 'paragraph', 'nested text', 'a0');
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, plain_text, order_key, in_trash, trash_owner)
                 VALUES ('gone', 'parent', 'page_id', 'parent', 'paragraph', 'gone text', 'a3', 1, 'gone');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        for statement in [
            "INSERT INTO notes_pages (id, parent_type, parent_page_id, child_block_id, title)
             VALUES ('child', 'page_id', 'parent', 'child', 'Child')",
            "INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, order_key)
             VALUES ('child', 'parent', 'page_id', 'parent', 'child_page', 'a1')",
            "INSERT INTO notes_pages (id, parent_type, parent_block_id, child_block_id, title)
             VALUES ('deep', 'block_id', 'root', 'deep', 'Deep')",
            "INSERT INTO notes_blocks (id, page_id, parent_type, parent_block_id, type, order_key)
             VALUES ('deep', 'parent', 'block_id', 'root', 'child_page', 'a1')",
            "INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, order_key)
             VALUES ('db', 'parent', 'page_id', 'parent', 'child_database', 'a2')",
            "INSERT INTO notes_databases (id, block_id, parent_type, parent_page_id, title)
             VALUES ('db', 'db', 'page_id', 'parent', 'Tasks')",
            "INSERT INTO notes_data_sources (id, database_id, title) VALUES ('source', 'db', 'Tasks')",
            "INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title)
             VALUES ('row', 'data_source_id', 'source', 'Row')",
            "INSERT INTO notes_pages (id, parent_type, parent_data_source_id, parent_row_page_id, title)
             VALUES ('subrow', 'data_source_id', 'source', 'row', 'Subrow')",
        ] {
            sqlx::query(statement).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();
        let pages_before = row_values(&pool, "notes_pages", REPLICATED_PAGE_COLUMNS).await;
        let blocks_before = row_values(&pool, "notes_blocks", REPLICATED_BLOCK_COLUMNS).await;
        let aliases_before = row_values(
            &pool,
            "notes_page_aliases",
            "id, page_id, alias, normalized_alias",
        )
        .await;
        assert_eq!(pages_before.len(), 6);
        assert_eq!(blocks_before.len(), 6);

        super::helpers::migrate_through(&pool, NOTES_STRUCTURE_REPLICATION)
            .await
            .unwrap();

        assert_eq!(
            row_values(&pool, "notes_pages", REPLICATED_PAGE_COLUMNS).await,
            pages_before
        );
        assert_eq!(
            row_values(&pool, "notes_blocks", REPLICATED_BLOCK_COLUMNS).await,
            blocks_before
        );
        assert_eq!(
            row_values(
                &pool,
                "notes_page_aliases",
                "id, page_id, alias, normalized_alias"
            )
            .await,
            aliases_before
        );
        let sources: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_data_sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(sources, 1);

        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table'
             AND name IN ('notes_page_history_pending', 'notes_sync_repair_pending')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pending, 2);
        for table in [
            "notes_local_users",
            "notes_folders",
            "notes_pages",
            "notes_page_aliases",
            "notes_blocks",
        ] {
            let sync_triggers: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_schema
                 WHERE type = 'trigger' AND tbl_name = ? AND name LIKE 'sync\\_%' ESCAPE '\\'",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(sync_triggers, 7, "{table}");
        }
        // Placement columns the Notes hook maintains carry no foreign key.
        let page_keys: Vec<String> = sqlx::query_scalar(
            "SELECT \"from\" FROM pragma_foreign_key_list('notes_pages') ORDER BY \"from\"",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(!page_keys.contains(&"parent_page_id".to_string()));
        assert!(!page_keys.contains(&"parent_data_source_id".to_string()));
        let violations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(violations, 0);
    });
}

#[test]
fn notes_databases_replication_keeps_databases_and_converts_views_options_and_rows() {
    super::block_on(async {
        let pool =
            super::helpers::memory_pool_migrated_through(BEFORE_NOTES_DATABASES_REPLICATION).await;
        sqlx::raw_sql(
            "INSERT INTO notes_pages (id, parent_type, title) VALUES ('page', 'workspace', 'Page');
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, order_key)
                 VALUES ('db', 'page', 'page_id', 'page', 'child_database', 'a0');
             INSERT INTO notes_databases (id, block_id, parent_type, parent_page_id, title)
                 VALUES ('db', 'db', 'page_id', 'page', 'Tasks');
             INSERT INTO notes_data_sources (id, database_id, title) VALUES ('source', 'db', 'Tasks');
             INSERT INTO notes_data_source_properties
                 (data_source_id, property_id, sync_key, name, type, config, order_key)
                 VALUES ('source', 'stage', 'key-stage', 'Stage', 'select', '{}', 'a1');
             INSERT INTO notes_data_source_property_options
                 (data_source_id, property_id, option_id, sync_key, name, position)
                 VALUES ('source', 'stage', 'todo', 'key-todo', 'To do', 'a0');
             INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title)
                 VALUES ('row', 'data_source_id', 'source', 'Row');
             INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title)
                 VALUES ('orphan', 'data_source_id', 'gone', 'Orphan');
             INSERT INTO notes_database_views (id, database_id, data_source_id, name, type, sort_order)
                 VALUES ('view-late', 'db', 'source', 'Late', 'table', 2.0),
                        ('view-early', 'db', 'source', 'Early', 'board', 1.0);
             INSERT INTO notes_page_templates (id, name) VALUES ('template', 'Weekly');
             INSERT INTO notes_page_template_blocks
                 (template_id, ordinal, id, parent_type, parent_block_id, type, payload)
                 VALUES ('template', 0, 'goals', 'template', NULL, 'paragraph', '{}'),
                        ('template', 1, 'review', 'block_id', 'goals', 'paragraph', '{}');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let databases_before = row_values(
            &pool,
            "notes_databases",
            "id, block_id, parent_type, parent_page_id, parent_block_id, title",
        )
        .await;
        let blocks_before = row_values(
            &pool,
            "notes_page_template_blocks",
            "template_id, ordinal, parent_type, parent_block_id, type, payload",
        )
        .await;

        super::helpers::migrate_through(&pool, NOTES_DATABASES_REPLICATION)
            .await
            .unwrap();

        assert_eq!(
            row_values(
                &pool,
                "notes_databases",
                "id, block_id, parent_type, parent_page_id, parent_block_id, title",
            )
            .await,
            databases_before
        );
        assert_eq!(
            row_values(
                &pool,
                "notes_page_template_blocks",
                "template_id, ordinal, parent_type, parent_block_id, type, payload",
            )
            .await,
            blocks_before
        );
        let views: Vec<String> =
            sqlx::query_scalar("SELECT id FROM notes_database_views ORDER BY order_key, id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(views, ["view-early", "view-late"], "views keep their order");
        let property_key: String = sqlx::query_scalar(
            "SELECT property_key FROM notes_data_source_property_options WHERE option_id = 'todo'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(property_key, "key-stage");
        let pages: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT id, parent_type, parent_data_source_id FROM notes_pages
             WHERE id IN ('row', 'orphan') ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            pages,
            [
                ("orphan".to_string(), "workspace".to_string(), None),
                (
                    "row".to_string(),
                    "data_source_id".to_string(),
                    Some("source".to_string())
                ),
            ],
            "a row page without its data source becomes a workspace page"
        );

        let page_keys: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT \"from\", \"table\", on_delete FROM pragma_foreign_key_list('notes_pages')
             WHERE \"from\" = 'parent_data_source_id'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            page_keys,
            [(
                "parent_data_source_id".to_string(),
                "notes_data_sources".to_string(),
                "CASCADE".to_string()
            )]
        );
        sqlx::query("DELETE FROM notes_data_sources WHERE id = 'source'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM notes_pages WHERE id = 'row')
                  + (SELECT count(*) FROM notes_data_source_property_options)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0, "row pages cascade from their data source");
        for table in [
            "notes_pages",
            "notes_databases",
            "notes_data_sources",
            "notes_database_views",
            "notes_page_templates",
            "notes_data_source_templates",
        ] {
            let sync_triggers: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_schema
                 WHERE type = 'trigger' AND tbl_name = ? AND name LIKE 'sync\\_%' ESCAPE '\\'",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(sync_triggers, 7, "{table}");
        }
        let violations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(violations, 0);
    });
}
