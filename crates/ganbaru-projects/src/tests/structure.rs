use super::*;
use crate::mutations::{delete_tag_with_history, delete_unused_status};
use crate::structure_commands::delete_project_group;

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
