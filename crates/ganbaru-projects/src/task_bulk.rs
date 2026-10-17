//! Atomic field-specific task edits, with canonical descendant closure and durable retry receipts.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use super::history::{current_timestamp, insert_task_field_change_event, status_is_terminal};
use super::models::{ProjectTaskChangeEventRow, ProjectTaskRow, ProjectsMutationRows};
use super::mutations::{ensure_priority_matches_project, next_task_sort_order};
use super::validation::require_non_empty;

const MAX_SELECTED_TASKS: usize = 1_000;
const MAX_CHANGED_TASKS: i64 = 10_000;
const MAX_RECEIPT_BYTES: usize = 32 * 1024 * 1024;
const TASK_ORDER_SPACING: f64 = 1_000.0;

/// The field value shown when the user chose the action. Unrelated fields may change freely.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskFieldExpectation {
    pub id: String,
    pub value: Option<String>,
}

/// One semantic action applied to the complete selection in one transaction.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskBulkChange {
    Status {
        #[serde(rename = "statusId")]
        status_id: String,
    },
    Priority {
        priority: String,
    },
    Archive {
        archived: bool,
    },
}

/// Retain the same operation ID and payload when retrying after an uncertain response.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBulkRequest {
    pub operation_id: String,
    pub project_id: String,
    pub tasks: Vec<TaskFieldExpectation>,
    pub change: TaskBulkChange,
}

#[derive(Deserialize, Serialize)]
struct TaskBulkReceipt {
    tasks: Vec<ProjectTaskRow>,
    task_change_events: Vec<ProjectTaskChangeEventRow>,
}

impl From<TaskBulkReceipt> for ProjectsMutationRows {
    fn from(receipt: TaskBulkReceipt) -> Self {
        Self {
            tasks: receipt.tasks,
            task_change_events: receipt.task_change_events,
            ..Self::default()
        }
    }
}

/// Applies selected status, priority, or archive intent without resubmitting cached task records.
pub async fn apply_task_bulk(
    pool: &SqlitePool,
    request: &TaskBulkRequest,
) -> Result<ProjectsMutationRows, String> {
    validate_request(request)?;
    let encoded_request = serde_json::to_string(request).map_err(|e| e.to_string())?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin bulk task edit: {e}"))?;
    // Acquire the SQLite writer reservation before reading canonical state or an existing receipt.
    sqlx::query(
        "INSERT INTO project_task_bulk_receipts (operation_id, project_id, request_json)
         VALUES (?, ?, ?) ON CONFLICT(operation_id) DO NOTHING",
    )
    .bind(&request.operation_id)
    .bind(&request.project_id)
    .bind(&encoded_request)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("reserve bulk task operation: {e}"))?;
    let (saved_request, saved_response): (String, Option<String>) = sqlx::query_as(
        "SELECT request_json, response_json FROM project_task_bulk_receipts WHERE operation_id = ?",
    )
    .bind(&request.operation_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| format!("read bulk task receipt: {e}"))?;
    if saved_request != encoded_request {
        return Err("bulk task operation ID was already used for different intent".to_string());
    }
    if let Some(response) = saved_response {
        let receipt: TaskBulkReceipt = serde_json::from_str(&response)
            .map_err(|e| format!("read committed bulk task result: {e}"))?;
        tx.commit()
            .await
            .map_err(|e| format!("finish bulk task retry: {e}"))?;
        return Ok(receipt.into());
    }

    let mut rows = load_affected_tasks(&mut tx, request).await?;
    validate_expected_fields(&rows, request)?;
    if matches!(request.change, TaskBulkChange::Archive { archived: false }) {
        let restored_ids = rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<BTreeSet<_>>();
        for row in &rows {
            if let Some(parent_id) = row
                .parent_task_id
                .as_deref()
                .filter(|id| !restored_ids.contains(id))
            {
                let parent_archived: bool = sqlx::query_scalar(
                    "SELECT archived_at IS NOT NULL FROM project_tasks WHERE id = ?",
                )
                .bind(parent_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| format!("check restored task parent: {e}"))?;
                if parent_archived {
                    return Err(
                        "restore the archived parent together with its descendants".to_string()
                    );
                }
            }
        }
    }
    let now = current_timestamp(&mut tx).await?;
    let mut next_order = match &request.change {
        TaskBulkChange::Status { status_id } => {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM project_statuses WHERE id = ? AND project_id = ?)",
            )
            .bind(status_id)
            .bind(&request.project_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| format!("check bulk task status: {e}"))?;
            if !exists {
                return Err("status does not belong to project".to_string());
            }
            next_task_sort_order(
                &mut tx,
                "status_id",
                &request.project_id,
                status_id,
                "status_sort_order",
            )
            .await?
        }
        TaskBulkChange::Priority { priority } => {
            ensure_priority_matches_project(&mut tx, &request.project_id, priority).await?;
            0.0
        }
        TaskBulkChange::Archive { .. } => 0.0,
    };
    for row in &mut rows {
        apply_field_change(&mut tx, row, &request.change, &now, &mut next_order).await?;
    }
    let mut response_bytes = serde_json::to_vec(&rows)
        .map_err(|e| format!("measure bulk task rows: {e}"))?
        .len();
    let mut task_change_events = Vec::new();
    for row in &rows {
        let events = sqlx::query_as::<_, ProjectTaskChangeEventRow>(
            "SELECT * FROM project_task_change_events WHERE task_id = ?
             ORDER BY occurred_at DESC, id DESC LIMIT 32",
        )
        .bind(&row.id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("read bulk task history: {e}"))?;
        response_bytes += serde_json::to_vec(&events)
            .map_err(|e| format!("measure bulk task history: {e}"))?
            .len();
        if response_bytes > MAX_RECEIPT_BYTES {
            return Err(
                "bulk task result exceeds the supported size; select fewer tasks".to_string(),
            );
        }
        task_change_events.extend(events);
    }
    let receipt = TaskBulkReceipt {
        tasks: rows,
        task_change_events,
    };
    let response =
        serde_json::to_string(&receipt).map_err(|e| format!("encode bulk task result: {e}"))?;
    if response.len() > MAX_RECEIPT_BYTES {
        return Err("bulk task result exceeds the supported size; select fewer tasks".to_string());
    }
    sqlx::query("UPDATE project_task_bulk_receipts SET response_json = ? WHERE operation_id = ?")
        .bind(response)
        .bind(&request.operation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("save bulk task receipt: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit bulk task edit: {e}"))?;
    Ok(receipt.into())
}

fn validate_request(request: &TaskBulkRequest) -> Result<(), String> {
    for (value, label) in [
        (&request.operation_id, "operation ID"),
        (&request.project_id, "project ID"),
    ] {
        require_non_empty(value, label)?;
        if value.len() > 128 {
            return Err(format!("{label} is too long"));
        }
    }
    if request.tasks.is_empty() || request.tasks.len() > MAX_SELECTED_TASKS {
        return Err(format!(
            "bulk task selection must contain 1 to {MAX_SELECTED_TASKS} tasks"
        ));
    }
    let mut ids = BTreeSet::new();
    for task in &request.tasks {
        require_non_empty(&task.id, "task ID")?;
        if task.id.len() > 128 || task.value.as_ref().is_some_and(|value| value.len() > 128) {
            return Err("bulk task expectation is too long".to_string());
        }
        if !ids.insert(&task.id) {
            return Err("bulk task selection contains duplicate IDs".to_string());
        }
    }
    match &request.change {
        TaskBulkChange::Status { status_id } if status_id.is_empty() || status_id.len() > 128 => {
            Err("invalid status ID".to_string())
        }
        TaskBulkChange::Priority { priority } if priority.is_empty() || priority.len() > 128 => {
            Err("invalid priority ID".to_string())
        }
        _ => Ok(()),
    }
}

async fn load_affected_tasks(
    tx: &mut Transaction<'_, Sqlite>,
    request: &TaskBulkRequest,
) -> Result<Vec<ProjectTaskRow>, String> {
    let ids = serde_json::to_string(
        &request
            .tasks
            .iter()
            .map(|task| &task.id)
            .collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())?;
    let affected_ids = sqlx::query_scalar::<_, String>(
        "WITH RECURSIVE selected(id) AS (
             SELECT id FROM project_tasks WHERE id IN (SELECT value FROM json_each(?))
             UNION
             SELECT task.id FROM project_tasks task JOIN selected ON task.parent_task_id = selected.id
             WHERE ? LIMIT ?
         ) SELECT id FROM selected ORDER BY id",
    )
    .bind(ids)
    .bind(matches!(request.change, TaskBulkChange::Archive { .. }))
    .bind(MAX_CHANGED_TASKS + 1)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("read complete bulk task selection: {e}"))?;
    if affected_ids.len() as i64 > MAX_CHANGED_TASKS {
        return Err(format!(
            "bulk task descendant closure exceeds {MAX_CHANGED_TASKS} tasks"
        ));
    }
    let affected_json = serde_json::to_string(&affected_ids).map_err(|e| e.to_string())?;
    let text_bytes: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(length(CAST(id AS BLOB)) + length(CAST(project_id AS BLOB))
            + length(CAST(section_id AS BLOB)) + length(CAST(status_id AS BLOB))
            + length(CAST(COALESCE(parent_task_id, '') AS BLOB))
            + length(CAST(title AS BLOB)) + length(CAST(description AS BLOB))
            + length(CAST(priority AS BLOB)) + length(CAST(task_type AS BLOB))
            + length(CAST(COALESCE(blocker_reason, '') AS BLOB))), 0)
         FROM project_tasks WHERE id IN (SELECT value FROM json_each(?))",
    )
    .bind(&affected_json)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("measure bulk task selection: {e}"))?;
    if text_bytes > (MAX_RECEIPT_BYTES / 6) as i64 {
        return Err("bulk task data exceeds the supported size; select fewer tasks".to_string());
    }
    let mut rows = sqlx::query_as::<_, ProjectTaskRow>(
        "SELECT * FROM project_tasks WHERE id IN (SELECT value FROM json_each(?)) ORDER BY id",
    )
    .bind(affected_json)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("read bulk task rows: {e}"))?;
    if rows.iter().any(|row| row.project_id != request.project_id) {
        return Err("bulk tasks and descendants must belong to the selected project".to_string());
    }
    let selection_order = request
        .tasks
        .iter()
        .enumerate()
        .map(|(index, task)| (task.id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    rows.sort_by_key(|row| {
        selection_order
            .get(row.id.as_str())
            .copied()
            .unwrap_or(usize::MAX)
    });
    Ok(rows)
}

fn validate_expected_fields(
    rows: &[ProjectTaskRow],
    request: &TaskBulkRequest,
) -> Result<(), String> {
    let by_id = rows
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect::<BTreeMap<_, _>>();
    for expected in &request.tasks {
        let row = by_id
            .get(expected.id.as_str())
            .ok_or_else(|| format!("selected task {} no longer exists", expected.id))?;
        let actual = match &request.change {
            TaskBulkChange::Status { .. } => Some(row.status_id.as_str()),
            TaskBulkChange::Priority { .. } => Some(row.priority.as_str()),
            TaskBulkChange::Archive { .. } => row.archived_at.as_deref(),
        };
        if actual != expected.value.as_deref() {
            return Err(format!(
                "task {} changed since selection; refresh before applying",
                row.id
            ));
        }
        if !matches!(request.change, TaskBulkChange::Archive { .. }) && row.archived_at.is_some() {
            return Err(
                "restore archived tasks before changing their status or priority".to_string(),
            );
        }
    }
    Ok(())
}

async fn apply_field_change(
    tx: &mut Transaction<'_, Sqlite>,
    row: &mut ProjectTaskRow,
    change: &TaskBulkChange,
    now: &str,
    next_order: &mut f64,
) -> Result<(), String> {
    let (field, old, new, event_type) = match change {
        TaskBulkChange::Status { status_id } => {
            if row.status_id == *status_id {
                return Ok(());
            }
            let was_terminal = status_is_terminal(tx, &row.status_id).await?;
            let terminal = status_is_terminal(tx, status_id).await?;
            let old_name: String =
                sqlx::query_scalar("SELECT name FROM project_statuses WHERE id = ?")
                    .bind(&row.status_id)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| e.to_string())?;
            let new_name: String =
                sqlx::query_scalar("SELECT name FROM project_statuses WHERE id = ?")
                    .bind(status_id)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| e.to_string())?;
            row.status_id.clone_from(status_id);
            row.status_sort_order = *next_order;
            *next_order += TASK_ORDER_SPACING;
            if !row.status_sort_order.is_finite() || !next_order.is_finite() {
                return Err("task ordering exceeds the supported range".to_string());
            }
            row.completed_at = if terminal {
                row.completed_at.clone().or_else(|| Some(now.to_string()))
            } else {
                None
            };
            let event = if !was_terminal && terminal {
                "completed"
            } else if was_terminal && !terminal {
                "reopened"
            } else {
                "status_changed"
            };
            ("status", Some(old_name), Some(new_name), event)
        }
        TaskBulkChange::Priority { priority } => {
            if row.priority == *priority {
                return Ok(());
            }
            let old = std::mem::replace(&mut row.priority, priority.clone());
            ("priority", Some(old), Some(priority.clone()), "updated")
        }
        TaskBulkChange::Archive { archived } => {
            if row.archived_at.is_some() == *archived {
                return Ok(());
            }
            let next = archived.then(|| now.to_string());
            let old = std::mem::replace(&mut row.archived_at, next.clone());
            (
                "archived_at",
                old,
                next,
                if *archived { "archived" } else { "updated" },
            )
        }
    };
    sqlx::query(
        "UPDATE project_tasks SET status_id = ?, status_sort_order = ?, completed_at = ?,
         priority = ?, archived_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&row.status_id)
    .bind(row.status_sort_order)
    .bind(&row.completed_at)
    .bind(&row.priority)
    .bind(&row.archived_at)
    .bind(now)
    .bind(&row.id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("apply bulk task field: {e}"))?;
    row.updated_at = now.to_string();
    row.revision = sqlx::query_scalar("SELECT revision FROM project_tasks WHERE id = ?")
        .bind(&row.id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("read bulk task revision: {e}"))?;
    insert_task_field_change_event(tx, &row.id, event_type, field, old, new, None).await
}
