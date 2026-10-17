use super::*;
use crate::mutations::{ensure_dependency_has_no_cycle, tag_for_task_tag_link};
use crate::relationship_commands::link_task_event_with_project_assignment;

#[test]
fn dependency_cycle_detection_rejects_reverse_chain() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_task_dependencies
                (id, blocking_task_id, blocked_task_id, dependency_type)
             VALUES ('dependency-a-b', 'task-a', 'task-b', 'blocks')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result = ensure_dependency_has_no_cycle(&mut tx, "task-b", "task-a").await;

        assert_eq!(result, Err("dependency would create a cycle".to_string()));
    });
}

#[test]
fn dependency_cycle_detection_allows_forward_chain() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_task_dependencies
                (id, blocking_task_id, blocked_task_id, dependency_type)
             VALUES ('dependency-a-b', 'task-a', 'task-b', 'blocks')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result = ensure_dependency_has_no_cycle(&mut tx, "task-c", "task-a").await;

        assert_eq!(result, Ok(()));
    });
}

#[test]
fn task_tag_link_rejects_tags_from_another_project() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, sort_order)
             VALUES ('project-b', 'group-a', 'Project B', 'lucide:folder', 200)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_sections (id, project_id, name, sort_order)
             VALUES ('section-b', 'project-b', 'General', 100)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name, category, sort_order, terminal)
             VALUES ('status-b', 'project-b', 'To do', 'not_started', 100, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_tasks (id, project_id, section_id, status_id, title)
             VALUES ('task-other', 'project-b', 'section-b', 'status-b', 'Other task')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_tags (id, project_id, name, sort_order)
             VALUES ('tag-a', 'project-a', 'Backend', 100)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result = tag_for_task_tag_link(&mut tx, "task-other", "tag-a").await;

        assert_eq!(
            result,
            Err("tag must belong to the task project".to_string())
        );
    });
}

#[test]
fn task_event_link_assigns_missing_event_project() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO calendar_events (id, title, start_time, end_time)
             VALUES ('event-a', 'Focus block', '2026-06-12T15:00:00Z', '2026-06-12T16:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let link = ProjectTaskEventLinkCreate {
            task_id: "task-a".to_string(),
            event_id: "event-a".to_string(),
            link_kind: "scheduled".to_string(),
        };

        let mut tx = pool.begin().await.unwrap();
        link_task_event_with_project_assignment(&mut tx, &link)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let event_project_id: Option<String> =
            sqlx::query_scalar("SELECT project_id FROM calendar_events WHERE id = 'event-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let link_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM project_task_event_links
             WHERE task_id = 'task-a' AND event_id = 'event-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let history_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM project_task_change_events
             WHERE task_id = 'task-a'
               AND event_type = 'scheduled'
               AND field_name = 'event_id'
               AND new_value = 'event-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(event_project_id.as_deref(), Some("project-a"));
        assert_eq!(link_count, 1);
        assert_eq!(history_count, 1);
    });
}

#[test]
fn task_event_link_rejects_event_from_another_project() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, sort_order)
             VALUES ('project-b', 'group-a', 'Project B', 'lucide:folder', 200)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO calendar_events (id, title, start_time, end_time, project_id)
             VALUES (
                'event-b',
                'Other focus block',
                '2026-06-12T15:00:00Z',
                '2026-06-12T16:00:00Z',
                'project-b'
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let link = ProjectTaskEventLinkCreate {
            task_id: "task-a".to_string(),
            event_id: "event-b".to_string(),
            link_kind: "scheduled".to_string(),
        };

        let mut tx = pool.begin().await.unwrap();
        let result = link_task_event_with_project_assignment(&mut tx, &link).await;

        assert_eq!(
            result,
            Err("calendar event does not belong to the task project".to_string())
        );
    });
}
