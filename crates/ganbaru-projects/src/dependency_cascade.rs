//! Reviewed dependency date shifts over one bounded canonical project graph.

use super::history::{current_timestamp, insert_task_field_change_event};
use super::models::{ProjectTaskChangeEventRow, ProjectTaskRow, ProjectsMutationRows};
use super::{dependency_cascade_graph as graph, dependency_cascade_plan as plan};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

pub(super) const MAX_TASKS: i64 = 10_000;
pub(super) const MAX_DEPENDENCIES: i64 = 20_000;
pub(super) const MAX_BYTES: usize = 32 * 1024 * 1024;
pub(super) const MAX_ID_BYTES: usize = 256;

/// The complete reviewed meaning of one finish-to-start date proposal.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyCascadePreview {
    pub project_id: String,
    pub digest: String,
    pub items: Vec<CascadeItem>,
    pub conflicts: Vec<CascadeConflict>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CascadeItem {
    pub task_id: String,
    pub title: String,
    pub shift_days: i64,
    pub original_start_date: Option<String>,
    pub original_due_date: Option<String>,
    pub original_target_end_date: Option<String>,
    pub original_range_start: String,
    pub original_range_end: String,
    pub next_start_date: Option<String>,
    pub next_due_date: Option<String>,
    pub next_target_end_date: Option<String>,
    pub next_range_start: String,
    pub next_range_end: String,
    pub reasons: Vec<CascadeReason>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CascadeReason {
    pub dependency_id: String,
    pub blocking_task_id: String,
    pub blocking_title: String,
    pub required_start_date: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictReason {
    MissingEndpoint,
    Cycle,
    UndatedTask,
    InvalidDate,
    Archived,
    Completed,
    Scheduled,
    DateOverflow,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CascadeConflict {
    pub dependency_id: String,
    pub task_id: String,
    pub title: String,
    pub reason: ConflictReason,
}

/// The digest binds the exact graph and dates shown in the native preview.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyCascadeApply {
    pub operation_id: String,
    pub project_id: String,
    pub reviewed_digest: String,
}

#[derive(Deserialize, Serialize)]
struct CascadeReceipt {
    tasks: Vec<ProjectTaskRow>,
    task_change_events: Vec<ProjectTaskChangeEventRow>,
}

impl From<CascadeReceipt> for ProjectsMutationRows {
    fn from(value: CascadeReceipt) -> Self {
        Self {
            tasks: value.tasks,
            task_change_events: value.task_change_events,
            ..Self::default()
        }
    }
}

pub(super) fn validate_id(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_ID_BYTES {
        return Err("invalid project cascade identity".into());
    }
    Ok(())
}

/// Reads all canonical dependencies, including endpoints outside the visible Gantt window.
pub async fn preview_dependency_cascade(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<DependencyCascadePreview, String> {
    validate_id(project_id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin dependency preview: {error}"))?;
    let result = preview_tx(&mut tx, project_id).await?;
    tx.commit()
        .await
        .map_err(|error| format!("finish dependency preview: {error}"))?;
    Ok(result)
}

async fn preview_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: &str,
) -> Result<DependencyCascadePreview, String> {
    let graph = graph::load_graph(tx, project_id).await?;
    plan::build_preview(project_id, &graph)
}

/// Applies only the reviewed graph; a changed preview must be reviewed again.
pub async fn apply_dependency_cascade(
    pool: &SqlitePool,
    request: &DependencyCascadeApply,
) -> Result<ProjectsMutationRows, String> {
    validate_id(&request.operation_id)?;
    validate_id(&request.project_id)?;
    if request.reviewed_digest.len() != 64
        || !request
            .reviewed_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("invalid dependency preview digest".into());
    }
    let encoded = serde_json::to_string(request).map_err(|error| error.to_string())?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin dependency cascade: {error}"))?;
    // Reserve the writer before checking the receipt or any graph precondition.
    sqlx::query("INSERT INTO project_dependency_cascade_receipts(operation_id, project_id, request_json) VALUES (?, ?, ?) ON CONFLICT(operation_id) DO NOTHING")
        .bind(&request.operation_id).bind(&request.project_id).bind(&encoded).execute(&mut *tx).await
        .map_err(|error| format!("reserve dependency cascade: {error}"))?;
    let (saved_request, saved_response): (String, Option<String>) = sqlx::query_as("SELECT request_json, response_json FROM project_dependency_cascade_receipts WHERE operation_id = ?")
        .bind(&request.operation_id).fetch_one(&mut *tx).await.map_err(|error| format!("read dependency cascade receipt: {error}"))?;
    if saved_request != encoded {
        return Err(
            "dependency cascade operation identity was reused with different intent".into(),
        );
    }
    if let Some(response) = saved_response {
        let receipt: CascadeReceipt = serde_json::from_str(&response)
            .map_err(|error| format!("decode dependency cascade receipt: {error}"))?;
        tx.commit()
            .await
            .map_err(|error| format!("finish dependency cascade retry: {error}"))?;
        return Ok(receipt.into());
    }
    let preview = preview_tx(&mut tx, &request.project_id).await?;
    if preview.digest != request.reviewed_digest {
        return Err("dependency preview changed; refresh and review it before applying".into());
    }
    if !preview.conflicts.is_empty() {
        return Err("resolve dependency preview conflicts before applying".into());
    }
    if preview.items.is_empty() {
        return Err("dependency preview has no date changes".into());
    }
    graph::bound_changed_rows(&mut tx, &preview.items).await?;
    let now = current_timestamp(&mut tx).await?;
    let mut tasks = Vec::with_capacity(preview.items.len());
    let mut events = Vec::new();
    for item in &preview.items {
        sqlx::query("UPDATE project_tasks SET start_date = ?, due_date = ?, target_end_date = ?, updated_at = ? WHERE id = ?")
            .bind(&item.next_start_date).bind(&item.next_due_date).bind(&item.next_target_end_date).bind(&now).bind(&item.task_id)
            .execute(&mut *tx).await.map_err(|error| format!("apply dependency date change: {error}"))?;
        for (field, old, new) in [
            (
                "start_date",
                &item.original_start_date,
                &item.next_start_date,
            ),
            ("due_date", &item.original_due_date, &item.next_due_date),
            (
                "target_end_date",
                &item.original_target_end_date,
                &item.next_target_end_date,
            ),
        ] {
            if old == new {
                continue;
            }
            insert_task_field_change_event(
                &mut tx,
                &item.task_id,
                "updated",
                field,
                old.clone(),
                new.clone(),
                Some("dependency_cascade"),
            )
            .await?;
            // Only the event just inserted belongs to this receipt, not a historical window.
            events.push(sqlx::query_as::<_, ProjectTaskChangeEventRow>("SELECT * FROM project_task_change_events WHERE task_id = ? ORDER BY rowid DESC LIMIT 1")
                .bind(&item.task_id).fetch_one(&mut *tx).await.map_err(|error| format!("read dependency date history: {error}"))?);
        }
        tasks.push(
            sqlx::query_as::<_, ProjectTaskRow>("SELECT * FROM project_tasks WHERE id = ?")
                .bind(&item.task_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|error| format!("read shifted task: {error}"))?,
        );
    }
    let receipt = CascadeReceipt {
        tasks,
        task_change_events: events,
    };
    let response = serde_json::to_string(&receipt)
        .map_err(|error| format!("encode dependency cascade receipt: {error}"))?;
    if response.len() > MAX_BYTES {
        return Err("dependency cascade receipt exceeds its byte limit".into());
    }
    sqlx::query(
        "UPDATE project_dependency_cascade_receipts SET response_json = ? WHERE operation_id = ?",
    )
    .bind(response)
    .bind(&request.operation_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| format!("save dependency cascade receipt: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit dependency cascade: {error}"))?;
    Ok(receipt.into())
}
