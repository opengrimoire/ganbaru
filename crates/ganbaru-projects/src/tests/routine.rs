use super::*;
use crate::structure_commands::delete_project_group;
use crate::workspace::{load_workspace, refresh_workspace};

#[test]
fn built_in_routine_group_is_protected_and_loads_never_repair_routine_rows() {
    block_on(async {
        let pool = migrated_memory_pool().await;

        assert_eq!(
            delete_project_group(&pool, ROUTINE_GROUP_ID).await,
            Err("built-in Routine group cannot be deleted".to_string()),
        );
        let seeded: Vec<String> =
            sqlx::query_scalar("SELECT id FROM projects WHERE group_id = ? ORDER BY order_key, id")
                .bind(ROUTINE_GROUP_ID)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(seeded.len(), BUILT_IN_ROUTINE_PROJECTS.len());

        sqlx::query(
            "UPDATE projects SET name = 'Renamed', icon = 'utensils' WHERE id = 'project-routine-eat'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM projects WHERE id = 'project-routine-reading'")
            .execute(&pool)
            .await
            .unwrap();
        let before: Vec<(String, String, String, String)> =
            sqlx::query_as("SELECT id, name, icon, order_key FROM projects ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();

        load_workspace(&pool, None).await.unwrap();
        refresh_workspace(&pool, None).await.unwrap();

        let after: Vec<(String, String, String, String)> =
            sqlx::query_as("SELECT id, name, icon, order_key FROM projects ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(after, before);
        assert!(!after.iter().any(|row| row.0 == "project-routine-reading"));
    });
}
