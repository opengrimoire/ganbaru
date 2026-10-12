use super::*;
use crate::task_commands::update_task;
use crate::task_reminders::{ProjectTaskReminder, list_upcoming_task_reminders};

async fn task_row(pool: &SqlitePool, id: &str) -> ProjectTaskRow {
    sqlx::query_as::<_, ProjectTaskRow>("SELECT * FROM project_tasks WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn set_reminders(
    pool: &SqlitePool,
    id: &str,
    start: Option<(&str, Option<&str>, i64)>,
    due: Option<(&str, Option<&str>, i64)>,
) {
    let mut task = task_update_from_row(&task_row(pool, id).await);
    task.start_date = start.map(|(date, _, _)| date.to_string());
    task.start_time = start.and_then(|(_, time, _)| time.map(str::to_string));
    task.start_reminder_minutes = start.map(|(_, _, minutes)| minutes);
    task.due_date = due.map(|(date, _, _)| date.to_string());
    task.due_time = due.and_then(|(_, time, _)| time.map(str::to_string));
    task.due_reminder_minutes = due.map(|(_, _, minutes)| minutes);
    update_task(pool, task).await.unwrap();
}

fn reminder(
    task_id: &str,
    anchor: &str,
    date: &str,
    time: Option<&str>,
    minutes_before: i64,
) -> ProjectTaskReminder {
    ProjectTaskReminder {
        task_id: task_id.to_string(),
        project_id: "project-a".to_string(),
        project_name: "Project A".to_string(),
        title: task_id.to_string(),
        anchor: anchor.to_string(),
        date: date.to_string(),
        time: time.map(str::to_string),
        minutes_before,
    }
}

#[test]
fn task_update_validates_date_reminders() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let mut task = task_update_from_row(&task_row(&pool, "task-a").await);

        task.due_reminder_minutes = Some(0);
        assert_eq!(
            validate_task_update(&task),
            Err("due_reminder_minutes requires due_date".to_string())
        );

        task.due_date = Some("2026-10-20".to_string());
        assert_eq!(validate_task_update(&task), Ok(()));

        task.start_date = Some("2026-10-19".to_string());
        task.start_reminder_minutes = Some(-5);
        assert_eq!(
            validate_task_update(&task),
            Err("start_reminder_minutes must be between 0 and 40320".to_string())
        );

        task.start_reminder_minutes = Some(40_321);
        assert!(validate_task_update(&task).is_err());

        task.start_reminder_minutes = Some(40_320);
        assert_eq!(validate_task_update(&task), Ok(()));
    });
}

#[test]
fn task_update_persists_reminders_and_the_schema_requires_their_dates() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        set_reminders(
            &pool,
            "task-a",
            Some(("2026-10-19", None, 1440)),
            Some(("2026-10-20", Some("17:30"), 15)),
        )
        .await;

        let stored = task_row(&pool, "task-a").await;
        assert_eq!(stored.start_reminder_minutes, Some(1440));
        assert_eq!(stored.due_reminder_minutes, Some(15));

        let orphaned = sqlx::query(
            "UPDATE project_tasks SET due_date = NULL, due_time = NULL WHERE id = 'task-a'",
        )
        .execute(&pool)
        .await;
        assert!(orphaned.is_err(), "a due reminder must keep its due date");

        let out_of_range = sqlx::query(
            "UPDATE project_tasks SET start_reminder_minutes = 40321 WHERE id = 'task-a'",
        )
        .execute(&pool)
        .await;
        assert!(out_of_range.is_err(), "the schema bounds reminder offsets");
    });
}

#[test]
fn upcoming_reminders_list_open_dates_from_the_given_day() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        set_reminders(
            &pool,
            "task-a",
            Some(("2026-10-09", None, 0)),
            Some(("2026-10-20", Some("17:30"), 15)),
        )
        .await;
        set_reminders(
            &pool,
            "task-b",
            Some(("2026-10-12", Some("09:00"), 60)),
            None,
        )
        .await;
        set_reminders(&pool, "task-c", None, Some(("2026-10-15", None, 1440))).await;

        let reminders = list_upcoming_task_reminders(&pool, "2026-10-10")
            .await
            .unwrap();
        assert_eq!(
            reminders,
            vec![
                reminder("task-b", "start", "2026-10-12", Some("09:00"), 60),
                reminder("task-c", "due", "2026-10-15", None, 1440),
                reminder("task-a", "due", "2026-10-20", Some("17:30"), 15),
            ]
        );
    });
}

#[test]
fn upcoming_reminders_skip_completed_archived_and_archived_project_tasks() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        for id in ["task-a", "task-b", "task-c"] {
            set_reminders(&pool, id, None, Some(("2026-10-20", None, 0))).await;
        }
        sqlx::query(
            "UPDATE project_tasks SET completed_at = '2026-10-11T00:00:00Z' WHERE id = 'task-a'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE project_tasks SET archived_at = '2026-10-11T00:00:00Z' WHERE id = 'task-b'",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(
            list_upcoming_task_reminders(&pool, "2026-10-10").await,
            Ok(vec![reminder("task-c", "due", "2026-10-20", None, 0)])
        );

        sqlx::query("UPDATE projects SET status = 'archived' WHERE id = 'project-a'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            list_upcoming_task_reminders(&pool, "2026-10-10").await,
            Ok(Vec::new())
        );
        assert!(
            list_upcoming_task_reminders(&pool, "tomorrow")
                .await
                .is_err()
        );
    });
}
