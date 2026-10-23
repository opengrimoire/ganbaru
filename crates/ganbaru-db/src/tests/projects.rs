use super::helpers::{memory_pool_migrated_through, migrated_memory_pool};
use crate::run_migrations;

const ICON_FILE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png";
const BEFORE_ORDER_KEYS: i64 = 20261022191859;

#[test]
fn schema_creates_project_custom_emojis() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let path = format!("project-icons/{ICON_FILE}");

        sqlx::query(
            "INSERT INTO project_custom_emojis (id, name, asset_path, asset_byte_size, order_key)
             VALUES ('emoji-1', 'Rocket', ?, 120, 'a0')",
        )
        .bind(&path)
        .execute(&pool)
        .await
        .unwrap();

        let asset_path: String =
            sqlx::query_scalar("SELECT asset_path FROM project_custom_emojis WHERE id = 'emoji-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(asset_path, path);

        let bad_file = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png";
        for (id, name, asset_path, size, order_key) in [
            (
                "bad-name",
                "",
                format!("project-icons/{bad_file}"),
                Some(1),
                "a1",
            ),
            (
                "bad-prefix",
                "Bad",
                format!("other/{bad_file}"),
                Some(1),
                "a1",
            ),
            (
                "bad-parent",
                "Bad",
                format!("project-icons/../{bad_file}"),
                Some(1),
                "a1",
            ),
            (
                "bad-nested",
                "Bad",
                format!("project-icons/nested/{bad_file}"),
                Some(1),
                "a1",
            ),
            (
                "bad-order",
                "Bad",
                format!("project-icons/{bad_file}"),
                Some(1),
                "",
            ),
            (
                "bad-no-size",
                "Bad",
                format!("project-icons/{bad_file}"),
                None,
                "a1",
            ),
            (
                "bad-empty",
                "Bad",
                format!("project-icons/{bad_file}"),
                Some(0),
                "a1",
            ),
            (
                "bad-large",
                "Bad",
                format!("project-icons/{bad_file}"),
                Some(3_145_729),
                "a1",
            ),
        ] {
            assert!(
                sqlx::query(
                    "INSERT INTO project_custom_emojis (id, name, asset_path, asset_byte_size, order_key)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(id)
                .bind(name)
                .bind(asset_path)
                .bind(size)
                .bind(order_key)
                .execute(&pool)
                .await
                .is_err(),
                "{id} should fail"
            );
        }
    });
}

#[test]
fn project_icons_require_their_size_only_for_managed_files() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let reference = format!("asset:project-icons/{ICON_FILE}");
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, icon_byte_size, order_key)
             VALUES ('group-1', 'Work', ?, 64, 'a0')",
        )
        .bind(&reference)
        .execute(&pool)
        .await
        .unwrap();

        for (icon, size) in [
            (reference.as_str(), None),
            (reference.as_str(), Some(0)),
            (reference.as_str(), Some(3_145_729)),
            ("lucide:folder", Some(64)),
        ] {
            let result = sqlx::query(
                "UPDATE project_groups SET icon = ?, icon_byte_size = ? WHERE id = 'group-1'",
            )
            .bind(icon)
            .bind(size)
            .execute(&pool)
            .await;
            assert!(result.is_err(), "{icon} {size:?}");
        }
        for (icon, size) in [
            (reference.as_str(), Some(3_145_728)),
            ("lucide:folder", None),
        ] {
            sqlx::query(
                "UPDATE project_groups SET icon = ?, icon_byte_size = ? WHERE id = 'group-1'",
            )
            .bind(icon)
            .bind(size)
            .execute(&pool)
            .await
            .unwrap();
        }

        let missing_size = sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, order_key)
             VALUES ('project-1', 'group-1', 'Launch', ?, 'a0')",
        )
        .bind(&reference)
        .execute(&pool)
        .await;
        assert!(
            missing_size.is_err(),
            "a managed project icon needs its size"
        );
    });
}

#[test]
fn project_views_allow_merged_hosted_views_and_cascade_their_preferences() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, icon, order_key) VALUES ('group-1', 'Work', 'lucide:folder', 'a0');
             INSERT INTO projects (id, group_id, name, icon, order_key)
             VALUES ('project-1', 'group-1', 'Launch', 'lucide:folder', 'a0');
             INSERT INTO project_views (id, project_id, view_type, order_key) VALUES
                ('view-list-a', 'project-1', 'list', 'a0'),
                ('view-list-b', 'project-1', 'list', 'a1'),
                ('view-notes', 'project-1', 'notes', 'a2');
             INSERT INTO project_view_preferences (view_id, preference_key, preference_value, sync_key)
             VALUES ('view-list-b', 'list-presentation', '{}', 'view-preference:1');",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO project_views (id, project_id, view_type, order_key)
             VALUES ('view-notes-2', 'project-1', 'notes', 'a3')",
        )
        .execute(&pool)
        .await
        .expect("two devices may each add a Notes view");

        let blank_name = sqlx::query(
            "INSERT INTO project_views (id, project_id, view_type, name, order_key)
             VALUES ('view-blank', 'project-1', 'kanban', '  ', 'a4')",
        )
        .execute(&pool)
        .await;
        assert!(blank_name.is_err(), "a renamed view needs a visible name");
        let missing_key = sqlx::query(
            "INSERT INTO project_views (id, project_id, view_type) VALUES ('view-x', 'project-1', 'kanban')",
        )
        .execute(&pool)
        .await;
        assert!(missing_key.is_err(), "a view needs an order key");

        sqlx::query("DELETE FROM project_views WHERE id = 'view-list-b'")
            .execute(&pool)
            .await
            .unwrap();
        let preferences: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_view_preferences")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(preferences, 0);

        sqlx::query("DELETE FROM projects WHERE id = 'project-1'")
            .execute(&pool)
            .await
            .unwrap();
        let views: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_views WHERE project_id = 'project-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(views, 0);
    });
}

#[test]
fn migration_gives_seeded_projects_the_default_views_in_order_key_order() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, view_type FROM project_views
             WHERE project_id = 'project-routine-learning'
             ORDER BY order_key, id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            [
                ("view-project-routine-learning-dashboard", "dashboard"),
                ("view-project-routine-learning-list", "list"),
                ("view-project-routine-learning-notes", "notes"),
                ("view-project-routine-learning-chat", "chat"),
            ]
            .map(|(id, view_type)| (id.to_string(), view_type.to_string())),
        );
    });
}

#[test]
fn task_revision_changes_only_when_a_command_writes_it() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_with_task(&pool).await;
        sqlx::query(
            "UPDATE project_tasks SET title = 'Renamed', section_order_key = 'a5' WHERE id = 'task-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM project_tasks WHERE id = 'task-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(revision, 0);
    });
}

#[test]
fn select_values_own_their_options_through_the_value_sync_key() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_with_task(&pool).await;
        sqlx::raw_sql(
            "INSERT INTO project_custom_fields (id, project_id, name, field_type, order_key)
             VALUES ('field-select', 'project-1', 'Area', 'multi_select', 'a0');
             INSERT INTO project_custom_field_options (id, field_id, name, order_key)
             VALUES ('option-a', 'field-select', 'A', 'a0'), ('option-b', 'field-select', 'B', 'a1');
             INSERT INTO project_custom_field_values (task_id, field_id, sync_key)
             VALUES ('task-1', 'field-select', 'pending:1');
             INSERT INTO project_custom_field_option_values (value_key, ordinal, option_id)
             VALUES ('pending:1', 0, 'option-b'), ('pending:1', 1, 'option-a');
             UPDATE project_custom_field_values SET sync_key = 'value:final' WHERE sync_key = 'pending:1';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let carried: Vec<(String, i64, String)> = sqlx::query_as(
            "SELECT value_key, ordinal, option_id FROM project_custom_field_option_values ORDER BY ordinal",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            carried,
            vec![
                ("value:final".to_string(), 0, "option-b".to_string()),
                ("value:final".to_string(), 1, "option-a".to_string()),
            ]
        );

        sqlx::query(
            "INSERT INTO project_custom_fields (id, project_id, name, field_type, order_key)
             VALUES ('field-text', 'project-1', 'Note', 'text', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let two_scalars = sqlx::query(
            "INSERT INTO project_custom_field_values (task_id, field_id, sync_key, text_value, number_value)
             VALUES ('task-1', 'field-text', 'value:other', 'x', 1)",
        )
        .execute(&pool)
        .await;
        assert!(two_scalars.is_err());
        let real_number = sqlx::query(
            "UPDATE project_custom_field_values SET number_value = 1.5 WHERE sync_key = 'value:final'",
        )
        .execute(&pool)
        .await;
        assert!(real_number.is_err(), "numbers are integer millionths");

        sqlx::query("DELETE FROM project_custom_field_values WHERE sync_key = 'value:final'")
            .execute(&pool)
            .await
            .unwrap();
        let owned: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_custom_field_option_values")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(owned, 0);
    });
}

#[test]
fn order_keys_migration_ranks_rows_and_converts_values() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_ORDER_KEYS).await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, icon, sort_order)
             VALUES ('group-b', 'Beta', 'lucide:folder', 5000), ('group-a', 'Alpha', 'lucide:folder', 5000);
             INSERT INTO projects (id, group_id, name, icon, sort_order)
             VALUES ('project-1', 'group-a', 'Launch', 'lucide:folder', 2),
                    ('project-2', 'group-a', 'Admin', 'lucide:folder', 1);
             INSERT INTO project_sections (id, project_id, name, sort_order)
             VALUES ('section-2', 'project-1', 'Later', 1), ('section-1', 'project-1', 'Now', 0);
             INSERT INTO project_statuses (id, project_id, name, category, sort_order)
             VALUES ('status-done', 'project-1', 'Done', 'done', 1),
                    ('status-todo', 'project-1', 'To do', 'not_started', 0);
             INSERT INTO project_priorities (id, project_id, name, color, sort_order)
             VALUES ('high', 'project-1', 'High', 1, 0), ('low', 'project-1', 'Low', 2, 1);
             INSERT INTO project_tags (id, project_id, name, sort_order)
             VALUES ('tag-1', 'project-1', 'Bug', 0);
             INSERT INTO project_tasks
                (id, project_id, section_id, status_id, title, section_sort_order, status_sort_order, created_at)
             VALUES ('task-c', 'project-1', 'section-1', 'status-todo', 'C', 2.5, 0.25, '2026-01-01T00:00:00.000Z'),
                    ('task-a', 'project-1', 'section-1', 'status-todo', 'A', 0.5, 0.75, '2026-01-01T00:00:00.000Z'),
                    ('task-b', 'project-1', 'section-1', 'status-done', 'B', 0.5, 0, '2026-01-02T00:00:00.000Z');
             INSERT INTO project_checklist_items (id, task_id, title, sort_order)
             VALUES ('check-2', 'task-a', 'Second', 1), ('check-1', 'task-a', 'First', 0);
             INSERT INTO project_task_dependencies (id, blocking_task_id, blocked_task_id)
             VALUES ('dependency-random', 'task-a', 'task-b');
             INSERT INTO project_task_tag_links (task_id, tag_id) VALUES ('task-a', 'tag-1');
             INSERT INTO project_custom_fields (id, project_id, name, field_type, sort_order)
             VALUES ('field-number', 'project-1', 'Cost', 'number', 0),
                    ('field-select', 'project-1', 'Area', 'multi_select', 1),
                    ('field-text', 'project-1', 'Note', 'text', 2);
             INSERT INTO project_custom_field_options (id, field_id, name, sort_order)
             VALUES ('option-a', 'field-select', 'A', 0), ('option-b', 'field-select', 'B', 1);
             INSERT INTO project_custom_field_values (task_id, field_id, number_value)
             VALUES ('task-a', 'field-number', 12.3456789), ('task-b', 'field-number', -0.0000005);
             INSERT INTO project_custom_field_values (task_id, field_id, text_value)
             VALUES ('task-a', 'field-text', 'hello');
             INSERT INTO project_custom_field_option_values (task_id, field_id, option_id, created_at)
             VALUES ('task-a', 'field-select', 'option-b', '2026-01-01T00:00:00.000Z'),
                    ('task-a', 'field-select', 'option-a', '2026-01-02T00:00:00.000Z');
             INSERT INTO project_views (id, project_id, view_type, sort_order)
             VALUES ('view-extra', 'project-1', 'kanban', 1500);
             INSERT INTO project_view_preferences (view_id, preference_key, preference_value)
             VALUES ('view-extra', 'kanban-grouping', 'status');
             INSERT INTO project_custom_emojis (id, name, asset_path, sort_order)
             VALUES ('emoji-1', 'Rocket', 'project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png', 0);
             INSERT INTO project_reorder_receipts (operation_id, project_id, request_json)
             VALUES ('operation-1', 'project-1', '{}');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let task_count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_tasks")
            .fetch_one(&pool)
            .await
            .unwrap();

        run_migrations(&pool).await.unwrap();

        let groups: Vec<String> =
            sqlx::query_scalar("SELECT id FROM project_groups ORDER BY order_key, id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(groups[groups.len() - 2..], ["group-a", "group-b"]);
        let ordered = |sql: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, String>(sql)
                    .fetch_all(&pool)
                    .await
                    .unwrap()
            }
        };
        assert_eq!(
            ordered("SELECT id FROM projects WHERE group_id = 'group-a' ORDER BY order_key, id")
                .await,
            ["project-2", "project-1"]
        );
        assert_eq!(
            ordered("SELECT id FROM project_sections WHERE project_id = 'project-1' ORDER BY order_key, id")
                .await,
            ["section-1", "section-2"]
        );
        assert_eq!(
            ordered("SELECT id FROM project_statuses WHERE project_id = 'project-1' ORDER BY order_key, id")
                .await,
            ["status-todo", "status-done"]
        );
        assert_eq!(
            ordered(
                "SELECT id FROM project_priorities WHERE project_id = 'project-1' ORDER BY order_key, id"
            )
            .await,
            ["high", "low"]
        );
        assert_eq!(
            ordered("SELECT id FROM project_tasks WHERE section_id = 'section-1' ORDER BY section_order_key, id")
                .await,
            ["task-a", "task-b", "task-c"]
        );
        assert_eq!(
            ordered("SELECT id FROM project_tasks WHERE status_id = 'status-todo' ORDER BY status_order_key, id")
                .await,
            ["task-c", "task-a"]
        );
        assert_eq!(
            ordered("SELECT id FROM project_checklist_items ORDER BY order_key, id").await,
            ["check-1", "check-2"]
        );
        assert_eq!(
            ordered(
                "SELECT id FROM project_custom_field_options WHERE field_id = 'field-select'
                 ORDER BY order_key, id"
            )
            .await,
            ["option-a", "option-b"]
        );
        assert_eq!(
            ordered(
                "SELECT id FROM project_views WHERE project_id = 'project-1' ORDER BY order_key, id"
            )
            .await,
            ["view-extra"]
        );

        let task_count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_tasks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(task_count_after, task_count_before);

        let numbers: Vec<(String, Option<i64>)> = sqlx::query_as(
            "SELECT task_id, number_value FROM project_custom_field_values
             WHERE field_id = 'field-number' ORDER BY task_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            numbers,
            vec![
                ("task-a".to_string(), Some(12_345_679)),
                ("task-b".to_string(), Some(-1))
            ]
        );
        let selected: Vec<(i64, String)> = sqlx::query_as(
            "SELECT options.ordinal, options.option_id
             FROM project_custom_field_option_values AS options
             JOIN project_custom_field_values AS parent ON parent.sync_key = options.value_key
             WHERE parent.task_id = 'task-a' AND parent.field_id = 'field-select'
               AND parent.text_value IS NULL AND parent.number_value IS NULL
               AND parent.date_value IS NULL AND parent.checkbox_value IS NULL
             ORDER BY options.ordinal",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            selected,
            vec![(0, "option-b".to_string()), (1, "option-a".to_string())]
        );

        let dependency: (String, String) =
            sqlx::query_as("SELECT id, sync_key FROM project_task_dependencies")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(dependency.0.starts_with("pending:"));
        assert_eq!(dependency.0, dependency.1);
        let placeholders: Vec<String> = sqlx::query_scalar(
            "SELECT sync_key FROM project_priorities
             UNION ALL SELECT sync_key FROM project_task_tag_links
             UNION ALL SELECT sync_key FROM project_custom_field_values
             UNION ALL SELECT sync_key FROM project_view_preferences",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(placeholders.iter().all(|key| key.starts_with("pending:")));
        let distinct_per_table: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT COUNT(*), COUNT(DISTINCT sync_key) FROM project_priorities
             UNION ALL SELECT COUNT(*), COUNT(DISTINCT sync_key) FROM project_task_tag_links
             UNION ALL SELECT COUNT(*), COUNT(DISTINCT sync_key) FROM project_custom_field_values
             UNION ALL SELECT COUNT(*), COUNT(DISTINCT sync_key) FROM project_view_preferences",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(distinct_per_table.iter().all(|(rows, keys)| rows == keys));

        let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_reorder_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(receipts, 0);
        let foreign_key_errors: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(foreign_key_errors, 0);
        let real_columns: Vec<String> = sqlx::query_scalar(
            "SELECT m.name || '.' || p.name FROM sqlite_schema AS m
             JOIN pragma_table_info(m.name) AS p
             WHERE m.type = 'table' AND m.name LIKE 'project%' AND upper(p.type) = 'REAL'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(real_columns.is_empty(), "{real_columns:?}");
        let sort_columns: Vec<String> = sqlx::query_scalar(
            "SELECT m.name || '.' || p.name FROM sqlite_schema AS m
             JOIN pragma_table_info(m.name) AS p
             WHERE m.type = 'table' AND m.name LIKE 'project%'
               AND p.name LIKE '%sort_order' AND m.name <> 'project_working_folders'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(sort_columns.is_empty(), "{sort_columns:?}");
        let removed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name IN (
                'project_task_revision_after_update',
                'project_custom_field_revision',
                'project_custom_field_option_revision',
                'idx_project_tags_project_name',
                'idx_project_custom_fields_project_name',
                'idx_project_custom_field_options_field_name',
                'ux_project_views_single_hosted'
             )",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(removed, 0);
        let emoji_size: Option<i64> =
            sqlx::query_scalar("SELECT asset_byte_size FROM project_custom_emojis")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(emoji_size, None, "the conversion pass fills existing sizes");
    });
}

#[test]
fn order_keys_migration_refuses_numbers_beyond_the_integer_range() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_ORDER_KEYS).await;
        insert_legacy_project_with_task(&pool).await;
        sqlx::raw_sql(
            "INSERT INTO project_custom_fields (id, project_id, name, field_type)
             VALUES ('field-number', 'project-1', 'Cost', 'number');
             INSERT INTO project_custom_field_values (task_id, field_id, number_value)
             VALUES ('task-1', 'field-number', 1e13);",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(run_migrations(&pool).await.is_err());
        let number: f64 =
            sqlx::query_scalar("SELECT number_value FROM project_custom_field_values")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            number, 1e13,
            "the failed migration leaves the row untouched"
        );
    });
}

async fn insert_project_with_task(pool: &sqlx::SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO project_groups (id, name, icon, order_key) VALUES ('group-1', 'Work', 'lucide:folder', 'a0');
         INSERT INTO projects (id, group_id, name, icon, order_key)
         VALUES ('project-1', 'group-1', 'Launch', 'lucide:folder', 'a0');
         INSERT INTO project_sections (id, project_id, name, order_key)
         VALUES ('section-1', 'project-1', 'Now', 'a0');
         INSERT INTO project_statuses (id, project_id, name, category, order_key)
         VALUES ('status-1', 'project-1', 'To do', 'not_started', 'a0');
         INSERT INTO project_tasks (id, project_id, section_id, status_id, title, section_order_key, status_order_key)
         VALUES ('task-1', 'project-1', 'section-1', 'status-1', 'Plan', 'a0', 'a0');",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_legacy_project_with_task(pool: &sqlx::SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO project_groups (id, name, icon) VALUES ('group-1', 'Work', 'lucide:folder');
         INSERT INTO projects (id, group_id, name, icon) VALUES ('project-1', 'group-1', 'Launch', 'lucide:folder');
         INSERT INTO project_sections (id, project_id, name) VALUES ('section-1', 'project-1', 'Now');
         INSERT INTO project_statuses (id, project_id, name, category)
         VALUES ('status-1', 'project-1', 'To do', 'not_started');
         INSERT INTO project_tasks (id, project_id, section_id, status_id, title)
         VALUES ('task-1', 'project-1', 'section-1', 'status-1', 'Plan');",
    )
    .execute(pool)
    .await
    .unwrap();
}
