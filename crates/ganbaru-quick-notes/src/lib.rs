//! Quick notes persistence independent of Tauri and the WebView: windowed lists and search,
//! revisioned writes, lifecycle and trash retention, ordering, tags, and resolution of
//! conflicts left by replicated edits.
//!
//! Callers supply an authorized vault pool and decide whether the device may write.

use std::collections::HashMap;

use ganbaru_sync::OPEN_CONFLICT_MASK_SQL;
use ganbaru_sync::manifest::vault::quick_notes::{NOTES_TABLE, note_group};
use ganbaru_sync_contracts::{Field, GroupId, OrderKey, Value};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool, Transaction};

mod conflicts;
mod order;
pub mod tags;

pub use conflicts::{
    QuickNoteConflictError, QuickNoteConflictRead, QuickNoteConflictResolution,
    QuickNoteConflictResolved, conflict_from_pool, resolve_from_pool,
};
use order::{OrderedRow, placement, take_key};
pub use tags::{QuickNoteTagError, QuickNoteTagRead, QuickNoteTagWrite};

const DEFAULT_PAGE_SIZE: i64 = 60;
const MAX_PAGE_SIZE: i64 = 60;
const MAX_TITLE_CHARS: usize = 200;
const MAX_BODY_CHARS: usize = 65_536;
const MAX_RUNS: usize = 4_096;
const PREVIEW_CHARS: usize = 4_096;
const TRASH_RETENTION_DAYS: i64 = 7;

/// Column `has_conflict` of a note aliased `q`: whether replicated edits left a conflict that
/// no local resolution settled.
fn has_conflict_column() -> String {
    format!(
        "EXISTS (SELECT 1 FROM sync_rows r
                 WHERE r.table_id = {} AND r.row_key = q.id AND {OPEN_CONFLICT_MASK_SQL} <> 0)
         AS has_conflict",
        NOTES_TABLE.0
    )
}

/// Stable write outcomes for conflict recovery, independent of diagnostic wording.
#[derive(Debug, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum QuickNoteWriteError {
    RevisionConflict(String),
    Failed(String),
}

impl From<String> for QuickNoteWriteError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteTextRun {
    content: String,
    bold: bool,
    italic: bool,
    underline: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteWrite {
    id: String,
    title: String,
    runs: Vec<QuickNoteTextRun>,
    color: i64,
    tag_id: Option<String>,
    pinned: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteUpdate {
    id: String,
    expected_revision: i64,
    title: String,
    runs: Vec<QuickNoteTextRun>,
    color: i64,
    tag_id: Option<String>,
    pinned: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteRevisionRequest {
    id: String,
    expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNotePinRequest {
    id: String,
    expected_revision: i64,
    pinned: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuickNotesCollection {
    Active,
    Archive,
    Trash,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNotesListRequest {
    collection: QuickNotesCollection,
    query: Option<String>,
    tag_id: Option<String>,
    cursor: Option<String>,
    page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteReorderRequest {
    id: String,
    previous_id: Option<String>,
    next_id: Option<String>,
    tag_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct QuickNotesCursor {
    pinned: i64,
    order_key: String,
    sort_time: String,
    id: String,
}

#[derive(Clone, Debug, FromRow)]
struct QuickNoteRow {
    id: String,
    title: String,
    body_plain_text: String,
    color: i64,
    tag_id: Option<String>,
    pinned: i64,
    archived: i64,
    trashed_at: Option<String>,
    revision: i64,
    order_key: String,
    created_at: String,
    updated_at: String,
    has_conflict: bool,
}

#[derive(Clone, Debug, FromRow)]
struct QuickNoteTextRunRow {
    note_id: String,
    content: String,
    bold: i64,
    italic: i64,
    underline: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteRead {
    id: String,
    title: String,
    body_plain_text: String,
    runs: Vec<QuickNoteTextRun>,
    preview_truncated: bool,
    color: i64,
    tag_id: Option<String>,
    pinned: bool,
    archived: bool,
    trashed_at: Option<String>,
    revision: i64,
    created_at: String,
    updated_at: String,
    /// Whether replicated edits left a conflict on the title or body.
    has_conflict: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNotesWindow {
    notes: Vec<QuickNoteRead>,
    next_cursor: Option<String>,
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.trim().is_empty() || id.len() > 128 {
        return Err("quick note id must be between 1 and 128 bytes".to_string());
    }
    Ok(())
}

fn validate_color(color: i64) -> Result<(), String> {
    if !(0..32).contains(&color) {
        return Err("quick note color must be between 0 and 31".to_string());
    }
    Ok(())
}

fn validate_optional_tag_id(tag_id: Option<&str>) -> Result<(), String> {
    if let Some(id) = tag_id {
        validate_id(id)?;
    }
    Ok(())
}

fn normalized_runs(runs: &[QuickNoteTextRun]) -> Result<Vec<QuickNoteTextRun>, String> {
    if runs.len() > MAX_RUNS {
        return Err(format!(
            "quick note body cannot contain more than {MAX_RUNS} text runs"
        ));
    }
    let mut output: Vec<QuickNoteTextRun> = Vec::new();
    let mut body_chars = 0;
    for run in runs {
        if run.content.is_empty() {
            continue;
        }
        body_chars += run.content.chars().count();
        if body_chars > MAX_BODY_CHARS {
            return Err(format!(
                "quick note body cannot exceed {MAX_BODY_CHARS} characters"
            ));
        }
        if let Some(previous) = output.last_mut() {
            if previous.bold == run.bold
                && previous.italic == run.italic
                && previous.underline == run.underline
            {
                previous.content.push_str(&run.content);
                continue;
            }
        }
        output.push(run.clone());
    }
    Ok(output)
}

fn validate_content(
    id: &str,
    title: &str,
    runs: &[QuickNoteTextRun],
    color: i64,
) -> Result<Vec<QuickNoteTextRun>, String> {
    validate_id(id)?;
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!(
            "quick note title cannot exceed {MAX_TITLE_CHARS} characters"
        ));
    }
    validate_color(color)?;
    normalized_runs(runs)
}

fn body_plain_text(runs: &[QuickNoteTextRun]) -> String {
    runs.iter().map(|run| run.content.as_str()).collect()
}

async fn replace_runs(
    tx: &mut Transaction<'_, Sqlite>,
    note_id: &str,
    runs: &[QuickNoteTextRun],
) -> Result<(), String> {
    sqlx::query("DELETE FROM quick_note_text_runs WHERE note_id = ?")
        .bind(note_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| format!("clear quick note text runs: {error}"))?;
    for (sort_order, run) in runs.iter().enumerate() {
        sqlx::query(
            "INSERT INTO quick_note_text_runs (
                note_id, sort_order, content, bold, italic, underline
             ) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(note_id)
        .bind(sort_order as i64)
        .bind(&run.content)
        .bind(i64::from(run.bold))
        .bind(i64::from(run.italic))
        .bind(i64::from(run.underline))
        .execute(&mut **tx)
        .await
        .map_err(|error| format!("insert quick note text run: {error}"))?;
    }
    Ok(())
}

async fn load_run_rows(
    pool: &SqlitePool,
    note_ids: &[String],
) -> Result<Vec<QuickNoteTextRunRow>, String> {
    if note_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT note_id, content, bold, italic, underline
         FROM quick_note_text_runs WHERE note_id IN (",
    );
    {
        let mut separated = query.separated(", ");
        for id in note_ids {
            separated.push_bind(id);
        }
    }
    query.push(") ORDER BY note_id, sort_order");
    query
        .build_query_as::<QuickNoteTextRunRow>()
        .fetch_all(pool)
        .await
        .map_err(|error| format!("load quick note text runs: {error}"))
}

fn read_runs(rows: Vec<QuickNoteTextRunRow>) -> HashMap<String, Vec<QuickNoteTextRun>> {
    let mut by_note: HashMap<String, Vec<QuickNoteTextRun>> = HashMap::new();
    for row in rows {
        by_note
            .entry(row.note_id)
            .or_default()
            .push(QuickNoteTextRun {
                content: row.content,
                bold: row.bold != 0,
                italic: row.italic != 0,
                underline: row.underline != 0,
            });
    }
    by_note
}

fn truncate_preview(runs: Vec<QuickNoteTextRun>) -> (Vec<QuickNoteTextRun>, bool) {
    let total = runs
        .iter()
        .map(|run| run.content.chars().count())
        .sum::<usize>();
    if total <= PREVIEW_CHARS {
        return (runs, false);
    }
    let mut remaining = PREVIEW_CHARS;
    let mut preview = Vec::new();
    for mut run in runs {
        if remaining == 0 {
            break;
        }
        let count = run.content.chars().count();
        if count > remaining {
            run.content = run.content.chars().take(remaining).collect();
        }
        remaining = remaining.saturating_sub(count);
        preview.push(run);
    }
    (preview, true)
}

fn read_note(mut row: QuickNoteRow, runs: Vec<QuickNoteTextRun>, preview: bool) -> QuickNoteRead {
    let (runs, preview_truncated) = if preview {
        row.body_plain_text = row.body_plain_text.chars().take(PREVIEW_CHARS).collect();
        truncate_preview(runs)
    } else {
        (runs, false)
    };
    QuickNoteRead {
        id: row.id,
        title: row.title,
        body_plain_text: row.body_plain_text,
        runs,
        preview_truncated,
        color: row.color,
        tag_id: row.tag_id,
        pinned: row.pinned != 0,
        archived: row.archived != 0,
        trashed_at: row.trashed_at,
        revision: row.revision,
        created_at: row.created_at,
        updated_at: row.updated_at,
        has_conflict: row.has_conflict,
    }
}

/// SQLite modifier that moves the current time back by the trash retention period.
fn trash_expiry_modifier() -> String {
    format!("-{TRASH_RETENTION_DAYS} days")
}

/// One note with its full body.
pub async fn load_note_from_pool(pool: &SqlitePool, id: &str) -> Result<QuickNoteRead, String> {
    validate_id(id)?;
    // Trash past its retention is hidden until the purge job deletes it.
    let sql = format!(
        "SELECT q.*, {} FROM quick_notes q
         WHERE q.id = ?
           AND (q.trashed_at IS NULL
                OR q.trashed_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?))",
        has_conflict_column()
    );
    let row = sqlx::query_as::<_, QuickNoteRow>(&sql)
        .bind(id)
        .bind(trash_expiry_modifier())
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("load quick note: {error}"))?
        .ok_or_else(|| "quick note not found".to_string())?;
    let rows = load_run_rows(pool, &[id.to_string()]).await?;
    Ok(read_note(
        row,
        read_runs(rows).remove(id).unwrap_or_default(),
        false,
    ))
}

/// Result of one trash purge pass.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNotesTrashPurge {
    purged: u64,
    /// When the oldest remaining trashed note expires, if this device may purge it.
    next_purge_at: Option<String>,
}

impl QuickNotesTrashPurge {
    /// The pass of a device that may not write Quick notes: nothing purged and no deadline.
    pub fn skipped() -> Self {
        Self {
            purged: 0,
            next_purge_at: None,
        }
    }
}

/// Deletes trash past its retention and reports when the oldest remaining trash expires.
pub async fn purge_expired_trash(pool: &SqlitePool) -> Result<QuickNotesTrashPurge, String> {
    let purged = sqlx::query(
        "DELETE FROM quick_notes
         WHERE trashed_at IS NOT NULL
           AND trashed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)",
    )
    .bind(trash_expiry_modifier())
    .execute(pool)
    .await
    .map_err(|error| format!("purge expired quick notes trash: {error}"))?
    .rows_affected();
    let next_purge_at = sqlx::query_scalar::<_, Option<String>>(
        "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', MIN(trashed_at), ?)
         FROM quick_notes WHERE trashed_at IS NOT NULL",
    )
    .bind(format!("+{TRASH_RETENTION_DAYS} days"))
    .fetch_one(pool)
    .await
    .map_err(|error| format!("load next quick notes trash expiry: {error}"))?;
    Ok(QuickNotesTrashPurge {
        purged,
        next_purge_at,
    })
}

fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn collection_predicate(collection: QuickNotesCollection) -> &'static str {
    match collection {
        QuickNotesCollection::Active => "q.archived = 0 AND q.trashed_at IS NULL",
        QuickNotesCollection::Archive => "q.archived = 1 AND q.trashed_at IS NULL",
        QuickNotesCollection::Trash => "q.trashed_at IS NOT NULL",
    }
}

fn cursor_pinned(row: &QuickNoteRow, collection: QuickNotesCollection) -> i64 {
    if collection == QuickNotesCollection::Active {
        row.pinned
    } else {
        0
    }
}

fn cursor_sort_time(row: &QuickNoteRow, collection: QuickNotesCollection) -> String {
    if collection == QuickNotesCollection::Trash {
        row.trashed_at
            .clone()
            .unwrap_or_else(|| row.updated_at.clone())
    } else {
        row.updated_at.clone()
    }
}

/// One page of a collection, filtered by search and tag, in display order.
pub async fn list_window_from_pool(
    pool: &SqlitePool,
    request: QuickNotesListRequest,
) -> Result<QuickNotesWindow, String> {
    let page_size = request.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if !(1..=MAX_PAGE_SIZE).contains(&page_size) {
        return Err(format!("page size must be between 1 and {MAX_PAGE_SIZE}"));
    }
    let cursor = request
        .cursor
        .map(|value| {
            serde_json::from_str::<QuickNotesCursor>(&value)
                .map_err(|_| "invalid quick notes cursor".to_string())
        })
        .transpose()?;
    let search = request.query.unwrap_or_default().trim().to_string();
    if search.chars().count() > 200 {
        return Err("quick notes search cannot exceed 200 characters".to_string());
    }
    validate_optional_tag_id(request.tag_id.as_deref())?;
    if request.tag_id.is_some() && request.collection != QuickNotesCollection::Active {
        return Err("quick note tag filters are available only in All".to_string());
    }

    let mut sql = QueryBuilder::<Sqlite>::new("SELECT q.*, ");
    sql.push(has_conflict_column()).push(" FROM quick_notes q");
    if !search.is_empty() {
        sql.push(" JOIN quick_notes_search_fts ON quick_notes_search_fts.note_id = q.id");
    }
    sql.push(" WHERE ")
        .push(collection_predicate(request.collection));
    if request.collection == QuickNotesCollection::Trash {
        // Trash past its retention is hidden until the purge job deletes it.
        sql.push(" AND q.trashed_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ")
            .push_bind(trash_expiry_modifier())
            .push(")");
    }
    if let Some(tag_id) = &request.tag_id {
        sql.push(" AND q.tag_id = ").push_bind(tag_id);
    }
    if !search.is_empty() {
        sql.push(" AND quick_notes_search_fts MATCH ")
            .push_bind(fts_query(&search));
    }
    if let Some(cursor) = &cursor {
        if request.collection == QuickNotesCollection::Active {
            sql.push(" AND (q.pinned < ").push_bind(cursor.pinned);
            sql.push(" OR (q.pinned = ").push_bind(cursor.pinned);
            sql.push(" AND (q.order_key > ")
                .push_bind(cursor.order_key.clone());
            sql.push(" OR (q.order_key = ")
                .push_bind(cursor.order_key.clone());
            sql.push(" AND q.id > ")
                .push_bind(cursor.id.clone())
                .push("))))");
        } else if request.collection == QuickNotesCollection::Archive {
            sql.push(" AND (q.updated_at < ")
                .push_bind(cursor.sort_time.clone());
            sql.push(" OR (q.updated_at = ")
                .push_bind(cursor.sort_time.clone());
            sql.push(" AND q.id > ")
                .push_bind(cursor.id.clone())
                .push("))");
        } else {
            sql.push(" AND (q.trashed_at < ")
                .push_bind(cursor.sort_time.clone());
            sql.push(" OR (q.trashed_at = ")
                .push_bind(cursor.sort_time.clone());
            sql.push(" AND q.id > ")
                .push_bind(cursor.id.clone())
                .push("))");
        }
    }
    match request.collection {
        QuickNotesCollection::Active => {
            sql.push(" ORDER BY q.pinned DESC, q.order_key ASC, q.id ASC")
        }
        QuickNotesCollection::Archive => sql.push(" ORDER BY q.updated_at DESC, q.id ASC"),
        QuickNotesCollection::Trash => sql.push(" ORDER BY q.trashed_at DESC, q.id ASC"),
    };
    sql.push(" LIMIT ").push_bind(page_size + 1);
    let mut rows = sql
        .build_query_as::<QuickNoteRow>()
        .fetch_all(pool)
        .await
        .map_err(|error| format!("list quick notes: {error}"))?;
    let has_more = rows.len() > page_size as usize;
    if has_more {
        rows.truncate(page_size as usize);
    }
    let next_cursor = if has_more {
        rows.last()
            .map(|row| QuickNotesCursor {
                pinned: cursor_pinned(row, request.collection),
                order_key: row.order_key.clone(),
                sort_time: cursor_sort_time(row, request.collection),
                id: row.id.clone(),
            })
            .map(|value| {
                serde_json::to_string(&value)
                    .map_err(|error| format!("serialize quick notes cursor: {error}"))
            })
            .transpose()?
    } else {
        None
    };
    let ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let mut runs = read_runs(load_run_rows(pool, &ids).await?);
    let notes = rows
        .into_iter()
        .map(|row| {
            let note_runs = runs.remove(&row.id).unwrap_or_default();
            read_note(row, note_runs, true)
        })
        .collect();
    Ok(QuickNotesWindow { notes, next_cursor })
}

#[derive(Debug, FromRow)]
struct QuickNoteOrderRow {
    id: String,
    tag_id: Option<String>,
    order_key: String,
}

impl From<&QuickNoteOrderRow> for OrderedRow {
    fn from(row: &QuickNoteOrderRow) -> Self {
        Self {
            id: row.id.clone(),
            order_key: row.order_key.clone(),
        }
    }
}

/// Active notes in one pinned group, excluding `id`, in display order.
async fn active_group(
    tx: &mut Transaction<'_, Sqlite>,
    pinned: bool,
    excluding: &str,
) -> Result<Vec<QuickNoteOrderRow>, String> {
    sqlx::query_as::<_, QuickNoteOrderRow>(
        "SELECT id, tag_id, order_key
         FROM quick_notes
         WHERE pinned = ? AND archived = 0 AND trashed_at IS NULL AND id <> ?
         ORDER BY order_key ASC, id ASC",
    )
    .bind(i64::from(pinned))
    .bind(excluding)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| format!("load quick note order group: {error}"))
}

async fn write_order_keys(
    tx: &mut Transaction<'_, Sqlite>,
    updates: &[(String, OrderKey)],
) -> Result<(), String> {
    for (id, key) in updates {
        sqlx::query("UPDATE quick_notes SET order_key = ? WHERE id = ?")
            .bind(key.as_str())
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(|error| format!("write quick note order: {error}"))?;
    }
    Ok(())
}

/// Places an existing note first in its active pinned group.
async fn move_to_front(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    pinned: bool,
) -> Result<(), String> {
    let group = active_group(tx, pinned, id).await?;
    let rows = group.iter().map(OrderedRow::from).collect::<Vec<_>>();
    write_order_keys(tx, &placement(&rows, 0, id)?).await
}

/// Creates a note first in its pinned group.
pub async fn create_from_pool(
    pool: &SqlitePool,
    note: QuickNoteWrite,
) -> Result<QuickNoteRead, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note create: {error}"))?;
    insert_note(&mut tx, &note).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note create: {error}"))?;
    load_note_from_pool(pool, &note.id).await
}

/// Inserts a note first in its pinned group.
async fn insert_note(
    tx: &mut Transaction<'_, Sqlite>,
    note: &QuickNoteWrite,
) -> Result<(), String> {
    let runs = validate_content(&note.id, &note.title, &note.runs, note.color)?;
    validate_optional_tag_id(note.tag_id.as_deref())?;
    let body = body_plain_text(&runs);
    let group = active_group(tx, note.pinned, &note.id).await?;
    let rows = group.iter().map(OrderedRow::from).collect::<Vec<_>>();
    let (order_key, others) = take_key(placement(&rows, 0, &note.id)?, &note.id)?;
    write_order_keys(tx, &others).await?;
    // A tag deleted by another window or device leaves the note untagged.
    sqlx::query(
        "INSERT INTO quick_notes (
             id, title, body_plain_text, color, tag_id, pinned, order_key
         ) VALUES (?, ?, ?, ?, (SELECT id FROM quick_note_tags WHERE id = ?), ?, ?)",
    )
    .bind(&note.id)
    .bind(note.title.trim())
    .bind(body)
    .bind(note.color)
    .bind(note.tag_id.as_deref())
    .bind(i64::from(note.pinned))
    .bind(order_key.as_str())
    .execute(&mut **tx)
    .await
    .map_err(|error| format!("create quick note: {error}"))?;
    replace_runs(tx, &note.id, &runs).await
}

/// A random version 4 UUID for notes created by the backend, in the form the frontend uses.
fn new_note_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "secure random generator is unavailable".to_string())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// Creates an active, unpinned note from the retained values of a deleted note offered for
/// recovery, first in its group. Returns the new note id.
pub async fn restore_recovered(
    pool: &SqlitePool,
    values: &[(GroupId, Value)],
) -> Result<String, String> {
    let mut note = QuickNoteWrite {
        id: new_note_id()?,
        title: String::new(),
        runs: Vec::new(),
        color: 0,
        tag_id: None,
        pinned: false,
    };
    for (group, value) in values {
        match (*group, value.fields()) {
            (note_group::TITLE, [Field::Text(title)]) => note.title = title.clone(),
            (note_group::BODY, _) => note.runs = conflicts::runs_from_value(value)?,
            (note_group::COLOR, [Field::Integer(color)]) => note.color = *color,
            (note_group::TAG, [Field::Text(tag_id)]) => note.tag_id = Some(tag_id.clone()),
            _ => {}
        }
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note restore: {error}"))?;
    insert_note(&mut tx, &note).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note restore: {error}"))?;
    Ok(note.id)
}

/// Replaces the content of a live note when its revision still matches.
pub async fn update_from_pool(
    pool: &SqlitePool,
    note: QuickNoteUpdate,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    let runs = validate_content(&note.id, &note.title, &note.runs, note.color)?;
    validate_optional_tag_id(note.tag_id.as_deref())?;
    if note.expected_revision < 1 {
        return Err("quick note revision must be positive".to_string().into());
    }
    let body = body_plain_text(&runs);
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note update: {error}"))?;
    let result = sqlx::query(
        "UPDATE quick_notes
         SET title = ?, body_plain_text = ?, color = ?,
             tag_id = (SELECT id FROM quick_note_tags WHERE id = ?),
             pinned = CASE WHEN archived = 0 AND trashed_at IS NULL THEN ? ELSE 0 END,
             revision = revision + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND revision = ? AND trashed_at IS NULL",
    )
    .bind(note.title.trim())
    .bind(body)
    .bind(note.color)
    .bind(note.tag_id)
    .bind(i64::from(note.pinned))
    .bind(&note.id)
    .bind(note.expected_revision)
    .execute(&mut *tx)
    .await
    .map_err(|error| format!("update quick note: {error}"))?;
    if result.rows_affected() != 1 {
        return Err(QuickNoteWriteError::RevisionConflict(
            "quick note revision conflict".to_string(),
        ));
    }
    replace_runs(&mut tx, &note.id, &runs).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note update: {error}"))?;
    load_note_from_pool(pool, &note.id)
        .await
        .map_err(Into::into)
}

/// Where a lifecycle change leaves the note in the active order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    Unchanged,
    FirstUnpinned,
}

async fn revision_mutation(
    pool: &SqlitePool,
    request: &QuickNoteRevisionRequest,
    sql: &str,
    context: &str,
    placement: Placement,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    validate_id(&request.id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin {context}: {error}"))?;
    let result = sqlx::query(sql)
        .bind(&request.id)
        .bind(request.expected_revision)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("{context}: {error}"))?;
    if result.rows_affected() != 1 {
        return Err(QuickNoteWriteError::RevisionConflict(
            "quick note revision conflict".to_string(),
        ));
    }
    if placement == Placement::FirstUnpinned {
        move_to_front(&mut tx, &request.id, false).await?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit {context}: {error}"))?;
    load_note_from_pool(pool, &request.id)
        .await
        .map_err(Into::into)
}

/// Pins or unpins an active note; a note that changes group moves to the front of it.
pub async fn set_pinned_from_pool(
    pool: &SqlitePool,
    request: QuickNotePinRequest,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    validate_id(&request.id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note pin: {error}"))?;
    let current = sqlx::query_scalar::<_, i64>(
        "SELECT pinned FROM quick_notes
         WHERE id = ? AND revision = ? AND archived = 0 AND trashed_at IS NULL",
    )
    .bind(&request.id)
    .bind(request.expected_revision)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| format!("load quick note pinned state: {error}"))?
    .ok_or_else(|| {
        QuickNoteWriteError::RevisionConflict("quick note revision conflict".to_string())
    })?;
    sqlx::query(
        "UPDATE quick_notes
         SET pinned = ?, revision = revision + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(i64::from(request.pinned))
    .bind(&request.id)
    .execute(&mut *tx)
    .await
    .map_err(|error| format!("set quick note pinned state: {error}"))?;
    if current != i64::from(request.pinned) {
        move_to_front(&mut tx, &request.id, request.pinned).await?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note pin: {error}"))?;
    load_note_from_pool(pool, &request.id)
        .await
        .map_err(Into::into)
}

/// Moves an active note between two adjacent visible neighbors of its pinned group.
pub async fn reorder_from_pool(
    pool: &SqlitePool,
    request: QuickNoteReorderRequest,
) -> Result<(), String> {
    validate_id(&request.id)?;
    validate_optional_tag_id(request.previous_id.as_deref())?;
    validate_optional_tag_id(request.next_id.as_deref())?;
    validate_optional_tag_id(request.tag_id.as_deref())?;
    if request.previous_id.as_deref() == Some(request.id.as_str())
        || request.next_id.as_deref() == Some(request.id.as_str())
        || request.previous_id == request.next_id
    {
        return Err("quick note reorder anchors must be distinct".to_string());
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note reorder: {error}"))?;
    let (pinned, tag_id) = sqlx::query_as::<_, (i64, Option<String>)>(
        "SELECT pinned, tag_id FROM quick_notes
         WHERE id = ? AND archived = 0 AND trashed_at IS NULL",
    )
    .bind(&request.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| format!("load quick note reorder source: {error}"))?
    .ok_or_else(|| "active quick note not found".to_string())?;
    if request.tag_id.is_some() && tag_id != request.tag_id {
        return Err("quick note reorder source is outside the selected tag".to_string());
    }
    let rows = active_group(&mut tx, pinned != 0, &request.id).await?;

    let is_visible = |row: &&QuickNoteOrderRow| match request.tag_id.as_ref() {
        Some(tag_id) => row.tag_id.as_ref() == Some(tag_id),
        None => true,
    };
    let visible_ids = rows
        .iter()
        .filter(is_visible)
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>();
    let visible_position = |anchor: Option<&str>, label: &str| -> Result<Option<usize>, String> {
        match anchor {
            Some(id) => visible_ids
                .iter()
                .position(|candidate| *candidate == id)
                .map(Some)
                .ok_or_else(|| format!("quick note reorder {label} anchor is invalid")),
            None => Ok(None),
        }
    };
    let previous_visible_index = visible_position(request.previous_id.as_deref(), "previous")?;
    let next_visible_index = visible_position(request.next_id.as_deref(), "next")?;
    if let (Some(previous), Some(next)) = (previous_visible_index, next_visible_index) {
        if previous + 1 != next {
            return Err("quick note reorder anchors are not adjacent".to_string());
        }
    }

    let insertion_index = if let Some(next_id) = request.next_id.as_deref() {
        rows.iter()
            .position(|row| row.id == next_id)
            .ok_or_else(|| "quick note reorder next anchor is unavailable".to_string())?
    } else if let Some(previous_id) = request.previous_id.as_deref() {
        rows.iter()
            .position(|row| row.id == previous_id)
            .map(|index| index + 1)
            .ok_or_else(|| "quick note reorder previous anchor is unavailable".to_string())?
    } else {
        0
    };
    let ordered = rows.iter().map(OrderedRow::from).collect::<Vec<_>>();
    write_order_keys(&mut tx, &placement(&ordered, insertion_index, &request.id)?).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note reorder: {error}"))?;
    Ok(())
}

const ARCHIVE_SQL: &str =
    "UPDATE quick_notes SET archived = 1, pinned = 0, revision = revision + 1,
     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
     WHERE id = ? AND revision = ? AND archived = 0 AND trashed_at IS NULL";

const UNARCHIVE_SQL: &str = "UPDATE quick_notes SET archived = 0, pinned = 0,
     revision = revision + 1,
     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
     WHERE id = ? AND revision = ? AND archived = 1 AND trashed_at IS NULL";

const TRASH_SQL: &str = "UPDATE quick_notes SET trashed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
     pinned = 0, revision = revision + 1,
     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
     WHERE id = ? AND revision = ? AND trashed_at IS NULL";

const RESTORE_SQL: &str = "UPDATE quick_notes SET trashed_at = NULL, pinned = 0,
     revision = revision + 1,
     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
     WHERE id = ? AND revision = ? AND trashed_at IS NOT NULL";

/// Archives an active note and unpins it.
pub async fn archive_from_pool(
    pool: &SqlitePool,
    request: &QuickNoteRevisionRequest,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    revision_mutation(
        pool,
        request,
        ARCHIVE_SQL,
        "archive quick note",
        Placement::Unchanged,
    )
    .await
}

/// Returns an archived note to the front of the unpinned active notes.
pub async fn unarchive_from_pool(
    pool: &SqlitePool,
    request: &QuickNoteRevisionRequest,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    revision_mutation(
        pool,
        request,
        UNARCHIVE_SQL,
        "unarchive quick note",
        Placement::FirstUnpinned,
    )
    .await
}

/// Moves a note to the trash and unpins it.
pub async fn trash_from_pool(
    pool: &SqlitePool,
    request: &QuickNoteRevisionRequest,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    revision_mutation(
        pool,
        request,
        TRASH_SQL,
        "trash quick note",
        Placement::Unchanged,
    )
    .await
}

/// Restores a trashed note to the front of the unpinned active notes.
pub async fn restore_from_pool(
    pool: &SqlitePool,
    request: &QuickNoteRevisionRequest,
) -> Result<QuickNoteRead, QuickNoteWriteError> {
    revision_mutation(
        pool,
        request,
        RESTORE_SQL,
        "restore quick note",
        Placement::FirstUnpinned,
    )
    .await
}

/// Deletes one trashed note.
pub async fn delete_permanently_from_pool(pool: &SqlitePool, id: &str) -> Result<(), String> {
    validate_id(id)?;
    let result = sqlx::query("DELETE FROM quick_notes WHERE id = ? AND trashed_at IS NOT NULL")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|error| format!("delete quick note permanently: {error}"))?;
    if result.rows_affected() != 1 {
        return Err("trashed quick note not found".to_string());
    }
    Ok(())
}

/// Deletes every trashed note and returns how many were deleted.
pub async fn empty_trash_from_pool(pool: &SqlitePool) -> Result<u64, String> {
    sqlx::query("DELETE FROM quick_notes WHERE trashed_at IS NOT NULL")
        .execute(pool)
        .await
        .map(|result| result.rows_affected())
        .map_err(|error| format!("empty quick notes trash: {error}"))
}

#[cfg(test)]
mod tests;
