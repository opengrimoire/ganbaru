use sqlx::SqlitePool;

use super::models::{
    ProjectGroupCreate, ProjectGroupUpdate, ProjectMutationRemoval, ProjectPriorityCreate,
    ProjectPriorityUpdate, ProjectSectionCreate, ProjectSectionUpdate, ProjectStatusCreate,
    ProjectStatusUpdate, ProjectTagCreate, ProjectTagUpdate, ProjectsMutationRows,
};
use super::mutations::{
    delete_tag_with_history, delete_unused_priority, delete_unused_status,
    ensure_project_exists_in_pool, group_mutation, latest_task_change_events, priority_mutation,
    section_mutation, status_mutation, tag_mutation,
};
use super::routine::ROUTINE_GROUP_ID;
use super::validation::{
    require_non_empty, validate_group_create, validate_group_update, validate_priority_create,
    validate_priority_update, validate_section_create, validate_section_update,
    validate_status_create, validate_status_update, validate_tag_create, validate_tag_update,
};

pub async fn create_group(
    pool: &SqlitePool,
    group: ProjectGroupCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_group_create(&group)?;
    sqlx::query(
        "INSERT INTO project_groups (id, name, icon, color, sort_order)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&group.id)
    .bind(group.name.trim())
    .bind(group.icon.trim())
    .bind(group.color)
    .bind(group.sort_order)
    .execute(pool)
    .await
    .map_err(|e| format!("create project group: {e}"))?;
    group_mutation(pool, &group.id).await
}

pub async fn update_group(
    pool: &SqlitePool,
    group: ProjectGroupUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_group_update(&group)?;
    let group_name = if group.id == ROUTINE_GROUP_ID {
        "Routine"
    } else {
        group.name.trim()
    };
    let result = sqlx::query(
        "UPDATE project_groups
         SET name = ?,
             icon = ?,
             color = ?,
             sort_order = ?,
             collapsed = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(group_name)
    .bind(group.icon.trim())
    .bind(group.color)
    .bind(group.sort_order)
    .bind(if group.collapsed { 1_i64 } else { 0_i64 })
    .bind(&group.id)
    .execute(pool)
    .await
    .map_err(|e| format!("update project group: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project group not found".to_string());
    }
    group_mutation(pool, &group.id).await
}

pub async fn delete_group(
    pool: &SqlitePool,
    group_id: String,
) -> Result<ProjectsMutationRows, String> {
    let normalized_group_id = group_id.trim();
    let project_ids = sqlx::query_scalar::<_, String>("SELECT id FROM projects WHERE group_id = ?")
        .bind(normalized_group_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load deleted group projects: {e}"))?;
    delete_project_group(pool, normalized_group_id).await?;
    let mut mutation = ProjectsMutationRows::default();
    mutation.removals.push(ProjectMutationRemoval::Group {
        id: normalized_group_id.to_string(),
    });
    mutation.removals.extend(
        project_ids
            .into_iter()
            .map(|id| ProjectMutationRemoval::Project { id }),
    );
    Ok(mutation)
}

pub(super) async fn delete_project_group(
    pool: &sqlx::SqlitePool,
    group_id: &str,
) -> Result<(), String> {
    let normalized_group_id = group_id.trim();
    require_non_empty(normalized_group_id, "group_id")?;
    if normalized_group_id == ROUTINE_GROUP_ID {
        return Err("built-in Routine group cannot be deleted".to_string());
    }
    let result = sqlx::query("DELETE FROM project_groups WHERE id = ?")
        .bind(normalized_group_id)
        .execute(pool)
        .await
        .map_err(|e| format!("delete project group: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project group not found".to_string());
    }
    Ok(())
}

pub async fn set_group_collapsed(
    pool: &SqlitePool,
    group_id: String,
    collapsed: bool,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&group_id, "group_id")?;
    let result = sqlx::query(
        "UPDATE project_groups
         SET collapsed = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(if collapsed { 1_i64 } else { 0_i64 })
    .bind(&group_id)
    .execute(pool)
    .await
    .map_err(|e| format!("set project group collapsed: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project group not found".to_string());
    }
    group_mutation(pool, group_id.trim()).await
}

pub async fn create_section(
    pool: &SqlitePool,
    section: ProjectSectionCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_section_create(&section)?;
    sqlx::query(
        "INSERT INTO project_sections (id, project_id, name, sort_order)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&section.id)
    .bind(&section.project_id)
    .bind(section.name.trim())
    .bind(section.sort_order)
    .execute(pool)
    .await
    .map_err(|e| format!("create project section: {e}"))?;
    section_mutation(pool, &section.id).await
}

pub async fn update_section(
    pool: &SqlitePool,
    section: ProjectSectionUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_section_update(&section)?;
    let result = sqlx::query(
        "UPDATE project_sections
         SET name = ?,
             sort_order = ?,
             collapsed = ?,
             hidden_at = ?,
             archived_at = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(section.name.trim())
    .bind(section.sort_order)
    .bind(if section.collapsed { 1_i64 } else { 0_i64 })
    .bind(&section.hidden_at)
    .bind(&section.archived_at)
    .bind(&section.id)
    .execute(pool)
    .await
    .map_err(|e| format!("update project section: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project section not found".to_string());
    }
    section_mutation(pool, &section.id).await
}

pub async fn create_status(
    pool: &SqlitePool,
    status: ProjectStatusCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_status_create(&status)?;
    ensure_project_exists_in_pool(pool, &status.project_id).await?;
    sqlx::query(
        "INSERT INTO project_statuses (id, project_id, name, category, color, sort_order, terminal)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&status.id)
    .bind(&status.project_id)
    .bind(status.name.trim())
    .bind(&status.category)
    .bind(status.color)
    .bind(status.sort_order)
    .bind(if status.terminal { 1_i64 } else { 0_i64 })
    .execute(pool)
    .await
    .map_err(|e| format!("create project status: {e}"))?;
    status_mutation(pool, &status.id).await
}

pub async fn update_status(
    pool: &SqlitePool,
    status: ProjectStatusUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_status_update(&status)?;
    let result = sqlx::query(
        "UPDATE project_statuses
         SET name = ?,
             category = ?,
             color = ?,
             sort_order = ?,
             terminal = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(status.name.trim())
    .bind(&status.category)
    .bind(status.color)
    .bind(status.sort_order)
    .bind(if status.terminal { 1_i64 } else { 0_i64 })
    .bind(&status.id)
    .execute(pool)
    .await
    .map_err(|e| format!("update project status: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project status not found".to_string());
    }
    status_mutation(pool, &status.id).await
}

pub async fn delete_status(
    pool: &SqlitePool,
    status_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&status_id, "status_id")?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    delete_unused_status(&mut tx, status_id.trim()).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    mutation.removals.push(ProjectMutationRemoval::Status {
        id: status_id.trim().to_string(),
    });
    Ok(mutation)
}

pub async fn create_priority(
    pool: &SqlitePool,
    priority: ProjectPriorityCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_priority_create(&priority)?;
    ensure_project_exists_in_pool(pool, &priority.project_id).await?;
    sqlx::query(
        "INSERT INTO project_priorities (id, project_id, name, color, sort_order)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(priority.id.trim())
    .bind(&priority.project_id)
    .bind(priority.name.trim())
    .bind(priority.color)
    .bind(priority.sort_order)
    .execute(pool)
    .await
    .map_err(|e| format!("create project priority: {e}"))?;
    priority_mutation(pool, priority.id.trim()).await
}

pub async fn update_priority(
    pool: &SqlitePool,
    priority: ProjectPriorityUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_priority_update(&priority)?;
    let result = sqlx::query(
        "UPDATE project_priorities
         SET name = ?,
             color = ?,
             sort_order = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE project_id = ? AND id = ?",
    )
    .bind(priority.name.trim())
    .bind(priority.color)
    .bind(priority.sort_order)
    .bind(&priority.project_id)
    .bind(priority.id.trim())
    .execute(pool)
    .await
    .map_err(|e| format!("update project priority: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project priority not found".to_string());
    }
    priority_mutation(pool, priority.id.trim()).await
}

pub async fn delete_priority(
    pool: &SqlitePool,
    project_id: String,
    priority_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&project_id, "project_id")?;
    require_non_empty(&priority_id, "priority_id")?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    delete_unused_priority(&mut tx, project_id.trim(), priority_id.trim()).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    mutation.removals.push(ProjectMutationRemoval::Priority {
        id: priority_id.trim().to_string(),
    });
    Ok(mutation)
}

pub async fn create_tag(
    pool: &SqlitePool,
    tag: ProjectTagCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_tag_create(&tag)?;
    ensure_project_exists_in_pool(pool, tag.project_id.trim()).await?;
    sqlx::query(
        "INSERT INTO project_tags (id, project_id, name, color, sort_order)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(tag.id.trim())
    .bind(tag.project_id.trim())
    .bind(tag.name.trim())
    .bind(tag.color)
    .bind(tag.sort_order)
    .execute(pool)
    .await
    .map_err(|e| format!("create project tag: {e}"))?;
    tag_mutation(pool, tag.id.trim()).await
}

pub async fn update_tag(
    pool: &SqlitePool,
    tag: ProjectTagUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_tag_update(&tag)?;
    let result = sqlx::query(
        "UPDATE project_tags
         SET name = ?,
             color = ?,
             sort_order = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(tag.name.trim())
    .bind(tag.color)
    .bind(tag.sort_order)
    .bind(tag.id.trim())
    .execute(pool)
    .await
    .map_err(|e| format!("update project tag: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project tag not found".to_string());
    }
    tag_mutation(pool, tag.id.trim()).await
}

pub async fn delete_tag(pool: &SqlitePool, tag_id: String) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&tag_id, "tag_id")?;
    let task_ids = sqlx::query_scalar::<_, String>(
        "SELECT task_id FROM project_task_tag_links WHERE tag_id = ?",
    )
    .bind(tag_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load tasks affected by tag deletion: {e}"))?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    delete_tag_with_history(&mut tx, tag_id.trim()).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    for task_id in task_ids {
        mutation
            .task_change_events
            .extend(latest_task_change_events(pool, &task_id).await?);
    }
    mutation.removals.push(ProjectMutationRemoval::Tag {
        id: tag_id.trim().to_string(),
    });
    Ok(mutation)
}
