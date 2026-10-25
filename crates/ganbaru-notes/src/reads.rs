use crate::models::{
    NoteBlockDto, NoteBlockFrontierDto, NoteBlockHydrationRequest, NoteBlockOutlineDto,
    NoteBlockRow, NoteLoadedPage, NotePageBreadcrumbItemDto, NotePageDto, NotePageOpenDto,
    NotePageRow, NotePageSummaryDto, NotePageSummaryWindowDto, NotePageSummaryWindowRequest,
    NotePaginatedBlockList, NoteSidebarPageList, NoteSidebarPagesRequest,
};
use crate::validation::{require_uuid, validate_page_size};
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, QueryBuilder, Sqlite, SqlitePool};
use std::collections::{HashMap, HashSet};

const DEFAULT_PAGE_SIZE: i64 = 50;
const BLOCK_FRONTIER_PARENT_BATCH_SIZE: usize = 400;
/// Most parent block IDs one frontier read accepts. Far above a page's nested parents per level;
/// the frontend sends larger levels in several reads.
pub const MAX_BLOCK_FRONTIER_PARENT_IDS: usize = 2_000;
const SUMMARY_WINDOW_SIZE: i64 = 50;
const MAX_SUMMARY_WINDOW_SIZE: i64 = 100;

#[derive(Deserialize, Serialize)]
struct PageSummaryCursor {
    last_edited_time: String,
    title: String,
    id: String,
}

#[cfg(test)]
pub async fn list_pages(pool: &SqlitePool) -> Result<Vec<NotePageDto>, String> {
    list_pages_by_state(pool, false, Some(false)).await
}

#[cfg(test)]
pub async fn list_trashed_pages(pool: &SqlitePool) -> Result<Vec<NotePageDto>, String> {
    list_pages_by_state(pool, true, None).await
}

pub async fn list_trashed_page_window(
    pool: &SqlitePool,
    request: NotePageSummaryWindowRequest,
) -> Result<NotePageSummaryWindowDto, String> {
    list_page_summary_window(pool, true, None, request).await
}

#[cfg(test)]
pub async fn list_archived_pages(pool: &SqlitePool) -> Result<Vec<NotePageDto>, String> {
    list_pages_by_state(pool, false, Some(true)).await
}

pub async fn list_archived_page_window(
    pool: &SqlitePool,
    request: NotePageSummaryWindowRequest,
) -> Result<NotePageSummaryWindowDto, String> {
    list_page_summary_window(pool, false, Some(true), request).await
}

async fn list_page_summary_window(
    pool: &SqlitePool,
    in_trash: bool,
    archived: Option<bool>,
    request: NotePageSummaryWindowRequest,
) -> Result<NotePageSummaryWindowDto, String> {
    let page_size = request.page_size.unwrap_or(SUMMARY_WINDOW_SIZE);
    if !(1..=MAX_SUMMARY_WINDOW_SIZE).contains(&page_size) {
        return Err(format!(
            "page_size must be between 1 and {MAX_SUMMARY_WINDOW_SIZE}"
        ));
    }
    let search = request.query.unwrap_or_default().trim().to_string();
    let cursor = request
        .cursor
        .map(|value| {
            serde_json::from_str::<PageSummaryCursor>(&value)
                .map_err(|_| "invalid page summary cursor".to_string())
        })
        .transpose()?;
    let mut count = QueryBuilder::<Sqlite>::new(format!(
        "SELECT COUNT(*) FROM notes_pages WHERE {} AND in_trash = ",
        crate::structure::placed_page_sql("notes_pages")
    ));
    count.push_bind(in_trash);
    if let Some(archived) = archived {
        count.push(" AND archived = ").push_bind(archived);
    }
    if !search.is_empty() {
        count
            .push(" AND instr(lower(title), lower(")
            .push_bind(search.clone())
            .push(")) > 0");
    }
    let total_count = count
        .build_query_scalar::<i64>()
        .fetch_one(pool)
        .await
        .map_err(|e| format!("count notes page summary window: {e}"))?;
    let mut query = QueryBuilder::<Sqlite>::new(format!(
        "SELECT {} FROM notes_pages WHERE {} AND in_trash = ",
        crate::workspace_shell::page_projection(),
        crate::structure::placed_page_sql("notes_pages")
    ));
    query.push_bind(in_trash);
    if let Some(archived) = archived {
        query.push(" AND archived = ").push_bind(archived);
    }
    if !search.is_empty() {
        query
            .push(" AND instr(lower(title), lower(")
            .push_bind(search)
            .push(")) > 0");
    }
    if let Some(cursor) = &cursor {
        push_page_summary_cursor(&mut query, cursor);
    }
    query
        .push(" ORDER BY last_edited_time DESC, title COLLATE NOCASE ASC, id ASC LIMIT ")
        .push_bind(page_size + 1);
    let mut pages = query
        .build_query_as::<NotePageSummaryDto>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("list notes page summary window: {e}"))?;
    let has_more = pages.len() > page_size as usize;
    if has_more {
        pages.truncate(page_size as usize);
    }
    let next_cursor = if has_more {
        pages
            .last()
            .map(|page| {
                serde_json::to_string(&PageSummaryCursor {
                    last_edited_time: page.last_edited_time.clone(),
                    title: page.title.clone(),
                    id: page.id.clone(),
                })
                .map_err(|e| format!("serialize page summary cursor: {e}"))
            })
            .transpose()?
    } else {
        None
    };
    Ok(NotePageSummaryWindowDto::new(
        pages,
        total_count,
        next_cursor,
    ))
}

fn push_page_summary_cursor(query: &mut QueryBuilder<Sqlite>, cursor: &PageSummaryCursor) {
    query
        .push(" AND (last_edited_time < ")
        .push_bind(cursor.last_edited_time.clone());
    query
        .push(" OR (last_edited_time = ")
        .push_bind(cursor.last_edited_time.clone());
    query
        .push(" AND (title COLLATE NOCASE > ")
        .push_bind(cursor.title.clone());
    query
        .push(" COLLATE NOCASE OR (title COLLATE NOCASE = ")
        .push_bind(cursor.title.clone());
    query
        .push(" COLLATE NOCASE AND id > ")
        .push_bind(cursor.id.clone())
        .push("))))");
}

pub async fn list_sidebar_pages(
    pool: &SqlitePool,
    request: NoteSidebarPagesRequest,
) -> Result<NoteSidebarPageList, String> {
    let expanded_page_ids = normalize_request_page_ids(request.expanded_page_ids);
    let mut seed_page_ids = normalize_request_page_ids(request.seed_page_ids);
    if let Some(selected_page_id) = request
        .selected_page_id
        .as_deref()
        .and_then(normalize_request_page_id)
        .filter(|page_id| !seed_page_ids.contains(page_id))
    {
        seed_page_ids.push(selected_page_id);
    }

    let mut rows_by_id = HashMap::<String, NotePageSummaryDto>::new();
    push_unique_page_rows(&mut rows_by_id, load_sidebar_root_page_rows(pool).await?);
    push_unique_page_rows(
        &mut rows_by_id,
        load_active_page_rows_by_ids(pool, &seed_page_ids).await?,
    );
    for page_id in &seed_page_ids {
        push_unique_page_rows(
            &mut rows_by_id,
            load_active_ancestor_page_rows(pool, page_id).await?,
        );
    }
    push_unique_page_rows(
        &mut rows_by_id,
        load_active_child_page_rows(pool, &expanded_page_ids).await?,
    );

    let mut rows = rows_by_id.into_values().collect::<Vec<_>>();
    sort_page_rows(&mut rows);
    let loaded_page_ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let page_ids_with_children =
        load_active_parent_page_ids_with_children(pool, &loaded_page_ids).await?;
    let (missing_parent_page_ids, trashed_parent_page_ids) =
        load_unavailable_parent_page_ids(pool, &rows, &loaded_page_ids).await?;

    Ok(NoteSidebarPageList::new(
        rows,
        page_ids_with_children,
        missing_parent_page_ids,
        trashed_parent_page_ids,
    ))
}

pub async fn page_breadcrumb(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<Vec<NotePageBreadcrumbItemDto>, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    let mut crumbs = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = Some(page_id.to_string());
    while let Some(current_page_id) = cursor {
        if !seen.insert(current_page_id.clone()) {
            break;
        }
        let Some(row) = load_page_row_including_trashed(pool, &current_page_id).await? else {
            crumbs.push(NotePageBreadcrumbItemDto::missing(current_page_id));
            break;
        };
        let current = row.id == page_id;
        if current && (row.in_trash != 0 || row.archived != 0) {
            return Err("notes page not found".to_string());
        }
        let parent_page_id = if row.parent_type == "page_id" {
            row.parent_page_id.clone()
        } else {
            None
        };
        let crumb = if row.in_trash != 0 {
            NotePageBreadcrumbItemDto::unavailable(row, "trashed")
        } else if row.archived != 0 {
            NotePageBreadcrumbItemDto::unavailable(row, "archived")
        } else {
            NotePageBreadcrumbItemDto::active(row, current)
        };
        crumbs.push(crumb);
        cursor = parent_page_id;
    }
    crumbs.reverse();
    Ok(crumbs)
}

/// One page in a location's ancestry, read from the outermost page inward.
#[derive(sqlx::FromRow)]
struct LocationPage {
    title: String,
    /// Title of the database whose source holds this page as a row.
    row_database_title: Option<String>,
    folder_id: Option<String>,
    project_id: Option<String>,
}

/// Read where an active page lives, outermost first and ending with its own title: project group, project, folders,
/// then the pages and row databases above it. Names follow the Notes sidebar, so empty page titles stay empty for the
/// caller to label.
pub async fn page_location(pool: &SqlitePool, page_id: &str) -> Result<Vec<String>, String> {
    let page_id = require_active_page(pool, page_id).await?;
    page_ancestry_location(pool, page_id).await
}

/// Read the projects that share a project group with the project an active page belongs to: the page's own project
/// and the active projects beside it. A page outside every project has none.
pub async fn page_project_group_project_ids(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<Vec<String>, String> {
    let page_id = require_active_page(pool, page_id).await?;
    let pages = ancestry_pages(pool, page_id).await?;
    let folders = match pages.iter().find_map(|page| page.folder_id.as_deref()) {
        Some(folder_id) => folder_location(pool, folder_id).await?,
        None => Vec::new(),
    };
    let Some(project_id) = ancestry_project_id(&pages, &folders) else {
        return Ok(Vec::new());
    };
    sqlx::query_scalar::<_, String>(
        "SELECT peer.id
         FROM projects AS owner
         JOIN projects AS peer ON peer.group_id = owner.group_id
         WHERE owner.id = ? AND (peer.status = 'active' OR peer.id = owner.id)
         ORDER BY peer.order_key, peer.name, peer.id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes page project group: {e}"))
}

/// Trim and validate a page id, requiring the page to be neither trashed nor archived.
async fn require_active_page<'a>(pool: &SqlitePool, page_id: &'a str) -> Result<&'a str, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    let active = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes page: {e}"))?;
    if active.is_none() {
        return Err("notes page not found".to_string());
    }
    Ok(page_id)
}

/// Read where a database lives: the location of the page that holds it, ending with that page's title.
pub(crate) async fn database_location(
    pool: &SqlitePool,
    database_id: &str,
) -> Result<Vec<String>, String> {
    let page_id = sqlx::query_scalar::<_, String>("SELECT page_id FROM notes_blocks WHERE id = ?")
        .bind(database_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load notes database location page: {e}"))?;
    match page_id {
        Some(page_id) => page_ancestry_location(pool, &page_id).await,
        None => Ok(Vec::new()),
    }
}

async fn page_ancestry_location(pool: &SqlitePool, page_id: &str) -> Result<Vec<String>, String> {
    let pages = ancestry_pages(pool, page_id).await?;
    let folder_id = pages.iter().find_map(|page| page.folder_id.as_deref());
    let folders = match folder_id {
        Some(folder_id) => folder_location(pool, folder_id).await?,
        None => Vec::new(),
    };
    let mut location = match ancestry_project_id(&pages, &folders) {
        Some(project_id) => project_location(pool, &project_id).await?,
        None => Vec::new(),
    };
    location.extend(folders.into_iter().map(|(name, _)| name));
    for page in pages {
        location.extend(page.row_database_title);
        location.push(page.title);
    }
    Ok(location)
}

/// The project a page belongs to: the project marked on the outermost page of its ancestry that has one, else the
/// project owning its outermost folder.
fn ancestry_project_id(pages: &[LocationPage], folders: &[(String, String)]) -> Option<String> {
    pages
        .iter()
        .find_map(|page| page.project_id.clone())
        .or_else(|| folders.first().map(|(_, project_id)| project_id.clone()))
}

/// Read a page and the pages above it, outermost first, following page, block, and row data source parents.
async fn ancestry_pages(pool: &SqlitePool, page_id: &str) -> Result<Vec<LocationPage>, String> {
    sqlx::query_as::<_, LocationPage>(
        "WITH RECURSIVE path(page_id, depth) AS (
           SELECT ?, 0
           UNION
           SELECT COALESCE(
                    page.parent_page_id,
                    (SELECT parent_block.page_id FROM notes_blocks AS parent_block
                     WHERE parent_block.id = page.parent_block_id),
                    (SELECT database_block.page_id
                     FROM notes_data_sources AS data_source
                     JOIN notes_blocks AS database_block ON database_block.id = data_source.database_id
                     WHERE data_source.id = page.parent_data_source_id)
                  ),
                  path.depth + 1
           FROM path
           JOIN notes_pages AS page ON page.id = path.page_id
           WHERE path.depth < ?
         )
         SELECT page.title,
                (SELECT database.title
                 FROM notes_data_sources AS data_source
                 JOIN notes_databases AS database ON database.id = data_source.database_id
                 WHERE data_source.id = page.parent_data_source_id) AS row_database_title,
                page.folder_id,
                page.project_id
         FROM path
         JOIN notes_pages AS page ON page.id = path.page_id
         ORDER BY path.depth DESC",
    )
    .bind(page_id)
    .bind(crate::structure::MAX_TREE_DEPTH)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes page location: {e}"))
}

/// Read a folder and its ancestors, outermost first, each with the project that owns it.
async fn folder_location(
    pool: &SqlitePool,
    folder_id: &str,
) -> Result<Vec<(String, String)>, String> {
    sqlx::query_as::<_, (String, String)>(
        "WITH RECURSIVE chain(folder_id, depth) AS (
           SELECT ?, 0
           UNION
           SELECT folder.parent_folder_id, chain.depth + 1
           FROM chain
           JOIN notes_folders AS folder ON folder.id = chain.folder_id
           WHERE folder.parent_folder_id IS NOT NULL AND chain.depth < ?
         )
         SELECT folder.name, folder.project_id
         FROM chain
         JOIN notes_folders AS folder ON folder.id = chain.folder_id
         ORDER BY chain.depth DESC",
    )
    .bind(folder_id)
    .bind(crate::structure::MAX_TREE_DEPTH)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes folder location: {e}"))
}

/// Read a project's group and name, or nothing when the project no longer exists.
async fn project_location(pool: &SqlitePool, project_id: &str) -> Result<Vec<String>, String> {
    let names = sqlx::query_as::<_, (String, String)>(
        "SELECT project_group.name, project.name
         FROM projects AS project
         JOIN project_groups AS project_group ON project_group.id = project.group_id
         WHERE project.id = ?",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes project location: {e}"))?;
    Ok(names.map_or_else(Vec::new, |(group, project)| vec![group, project]))
}

#[cfg(test)]
async fn list_pages_by_state(
    pool: &SqlitePool,
    in_trash: bool,
    archived: Option<bool>,
) -> Result<Vec<NotePageDto>, String> {
    let rows = if let Some(archived) = archived {
        sqlx::query_as::<_, NotePageRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {}
             FROM notes_pages
             WHERE {} AND in_trash = ? AND archived = ?
             ORDER BY last_edited_time DESC, title COLLATE NOCASE ASC, id ASC",
            crate::structure::page_row_columns("notes_pages"),
            crate::structure::placed_page_sql("notes_pages")
        )))
        .bind(if in_trash { 1_i64 } else { 0_i64 })
        .bind(if archived { 1_i64 } else { 0_i64 })
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, NotePageRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {}
             FROM notes_pages
             WHERE {} AND in_trash = ?
             ORDER BY last_edited_time DESC, title COLLATE NOCASE ASC, id ASC",
            crate::structure::page_row_columns("notes_pages"),
            crate::structure::placed_page_sql("notes_pages")
        )))
        .bind(if in_trash { 1_i64 } else { 0_i64 })
        .fetch_all(pool)
        .await
    }
    .map_err(|e| format!("list notes pages: {e}"))?;
    rows.into_iter().map(NotePageDto::new).collect()
}

fn normalize_request_page_ids(page_ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    page_ids
        .into_iter()
        .filter_map(|page_id| normalize_request_page_id(&page_id))
        .filter(|page_id| seen.insert(page_id.clone()))
        .collect()
}

fn normalize_request_page_id(page_id: &str) -> Option<String> {
    let trimmed = page_id.trim();
    require_uuid(trimmed, "page_id").ok()?;
    Some(trimmed.to_string())
}

fn push_unique_page_rows(
    rows_by_id: &mut HashMap<String, NotePageSummaryDto>,
    rows: Vec<NotePageSummaryDto>,
) {
    for row in rows {
        rows_by_id.entry(row.id.clone()).or_insert(row);
    }
}

fn sort_page_rows(rows: &mut [NotePageSummaryDto]) {
    rows.sort_by(|left, right| {
        right
            .last_edited_time
            .cmp(&left.last_edited_time)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
}

async fn load_sidebar_root_page_rows(pool: &SqlitePool) -> Result<Vec<NotePageSummaryDto>, String> {
    sqlx::query_as::<_, NotePageSummaryDto>(AssertSqlSafe(format!(
        "SELECT {}
         FROM notes_pages
         WHERE in_trash = 0
           AND archived = 0
           AND parent_type <> 'page_id'
           AND parent_type <> 'data_source_id'
         ORDER BY last_edited_time DESC, title COLLATE NOCASE ASC, id ASC LIMIT 50",
        crate::workspace_shell::page_projection()
    )))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes sidebar root pages: {e}"))
}

async fn load_active_page_rows_by_ids(
    pool: &SqlitePool,
    page_ids: &[String],
) -> Result<Vec<NotePageSummaryDto>, String> {
    if page_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(format!(
        "SELECT {} FROM notes_pages WHERE in_trash = 0 AND archived = 0 AND id IN (",
        crate::workspace_shell::page_projection()
    ));
    let mut separated = query.separated(", ");
    for page_id in page_ids {
        separated.push_bind(page_id);
    }
    query.push(") ORDER BY last_edited_time DESC, title COLLATE NOCASE ASC, id ASC");
    query
        .build_query_as::<NotePageSummaryDto>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("list notes sidebar pages by id: {e}"))
}

async fn load_active_child_page_rows(
    pool: &SqlitePool,
    parent_page_ids: &[String],
) -> Result<Vec<NotePageSummaryDto>, String> {
    if parent_page_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(format!(
        "SELECT {} FROM notes_pages
         WHERE in_trash = 0
           AND archived = 0
           AND parent_type = 'page_id'
           AND parent_page_id IN (",
        crate::workspace_shell::page_projection()
    ));
    let mut separated = query.separated(", ");
    for page_id in parent_page_ids {
        separated.push_bind(page_id);
    }
    query.push(") ORDER BY last_edited_time DESC, title COLLATE NOCASE ASC, id ASC");
    query
        .build_query_as::<NotePageSummaryDto>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("list notes sidebar child pages: {e}"))
}

async fn load_active_ancestor_page_rows(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<Vec<NotePageSummaryDto>, String> {
    let query = format!(
        "WITH RECURSIVE ancestors(id, depth) AS (
             SELECT parent_page_id, 1 FROM notes_pages
             WHERE id = ? AND parent_type = 'page_id'
             UNION
             SELECT page.parent_page_id, ancestors.depth + 1
             FROM notes_pages AS page
             JOIN ancestors ON page.id = ancestors.id
             WHERE page.parent_type = 'page_id'
               AND page.in_trash = 0
               AND page.archived = 0
               AND ancestors.depth < {max_depth}
         )
         SELECT {} FROM notes_pages
         WHERE id IN (SELECT id FROM ancestors WHERE id IS NOT NULL)
           AND in_trash = 0 AND archived = 0
         LIMIT {max_depth}",
        crate::workspace_shell::page_projection(),
        max_depth = crate::structure::MAX_TREE_DEPTH,
    );
    sqlx::query_as::<_, NotePageSummaryDto>(AssertSqlSafe(query.as_str()))
        .bind(page_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("list notes sidebar ancestor pages: {e}"))
}

async fn load_page_row_including_trashed(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<Option<NotePageRow>, String> {
    sqlx::query_as::<_, NotePageRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {}
         FROM notes_pages
         WHERE id = ?
         LIMIT 1",
        crate::structure::page_row_columns("notes_pages")
    )))
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes page metadata: {e}"))
}

async fn load_active_parent_page_ids_with_children(
    pool: &SqlitePool,
    page_ids: &[String],
) -> Result<Vec<String>, String> {
    if page_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT DISTINCT parent_page_id
         FROM notes_pages
         WHERE in_trash = 0
           AND archived = 0
           AND parent_type = 'page_id'
           AND parent_page_id IN (",
    );
    let mut separated = query.separated(", ");
    for page_id in page_ids {
        separated.push_bind(page_id);
    }
    query.push(") ORDER BY parent_page_id ASC");
    let rows = query
        .build_query_scalar::<String>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("list notes sidebar child page markers: {e}"))?;
    Ok(rows)
}

async fn load_unavailable_parent_page_ids(
    pool: &SqlitePool,
    rows: &[NotePageSummaryDto],
    loaded_page_ids: &[String],
) -> Result<(Vec<String>, Vec<String>), String> {
    let loaded_page_id_set = loaded_page_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut checked_parent_ids = HashSet::new();
    let mut missing_parent_page_ids = Vec::new();
    let mut trashed_parent_page_ids = Vec::new();
    for row in rows {
        if row.parent_type != "page_id" {
            continue;
        }
        let Some(parent_page_id) = row.parent_page_id.as_deref() else {
            continue;
        };
        if loaded_page_id_set.contains(parent_page_id)
            || !checked_parent_ids.insert(parent_page_id.to_string())
        {
            continue;
        }
        let state = sqlx::query_as::<_, (i64, i64)>(
            "SELECT in_trash, archived FROM notes_pages WHERE id = ?",
        )
        .bind(parent_page_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load notes sidebar parent state: {e}"))?;
        match state {
            None => missing_parent_page_ids.push(parent_page_id.to_string()),
            Some((in_trash, _)) if in_trash != 0 => {
                trashed_parent_page_ids.push(parent_page_id.to_string());
            }
            Some((_, archived)) if archived != 0 => {
                missing_parent_page_ids.push(parent_page_id.to_string());
            }
            Some(_) => {}
        }
    }
    missing_parent_page_ids.sort();
    trashed_parent_page_ids.sort();
    Ok((missing_parent_page_ids, trashed_parent_page_ids))
}

pub async fn load_page(pool: &SqlitePool, page_id: &str) -> Result<NoteLoadedPage, String> {
    require_uuid(page_id.trim(), "page_id")?;
    let page = get_page(pool, page_id.trim(), false).await?;
    let blocks =
        get_page_block_children(pool, page_id.trim(), None, Some(DEFAULT_PAGE_SIZE)).await?;
    Ok(NoteLoadedPage::new(page, blocks))
}

pub async fn open_page(pool: &SqlitePool, page_id: &str) -> Result<NotePageOpenDto, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin notes page open: {error}"))?;
    let page_row = sqlx::query_as::<_, NotePageRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {} FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
        crate::structure::page_row_columns("notes_pages")
    )))
    .bind(page_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| format!("load notes page: {error}"))?
    .ok_or_else(|| "notes page not found".to_string())?;
    let breadcrumb = page_breadcrumb_tx(&mut transaction, page_id).await?;
    let rows = sqlx::query_as::<_, NoteBlockRow>(concat!(
        "SELECT ",
        crate::note_block_row_columns!("notes_blocks"),
        "
         FROM notes_blocks
         WHERE parent_type = 'page_id' AND parent_page_id = ? AND in_trash = 0
         ORDER BY order_key ASC, id ASC
         LIMIT ?"
    ))
    .bind(page_id)
    .bind(DEFAULT_PAGE_SIZE + 1)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| format!("load notes page blocks: {error}"))?;
    let outline_rows = sqlx::query_as::<_, BlockOutlineRow>(
        "SELECT id, page_id, parent_type, parent_page_id, parent_block_id,
                type AS block_type, CAST(ROW_NUMBER() OVER (PARTITION BY parent_page_id, parent_block_id ORDER BY order_key, id) AS REAL) AS sort_order, EXISTS (SELECT 1 FROM notes_blocks AS has_child WHERE has_child.parent_block_id = notes_blocks.id AND has_child.in_trash = 0) AS has_children, COALESCE(json_extract(attrs, '$.ganbaru_indent'), 0) AS ganbaru_indent
         FROM notes_blocks
         WHERE parent_type = 'page_id' AND parent_page_id = ? AND in_trash = 0
         ORDER BY order_key ASC, id ASC",
    )
    .bind(page_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| format!("load notes page block outline: {error}"))?;
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit notes page open: {error}"))?;
    let page = NotePageDto::new(page_row)?;
    let blocks = block_page_from_rows(rows, DEFAULT_PAGE_SIZE as usize)?;
    let outlines = outline_rows
        .into_iter()
        .map(NoteBlockOutlineDto::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NotePageOpenDto::new(page, breadcrumb, blocks, outlines))
}

pub async fn block_outline_frontier(
    pool: &SqlitePool,
    page_id: &str,
    parent_ids: &[String],
) -> Result<Vec<NoteBlockOutlineDto>, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    let parent_ids = normalize_frontier_parent_ids(parent_ids)?;
    if parent_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for parent_batch in parent_ids.chunks(BLOCK_FRONTIER_PARENT_BATCH_SIZE) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, page_id, parent_type, parent_page_id, parent_block_id, \
                    type AS block_type, CAST(ROW_NUMBER() OVER (PARTITION BY parent_page_id, parent_block_id ORDER BY order_key, id) AS REAL) AS sort_order, EXISTS (SELECT 1 FROM notes_blocks AS has_child WHERE has_child.parent_block_id = notes_blocks.id AND has_child.in_trash = 0) AS has_children, COALESCE(json_extract(attrs, '$.ganbaru_indent'), 0) AS ganbaru_indent \
             FROM notes_blocks WHERE page_id = ",
        );
        query.push_bind(page_id);
        query.push(" AND parent_type = 'block_id' AND in_trash = 0 AND parent_block_id IN (");
        let mut separated = query.separated(", ");
        for parent_id in parent_batch {
            separated.push_bind(parent_id);
        }
        query.push(") ORDER BY parent_block_id ASC, order_key ASC, id ASC");
        rows.extend(
            query
                .build_query_as::<BlockOutlineRow>()
                .fetch_all(pool)
                .await
                .map_err(|error| format!("load notes block outline frontier: {error}"))?,
        );
    }
    rows.into_iter()
        .map(NoteBlockOutlineDto::try_from)
        .collect()
}

pub async fn hydrate_blocks(
    pool: &SqlitePool,
    request: NoteBlockHydrationRequest,
) -> Result<Vec<NoteBlockDto>, String> {
    let page_id = request.page_id.trim();
    require_uuid(page_id, "page_id")?;
    let block_ids = normalize_block_ids(&request.block_ids, "block_id")?;
    if block_ids.len() > 200 {
        return Err("block hydration is limited to 200 blocks".to_string());
    }
    if block_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(concat!(
        "SELECT ",
        crate::note_block_row_columns!("notes_blocks"),
        " FROM notes_blocks WHERE page_id = "
    ));
    query.push_bind(page_id);
    query.push(" AND in_trash = 0 AND id IN (");
    let mut separated = query.separated(", ");
    for block_id in &block_ids {
        separated.push_bind(block_id);
    }
    query.push(") ORDER BY order_key ASC, id ASC");
    query
        .build_query_as::<NoteBlockRow>()
        .fetch_all(pool)
        .await
        .map_err(|error| format!("hydrate notes blocks: {error}"))?
        .into_iter()
        .map(NoteBlockDto::new)
        .collect()
}

/// Validates and dedupes one frontier read's parent IDs, refusing more than
/// [`MAX_BLOCK_FRONTIER_PARENT_IDS`] before any ID is parsed.
fn normalize_frontier_parent_ids(parent_ids: &[String]) -> Result<Vec<String>, String> {
    if parent_ids.len() > MAX_BLOCK_FRONTIER_PARENT_IDS {
        return Err(format!(
            "block frontier reads are limited to {MAX_BLOCK_FRONTIER_PARENT_IDS} parent blocks"
        ));
    }
    normalize_block_ids(parent_ids, "parent_id")
}

fn normalize_block_ids(ids: &[String], label: &str) -> Result<Vec<String>, String> {
    let mut normalized = Vec::with_capacity(ids.len());
    let mut seen = HashSet::new();
    for id in ids {
        let id = id.trim();
        require_uuid(id, label)?;
        if seen.insert(id.to_string()) {
            normalized.push(id.to_string());
        }
    }
    Ok(normalized)
}

async fn page_breadcrumb_tx(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<Vec<NotePageBreadcrumbItemDto>, String> {
    let mut crumbs = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = Some(page_id.to_string());
    while let Some(current_page_id) = cursor {
        if !seen.insert(current_page_id.clone()) {
            break;
        }
        let row = sqlx::query_as::<_, NotePageRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {} FROM notes_pages WHERE id = ?",
            crate::structure::page_row_columns("notes_pages")
        )))
        .bind(&current_page_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| format!("load notes page breadcrumb: {error}"))?;
        let Some(row) = row else {
            crumbs.push(NotePageBreadcrumbItemDto::missing(current_page_id));
            break;
        };
        let current = row.id == page_id;
        if current && (row.in_trash != 0 || row.archived != 0) {
            return Err("notes page not found".to_string());
        }
        cursor = (row.parent_type == "page_id")
            .then(|| row.parent_page_id.clone())
            .flatten();
        crumbs.push(if row.in_trash != 0 {
            NotePageBreadcrumbItemDto::unavailable(row, "trashed")
        } else if row.archived != 0 {
            NotePageBreadcrumbItemDto::unavailable(row, "archived")
        } else {
            NotePageBreadcrumbItemDto::active(row, current)
        });
    }
    crumbs.reverse();
    Ok(crumbs)
}

pub async fn block_frontier(
    pool: &SqlitePool,
    parent_ids: &[String],
) -> Result<NoteBlockFrontierDto, String> {
    let normalized = normalize_frontier_parent_ids(parent_ids)?;
    if normalized.is_empty() {
        return Ok(NoteBlockFrontierDto::new(Vec::new()));
    }
    let mut rows = Vec::new();
    for parent_batch in normalized.chunks(BLOCK_FRONTIER_PARENT_BATCH_SIZE) {
        let mut query = QueryBuilder::<Sqlite>::new(concat!(
            "SELECT ",
            crate::note_block_row_columns!("notes_blocks"),
            " FROM notes_blocks WHERE parent_type = 'block_id' AND in_trash = 0 AND parent_block_id IN ("
        ));
        let mut separated = query.separated(", ");
        for parent_id in parent_batch {
            separated.push_bind(parent_id);
        }
        query.push(") ORDER BY parent_block_id ASC, order_key ASC, id ASC");
        rows.extend(
            query
                .build_query_as::<NoteBlockRow>()
                .fetch_all(pool)
                .await
                .map_err(|error| format!("load notes block frontier: {error}"))?,
        );
    }
    let blocks = rows
        .into_iter()
        .map(NoteBlockDto::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NoteBlockFrontierDto::new(blocks))
}

fn block_page_from_rows(
    mut rows: Vec<NoteBlockRow>,
    page_size: usize,
) -> Result<NotePaginatedBlockList, String> {
    let has_more = rows.len() > page_size;
    if has_more {
        rows.truncate(page_size);
    }
    let next_cursor = if has_more {
        rows.last().map(|row| row.id.clone())
    } else {
        None
    };
    let blocks = rows
        .into_iter()
        .map(NoteBlockDto::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NotePaginatedBlockList::new(blocks, next_cursor, has_more))
}

struct BlockOutlineRow {
    ganbaru_indent: i64,
    id: String,
    page_id: String,
    parent_type: String,
    parent_page_id: Option<String>,
    parent_block_id: Option<String>,
    block_type: String,
    sort_order: f64,
    has_children: i64,
}

impl<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> for BlockOutlineRow {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            id: row.try_get("id")?,
            page_id: row.try_get("page_id")?,
            parent_type: row.try_get("parent_type")?,
            parent_page_id: row.try_get("parent_page_id")?,
            parent_block_id: row.try_get("parent_block_id")?,
            block_type: row.try_get("block_type")?,
            sort_order: row.try_get("sort_order")?,
            has_children: row.try_get("has_children")?,
            ganbaru_indent: row.try_get("ganbaru_indent")?,
        })
    }
}

impl TryFrom<BlockOutlineRow> for NoteBlockOutlineDto {
    type Error = String;

    fn try_from(row: BlockOutlineRow) -> Result<Self, Self::Error> {
        let parent = match row.parent_type.as_str() {
            "page_id" => crate::models::NoteParent::PageId {
                page_id: row
                    .parent_page_id
                    .ok_or_else(|| "page block outline is missing parent_page_id".to_string())?,
            },
            "block_id" => crate::models::NoteParent::BlockId {
                block_id: row
                    .parent_block_id
                    .ok_or_else(|| "nested block outline is missing parent_block_id".to_string())?,
            },
            other => return Err(format!("unsupported block outline parent type: {other}")),
        };
        let retained_height = match row.block_type.as_str() {
            "image" | "video" | "pdf" | "bookmark" | "link_preview" | "embed" => 240,
            "child_database" | "table" | "column_list" | "tab" => 180,
            "code" | "callout" => 72,
            "heading_1" | "heading_2" | "heading_3" | "heading_4" | "heading_5" | "heading_6" => 48,
            _ => 36,
        };
        let mut outline = NoteBlockOutlineDto::new(
            row.id,
            row.page_id,
            parent,
            row.block_type,
            row.sort_order,
            row.has_children != 0,
            retained_height,
        );
        outline.ganbaru_indent = row.ganbaru_indent;
        Ok(outline)
    }
}

pub async fn get_page(
    pool: &SqlitePool,
    page_id: &str,
    include_trash: bool,
) -> Result<NotePageDto, String> {
    let row = if include_trash {
        sqlx::query_as::<_, NotePageRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {} FROM notes_pages WHERE id = ?",
            crate::structure::page_row_columns("notes_pages")
        )))
        .bind(page_id)
        .fetch_optional(pool)
        .await
    } else {
        sqlx::query_as::<_, NotePageRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {} FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
            crate::structure::page_row_columns("notes_pages")
        )))
        .bind(page_id)
        .fetch_optional(pool)
        .await
    }
    .map_err(|e| format!("load notes page: {e}"))?
    .ok_or_else(|| "notes page not found".to_string())?;
    NotePageDto::new(row)
}

pub async fn get_block(
    pool: &SqlitePool,
    block_id: &str,
    include_trash: bool,
) -> Result<NoteBlockDto, String> {
    let row = get_block_row(pool, block_id, include_trash).await?;
    NoteBlockDto::new(row)
}

pub async fn get_block_row(
    pool: &SqlitePool,
    block_id: &str,
    include_trash: bool,
) -> Result<NoteBlockRow, String> {
    let row = if include_trash {
        sqlx::query_as::<_, NoteBlockRow>(concat!(
            "SELECT ",
            crate::note_block_row_columns!("notes_blocks"),
            "
             FROM notes_blocks
             WHERE id = ?"
        ))
        .bind(block_id)
        .fetch_optional(pool)
        .await
    } else {
        sqlx::query_as::<_, NoteBlockRow>(concat!(
            "SELECT ",
            crate::note_block_row_columns!("notes_blocks"),
            "
             FROM notes_blocks
             WHERE id = ? AND in_trash = 0"
        ))
        .bind(block_id)
        .fetch_optional(pool)
        .await
    }
    .map_err(|e| format!("load notes block: {e}"))?
    .ok_or_else(|| "notes block not found".to_string())?;
    Ok(row)
}

pub async fn block_children(
    pool: &SqlitePool,
    parent_id: &str,
    start_cursor: Option<&str>,
    page_size: Option<i64>,
) -> Result<NotePaginatedBlockList, String> {
    let parent_id = parent_id.trim();
    require_uuid(parent_id, "parent_id")?;
    if page_exists(pool, parent_id).await? {
        return get_page_block_children(pool, parent_id, start_cursor, page_size).await;
    }
    let parent = get_block_row(pool, parent_id, false).await?;
    get_child_blocks_by_parent_block(pool, &parent.id, start_cursor, page_size).await
}

pub async fn get_page_block_children(
    pool: &SqlitePool,
    page_id: &str,
    start_cursor: Option<&str>,
    page_size: Option<i64>,
) -> Result<NotePaginatedBlockList, String> {
    get_child_blocks_by_parent_page(pool, page_id, start_cursor, page_size).await
}

pub async fn page_exists(pool: &SqlitePool, page_id: &str) -> Result<bool, String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("check notes page: {e}"))?;
    Ok(exists.is_some())
}

async fn get_child_blocks_by_parent_page(
    pool: &SqlitePool,
    page_id: &str,
    start_cursor: Option<&str>,
    page_size: Option<i64>,
) -> Result<NotePaginatedBlockList, String> {
    let cursor_order = cursor_order_key(pool, start_cursor).await?;
    let size = normalized_page_size(page_size)?;
    let rows = if let Some((order_key, cursor_id)) = cursor_order {
        sqlx::query_as::<_, NoteBlockRow>(concat!(
            "SELECT ",
            crate::note_block_row_columns!("notes_blocks"),
            "
             FROM notes_blocks
             WHERE parent_type = 'page_id'
               AND parent_page_id = ?
               AND in_trash = 0
               AND (order_key > ? OR (order_key = ? AND id > ?))
             ORDER BY order_key ASC, id ASC
             LIMIT ?"
        ))
        .bind(page_id)
        .bind(&order_key)
        .bind(&order_key)
        .bind(cursor_id)
        .bind(size + 1)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, NoteBlockRow>(concat!(
            "SELECT ",
            crate::note_block_row_columns!("notes_blocks"),
            "
             FROM notes_blocks
             WHERE parent_type = 'page_id'
               AND parent_page_id = ?
               AND in_trash = 0
             ORDER BY order_key ASC, id ASC
             LIMIT ?"
        ))
        .bind(page_id)
        .bind(size + 1)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| format!("load page block children: {e}"))?;
    paginated_rows(rows, size as usize)
}

async fn get_child_blocks_by_parent_block(
    pool: &SqlitePool,
    block_id: &str,
    start_cursor: Option<&str>,
    page_size: Option<i64>,
) -> Result<NotePaginatedBlockList, String> {
    let cursor_order = cursor_order_key(pool, start_cursor).await?;
    let size = normalized_page_size(page_size)?;
    let rows = if let Some((order_key, cursor_id)) = cursor_order {
        sqlx::query_as::<_, NoteBlockRow>(concat!(
            "SELECT ",
            crate::note_block_row_columns!("notes_blocks"),
            "
             FROM notes_blocks
             WHERE parent_type = 'block_id'
               AND parent_block_id = ?
               AND in_trash = 0
               AND (order_key > ? OR (order_key = ? AND id > ?))
             ORDER BY order_key ASC, id ASC
             LIMIT ?"
        ))
        .bind(block_id)
        .bind(&order_key)
        .bind(&order_key)
        .bind(cursor_id)
        .bind(size + 1)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, NoteBlockRow>(concat!(
            "SELECT ",
            crate::note_block_row_columns!("notes_blocks"),
            "
             FROM notes_blocks
             WHERE parent_type = 'block_id'
               AND parent_block_id = ?
               AND in_trash = 0
             ORDER BY order_key ASC, id ASC
             LIMIT ?"
        ))
        .bind(block_id)
        .bind(size + 1)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| format!("load block children: {e}"))?;
    paginated_rows(rows, size as usize)
}

async fn cursor_order_key(
    pool: &SqlitePool,
    start_cursor: Option<&str>,
) -> Result<Option<(String, String)>, String> {
    let Some(cursor) = start_cursor
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    require_uuid(cursor, "start_cursor")?;
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT order_key, id FROM notes_blocks WHERE id = ? AND in_trash = 0")
            .bind(cursor)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("load notes cursor: {e}"))?;
    row.ok_or_else(|| "start_cursor block not found".to_string())
        .map(Some)
}

fn normalized_page_size(page_size: Option<i64>) -> Result<i64, String> {
    let size = page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    validate_page_size(size)?;
    Ok(size)
}

fn paginated_rows(
    rows: Vec<NoteBlockRow>,
    page_size: usize,
) -> Result<NotePaginatedBlockList, String> {
    let has_more = rows.len() > page_size;
    let visible_rows = rows.into_iter().take(page_size).collect::<Vec<_>>();
    let next_cursor = if has_more {
        visible_rows.last().map(|row| row.id.clone())
    } else {
        None
    };
    let blocks = visible_rows
        .into_iter()
        .map(NoteBlockDto::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NotePaginatedBlockList::new(blocks, next_cursor, has_more))
}
