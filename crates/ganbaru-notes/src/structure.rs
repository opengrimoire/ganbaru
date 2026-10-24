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

/// Deepest step count a recursive walk across nested pages and their blocks,
/// databases, and data sources takes.
///
/// Each nested page level adds at most its block nesting, a database, and a
/// data source, so a complete walk of any real tree stays below it; it only
/// stops a corrupt or adversarial cycle.
pub(crate) const MAX_CONTENT_WALK_DEPTH: i64 =
    MAX_TREE_DEPTH * (crate::writes::MAX_BLOCK_DEPTH + 3);

/// SQL expression for the project of the page row that `page` names in the enclosing query.
///
/// Only workspace pages store a project; a nested page (under a page, a block, or a data
/// source row) stores none and belongs to the project of its nearest ancestor that stores
/// one. The walk follows the same containers as the content tree and stops at
/// [`MAX_TREE_DEPTH`], so a corrupt parent cycle resolves to no project instead of looping.
/// `page` is a table alias or table name of `notes_pages` in the enclosing query.
pub(crate) fn page_project_sql(page: &str) -> String {
    format!(
        "(WITH RECURSIVE project_walk(id, project_id, depth) AS (
             SELECT {page}.id, {page}.project_id, 0
             UNION
             SELECT project_container.id, project_container.project_id, project_walk.depth + 1
             FROM project_walk
             JOIN notes_pages AS project_walked ON project_walked.id = project_walk.id
             JOIN notes_pages AS project_container ON project_container.id = COALESCE(
                 project_walked.parent_page_id,
                 (SELECT project_block.page_id FROM notes_blocks AS project_block
                  WHERE project_block.id = project_walked.parent_block_id),
                 (SELECT COALESCE(project_database.parent_page_id, project_database_block.page_id)
                  FROM notes_data_sources AS project_data_source
                  JOIN notes_databases AS project_database
                    ON project_database.id = project_data_source.database_id
                  LEFT JOIN notes_blocks AS project_database_block
                    ON project_database_block.id = project_database.id
                  WHERE project_data_source.id = project_walked.parent_data_source_id)
             )
             WHERE project_walk.project_id IS NULL AND project_walk.depth < {MAX_TREE_DEPTH}
         )
         SELECT project_walk.project_id FROM project_walk
         WHERE project_walk.project_id IS NOT NULL
         ORDER BY project_walk.depth
         LIMIT 1)"
    )
}

/// Column list for a [`crate::models::NotePageRow`] read of the `notes_pages` row named
/// `page`, with `project_id` derived by [`page_project_sql`].
pub(crate) fn page_row_columns(page: &str) -> String {
    format!(
        "{page}.id, {page}.parent_type, {page}.parent_page_id, {page}.parent_block_id,
         {page}.parent_data_source_id, {page}.folder_id, {project} AS project_id, {page}.title,
         {page}.properties, {page}.icon, {page}.cover, {page}.in_trash, {page}.archived,
         {page}.source_provider, {page}.source_object_id, {page}.source_workspace_id,
         {page}.source_last_edited_time, {page}.url, {page}.public_url, {page}.created_time,
         {page}.last_edited_time",
        project = page_project_sql(page)
    )
}

/// Column list for a [`crate::models::NotePageSummaryDto`] read of the `notes_pages` row
/// named `page`, with `project_id` derived by [`page_project_sql`].
pub(crate) fn page_summary_columns(page: &str) -> String {
    format!(
        "{page}.id, {page}.parent_type, {page}.parent_page_id, {page}.parent_block_id,
         {page}.parent_data_source_id, {page}.folder_id, {page}.title, {project} AS project_id,
         {page}.icon, {page}.in_trash, {page}.archived, {page}.created_time,
         {page}.last_edited_time",
        project = page_project_sql(page)
    )
}

/// The project a page stores for its placement: `project_id` for a workspace page and
/// none for a nested page, which derives its project from its ancestors.
pub(crate) fn stored_page_project<'a>(
    parent_type: &str,
    project_id: Option<&'a str>,
) -> Option<&'a str> {
    (parent_type == "workspace").then_some(project_id).flatten()
}

/// Store the project of a page placed at the workspace (`project_id`) or nested (none).
pub(crate) async fn store_page_project_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    project_id: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_pages
         SET project_id = CASE WHEN parent_type = 'workspace' THEN ? END
         WHERE id = ?",
    )
    .bind(project_id)
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("store Notes page project: {e}"))?;
    Ok(())
}

/// The `child_block_id` a page with `parent_type` stores.
///
/// A page nested under a page or a block is placed by its `child_page` block,
/// which shares the page id; workspace, folder, and data source row pages have
/// no such block.
pub(crate) fn nested_page_block_id<'a>(parent_type: &str, page_id: &'a str) -> Option<&'a str> {
    matches!(parent_type, "page_id" | "block_id").then_some(page_id)
}

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

/// A page's parent type, folder, and project, with the folder's project.
type PageFolderPlacement = (String, Option<String>, Option<String>, Option<String>);

/// Require a page placed in a folder to be a workspace page of the folder's project.
pub(crate) async fn validate_page_folder_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<(), String> {
    let placement: Option<PageFolderPlacement> = sqlx::query_as(
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

/// Read the project that owns a page, stored or derived from its ancestors (see
/// [`page_project_sql`]).
pub(crate) async fn page_project_id_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<Option<String>, String> {
    let project: Option<Option<String>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {} FROM notes_pages AS page WHERE page.id = ?",
        page_project_sql("page")
    )))
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
