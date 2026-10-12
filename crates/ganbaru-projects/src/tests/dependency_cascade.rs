use super::*;
use crate::dependency_cascade::{
    DependencyCascadeApply, apply_dependency_cascade, preview_dependency_cascade,
};

async fn dated_graph(pool: &SqlitePool) {
    insert_project_graph_fixture(pool).await;
    sqlx::raw_sql("UPDATE project_tasks SET start_date = '2026-06-10', target_end_date = '2026-06-12' WHERE id = 'task-a';
        UPDATE project_tasks SET start_date = '2026-06-11', target_end_date = '2026-06-13' WHERE id = 'task-b';
        UPDATE project_tasks SET start_date = '2026-06-14', target_end_date = '2026-06-15' WHERE id = 'task-c';
        INSERT INTO project_task_dependencies(id, blocking_task_id, blocked_task_id) VALUES ('edge-a', 'task-a', 'task-b'), ('edge-b', 'task-b', 'task-c');")
        .execute(pool).await.unwrap();
}

async fn request(pool: &SqlitePool) -> DependencyCascadeApply {
    DependencyCascadeApply {
        operation_id: "cascade-operation".into(),
        project_id: "project-a".into(),
        reviewed_digest: preview_dependency_cascade(pool, "project-a")
            .await
            .unwrap()
            .digest,
    }
}

async fn dates(pool: &SqlitePool) -> Vec<(String, Option<String>, Option<String>, i64)> {
    sqlx::query_as(
        "SELECT id, start_date, target_end_date, revision FROM project_tasks ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[test]
fn dependency_cascade_reads_hidden_endpoints_and_commits_the_complete_chain() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        dated_graph(&pool).await;
        // No rendered tasks or view filters are supplied to the native preview.
        let preview = preview_dependency_cascade(&pool, "project-a")
            .await
            .unwrap();
        assert!(preview.conflicts.is_empty());
        assert_eq!(preview.items.len(), 2);
        assert_eq!(preview.items[0].task_id, "task-b");
        assert_eq!(preview.items[0].next_range_start, "2026-06-13");
        assert_eq!(preview.items[0].next_range_end, "2026-06-15");
        assert_eq!(preview.items[1].next_range_start, "2026-06-16");
        assert_eq!(preview.items[1].next_range_end, "2026-06-17");
        assert_eq!(preview.items[1].reasons[0].blocking_task_id, "task-b");
        assert_eq!(
            preview.items[1].reasons[0].required_start_date,
            "2026-06-16"
        );
        let mutation = apply_dependency_cascade(&pool, &request(&pool).await)
            .await
            .unwrap();
        assert_eq!(mutation.tasks.len(), 2);
        assert_eq!(mutation.task_change_events.len(), 4);
        assert!(
            mutation
                .tasks
                .iter()
                .all(|task| task.description.is_empty())
        );
        assert!(
            preview_dependency_cascade(&pool, "project-a")
                .await
                .unwrap()
                .items
                .is_empty()
        );
    });
}

#[test]
fn dependency_cascade_preserves_optional_dates_and_handles_leap_day_milestones() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        dated_graph(&pool).await;
        sqlx::raw_sql("DELETE FROM project_task_dependencies WHERE id = 'edge-b';
            UPDATE project_tasks SET start_date = NULL, target_end_date = NULL, due_date = '2024-02-28' WHERE id IN ('task-a', 'task-b');
            UPDATE project_tasks SET milestone = 1 WHERE id = 'task-b';").execute(&pool).await.unwrap();
        let preview = preview_dependency_cascade(&pool, "project-a")
            .await
            .unwrap();
        let item = &preview.items[0];
        assert_eq!(item.next_start_date, None);
        assert_eq!(item.next_target_end_date, None);
        assert_eq!(item.next_due_date.as_deref(), Some("2024-02-29"));
        assert_eq!(item.shift_days, 1);
    });
}

#[test]
fn dependency_cascade_rejects_cycles_without_proposing_repeated_shifts() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        dated_graph(&pool).await;
        sqlx::query("INSERT INTO project_task_dependencies(id, blocking_task_id, blocked_task_id) VALUES ('edge-cycle', 'task-c', 'task-a')").execute(&pool).await.unwrap();
        let preview = preview_dependency_cascade(&pool, "project-a")
            .await
            .unwrap();
        assert!(preview.items.is_empty());
        assert_eq!(preview.conflicts.len(), 3);
        assert!(
            serde_json::to_value(&preview.conflicts)
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["reason"] == "cycle")
        );
        assert!(
            apply_dependency_cascade(&pool, &request(&pool).await)
                .await
                .err()
                .unwrap()
                .contains("conflicts")
        );
    });
}

#[test]
fn dependency_cascade_reports_undated_invalid_and_foreign_endpoints() {
    block_on(async {
        for (change, reason) in [
            (
                "UPDATE project_tasks SET start_date = NULL, target_end_date = NULL WHERE id = 'task-b'",
                "undated_task",
            ),
            (
                "UPDATE project_tasks SET start_date = '2026-02-30' WHERE id = 'task-b'",
                "invalid_date",
            ),
            (
                "INSERT INTO projects(id, group_id, name) VALUES ('project-b', 'group-a', 'Other'); UPDATE project_tasks SET project_id = 'project-b' WHERE id = 'task-c'",
                "missing_endpoint",
            ),
            (
                "UPDATE project_tasks SET target_end_date = '9999-12-31' WHERE id = 'task-a'",
                "date_overflow",
            ),
        ] {
            let pool = migrated_memory_pool().await;
            dated_graph(&pool).await;
            sqlx::raw_sql(change).execute(&pool).await.unwrap();
            let preview = preview_dependency_cascade(&pool, "project-a")
                .await
                .unwrap();
            assert!(
                serde_json::to_value(&preview.conflicts)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["reason"] == reason),
                "{reason}"
            );
            let before = dates(&pool).await;
            assert!(
                apply_dependency_cascade(&pool, &request(&pool).await)
                    .await
                    .is_err()
            );
            assert_eq!(dates(&pool).await, before);
        }
    });
}

#[test]
fn dependency_cascade_protects_archived_completed_and_scheduled_work() {
    block_on(async {
        for (change, reason) in [
            (
                "UPDATE project_tasks SET archived_at = '2026-10-02T00:00:00.000Z' WHERE id = 'task-b'",
                "archived",
            ),
            (
                "UPDATE project_tasks SET completed_at = '2026-10-02T00:00:00.000Z' WHERE id = 'task-b'",
                "completed",
            ),
            (
                "UPDATE project_statuses SET terminal = 1 WHERE id = 'status-a'",
                "completed",
            ),
            (
                "INSERT INTO calendar_events(id, title, start_time, end_time, project_id) VALUES ('event-a', 'Scheduled', '2026-06-11T09:00:00Z', '2026-06-11T10:00:00Z', 'project-a'); INSERT INTO project_task_event_links(task_id, event_id) VALUES ('task-b', 'event-a')",
                "scheduled",
            ),
        ] {
            let pool = migrated_memory_pool().await;
            dated_graph(&pool).await;
            sqlx::raw_sql(change).execute(&pool).await.unwrap();
            let preview = preview_dependency_cascade(&pool, "project-a")
                .await
                .unwrap();
            assert!(
                serde_json::to_value(&preview.conflicts)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["reason"] == reason),
                "{reason}"
            );
            let before = dates(&pool).await;
            assert!(
                apply_dependency_cascade(&pool, &request(&pool).await)
                    .await
                    .is_err()
            );
            assert_eq!(dates(&pool).await, before);
        }
    });
}

#[test]
fn dependency_cascade_second_write_failure_rolls_back_dates_history_and_receipt() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        dated_graph(&pool).await;
        let request = request(&pool).await;
        let before = dates(&pool).await;
        sqlx::raw_sql("CREATE TRIGGER fail_cascade_second BEFORE UPDATE ON project_tasks WHEN NEW.id = 'task-c' BEGIN SELECT RAISE(ABORT, 'injected second date failure'); END;").execute(&pool).await.unwrap();
        assert!(
            apply_dependency_cascade(&pool, &request)
                .await
                .err()
                .unwrap()
                .contains("second date failure")
        );
        assert_eq!(dates(&pool).await, before);
        let counts: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM project_task_change_events), (SELECT count(*) FROM project_dependency_cascade_receipts)").fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (0, 0));
        sqlx::query("DROP TRIGGER fail_cascade_second")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            apply_dependency_cascade(&pool, &request)
                .await
                .unwrap()
                .tasks
                .len(),
            2
        );
    });
}

#[test]
fn dependency_cascade_requires_review_after_a_task_or_graph_change() {
    block_on(async {
        for change in [
            "UPDATE project_tasks SET title = 'Renamed unseen predecessor' WHERE id = 'task-a'",
            "UPDATE project_tasks SET target_end_date = '2026-06-20' WHERE id = 'task-a'",
            "DELETE FROM project_task_dependencies WHERE id = 'edge-b'",
            "INSERT INTO calendar_events(id, start_time, end_time, project_id) VALUES ('event-a', '2026-06-11T09:00:00Z', '2026-06-11T10:00:00Z', 'project-a'); INSERT INTO project_task_event_links(task_id, event_id) VALUES ('task-b', 'event-a')",
        ] {
            let pool = migrated_memory_pool().await;
            dated_graph(&pool).await;
            let request = request(&pool).await;
            sqlx::raw_sql(change).execute(&pool).await.unwrap();
            let before = dates(&pool).await;
            assert!(
                apply_dependency_cascade(&pool, &request)
                    .await
                    .err()
                    .unwrap()
                    .contains("preview changed")
            );
            assert_eq!(dates(&pool).await, before);
        }
    });
}

#[test]
fn dependency_cascade_lost_response_replays_after_restart_without_overwriting_later_edits() {
    block_on(async {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-project-cascade-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        dated_graph(&pool).await;
        let mut request = request(&pool).await;
        let receipt =
            serde_json::to_value(apply_dependency_cascade(&pool, &request).await.unwrap()).unwrap();
        pool.close().await;
        let reopened = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("UPDATE project_tasks SET start_date = '2026-07-01', target_end_date = '2026-07-03', title = 'Later' WHERE id = 'task-b'").execute(&reopened).await.unwrap();
        let before = dates(&reopened).await;
        assert_eq!(
            serde_json::to_value(apply_dependency_cascade(&reopened, &request).await.unwrap())
                .unwrap(),
            receipt
        );
        assert_eq!(dates(&reopened).await, before);
        let events: i64 = sqlx::query_scalar("SELECT count(*) FROM project_task_change_events")
            .fetch_one(&reopened)
            .await
            .unwrap();
        assert_eq!(events, 4);
        request.reviewed_digest = "a".repeat(64);
        assert!(
            apply_dependency_cascade(&reopened, &request)
                .await
                .err()
                .unwrap()
                .contains("different intent")
        );
        reopened.close().await;
        std::fs::remove_file(path).unwrap();
    });
}

#[test]
fn dependency_cascade_bounds_graph_and_full_result_before_mutation() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        dated_graph(&pool).await;
        sqlx::query("UPDATE project_tasks SET description = printf('%*s', 6000000, 'x') WHERE id = 'task-b'").execute(&pool).await.unwrap();
        let request = request(&pool).await;
        let before = dates(&pool).await;
        assert!(
            apply_dependency_cascade(&pool, &request)
                .await
                .err()
                .unwrap()
                .contains("byte limit")
        );
        assert_eq!(dates(&pool).await, before);
        sqlx::query("UPDATE project_tasks SET title = description WHERE id = 'task-b'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            preview_dependency_cascade(&pool, "project-a")
                .await
                .err()
                .unwrap()
                .contains("byte limit")
        );
    });
}

#[test]
fn dependency_cascade_rejects_an_excessive_task_count_without_truncation() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        dated_graph(&pool).await;
        sqlx::query("WITH RECURSIVE items(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM items WHERE value < 10000)
            INSERT INTO project_tasks(id, project_id, section_id, status_id, title)
            SELECT 'extra-' || value, 'project-a', 'section-a', 'status-a', 'Extra task' FROM items")
            .execute(&pool).await.unwrap();
        assert!(
            preview_dependency_cascade(&pool, "project-a")
                .await
                .err()
                .unwrap()
                .contains("count or byte limit")
        );
        let receipts: i64 =
            sqlx::query_scalar("SELECT count(*) FROM project_dependency_cascade_receipts")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(receipts, 0);
    });
}
