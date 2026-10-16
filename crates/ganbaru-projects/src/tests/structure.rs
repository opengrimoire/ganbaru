use super::*;
use crate::mutations::{delete_tag_with_history, delete_unused_status};
use crate::structure_commands::{delete_project_group, reorder_group_projects, reorder_groups};

#[test]
fn delete_group_cascades_projects_and_keeps_calendar_events() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO calendar_events (id, title, start_time, end_time, project_id)
             VALUES ('event-a', 'Event A', '2026-06-24T10:00:00Z', '2026-06-24T11:00:00Z', 'project-a')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_task_event_links (task_id, event_id)
             VALUES ('task-a', 'event-a')",
        )
        .execute(&pool)
        .await
        .unwrap();

        delete_project_group(&pool, " group-a ").await.unwrap();

        let group_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_groups WHERE id = 'group-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let project_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = 'project-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let task_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_tasks WHERE project_id = 'project-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let link_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_task_event_links WHERE event_id = 'event-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let event_project_id: Option<String> =
            sqlx::query_scalar("SELECT project_id FROM calendar_events WHERE id = 'event-a'")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(group_count, 0);
        assert_eq!(project_count, 0);
        assert_eq!(task_count, 0);
        assert_eq!(link_count, 0);
        assert_eq!(event_project_id, None);
    });
}

#[test]
fn delete_status_removes_empty_status() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name, category, sort_order, terminal)
             VALUES ('status-empty', 'project-a', 'Later', 'active', 200, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        delete_unused_status(&mut tx, "status-empty").await.unwrap();
        tx.commit().await.unwrap();

        let status_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_statuses WHERE id = 'status-empty'")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(status_count, 0);
    });
}

#[test]
fn delete_status_rejects_status_with_tasks() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name, category, sort_order, terminal)
             VALUES ('status-empty', 'project-a', 'Later', 'active', 200, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result = delete_unused_status(&mut tx, "status-a").await;

        assert_eq!(
            result,
            Err("move or delete tasks before deleting this task status".to_string())
        );
    });
}

#[test]
fn delete_status_rejects_last_project_status() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, sort_order)
             VALUES ('group-empty', 'Group Empty', 'lucide:folder', 100)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, sort_order)
             VALUES ('project-empty', 'group-empty', 'Project Empty', 'lucide:folder', 100)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name, category, sort_order, terminal)
             VALUES ('status-empty', 'project-empty', 'To do', 'not_started', 100, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result = delete_unused_status(&mut tx, "status-empty").await;

        assert_eq!(
            result,
            Err("project must keep at least one task status".to_string())
        );
    });
}

#[test]
fn delete_tag_removes_links_and_records_task_history() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_tags (id, project_id, name, sort_order)
             VALUES ('tag-a', 'project-a', 'Backend', 100)",
        )
        .execute(&pool)
        .await
        .unwrap();
        for task_id in ["task-a", "task-b"] {
            sqlx::query(
                "INSERT INTO project_task_tag_links (task_id, tag_id)
                 VALUES (?, 'tag-a')",
            )
            .bind(task_id)
            .execute(&pool)
            .await
            .unwrap();
        }

        let mut tx = pool.begin().await.unwrap();
        delete_tag_with_history(&mut tx, "tag-a").await.unwrap();
        tx.commit().await.unwrap();

        let tag_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_tags WHERE id = 'tag-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let link_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_task_tag_links WHERE tag_id = 'tag-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let history_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM project_task_change_events
             WHERE field_name = 'tags'
               AND old_value = 'Backend'
               AND new_value IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(tag_count, 0);
        assert_eq!(link_count, 0);
        assert_eq!(history_count, 2);
    });
}

#[test]
fn reordering_groups_requires_every_group_and_keeps_modified_times() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, sort_order, updated_at)
             VALUES ('group-b', 'Group B', 'lucide:folder', 200, '2026-01-01T00:00:00.000Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut group_ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM project_groups ORDER BY sort_order, name")
                .fetch_all(&pool)
                .await
                .unwrap();
        let reorder = |ids: &[String]| ProjectGroupReorder {
            group_ids: ids.to_vec(),
        };

        let duplicated = [group_ids.clone(), vec!["group-a".to_string()]].concat();
        let unknown = [group_ids.clone(), vec!["other".to_string()]].concat();
        for incomplete in [&group_ids[1..], &duplicated[..], &unknown[..]] {
            assert!(reorder_groups(&pool, reorder(incomplete)).await.is_err());
        }

        group_ids.reverse();
        group_ids[0] = format!(" {} ", group_ids[0]);
        let mutation = reorder_groups(&pool, reorder(&group_ids)).await.unwrap();
        let returned: Vec<&str> = mutation
            .groups
            .iter()
            .map(|group| group.id.as_str())
            .collect();
        let expected: Vec<&str> = group_ids.iter().map(|id| id.trim()).collect();
        assert_eq!(returned, expected);
        assert_eq!(mutation.groups[0].sort_order, 1_000);
        let updated_at: String =
            sqlx::query_scalar("SELECT updated_at FROM project_groups WHERE id = 'group-b'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(updated_at, "2026-01-01T00:00:00.000Z");
    });
}

#[test]
fn reordering_group_projects_covers_inactive_projects_of_that_group_only() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        for (id, status) in [("project-b", "hidden"), ("project-c", "archived")] {
            sqlx::query(
                "INSERT INTO projects (id, group_id, name, icon, sort_order, status, updated_at)
                 VALUES (?, 'group-a', ?, 'lucide:folder', 100, ?, '2026-01-01T00:00:00.000Z')",
            )
            .bind(id)
            .bind(id)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
        }
        let reorder = |group_id: &str, ids: &[&str]| GroupProjectReorder {
            group_id: group_id.to_string(),
            project_ids: ids.iter().map(|id| id.to_string()).collect(),
        };

        assert!(
            reorder_group_projects(&pool, reorder("group-a", &["project-c", "project-a"]))
                .await
                .is_err()
        );
        assert!(
            reorder_group_projects(&pool, reorder("missing", &[]))
                .await
                .is_err()
        );
        assert!(
            reorder_group_projects(&pool, reorder(" ", &[]))
                .await
                .is_err()
        );

        let mutation = reorder_group_projects(
            &pool,
            reorder("group-a", &["project-c", "project-a", "project-b"]),
        )
        .await
        .unwrap();
        let returned: Vec<(&str, i64)> = mutation
            .projects
            .iter()
            .map(|project| (project.id.as_str(), project.sort_order))
            .collect();
        assert_eq!(
            returned,
            [
                ("project-c", 1_000),
                ("project-a", 2_000),
                ("project-b", 3_000)
            ]
        );
        let updated_at: String =
            sqlx::query_scalar("SELECT updated_at FROM projects WHERE id = 'project-c'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(updated_at, "2026-01-01T00:00:00.000Z");
    });
}
