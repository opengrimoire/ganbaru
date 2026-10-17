use super::helpers::migrated_memory_pool;

#[test]
fn schema_creates_project_custom_emojis() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        sqlx::query(
            "INSERT INTO project_custom_emojis (id, name, asset_path, sort_order)
             VALUES ('emoji-1', 'Rocket',
                     'project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png',
                     1000)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let asset_path: String =
            sqlx::query_scalar("SELECT asset_path FROM project_custom_emojis WHERE id = 'emoji-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            asset_path,
            "project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
        );

        for (id, name, asset_path, sort_order) in [
            (
                "bad-name",
                "",
                "project-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                0,
            ),
            (
                "bad-prefix",
                "Bad",
                "other/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                0,
            ),
            (
                "bad-parent",
                "Bad",
                "project-icons/../bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                0,
            ),
            (
                "bad-nested",
                "Bad",
                "project-icons/nested/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                0,
            ),
            (
                "bad-order",
                "Bad",
                "project-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                -1,
            ),
        ] {
            assert!(
                sqlx::query(
                    "INSERT INTO project_custom_emojis (id, name, asset_path, sort_order)
                     VALUES (?, ?, ?, ?)",
                )
                .bind(id)
                .bind(name)
                .bind(asset_path)
                .bind(sort_order)
                .execute(&pool)
                .await
                .is_err(),
                "{id} should fail"
            );
        }
    });
}

#[test]
fn project_views_keep_one_notes_and_chat_view_and_cascade_their_preferences() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, sort_order) VALUES ('group-1', 'Work', 'lucide:folder', 0);
             INSERT INTO projects (id, group_id, name, icon, sort_order)
             VALUES ('project-1', 'group-1', 'Launch', 'lucide:folder', 0);
             INSERT INTO project_views (id, project_id, view_type, sort_order) VALUES
                ('view-list-a', 'project-1', 'list', 1000),
                ('view-list-b', 'project-1', 'list', 2000),
                ('view-notes', 'project-1', 'notes', 3000);
             INSERT INTO project_view_preferences (view_id, preference_key, preference_value)
             VALUES ('view-list-b', 'list-presentation', '{}');",
        )
        .execute(&pool)
        .await
        .unwrap();

        let duplicate_notes = sqlx::query(
            "INSERT INTO project_views (id, project_id, view_type, sort_order)
             VALUES ('view-notes-2', 'project-1', 'notes', 4000)",
        )
        .execute(&pool)
        .await;
        assert!(duplicate_notes.is_err(), "a project keeps one Notes view");

        let blank_name = sqlx::query(
            "INSERT INTO project_views (id, project_id, view_type, name, sort_order)
             VALUES ('view-blank', 'project-1', 'kanban', '  ', 4000)",
        )
        .execute(&pool)
        .await;
        assert!(blank_name.is_err(), "a renamed view needs a visible name");

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
fn migration_gives_seeded_projects_the_default_views() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let rows: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT id, view_type, sort_order FROM project_views
             WHERE project_id = 'project-routine-learning'
             ORDER BY sort_order",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            [
                ("view-project-routine-learning-dashboard", "dashboard", 1000),
                ("view-project-routine-learning-list", "list", 2000),
                ("view-project-routine-learning-notes", "notes", 3000),
                ("view-project-routine-learning-chat", "chat", 4000),
            ]
            .map(|(id, view_type, order)| (
                id.to_string(),
                view_type.to_string(),
                order
            )),
        );
    });
}
