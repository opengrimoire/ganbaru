use sqlx::SqlitePool;

use super::history::{
    current_timestamp, insert_task_change_event, insert_task_update_change_events,
    status_is_terminal,
};
use super::models::{
    ProjectChecklistItemCreate, ProjectChecklistItemUpdate, ProjectMutationRemoval,
    ProjectTaskCreate, ProjectTaskRow, ProjectTaskUpdate, ProjectsMutationRows,
};
use super::mutations::{
    checklist_item_mutation, ensure_parent_task_matches_project, ensure_priority_is_live,
    ensure_priority_matches_project, ensure_section_and_status_match_project,
    ensure_status_is_live, ensure_task_exists, ensure_task_has_no_children,
    latest_task_change_events, task_id_for_checklist_item, task_mutation, task_section_order,
    task_status_order,
};
use super::order::{OrderGroup, OrderPlacement};
use super::validation::{
    require_non_empty, validate_checklist_item_create, validate_checklist_item_update,
    validate_task_create, validate_task_update,
};

fn checklist_order(task_id: &str) -> OrderGroup<'_> {
    OrderGroup::new("project_checklist_items", "order_key").filter("task_id", Some(task_id))
}

/// Order key of an updated task on one axis. A named placement wins; a task that changes group goes to its end;
/// otherwise the task keeps `current_key`.
async fn task_order_key(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    order: OrderGroup<'_>,
    task_id: &str,
    placement: Option<&OrderPlacement>,
    group_changed: bool,
    current_key: String,
) -> Result<String, String> {
    let placement = match placement {
        Some(placement) => placement,
        None if group_changed => &OrderPlacement::End,
        None => return Ok(current_key),
    };
    Ok(order.place(tx, task_id, placement).await?.into_string())
}

pub async fn create_task(
    pool: &SqlitePool,
    task: ProjectTaskCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_task_create(&task)?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    ensure_section_and_status_match_project(
        &mut tx,
        &task.project_id,
        &task.section_id,
        &task.status_id,
    )
    .await?;
    ensure_status_is_live(&mut tx, &task.status_id).await?;
    if let Some(parent_id) = &task.parent_task_id {
        ensure_parent_task_matches_project(&mut tx, &task.project_id, parent_id).await?;
    }
    let parent_task_id = task.parent_task_id.as_deref();
    let section_order_key = task_section_order(&task.project_id, &task.section_id, parent_task_id)
        .place(&mut tx, &task.id, &OrderPlacement::End)
        .await?;
    let status_order_key = task_status_order(&task.project_id, &task.status_id, parent_task_id)
        .place(&mut tx, &task.id, &OrderPlacement::End)
        .await?;
    sqlx::query(
        "INSERT INTO project_tasks (
            id, project_id, section_id, status_id, parent_task_id, title,
            section_order_key, status_order_key
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&task.id)
    .bind(&task.project_id)
    .bind(&task.section_id)
    .bind(&task.status_id)
    .bind(&task.parent_task_id)
    .bind(task.title.trim())
    .bind(section_order_key.as_str())
    .bind(status_order_key.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project task: {e}"))?;
    insert_task_change_event(&mut tx, &task.id, "created", None, None, None).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    task_mutation(pool, &task.id).await
}

pub async fn update_task(
    pool: &SqlitePool,
    task: ProjectTaskUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_task_update(&task)?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let previous_task =
        sqlx::query_as::<_, ProjectTaskRow>("SELECT * FROM project_tasks WHERE id = ?")
            .bind(&task.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| format!("load project task: {e}"))?
            .ok_or_else(|| "project task not found".to_string())?;
    if task
        .expected_revision
        .is_some_and(|expected| expected != previous_task.revision)
    {
        return Err(task_conflict(&task.id));
    }
    let project_id = previous_task.project_id.clone();
    ensure_section_and_status_match_project(
        &mut tx,
        &project_id,
        &task.section_id,
        &task.status_id,
    )
    .await?;
    ensure_priority_matches_project(&mut tx, &project_id, &task.priority).await?;
    if task.status_id != previous_task.status_id {
        ensure_status_is_live(&mut tx, &task.status_id).await?;
    }
    if task.priority != previous_task.priority {
        ensure_priority_is_live(&mut tx, &project_id, &task.priority).await?;
    }
    if let Some(parent_id) = &task.parent_task_id {
        ensure_parent_task_matches_project(&mut tx, &project_id, parent_id).await?;
        if parent_id == &task.id {
            return Err("task cannot be its own parent".to_string());
        }
        ensure_task_has_no_children(&mut tx, &task.id).await?;
    }
    let parent_task_id = task.parent_task_id.as_deref();
    let parent_changed = task.parent_task_id != previous_task.parent_task_id;
    let section_order_key = task_order_key(
        &mut tx,
        task_section_order(&project_id, &task.section_id, parent_task_id),
        &task.id,
        task.section_placement.as_ref(),
        parent_changed || task.section_id != previous_task.section_id,
        previous_task.section_order_key.clone(),
    )
    .await?;
    let status_order_key = task_order_key(
        &mut tx,
        task_status_order(&project_id, &task.status_id, parent_task_id),
        &task.id,
        task.status_placement.as_ref(),
        parent_changed || task.status_id != previous_task.status_id,
        previous_task.status_order_key.clone(),
    )
    .await?;
    let terminal_status = status_is_terminal(&mut tx, &task.status_id).await?;
    let completed_at = if terminal_status {
        Some(current_timestamp(&mut tx).await?)
    } else {
        None
    };
    let updated = sqlx::query(
        "UPDATE project_tasks
         SET section_id = ?,
             status_id = ?,
             parent_task_id = ?,
             title = ?,
             description = ?,
             priority = ?,
             task_type = ?,
             section_order_key = ?,
             status_order_key = ?,
             estimate_minutes = ?,
             due_date = ?,
             due_time = ?,
             due_reminder_minutes = ?,
             start_date = ?,
             start_time = ?,
             start_reminder_minutes = ?,
             target_end_date = ?,
             completed_at = ?,
             archived_at = ?,
             blocker_reason = ?,
             milestone = ?,
             revision = revision + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND revision = ?",
    )
    .bind(&task.section_id)
    .bind(&task.status_id)
    .bind(&task.parent_task_id)
    .bind(task.title.trim())
    .bind(&task.description)
    .bind(&task.priority)
    .bind(&task.task_type)
    .bind(section_order_key)
    .bind(status_order_key)
    .bind(task.estimate_minutes)
    .bind(&task.due_date)
    .bind(&task.due_time)
    .bind(task.due_reminder_minutes)
    .bind(&task.start_date)
    .bind(&task.start_time)
    .bind(task.start_reminder_minutes)
    .bind(&task.target_end_date)
    .bind(&completed_at)
    .bind(&task.archived_at)
    .bind(&task.blocker_reason)
    .bind(if task.milestone { 1_i64 } else { 0_i64 })
    .bind(&task.id)
    .bind(previous_task.revision)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update project task: {e}"))?;
    if updated.rows_affected() != 1 {
        return Err(task_conflict(&task.id));
    }
    insert_task_update_change_events(&mut tx, &previous_task, &task).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    task_mutation(pool, &task.id).await
}

pub async fn create_checklist_item(
    pool: &SqlitePool,
    item: ProjectChecklistItemCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_checklist_item_create(&item)?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    ensure_task_exists(&mut tx, &item.task_id).await?;
    let order_key = checklist_order(&item.task_id)
        .place(&mut tx, &item.id, &item.placement)
        .await?;
    sqlx::query(
        "INSERT INTO project_checklist_items (id, task_id, title, order_key)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&item.id)
    .bind(&item.task_id)
    .bind(item.title.trim())
    .bind(order_key.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project checklist item: {e}"))?;
    insert_task_change_event(
        &mut tx,
        &item.task_id,
        "updated",
        Some("checklist"),
        None,
        None,
    )
    .await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    checklist_item_mutation(pool, &item.id, &item.task_id).await
}

pub async fn update_checklist_item(
    pool: &SqlitePool,
    item: ProjectChecklistItemUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_checklist_item_update(&item)?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let task_id = task_id_for_checklist_item(&mut tx, &item.id).await?;
    let completed_at = if item.completed {
        Some(current_timestamp(&mut tx).await?)
    } else {
        None
    };
    sqlx::query(
        "UPDATE project_checklist_items
         SET title = ?,
             completed_at = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(item.title.trim())
    .bind(&completed_at)
    .bind(&item.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update project checklist item: {e}"))?;
    if let Some(placement) = &item.placement {
        checklist_order(&task_id)
            .move_row(&mut tx, &item.id, placement)
            .await?;
    }
    insert_task_change_event(&mut tx, &task_id, "updated", Some("checklist"), None, None).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    checklist_item_mutation(pool, &item.id, &task_id).await
}

pub async fn delete_checklist_item(
    pool: &SqlitePool,
    item_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&item_id, "item_id")?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let task_id = task_id_for_checklist_item(&mut tx, &item_id).await?;
    sqlx::query("DELETE FROM project_checklist_items WHERE id = ?")
        .bind(&item_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("delete project checklist item: {e}"))?;
    insert_task_change_event(&mut tx, &task_id, "updated", Some("checklist"), None, None).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows {
        task_change_events: latest_task_change_events(pool, &task_id).await?,
        ..ProjectsMutationRows::default()
    };
    mutation
        .removals
        .push(ProjectMutationRemoval::ChecklistItem {
            id: item_id.trim().to_string(),
        });
    Ok(mutation)
}

/// Message prefix the frontend recognizes to reload the task and reapply only its own edit.
pub const TASK_CONFLICT_PREFIX: &str = "Project task conflict:";

fn task_conflict(task_id: &str) -> String {
    format!("{TASK_CONFLICT_PREFIX} task {task_id} changed; reload before retrying")
}
