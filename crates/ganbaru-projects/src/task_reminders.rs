//! Upcoming task date reminders across projects, for the local notification scheduler.

use ganbaru_db::impl_sqlite_from_row;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::validation::validate_date;

/// One date of a task that reminds before it. The date and time are floating wall values; the caller places them in
/// its own time zone and picks the hour for a date without a time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectTaskReminder {
    pub task_id: String,
    pub project_id: String,
    pub project_name: String,
    pub title: String,
    /// `start` or `due`.
    pub anchor: String,
    pub date: String,
    pub time: Option<String>,
    pub minutes_before: i64,
}

impl_sqlite_from_row!(ProjectTaskReminder {
    task_id,
    project_id,
    project_name,
    title,
    anchor,
    date,
    time,
    minutes_before,
});

/// List the reminders of open, unarchived tasks in unarchived projects whose date is on or after `from_date`
/// (`YYYY-MM-DD`), earliest date first.
pub async fn list_upcoming_task_reminders(
    pool: &SqlitePool,
    from_date: &str,
) -> Result<Vec<ProjectTaskReminder>, String> {
    validate_date(from_date, "from_date")?;
    sqlx::query_as::<_, ProjectTaskReminder>(
        "SELECT t.id AS task_id, t.project_id, p.name AS project_name, t.title, 'start' AS anchor,
                t.start_date AS date, t.start_time AS time, t.start_reminder_minutes AS minutes_before
         FROM project_tasks t JOIN projects p ON p.id = t.project_id
         WHERE t.start_reminder_minutes IS NOT NULL AND t.start_date >= ?1
           AND t.completed_at IS NULL AND t.archived_at IS NULL AND p.status <> 'archived'
         UNION ALL
         SELECT t.id, t.project_id, p.name, t.title, 'due',
                t.due_date, t.due_time, t.due_reminder_minutes
         FROM project_tasks t JOIN projects p ON p.id = t.project_id
         WHERE t.due_reminder_minutes IS NOT NULL AND t.due_date >= ?1
           AND t.completed_at IS NULL AND t.archived_at IS NULL AND p.status <> 'archived'
         ORDER BY date, time, task_id, anchor",
    )
    .bind(from_date)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list upcoming task reminders: {e}"))
}
