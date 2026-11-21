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
        ] {
            let class = asset_path
                .rsplit_once('/')
                .map_or(asset_path, |(class, _)| class);
            let asset_id = format!("{class}:{sha256}");
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

/// Every block type the Notes schema accepts.
const NOTES_BLOCK_TYPES: [&str; 37] = [
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

#[test]
fn notes_blocks_accept_every_block_type() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, title) VALUES ('page', 'workspace', 'Page')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (index, block_type) in NOTES_BLOCK_TYPES.iter().enumerate() {
            sqlx::query(
                "INSERT INTO notes_blocks
                    (id, page_id, parent_type, parent_page_id, type, attrs, order_key)
                 VALUES (?, 'page', 'page_id', 'page', ?, '{}', ?)",
            )
            .bind(format!("block-{index:02}"))
            .bind(block_type)
            .bind(format!("a0{index:02}1"))
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{block_type}: {error}"));
        }
        let unknown = sqlx::query(
            "INSERT INTO notes_blocks
                (id, page_id, parent_type, parent_page_id, type, attrs, order_key)
             VALUES ('unknown', 'page', 'page_id', 'page', 'unknown_type', '{}', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap_err();
        assert!(
            unknown.to_string().contains("CHECK constraint failed"),
            "{unknown}"
        );
    });
}

#[test]
fn replicated_notes_tables_capture_changes_and_keep_hook_maintained_placement_unchecked() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
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
        let page_keys: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT \"from\", \"table\", on_delete FROM pragma_foreign_key_list('notes_pages')
             WHERE \"from\" IN ('parent_page_id', 'parent_data_source_id')",
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
    });
}

#[test]
fn deleting_a_data_source_removes_its_row_pages_and_options() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO notes_pages (id, parent_type, title) VALUES ('page', 'workspace', 'Page');
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, order_key)
                 VALUES ('db', 'page', 'page_id', 'page', 'child_database', 'a0');
             INSERT INTO notes_databases
                 (id, block_id, parent_type, parent_page_id, title, title_rich_text, description)
                 VALUES ('db', 'db', 'page_id', 'page', 'Tasks', '[]', '[]');
             INSERT INTO notes_data_sources (id, database_id, title, title_rich_text, description)
                 VALUES ('source', 'db', 'Tasks', '[]', '[]');
             INSERT INTO notes_data_source_properties
                 (data_source_id, property_id, sync_key, name, type, config, order_key)
                 VALUES ('source', 'stage', 'key-stage', 'Stage', 'select', '{}', 'a1');
             INSERT INTO notes_data_source_property_options
                 (data_source_id, property_id, property_key, option_id, sync_key, name, position)
                 VALUES ('source', 'stage', 'key-stage', 'todo', 'key-todo', 'To do', 'a0');
             INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title)
                 VALUES ('row', 'data_source_id', 'source', 'Row');
             DELETE FROM notes_data_sources WHERE id = 'source';",
        )
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
        assert_eq!(remaining, 0);
    });
}
