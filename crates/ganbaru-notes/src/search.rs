pub mod properties;

use crate::models::{
    NoteBlockRow, NoteCommentAnchorRow, NoteCommentRow, NotePageAliasRow, NotePageSummaryDto,
    NoteSearchResultDto, NoteSearchWindowDto,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, QueryBuilder, Row, Sqlite, SqlitePool};
use std::cmp::Ordering;
use std::collections::HashSet;

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 50;
const FINGERPRINT_KEY: &str = "source_fingerprint";

#[derive(Deserialize, Serialize)]
struct SearchCursor {
    source_order: i64,
    rank: f64,
    sort_time: String,
    index_id: String,
}

pub async fn search_window(
    pool: &SqlitePool,
    query: &str,
    page_size: Option<i64>,
    include_resolved_comments: bool,
    cursor: Option<&str>,
) -> Result<NoteSearchWindowDto, String> {
    let query = query.trim();
    validate_search_query(query)?;
    let fts_query = fts_query(query)?;
    let page_size = normalized_page_size(page_size)?;
    let cursor = cursor
        .map(|value| {
            serde_json::from_str::<SearchCursor>(value)
                .map_err(|_| "invalid search cursor".to_string())
        })
        .transpose()?;
    ensure_index_current(pool).await?;

    let mut matches_query = QueryBuilder::<Sqlite>::new(
        "SELECT id, page_id, block_id, comment_id, property_id, block_type, title, body,
                metadata, source_last_edited_time, rank, source_order FROM (SELECT
            idx.id,
            idx.page_id,
            idx.block_id,
            idx.comment_id,
            idx.property_id,
            idx.block_type,
            idx.title,
            idx.body,
            idx.metadata,
            idx.source_last_edited_time,
            bm25(notes_search_fts, 3.0, 1.5, 0.7) AS rank,
            CASE
              WHEN idx.source_type = 'page' THEN 0
              WHEN idx.source_type IN ('property', 'alias') THEN 1
              WHEN idx.source_type = 'block' THEN 2
              WHEN idx.source_type = 'file' AND idx.block_id IS NOT NULL THEN 3
              WHEN idx.source_type = 'file' AND idx.comment_id IS NOT NULL THEN 5
              WHEN idx.source_type = 'file' THEN 1
              WHEN idx.source_type = 'comment' THEN 5
              WHEN idx.source_type = 'metadata' THEN 6
              ELSE 7
            END AS source_order
         FROM notes_search_fts
         JOIN notes_search_index AS idx ON idx.id = notes_search_fts.index_id
         JOIN notes_pages AS page ON page.id = idx.page_id
         LEFT JOIN notes_blocks AS block ON block.id = idx.block_id
         LEFT JOIN notes_comments AS comment ON comment.id = idx.comment_id
         LEFT JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id
         LEFT JOIN notes_blocks AS target_block ON target_block.id = thread.parent_block_id
         WHERE notes_search_fts MATCH ",
    );
    matches_query.push_bind(&fts_query);
    matches_query.push(
        "
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
           )
           AND (
               idx.block_id IS NULL
               OR (block.id IS NOT NULL AND block.in_trash = 0)
           )
           AND (
               idx.comment_id IS NULL
               OR (
                   comment.id IS NOT NULL
                   AND comment.deleted_at IS NULL
                   AND thread.id IS NOT NULL
                   AND (",
    );
    matches_query.push_bind(include_resolved_comments);
    matches_query.push(
        " OR thread.status = 'open')
                   AND (
                       thread.parent_block_id IS NULL
                       OR (target_block.id IS NOT NULL AND target_block.in_trash = 0)
                   )
               )
           )
         ) AS ranked WHERE 1 = 1",
    );
    if let Some(cursor) = &cursor {
        push_search_cursor(&mut matches_query, cursor);
    }
    matches_query
        .push(" ORDER BY source_order ASC, rank ASC, source_last_edited_time DESC, id ASC LIMIT ");
    matches_query.push_bind(page_size * 6 + 1);
    let matches = matches_query
        .build_query_as::<SearchIndexMatch>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("search notes FTS index: {e}"))?;

    let raw_window_full = matches.len() > (page_size * 6) as usize;
    let mut candidates = Vec::new();
    for row in matches {
        if let Some(candidate) = search_candidate(pool, row, query).await? {
            candidates.push(candidate);
        }
    }
    candidates.sort_by(|left, right| {
        left.source_order
            .cmp(&right.source_order)
            .then_with(|| {
                left.rank
                    .partial_cmp(&right.rank)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| right.sort_time.cmp(&left.sort_time))
            .then_with(|| left.index_id.cmp(&right.index_id))
    });

    let mut seen = HashSet::new();
    let mut results = Vec::new();
    let mut next_cursor = None;
    let candidate_count = candidates.len();
    let mut processed_count = 0;
    for candidate in candidates {
        processed_count += 1;
        next_cursor = Some(SearchCursor {
            source_order: candidate.source_order,
            rank: candidate.rank,
            sort_time: candidate.sort_time.clone(),
            index_id: candidate.index_id.clone(),
        });
        if seen.insert(candidate.id) {
            results.push(candidate.result);
        }
        if results.len() >= page_size as usize {
            break;
        }
    }
    let has_more = processed_count < candidate_count || raw_window_full;
    let next_cursor = has_more
        .then_some(next_cursor)
        .flatten()
        .map(|cursor| {
            serde_json::to_string(&cursor).map_err(|e| format!("serialize search cursor: {e}"))
        })
        .transpose()?;
    Ok(NoteSearchWindowDto::new(results, next_cursor))
}

#[cfg(test)]
pub async fn search(
    pool: &SqlitePool,
    query: &str,
    page_size: Option<i64>,
    include_resolved_comments: bool,
) -> Result<Vec<NoteSearchResultDto>, String> {
    search_window(pool, query, page_size, include_resolved_comments, None)
        .await
        .map(NoteSearchWindowDto::into_results)
}

fn push_search_cursor(query: &mut QueryBuilder<'_, Sqlite>, cursor: &SearchCursor) {
    query
        .push(" AND (source_order > ")
        .push_bind(cursor.source_order);
    query
        .push(" OR (source_order = ")
        .push_bind(cursor.source_order);
    query.push(" AND (rank > ").push_bind(cursor.rank);
    query.push(" OR (rank = ").push_bind(cursor.rank);
    query
        .push(" AND (source_last_edited_time < ")
        .push_bind(cursor.sort_time.clone());
    query
        .push(" OR (source_last_edited_time = ")
        .push_bind(cursor.sort_time.clone());
    query
        .push(" AND id > ")
        .push_bind(cursor.index_id.clone())
        .push("))))))");
}

pub async fn rebuild_index(pool: &SqlitePool) -> Result<i64, String> {
    let fingerprint = search_source_fingerprint(pool).await?;
    let entries = build_index_entries(pool).await?;
    let entry_count = entries.len() as i64;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes search index rebuild: {e}"))?;

    sqlx::query("DELETE FROM notes_search_fts")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("clear notes search FTS rows: {e}"))?;
    sqlx::query("DELETE FROM notes_search_index")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("clear notes search index rows: {e}"))?;

    for entry in &entries {
        sqlx::query(
            "INSERT INTO notes_search_index (
                id,
                source_type,
                page_id,
                block_id,
                comment_id,
                property_id,
                block_type,
                title,
                body,
                metadata,
                source_last_edited_time
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.id)
        .bind(&entry.source_type)
        .bind(&entry.page_id)
        .bind(&entry.block_id)
        .bind(&entry.comment_id)
        .bind(&entry.property_id)
        .bind(&entry.block_type)
        .bind(&entry.title)
        .bind(&entry.body)
        .bind(&entry.metadata)
        .bind(&entry.source_last_edited_time)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("insert notes search index row: {e}"))?;

        sqlx::query(
            "INSERT INTO notes_search_fts (index_id, title, body, metadata)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&entry.id)
        .bind(&entry.title)
        .bind(&entry.body)
        .bind(&entry.metadata)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("insert notes search FTS row: {e}"))?;
    }

    sqlx::query(
        "INSERT INTO notes_search_index_state (key, value, updated_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at",
    )
    .bind(FINGERPRINT_KEY)
    .bind(&fingerprint.value)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("record notes search index fingerprint: {e}"))?;

    tx.commit()
        .await
        .map_err(|e| format!("commit notes search index rebuild: {e}"))?;
    Ok(entry_count)
}

async fn ensure_index_current(pool: &SqlitePool) -> Result<(), String> {
    let fingerprint = search_source_fingerprint(pool).await?;
    let stored: Option<String> =
        sqlx::query_scalar("SELECT value FROM notes_search_index_state WHERE key = ?")
            .bind(FINGERPRINT_KEY)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("load notes search index fingerprint: {e}"))?;
    let index_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_search_index")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("count notes search index rows: {e}"))?;
    let fts_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_search_fts")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("count notes search FTS rows: {e}"))?;

    if stored.as_deref() != Some(fingerprint.value.as_str())
        || (fingerprint.source_rows > 0 && (index_rows == 0 || fts_rows == 0))
    {
        rebuild_index(pool).await?;
    }
    Ok(())
}

async fn build_index_entries(pool: &SqlitePool) -> Result<Vec<SearchIndexEntry>, String> {
    let mut entries = Vec::new();
    append_page_entries(pool, &mut entries).await?;
    append_alias_entries(pool, &mut entries).await?;
    append_property_value_entries(pool, &mut entries).await?;
    append_block_entries(pool, &mut entries).await?;
    append_comment_entries(pool, &mut entries).await?;
    append_relation_entries(pool, &mut entries).await?;
    append_file_entries(pool, &mut entries).await?;
    Ok(entries)
}

async fn append_property_value_entries(
    pool: &SqlitePool,
    entries: &mut Vec<SearchIndexEntry>,
) -> Result<(), String> {
    for row in properties::property_search_entries(pool).await? {
        entries.push(SearchIndexEntry {
            id: row.id,
            source_type: "property".to_string(),
            page_id: row.page_id,
            block_id: None,
            comment_id: None,
            property_id: Some(row.property_id),
            block_type: None,
            title: row.title,
            body: row.body,
            metadata: row.metadata,
            source_last_edited_time: row.source_last_edited_time,
        });
    }
    Ok(())
}

async fn append_page_entries(
    pool: &SqlitePool,
    entries: &mut Vec<SearchIndexEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, SearchPageIndexRow>(
        "SELECT id, title, properties, icon, cover, url, public_url, source_provider,
                source_object_id, source_workspace_id, source_last_edited_time, last_edited_time
         FROM notes_pages",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes pages for search index: {e}"))?;
    for row in rows {
        let property_text = indexable_json_text(&row.properties, "page properties")?;
        let metadata = page_metadata_text(&row)?;
        entries.push(SearchIndexEntry {
            id: format!("page:{}", row.id),
            source_type: "page".to_string(),
            page_id: row.id,
            block_id: None,
            comment_id: None,
            property_id: None,
            block_type: None,
            title: row.title,
            body: property_text,
            metadata,
            source_last_edited_time: row.last_edited_time,
        });
    }
    Ok(())
}

async fn append_alias_entries(
    pool: &SqlitePool,
    entries: &mut Vec<SearchIndexEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, NotePageAliasRow>(
        "SELECT alias.id, alias.page_id, alias.alias, alias.normalized_alias,
                alias.created_time, alias.last_edited_time
         FROM notes_page_aliases AS alias
         JOIN notes_pages AS page ON page.id = alias.page_id
         WHERE page.in_trash = 0
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
           )",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes page aliases for search index: {e}"))?;
    for row in rows {
        entries.push(SearchIndexEntry {
            id: format!("alias:{}", row.id),
            source_type: "alias".to_string(),
            page_id: row.page_id,
            block_id: None,
            comment_id: None,
            property_id: None,
            block_type: None,
            title: row.alias,
            body: String::new(),
            metadata: "page alias".to_string(),
            source_last_edited_time: row.last_edited_time,
        });
    }
    Ok(())
}

async fn append_block_entries(
    pool: &SqlitePool,
    entries: &mut Vec<SearchIndexEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, NoteBlockRow>(
        "SELECT
            id,
            page_id,
            parent_type,
            parent_page_id,
            parent_block_id,
            has_children,
            in_trash,
            type AS block_type,
            payload,
            plain_text,
            sort_order,
            source_provider,
            source_object_id,
            source_last_edited_time,
            created_time,
            last_edited_time
         FROM notes_blocks",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes blocks for search index: {e}"))?;
    for row in rows {
        let payload_text = indexable_json_text(&row.payload, "block payload")?;
        let metadata = join_text([
            Some(row.block_type.replace('_', " ")),
            row.source_provider.clone(),
            row.source_object_id.clone(),
            row.source_last_edited_time.clone(),
            Some(payload_text),
        ]);
        entries.push(SearchIndexEntry {
            id: format!("block:{}", row.id),
            source_type: "block".to_string(),
            page_id: row.page_id,
            block_id: Some(row.id),
            comment_id: None,
            property_id: None,
            block_type: Some(row.block_type),
            title: String::new(),
            body: row.plain_text,
            metadata,
            source_last_edited_time: row.last_edited_time,
        });
    }
    Ok(())
}

async fn append_comment_entries(
    pool: &SqlitePool,
    entries: &mut Vec<SearchIndexEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, CommentIndexRow>(
        "SELECT
            comment.id,
            comment.plain_text,
            comment.display_name,
            comment.attachments,
            thread.page_id,
            thread.parent_block_id,
            comment.last_edited_time
         FROM notes_comments AS comment
         JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes comments for search index: {e}"))?;
    for row in rows {
        let attachment_text = indexable_json_text(&row.attachments, "comment attachments")?;
        let display_name_text = indexable_json_text(&row.display_name, "comment display name")?;
        entries.push(SearchIndexEntry {
            id: format!("comment:{}", row.id),
            source_type: "comment".to_string(),
            page_id: row.page_id,
            block_id: row.parent_block_id,
            comment_id: Some(row.id),
            property_id: None,
            block_type: None,
            title: String::new(),
            body: row.plain_text,
            metadata: join_text([Some(display_name_text), Some(attachment_text)]),
            source_last_edited_time: row.last_edited_time,
        });
    }
    Ok(())
}

async fn append_relation_entries(
    pool: &SqlitePool,
    entries: &mut Vec<SearchIndexEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, RelationIndexRow>(
        "SELECT DISTINCT
            link.source_page_id AS page_id,
            link.source_property_id AS property_id,
            link.source_property_name AS property_name,
            link.target_page_id,
            target_page.title AS target_title,
            max(source_page.last_edited_time, target_page.last_edited_time, link.created_time)
                AS source_last_edited_time
         FROM notes_data_source_relation_links AS link
         JOIN notes_pages AS source_page ON source_page.id = link.source_page_id
         JOIN notes_pages AS target_page ON target_page.id = link.target_page_id
         JOIN notes_data_sources AS source_data_source
              ON source_data_source.id = link.source_data_source_id
         JOIN notes_databases AS source_database
              ON source_database.id = source_data_source.database_id
         JOIN notes_data_sources AS target_data_source
              ON target_data_source.id = link.target_data_source_id
         JOIN notes_databases AS target_database
              ON target_database.id = target_data_source.database_id
         WHERE source_page.in_trash = 0
           AND source_page.archived = 0
           AND target_page.in_trash = 0
           AND target_page.archived = 0
           AND source_data_source.in_trash = 0
           AND target_data_source.in_trash = 0
           AND source_database.in_trash = 0
           AND target_database.in_trash = 0",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes relation links for search index: {e}"))?;
    for row in rows {
        entries.push(SearchIndexEntry {
            id: format!(
                "property:{}:{}:{}",
                row.page_id, row.property_id, row.target_page_id
            ),
            source_type: "property".to_string(),
            page_id: row.page_id,
            block_id: None,
            comment_id: None,
            property_id: Some(row.property_id),
            block_type: None,
            title: row.property_name,
            body: row.target_title,
            metadata: "database relation".to_string(),
            source_last_edited_time: row.source_last_edited_time,
        });
    }
    Ok(())
}

async fn append_file_entries(
    pool: &SqlitePool,
    entries: &mut Vec<SearchIndexEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, FileIndexRow>(
        "SELECT
            asset.id AS asset_id,
            asset.asset_path,
            asset.kind,
            asset.source_type AS asset_source_type,
            asset.original_name,
            asset.content_type,
            asset.storage_state,
            reference.owner_type,
            reference.owner_id,
            reference.page_id,
            reference.block_id,
            reference.comment_id,
            reference.property_id,
            reference.role,
            block.type AS block_type,
            max(asset.updated_at, reference.updated_at) AS source_last_edited_time
         FROM notes_asset_references AS reference
         JOIN notes_assets AS asset ON asset.id = reference.asset_id
         LEFT JOIN notes_blocks AS block ON block.id = reference.block_id
         WHERE reference.page_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes file references for search index: {e}"))?;
    for row in rows {
        let body = join_text([
            row.original_name.clone(),
            Some(row.asset_path.clone()),
            Some(row.kind.clone()),
            Some(row.content_type.clone()),
        ]);
        let metadata = join_text([
            Some(row.role.clone()),
            Some(row.owner_type.clone()),
            Some(row.owner_id.clone()),
            Some(row.asset_source_type.clone()),
            Some(row.storage_state.clone()),
        ]);
        entries.push(SearchIndexEntry {
            id: format!(
                "file:{}:{}:{}:{}",
                row.asset_id, row.owner_type, row.owner_id, row.role
            ),
            source_type: "file".to_string(),
            page_id: row.page_id,
            block_id: row.block_id,
            comment_id: row.comment_id,
            property_id: row.property_id,
            block_type: row.block_type,
            title: row.original_name.unwrap_or_default(),
            body,
            metadata,
            source_last_edited_time: row.source_last_edited_time,
        });
    }
    Ok(())
}

async fn search_candidate(
    pool: &SqlitePool,
    row: SearchIndexMatch,
    query: &str,
) -> Result<Option<SearchCandidate>, String> {
    let snippet = search_snippet(&row.best_text(), query);
    if let Some(comment_id) = row.comment_id.as_deref() {
        let comment = load_comment_row(pool, comment_id).await?;
        let target = comment_thread_search_target(pool, &comment.thread_id).await?;
        let source_page = load_page_summary(pool, &target.page_id).await?;
        return Ok(Some(SearchCandidate {
            index_id: row.id.clone(),
            id: format!("comment:{comment_id}"),
            source_order: source_order(&row),
            rank: row.rank,
            sort_time: row.source_last_edited_time,
            result: NoteSearchResultDto::comment(
                source_page,
                comment,
                target.block_id,
                target.status,
                target.anchor,
                snippet,
            )?,
        }));
    }

    if let Some(block_id) = row.block_id.as_deref() {
        let block = load_block_row(pool, block_id).await?;
        let source_page = load_page_summary(pool, &block.page_id).await?;
        return Ok(Some(SearchCandidate {
            index_id: row.id.clone(),
            id: format!("block:{block_id}"),
            source_order: source_order(&row),
            rank: row.rank,
            sort_time: row.source_last_edited_time,
            result: NoteSearchResultDto::block(source_page, block, snippet),
        }));
    }

    let page = load_page_summary(pool, &row.page_id).await?;
    let sort_time = row.source_last_edited_time.clone();
    Ok(Some(SearchCandidate {
        index_id: row.id.clone(),
        id: format!("page:{}", row.page_id),
        source_order: source_order(&row),
        rank: row.rank,
        sort_time: sort_time.clone(),
        result: NoteSearchResultDto::page(page, snippet, sort_time),
    }))
}

async fn load_page_summary(pool: &SqlitePool, page_id: &str) -> Result<NotePageSummaryDto, String> {
    sqlx::query_as::<_, NotePageSummaryDto>(&format!(
        "SELECT {} FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
        crate::workspace_shell::page_projection()
    ))
    .bind(page_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load notes search result page summary: {e}"))
}

async fn load_block_row(pool: &SqlitePool, block_id: &str) -> Result<NoteBlockRow, String> {
    sqlx::query_as::<_, NoteBlockRow>(
        "SELECT
            id,
            page_id,
            parent_type,
            parent_page_id,
            parent_block_id,
            has_children,
            in_trash,
            type AS block_type,
            payload,
            plain_text,
            sort_order,
            source_provider,
            source_object_id,
            source_last_edited_time,
            created_time,
            last_edited_time
         FROM notes_blocks
         WHERE id = ?",
    )
    .bind(block_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load notes search result block: {e}"))
}

async fn load_comment_row(pool: &SqlitePool, comment_id: &str) -> Result<NoteCommentRow, String> {
    sqlx::query_as::<_, NoteCommentRow>(
        "SELECT id, thread_id, rich_text, plain_text, created_by, display_name, attachments,
                deleted_at, sync_version, created_time, last_edited_time
         FROM notes_comments WHERE id = ?",
    )
    .bind(comment_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load notes search result comment: {e}"))
}

async fn comment_thread_search_target(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<CommentThreadSearchTarget, String> {
    let row: CommentThreadSearchTargetRow = sqlx::query_as(
        "SELECT page_id, parent_block_id, status
         FROM notes_comment_threads
         WHERE id = ?",
    )
    .bind(thread_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load notes comment thread target: {e}"))?;
    let anchor = sqlx::query_as::<_, NoteCommentAnchorRow>(
        "SELECT thread_id, page_id, block_id, start_offset, end_offset, anchor_text,
                prefix_text, suffix_text, created_time, last_edited_time
         FROM notes_comment_thread_anchors
         WHERE thread_id = ?",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes comment search anchor: {e}"))?;
    Ok(CommentThreadSearchTarget {
        page_id: row.page_id,
        block_id: row.parent_block_id,
        status: row.status,
        anchor,
    })
}

async fn search_source_fingerprint(pool: &SqlitePool) -> Result<SearchSourceFingerprint, String> {
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
         FROM notes_databases
         UNION ALL
         SELECT 'relation_links', COUNT(*), COALESCE(MAX(created_time), '')
         FROM notes_data_source_relation_links
         UNION ALL
         SELECT 'rollup_cache', COUNT(*), COALESCE(MAX(computed_time), '')
         FROM notes_data_source_rollup_cache
         UNION ALL
         SELECT 'assets', COUNT(DISTINCT asset.id), COALESCE(MAX(asset.updated_at), '')
         FROM notes_assets AS asset
         JOIN notes_asset_references AS reference ON reference.asset_id = asset.id
         WHERE reference.page_id IS NOT NULL
         UNION ALL
         SELECT 'asset_references', COUNT(*), COALESCE(MAX(updated_at), '')
         FROM notes_asset_references
         WHERE page_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("compute notes search source fingerprint: {e}"))?;

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
    Ok(SearchSourceFingerprint { value, source_rows })
}

fn validate_search_query(query: &str) -> Result<(), String> {
    if query.is_empty() {
        return Err("search query must not be empty".to_string());
    }
    if query.chars().count() > 200 {
        return Err("search query must be 200 characters or fewer".to_string());
    }
    if query
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err("search query must not contain control characters".to_string());
    }
    Ok(())
}

fn normalized_page_size(page_size: Option<i64>) -> Result<i64, String> {
    let page_size = page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if page_size <= 0 {
        return Err("search page size must be greater than zero".to_string());
    }
    Ok(page_size.min(MAX_PAGE_SIZE))
}

fn fts_query(query: &str) -> Result<String, String> {
    let mut terms = Vec::new();
    for segment in query.split_whitespace() {
        for term in segment.split(|character: char| !character.is_alphanumeric()) {
            let term = term.trim().to_lowercase();
            if !term.is_empty() {
                terms.push(term);
            }
        }
    }
    terms.sort();
    terms.dedup();
    if terms.is_empty() {
        return Err("search query must contain searchable text".to_string());
    }
    Ok(terms
        .into_iter()
        .take(12)
        .map(|term| format!("{term}*"))
        .collect::<Vec<_>>()
        .join(" AND "))
}

fn page_metadata_text(row: &SearchPageIndexRow) -> Result<String, String> {
    let icon_text = row
        .icon
        .as_deref()
        .map(|icon| indexable_json_text(icon, "page icon"))
        .transpose()?;
    let cover_text = row
        .cover
        .as_deref()
        .map(|cover| indexable_json_text(cover, "page cover"))
        .transpose()?;
    Ok(join_text([
        row.url.clone(),
        row.public_url.clone(),
        row.source_provider.clone(),
        row.source_object_id.clone(),
        row.source_workspace_id.clone(),
        row.source_last_edited_time.clone(),
        icon_text,
        cover_text,
    ]))
}

fn indexable_json_text(raw: &str, label: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(raw).map_err(|e| format!("parse {label}: {e}"))?;
    let mut parts = Vec::new();
    collect_indexable_json_text(&value, None, &mut parts);
    Ok(parts.join(" "))
}

fn collect_indexable_json_text(value: &Value, key: Option<&str>, parts: &mut Vec<String>) {
    if key.is_some_and(should_skip_json_key) {
        return;
    }
    match value {
        Value::Null => {}
        Value::Bool(value) => parts.push(value.to_string()),
        Value::Number(value) => parts.push(value.to_string()),
        Value::String(value) => push_indexable_text(parts, value),
        Value::Array(items) => {
            for item in items {
                collect_indexable_json_text(item, None, parts);
            }
        }
        Value::Object(object) => {
            for (child_key, child_value) in object {
                collect_indexable_json_text(child_value, Some(child_key), parts);
            }
        }
    }
}

fn push_indexable_text(parts: &mut Vec<String>, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() || looks_like_internal_identifier(trimmed) {
        return;
    }
    let clipped = trimmed.chars().take(500).collect::<String>();
    if !parts.iter().any(|part| part == &clipped) {
        parts.push(clipped);
    }
}

fn should_skip_json_key(key: &str) -> bool {
    matches!(
        key,
        "id" | "object"
            | "type"
            | "color"
            | "source_type"
            | "source_object_id"
            | "source_workspace_id"
            | "sha256"
            | "byte_size"
            | "content_type"
            | "ganbaru_asset_path"
            | "sync_version"
    )
}

fn looks_like_internal_identifier(value: &str) -> bool {
    let hexish = value
        .chars()
        .all(|character| character.is_ascii_hexdigit() || character == '-');
    hexish && value.chars().filter(|character| *character != '-').count() >= 24
}

fn join_text(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn source_order(row: &SearchIndexMatch) -> i64 {
    row.source_order
}

fn search_snippet(text: &str, query: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= 160 {
        return trimmed.to_string();
    }
    let normalized_text = trimmed.to_lowercase();
    let normalized_query = query.trim().to_lowercase();
    let match_index = normalized_text.find(&normalized_query).unwrap_or(0);
    let prefix = trimmed
        .char_indices()
        .take_while(|(index, _)| *index < match_index)
        .count();
    let start = prefix.saturating_sub(50);
    let snippet = trimmed.chars().skip(start).take(160).collect::<String>();
    if start == 0 {
        snippet
    } else {
        format!("...{snippet}")
    }
}

#[derive(Debug)]
struct SearchIndexEntry {
    id: String,
    source_type: String,
    page_id: String,
    block_id: Option<String>,
    comment_id: Option<String>,
    property_id: Option<String>,
    block_type: Option<String>,
    title: String,
    body: String,
    metadata: String,
    source_last_edited_time: String,
}

#[derive(FromRow)]
struct SearchPageIndexRow {
    id: String,
    title: String,
    properties: String,
    icon: Option<String>,
    cover: Option<String>,
    url: Option<String>,
    public_url: Option<String>,
    source_provider: Option<String>,
    source_object_id: Option<String>,
    source_workspace_id: Option<String>,
    source_last_edited_time: Option<String>,
    last_edited_time: String,
}

#[derive(FromRow)]
struct SearchIndexMatch {
    id: String,
    page_id: String,
    block_id: Option<String>,
    comment_id: Option<String>,
    property_id: Option<String>,
    block_type: Option<String>,
    title: String,
    body: String,
    metadata: String,
    source_last_edited_time: String,
    rank: f64,
    source_order: i64,
}

impl SearchIndexMatch {
    fn best_text(&self) -> String {
        join_text([
            Some(self.title.clone()),
            Some(self.body.clone()),
            Some(self.metadata.clone()),
            self.block_type.clone(),
            self.property_id.clone(),
            Some(self.id.clone()),
        ])
    }
}

struct SearchCandidate {
    index_id: String,
    id: String,
    source_order: i64,
    rank: f64,
    sort_time: String,
    result: NoteSearchResultDto,
}

struct SearchSourceFingerprint {
    value: String,
    source_rows: i64,
}

struct CommentThreadSearchTarget {
    page_id: String,
    block_id: Option<String>,
    status: String,
    anchor: Option<NoteCommentAnchorRow>,
}

#[derive(FromRow)]
struct CommentThreadSearchTargetRow {
    page_id: String,
    parent_block_id: Option<String>,
    status: String,
}

#[derive(FromRow)]
struct CommentIndexRow {
    id: String,
    plain_text: String,
    display_name: String,
    attachments: String,
    page_id: String,
    parent_block_id: Option<String>,
    last_edited_time: String,
}

#[derive(FromRow)]
struct RelationIndexRow {
    page_id: String,
    property_id: String,
    property_name: String,
    target_page_id: String,
    target_title: String,
    source_last_edited_time: String,
}

#[derive(FromRow)]
struct FileIndexRow {
    asset_id: String,
    asset_path: String,
    kind: String,
    asset_source_type: String,
    original_name: Option<String>,
    content_type: String,
    storage_state: String,
    owner_type: String,
    owner_id: String,
    page_id: String,
    block_id: Option<String>,
    comment_id: Option<String>,
    property_id: Option<String>,
    role: String,
    block_type: Option<String>,
    source_last_edited_time: String,
}
