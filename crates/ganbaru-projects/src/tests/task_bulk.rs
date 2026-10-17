use super::*;
use crate::task_bulk::{TaskBulkChange, TaskBulkRequest, TaskFieldExpectation, apply_task_bulk};

fn archive_request() -> TaskBulkRequest {
    TaskBulkRequest {
        operation_id: "archive-operation".to_string(),
        project_id: "project-a".to_string(),
        tasks: vec![TaskFieldExpectation {
            id: "task-a".to_string(),
            value: None,
        }],
        change: TaskBulkChange::Archive { archived: true },
    }
}

#[test]
fn archive_and_restore_include_descendants_absent_from_the_selection() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query("UPDATE project_tasks SET parent_task_id = 'task-a' WHERE id = 'task-b'")
            .execute(&pool)
            .await
            .unwrap();
        let mut request = archive_request();
        let mutation = apply_task_bulk(&pool, &request).await.unwrap();
        assert_eq!(mutation.tasks.len(), 2);
        assert!(mutation.tasks.iter().all(|task| task.archived_at.is_some()));
        assert_eq!(mutation.task_change_events.len(), 2);
        let archived_at = mutation.tasks[0].archived_at.clone();
        request.operation_id = "restore-operation".to_string();
        request.change = TaskBulkChange::Archive { archived: false };
        request.tasks[0].value = archived_at;
        let restored = apply_task_bulk(&pool, &request).await.unwrap();
        assert_eq!(restored.tasks.len(), 2);
        assert!(restored.tasks.iter().all(|task| task.archived_at.is_none()));
        let unrelated: Option<String> =
            sqlx::query_scalar("SELECT archived_at FROM project_tasks WHERE id = 'task-c'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(unrelated.is_none());
    });
}

#[test]
fn a_late_descendant_failure_rolls_back_rows_history_and_receipt() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::raw_sql(
            "UPDATE project_tasks SET parent_task_id = 'task-a' WHERE id = 'task-b';
             CREATE TRIGGER fail_second_task BEFORE UPDATE ON project_tasks
             WHEN NEW.id = 'task-b' BEGIN SELECT RAISE(ABORT, 'injected second write failure'); END;",
        ).execute(&pool).await.unwrap();
        let error = apply_task_bulk(&pool, &archive_request())
            .await
            .err()
            .unwrap();
        assert!(error.contains("injected second write failure"));
        let state: (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM project_tasks WHERE archived_at IS NOT NULL),
             (SELECT COUNT(*) FROM project_task_change_events),
             (SELECT COUNT(*) FROM project_task_bulk_receipts)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, (0, 0, 0));
        sqlx::query("DROP TRIGGER fail_second_task")
            .execute(&pool)
            .await
            .unwrap();
        assert!(apply_task_bulk(&pool, &archive_request()).await.is_ok());
    });
}

#[test]
fn retry_recovers_the_original_result_and_rejects_reused_identity() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let mut request = archive_request();
        let first = apply_task_bulk(&pool, &request).await.unwrap();
        sqlx::query("UPDATE project_tasks SET title = 'Later edit' WHERE id = 'task-a'")
            .execute(&pool)
            .await
            .unwrap();
        let replay = apply_task_bulk(&pool, &request).await.unwrap();
        assert_eq!(
            serde_json::to_value(first).unwrap(),
            serde_json::to_value(replay).unwrap()
        );
        let history_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_task_change_events")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(history_count, 1);
        request.change = TaskBulkChange::Archive { archived: false };
        assert!(
            apply_task_bulk(&pool, &request)
                .await
                .err()
                .unwrap()
                .contains("different intent")
        );
    });
}

#[test]
fn priority_intent_preserves_concurrent_unrelated_fields_and_rejects_stale_priority() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query("UPDATE project_tasks SET title = 'New title', due_date = '2026-10-10' WHERE id = 'task-a'")
            .execute(&pool).await.unwrap();
        let mut request = archive_request();
        request.tasks[0].value = Some("normal".to_string());
        request.change = TaskBulkChange::Priority {
            priority: "high".to_string(),
        };
        let mutation = apply_task_bulk(&pool, &request).await.unwrap();
        assert_eq!(mutation.tasks[0].title, "New title");
        assert_eq!(mutation.tasks[0].due_date.as_deref(), Some("2026-10-10"));
        assert_eq!(mutation.tasks[0].priority, "high");
        request.operation_id = "stale-priority".to_string();
        assert!(
            apply_task_bulk(&pool, &request)
                .await
                .err()
                .unwrap()
                .contains("changed since selection")
        );
    });
}

#[test]
fn missing_duplicate_and_cross_project_selections_never_partially_apply() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let mut request = archive_request();
        request.tasks.push(TaskFieldExpectation {
            id: "missing".to_string(),
            value: None,
        });
        assert!(
            apply_task_bulk(&pool, &request)
                .await
                .err()
                .unwrap()
                .contains("no longer exists")
        );
        request.tasks[1].id = "task-a".to_string();
        assert!(
            apply_task_bulk(&pool, &request)
                .await
                .err()
                .unwrap()
                .contains("duplicate")
        );
        sqlx::raw_sql(
            "INSERT INTO projects (id, group_id, name, icon) VALUES ('project-b', 'group-a', 'Other', 'lucide:folder');
             INSERT INTO project_sections (id, project_id, name) VALUES ('section-b', 'project-b', 'Other');
             INSERT INTO project_statuses (id, project_id, name, category) VALUES ('status-b', 'project-b', 'Other', 'not_started');
             INSERT INTO project_tasks (id, project_id, section_id, status_id, title)
             VALUES ('foreign-task', 'project-b', 'section-b', 'status-b', 'Foreign');",
        ).execute(&pool).await.unwrap();
        request.tasks[1].id = "foreign-task".to_string();
        assert!(
            apply_task_bulk(&pool, &request)
                .await
                .err()
                .unwrap()
                .contains("selected project")
        );
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_tasks WHERE archived_at IS NOT NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
    });
}

#[test]
fn status_changes_assign_native_order_and_preserve_completion_on_noop() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query("INSERT INTO project_statuses (id, project_id, name, category, terminal) VALUES ('done', 'project-a', 'Done', 'done', 1)")
            .execute(&pool).await.unwrap();
        let mut request = archive_request();
        request.tasks[0].value = Some("status-a".to_string());
        request.tasks.push(TaskFieldExpectation {
            id: "task-b".to_string(),
            value: Some("status-a".to_string()),
        });
        request.change = TaskBulkChange::Status {
            status_id: "done".to_string(),
        };
        let result = apply_task_bulk(&pool, &request).await.unwrap();
        assert!(result.tasks.iter().all(|task| task.completed_at.is_some()));
        assert!(result.tasks[0].status_sort_order < result.tasks[1].status_sort_order);
        request.operation_id = "noop-completion".to_string();
        for task in &mut request.tasks {
            task.value = Some("done".to_string());
        }
        let repeated = apply_task_bulk(&pool, &request).await.unwrap();
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::to_value(&repeated).unwrap()
        );
    });
}

#[test]
fn a_committed_receipt_survives_reopening_the_database() {
    block_on(async {
        let directory = std::env::temp_dir().join(format!(
            "ganbaru-task-bulk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir(&directory).unwrap();
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.join("projects.sqlite"))
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        insert_project_graph_fixture(&pool).await;
        let request = archive_request();
        let first = apply_task_bulk(&pool, &request).await.unwrap();
        pool.close().await;
        let reopened = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        let retry = apply_task_bulk(&reopened, &request).await.unwrap();
        assert_eq!(
            serde_json::to_value(first).unwrap(),
            serde_json::to_value(retry).unwrap()
        );
        reopened.close().await;
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn restoring_a_child_requires_its_archived_parent() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query("UPDATE project_tasks SET parent_task_id = 'task-a' WHERE id = 'task-b'")
            .execute(&pool)
            .await
            .unwrap();
        let archived = apply_task_bulk(&pool, &archive_request()).await.unwrap();
        let request = TaskBulkRequest {
            operation_id: "restore-child".to_string(),
            project_id: "project-a".to_string(),
            tasks: vec![TaskFieldExpectation {
                id: "task-b".to_string(),
                value: archived.tasks[1].archived_at.clone(),
            }],
            change: TaskBulkChange::Archive { archived: false },
        };
        assert!(
            apply_task_bulk(&pool, &request)
                .await
                .err()
                .unwrap()
                .contains("archived parent")
        );
    });
}
