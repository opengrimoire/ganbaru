use super::*;
use crate::models::{ProjectChecklistItemCreate, ProjectChecklistItemUpdate, ProjectStatusUpdate};
use crate::order::OrderPlacement;
use crate::structure_commands::update_status;

const EARLIER: &str = "2026-10-01T09:00:00.000Z";

async fn load_task(pool: &SqlitePool, id: &str) -> ProjectTaskRow {
    sqlx::query_as::<_, ProjectTaskRow>("SELECT * FROM project_tasks WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn history(pool: &SqlitePool, task_id: &str) -> Vec<(String, Option<String>)> {
    sqlx::query_as(
        "SELECT event_type, field_name FROM project_task_change_events
         WHERE task_id = ? ORDER BY occurred_at, id",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

fn status_update(id: &str, name: &str, category: &str, terminal: bool) -> ProjectStatusUpdate {
    ProjectStatusUpdate {
        id: id.to_string(),
        name: name.to_string(),
        category: category.to_string(),
        color: 0,
        placement: None,
        terminal,
    }
}

#[test]
fn editing_a_completed_task_keeps_its_completion_time() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        complete_fixture_task(&pool, "task-a", EARLIER).await;
        let mut edit = task_update_from_row(&load_task(&pool, "task-a").await);
        edit.title = "Renamed after completion".to_string();

        task_commands::update_task(&pool, edit).await.unwrap();

        let task = load_task(&pool, "task-a").await;
        assert_eq!(task.title, "Renamed after completion");
        assert_eq!(task.completed_at.as_deref(), Some(EARLIER));
    });
}

#[test]
fn task_status_moves_set_clear_and_record_completion() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        complete_fixture_task(&pool, "task-b", EARLIER).await;
        sqlx::query("UPDATE project_tasks SET completed_at = NULL WHERE id = 'task-b'")
            .execute(&pool)
            .await
            .unwrap();

        let mut complete = task_update_from_row(&load_task(&pool, "task-a").await);
        complete.status_id = "status-done".to_string();
        task_commands::update_task(&pool, complete).await.unwrap();
        let completed = load_task(&pool, "task-a").await;
        assert!(completed.completed_at.is_some());

        let mut repair = task_update_from_row(&load_task(&pool, "task-b").await);
        repair.title = "Merged without a time".to_string();
        task_commands::update_task(&pool, repair).await.unwrap();
        assert!(load_task(&pool, "task-b").await.completed_at.is_some());

        let mut reopen = task_update_from_row(&completed);
        reopen.status_id = "status-a".to_string();
        task_commands::update_task(&pool, reopen).await.unwrap();
        assert_eq!(load_task(&pool, "task-a").await.completed_at, None);

        let events: Vec<String> = history(&pool, "task-a")
            .await
            .into_iter()
            .filter(|(_, field)| field.as_deref() == Some("status"))
            .map(|(event, _)| event)
            .collect();
        assert_eq!(events, ["completed", "reopened"]);
    });
}

#[test]
fn editing_a_checked_checklist_item_keeps_its_completion_time() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        task_commands::create_checklist_item(
            &pool,
            ProjectChecklistItemCreate {
                id: "item-a".to_string(),
                task_id: "task-a".to_string(),
                title: "Step".to_string(),
                placement: OrderPlacement::End,
            },
        )
        .await
        .unwrap();
        let update = |title: &str, completed: bool| ProjectChecklistItemUpdate {
            id: "item-a".to_string(),
            title: title.to_string(),
            completed,
            placement: None,
        };
        let completed_at = || async {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT completed_at FROM project_checklist_items WHERE id = 'item-a'",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
        };

        task_commands::update_checklist_item(&pool, update("Step", true))
            .await
            .unwrap();
        sqlx::query("UPDATE project_checklist_items SET completed_at = ? WHERE id = 'item-a'")
            .bind(EARLIER)
            .execute(&pool)
            .await
            .unwrap();
        task_commands::update_checklist_item(&pool, update("Renamed step", true))
            .await
            .unwrap();
        assert_eq!(completed_at().await.as_deref(), Some(EARLIER));

        task_commands::update_checklist_item(&pool, update("Renamed step", false))
            .await
            .unwrap();
        assert_eq!(completed_at().await, None);
        task_commands::update_checklist_item(&pool, update("Renamed step", true))
            .await
            .unwrap();
        assert!(
            completed_at()
                .await
                .is_some_and(|at| at.as_str() != EARLIER)
        );
    });
}

#[test]
fn flipping_a_status_to_done_completes_its_tasks_and_back_reopens_them() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let before = load_task(&pool, "task-a").await;

        let flipped = update_status(&pool, status_update("status-a", "Shipped", "done", true))
            .await
            .unwrap();

        let mut moved: Vec<&str> = flipped.tasks.iter().map(|task| task.id.as_str()).collect();
        moved.sort_unstable();
        assert_eq!(moved, ["task-a", "task-b", "task-c"]);
        let task = load_task(&pool, "task-a").await;
        let done_at = task.completed_at.clone().expect("flip completes the task");
        assert_eq!(task.revision, before.revision + 1);
        assert_eq!(
            history(&pool, "task-a").await.last(),
            Some(&("completed".to_string(), Some("status".to_string())))
        );

        let renamed = update_status(&pool, status_update("status-a", "Released", "done", true))
            .await
            .unwrap();
        assert!(renamed.tasks.is_empty());
        assert_eq!(load_task(&pool, "task-a").await.completed_at, Some(done_at));

        let reopened = update_status(&pool, status_update("status-a", "To do", "active", false))
            .await
            .unwrap();
        assert_eq!(reopened.tasks.len(), 3);
        assert!(
            reopened
                .tasks
                .iter()
                .all(|task| task.completed_at.is_none())
        );
        assert_eq!(
            history(&pool, "task-a").await.last(),
            Some(&("reopened".to_string(), Some("status".to_string())))
        );
    });
}

#[test]
fn a_status_update_that_breaks_the_done_rule_writes_nothing() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;

        for (category, terminal) in [("done", false), ("active", true)] {
            assert!(
                update_status(
                    &pool,
                    status_update("status-a", "Broken", category, terminal)
                )
                .await
                .is_err()
            );
        }
        assert!(
            sqlx::query(
                "UPDATE project_statuses SET category = 'done', terminal = 0 WHERE id = 'status-a'"
            )
            .execute(&pool)
            .await
            .is_err()
        );

        let (name, terminal): (String, i64) =
            sqlx::query_as("SELECT name, terminal FROM project_statuses WHERE id = 'status-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((name.as_str(), terminal), ("To do", 0));
        assert_eq!(load_task(&pool, "task-a").await.completed_at, None);
    });
}
