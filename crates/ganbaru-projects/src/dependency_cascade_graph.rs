//! Bounded canonical input and result admission for Project dependency cascades.

use super::dependency_cascade::{
    CascadeItem, MAX_BYTES, MAX_DEPENDENCIES, MAX_ID_BYTES, MAX_TASKS,
};
use ganbaru_db::impl_sqlite_from_row;
use serde::Serialize;
use sqlx::{Sqlite, Transaction};

#[derive(Serialize)]
pub(super) struct CascadeTask {
    pub id: String,
    pub title: String,
    pub revision: i64,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub target_end_date: Option<String>,
    pub milestone: i64,
    pub archived: i64,
    pub completed: i64,
    pub scheduled: i64,
}
impl_sqlite_from_row!(CascadeTask {
    id,
    title,
    revision,
    start_date,
    due_date,
    target_end_date,
    milestone,
    archived,
    completed,
    scheduled
});

#[derive(Serialize)]
pub(super) struct CascadeDependency {
    pub id: String,
    pub blocking_task_id: String,
    pub blocked_task_id: String,
}
impl_sqlite_from_row!(CascadeDependency {
    id,
    blocking_task_id,
    blocked_task_id
});

#[derive(Serialize)]
pub(super) struct CascadeGraph {
    pub tasks: Vec<CascadeTask>,
    pub dependencies: Vec<CascadeDependency>,
}

const DEPENDENCY_SCOPE: &str = "(d.blocking_task_id IN (SELECT id FROM project_tasks WHERE project_id = ?) OR d.blocked_task_id IN (SELECT id FROM project_tasks WHERE project_id = ?))";
const TASK_TEXT_COLUMNS: &[&str] = &[
    "id",
    "project_id",
    "section_id",
    "status_id",
    "parent_task_id",
    "title",
    "description",
    "priority",
    "task_type",
    "due_date",
    "due_time",
    "start_date",
    "start_time",
    "target_end_date",
    "completed_at",
    "archived_at",
    "blocker_reason",
    "created_at",
    "updated_at",
];

/// Count and measure variable-size columns before materializing the complete input graph.
pub(super) async fn load_graph(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: &str,
) -> Result<CascadeGraph, String> {
    let archived: Option<bool> =
        sqlx::query_scalar("SELECT status = 'archived' FROM projects WHERE id = ?")
            .bind(project_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|error| format!("read cascade project: {error}"))?;
    match archived {
        Some(false) => {}
        Some(true) => return Err("restore the project before repairing dependency dates".into()),
        None => return Err("project no longer exists".into()),
    }
    let (count, bytes, max_id): (i64, i64, i64) = sqlx::query_as("SELECT count(*), coalesce(sum(length(CAST(id AS BLOB)) + length(CAST(title AS BLOB)) + length(CAST(coalesce(start_date, '') AS BLOB)) + length(CAST(coalesce(due_date, '') AS BLOB)) + length(CAST(coalesce(target_end_date, '') AS BLOB))), 0), coalesce(max(length(CAST(id AS BLOB))), 0) FROM (SELECT id, title, start_date, due_date, target_end_date FROM project_tasks WHERE project_id = ? LIMIT ?)")
        .bind(project_id).bind(MAX_TASKS + 1).fetch_one(&mut **tx).await.map_err(|error| format!("bound cascade task graph: {error}"))?;
    if count > MAX_TASKS || bytes > (MAX_BYTES / 6) as i64 || max_id > MAX_ID_BYTES as i64 {
        return Err("dependency task graph exceeds its count or byte limit".into());
    }
    let (edges, edge_bytes, edge_id): (i64, i64, i64) = sqlx::query_as(&format!("SELECT count(*), coalesce(sum(length(CAST(id AS BLOB)) + length(CAST(blocking_task_id AS BLOB)) + length(CAST(blocked_task_id AS BLOB))), 0), coalesce(max(max(length(CAST(id AS BLOB)), length(CAST(blocking_task_id AS BLOB)), length(CAST(blocked_task_id AS BLOB)))), 0) FROM (SELECT id, blocking_task_id, blocked_task_id FROM project_task_dependencies d WHERE {DEPENDENCY_SCOPE} LIMIT ?)"))
        .bind(project_id).bind(project_id).bind(MAX_DEPENDENCIES + 1).fetch_one(&mut **tx).await.map_err(|error| format!("bound cascade dependency graph: {error}"))?;
    if edges > MAX_DEPENDENCIES
        || bytes + edge_bytes > (MAX_BYTES / 6) as i64
        || edge_id > MAX_ID_BYTES as i64
    {
        return Err("dependency graph exceeds its count or byte limit".into());
    }
    let tasks = sqlx::query_as::<_, CascadeTask>("SELECT t.id, t.title, t.revision, t.start_date, t.due_date, t.target_end_date,
        (t.milestone != 0 OR t.task_type = 'milestone') AS milestone,
        (t.archived_at IS NOT NULL) AS archived,
        (t.completed_at IS NOT NULL OR s.terminal != 0 OR s.category = 'done') AS completed,
        EXISTS(SELECT 1 FROM project_task_event_links l WHERE l.task_id = t.id AND l.link_kind = 'scheduled') AS scheduled
        FROM project_tasks t JOIN project_statuses s ON s.id = t.status_id AND s.project_id = t.project_id WHERE t.project_id = ? ORDER BY t.id")
        .bind(project_id).fetch_all(&mut **tx).await.map_err(|error| format!("read cascade task graph: {error}"))?;
    if tasks.len() as i64 != count {
        return Err("dependency graph contains a task without its status".into());
    }
    let dependencies = sqlx::query_as::<_, CascadeDependency>(&format!("SELECT d.id, d.blocking_task_id, d.blocked_task_id FROM project_task_dependencies d WHERE {DEPENDENCY_SCOPE} ORDER BY d.id"))
        .bind(project_id).bind(project_id).fetch_all(&mut **tx).await.map_err(|error| format!("read cascade dependencies: {error}"))?;
    Ok(CascadeGraph {
        tasks,
        dependencies,
    })
}

/// Admit full returned task rows and bounded new history before applying the first date change.
pub(super) async fn bound_changed_rows(
    tx: &mut Transaction<'_, Sqlite>,
    items: &[CascadeItem],
) -> Result<(), String> {
    let ids = serde_json::to_string(&items.iter().map(|item| &item.task_id).collect::<Vec<_>>())
        .map_err(|error| error.to_string())?;
    let terms = TASK_TEXT_COLUMNS
        .iter()
        .map(|column| format!("length(CAST(coalesce({column}, '') AS BLOB))"))
        .collect::<Vec<_>>()
        .join(" + ");
    let bytes: i64 = sqlx::query_scalar(&format!("SELECT coalesce(sum({terms}), 0) FROM project_tasks WHERE id IN (SELECT value FROM json_each(?))"))
        .bind(ids).fetch_one(&mut **tx).await.map_err(|error| format!("bound shifted task receipt: {error}"))?;
    // JSON escaping can multiply text by six; three date history rows add bounded overhead.
    if bytes < 0 || bytes as usize * 6 + items.len() * 4096 > MAX_BYTES {
        return Err("dependency cascade result exceeds its byte limit".into());
    }
    Ok(())
}
