//! Structural rules for Notes folders and pages that writes check in Rust.
//!
//! The schema keeps only row-local checks and foreign keys, so rules that span
//! rows (folder placement, folder cycles, data source row hierarchy) are
//! enforced here inside the writing transaction.

use sqlx::{Sqlite, Transaction};

/// Deepest folder or page tree any recursive structural query walks.
///
/// Real trees stay far below this bound; it stops a corrupt or adversarial
/// cycle from turning a recursive query into unbounded work.
pub(crate) const MAX_TREE_DEPTH: i64 = 64;

/// Reject a folder parent that would place `folder_id` under itself or one of
/// its descendants.
pub(crate) async fn ensure_folder_parent_acyclic_tx(
    tx: &mut Transaction<'_, Sqlite>,
    folder_id: &str,
    parent_folder_id: Option<&str>,
) -> Result<(), String> {
    let Some(parent_folder_id) = parent_folder_id else {
        return Ok(());
    };
    if parent_folder_id == folder_id {
        return Err("folder cannot be moved under itself".to_string());
    }
    let parent_is_descendant: i64 = sqlx::query_scalar(
        "WITH RECURSIVE descendants(id, depth) AS (
            SELECT id, 1 FROM notes_folders WHERE parent_folder_id = ?
            UNION
            SELECT child.id, descendants.depth + 1
            FROM notes_folders AS child
            JOIN descendants ON child.parent_folder_id = descendants.id
            WHERE descendants.depth < ?
         )
         SELECT EXISTS(SELECT 1 FROM descendants WHERE id = ?)",
    )
    .bind(folder_id)
    .bind(MAX_TREE_DEPTH)
    .bind(parent_folder_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("check notes folder descendants: {e}"))?;
    if parent_is_descendant != 0 {
        return Err("notes folder cannot be moved under its descendant".to_string());
    }
    Ok(())
}

/// Require a page placed in a folder to be a workspace page of the folder's project.
pub(crate) async fn validate_page_folder_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<(), String> {
    let placement: Option<(String, Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT page.parent_type, page.folder_id, page.project_id, folder.project_id
             FROM notes_pages AS page
             LEFT JOIN notes_folders AS folder ON folder.id = page.folder_id
             WHERE page.id = ?",
        )
        .bind(page_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load notes page folder placement: {e}"))?;
    let Some((parent_type, folder_id, page_project_id, folder_project_id)) = placement else {
        return Err("notes page not found".to_string());
    };
    if folder_id.is_none() {
        return Ok(());
    }
    if parent_type != "workspace" {
        return Err("notes folder pages must have a workspace parent".to_string());
    }
    match (page_project_id, folder_project_id) {
        (Some(page_project), Some(folder_project)) if page_project == folder_project => Ok(()),
        _ => Err("notes folder page must belong to the same project".to_string()),
    }
}

/// Count folder pages whose placement breaks the folder rules, for whole-graph restores.
pub(crate) async fn misplaced_folder_page_count_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<i64, String> {
    sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM notes_pages AS page
         LEFT JOIN notes_folders AS folder ON folder.id = page.folder_id
         WHERE page.folder_id IS NOT NULL
           AND (page.parent_type <> 'workspace'
             OR page.project_id IS NULL
             OR folder.project_id IS NOT page.project_id)",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("check notes folder page placement: {e}"))
}

/// Read the project that owns a page, if any.
pub(crate) async fn page_project_id_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<Option<String>, String> {
    let project: Option<Option<String>> =
        sqlx::query_scalar("SELECT project_id FROM notes_pages WHERE id = ?")
            .bind(page_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load notes page project: {e}"))?;
    project.ok_or_else(|| "notes page not found".to_string())
}

/// Require a project row before a page is assigned to it.
pub(crate) async fn require_page_project_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: &str,
) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load notes page project: {e}"))?;
    exists
        .map(|_| ())
        .ok_or_else(|| "project not found".to_string())
}

/// Drop data source row hierarchy links that no longer match the page parent.
///
/// A row page keeps its sub-item links only while it stays a row of the same
/// data source as the linked rows.
pub(crate) async fn detach_row_hierarchy_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "DELETE FROM notes_data_source_row_hierarchy
         WHERE (row_page_id = ?1 OR parent_row_page_id = ?1)
           AND NOT EXISTS (
               SELECT 1 FROM notes_pages AS page
               WHERE page.id = ?1
                 AND page.parent_type = 'data_source_id'
                 AND page.parent_data_source_id = notes_data_source_row_hierarchy.data_source_id
           )",
    )
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("detach notes row hierarchy: {e}"))?;
    Ok(())
}
