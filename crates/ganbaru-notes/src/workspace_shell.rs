use crate::models::{
    NoteFolderDto, NoteFolderRow, NoteNavigationDatabaseDto, NotePageSummaryDto,
    NoteWorkspaceShellDto, NoteWorkspaceShellRequest,
};
use crate::validation::{require_uuid, validate_folder_project_id};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::collections::{HashMap, HashSet};

const PAGE_WINDOW_SIZE: i64 = 50;
const FOLDER_WINDOW_SIZE: i64 = 100;
const MAX_SHELL_PAGES: usize = 200;
const MAX_SEED_IDS: usize = 50;
const MAX_EXPANDED_IDS: usize = 50;

#[derive(Deserialize, Serialize)]
struct PageCursor {
    last_edited_time: String,
    title: String,
    id: String,
}

#[derive(Deserialize, Serialize)]
struct FolderCursor {
    name: String,
    id: String,
}

pub async fn load_workspace_shell(
    pool: &SqlitePool,
    request: NoteWorkspaceShellRequest,
) -> Result<NoteWorkspaceShellDto, String> {
    let project_id = normalize_optional_project_id(request.project_id)?;
    let selected_page_id = normalize_optional_uuid(request.selected_page_id, "selected_page_id")?;
    let page_query = request
        .page_query
        .unwrap_or_default()
        .trim()
        .chars()
        .take(200)
        .collect::<String>();
    let seed_page_ids = normalize_ids(request.seed_page_ids, MAX_SEED_IDS);
    let expanded_page_ids = normalize_ids(request.expanded_page_ids, MAX_EXPANDED_IDS);
    let should_load_page_window = request.page_cursor.as_deref() != Some("end");
    let should_load_folder_window = request.folder_cursor.as_deref() != Some("end");
    let page_cursor = decode_cursor::<PageCursor>(request.page_cursor.as_deref(), "page")?;
    let folder_cursor = decode_cursor::<FolderCursor>(request.folder_cursor.as_deref(), "folder")?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin notes workspace shell read: {error}"))?;

    let total_page_count = count_pages(&mut transaction, project_id.as_deref()).await?;
    let total_folder_count = count_folders(&mut transaction, project_id.as_deref()).await?;
    let mut rows_by_id = HashMap::<String, NotePageSummaryDto>::new();

    let initial_pages = if !should_load_page_window {
        Vec::new()
    } else if request.destination_candidates {
        load_destination_window(
            &mut transaction,
            project_id.as_deref(),
            &page_query,
            page_cursor.as_ref(),
            PAGE_WINDOW_SIZE,
        )
        .await?
    } else {
        load_root_window(
            &mut transaction,
            project_id.as_deref(),
            page_cursor.as_ref(),
            PAGE_WINDOW_SIZE,
        )
        .await?
    };
    let initial_page_window_full = initial_pages.len() == PAGE_WINDOW_SIZE as usize;
    let initial_page_cursor = pages_cursor(&initial_pages);
    add_unique(&mut rows_by_id, initial_pages);

    let mut recovery_ids = seed_page_ids;
    if let Some(selected) = selected_page_id.as_ref() {
        if !recovery_ids.contains(selected) {
            recovery_ids.push(selected.clone());
        }
    }
    add_unique(
        &mut rows_by_id,
        load_pages_by_ids(&mut transaction, project_id.as_deref(), &recovery_ids).await?,
    );
    add_unique(
        &mut rows_by_id,
        load_ancestors(&mut transaction, project_id.as_deref(), &recovery_ids).await?,
    );
    add_unique(
        &mut rows_by_id,
        load_children(&mut transaction, project_id.as_deref(), &expanded_page_ids).await?,
    );

    let mut pages = rows_by_id.into_values().collect::<Vec<_>>();
    pages.sort_by(|left, right| {
        right
            .last_edited_time
            .cmp(&left.last_edited_time)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
    pages.truncate(MAX_SHELL_PAGES);
    let loaded_ids = pages.iter().map(|page| page.id.clone()).collect::<Vec<_>>();
    let page_ids_with_children =
        load_parent_ids_with_children(&mut transaction, &loaded_ids).await?;
    let (missing_parent_page_ids, trashed_parent_page_ids) =
        load_unavailable_parent_ids(&mut transaction, &pages, &loaded_ids).await?;
    let resolved_selected_page_id =
        selected_page_id.filter(|selected| pages.iter().any(|page| page.id == *selected));
    let folder_rows = if should_load_folder_window {
        load_folder_window(
            &mut transaction,
            project_id.as_deref(),
            folder_cursor.as_ref(),
            FOLDER_WINDOW_SIZE,
        )
        .await?
    } else {
        Vec::new()
    };
    let folder_window_full = folder_rows.len() == FOLDER_WINDOW_SIZE as usize;
    let folder_window_cursor = folder_cursor_from_rows(&folder_rows);
    let folders = folder_rows.into_iter().map(NoteFolderDto::new).collect();
    let navigation_pages = if request.include_navigation_index {
        load_navigation_pages(&mut transaction).await?
    } else {
        Vec::new()
    };
    let navigation_page_ids_with_children = if request.include_navigation_index {
        load_navigation_parent_ids_with_children(&mut transaction).await?
    } else {
        Vec::new()
    };
    let navigation_folders = if request.include_navigation_index {
        load_navigation_folders(&mut transaction)
            .await?
            .into_iter()
            .map(NoteFolderDto::new)
            .collect()
    } else {
        Vec::new()
    };
    let navigation_databases = if request.include_navigation_index && navigation_pages.is_empty() {
        Vec::new()
    } else {
        load_navigation_databases(
            &mut transaction,
            (!request.include_navigation_index).then_some(loaded_ids.as_slice()),
        )
        .await?
    };

    transaction
        .commit()
        .await
        .map_err(|error| format!("commit notes workspace shell read: {error}"))?;
    let next_page_cursor = initial_page_window_full
        .then_some(initial_page_cursor)
        .flatten()
        .map(encode_cursor)
        .transpose()?;
    let next_folder_cursor = folder_window_full
        .then_some(folder_window_cursor)
        .flatten()
        .map(encode_cursor)
        .transpose()?;
    Ok(NoteWorkspaceShellDto::new(
        pages,
        folders,
        navigation_pages,
        navigation_folders,
        navigation_page_ids_with_children,
        navigation_databases,
        page_ids_with_children,
        missing_parent_page_ids,
        trashed_parent_page_ids,
        resolved_selected_page_id,
        total_page_count,
        total_folder_count,
        next_page_cursor,
        next_folder_cursor,
    ))
}

/// Read database shell metadata for the navigation index or one bounded page window.
async fn load_navigation_databases(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    page_ids: Option<&[String]>,
) -> Result<Vec<NoteNavigationDatabaseDto>, String> {
    if page_ids.is_some_and(|ids| ids.is_empty()) {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT database.id, block.page_id, database.title, source.id AS data_source_id
         FROM notes_databases AS database
         JOIN notes_blocks AS block ON block.id = database.id
         JOIN notes_pages AS page ON page.id = block.page_id
         JOIN notes_data_sources AS source ON source.id = json_extract(block.payload, '$.data_source_id')
         WHERE database.in_trash = 0 AND block.in_trash = 0
           AND block.type = 'child_database' AND page.in_trash = 0 AND page.archived = 0
           AND source.in_trash = 0",
    );
    if let Some(ids) = page_ids {
        query.push(" AND page.id IN (");
        let mut separated = query.separated(", ");
        for id in ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");
    } else {
        query.push(" AND page.parent_type <> 'data_source_id'");
    }
    query.push(" ORDER BY block.page_id, database.title COLLATE NOCASE, database.id");
    query
        .build_query_as()
        .fetch_all(&mut **transaction)
        .await
        .map_err(|error| format!("list Notes navigation databases: {error}"))
}

async fn load_navigation_pages(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
) -> Result<Vec<NotePageSummaryDto>, String> {
    sqlx::query_as::<_, NotePageSummaryDto>(&format!(
        "SELECT {} FROM notes_pages
         WHERE in_trash = 0
           AND archived = 0
           AND parent_type <> 'data_source_id'
         ORDER BY last_edited_time DESC, title COLLATE NOCASE ASC, id ASC",
        page_projection()
    ))
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| format!("list Notes navigation pages: {error}"))
}

async fn load_navigation_folders(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
) -> Result<Vec<NoteFolderRow>, String> {
    sqlx::query_as::<_, NoteFolderRow>(
        "SELECT id, project_id, parent_folder_id, name, created_time, last_edited_time
         FROM notes_folders
         ORDER BY project_id, name COLLATE NOCASE ASC, id ASC",
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| format!("list Notes navigation folders: {error}"))
}

async fn load_navigation_parent_ids_with_children(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
) -> Result<Vec<String>, String> {
    sqlx::query_scalar(
        "SELECT DISTINCT parent_page_id
         FROM notes_pages
         WHERE in_trash = 0
           AND archived = 0
           AND parent_type = 'page_id'
           AND parent_page_id IS NOT NULL
         ORDER BY parent_page_id",
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| format!("list Notes navigation child markers: {error}"))
}

fn normalize_optional_uuid(value: Option<String>, field: &str) -> Result<Option<String>, String> {
    value
        .map(|value| {
            let trimmed = value.trim();
            require_uuid(trimmed, field)?;
            Ok(trimmed.to_string())
        })
        .transpose()
}

fn normalize_optional_project_id(value: Option<String>) -> Result<Option<String>, String> {
    value
        .map(|value| {
            let trimmed = value.trim();
            validate_folder_project_id(trimmed)?;
            Ok(trimmed.to_string())
        })
        .transpose()
}

fn normalize_ids(values: Vec<String>, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter_map(|value| {
            normalize_optional_uuid(Some(value), "page_id")
                .ok()
                .flatten()
        })
        .filter(|value| seen.insert(value.clone()))
        .take(limit)
        .collect()
}

fn decode_cursor<T: for<'de> Deserialize<'de>>(
    cursor: Option<&str>,
    field: &str,
) -> Result<Option<T>, String> {
    cursor
        .filter(|value| *value != "end")
        .map(|value| serde_json::from_str(value).map_err(|_| format!("invalid {field} cursor")))
        .transpose()
}

fn encode_cursor<T: Serialize>(cursor: T) -> Result<String, String> {
    serde_json::to_string(&cursor).map_err(|error| format!("serialize workspace cursor: {error}"))
}

fn pages_cursor(pages: &[NotePageSummaryDto]) -> Option<PageCursor> {
    pages.last().map(|page| PageCursor {
        last_edited_time: page.last_edited_time.clone(),
        title: page.title.clone(),
        id: page.id.clone(),
    })
}

fn folder_cursor_from_rows(folders: &[NoteFolderRow]) -> Option<FolderCursor> {
    folders.last().map(|folder| FolderCursor {
        name: folder.name.clone(),
        id: folder.id.clone(),
    })
}

fn add_unique(
    destination: &mut HashMap<String, NotePageSummaryDto>,
    rows: Vec<NotePageSummaryDto>,
) {
    for row in rows {
        if destination.len() >= MAX_SHELL_PAGES {
            break;
        }
        destination.entry(row.id.clone()).or_insert(row);
    }
}

pub(super) fn page_projection() -> &'static str {
    "id, parent_type, parent_page_id, parent_block_id, parent_data_source_id, folder_id, title, json_extract(properties, '$.__ganbaru_project_id') AS project_id, icon, in_trash, archived, created_time, last_edited_time"
}

fn push_project_filter(query: &mut QueryBuilder<'_, Sqlite>, project_id: Option<&str>) {
    if let Some(project_id) = project_id {
        query.push(" AND json_extract(properties, '$.__ganbaru_project_id') = ");
        query.push_bind(project_id.to_string());
    } else {
        query.push(" AND json_extract(properties, '$.__ganbaru_project_id') IS NULL");
    }
}

async fn count_pages(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    project_id: Option<&str>,
) -> Result<i64, String> {
    let mut query =
        QueryBuilder::new("SELECT COUNT(*) FROM notes_pages WHERE in_trash = 0 AND archived = 0");
    push_project_filter(&mut query, project_id);
    query
        .build_query_scalar()
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| format!("count notes workspace pages: {error}"))
}

async fn count_folders(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    project_id: Option<&str>,
) -> Result<i64, String> {
    let mut query = QueryBuilder::new("SELECT COUNT(*) FROM notes_folders WHERE 1 = 1");
    if let Some(project_id) = project_id {
        query
            .push(" AND project_id = ")
            .push_bind(project_id.to_string());
    } else {
        query.push(" AND project_id = ''");
    }
    query
        .build_query_scalar()
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| format!("count notes workspace folders: {error}"))
}

async fn load_root_window(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    project_id: Option<&str>,
    cursor: Option<&PageCursor>,
    limit: i64,
) -> Result<Vec<NotePageSummaryDto>, String> {
    let mut query = QueryBuilder::new(format!(
        "SELECT {} FROM notes_pages WHERE in_trash = 0 AND archived = 0 AND parent_type <> 'page_id' AND parent_type <> 'data_source_id'",
        page_projection()
    ));
    push_project_filter(&mut query, project_id);
    push_page_cursor(&mut query, cursor);
    query
        .push(" ORDER BY last_edited_time DESC, title COLLATE NOCASE ASC, id ASC LIMIT ")
        .push_bind(limit);
    query
        .build_query_as()
        .fetch_all(&mut **transaction)
        .await
        .map_err(|error| format!("list notes workspace roots: {error}"))
}

async fn load_destination_window(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    project_id: Option<&str>,
    search: &str,
    cursor: Option<&PageCursor>,
    limit: i64,
) -> Result<Vec<NotePageSummaryDto>, String> {
    let mut query = QueryBuilder::new(format!(
        "SELECT {} FROM notes_pages WHERE in_trash = 0 AND archived = 0",
        page_projection()
    ));
    push_project_filter(&mut query, project_id);
    if !search.is_empty() {
        query
            .push(" AND instr(lower(title), lower(")
            .push_bind(search.to_string())
            .push(")) > 0");
    }
    push_page_cursor(&mut query, cursor);
    query
        .push(" ORDER BY last_edited_time DESC, title COLLATE NOCASE ASC, id ASC LIMIT ")
        .push_bind(limit);
    query
        .build_query_as()
        .fetch_all(&mut **transaction)
        .await
        .map_err(|error| format!("list notes destination candidates: {error}"))
}

async fn load_pages_by_ids(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    project_id: Option<&str>,
    ids: &[String],
) -> Result<Vec<NotePageSummaryDto>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::new(format!(
        "SELECT {} FROM notes_pages WHERE in_trash = 0 AND archived = 0 AND id IN (",
        page_projection()
    ));
    let mut separated = query.separated(", ");
    for id in ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");
    push_project_filter(&mut query, project_id);
    query
        .build_query_as()
        .fetch_all(&mut **transaction)
        .await
        .map_err(|error| format!("list notes workspace recovery pages: {error}"))
}

async fn load_ancestors(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    project_id: Option<&str>,
    ids: &[String],
) -> Result<Vec<NotePageSummaryDto>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::new(
        "WITH RECURSIVE ancestors(id, depth) AS (SELECT parent_page_id, 1 FROM notes_pages WHERE id IN (",
    );
    let mut separated = query.separated(", ");
    for id in ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(") AND parent_type = 'page_id' UNION ALL SELECT page.parent_page_id, ancestors.depth + 1 FROM notes_pages page JOIN ancestors ON page.id = ancestors.id WHERE page.parent_type = 'page_id' AND ancestors.depth < 64) SELECT DISTINCT ");
    query.push(page_projection()).push(" FROM notes_pages WHERE in_trash = 0 AND archived = 0 AND id IN (SELECT id FROM ancestors WHERE id IS NOT NULL)");
    push_project_filter(&mut query, project_id);
    query.push(" LIMIT 64");
    query
        .build_query_as()
        .fetch_all(&mut **transaction)
        .await
        .map_err(|error| format!("list notes workspace ancestors: {error}"))
}

async fn load_children(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    project_id: Option<&str>,
    ids: &[String],
) -> Result<Vec<NotePageSummaryDto>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::new(format!(
        "SELECT {} FROM notes_pages WHERE in_trash = 0 AND archived = 0 AND parent_type = 'page_id' AND parent_page_id IN (",
        page_projection()
    ));
    let mut separated = query.separated(", ");
    for id in ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");
    push_project_filter(&mut query, project_id);
    query.push(" ORDER BY last_edited_time DESC, title COLLATE NOCASE ASC, id ASC LIMIT 100");
    query
        .build_query_as()
        .fetch_all(&mut **transaction)
        .await
        .map_err(|error| format!("list notes workspace expanded children: {error}"))
}

async fn load_parent_ids_with_children(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    ids: &[String],
) -> Result<Vec<String>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::new(
        "SELECT DISTINCT parent_page_id FROM notes_pages WHERE in_trash = 0 AND archived = 0 AND parent_type = 'page_id' AND parent_page_id IN (",
    );
    let mut separated = query.separated(", ");
    for id in ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(") ORDER BY parent_page_id");
    query
        .build_query_scalar()
        .fetch_all(&mut **transaction)
        .await
        .map_err(|error| format!("list notes workspace child markers: {error}"))
}

async fn load_unavailable_parent_ids(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    pages: &[NotePageSummaryDto],
    loaded_ids: &[String],
) -> Result<(Vec<String>, Vec<String>), String> {
    let loaded = loaded_ids.iter().collect::<HashSet<_>>();
    let parent_ids = pages
        .iter()
        .filter(|page| page.parent_type == "page_id")
        .filter_map(|page| page.parent_page_id.as_ref())
        .filter(|id| !loaded.contains(id))
        .cloned()
        .collect::<HashSet<_>>();
    if parent_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut query =
        QueryBuilder::new("SELECT id, in_trash, archived FROM notes_pages WHERE id IN (");
    let mut separated = query.separated(", ");
    for id in &parent_ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");
    let states = query
        .build_query_as::<(String, i64, i64)>()
        .fetch_all(&mut **transaction)
        .await
        .map_err(|error| format!("load notes workspace parent states: {error}"))?;
    let state_by_id = states
        .into_iter()
        .map(|(id, trash, archive)| (id, (trash, archive)))
        .collect::<HashMap<_, _>>();
    let mut missing = Vec::new();
    let mut trashed = Vec::new();
    for id in parent_ids {
        match state_by_id.get(&id) {
            Some((trash, _)) if *trash != 0 => trashed.push(id),
            Some((_, archive)) if *archive != 0 => missing.push(id),
            None => missing.push(id),
            Some(_) => {}
        }
    }
    missing.sort();
    trashed.sort();
    Ok((missing, trashed))
}

async fn load_folder_window(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    project_id: Option<&str>,
    cursor: Option<&FolderCursor>,
    limit: i64,
) -> Result<Vec<NoteFolderRow>, String> {
    let mut query = QueryBuilder::new(
        "SELECT id, project_id, parent_folder_id, name, created_time, last_edited_time FROM notes_folders WHERE 1 = 1",
    );
    if let Some(project_id) = project_id {
        query
            .push(" AND project_id = ")
            .push_bind(project_id.to_string());
    } else {
        query.push(" AND project_id = ''");
    }
    if let Some(cursor) = cursor {
        query
            .push(" AND (name COLLATE NOCASE > ")
            .push_bind(cursor.name.clone());
        query
            .push(" COLLATE NOCASE OR (name COLLATE NOCASE = ")
            .push_bind(cursor.name.clone());
        query
            .push(" COLLATE NOCASE AND id > ")
            .push_bind(cursor.id.clone())
            .push("))");
    }
    query
        .push(" ORDER BY name COLLATE NOCASE ASC, id ASC LIMIT ")
        .push_bind(limit);
    query
        .build_query_as()
        .fetch_all(&mut **transaction)
        .await
        .map_err(|error| format!("list notes workspace folders: {error}"))
}

fn push_page_cursor(query: &mut QueryBuilder<'_, Sqlite>, cursor: Option<&PageCursor>) {
    let Some(cursor) = cursor else { return };
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
