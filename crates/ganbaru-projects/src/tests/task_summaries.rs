use super::*;
use crate::task_summaries::{ProjectTaskSummary, list_task_summaries};

async fn set_task(pool: &SqlitePool, id: &str, updated_at: &str, completed: bool, archived: bool) {
    sqlx::query(
        "UPDATE project_tasks
         SET updated_at = ?,
             completed_at = CASE WHEN ? THEN '2026-10-01T00:00:00Z' END,
             archived_at = CASE WHEN ? THEN '2026-10-01T00:00:00Z' END
         WHERE id = ?",
    )
    .bind(updated_at)
    .bind(completed)
    .bind(archived)
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

fn summary(id: &str, completed: bool) -> ProjectTaskSummary {
    ProjectTaskSummary {
        id: id.to_string(),
        project_id: "project-a".to_string(),
        title: id.to_string(),
        completed,
    }
}

#[test]
fn task_summaries_list_open_tasks_first_and_skip_archived_ones() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        set_task(&pool, "task-a", "2026-10-03T00:00:00Z", true, false).await;
        set_task(&pool, "task-b", "2026-10-01T00:00:00Z", false, false).await;
        set_task(&pool, "task-c", "2026-10-02T00:00:00Z", false, true).await;

        let tasks = list_task_summaries(&pool, &["project-a".to_string()], 10)
            .await
            .unwrap();

        assert_eq!(
            tasks,
            vec![summary("task-b", false), summary("task-a", true)]
        );
    });
}

#[test]
fn task_summaries_respect_the_project_filter_and_limit() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;

        assert_eq!(
            list_task_summaries(&pool, &["other-project".to_string()], 10).await,
            Ok(Vec::new())
        );
        assert_eq!(list_task_summaries(&pool, &[], 10).await, Ok(Vec::new()));
        assert_eq!(
            list_task_summaries(&pool, &["project-a".to_string()], 2)
                .await
                .unwrap()
                .len(),
            2
        );
    });
}
