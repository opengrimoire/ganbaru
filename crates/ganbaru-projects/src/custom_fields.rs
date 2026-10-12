use super::history::{bool_to_string, insert_task_field_change_event};
use super::models::{
    ProjectCustomFieldCreate, ProjectCustomFieldOptionCreate, ProjectCustomFieldOptionRow,
    ProjectCustomFieldOptionUpdate, ProjectCustomFieldOptionValueRow, ProjectCustomFieldRow,
    ProjectCustomFieldUpdate, ProjectCustomFieldValueRow, ProjectCustomFieldValueUpdate,
    ProjectMutationRemoval, ProjectsMutationRows,
};
use super::mutations::{
    custom_field_mutation, custom_field_option_mutation, ensure_project_exists_in_pool,
    latest_task_change_events,
};
use super::validation::{
    require_non_empty, validate_custom_field_create, validate_custom_field_option_create,
    validate_custom_field_option_update, validate_custom_field_update,
    validate_custom_field_value_update, validate_date,
};
use sqlx::SqlitePool;
use std::collections::HashSet;

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
    if field_type != "select" && field_type != "multi_select" && field_type != "status" {
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
             WHERE task_id = ? AND field_id = ?",
        )
        .bind(task_id)
        .bind(&field.id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load custom field text value: {e}")),
        "number" => {
            let value = sqlx::query_scalar::<_, f64>(
                "SELECT number_value
                 FROM project_custom_field_values
                 WHERE task_id = ? AND field_id = ?",
            )
            .bind(task_id)
            .bind(&field.id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load custom field number value: {e}"))?;
            Ok(value.map(|number| {
                if number.fract() == 0.0 {
                    format!("{number:.0}")
                } else {
                    number.to_string()
                }
            }))
        }
        "date" => sqlx::query_scalar::<_, String>(
            "SELECT date_value
             FROM project_custom_field_values
             WHERE task_id = ? AND field_id = ?",
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
                 WHERE task_id = ? AND field_id = ?",
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
                "SELECT opt.name
                 FROM project_custom_field_option_values val
                 JOIN project_custom_field_options opt ON opt.id = val.option_id
                 WHERE val.task_id = ? AND val.field_id = ?
                 ORDER BY opt.sort_order ASC, opt.name ASC",
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

async fn upsert_scalar_custom_field_value(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    field_id: &str,
    text_value: Option<&str>,
    number_value: Option<f64>,
    date_value: Option<&str>,
    checkbox_value: Option<i64>,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO project_custom_field_values (
            task_id, field_id, text_value, number_value, date_value, checkbox_value
         )
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(task_id, field_id) DO UPDATE SET
            text_value = excluded.text_value,
            number_value = excluded.number_value,
            date_value = excluded.date_value,
            checkbox_value = excluded.checkbox_value,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(task_id)
    .bind(field_id)
    .bind(text_value)
    .bind(number_value)
    .bind(date_value)
    .bind(checkbox_value)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("update custom field value: {e}"))?;
    Ok(())
}

pub(crate) async fn update_custom_field_value_with_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    value: &ProjectCustomFieldValueUpdate,
) -> Result<(), String> {
    let field = custom_field_for_task_value(tx, &value.task_id, &value.field_id).await?;
    ensure_custom_field_value_payload_matches_type(&field.field_type, value)?;
    let old_value = custom_field_value_label(tx, &value.task_id, &field).await?;
    sqlx::query("DELETE FROM project_custom_field_values WHERE task_id = ? AND field_id = ?")
        .bind(value.task_id.trim())
        .bind(value.field_id.trim())
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear custom field value: {e}"))?;
    sqlx::query(
        "DELETE FROM project_custom_field_option_values WHERE task_id = ? AND field_id = ?",
    )
    .bind(value.task_id.trim())
    .bind(value.field_id.trim())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("clear custom field option values: {e}"))?;

    match field.field_type.as_str() {
        "text" | "url" | "phone" | "email" | "person" | "files" => {
            if let Some(text) = value.text_value.as_ref().map(|text| text.trim()) {
                if !text.is_empty() {
                    upsert_scalar_custom_field_value(
                        tx,
                        value.task_id.trim(),
                        value.field_id.trim(),
                        Some(text),
                        None,
                        None,
                        None,
                    )
                    .await?;
                }
            }
        }
        "number" => {
            if let Some(number) = value.number_value {
                upsert_scalar_custom_field_value(
                    tx,
                    value.task_id.trim(),
                    value.field_id.trim(),
                    None,
                    Some(number),
                    None,
                    None,
                )
                .await?;
            }
        }
        "date" => {
            if let Some(date) = value.date_value.as_ref().map(|date| date.trim()) {
                if !date.is_empty() {
                    validate_date(date, "date_value")?;
                    upsert_scalar_custom_field_value(
                        tx,
                        value.task_id.trim(),
                        value.field_id.trim(),
                        None,
                        None,
                        Some(date),
                        None,
                    )
                    .await?;
                }
            }
        }
        "checkbox" => {
            if let Some(checked) = value.checkbox_value {
                upsert_scalar_custom_field_value(
                    tx,
                    value.task_id.trim(),
                    value.field_id.trim(),
                    None,
                    None,
                    None,
                    Some(if checked { 1 } else { 0 }),
                )
                .await?;
            }
        }
        "select" | "multi_select" | "status" => {
            let option_ids =
                ensure_options_belong_to_field(tx, &field.id, &value.option_ids).await?;
            for option_id in option_ids {
                sqlx::query(
                    "INSERT INTO project_custom_field_option_values (task_id, field_id, option_id)
                     VALUES (?, ?, ?)",
                )
                .bind(value.task_id.trim())
                .bind(value.field_id.trim())
                .bind(option_id)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("update custom field option value: {e}"))?;
            }
        }
        _ => return Err("unsupported custom field type".to_string()),
    }

    let new_value = custom_field_value_label(tx, &value.task_id, &field).await?;
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

pub(crate) async fn delete_custom_field_with_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    field_id: &str,
) -> Result<(), String> {
    let field = custom_field_by_id(tx, field_id).await?;
    let task_ids = sqlx::query_scalar::<_, String>(
        "SELECT task_id FROM project_custom_field_values WHERE field_id = ?
         UNION
         SELECT task_id FROM project_custom_field_option_values WHERE field_id = ?",
    )
    .bind(field_id)
    .bind(field_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load custom field task values: {e}"))?;
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

pub(crate) async fn delete_custom_field_option_with_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    option_id: &str,
) -> Result<(), String> {
    let (_option, field) = custom_field_option_with_field(tx, option_id).await?;
    let task_ids = sqlx::query_scalar::<_, String>(
        "SELECT task_id
         FROM project_custom_field_option_values
         WHERE option_id = ?
         ORDER BY created_at ASC",
    )
    .bind(option_id)
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
    Ok(())
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
    ensure_project_exists_in_pool(pool, field.project_id.trim()).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin custom field creation: {e}"))?;
    if let Some(source_id) = field.duplicate_source_id.as_deref() {
        let source = custom_field_by_id(&mut tx, source_id).await?;
        if source.project_id != field.project_id.trim()
            || source.field_type != field.field_type.trim()
        {
            return Err(
                "duplicate property must match the source project and field type".to_string(),
            );
        }
    }
    sqlx::query(
        "INSERT INTO project_custom_fields (id, project_id, name, field_type, sort_order, icon)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(field.id.trim())
    .bind(field.project_id.trim())
    .bind(field.name.trim())
    .bind(field.field_type.trim())
    .bind(field.sort_order)
    .bind(field.icon.as_deref().map(str::trim))
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project custom field: {e}"))?;
    if let Some(source_id) = field.duplicate_source_id.as_deref() {
        sqlx::query("INSERT INTO project_custom_field_options (id, field_id, name, sort_order)
            SELECT lower(hex(randomblob(16))), ?, name, sort_order FROM project_custom_field_options WHERE field_id = ?")
            .bind(field.id.trim()).bind(source_id.trim()).execute(&mut *tx).await
            .map_err(|e| format!("copy duplicate property options: {e}"))?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit custom field creation: {e}"))?;
    let mut mutation = custom_field_mutation(pool, field.id.trim()).await?;
    mutation.custom_field_options = sqlx::query_as::<_, ProjectCustomFieldOptionRow>(
        "SELECT * FROM project_custom_field_options WHERE field_id = ? ORDER BY sort_order, id",
    )
    .bind(field.id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load created property options: {e}"))?;
    Ok(mutation)
}

pub async fn update_custom_field(
    pool: &SqlitePool,
    field: ProjectCustomFieldUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_custom_field_update(&field)?;
    let result = sqlx::query(
        "UPDATE project_custom_fields
         SET name = ?,
             sort_order = ?,
             icon = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(field.name.trim())
    .bind(field.sort_order)
    .bind(field.icon.as_deref().map(str::trim))
    .bind(field.id.trim())
    .execute(pool)
    .await
    .map_err(|e| format!("update project custom field: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project custom field not found".to_string());
    }
    custom_field_mutation(pool, field.id.trim()).await
}

pub async fn delete_custom_field(
    pool: &SqlitePool,
    field_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&field_id, "field_id")?;
    let task_ids = sqlx::query_scalar::<_, String>(
        "SELECT task_id FROM project_custom_field_values WHERE field_id = ?
         UNION
         SELECT task_id FROM project_custom_field_option_values WHERE field_id = ?",
    )
    .bind(field_id.trim())
    .bind(field_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load tasks affected by custom field deletion: {e}"))?;
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
    ensure_custom_field_accepts_options_in_pool(pool, option.field_id.trim()).await?;
    sqlx::query(
        "INSERT INTO project_custom_field_options (id, field_id, name, sort_order)
         VALUES (?, ?, ?, ?)",
    )
    .bind(option.id.trim())
    .bind(option.field_id.trim())
    .bind(option.name.trim())
    .bind(option.sort_order)
    .execute(pool)
    .await
    .map_err(|e| format!("create project custom field option: {e}"))?;
    custom_field_option_mutation(pool, option.id.trim()).await
}

pub async fn update_custom_field_option(
    pool: &SqlitePool,
    option: ProjectCustomFieldOptionUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_custom_field_option_update(&option)?;
    let result = sqlx::query(
        "UPDATE project_custom_field_options
         SET name = ?,
             sort_order = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(option.name.trim())
    .bind(option.sort_order)
    .bind(option.id.trim())
    .execute(pool)
    .await
    .map_err(|e| format!("update project custom field option: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project custom field option not found".to_string());
    }
    custom_field_option_mutation(pool, option.id.trim()).await
}

pub async fn delete_custom_field_option(
    pool: &SqlitePool,
    option_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&option_id, "option_id")?;
    let task_ids = sqlx::query_scalar::<_, String>(
        "SELECT task_id FROM project_custom_field_option_values WHERE option_id = ?",
    )
    .bind(option_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load tasks affected by custom field option deletion: {e}"))?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    delete_custom_field_option_with_history(&mut tx, option_id.trim()).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    for task_id in task_ids {
        mutation
            .task_change_events
            .extend(latest_task_change_events(pool, &task_id).await?);
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
        "SELECT * FROM project_custom_field_option_values WHERE task_id = ? AND field_id = ?",
    )
    .bind(value.task_id.trim())
    .bind(value.field_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load custom field option value mutation result: {e}"))?;
    mutation.task_change_events = latest_task_change_events(pool, value.task_id.trim()).await?;
    Ok(mutation)
}
