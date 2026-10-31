//! Project views: the ordered views a project lists in its sidebar, with their names and types.

use std::collections::HashSet;

use sqlx::SqlitePool;

use super::models::{
    ProjectMutationRemoval, ProjectViewCreate, ProjectViewRename, ProjectViewReorder,
    ProjectViewRow, ProjectsMutationRows,
};
use super::mutations::ensure_project_exists_in_pool;
use super::order::{OrderGroup, OrderPlacement, rank_key};
use super::validation::{
    SINGLE_PROJECT_VIEW_TYPES, require_non_empty, validate_view_create, validate_view_reorder,
};

/// Views a new project starts with, in sidebar order. Other types are added from the sidebar.
const DEFAULT_VIEW_TYPES: &[&str] = &["dashboard", "list", "notes", "chat"];

fn view_order(project_id: &str) -> OrderGroup<'_> {
    OrderGroup::new("project_views", "order_key").filter("project_id", Some(project_id))
}

/// Adds the default views to a new project inside the caller's transaction.
pub(super) async fn insert_default_views(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
) -> Result<(), String> {
    for (index, view_type) in DEFAULT_VIEW_TYPES.iter().enumerate() {
        sqlx::query(
            "INSERT INTO project_views (id, project_id, view_type, order_key) VALUES (?, ?, ?, ?)",
        )
        .bind(format!("view-{project_id}-{view_type}"))
        .bind(project_id)
        .bind(view_type)
        .bind(rank_key(index)?.as_str())
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("create default project views: {e}"))?;
    }
    Ok(())
}

/// Loads every view of every project, in sidebar order.
pub(super) async fn load_all_views(pool: &SqlitePool) -> Result<Vec<ProjectViewRow>, String> {
    sqlx::query_as::<_, ProjectViewRow>(
        "SELECT * FROM project_views ORDER BY project_id ASC, order_key ASC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project views: {e}"))
}

/// Loads the views of one project, in sidebar order.
pub(super) async fn load_project_views(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<Vec<ProjectViewRow>, String> {
    sqlx::query_as::<_, ProjectViewRow>(
        "SELECT * FROM project_views WHERE project_id = ? ORDER BY order_key ASC, id ASC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project views: {e}"))
}

/// Adds a view after the project's last view. A project keeps one Notes and one Chat view locally; concurrent adds
/// from two devices may keep two, which open the same pages and channels.
pub async fn create_view(
    pool: &SqlitePool,
    view: ProjectViewCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_view_create(&view)?;
    let project_id = view.project_id.trim();
    let view_type = view.view_type.trim();
    ensure_project_exists_in_pool(pool, project_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project view create: {e}"))?;
    if SINGLE_PROJECT_VIEW_TYPES.contains(&view_type) {
        let existing: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_views WHERE project_id = ? AND view_type = ?",
        )
        .bind(project_id)
        .bind(view_type)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| format!("check project view type: {e}"))?;
        if existing > 0 {
            return Err(format!("project already has a {view_type} view"));
        }
    }
    let order_key = view_order(project_id)
        .place(&mut tx, view.id.trim(), &OrderPlacement::End)
        .await?;
    sqlx::query(
        "INSERT INTO project_views (id, project_id, view_type, name, order_key) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(view.id.trim())
    .bind(project_id)
    .bind(view_type)
    .bind(normalized_view_name(view.name.as_deref()))
    .bind(order_key.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project view: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project view create: {e}"))?;
    view_mutation(pool, view.id.trim()).await
}

/// Renames a view. A missing or blank name restores the label of its type.
pub async fn rename_view(
    pool: &SqlitePool,
    rename: ProjectViewRename,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&rename.id, "id")?;
    let result = sqlx::query(
        "UPDATE project_views
         SET name = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(normalized_view_name(rename.name.as_deref()))
    .bind(rename.id.trim())
    .execute(pool)
    .await
    .map_err(|e| format!("rename project view: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project view not found".to_string());
    }
    view_mutation(pool, rename.id.trim()).await
}

/// Rewrites the order of a project's views in one transaction. The request must list each view of the project once.
pub async fn reorder_views(
    pool: &SqlitePool,
    reorder: ProjectViewReorder,
) -> Result<ProjectsMutationRows, String> {
    validate_view_reorder(&reorder)?;
    let project_id = reorder.project_id.trim();
    let requested: Vec<&str> = reorder.view_ids.iter().map(|id| id.trim()).collect();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project view reorder: {e}"))?;
    let current: HashSet<String> =
        sqlx::query_scalar("SELECT id FROM project_views WHERE project_id = ?")
            .bind(project_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| format!("load project views to reorder: {e}"))?
            .into_iter()
            .collect();
    let requested_set: HashSet<&str> = requested.iter().copied().collect();
    if requested_set.len() != requested.len()
        || requested.len() != current.len()
        || !requested.iter().all(|id| current.contains(*id))
    {
        return Err("view order must list each view of the project once".to_string());
    }
    view_order(project_id).reorder(&mut tx, &requested).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project view reorder: {e}"))?;
    Ok(ProjectsMutationRows {
        views: load_project_views(pool, project_id).await?,
        ..ProjectsMutationRows::default()
    })
}

/// Removes a view and its preferences. Tasks, pages, and channels stay with the project.
pub async fn delete_view(pool: &SqlitePool, id: &str) -> Result<ProjectsMutationRows, String> {
    require_non_empty(id, "id")?;
    let result = sqlx::query("DELETE FROM project_views WHERE id = ?")
        .bind(id.trim())
        .execute(pool)
        .await
        .map_err(|e| format!("delete project view: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project view not found".to_string());
    }
    let mut mutation = ProjectsMutationRows::default();
    mutation.removals.push(ProjectMutationRemoval::View {
        id: id.trim().to_string(),
    });
    Ok(mutation)
}

async fn view_mutation(pool: &SqlitePool, id: &str) -> Result<ProjectsMutationRows, String> {
    let view = sqlx::query_as::<_, ProjectViewRow>("SELECT * FROM project_views WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("load project view mutation result: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    mutation.views.push(view);
    Ok(mutation)
}

fn normalized_view_name(name: Option<&str>) -> Option<String> {
    name.map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.is_empty())
}
