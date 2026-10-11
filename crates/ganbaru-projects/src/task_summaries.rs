//! Compact task listings for other domains that reference tasks by name, such as Notes mentions.

use ganbaru_db::impl_sqlite_from_row;
use serde::Serialize;
use sqlx::SqlitePool;

/// A task's identity, owner, and title, without planning details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectTaskSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub completed: bool,
}

impl_sqlite_from_row!(ProjectTaskSummary {
    id,
    project_id,
    title,
    completed,
});

/// List the unarchived tasks of the given projects, open tasks first and then the most recently updated, up to
/// `limit` tasks.
pub async fn list_task_summaries(
    pool: &SqlitePool,
    project_ids: &[String],
    limit: u32,
) -> Result<Vec<ProjectTaskSummary>, String> {
    if project_ids.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let project_ids = serde_json::to_string(project_ids)
        .map_err(|e| format!("encode project task summary filter: {e}"))?;
    sqlx::query_as::<_, ProjectTaskSummary>(
        "SELECT id, project_id, title, completed_at IS NOT NULL AS completed
         FROM project_tasks
         WHERE project_id IN (SELECT value FROM json_each(?)) AND archived_at IS NULL
         ORDER BY completed_at IS NOT NULL, updated_at DESC, id
         LIMIT ?",
    )
    .bind(project_ids)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list project task summaries: {e}"))
}
