use std::collections::HashSet;
use std::path::Path;

use sqlx::SqlitePool;

use super::icons::required_icon_byte_size;
use super::keys::priority_key;
use super::models::{
    GroupProjectReorder, ProjectGroupCreate, ProjectGroupReorder, ProjectGroupRow,
    ProjectGroupUpdate, ProjectMutationRemoval, ProjectPriorityCreate, ProjectPriorityUpdate,
    ProjectRow, ProjectSectionCreate, ProjectSectionUpdate, ProjectStatusCreate,
    ProjectStatusUpdate, ProjectTagCreate, ProjectTagUpdate, ProjectsMutationRows,
};
use super::mutations::{
    UniqueName, delete_tag_with_history, delete_unused_priority, delete_unused_status,
    ensure_name_is_unique, ensure_project_exists_in_pool, group_mutation,
    latest_task_change_events, priority_mutation, section_mutation, status_mutation, tag_mutation,
};
use super::order::{OrderGroup, OrderPlacement};
use super::project_commands::project_order;
use super::routine::ROUTINE_GROUP_ID;
use super::validation::{
    require_non_empty, validate_group_create, validate_group_update, validate_priority_create,
    validate_priority_update, validate_section_create, validate_section_update,
    validate_status_create, validate_status_update, validate_tag_create, validate_tag_update,
};

fn group_order() -> OrderGroup<'static> {
    OrderGroup::new("project_groups", "order_key")
}

/// Order of the rows of `table` within one project.
fn project_scoped_order<'a>(table: &'static str, project_id: &'a str) -> OrderGroup<'a> {
    OrderGroup::new(table, "order_key").filter("project_id", Some(project_id))
}

/// Project that owns the row `id` of a project-scoped table.
async fn owning_project_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &'static str,
    id: &str,
    not_found: &str,
) -> Result<String, String> {
    let sql = format!("SELECT project_id FROM {table} WHERE id = ?");
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load {table} project: {e}"))?
        .ok_or_else(|| not_found.to_string())
}

/// Moves an existing row when the update names a placement.
async fn move_if_placed(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    order: OrderGroup<'_>,
    id: &str,
    placement: Option<&OrderPlacement>,
) -> Result<(), String> {
    if let Some(placement) = placement {
        order.move_row(tx, id, placement).await?;
    }
    Ok(())
}

/// Creates a group; an icon file under `vault_root` is sized.
pub async fn create_group(
    pool: &SqlitePool,
    vault_root: &Path,
    group: ProjectGroupCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_group_create(&group)?;
    let icon_byte_size = required_icon_byte_size(vault_root, &group.icon)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project group create: {e}"))?;
    let order_key = group_order()
        .place(&mut tx, &group.id, &group.placement)
        .await?;
    sqlx::query(
        "INSERT INTO project_groups (id, name, icon, icon_byte_size, color, order_key)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&group.id)
    .bind(group.name.trim())
    .bind(group.icon.trim())
    .bind(icon_byte_size)
    .bind(group.color)
    .bind(order_key.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project group: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project group create: {e}"))?;
    group_mutation(pool, &group.id).await
}

/// Updates a group. The built-in Routine group keeps its name; an icon file under `vault_root` is sized.
pub async fn update_group(
    pool: &SqlitePool,
    vault_root: &Path,
    group: ProjectGroupUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_group_update(&group)?;
    let group_name = if group.id == ROUTINE_GROUP_ID {
        "Routine"
    } else {
        group.name.trim()
    };
    let icon_byte_size = required_icon_byte_size(vault_root, &group.icon)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project group update: {e}"))?;
    let result = sqlx::query(
        "UPDATE project_groups
         SET name = ?,
             icon = ?,
             icon_byte_size = ?,
             color = ?,
             collapsed = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(group_name)
    .bind(group.icon.trim())
    .bind(icon_byte_size)
    .bind(group.color)
    .bind(if group.collapsed { 1_i64 } else { 0_i64 })
    .bind(&group.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update project group: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project group not found".to_string());
    }
    move_if_placed(&mut tx, group_order(), &group.id, group.placement.as_ref()).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project group update: {e}"))?;
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

/// Rewrites the custom sidebar order of every project group in one transaction, changing as few keys as possible.
/// Modified times stay, since a sidebar order is not an edit of the group.
pub async fn reorder_groups(
    pool: &SqlitePool,
    reorder: ProjectGroupReorder,
) -> Result<ProjectsMutationRows, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project group reorder: {e}"))?;
    let current: Vec<String> = sqlx::query_scalar("SELECT id FROM project_groups")
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("load project groups to reorder: {e}"))?;
    let requested = complete_sibling_order(&reorder.group_ids, &current)
        .ok_or_else(|| "group order must list each project group once".to_string())?;
    group_order().reorder(&mut tx, &requested).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project group reorder: {e}"))?;
    let groups = sqlx::query_as::<_, ProjectGroupRow>(
        "SELECT * FROM project_groups ORDER BY order_key ASC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load reordered project groups: {e}"))?;
    Ok(ProjectsMutationRows {
        groups,
        ..ProjectsMutationRows::default()
    })
}

/// Rewrites the custom sidebar order of the projects in one group, including hidden and archived ones, in one
/// transaction. Modified times stay, since a sidebar order is not an edit of the project.
pub async fn reorder_group_projects(
    pool: &SqlitePool,
    reorder: GroupProjectReorder,
) -> Result<ProjectsMutationRows, String> {
    let group_id = reorder.group_id.trim();
    require_non_empty(group_id, "group_id")?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project reorder: {e}"))?;
    let group_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM project_groups WHERE id = ?)")
            .bind(group_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| format!("load project group to reorder: {e}"))?;
    if !group_exists {
        return Err("project group not found".to_string());
    }
    let current: Vec<String> = sqlx::query_scalar("SELECT id FROM projects WHERE group_id = ?")
        .bind(group_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("load projects to reorder: {e}"))?;
    let requested = complete_sibling_order(&reorder.project_ids, &current)
        .ok_or_else(|| "project order must list each project of the group once".to_string())?;
    project_order(group_id).reorder(&mut tx, &requested).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project reorder: {e}"))?;
    let projects = sqlx::query_as::<_, ProjectRow>(
        "SELECT * FROM projects WHERE group_id = ? ORDER BY order_key ASC, id ASC",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load reordered projects: {e}"))?;
    Ok(ProjectsMutationRows {
        projects,
        ..ProjectsMutationRows::default()
    })
}

/// Trims the requested order and accepts it only when it names every current sibling exactly once.
fn complete_sibling_order<'a>(requested: &'a [String], current: &[String]) -> Option<Vec<&'a str>> {
    let requested: Vec<&str> = requested.iter().map(|id| id.trim()).collect();
    let current: HashSet<&str> = current.iter().map(String::as_str).collect();
    let unique: HashSet<&str> = requested.iter().copied().collect();
    (unique.len() == requested.len()
        && requested.len() == current.len()
        && requested.iter().all(|id| current.contains(id)))
    .then_some(requested)
}

pub async fn create_section(
    pool: &SqlitePool,
    section: ProjectSectionCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_section_create(&section)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project section create: {e}"))?;
    let order_key = project_scoped_order("project_sections", &section.project_id)
        .place(&mut tx, &section.id, &section.placement)
        .await?;
    sqlx::query(
        "INSERT INTO project_sections (id, project_id, name, order_key)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&section.id)
    .bind(&section.project_id)
    .bind(section.name.trim())
    .bind(order_key.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project section: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project section create: {e}"))?;
    section_mutation(pool, &section.id).await
}

pub async fn update_section(
    pool: &SqlitePool,
    section: ProjectSectionUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_section_update(&section)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project section update: {e}"))?;
    let project_id = owning_project_id(
        &mut tx,
        "project_sections",
        &section.id,
        "project section not found",
    )
    .await?;
    sqlx::query(
        "UPDATE project_sections
         SET name = ?,
             collapsed = ?,
             hidden_at = ?,
             archived_at = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(section.name.trim())
    .bind(if section.collapsed { 1_i64 } else { 0_i64 })
    .bind(&section.hidden_at)
    .bind(&section.archived_at)
    .bind(&section.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update project section: {e}"))?;
    move_if_placed(
        &mut tx,
        project_scoped_order("project_sections", &project_id),
        &section.id,
        section.placement.as_ref(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project section update: {e}"))?;
    section_mutation(pool, &section.id).await
}

pub async fn create_status(
    pool: &SqlitePool,
    status: ProjectStatusCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_status_create(&status)?;
    ensure_project_exists_in_pool(pool, &status.project_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project status create: {e}"))?;
    let order_key = project_scoped_order("project_statuses", &status.project_id)
        .place(&mut tx, &status.id, &status.placement)
        .await?;
    sqlx::query(
        "INSERT INTO project_statuses (id, project_id, name, category, color, order_key, terminal)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&status.id)
    .bind(&status.project_id)
    .bind(status.name.trim())
    .bind(&status.category)
    .bind(status.color)
    .bind(order_key.as_str())
    .bind(if status.terminal { 1_i64 } else { 0_i64 })
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project status: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project status create: {e}"))?;
    status_mutation(pool, &status.id).await
}

pub async fn update_status(
    pool: &SqlitePool,
    status: ProjectStatusUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_status_update(&status)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project status update: {e}"))?;
    let project_id = owning_project_id(
        &mut tx,
        "project_statuses",
        &status.id,
        "project status not found",
    )
    .await?;
    sqlx::query(
        "UPDATE project_statuses
         SET name = ?,
             category = ?,
             color = ?,
             terminal = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(status.name.trim())
    .bind(&status.category)
    .bind(status.color)
    .bind(if status.terminal { 1_i64 } else { 0_i64 })
    .bind(&status.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update project status: {e}"))?;
    move_if_placed(
        &mut tx,
        project_scoped_order("project_statuses", &project_id),
        &status.id,
        status.placement.as_ref(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project status update: {e}"))?;
    status_mutation(pool, &status.id).await
}

/// Deletes an unused status softly and returns the row with its deletion time.
pub async fn delete_status(
    pool: &SqlitePool,
    status_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&status_id, "status_id")?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    delete_unused_status(&mut tx, status_id.trim()).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    status_mutation(pool, status_id.trim()).await
}

pub async fn create_priority(
    pool: &SqlitePool,
    priority: ProjectPriorityCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_priority_create(&priority)?;
    ensure_project_exists_in_pool(pool, &priority.project_id).await?;
    let project_id = priority.project_id.trim();
    let id = priority.id.trim();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project priority create: {e}"))?;
    let order_key = project_scoped_order("project_priorities", project_id)
        .place(&mut tx, id, &priority.placement)
        .await?;
    sqlx::query(
        "INSERT INTO project_priorities (id, project_id, name, color, order_key, sync_key)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(project_id)
    .bind(priority.name.trim())
    .bind(priority.color)
    .bind(order_key.as_str())
    .bind(priority_key(project_id, id)?)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project priority: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project priority create: {e}"))?;
    priority_mutation(pool, project_id, id).await
}

pub async fn update_priority(
    pool: &SqlitePool,
    priority: ProjectPriorityUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_priority_update(&priority)?;
    let project_id = priority.project_id.trim();
    let id = priority.id.trim();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project priority update: {e}"))?;
    let result = sqlx::query(
        "UPDATE project_priorities
         SET name = ?,
             color = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE project_id = ? AND id = ?",
    )
    .bind(priority.name.trim())
    .bind(priority.color)
    .bind(project_id)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update project priority: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project priority not found".to_string());
    }
    move_if_placed(
        &mut tx,
        project_scoped_order("project_priorities", project_id),
        id,
        priority.placement.as_ref(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project priority update: {e}"))?;
    priority_mutation(pool, project_id, id).await
}

/// Deletes an unused priority softly and returns the row with its deletion time.
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
    priority_mutation(pool, project_id.trim(), priority_id.trim()).await
}

pub async fn create_tag(
    pool: &SqlitePool,
    tag: ProjectTagCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_tag_create(&tag)?;
    let project_id = tag.project_id.trim();
    ensure_project_exists_in_pool(pool, project_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project tag create: {e}"))?;
    ensure_name_is_unique(&mut tx, UniqueName::Tag(project_id), &tag.name, None).await?;
    let order_key = project_scoped_order("project_tags", project_id)
        .place(&mut tx, tag.id.trim(), &tag.placement)
        .await?;
    sqlx::query(
        "INSERT INTO project_tags (id, project_id, name, color, order_key)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(tag.id.trim())
    .bind(project_id)
    .bind(tag.name.trim())
    .bind(tag.color)
    .bind(order_key.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project tag: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project tag create: {e}"))?;
    tag_mutation(pool, tag.id.trim()).await
}

pub async fn update_tag(
    pool: &SqlitePool,
    tag: ProjectTagUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_tag_update(&tag)?;
    let id = tag.id.trim();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project tag update: {e}"))?;
    let project_id =
        owning_project_id(&mut tx, "project_tags", id, "project tag not found").await?;
    ensure_name_is_unique(&mut tx, UniqueName::Tag(&project_id), &tag.name, Some(id)).await?;
    sqlx::query(
        "UPDATE project_tags
         SET name = ?,
             color = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(tag.name.trim())
    .bind(tag.color)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update project tag: {e}"))?;
    move_if_placed(
        &mut tx,
        project_scoped_order("project_tags", &project_id),
        id,
        tag.placement.as_ref(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project tag update: {e}"))?;
    tag_mutation(pool, id).await
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
