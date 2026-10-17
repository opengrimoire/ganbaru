//! Ownership-aware trash journaling for Notes page and block graphs.

use crate::{data_sources, page_history, project_history};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Sqlite, Transaction};
use std::collections::{HashSet, VecDeque};

const JOURNAL_KEY: &str = "__ganbaru_trash";
pub(super) const OWNER_KEY: &str = "__ganbaru_trash_owner";
const MAX_TRASH_OBJECTS: usize = super::database_copy::MAX_COPY_OBJECTS;

#[derive(Clone, Hash, PartialEq, Eq)]
enum TrashObject {
    Page(String),
    Block(String),
    Database(String),
    Source(String),
}

#[derive(Default, Serialize, Deserialize)]
struct TrashJournal {
    token: String,
    pages: Vec<String>,
    blocks: Vec<String>,
    databases: Vec<String>,
    sources: Vec<String>,
}

/// Move an owned block closure to Trash, or restore only records changed by that deletion.
pub(super) async fn set_block_trash(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    in_trash: bool,
) -> Result<(), String> {
    set_graph_trash(tx, TrashObject::Block(id.to_string()), in_trash).await
}

/// Move a page, nested notes, and owned database row graphs together.
pub(super) async fn set_page_trash(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    in_trash: bool,
) -> Result<(), String> {
    set_graph_trash(tx, TrashObject::Page(id.to_string()), in_trash).await
}

/// Reassign only owned note pages when a database or note block moves between projects.
pub(super) async fn adopt_block_project(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    project_id: Option<&str>,
) -> Result<(), String> {
    let graph = collect_graph(tx, TrashObject::Block(id.to_string()), None).await?;
    for page in graph.pages {
        let properties = match project_id {
            Some(_) => "json_set(properties, '$.__ganbaru_project_id', ?)",
            None => "json_remove(properties, '$.__ganbaru_project_id')",
        };
        let statement = format!("UPDATE notes_pages SET properties = {properties} WHERE id = ?");
        let mut query = sqlx::query(&statement);
        if let Some(project_id) = project_id {
            query = query.bind(project_id);
        }
        query
            .bind(&page)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("adopt moved Notes graph project: {e}"))?;
        project_history::mark_page_dirty_tx(tx, &page, "Move Notes content", true).await?;
    }
    Ok(())
}

async fn set_graph_trash(
    tx: &mut Transaction<'_, Sqlite>,
    root: TrashObject,
    in_trash: bool,
) -> Result<(), String> {
    let (table, column, id) = root_location(&root);
    let (state, raw): (i64, String) = sqlx::query_as(&format!(
        "SELECT in_trash, {column} FROM {table} WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load Notes trash root: {e}"))?
    .ok_or_else(|| "Notes trash target not found".to_string())?;
    let mut metadata: Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse Notes trash metadata: {e}"))?;
    if in_trash {
        if state != 0 {
            return Ok(());
        }
        let mut journal = collect_graph(tx, root.clone(), None).await?;
        journal.token = super::ids::new_note_id(tx, &mut HashSet::new()).await?;
        for page in &journal.pages {
            page_history::record_page_snapshot_tx(tx, page, "trash_page").await?;
            project_history::mark_page_dirty_tx(tx, page, "Trash Notes content", true).await?;
            sqlx::query("UPDATE notes_pages SET in_trash = 1, trashed_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                properties = json_set(properties, '$.__ganbaru_trash_owner', ?),
                last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND in_trash = 0")
                .bind(&journal.token).bind(page).execute(&mut **tx).await
                .map_err(|e| format!("trash owned Notes page: {e}"))?;
            data_sources::rollups::invalidate_rollup_cache_for_page_tx(tx, page).await?;
        }
        for block in &journal.blocks {
            sqlx::query("UPDATE notes_blocks SET in_trash = 1, payload = json_set(payload, '$.__ganbaru_trash_owner', ?),
                last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND in_trash = 0")
                .bind(&journal.token).bind(block).execute(&mut **tx).await
                .map_err(|e| format!("trash owned Notes block: {e}"))?;
        }
        for database in &journal.databases {
            sqlx::query("UPDATE notes_databases SET in_trash = 1,
                last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND in_trash = 0")
                .bind(database).execute(&mut **tx).await.map_err(|e| format!("trash Notes database shell: {e}"))?;
        }
        for source in &journal.sources {
            sqlx::query("UPDATE notes_data_sources SET in_trash = 1,
                last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND in_trash = 0")
                .bind(source).execute(&mut **tx).await.map_err(|e| format!("trash owned Notes database source: {e}"))?;
        }
        // The journal lists only active records changed by this operation.
        metadata[JOURNAL_KEY] = serde_json::to_value(&journal)
            .map_err(|e| format!("serialize Notes trash journal: {e}"))?;
        metadata[OWNER_KEY] = journal.token.into();
        write_metadata(tx, table, column, id, &metadata).await?;
    } else {
        let journal = metadata
            .get(JOURNAL_KEY)
            .cloned()
            .map(serde_json::from_value::<TrashJournal>)
            .transpose()
            .map_err(|e| format!("parse Notes trash journal: {e}"))?;
        if let Some(journal) = journal {
            validate_journal(&journal)?;
            restore_journal(tx, &journal).await?;
            let raw: String =
                sqlx::query_scalar(&format!("SELECT {column} FROM {table} WHERE id = ?"))
                    .bind(id)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| format!("reload restored Notes metadata: {e}"))?;
            metadata = serde_json::from_str(&raw)
                .map_err(|e| format!("parse restored Notes metadata: {e}"))?;
            if let Some(object) = metadata.as_object_mut() {
                object.remove(JOURNAL_KEY);
            }
            write_metadata(tx, table, column, id, &metadata).await?;
        } else if let Some(token) = metadata.get(OWNER_KEY).and_then(Value::as_str) {
            let mut journal = collect_graph(tx, root.clone(), Some(token)).await?;
            journal.token = token.to_string();
            validate_journal(&journal)?;
            restore_journal(tx, &journal).await?;
        } else {
            // History restoration and page moves can trash individual records without a graph journal.
            // Restore only that record, preserving independently trashed descendants.
            sqlx::query(&format!(
                "UPDATE {table} SET in_trash = 0,
                last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?"
            ))
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("restore individual Notes trash root: {e}"))?;
            if table == "notes_pages" {
                sqlx::query("UPDATE notes_pages SET trashed_time = NULL WHERE id = ?")
                    .bind(id)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| format!("clear restored Notes trash time: {e}"))?;
            }
        }
    }
    Ok(())
}

fn root_location(root: &TrashObject) -> (&'static str, &'static str, &str) {
    match root {
        TrashObject::Page(id) => ("notes_pages", "properties", id),
        TrashObject::Block(id) => ("notes_blocks", "payload", id),
        TrashObject::Database(id) => ("notes_databases", "description", id),
        TrashObject::Source(id) => ("notes_data_sources", "properties", id),
    }
}

async fn write_metadata(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    column: &str,
    id: &str,
    value: &Value,
) -> Result<(), String> {
    sqlx::query(&format!("UPDATE {table} SET {column} = ? WHERE id = ?"))
        .bind(value.to_string())
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("save Notes trash metadata: {e}"))?;
    Ok(())
}

async fn collect_graph(
    tx: &mut Transaction<'_, Sqlite>,
    root: TrashObject,
    token: Option<&str>,
) -> Result<TrashJournal, String> {
    let mut queue = VecDeque::from([root]);
    let mut seen = HashSet::new();
    let mut journal = TrashJournal::default();
    while let Some(object) = queue.pop_front() {
        if !seen.insert(object.clone()) {
            continue;
        }
        if seen.len() > MAX_TRASH_OBJECTS {
            return Err("Notes trash graph exceeds the object limit".to_string());
        }
        let (table, _, id) = root_location(&object);
        let (state_query, owner) = match (&object, token) {
            (TrashObject::Page(_), Some(_)) => ("in_trash = 1 AND json_extract(properties, '$.__ganbaru_trash_owner') = ?".to_string(), true),
            (TrashObject::Block(_), Some(_)) => ("in_trash = 1 AND json_extract(payload, '$.__ganbaru_trash_owner') = ?".to_string(), true),
            (TrashObject::Database(_), Some(_)) => ("in_trash = 1 AND EXISTS(SELECT 1 FROM notes_blocks WHERE id = notes_databases.id AND json_extract(payload, '$.__ganbaru_trash_owner') = ?)".to_string(), true),
            (TrashObject::Source(_), Some(_)) => ("in_trash = 1 AND EXISTS(SELECT 1 FROM notes_blocks WHERE id = notes_data_sources.database_id AND json_extract(payload, '$.__ganbaru_trash_owner') = ?)".to_string(), true),
            _ => ("in_trash = 0".to_string(), false),
        };
        let statement =
            format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ? AND {state_query})");
        let mut query = sqlx::query_scalar::<_, bool>(&statement).bind(id);
        if owner {
            query = query.bind(token);
        }
        let active = query
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("check Notes trash graph state: {e}"))?;
        if !active {
            continue;
        }
        match object {
            TrashObject::Page(id) => {
                journal.pages.push(id.clone());
                let blocks = bounded_ids(
                    tx,
                    "SELECT id FROM notes_blocks WHERE page_id = ? AND in_trash = 0 LIMIT ?",
                    &id,
                    token,
                )
                .await?;
                queue.extend(blocks.into_iter().map(TrashObject::Block));
                let children = bounded_ids(
                    tx,
                    "SELECT id FROM notes_pages WHERE parent_page_id = ? AND in_trash = 0 LIMIT ?",
                    &id,
                    token,
                )
                .await?;
                queue.extend(children.into_iter().map(TrashObject::Page));
            }
            TrashObject::Block(id) => {
                journal.blocks.push(id.clone());
                let children = bounded_ids(tx, "SELECT id FROM notes_blocks WHERE parent_block_id = ? AND in_trash = 0 LIMIT ?", &id, token).await?;
                queue.extend(children.into_iter().map(TrashObject::Block));
                let pages = bounded_ids(tx, "SELECT id FROM notes_pages WHERE (parent_block_id = ? OR id = ?) AND in_trash = 0 LIMIT ?", &id, token).await?;
                queue.extend(pages.into_iter().map(TrashObject::Page));
                queue.push_back(TrashObject::Database(id));
            }
            TrashObject::Database(id) => {
                journal.databases.push(id.clone());
                // A linked shell owns no shared source. Its views never widen this traversal.
                let sources = bounded_ids(tx, "SELECT id FROM notes_data_sources WHERE database_id = ? AND in_trash = 0 LIMIT ?", &id, token).await?;
                queue.extend(sources.into_iter().map(TrashObject::Source));
            }
            TrashObject::Source(id) => {
                journal.sources.push(id.clone());
                let rows = bounded_ids(tx, "SELECT id FROM notes_pages WHERE parent_data_source_id = ? AND in_trash = 0 LIMIT ?", &id, token).await?;
                queue.extend(rows.into_iter().map(TrashObject::Page));
            }
        }
    }
    Ok(journal)
}

async fn bounded_ids(
    tx: &mut Transaction<'_, Sqlite>,
    query: &str,
    id: &str,
    token: Option<&str>,
) -> Result<Vec<String>, String> {
    let query = if token.is_some() {
        query.replace("in_trash = 0", "in_trash = 1")
    } else {
        query.to_string()
    };
    let mut request = sqlx::query_scalar::<_, String>(&query).bind(id);
    if query.contains("OR id = ?") {
        request = request.bind(id);
    }
    let ids = request
        .bind((MAX_TRASH_OBJECTS + 1) as i64)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| format!("load owned Notes trash graph: {e}"))?;
    if ids.len() > MAX_TRASH_OBJECTS {
        return Err("Notes trash graph exceeds the object limit".to_string());
    }
    Ok(ids)
}

fn validate_journal(journal: &TrashJournal) -> Result<(), String> {
    crate::validation::require_uuid(&journal.token, "trash token")?;
    let count = journal.pages.len()
        + journal.blocks.len()
        + journal.databases.len()
        + journal.sources.len();
    if count > MAX_TRASH_OBJECTS {
        return Err("Notes trash journal exceeds the object limit".to_string());
    }
    for id in journal
        .pages
        .iter()
        .chain(&journal.blocks)
        .chain(&journal.databases)
        .chain(&journal.sources)
    {
        crate::validation::require_uuid(id, "trash object id")?;
    }
    Ok(())
}

async fn restore_journal(
    tx: &mut Transaction<'_, Sqlite>,
    journal: &TrashJournal,
) -> Result<(), String> {
    // Restore shells and sources only while their owning block still carries this deletion's token.
    for id in &journal.databases {
        sqlx::query("UPDATE notes_databases SET in_trash = 0,
            last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND EXISTS (
            SELECT 1 FROM notes_blocks WHERE id = notes_databases.id AND json_extract(payload, '$.__ganbaru_trash_owner') = ?)")
            .bind(id).bind(&journal.token).execute(&mut **tx).await.map_err(|e| format!("restore Notes database shell: {e}"))?;
    }
    for id in &journal.sources {
        sqlx::query("UPDATE notes_data_sources SET in_trash = 0,
            last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND EXISTS (
            SELECT 1 FROM notes_blocks WHERE id = notes_data_sources.database_id AND json_extract(payload, '$.__ganbaru_trash_owner') = ?)")
            .bind(id).bind(&journal.token).execute(&mut **tx).await.map_err(|e| format!("restore owned Notes data source: {e}"))?;
    }
    for id in &journal.pages {
        sqlx::query("UPDATE notes_pages SET in_trash = 0, trashed_time = NULL,
            properties = json_remove(properties, '$.__ganbaru_trash_owner'),
            last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND json_extract(properties, '$.__ganbaru_trash_owner') = ?")
            .bind(id).bind(&journal.token).execute(&mut **tx).await.map_err(|e| format!("restore owned Notes page: {e}"))?;
        data_sources::rollups::invalidate_rollup_cache_for_page_tx(tx, id).await?;
    }
    for id in &journal.blocks {
        sqlx::query("UPDATE notes_blocks SET in_trash = 0, payload = json_remove(payload, '$.__ganbaru_trash_owner'),
            last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND json_extract(payload, '$.__ganbaru_trash_owner') = ?")
            .bind(id).bind(&journal.token).execute(&mut **tx).await.map_err(|e| format!("restore owned Notes block: {e}"))?;
    }
    Ok(())
}
