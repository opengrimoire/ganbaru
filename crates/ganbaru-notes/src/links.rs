pub mod backlinks;
pub mod facts;

use crate::models::{
    NotePageAliasCreate, NotePageAliasDto, NotePageAliasRow, NoteUnresolvedLinkDto,
    NoteUnresolvedLinkResolve, NoteUnresolvedLinkRow,
};
use crate::validation::require_uuid;
mod resolver;

pub use self::resolver::{
    LocalLinkResolver, block_id_from_local_notes_url, canonical_notes_id, local_link_resolver,
    page_ids_from_local_notes_url,
};
use self::resolver::{UnresolvedCandidate, normalize_alias, unresolved_candidates};

use crate::page_history;
use serde_json::Value;
use sqlx::{FromRow, Row, Sqlite, SqlitePool, Transaction};
use std::collections::HashSet;

const FINGERPRINT_KEY: &str = "source_fingerprint";
const MAX_ALIAS_CHARS: usize = 200;

pub fn normalized_page_alias(value: &str) -> Option<String> {
    normalize_alias(value)
}

pub async fn list_page_aliases(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<Vec<NotePageAliasDto>, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    ensure_active_page(pool, page_id).await?;
    let rows = sqlx::query_as::<_, NotePageAliasRow>(
        "SELECT *
         FROM notes_page_aliases
         WHERE page_id = ?
         ORDER BY alias COLLATE NOCASE ASC, id ASC",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes page aliases: {e}"))?;
    Ok(rows.into_iter().map(NotePageAliasDto::new).collect())
}

pub async fn add_page_alias(
    pool: &SqlitePool,
    page_id: &str,
    request: NotePageAliasCreate,
) -> Result<Vec<NotePageAliasDto>, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    require_uuid(request.id.trim(), "id")?;
    let alias = normalize_alias_display(&request.alias)?;
    let normalized_alias = normalize_alias(&alias)
        .ok_or_else(|| "page alias must contain searchable text".to_string())?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page alias create: {e}"))?;
    ensure_active_page_tx(&mut tx, page_id).await?;
    sqlx::query(
        "INSERT INTO notes_page_aliases (id, page_id, alias, normalized_alias)
         VALUES (?, ?, ?, ?)",
    )
    .bind(request.id.trim())
    .bind(page_id)
    .bind(&alias)
    .bind(&normalized_alias)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        if is_unique_error(&e) {
            "page alias already exists".to_string()
        } else {
            format!("create notes page alias: {e}")
        }
    })?;
    touch_page_tx(&mut tx, page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page alias create: {e}"))?;
    list_page_aliases(pool, page_id).await
}

pub async fn delete_page_alias(
    pool: &SqlitePool,
    page_id: &str,
    alias_id: &str,
) -> Result<Vec<NotePageAliasDto>, String> {
    let page_id = page_id.trim();
    let alias_id = alias_id.trim();
    require_uuid(page_id, "page_id")?;
    require_uuid(alias_id, "alias_id")?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page alias delete: {e}"))?;
    ensure_active_page_tx(&mut tx, page_id).await?;
    sqlx::query(
        "DELETE FROM notes_page_aliases
         WHERE id = ? AND page_id = ?",
    )
    .bind(alias_id)
    .bind(page_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("delete notes page alias: {e}"))?;
    touch_page_tx(&mut tx, page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page alias delete: {e}"))?;
    list_page_aliases(pool, page_id).await
}

pub async fn list_unresolved_links(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<Vec<NoteUnresolvedLinkDto>, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    ensure_active_page(pool, page_id).await?;
    ensure_unresolved_index_current(pool).await?;
    let rows = sqlx::query_as::<_, NoteUnresolvedLinkRow>(
        "SELECT idx.*
         FROM notes_unresolved_link_index AS idx
         JOIN notes_pages AS source_page ON source_page.id = idx.source_page_id
         LEFT JOIN notes_blocks AS block ON block.id = idx.source_block_id
         LEFT JOIN notes_comments AS comment ON comment.id = idx.source_comment_id
         LEFT JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id
         LEFT JOIN notes_blocks AS thread_block ON thread_block.id = thread.parent_block_id
         WHERE idx.source_page_id = ?
           AND source_page.in_trash = 0
           AND source_page.archived = 0
           AND (
               source_page.parent_type != 'data_source_id'
               OR EXISTS (
                   SELECT 1
                   FROM notes_data_sources AS data_source
                   JOIN notes_databases AS database ON database.id = data_source.database_id
                   WHERE data_source.id = source_page.parent_data_source_id
                     AND data_source.in_trash = 0
                     AND database.in_trash = 0
               )
           )
           AND (
               idx.source_block_id IS NULL
               OR (block.id IS NOT NULL AND block.in_trash = 0)
           )
           AND (
               idx.source_comment_id IS NULL
               OR (
                   comment.id IS NOT NULL
                   AND comment.deleted_at IS NULL
                   AND thread.id IS NOT NULL
                   AND (
                       thread.parent_block_id IS NULL
                       OR (thread_block.id IS NOT NULL AND thread_block.in_trash = 0)
                   )
               )
           )
         ORDER BY idx.last_edited_time DESC, idx.raw_target COLLATE NOCASE ASC, idx.id ASC",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes unresolved links: {e}"))?;
    Ok(rows.into_iter().map(NoteUnresolvedLinkDto::new).collect())
}

pub async fn resolve_unresolved_link(
    pool: &SqlitePool,
    link_id: &str,
    request: NoteUnresolvedLinkResolve,
) -> Result<Vec<NoteUnresolvedLinkDto>, String> {
    let link_id = link_id.trim();
    let target_page_id = request.target_page_id.trim().to_string();
    if link_id.is_empty() {
        return Err("unresolved link id is required".to_string());
    }
    require_uuid(&target_page_id, "target_page_id")?;
    ensure_unresolved_index_current(pool).await?;
    let row = sqlx::query_as::<_, NoteUnresolvedLinkRow>(
        "SELECT *
         FROM notes_unresolved_link_index
         WHERE id = ?",
    )
    .bind(link_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes unresolved link: {e}"))?
    .ok_or_else(|| "unresolved link not found".to_string())?;
    ensure_active_page(pool, &target_page_id).await?;
    let target_url = local_notes_page_url(&target_page_id);
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes unresolved link resolution: {e}"))?;
    if row.source_type == "block" {
        resolve_block_link_tx(&mut tx, &row, &target_url).await?;
    } else {
        resolve_comment_link_tx(&mut tx, &row, &target_url).await?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit notes unresolved link resolution: {e}"))?;
    list_unresolved_links(pool, &row.source_page_id).await
}

pub async fn rebuild_unresolved_index(pool: &SqlitePool) -> Result<i64, String> {
    let fingerprint = unresolved_source_fingerprint(pool).await?;
    let entries = build_unresolved_entries(pool).await?;
    let entry_count = entries.len() as i64;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes unresolved link rebuild: {e}"))?;
    sqlx::query("DELETE FROM notes_unresolved_link_index")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("clear notes unresolved link index: {e}"))?;
    for entry in &entries {
        sqlx::query(
            "INSERT INTO notes_unresolved_link_index (
                id,
                source_type,
                source_page_id,
                source_block_id,
                source_comment_id,
                raw_url,
                raw_target,
                normalized_target,
                link_text,
                snippet,
                created_time,
                last_edited_time
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.id)
        .bind(&entry.source_type)
        .bind(&entry.source_page_id)
        .bind(&entry.source_block_id)
        .bind(&entry.source_comment_id)
        .bind(&entry.raw_url)
        .bind(&entry.raw_target)
        .bind(&entry.normalized_target)
        .bind(&entry.link_text)
        .bind(&entry.snippet)
        .bind(&entry.created_time)
        .bind(&entry.last_edited_time)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("insert notes unresolved link row: {e}"))?;
    }
    sqlx::query(
        "INSERT INTO notes_unresolved_link_index_state (key, value, updated_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at",
    )
    .bind(FINGERPRINT_KEY)
    .bind(&fingerprint.value)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("record notes unresolved link fingerprint: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes unresolved link rebuild: {e}"))?;
    Ok(entry_count)
}

async fn ensure_unresolved_index_current(pool: &SqlitePool) -> Result<(), String> {
    let fingerprint = unresolved_source_fingerprint(pool).await?;
    let stored: Option<String> =
        sqlx::query_scalar("SELECT value FROM notes_unresolved_link_index_state WHERE key = ?")
            .bind(FINGERPRINT_KEY)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("load notes unresolved link fingerprint: {e}"))?;
    let index_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_unresolved_link_index")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("count notes unresolved link rows: {e}"))?;
    if stored.as_deref() != Some(fingerprint.value.as_str())
        || (fingerprint.source_rows > 0 && index_rows == 0)
    {
        rebuild_unresolved_index(pool).await?;
    }
    Ok(())
}

async fn build_unresolved_entries(pool: &SqlitePool) -> Result<Vec<UnresolvedLinkEntry>, String> {
    let resolver = local_link_resolver(pool).await?;
    let mut entries = Vec::new();
    append_block_unresolved_entries(pool, &resolver, &mut entries).await?;
    append_comment_unresolved_entries(pool, &resolver, &mut entries).await?;
    Ok(entries)
}

async fn append_block_unresolved_entries(
    pool: &SqlitePool,
    resolver: &LocalLinkResolver,
    entries: &mut Vec<UnresolvedLinkEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, UnresolvedBlockSourceRow>(
        "SELECT id, page_id, payload, plain_text, type AS block_type, created_time, last_edited_time
         FROM notes_blocks",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes blocks for unresolved links: {e}"))?;
    for row in rows {
        let payload: Value = serde_json::from_str(&row.payload)
            .map_err(|e| format!("parse notes block links: {e}"))?;
        let candidates = unresolved_candidates(&payload, resolver);
        push_unresolved_candidates(
            entries,
            SourceLinkEntry {
                source_type: "block",
                source_page_id: &row.page_id,
                source_block_id: Some(&row.id),
                source_comment_id: None,
                snippet: source_snippet(&row.plain_text, &row.block_type),
                created_time: &row.created_time,
                last_edited_time: &row.last_edited_time,
            },
            candidates,
        );
    }
    Ok(())
}

async fn append_comment_unresolved_entries(
    pool: &SqlitePool,
    resolver: &LocalLinkResolver,
    entries: &mut Vec<UnresolvedLinkEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, UnresolvedCommentSourceRow>(
        "SELECT comment.id,
                comment.rich_text,
                comment.plain_text,
                comment.created_time,
                comment.last_edited_time,
                thread.page_id,
                thread.parent_block_id
         FROM notes_comments AS comment
         JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes comments for unresolved links: {e}"))?;
    for row in rows {
        let rich_text: Value = serde_json::from_str(&row.rich_text)
            .map_err(|e| format!("parse notes comment links: {e}"))?;
        let candidates = unresolved_candidates(&rich_text, resolver);
        push_unresolved_candidates(
            entries,
            SourceLinkEntry {
                source_type: "comment",
                source_page_id: &row.page_id,
                source_block_id: row.parent_block_id.as_deref(),
                source_comment_id: Some(&row.id),
                snippet: comment_snippet(&row.plain_text),
                created_time: &row.created_time,
                last_edited_time: &row.last_edited_time,
            },
            candidates,
        );
    }
    Ok(())
}

fn push_unresolved_candidates(
    entries: &mut Vec<UnresolvedLinkEntry>,
    source: SourceLinkEntry<'_>,
    candidates: Vec<UnresolvedCandidate>,
) {
    let mut seen = HashSet::new();
    for candidate in candidates {
        let key = format!("{}:{}", candidate.raw_url, candidate.normalized_target);
        if !seen.insert(key) {
            continue;
        }
        let source_id = source
            .source_comment_id
            .or(source.source_block_id)
            .unwrap_or(source.source_page_id);
        let id = format!(
            "unresolved:{}:{}:{}",
            source.source_type,
            source_id,
            stable_hex(&format!(
                "{}:{}",
                candidate.raw_url, candidate.normalized_target
            ))
        );
        entries.push(UnresolvedLinkEntry {
            id,
            source_type: source.source_type.to_string(),
            source_page_id: source.source_page_id.to_string(),
            source_block_id: source.source_block_id.map(str::to_string),
            source_comment_id: source.source_comment_id.map(str::to_string),
            raw_url: candidate.raw_url,
            raw_target: candidate.raw_target,
            normalized_target: candidate.normalized_target,
            link_text: candidate.link_text,
            snippet: source.snippet.clone(),
            created_time: source.created_time.to_string(),
            last_edited_time: source.last_edited_time.to_string(),
        });
    }
}

async fn resolve_block_link_tx(
    tx: &mut Transaction<'_, Sqlite>,
    row: &NoteUnresolvedLinkRow,
    target_url: &str,
) -> Result<(), String> {
    let block_id = row
        .source_block_id
        .as_deref()
        .ok_or_else(|| "unresolved block link is missing a block id".to_string())?;
    let block = sqlx::query_as::<_, ResolveBlockRow>(
        "SELECT id, page_id, payload
         FROM notes_blocks
         WHERE id = ? AND in_trash = 0",
    )
    .bind(block_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes unresolved block source: {e}"))?
    .ok_or_else(|| "unresolved link source block not found".to_string())?;
    let mut payload: Value = serde_json::from_str(&block.payload)
        .map_err(|e| format!("parse notes unresolved block payload: {e}"))?;
    if !replace_url_value(&mut payload, &row.raw_url, target_url) {
        return Err("unresolved link source no longer contains that URL".to_string());
    }
    page_history::record_page_snapshot_tx(tx, &block.page_id, "resolve_unresolved_link").await?;
    sqlx::query(
        "UPDATE notes_blocks
         SET payload = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND in_trash = 0",
    )
    .bind(payload.to_string())
    .bind(&block.id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("resolve notes block link: {e}"))?;
    touch_page_tx(tx, &block.page_id).await
}

async fn resolve_comment_link_tx(
    tx: &mut Transaction<'_, Sqlite>,
    row: &NoteUnresolvedLinkRow,
    target_url: &str,
) -> Result<(), String> {
    let comment_id = row
        .source_comment_id
        .as_deref()
        .ok_or_else(|| "unresolved comment link is missing a comment id".to_string())?;
    let comment = sqlx::query_as::<_, ResolveCommentRow>(
        "SELECT comment.id,
                comment.thread_id,
                comment.rich_text,
                thread.page_id
         FROM notes_comments AS comment
         JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id
         WHERE comment.id = ?
           AND comment.deleted_at IS NULL",
    )
    .bind(comment_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes unresolved comment source: {e}"))?
    .ok_or_else(|| "unresolved link source comment not found".to_string())?;
    let mut rich_text: Value = serde_json::from_str(&comment.rich_text)
        .map_err(|e| format!("parse notes unresolved comment rich_text: {e}"))?;
    if !replace_url_value(&mut rich_text, &row.raw_url, target_url) {
        return Err("unresolved link source no longer contains that URL".to_string());
    }
    sqlx::query(
        "UPDATE notes_comments
         SET rich_text = ?,
             sync_version = sync_version + 1,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(rich_text.to_string())
    .bind(&comment.id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("resolve notes comment link: {e}"))?;
    sqlx::query(
        "UPDATE notes_comment_threads
         SET sync_version = sync_version + 1,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(&comment.thread_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch notes comment thread after link resolution: {e}"))?;
    touch_page_tx(tx, &comment.page_id).await
}

fn replace_url_value(value: &mut Value, raw_url: &str, target_url: &str) -> bool {
    match value {
        Value::Object(object) => {
            let mut changed = false;
            for field in ["href", "url"] {
                if object.get(field).and_then(Value::as_str) == Some(raw_url) {
                    object.insert(field.to_string(), Value::String(target_url.to_string()));
                    changed = true;
                }
            }
            if object
                .get("text")
                .and_then(|text| text.get("link"))
                .and_then(|link| link.get("url"))
                .and_then(Value::as_str)
                == Some(raw_url)
            {
                if let Some(link) = object
                    .get_mut("text")
                    .and_then(|text| text.get_mut("link"))
                    .and_then(Value::as_object_mut)
                {
                    link.insert("url".to_string(), Value::String(target_url.to_string()));
                    changed = true;
                }
            }
            if object
                .get("link")
                .and_then(|link| link.get("url"))
                .and_then(Value::as_str)
                == Some(raw_url)
            {
                if let Some(link) = object.get_mut("link").and_then(Value::as_object_mut) {
                    link.insert("url".to_string(), Value::String(target_url.to_string()));
                    changed = true;
                }
            }
            for nested in object.values_mut() {
                changed |= replace_url_value(nested, raw_url, target_url);
            }
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= replace_url_value(item, raw_url, target_url);
            }
            changed
        }
        _ => false,
    }
}

async fn unresolved_source_fingerprint(
    pool: &SqlitePool,
) -> Result<UnresolvedSourceFingerprint, String> {
    let rows = sqlx::query(
        "SELECT 'pages' AS source, COUNT(*) AS source_count,
            COALESCE(MAX(last_edited_time), '') AS source_marker
         FROM notes_pages
         UNION ALL
         SELECT 'page_aliases', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_page_aliases
         UNION ALL
         SELECT 'blocks', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_blocks
         UNION ALL
         SELECT 'comment_threads', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_comment_threads
         UNION ALL
         SELECT 'comments', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_comments
         UNION ALL
         SELECT 'data_sources', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_data_sources
         UNION ALL
         SELECT 'databases', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_databases",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("compute notes unresolved link source fingerprint: {e}"))?;
    let mut source_rows = 0;
    let mut value = String::new();
    for row in rows {
        let source: String = row.get("source");
        let source_count: i64 = row.get("source_count");
        let source_marker: String = row.get("source_marker");
        source_rows += source_count;
        value.push_str(&source);
        value.push(':');
        value.push_str(&source_count.to_string());
        value.push(':');
        value.push_str(&source_marker);
        value.push(';');
    }
    Ok(UnresolvedSourceFingerprint { value, source_rows })
}

fn normalize_alias_display(alias: &str) -> Result<String, String> {
    if alias.chars().any(char::is_control) {
        return Err("page alias must not contain control characters".to_string());
    }
    let normalized = alias.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err("page alias must not be empty".to_string());
    }
    if normalized.chars().count() > MAX_ALIAS_CHARS {
        return Err("page alias must be 200 characters or fewer".to_string());
    }
    Ok(normalized)
}

fn is_unique_error(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database_error| database_error.is_unique_violation())
}

async fn ensure_active_page(pool: &SqlitePool, page_id: &str) -> Result<(), String> {
    if active_page_exists(pool, page_id).await? {
        Ok(())
    } else {
        Err("notes page not found".to_string())
    }
}

async fn ensure_active_page_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<(), String> {
    let exists: i64 = sqlx::query_scalar(active_page_exists_sql())
        .bind(page_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("check notes page exists: {e}"))?;
    if exists > 0 {
        Ok(())
    } else {
        Err("notes page not found".to_string())
    }
}

async fn active_page_exists(pool: &SqlitePool, page_id: &str) -> Result<bool, String> {
    let exists: i64 = sqlx::query_scalar(active_page_exists_sql())
        .bind(page_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("check notes page exists: {e}"))?;
    Ok(exists > 0)
}

fn active_page_exists_sql() -> &'static str {
    "SELECT COUNT(*)
     FROM notes_pages AS page
     WHERE page.id = ?
       AND page.in_trash = 0
       AND page.archived = 0
       AND (
           page.parent_type != 'data_source_id'
           OR EXISTS (
               SELECT 1
               FROM notes_data_sources AS data_source
               JOIN notes_databases AS database ON database.id = data_source.database_id
               WHERE data_source.id = page.parent_data_source_id
                 AND data_source.in_trash = 0
                 AND database.in_trash = 0
           )
       )"
}

async fn touch_page_tx(tx: &mut Transaction<'_, Sqlite>, page_id: &str) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_pages
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch notes page: {e}"))?;
    Ok(())
}

fn local_notes_page_url(page_id: &str) -> String {
    format!("http://localhost:1420/?view=notes#notes?page={page_id}")
}

fn source_snippet(plain_text: &str, block_type: &str) -> String {
    let trimmed = plain_text.trim();
    if trimmed.is_empty() {
        block_type.to_string()
    } else {
        trimmed.chars().take(160).collect()
    }
}

fn comment_snippet(plain_text: &str) -> String {
    let trimmed = plain_text.trim();
    if trimmed.is_empty() {
        "Comment".to_string()
    } else {
        trimmed.chars().take(160).collect()
    }
}

fn stable_hex(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[derive(Clone)]
struct SourceLinkEntry<'a> {
    source_type: &'a str,
    source_page_id: &'a str,
    source_block_id: Option<&'a str>,
    source_comment_id: Option<&'a str>,
    snippet: String,
    created_time: &'a str,
    last_edited_time: &'a str,
}

struct UnresolvedLinkEntry {
    id: String,
    source_type: String,
    source_page_id: String,
    source_block_id: Option<String>,
    source_comment_id: Option<String>,
    raw_url: String,
    raw_target: String,
    normalized_target: String,
    link_text: String,
    snippet: String,
    created_time: String,
    last_edited_time: String,
}

struct UnresolvedSourceFingerprint {
    value: String,
    source_rows: i64,
}

#[derive(FromRow)]
struct UnresolvedBlockSourceRow {
    id: String,
    page_id: String,
    payload: String,
    plain_text: String,
    block_type: String,
    created_time: String,
    last_edited_time: String,
}

#[derive(FromRow)]
struct UnresolvedCommentSourceRow {
    id: String,
    rich_text: String,
    plain_text: String,
    created_time: String,
    last_edited_time: String,
    page_id: String,
    parent_block_id: Option<String>,
}

#[derive(FromRow)]
struct ResolveBlockRow {
    id: String,
    page_id: String,
    payload: String,
}

#[derive(FromRow)]
struct ResolveCommentRow {
    id: String,
    thread_id: String,
    rich_text: String,
    page_id: String,
}
