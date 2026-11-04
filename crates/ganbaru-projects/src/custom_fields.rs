use super::history::{bool_to_string, insert_task_field_change_event};
use super::keys::custom_field_value_key;
use super::models::{
    ProjectCustomFieldCreate, ProjectCustomFieldOptionCreate, ProjectCustomFieldOptionRow,
    ProjectCustomFieldOptionUpdate, ProjectCustomFieldOptionValueRow, ProjectCustomFieldRow,
    ProjectCustomFieldUpdate, ProjectCustomFieldValueRow, ProjectCustomFieldValueUpdate,
    ProjectMutationRemoval, ProjectsMutationRows,
};
use super::mutations::{
    UniqueName, custom_field_mutation, custom_field_option_mutation, ensure_name_is_unique,
    ensure_project_exists_in_pool, latest_task_change_events,
};
use super::number::FixedPointNumber;
use super::order::OrderGroup;
use super::validation::{
    require_non_empty, validate_custom_field_create, validate_custom_field_option_create,
    validate_custom_field_option_update, validate_custom_field_update,
    validate_custom_field_value_update, validate_date,
};
use sqlx::SqlitePool;
use std::collections::HashSet;

/// Selected options as `(task_id, field_id, option_id)`, read through their value row. An option whose field
/// differs from the value's field is ignored. Callers append a `WHERE` clause over `value`, `selected`, or
/// `field_option` and an order.
pub(crate) const SELECTED_OPTIONS_SQL: &str =
    "SELECT value.task_id, value.field_id, selected.option_id
     FROM project_custom_field_option_values AS selected
     JOIN project_custom_field_values AS value ON value.sync_key = selected.value_key
     JOIN project_custom_field_options AS field_option
        ON field_option.id = selected.option_id AND field_option.field_id = value.field_id";

pub(crate) fn custom_field_order(project_id: &str) -> OrderGroup<'_> {
    OrderGroup::new("project_custom_fields", "order_key").filter("project_id", Some(project_id))
}

pub(crate) fn custom_field_option_order(field_id: &str) -> OrderGroup<'_> {
    OrderGroup::new("project_custom_field_options", "order_key").filter("field_id", Some(field_id))
}

fn field_accepts_options(field_type: &str) -> bool {
    matches!(field_type, "select" | "multi_select" | "status")
}
async fn custom_field_for_task_value(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    field_id: &str,
) -> Result<ProjectCustomFieldRow, String> {
    sqlx::query_as::<_, ProjectCustomFieldRow>(
        "SELECT cf.*
         FROM project_custom_fields cf
         JOIN project_tasks pt ON pt.project_id = cf.project_id
         WHERE pt.id = ? AND cf.id = ?",
    )
    .bind(task_id.trim())
    .bind(field_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load project custom field: {e}"))?
    .ok_or_else(|| "custom field must belong to the task project".to_string())
}

async fn custom_field_by_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    field_id: &str,
) -> Result<ProjectCustomFieldRow, String> {
    sqlx::query_as::<_, ProjectCustomFieldRow>("SELECT * FROM project_custom_fields WHERE id = ?")
        .bind(field_id.trim())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load project custom field: {e}"))?
        .ok_or_else(|| "project custom field not found".to_string())
}

async fn custom_field_option_with_field(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    option_id: &str,
) -> Result<(ProjectCustomFieldOptionRow, ProjectCustomFieldRow), String> {
    let option = sqlx::query_as::<_, ProjectCustomFieldOptionRow>(
        "SELECT * FROM project_custom_field_options WHERE id = ?",
    )
    .bind(option_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load project custom field option: {e}"))?
    .ok_or_else(|| "project custom field option not found".to_string())?;
    let field = custom_field_by_id(tx, &option.field_id).await?;
    Ok((option, field))
}

pub(crate) async fn ensure_custom_field_accepts_options_in_pool(
    pool: &sqlx::SqlitePool,
    field_id: &str,
) -> Result<(), String> {
    let field_type = sqlx::query_scalar::<_, String>(
        "SELECT field_type FROM project_custom_fields WHERE id = ?",
    )
    .bind(field_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load project custom field: {e}"))?
    .ok_or_else(|| "project custom field not found".to_string())?;
    if !field_accepts_options(&field_type) {
        return Err(
            "custom field options require select, multi-select, or status field".to_string(),
        );
    }
    Ok(())
}

async fn ensure_options_belong_to_field(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    field_id: &str,
    option_ids: &[String],
) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut unique_option_ids = Vec::new();
    for option_id in option_ids {
        let trimmed = option_id.trim().to_string();
        if seen.insert(trimmed.clone()) {
            unique_option_ids.push(trimmed);
        }
    }
    for option_id in &unique_option_ids {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM project_custom_field_options
             WHERE id = ? AND field_id = ?",
        )
        .bind(option_id)
        .bind(field_id.trim())
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("load project custom field option: {e}"))?;
        if exists == 0 {
            return Err("custom field option must belong to the field".to_string());
        }
    }
    Ok(unique_option_ids)
}

fn custom_field_history_name(field: &ProjectCustomFieldRow) -> String {
    format!("custom_field:{}", field.name)
}

async fn custom_field_value_label(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    field: &ProjectCustomFieldRow,
) -> Result<Option<String>, String> {
    match field.field_type.as_str() {
        "text" | "url" | "phone" | "email" | "person" | "files" => sqlx::query_scalar::<_, String>(
            "SELECT text_value
             FROM project_custom_field_values
             WHERE task_id = ? AND field_id = ? AND text_value IS NOT NULL",
        )
        .bind(task_id)
        .bind(&field.id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load custom field text value: {e}")),
        "number" => {
            let value = sqlx::query_scalar::<_, i64>(
                "SELECT number_value
                 FROM project_custom_field_values
                 WHERE task_id = ? AND field_id = ? AND number_value IS NOT NULL",
            )
            .bind(task_id)
            .bind(&field.id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load custom field number value: {e}"))?;
            Ok(value.map(|millionths| FixedPointNumber::from_millionths(millionths).to_string()))
        }
        "date" => sqlx::query_scalar::<_, String>(
            "SELECT date_value
             FROM project_custom_field_values
             WHERE task_id = ? AND field_id = ? AND date_value IS NOT NULL",
        )
        .bind(task_id)
        .bind(&field.id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load custom field date value: {e}")),
        "checkbox" => {
            let value = sqlx::query_scalar::<_, i64>(
                "SELECT checkbox_value
                 FROM project_custom_field_values
                 WHERE task_id = ? AND field_id = ? AND checkbox_value IS NOT NULL",
            )
            .bind(task_id)
            .bind(&field.id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load custom field checkbox value: {e}"))?;
            Ok(value.map(|checked| bool_to_string(checked != 0)))
        }
        "select" | "multi_select" | "status" => {
            let option_names = sqlx::query_scalar::<_, String>(
                "SELECT field_option.name
                 FROM project_custom_field_option_values AS selected
                 JOIN project_custom_field_values AS value ON value.sync_key = selected.value_key
                 JOIN project_custom_field_options AS field_option
                    ON field_option.id = selected.option_id AND field_option.field_id = value.field_id
                 WHERE value.task_id = ? AND value.field_id = ?
                 ORDER BY field_option.order_key ASC, field_option.id ASC",
            )
            .bind(task_id)
            .bind(&field.id)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| format!("load custom field option values: {e}"))?;
            if option_names.is_empty() {
                Ok(None)
            } else {
                Ok(Some(option_names.join(", ")))
            }
        }
        _ => Err("unsupported custom field type".to_string()),
    }
}

fn ensure_custom_field_value_payload_matches_type(
    field_type: &str,
    value: &ProjectCustomFieldValueUpdate,
) -> Result<(), String> {
    let has_text = value
        .text_value
        .as_ref()
        .is_some_and(|text| !text.trim().is_empty());
    let has_date = value
        .date_value
        .as_ref()
        .is_some_and(|date| !date.trim().is_empty());
    match field_type {
        "text" | "url" | "phone" | "email" | "person" | "files" => {
            if value.number_value.is_some()
                || value.date_value.is_some()
                || value.checkbox_value.is_some()
                || !value.option_ids.is_empty()
            {
                return Err("text-backed custom fields only accept text values".to_string());
            }
        }
        "number" => {
            if value.text_value.is_some()
                || value.date_value.is_some()
                || value.checkbox_value.is_some()
                || !value.option_ids.is_empty()
            {
                return Err("number custom fields only accept number values".to_string());
            }
        }
        "date" => {
            if value.text_value.is_some()
                || value.number_value.is_some()
                || value.checkbox_value.is_some()
                || !value.option_ids.is_empty()
            {
                return Err("date custom fields only accept date values".to_string());
            }
        }
        "checkbox" => {
            if value.text_value.is_some()
                || value.number_value.is_some()
                || value.date_value.is_some()
                || !value.option_ids.is_empty()
            {
                return Err("checkbox custom fields only accept checkbox values".to_string());
            }
        }
        "select" | "status" => {
            if value.text_value.is_some()
                || value.number_value.is_some()
                || value.date_value.is_some()
                || value.checkbox_value.is_some()
                || value.option_ids.len() > 1
            {
                return Err("select and status custom fields accept one option".to_string());
            }
        }
        "multi_select" => {
            if value.text_value.is_some()
                || value.number_value.is_some()
                || value.date_value.is_some()
                || value.checkbox_value.is_some()
            {
                return Err("multi-select custom fields only accept option values".to_string());
            }
        }
        _ => return Err("unsupported custom field type".to_string()),
    }
    if !matches!(
        field_type,
        "text" | "url" | "phone" | "email" | "person" | "files"
    ) && has_text
    {
        return Err("text_value is not valid for this custom field".to_string());
    }
    if field_type != "date" && has_date {
        return Err("date_value is not valid for this custom field".to_string());
    }
    Ok(())
}

/// The stored form of one task's value for one field. A value row exists only while it holds a scalar or at least
/// one option; an empty value deletes the row.
#[derive(Default)]
struct StoredValue<'a> {
    text: Option<&'a str>,
    number: Option<FixedPointNumber>,
    date: Option<&'a str>,
    checkbox: Option<i64>,
    option_ids: Vec<String>,
}

impl StoredValue<'_> {
    fn is_empty(&self) -> bool {
        self.text.is_none()
            && self.number.is_none()
            && self.date.is_none()
            && self.checkbox.is_none()
            && self.option_ids.is_empty()
    }
}

async fn stored_value<'a>(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    field: &ProjectCustomFieldRow,
    value: &'a ProjectCustomFieldValueUpdate,
) -> Result<StoredValue<'a>, String> {
    let non_empty =
        |text: Option<&'a String>| text.map(|text| text.trim()).filter(|text| !text.is_empty());
    Ok(match field.field_type.as_str() {
        "text" | "url" | "phone" | "email" | "person" | "files" => StoredValue {
            text: non_empty(value.text_value.as_ref()),
            ..StoredValue::default()
        },
        "number" => StoredValue {
            number: value.number_value,
            ..StoredValue::default()
        },
        "date" => {
            let date = non_empty(value.date_value.as_ref());
            if let Some(date) = date {
                validate_date(date, "date_value")?;
            }
            StoredValue {
                date,
                ..StoredValue::default()
            }
        }
        "checkbox" => StoredValue {
            checkbox: value.checkbox_value.map(i64::from),
            ..StoredValue::default()
        },
        "select" | "multi_select" | "status" => StoredValue {
            option_ids: ensure_options_belong_to_field(tx, &field.id, &value.option_ids).await?,
            ..StoredValue::default()
        },
        _ => return Err("unsupported custom field type".to_string()),
    })
}

/// Writes one task's value for one field: an upsert of the value row keyed by its derived `sync_key`, with the
/// selected options replaced in order, or a delete when the value is empty.
async fn write_stored_value(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    field_id: &str,
    stored: &StoredValue<'_>,
) -> Result<(), String> {
    if stored.is_empty() {
        sqlx::query("DELETE FROM project_custom_field_values WHERE task_id = ? AND field_id = ?")
            .bind(task_id)
            .bind(field_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("clear custom field value: {e}"))?;
        return Ok(());
    }
    let sync_key = custom_field_value_key(task_id, field_id)?;
    sqlx::query(
        "INSERT INTO project_custom_field_values (
            task_id, field_id, sync_key, text_value, number_value, date_value, checkbox_value
         )
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(task_id, field_id) DO UPDATE SET
            text_value = excluded.text_value,
            number_value = excluded.number_value,
            date_value = excluded.date_value,
            checkbox_value = excluded.checkbox_value,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(task_id)
    .bind(field_id)
    .bind(&sync_key)
    .bind(stored.text)
    .bind(stored.number.map(FixedPointNumber::millionths))
    .bind(stored.date)
    .bind(stored.checkbox)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("update custom field value: {e}"))?;
    let value_key: String = sqlx::query_scalar(
        "SELECT sync_key FROM project_custom_field_values WHERE task_id = ? AND field_id = ?",
    )
    .bind(task_id)
    .bind(field_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load custom field value key: {e}"))?;
    sqlx::query("DELETE FROM project_custom_field_option_values WHERE value_key = ?")
        .bind(&value_key)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear custom field option values: {e}"))?;
    for (ordinal, option_id) in stored.option_ids.iter().enumerate() {
        let ordinal =
            i64::try_from(ordinal).map_err(|_| "too many selected options".to_string())?;
        sqlx::query(
            "INSERT INTO project_custom_field_option_values (value_key, ordinal, option_id)
             VALUES (?, ?, ?)",
        )
        .bind(&value_key)
        .bind(ordinal)
        .bind(option_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("update custom field option value: {e}"))?;
    }
    Ok(())
}

pub(crate) async fn update_custom_field_value_with_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    value: &ProjectCustomFieldValueUpdate,
) -> Result<(), String> {
    let field = custom_field_for_task_value(tx, &value.task_id, &value.field_id).await?;
    ensure_custom_field_value_payload_matches_type(&field.field_type, value)?;
    let old_value = custom_field_value_label(tx, value.task_id.trim(), &field).await?;
    let stored = stored_value(tx, &field, value).await?;
    write_stored_value(tx, value.task_id.trim(), &field.id, &stored).await?;
    let new_value = custom_field_value_label(tx, value.task_id.trim(), &field).await?;
    if old_value != new_value {
        insert_task_field_change_event(
            tx,
            value.task_id.trim(),
            "updated",
            &custom_field_history_name(&field),
            old_value,
            new_value,
            None,
        )
        .await?;
    }
    Ok(())
}

async fn tasks_with_field_value(
    executor: impl sqlx::SqliteExecutor<'_>,
    field_id: &str,
) -> Result<Vec<String>, String> {
    sqlx::query_scalar::<_, String>(
        "SELECT task_id FROM project_custom_field_values WHERE field_id = ? ORDER BY task_id",
    )
    .bind(field_id)
    .fetch_all(executor)
    .await
    .map_err(|e| format!("load custom field task values: {e}"))
}

pub(crate) async fn delete_custom_field_with_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    field_id: &str,
) -> Result<(), String> {
    let field = custom_field_by_id(tx, field_id).await?;
    let task_ids = tasks_with_field_value(&mut **tx, field_id).await?;
    let mut old_values = Vec::new();
    for task_id in task_ids {
        let old_value = custom_field_value_label(tx, &task_id, &field).await?;
        old_values.push((task_id, old_value));
    }
    sqlx::query("DELETE FROM project_custom_fields WHERE id = ?")
        .bind(field_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("delete project custom field: {e}"))?;
    for (task_id, old_value) in old_values {
        if old_value.is_some() {
            insert_task_field_change_event(
                tx,
                &task_id,
                "updated",
                &custom_field_history_name(&field),
                old_value,
                None,
                None,
            )
            .await?;
        }
    }
    Ok(())
}

/// Deletes an option and the value rows left without any option, returning the tasks whose value row was
/// deleted.
pub(crate) async fn delete_custom_field_option_with_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    option_id: &str,
) -> Result<Vec<String>, String> {
    let (_option, field) = custom_field_option_with_field(tx, option_id).await?;
    let task_ids = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT value.task_id
         FROM project_custom_field_option_values AS selected
         JOIN project_custom_field_values AS value ON value.sync_key = selected.value_key
         WHERE selected.option_id = ? AND value.field_id = ?
         ORDER BY value.task_id",
    )
    .bind(option_id)
    .bind(&field.id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load custom field option tasks: {e}"))?;
    let mut old_values = Vec::new();
    for task_id in task_ids {
        let old_value = custom_field_value_label(tx, &task_id, &field).await?;
        old_values.push((task_id, old_value));
    }
    sqlx::query("DELETE FROM project_custom_field_options WHERE id = ?")
        .bind(option_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("delete project custom field option: {e}"))?;
    let emptied_task_ids = sqlx::query_scalar::<_, String>(
        "DELETE FROM project_custom_field_values
         WHERE field_id = ?
           AND NOT EXISTS (
             SELECT 1 FROM project_custom_field_option_values AS selected
             WHERE selected.value_key = project_custom_field_values.sync_key
           )
         RETURNING task_id",
    )
    .bind(&field.id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("delete emptied custom field values: {e}"))?;
    for (task_id, old_value) in old_values {
        let new_value = custom_field_value_label(tx, &task_id, &field).await?;
        if old_value != new_value {
            insert_task_field_change_event(
                tx,
                &task_id,
                "updated",
                &custom_field_history_name(&field),
                old_value,
                new_value,
                None,
            )
            .await?;
        }
    }
    Ok(emptied_task_ids)
}

pub async fn create_custom_field(
    pool: &SqlitePool,
    field: ProjectCustomFieldCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_custom_field_create(&field)?;
    create_custom_field_in_pool(pool, &field).await
}

/// Create a fresh property, optionally copying only another property's option schema atomically.
pub(crate) async fn create_custom_field_in_pool(
    pool: &sqlx::SqlitePool,
    field: &ProjectCustomFieldCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_custom_field_create(field)?;
    let project_id = field.project_id.trim();
    ensure_project_exists_in_pool(pool, project_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin custom field creation: {e}"))?;
    if let Some(source_id) = field.duplicate_source_id.as_deref() {
        let source = custom_field_by_id(&mut tx, source_id).await?;
        if source.project_id != project_id || source.field_type != field.field_type.trim() {
            return Err(
                "duplicate property must match the source project and field type".to_string(),
            );
        }
    }
    ensure_name_is_unique(
        &mut tx,
        UniqueName::CustomField(project_id),
        field.name.trim(),
        None,
    )
    .await?;
    let order_key = custom_field_order(project_id)
        .place(&mut tx, field.id.trim(), &field.placement)
        .await?;
    sqlx::query(
        "INSERT INTO project_custom_fields (id, project_id, name, field_type, order_key, icon)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(field.id.trim())
    .bind(project_id)
    .bind(field.name.trim())
    .bind(field.field_type.trim())
    .bind(order_key.as_str())
    .bind(field.icon.as_deref().map(str::trim))
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project custom field: {e}"))?;
    if let Some(source_id) = field.duplicate_source_id.as_deref() {
        sqlx::query(
            "INSERT INTO project_custom_field_options (id, field_id, name, order_key)
             SELECT lower(hex(randomblob(16))), ?, name, order_key
             FROM project_custom_field_options
             WHERE field_id = ?",
        )
        .bind(field.id.trim())
        .bind(source_id.trim())
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("copy duplicate property options: {e}"))?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit custom field creation: {e}"))?;
    let mut mutation = custom_field_mutation(pool, field.id.trim()).await?;
    mutation.custom_field_options = sqlx::query_as::<_, ProjectCustomFieldOptionRow>(
        "SELECT * FROM project_custom_field_options WHERE field_id = ? ORDER BY order_key, id",
    )
    .bind(field.id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load created property options: {e}"))?;
    Ok(mutation)
}

/// Updates a field's definition, bumping its revision, and moves it when a placement is given.
pub async fn update_custom_field(
    pool: &SqlitePool,
    field: ProjectCustomFieldUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_custom_field_update(&field)?;
    let field_id = field.id.trim();
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let current = custom_field_by_id(&mut tx, field_id).await?;
    ensure_name_is_unique(
        &mut tx,
        UniqueName::CustomField(&current.project_id),
        field.name.trim(),
        Some(field_id),
    )
    .await?;
    sqlx::query(
        "UPDATE project_custom_fields
         SET name = ?,
             icon = ?,
             revision = revision + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(field.name.trim())
    .bind(field.icon.as_deref().map(str::trim))
    .bind(field_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update project custom field: {e}"))?;
    if let Some(placement) = &field.placement {
        custom_field_order(&current.project_id)
            .move_row(&mut tx, field_id, placement)
            .await?;
    }
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    custom_field_mutation(pool, field_id).await
}

pub async fn delete_custom_field(
    pool: &SqlitePool,
    field_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&field_id, "field_id")?;
    let task_ids = tasks_with_field_value(pool, field_id.trim()).await?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    delete_custom_field_with_history(&mut tx, field_id.trim()).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    for task_id in task_ids {
        mutation
            .task_change_events
            .extend(latest_task_change_events(pool, &task_id).await?);
    }
    mutation.removals.push(ProjectMutationRemoval::CustomField {
        id: field_id.trim().to_string(),
    });
    Ok(mutation)
}

pub async fn create_custom_field_option(
    pool: &SqlitePool,
    option: ProjectCustomFieldOptionCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_custom_field_option_create(&option)?;
    let field_id = option.field_id.trim();
    ensure_custom_field_accepts_options_in_pool(pool, field_id).await?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    ensure_name_is_unique(
        &mut tx,
        UniqueName::CustomFieldOption(field_id),
        option.name.trim(),
        None,
    )
    .await?;
    let order_key = custom_field_option_order(field_id)
        .place(&mut tx, option.id.trim(), &option.placement)
        .await?;
    sqlx::query(
        "INSERT INTO project_custom_field_options (id, field_id, name, order_key)
         VALUES (?, ?, ?, ?)",
    )
    .bind(option.id.trim())
    .bind(field_id)
    .bind(option.name.trim())
    .bind(order_key.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project custom field option: {e}"))?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    custom_field_option_mutation(pool, option.id.trim()).await
}

/// Renames an option, bumping its revision, and moves it when a placement is given.
pub async fn update_custom_field_option(
    pool: &SqlitePool,
    option: ProjectCustomFieldOptionUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_custom_field_option_update(&option)?;
    let option_id = option.id.trim();
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let (current, _field) = custom_field_option_with_field(&mut tx, option_id).await?;
    ensure_name_is_unique(
        &mut tx,
        UniqueName::CustomFieldOption(&current.field_id),
        option.name.trim(),
        Some(option_id),
    )
    .await?;
    sqlx::query(
        "UPDATE project_custom_field_options
         SET name = ?,
             revision = revision + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(option.name.trim())
    .bind(option_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update project custom field option: {e}"))?;
    if let Some(placement) = &option.placement {
        custom_field_option_order(&current.field_id)
            .move_row(&mut tx, option_id, placement)
            .await?;
    }
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    custom_field_option_mutation(pool, option_id).await
}

pub async fn delete_custom_field_option(
    pool: &SqlitePool,
    option_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&option_id, "option_id")?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let (_option, field) = custom_field_option_with_field(&mut tx, option_id.trim()).await?;
    let task_ids = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT value.task_id
         FROM project_custom_field_option_values AS selected
         JOIN project_custom_field_values AS value ON value.sync_key = selected.value_key
         WHERE selected.option_id = ?
         ORDER BY value.task_id",
    )
    .bind(option_id.trim())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| format!("load tasks affected by custom field option deletion: {e}"))?;
    let emptied_task_ids =
        delete_custom_field_option_with_history(&mut tx, option_id.trim()).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    for task_id in task_ids {
        mutation
            .task_change_events
            .extend(latest_task_change_events(pool, &task_id).await?);
    }
    for task_id in emptied_task_ids {
        mutation
            .removals
            .push(ProjectMutationRemoval::CustomFieldValue {
                task_id,
                field_id: field.id.clone(),
            });
    }
    mutation
        .removals
        .push(ProjectMutationRemoval::CustomFieldOption {
            id: option_id.trim().to_string(),
        });
    Ok(mutation)
}

pub async fn update_custom_field_value(
    pool: &SqlitePool,
    value: ProjectCustomFieldValueUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_custom_field_value_update(&value)?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    update_custom_field_value_with_history(&mut tx, &value).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    mutation
        .removals
        .push(ProjectMutationRemoval::CustomFieldValue {
            task_id: value.task_id.trim().to_string(),
            field_id: value.field_id.trim().to_string(),
        });
    if let Some(row) = sqlx::query_as::<_, ProjectCustomFieldValueRow>(
        "SELECT * FROM project_custom_field_values WHERE task_id = ? AND field_id = ?",
    )
    .bind(value.task_id.trim())
    .bind(value.field_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load custom field value mutation result: {e}"))?
    {
        mutation.custom_field_values.push(row);
    }
    mutation.custom_field_option_values = sqlx::query_as::<_, ProjectCustomFieldOptionValueRow>(
        sqlx::AssertSqlSafe(format!(
            "{SELECTED_OPTIONS_SQL} WHERE value.task_id = ? AND value.field_id = ? ORDER BY selected.ordinal"
        )),
    )
    .bind(value.task_id.trim())
    .bind(value.field_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load custom field option value mutation result: {e}"))?;
    mutation.task_change_events = latest_task_change_events(pool, value.task_id.trim()).await?;
    Ok(mutation)
}
