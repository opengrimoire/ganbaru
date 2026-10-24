use super::copy_budget::{CopyBudget, MAX_COPY_DEPTH, MAX_COPY_OBJECTS};
use crate::models::{NoteBlockDto, NoteBlockRow, NotePaginatedBlockList};
use crate::reads;
use crate::validation::{require_uuid, validate_children_count};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

pub(super) async fn normalize_selection_root_ids(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_ids: &[String],
    include_trashed: bool,
) -> Result<Vec<String>, String> {
    validate_children_count(block_ids.len())?;
    let mut seen = HashSet::with_capacity(block_ids.len());
    let mut normalized = Vec::with_capacity(block_ids.len());
    for raw_id in block_ids {
        let block_id = raw_id.trim().to_string();
        require_uuid(&block_id, "block_id")?;
        if !seen.insert(block_id.clone()) {
            return Err("block_ids must be unique".to_string());
        }
        normalized.push(block_id);
    }
    let selected = normalized.iter().cloned().collect::<HashSet<_>>();
    let mut roots = Vec::with_capacity(normalized.len());
    for block_id in normalized {
        let row = load_block_row_tx(tx, &block_id, include_trashed).await?;
        if selected_ancestor_exists(tx, row.parent_block_id.as_deref(), &selected).await? {
            continue;
        }
        roots.push(block_id);
    }
    if roots.is_empty() {
        return Err("block_ids must include at least one root block".to_string());
    }
    Ok(roots)
}

pub(super) async fn selected_ancestor_exists(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent_block_id: Option<&str>,
    selected: &HashSet<String>,
) -> Result<bool, String> {
    let mut current = parent_block_id.map(str::to_string);
    let mut visited = HashSet::new();
    while let Some(block_id) = current {
        if visited.len() >= MAX_COPY_DEPTH as usize || !visited.insert(block_id.clone()) {
            return Err("Notes selection has a cycle or exceeds the nesting limit".to_string());
        }
        if selected.contains(&block_id) {
            return Ok(true);
        }
        current = sqlx::query_scalar::<_, Option<String>>(
            "SELECT parent_block_id
             FROM notes_blocks
             WHERE id = ?",
        )
        .bind(&block_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load selected notes block ancestor: {e}"))?
        .flatten();
    }
    Ok(false)
}

pub(super) async fn load_block_row_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
    include_trashed: bool,
) -> Result<NoteBlockRow, String> {
    sqlx::query_as::<_, NoteBlockRow>(concat!(
        "SELECT ",
        crate::note_block_row_columns!("notes_blocks"),
        "
         FROM notes_blocks
         WHERE id = ? AND (? = 1 OR in_trash = 0)"
    ))
    .bind(block_id)
    .bind(if include_trashed { 1_i64 } else { 0_i64 })
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes block row: {e}"))?
    .ok_or_else(|| "notes block not found".to_string())
}

pub(super) async fn set_block_subtree_trash(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
    in_trash: bool,
) -> Result<(), String> {
    super::database_lifecycle::set_block_trash(tx, block_id, in_trash).await
}

pub(super) async fn load_block_subtree_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
    budget: &mut CopyBudget,
) -> Result<Vec<NoteBlockRow>, String> {
    load_block_subtree_rows_including_trashed(tx, block_id, false, budget).await
}

pub(super) async fn load_block_subtree_rows_including_trashed(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
    include_trashed: bool,
    budget: &mut CopyBudget,
) -> Result<Vec<NoteBlockRow>, String> {
    let descriptors = sqlx::query_as::<_, CopyBlockDescriptor>(
        "WITH RECURSIVE subtree(id, path, depth) AS (
            SELECT id, order_key || ' ' || id, 0
            FROM notes_blocks
            WHERE id = ? AND (? = 1 OR in_trash = 0)
            UNION ALL
            SELECT child.id, subtree.path || '/' || child.order_key || ' ' || child.id, subtree.depth + 1
            FROM notes_blocks AS child
            JOIN subtree ON child.parent_block_id = subtree.id
            WHERE (? = 1 OR child.in_trash = 0) AND subtree.depth <= ?
            LIMIT ?
         )
         SELECT notes_blocks.id, subtree.depth,
            length(CAST(attrs AS BLOB)) + length(CAST(text AS BLOB)) + length(CAST(plain_text AS BLOB))
                + coalesce(length(CAST(source_provider AS BLOB)), 0)
                + coalesce(length(CAST(source_object_id AS BLOB)), 0)
                + coalesce(length(CAST(source_last_edited_time AS BLOB)), 0) AS bytes
         FROM notes_blocks
         JOIN subtree ON subtree.id = notes_blocks.id
         ORDER BY subtree.path ASC",
    )
    .bind(block_id)
    .bind(if include_trashed { 1_i64 } else { 0_i64 })
    .bind(if include_trashed { 1_i64 } else { 0_i64 })
    .bind(MAX_COPY_DEPTH)
    .bind((MAX_COPY_OBJECTS + 1) as i64)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("measure notes block subtree: {e}"))?;
    load_admitted_copy_blocks(tx, descriptors, budget).await
}

pub(super) async fn load_page_block_subtree_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
    budget: &mut CopyBudget,
) -> Result<Vec<NoteBlockRow>, String> {
    load_page_block_subtree_rows_for_copy(tx, page_id, false, budget).await
}

/// Include paired notes trashed with their containing page when pasting a cut note.
pub(super) async fn load_page_block_subtree_rows_for_copy(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
    include_trashed: bool,
    budget: &mut CopyBudget,
) -> Result<Vec<NoteBlockRow>, String> {
    let descriptors = sqlx::query_as::<_, CopyBlockDescriptor>(
        "WITH RECURSIVE eligible AS (
            SELECT block.id, block.parent_type, block.parent_page_id, block.parent_block_id, block.order_key
            FROM notes_blocks AS block
            WHERE block.page_id = ? AND (block.in_trash = 0 OR (
                ? AND EXISTS (
                    SELECT 1 FROM notes_pages AS child
                    JOIN notes_pages AS parent ON parent.id = block.page_id
                    WHERE child.id = block.id AND parent.in_trash = 1
                      AND child.trashed_time = parent.trashed_time
                      AND block.trash_owner IS NULL
                      AND parent.trash_owner IS NULL
                )
            ) OR (
                ? AND EXISTS (SELECT 1 FROM notes_pages AS parent WHERE parent.id = block.page_id
                    AND parent.in_trash = 1
                    AND block.trash_owner = parent.trash_owner)
            ))
         ), subtree(id, path, depth) AS (
            SELECT id, order_key || ' ' || id, 0
            FROM eligible
            WHERE parent_type = 'page_id' AND parent_page_id = ?
            UNION ALL
            SELECT child.id, subtree.path || '/' || child.order_key || ' ' || child.id, subtree.depth + 1
            FROM eligible AS child
            JOIN subtree ON child.parent_block_id = subtree.id
            WHERE subtree.depth <= ? LIMIT ?
         )
         SELECT notes_blocks.id, subtree.depth,
            length(CAST(attrs AS BLOB)) + length(CAST(text AS BLOB)) + length(CAST(plain_text AS BLOB))
                + coalesce(length(CAST(source_provider AS BLOB)), 0)
                + coalesce(length(CAST(source_object_id AS BLOB)), 0)
                + coalesce(length(CAST(source_last_edited_time AS BLOB)), 0) AS bytes
         FROM notes_blocks
         JOIN subtree ON subtree.id = notes_blocks.id
         ORDER BY subtree.path ASC",
    )
    .bind(page_id)
    .bind(include_trashed)
    .bind(include_trashed)
    .bind(page_id)
    .bind(MAX_COPY_DEPTH)
    .bind((MAX_COPY_OBJECTS + 1) as i64)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("measure notes page block subtree: {e}"))?;
    load_admitted_copy_blocks(tx, descriptors, budget).await
}

/// Counts the levels of a loaded subtree whose rows list every parent before its children.
pub(super) fn copied_subtree_height(rows: &[NoteBlockRow]) -> i64 {
    let mut depths = HashMap::with_capacity(rows.len());
    let mut height = 0;
    for row in rows {
        let depth = row
            .parent_block_id
            .as_deref()
            .and_then(|parent_id| depths.get(parent_id))
            .map_or(1, |parent_depth| parent_depth + 1);
        depths.insert(row.id.as_str(), depth);
        height = height.max(depth);
    }
    height
}

#[derive(sqlx::FromRow)]
struct CopyBlockDescriptor {
    id: String,
    depth: i64,
    bytes: i64,
}

/// Load payloads only after admitting the complete bounded traversal.
async fn load_admitted_copy_blocks(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    descriptors: Vec<CopyBlockDescriptor>,
    budget: &mut CopyBudget,
) -> Result<Vec<NoteBlockRow>, String> {
    if descriptors.iter().any(|row| row.depth > MAX_COPY_DEPTH) {
        return Err("Notes copy has a cycle or exceeds the nesting limit".to_string());
    }
    let bytes = descriptors.iter().try_fold(0_i64, |total, row| {
        total
            .checked_add(row.bytes)
            .ok_or("Notes copy byte count overflow")
    })?;
    budget.charge(descriptors.len() as i64, bytes)?;
    let ids = descriptors
        .into_iter()
        .map(|row| row.id)
        .collect::<Vec<_>>();
    let encoded_ids = serde_json::to_string(&ids)
        .map_err(|error| format!("encode copied block identities: {error}"))?;
    budget.block_comments(tx, &encoded_ids).await?;
    sqlx::query_as::<_, NoteBlockRow>(concat!(
        "SELECT ",
        crate::note_block_row_columns!("block"),
        "
         FROM json_each(?) AS selected JOIN notes_blocks AS block ON block.id = selected.value
         ORDER BY CAST(selected.key AS INTEGER)"
    ))
    .bind(encoded_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| format!("load admitted copy blocks: {error}"))
}

pub(super) async fn load_child_page_block_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
) -> Result<NoteBlockRow, String> {
    sqlx::query_as::<_, NoteBlockRow>(concat!(
        "SELECT ",
        crate::note_block_row_columns!("notes_blocks"),
        "
         FROM notes_blocks
         WHERE id = ? AND type = 'child_page' AND in_trash = 0"
    ))
    .bind(block_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes child page block: {e}"))?
    .ok_or_else(|| "child page block not found".to_string())
}

pub(super) async fn load_child_page_block_row_including_trashed(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
) -> Result<Option<NoteBlockRow>, String> {
    sqlx::query_as::<_, NoteBlockRow>(concat!(
        "SELECT ",
        crate::note_block_row_columns!("notes_blocks"),
        "
         FROM notes_blocks
         WHERE id = ? AND type = 'child_page'"
    ))
    .bind(block_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes child page block: {e}"))
}

pub(super) async fn load_blocks_by_ids(
    pool: &SqlitePool,
    ids: Vec<String>,
) -> Result<NotePaginatedBlockList, String> {
    load_blocks_by_ids_including_trashed(pool, ids, false).await
}

pub(super) async fn load_blocks_by_ids_including_trashed(
    pool: &SqlitePool,
    ids: Vec<String>,
    include_trashed: bool,
) -> Result<NotePaginatedBlockList, String> {
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        rows.push(reads::get_block_row(pool, &id, include_trashed).await?);
    }
    let blocks = rows
        .into_iter()
        .map(NoteBlockDto::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NotePaginatedBlockList::new(blocks, None, false))
}
