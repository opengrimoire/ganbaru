//! Project views: the ordered views a project lists in its sidebar, with their names and types.

use std::collections::HashSet;

use sqlx::SqlitePool;

use super::models::{
    ProjectMutationRemoval, ProjectViewCreate, ProjectViewRename, ProjectViewReorder,
    ProjectViewRow, ProjectsMutationRows,
};
use super::mutations::ensure_project_exists_in_pool;
use super::validation::{
    SINGLE_PROJECT_VIEW_TYPES, require_non_empty, validate_view_create, validate_view_reorder,
};

const ORDER_SPACING: i64 = 1_000;

/// Views a new project starts with, in sidebar order. Other types are added from the sidebar.
const DEFAULT_VIEW_TYPES: &[&str] = &["dashboard", "list", "notes", "chat"];

/// Adds the default views to new projects inside the caller's transaction.
pub(super) async fn insert_default_views(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_ids: &[&str],
) -> Result<(), String> {
    if project_ids.is_empty() {
        return Ok(());
    }
    let rows = project_ids.iter().flat_map(|project_id| {
        DEFAULT_VIEW_TYPES
            .iter()
            .enumerate()
            .map(move |(index, view_type)| (*project_id, *view_type, index))
    });
    let mut insert = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "INSERT INTO project_views (id, project_id, view_type, sort_order) ",
    );
    insert.push_values(rows, |mut row, (project_id, view_type, index)| {
        row.push_bind(format!("view-{project_id}-{view_type}"))
            .push_bind(project_id)
            .push_bind(view_type)
            .push_bind(order_at(index));
    });
    insert
        .build()
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("create default project views: {e}"))?;
    Ok(())
}

/// Loads every view of every project, in sidebar order.
pub(super) async fn load_all_views(pool: &SqlitePool) -> Result<Vec<ProjectViewRow>, String> {
    sqlx::query_as::<_, ProjectViewRow>(
        "SELECT * FROM project_views ORDER BY project_id ASC, sort_order ASC, id ASC",
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
        "SELECT * FROM project_views WHERE project_id = ? ORDER BY sort_order ASC, id ASC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project views: {e}"))
}

/// Adds a view after the project's last view. Notes and Chat can appear once per project.
pub async fn create_view(
    pool: &SqlitePool,
    view: ProjectViewCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_view_create(&view)?;
    let project_id = view.project_id.trim();
    let view_type = view.view_type.trim();
    ensure_project_exists_in_pool(pool, project_id).await?;
    if SINGLE_PROJECT_VIEW_TYPES.contains(&view_type) {
        let existing: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_views WHERE project_id = ? AND view_type = ?",
        )
        .bind(project_id)
        .bind(view_type)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("check project view type: {e}"))?;
        if existing > 0 {
            return Err(format!("project already has a {view_type} view"));
        }
    }
    sqlx::query(
        "INSERT INTO project_views (id, project_id, view_type, name, sort_order)
         VALUES (?, ?, ?, ?, (
            SELECT COALESCE(MAX(sort_order), 0) + ? FROM project_views WHERE project_id = ?
         ))",
    )
    .bind(view.id.trim())
    .bind(project_id)
    .bind(view_type)
    .bind(normalized_view_name(view.name.as_deref()))
    .bind(ORDER_SPACING)
    .bind(project_id)
    .execute(pool)
    .await
    .map_err(|e| format!("create project view: {e}"))?;
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
    for (index, view_id) in requested.iter().enumerate() {
        sqlx::query(
            "UPDATE project_views
             SET sort_order = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND sort_order <> ?",
        )
        .bind(order_at(index))
        .bind(view_id)
        .bind(order_at(index))
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("reorder project view: {e}"))?;
    }
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

fn order_at(index: usize) -> i64 {
    (index as i64 + 1) * ORDER_SPACING
}
