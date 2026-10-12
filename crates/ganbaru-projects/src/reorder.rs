//! Semantic adjacent moves over canonical siblings, committed with durable retry receipts.

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use super::models::{
    ProjectCustomFieldOptionRow, ProjectCustomFieldRow, ProjectTaskRow, ProjectsMutationRows,
};
use super::validation::require_non_empty;

const MAX_SIBLINGS: i64 = 10_000;
const MAX_RECEIPT_BYTES: usize = 32 * 1024 * 1024;
const ORDER_SPACING: i64 = 1_000;
const MAX_ID_BYTES: i64 = 256;
const MAX_REQUEST_BYTES: usize = 4_096;
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

/// The ordering lane, independent of view filters and pagination.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOrderAxis {
    Section,
    Status,
}

impl TaskOrderAxis {
    fn columns(self) -> (&'static str, &'static str) {
        match self {
            Self::Section => ("section_id", "section_sort_order"),
            Self::Status => ("status_id", "status_sort_order"),
        }
    }
}

/// Preconditions describe only placement, so concurrent unrelated edits are preserved.
#[derive(Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ReorderItem {
    Task {
        id: String,
        axis: TaskOrderAxis,
        group_id: String,
        parent_task_id: Option<String>,
        expected_order: f64,
    },
    CustomField {
        id: String,
        expected_order: i64,
    },
    CustomFieldOption {
        id: String,
        field_id: String,
        expected_order: i64,
    },
}

/// An adjacent move retains its operation identity across uncertain responses.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectReorderRequest {
    pub operation_id: String,
    pub project_id: String,
    pub direction: i8,
    pub item: ReorderItem,
}

#[derive(Default, Deserialize, Serialize)]
struct ReorderReceipt {
    tasks: Vec<ProjectTaskRow>,
    custom_fields: Vec<ProjectCustomFieldRow>,
    custom_field_options: Vec<ProjectCustomFieldOptionRow>,
}

impl From<ReorderReceipt> for ProjectsMutationRows {
    fn from(receipt: ReorderReceipt) -> Self {
        Self {
            tasks: receipt.tasks,
            custom_fields: receipt.custom_fields,
            custom_field_options: receipt.custom_field_options,
            ..Self::default()
        }
    }
}

/// Moves against the current complete sibling order, without replaying cached row contents.
pub async fn reorder_item(
    pool: &SqlitePool,
    request: &ProjectReorderRequest,
) -> Result<ProjectsMutationRows, String> {
    for (name, value) in [
        ("operation_id", &request.operation_id),
        ("project_id", &request.project_id),
    ] {
        require_non_empty(value, name)?;
        if value.len() > MAX_ID_BYTES as usize {
            return Err(format!("{name} exceeds the reorder identity limit"));
        }
    }
    if request.direction != -1 && request.direction != 1 {
        return Err("reorder direction must be previous or next".to_string());
    }
    let encoded = serde_json::to_string(request)
        .map_err(|error| format!("encode reorder intent: {error}"))?;
    if encoded.len() > MAX_REQUEST_BYTES {
        return Err("reorder intent exceeds the request limit".to_string());
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin project reorder: {error}"))?;
    sqlx::query("INSERT INTO project_reorder_receipts (operation_id, project_id, request_json) VALUES (?, ?, ?) ON CONFLICT(operation_id) DO NOTHING")
        .bind(&request.operation_id).bind(&request.project_id).bind(&encoded).execute(&mut *tx).await
        .map_err(|error| format!("reserve project reorder: {error}"))?;
    let (saved_request, saved_response): (String, Option<String>) = sqlx::query_as(
        "SELECT request_json, response_json FROM project_reorder_receipts WHERE operation_id = ?",
    )
    .bind(&request.operation_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| format!("read project reorder receipt: {error}"))?;
    if saved_request != encoded {
        return Err("project reorder identity was reused for different intent".to_string());
    }
    if let Some(saved) = saved_response {
        let receipt: ReorderReceipt = serde_json::from_str(&saved)
            .map_err(|error| format!("decode project reorder receipt: {error}"))?;
        tx.commit()
            .await
            .map_err(|error| format!("finish project reorder retry: {error}"))?;
        return Ok(receipt.into());
    }
    let receipt = match &request.item {
        ReorderItem::Task {
            id,
            axis,
            group_id,
            parent_task_id,
            expected_order,
        } => {
            let changes = task_placement_changes(
                &mut tx,
                request,
                id,
                *axis,
                group_id,
                parent_task_id.as_deref(),
                *expected_order,
            )
            .await?;
            let (_, order_column) = axis.columns();
            for (id, order) in &changes {
                sqlx::query(&format!("UPDATE project_tasks SET {order_column} = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?"))
                    .bind(order).bind(id).execute(&mut *tx).await.map_err(|error| format!("reorder project task: {error}"))?;
            }
            let ids = serde_json::to_string(&changes.iter().map(|(id, _)| id).collect::<Vec<_>>())
                .map_err(|error| format!("encode reordered tasks: {error}"))?;
            let text_bytes = TASK_TEXT_COLUMNS
                .iter()
                .map(|column| format!("coalesce(length(CAST({column} AS BLOB)), 0)"))
                .collect::<Vec<_>>()
                .join(" + ");
            let bytes: i64 = sqlx::query_scalar(&format!("SELECT coalesce(sum({text_bytes}), 0) FROM project_tasks WHERE id IN (SELECT value FROM json_each(?))"))
                .bind(&ids).fetch_one(&mut *tx).await.map_err(|error| format!("measure reordered task result: {error}"))?;
            if bytes > (MAX_RECEIPT_BYTES / 6) as i64 {
                return Err("project reorder result exceeds the byte limit".to_string());
            }
            let tasks = sqlx::query_as::<_, ProjectTaskRow>("SELECT * FROM project_tasks WHERE id IN (SELECT value FROM json_each(?)) ORDER BY id")
                .bind(ids).fetch_all(&mut *tx).await.map_err(|error| format!("read reordered tasks: {error}"))?;
            ReorderReceipt {
                tasks,
                ..ReorderReceipt::default()
            }
        }
        ReorderItem::CustomField { id, expected_order } => {
            check_schema_sibling_ids(
                &mut tx,
                "project_custom_fields",
                "project_id",
                &request.project_id,
            )
            .await?;
            let rows: Vec<(String, i64)> = sqlx::query_as("SELECT id, sort_order FROM project_custom_fields WHERE project_id = ? ORDER BY sort_order, name, id LIMIT ?")
                .bind(&request.project_id).bind(MAX_SIBLINGS + 1).fetch_all(&mut *tx).await.map_err(|error| format!("read custom field order: {error}"))?;
            let changes = integer_order_changes(rows, id, *expected_order, request.direction)?;
            check_schema_result_size(&mut tx, "project_custom_fields", &changes).await?;
            let mut fields = Vec::new();
            for (id, order) in changes {
                sqlx::query("UPDATE project_custom_fields SET sort_order = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
                    .bind(order).bind(&id).execute(&mut *tx).await.map_err(|error| format!("reorder custom field: {error}"))?;
                fields.push(
                    sqlx::query_as::<_, ProjectCustomFieldRow>(
                        "SELECT * FROM project_custom_fields WHERE id = ?",
                    )
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|error| format!("read reordered custom field: {error}"))?,
                );
            }
            ReorderReceipt {
                custom_fields: fields,
                ..ReorderReceipt::default()
            }
        }
        ReorderItem::CustomFieldOption {
            id,
            field_id,
            expected_order,
        } => {
            check_schema_sibling_ids(
                &mut tx,
                "project_custom_field_options",
                "field_id",
                field_id,
            )
            .await?;
            let rows: Vec<(String, i64)> = sqlx::query_as("SELECT option.id, option.sort_order FROM project_custom_field_options AS option JOIN project_custom_fields AS field ON field.id = option.field_id WHERE field.project_id = ? AND field.id = ? ORDER BY option.sort_order, option.name, option.id LIMIT ?")
                .bind(&request.project_id).bind(field_id).bind(MAX_SIBLINGS + 1).fetch_all(&mut *tx).await.map_err(|error| format!("read custom field option order: {error}"))?;
            let changes = integer_order_changes(rows, id, *expected_order, request.direction)?;
            check_schema_result_size(&mut tx, "project_custom_field_options", &changes).await?;
            let mut options = Vec::new();
            for (id, order) in changes {
                sqlx::query("UPDATE project_custom_field_options SET sort_order = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
                    .bind(order).bind(&id).execute(&mut *tx).await.map_err(|error| format!("reorder custom field option: {error}"))?;
                options.push(
                    sqlx::query_as::<_, ProjectCustomFieldOptionRow>(
                        "SELECT * FROM project_custom_field_options WHERE id = ?",
                    )
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|error| format!("read reordered custom field option: {error}"))?,
                );
            }
            ReorderReceipt {
                custom_field_options: options,
                ..ReorderReceipt::default()
            }
        }
    };
    let response = serde_json::to_string(&receipt)
        .map_err(|error| format!("encode project reorder receipt: {error}"))?;
    if response.len() > MAX_RECEIPT_BYTES {
        return Err("project reorder result exceeds the byte limit".to_string());
    }
    sqlx::query("UPDATE project_reorder_receipts SET response_json = ? WHERE operation_id = ?")
        .bind(response)
        .bind(&request.operation_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("save project reorder receipt: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit project reorder: {error}"))?;
    Ok(receipt.into())
}

async fn task_placement_changes(
    tx: &mut Transaction<'_, Sqlite>,
    request: &ProjectReorderRequest,
    id: &str,
    axis: TaskOrderAxis,
    group_id: &str,
    parent_task_id: Option<&str>,
    expected_order: f64,
) -> Result<Vec<(String, f64)>, String> {
    if !expected_order.is_finite() {
        return Err("task order must be finite".to_string());
    }
    let (group_column, order_column) = axis.columns();
    if matches!(axis, TaskOrderAxis::Status) && parent_task_id.is_some() {
        return Err("subtasks use their parent ordering lane".to_string());
    }
    let current_order: Option<f64> = sqlx::query_scalar(&format!(
        "SELECT {order_column} FROM project_tasks WHERE id = ? AND project_id = ?
         AND {group_column} = ? AND parent_task_id IS ? AND archived_at IS NULL",
    ))
    .bind(id)
    .bind(&request.project_id)
    .bind(group_id)
    .bind(parent_task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| format!("validate selected task placement: {error}"))?;
    let current_order =
        current_order.ok_or("reordered item no longer belongs to the selected group")?;
    if current_order != expected_order {
        return Err("item order changed since selection".to_string());
    }
    let (records, maximum_id_bytes): (i64, i64) = sqlx::query_as(&format!(
        "SELECT count(*), coalesce(max(length(CAST(id AS BLOB))), 0) FROM (
            SELECT id FROM project_tasks WHERE project_id = ? AND (? OR {group_column} = ?)
            AND parent_task_id IS ? AND archived_at IS NULL LIMIT ?)",
    ))
    .bind(&request.project_id)
    .bind(parent_task_id.is_some())
    .bind(group_id)
    .bind(parent_task_id)
    .bind(MAX_SIBLINGS + 1)
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| format!("measure canonical task siblings: {error}"))?;
    if records > MAX_SIBLINGS || maximum_id_bytes > MAX_ID_BYTES {
        return Err("project reorder exceeds the sibling identity limit".to_string());
    }
    let rows: Vec<(String, f64)> = sqlx::query_as(&format!(
        "SELECT id, {order_column} FROM project_tasks WHERE project_id = ? AND (? OR {group_column} = ?)
         AND parent_task_id IS ? AND archived_at IS NULL ORDER BY {order_column}, created_at, id LIMIT ?",
    )).bind(&request.project_id).bind(parent_task_id.is_some()).bind(group_id).bind(parent_task_id).bind(MAX_SIBLINGS + 1)
        .fetch_all(&mut **tx).await.map_err(|error| format!("read canonical task siblings: {error}"))?;
    let Some((index, target)) = adjacent_indices(&rows, id, expected_order, request.direction)?
    else {
        return Ok(Vec::new());
    };
    if rows.iter().any(|(_, order)| !order.is_finite()) {
        return Err("canonical task order is not finite".to_string());
    }
    if !rows.windows(2).any(|pair| pair[0].1 == pair[1].1) {
        return Ok(vec![
            (rows[index].0.clone(), rows[target].1),
            (rows[target].0.clone(), rows[index].1),
        ]);
    }
    let mut reordered = rows;
    reordered.swap(index, target);
    Ok(reordered
        .into_iter()
        .enumerate()
        .filter_map(|(index, (id, old))| {
            let next = (index as i64 + 1) * ORDER_SPACING;
            (old != next as f64).then_some((id, next as f64))
        })
        .collect())
}

/// Check variable-size values before reading complete rows into the receipt.
async fn check_schema_result_size(
    tx: &mut Transaction<'_, Sqlite>,
    table: &'static str,
    changes: &[(String, i64)],
) -> Result<(), String> {
    let ids = serde_json::to_string(&changes.iter().map(|(id, _)| id).collect::<Vec<_>>())
        .map_err(|error| format!("encode reordered schema identities: {error}"))?;
    let owner_bytes = if table == "project_custom_fields" {
        "length(CAST(project_id AS BLOB)) + length(CAST(field_type AS BLOB))"
    } else {
        "length(CAST(field_id AS BLOB))"
    };
    let bytes: i64 = sqlx::query_scalar(&format!("SELECT coalesce(sum(length(CAST(name AS BLOB)) + length(CAST(id AS BLOB)) + length(CAST(created_at AS BLOB)) + length(CAST(updated_at AS BLOB)) + {owner_bytes}), 0) FROM {table} WHERE id IN (SELECT value FROM json_each(?))"))
        .bind(ids).fetch_one(&mut **tx).await.map_err(|error| format!("measure reordered schema result: {error}"))?;
    if bytes > (MAX_RECEIPT_BYTES / 6) as i64 {
        return Err("project reorder result exceeds the byte limit".to_string());
    }
    Ok(())
}

/// Bound identity materialization before collecting a complete schema sibling order.
async fn check_schema_sibling_ids(
    tx: &mut Transaction<'_, Sqlite>,
    table: &'static str,
    parent_column: &'static str,
    parent_id: &str,
) -> Result<(), String> {
    let (records, maximum_id_bytes): (i64, i64) = sqlx::query_as(&format!(
        "SELECT count(*), coalesce(max(length(CAST(id AS BLOB))), 0) FROM (
            SELECT id FROM {table} WHERE {parent_column} = ? LIMIT ?)",
    ))
    .bind(parent_id)
    .bind(MAX_SIBLINGS + 1)
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| format!("measure canonical schema siblings: {error}"))?;
    if records > MAX_SIBLINGS || maximum_id_bytes > MAX_ID_BYTES {
        return Err("project reorder exceeds the sibling identity limit".to_string());
    }
    Ok(())
}

fn integer_order_changes(
    mut rows: Vec<(String, i64)>,
    id: &str,
    expected: i64,
    direction: i8,
) -> Result<Vec<(String, i64)>, String> {
    let Some((index, target)) = adjacent_indices(&rows, id, expected, direction)? else {
        return Ok(Vec::new());
    };
    if !rows.windows(2).any(|pair| pair[0].1 == pair[1].1) {
        return Ok(vec![
            (rows[index].0.clone(), rows[target].1),
            (rows[target].0.clone(), rows[index].1),
        ]);
    }
    rows.swap(index, target);
    Ok(rows
        .into_iter()
        .enumerate()
        .filter_map(|(index, (id, old))| {
            let next = (index as i64 + 1) * ORDER_SPACING;
            (old != next).then_some((id, next))
        })
        .collect())
}

fn adjacent_indices<T: PartialEq>(
    rows: &[(String, T)],
    id: &str,
    expected: T,
    direction: i8,
) -> Result<Option<(usize, usize)>, String> {
    if rows.len() > MAX_SIBLINGS as usize {
        return Err("project reorder exceeds the sibling limit".to_string());
    }
    let index = rows
        .iter()
        .position(|(row_id, _)| row_id == id)
        .ok_or("reordered item no longer belongs to the selected group")?;
    if rows[index].1 != expected {
        return Err("item order changed since selection".to_string());
    }
    Ok(index
        .checked_add_signed(isize::from(direction))
        .filter(|target| *target < rows.len())
        .map(|target| (index, target)))
}
