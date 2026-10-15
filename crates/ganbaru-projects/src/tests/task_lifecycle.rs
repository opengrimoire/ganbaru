use super::*;
use crate::history::{insert_task_change_event, insert_task_update_change_events};
use crate::mutations::{next_task_sort_order, task_mutation};

#[test]
fn next_task_sort_order_handles_empty_project() {
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
            "INSERT INTO project_sections (id, project_id, name, sort_order)
             VALUES ('section-empty', 'project-empty', 'General', 100)",
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
        let section_order = next_task_sort_order(
            &mut tx,
            "section_id",
            "project-empty",
            "section-empty",
            "section_sort_order",
        )
        .await;
        let status_order = next_task_sort_order(
            &mut tx,
            "status_id",
            "project-empty",
            "status-empty",
            "status_sort_order",
        )
        .await;

        assert_eq!(section_order, Ok(1000.0));
        assert_eq!(status_order, Ok(1000.0));
    });
}

#[test]
fn task_update_records_trimmed_change_reason() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let previous =
            sqlx::query_as::<_, ProjectTaskRow>("SELECT * FROM project_tasks WHERE id = 'task-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let mut next = task_update_from_row(&previous);
        next.estimate_minutes = Some(45);
        next.change_reason = Some("  Scope changed  ".to_string());

        let mut tx = pool.begin().await.unwrap();
        insert_task_update_change_events(&mut tx, &previous, &next)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let reason: Option<String> = sqlx::query_scalar(
            "SELECT reason
             FROM project_task_change_events
             WHERE task_id = 'task-a'
               AND event_type = 'updated'
               AND field_name = 'estimate'
               AND old_value IS NULL
               AND new_value = '45'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(reason.as_deref(), Some("Scope changed"));
    });
}

#[test]
fn task_update_rejects_oversized_change_reason() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let previous =
            sqlx::query_as::<_, ProjectTaskRow>("SELECT * FROM project_tasks WHERE id = 'task-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let mut task = task_update_from_row(&previous);
        task.change_reason = Some("x".repeat(MAX_TASK_CHANGE_REASON_LENGTH + 1));

        assert_eq!(
            validate_task_update(&task),
            Err("change_reason is too long".to_string())
        );
    });
}

#[test]
fn task_update_validates_task_times() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let previous =
            sqlx::query_as::<_, ProjectTaskRow>("SELECT * FROM project_tasks WHERE id = 'task-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let mut task = task_update_from_row(&previous);

        task.start_time = Some("09:30".to_string());
        assert_eq!(
            validate_task_update(&task),
            Err("start_time requires start_date".to_string())
        );

        task.start_date = Some("2026-06-21".to_string());
        assert_eq!(validate_task_update(&task), Ok(()));

        task.due_date = Some("2026-06-22".to_string());
        task.due_time = Some("24:00".to_string());
        assert_eq!(
            validate_task_update(&task),
            Err("due_time must use HH:MM".to_string())
        );
    });
}

#[test]
fn task_mutation_returns_the_authoritative_row_and_committed_history() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(
            "UPDATE project_tasks
             SET title = 'Authoritative', updated_at = '2026-07-11T12:00:00.000Z'
             WHERE id = 'task-a'",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        insert_task_change_event(
            &mut tx,
            "task-a",
            "updated",
            Some("title"),
            Some("task-a"),
            Some("Authoritative"),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let mutation = task_mutation(&pool, "task-a").await.unwrap();

        assert_eq!(mutation.tasks.len(), 1);
        assert_eq!(mutation.tasks[0].title, "Authoritative");
        assert_eq!(mutation.tasks[0].updated_at, "2026-07-11T12:00:00.000Z");
        assert!(mutation.task_change_events.iter().any(|event| {
            event.task_id == "task-a"
                && event.field_name.as_deref() == Some("title")
                && event.new_value.as_deref() == Some("Authoritative")
        }));
    });
}

#[test]
fn task_update_refuses_a_stale_expected_revision_without_writing() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let load = || async {
            sqlx::query_as::<_, ProjectTaskRow>("SELECT * FROM project_tasks WHERE id = 'task-a'")
                .fetch_one(&pool)
                .await
                .unwrap()
        };
        let base = load().await;
        let mut first = task_update_from_row(&base);
        first.title = "Edited in one window".to_string();
        first.expected_revision = Some(base.revision);
        task_commands::update_task(&pool, first).await.unwrap();

        let mut stale = task_update_from_row(&base);
        stale.description = "Edited in another window".to_string();
        stale.expected_revision = Some(base.revision);
        let error = task_commands::update_task(&pool, stale)
            .await
            .err()
            .unwrap();
        assert!(
            error.starts_with(task_commands::TASK_CONFLICT_PREFIX),
            "{error}"
        );

        let current = load().await;
        assert_eq!(current.title, "Edited in one window");
        assert_eq!(current.description, base.description);
        assert!(current.revision > base.revision);

        let mut fresh = task_update_from_row(&current);
        fresh.description = "Reapplied".to_string();
        fresh.expected_revision = Some(current.revision);
        task_commands::update_task(&pool, fresh).await.unwrap();
        assert_eq!(load().await.description, "Reapplied");
    });
}
