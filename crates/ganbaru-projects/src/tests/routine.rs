use super::*;
use crate::structure_commands::delete_project_group;

#[test]
fn built_in_routine_defaults_are_protected_and_repaired() {
    block_on(async {
        let pool = migrated_memory_pool().await;

        assert_eq!(
            delete_project_group(&pool, ROUTINE_GROUP_ID).await,
            Err("built-in Routine group cannot be deleted".to_string()),
        );

        sqlx::query(
            "UPDATE projects
             SET name = 'Renamed', group_id = 'group-routine', sort_order = 999,
                 icon = 'utensils', status = 'hidden'
             WHERE id = 'project-routine-eat'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM projects WHERE id = 'project-routine-reading'")
            .execute(&pool)
            .await
            .unwrap();

        ensure_built_in_routine_defaults(&pool).await.unwrap();

        let eating: (String, String, i64, String, String) = sqlx::query_as(
            "SELECT group_id, name, sort_order, icon, status
             FROM projects WHERE id = 'project-routine-eat'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            eating,
            (
                ROUTINE_GROUP_ID.to_string(),
                "Eating".to_string(),
                40,
                "utensils".to_string(),
                "hidden".to_string(),
            ),
        );
        let restored_reading: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM projects WHERE id = 'project-routine-reading'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(restored_reading, 1);

        sqlx::query("DELETE FROM project_groups WHERE id = 'group-routine'")
            .execute(&pool)
            .await
            .unwrap();
        ensure_built_in_routine_defaults(&pool).await.unwrap();

        let restored_projects: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE group_id = 'group-routine'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(restored_projects, BUILT_IN_ROUTINE_PROJECTS.len() as i64);
        let restored_working_folders: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM project_working_folders AS folder
             INNER JOIN projects AS project ON project.id = folder.project_id
             WHERE project.group_id = 'group-routine'
               AND folder.kind = 'managed'
               AND folder.archived_at IS NULL
               AND folder.managed_relative_path = ('projects/' || project.id)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            restored_working_folders,
            BUILT_IN_ROUTINE_PROJECTS.len() as i64
        );
        let reordered_defaults: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT id, icon, sort_order
             FROM projects
             WHERE id IN (
                'project-routine-exercise',
                'project-routine-hygiene',
                'project-routine-eat',
                'project-routine-commute',
                'project-routine-chores',
                'project-routine-health',
                'project-routine-sleep'
             )
             ORDER BY sort_order ASC",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            reordered_defaults,
            vec![
                (
                    "project-routine-exercise".to_string(),
                    "lucide:sport-shoe".to_string(),
                    20,
                ),
                (
                    "project-routine-hygiene".to_string(),
                    "lucide:bath".to_string(),
                    30,
                ),
                (
                    "project-routine-eat".to_string(),
                    "lucide:apple".to_string(),
                    40,
                ),
                (
                    "project-routine-commute".to_string(),
                    "lucide:bike".to_string(),
                    50,
                ),
                (
                    "project-routine-chores".to_string(),
                    "lucide:shopping-cart".to_string(),
                    70,
                ),
                (
                    "project-routine-health".to_string(),
                    "lucide:pill".to_string(),
                    100,
                ),
                (
                    "project-routine-sleep".to_string(),
                    "lucide:bed".to_string(),
                    110,
                ),
            ],
        );
    });
}
