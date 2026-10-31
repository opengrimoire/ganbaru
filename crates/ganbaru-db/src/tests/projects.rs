use super::helpers::migrated_memory_pool;

const ICON_FILE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png";

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
fn project_tables_store_order_keys_and_exact_numbers() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
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

async fn chat_row_count(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM chat_channels)
              + (SELECT count(*) FROM chat_execution_environments)",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn primary_row(pool: &sqlx::SqlitePool, project_id: &str) -> Option<(String, i64, String)> {
    sqlx::query_as(
        "SELECT working_folder_id, revision, created_at
         FROM project_primary_working_folders WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[test]
fn primary_working_folders_only_reference_projects_and_create_no_chat_rows() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, order_key) VALUES ('group-folders', 'Folders', 'a0');
             INSERT INTO projects (id, group_id, name, order_key)
             VALUES ('project-folders', 'group-folders', 'Folders', 'a0');
             INSERT INTO project_working_folders
                 (id, project_id, display_name, kind, managed_relative_path)
             VALUES ('working-folder:project-folders', 'project-folders', 'Folders', 'managed',
                     'projects/project-folders');
             INSERT INTO project_primary_working_folders
                 (project_id, working_folder_id, created_at, updated_at)
             VALUES ('project-folders', 'working-folder:project-folders',
                     '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(primary_row(&pool, "project-folders").await.is_some());

        let triggers: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name IN (
                'chat_channels_create_project_general',
                'project_working_folders_create_chat_environment',
                'project_primary_working_folder_prevent_archive',
                'project_primary_working_folder_active_insert',
                'project_primary_working_folder_active_update'
             )",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(triggers, 0);
        let parents: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT \"table\" FROM pragma_foreign_key_list('project_primary_working_folders')",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(parents, vec!["projects".to_string()]);

        let chat_rows_before = chat_row_count(&pool).await;
        sqlx::raw_sql(
            "INSERT INTO projects (id, group_id, name, order_key)
             VALUES ('project-replicated', 'group-folders', 'Replicated', 'a1');
             INSERT INTO project_primary_working_folders
                 (project_id, working_folder_id, created_at, updated_at)
             VALUES ('project-replicated', 'working-folder:not-yet-arrived',
                     '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
             INSERT INTO project_working_folders
                 (id, project_id, display_name, kind, managed_relative_path)
             VALUES ('working-folder:project-replicated', 'project-replicated', 'Replicated',
                     'managed', 'projects/project-replicated');
             INSERT INTO project_working_folders (id, project_id, display_name, kind)
             VALUES ('working-folder:external', 'project-folders', 'External', 'external');
             UPDATE project_primary_working_folders
             SET working_folder_id = 'working-folder:external'
             WHERE project_id = 'project-folders';
             UPDATE project_working_folders
             SET archived_at = '2026-10-23T00:00:00.000Z'
             WHERE id = 'working-folder:external';",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(chat_row_count(&pool).await, chat_rows_before);

        sqlx::query("DELETE FROM projects WHERE id = 'project-replicated'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(primary_row(&pool, "project-replicated").await, None);
    });
}
