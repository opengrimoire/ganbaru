//! Restores a page's blocks from a history snapshot as a diff against its current blocks.
//!
//! A snapshot block whose row still exists on the page, trashed or not, is updated in place, so
//! its id, device view state, and anything that hangs off it (a nested page and its content,
//! comments, references) survive. Current blocks the snapshot does not list are trashed rather
//! than deleted. Only a block whose row is gone, or that now lives elsewhere, comes back under a
//! fresh id, since its old id may already be known as deleted.

use super::{new_note_id, upsert_snapshot_child_page};
use crate::block_storage::{BlockRowInsert, insert_block_row, split_block_payload};
use crate::models::NoteBlockRow;
use crate::validation::validate_order_key;
use serde_json::Value;
use sqlx::{Sqlite, Transaction};
use std::collections::{HashMap, HashSet};

/// The stored columns of one current block that a restore compares and rewrites.
#[derive(sqlx::FromRow)]
struct CurrentBlock {
    id: String,
    parent_type: String,
    parent_page_id: Option<String>,
    parent_block_id: Option<String>,
    block_type: String,
    attrs: String,
    text: String,
    plain_text: String,
    order_key: String,
    in_trash: i64,
    trash_owner: Option<String>,
    source_provider: Option<String>,
    source_object_id: Option<String>,
    source_last_edited_time: Option<String>,
    /// Whether a page is nested under this block through `child_block_id`.
    nested_page: i64,
}

/// The placement a restored block takes on the restored page.
struct Placement {
    parent_type: &'static str,
    parent_page_id: Option<String>,
    parent_block_id: Option<String>,
}

/// Rewrites the blocks of `page_id` to match `blocks`, the snapshot rows in depth-first order with
/// parents before children.
pub(super) async fn restore_blocks_from_snapshot(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    blocks: &[NoteBlockRow],
) -> Result<(), String> {
    let current = load_current_blocks(tx, page_id).await?;
    let mut reserved_ids = HashSet::new();
    let mut id_map = HashMap::with_capacity(blocks.len());
    let mut kept_ids = HashSet::new();
    for block in blocks {
        let target_id = if current
            .get(&block.id)
            .is_some_and(|existing| restores_in_place(existing, block))
        {
            kept_ids.insert(block.id.clone());
            block.id.clone()
        } else {
            new_note_id(tx, &mut reserved_ids).await?
        };
        reserved_ids.insert(target_id.clone());
        if id_map.insert(block.id.clone(), target_id).is_some() {
            return Err("notes page history snapshot repeats a block id".to_string());
        }
    }

    for block in blocks {
        let block_id = id_map
            .get(&block.id)
            .ok_or_else(|| "snapshot block id mapping is missing".to_string())?;
        let placement = snapshot_placement(page_id, block, &id_map)?;
        validate_order_key(&block.order_key)?;
        let payload: Value = serde_json::from_str(&block.payload)
            .map_err(|e| format!("parse notes page history block payload: {e}"))?;
        match current
            .get(block_id)
            .filter(|_| kept_ids.contains(block_id))
        {
            Some(existing) => {
                update_block_in_place(tx, existing, block, &placement, &payload).await?;
            }
            None => {
                insert_restored_block(tx, block_id, page_id, block, &placement, &payload).await?
            }
        }
        if block.block_type == "child_page" {
            upsert_snapshot_child_page(
                tx,
                block_id,
                page_id,
                placement.parent_type,
                placement.parent_block_id.clone(),
                block,
                &mut reserved_ids,
            )
            .await?;
        }
    }

    for existing in current.values() {
        if existing.in_trash == 0 && !kept_ids.contains(&existing.id) {
            trash_missing_block(tx, existing).await?;
        }
    }
    Ok(())
}

/// Whether a snapshot block can take back the row of `existing`, which lives on the restored
/// page. A `child_page` row qualifies only while its page is still nested under it.
fn restores_in_place(existing: &CurrentBlock, block: &NoteBlockRow) -> bool {
    let snapshot_child_page = block.block_type == "child_page";
    let current_child_page = existing.block_type == "child_page";
    snapshot_child_page == current_child_page && (!snapshot_child_page || existing.nested_page != 0)
}

fn snapshot_placement(
    page_id: &str,
    block: &NoteBlockRow,
    id_map: &HashMap<String, String>,
) -> Result<Placement, String> {
    if block.parent_type == "page_id" {
        return Ok(Placement {
            parent_type: "page_id",
            parent_page_id: Some(page_id.to_string()),
            parent_block_id: None,
        });
    }
    let source_parent_id = block
        .parent_block_id
        .as_ref()
        .ok_or_else(|| "snapshot child block is missing its parent".to_string())?;
    let parent_block_id = id_map
        .get(source_parent_id)
        .cloned()
        .ok_or_else(|| "snapshot child block parent was not restored".to_string())?;
    Ok(Placement {
        parent_type: "block_id",
        parent_page_id: None,
        parent_block_id: Some(parent_block_id),
    })
}

async fn load_current_blocks(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<HashMap<String, CurrentBlock>, String> {
    let rows = sqlx::query_as::<_, CurrentBlock>(
        "SELECT block.id, block.parent_type, block.parent_page_id, block.parent_block_id,
                block.type AS block_type, block.attrs, block.text, block.plain_text,
                block.order_key, block.in_trash, block.trash_owner, block.source_provider,
                block.source_object_id, block.source_last_edited_time,
                EXISTS (
                    SELECT 1 FROM notes_pages AS page WHERE page.child_block_id = block.id
                ) AS nested_page
         FROM notes_blocks AS block
         WHERE block.page_id = ?",
    )
    .bind(page_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load current notes blocks before history restore: {e}"))?;
    Ok(rows.into_iter().map(|row| (row.id.clone(), row)).collect())
}

/// Updates a kept block when any restored column differs, leaving its device view state alone.
async fn update_block_in_place(
    tx: &mut Transaction<'_, Sqlite>,
    existing: &CurrentBlock,
    block: &NoteBlockRow,
    placement: &Placement,
    payload: &Value,
) -> Result<(), String> {
    let content = split_block_payload(&block.block_type, payload)?;
    let unchanged = existing.in_trash == 0
        && existing.trash_owner.is_none()
        && existing.parent_type == placement.parent_type
        && existing.parent_page_id == placement.parent_page_id
        && existing.parent_block_id == placement.parent_block_id
        && existing.block_type == block.block_type
        && same_json(&existing.attrs, &content.attrs)?
        && same_json(&existing.text, &content.text)?
        && existing.plain_text == block.plain_text
        && existing.order_key == block.order_key
        && existing.source_provider == block.source_provider
        && existing.source_object_id == block.source_object_id
        && existing.source_last_edited_time == block.source_last_edited_time;
    if unchanged {
        return Ok(());
    }
    sqlx::query(
        "UPDATE notes_blocks
         SET parent_type = ?, parent_page_id = ?, parent_block_id = ?, type = ?, attrs = ?,
             text = ?, plain_text = ?, order_key = ?, source_provider = ?, source_object_id = ?,
             source_last_edited_time = ?, in_trash = 0, trash_owner = NULL, last_edited_time = ?
         WHERE id = ?",
    )
    .bind(placement.parent_type)
    .bind(&placement.parent_page_id)
    .bind(&placement.parent_block_id)
    .bind(&block.block_type)
    .bind(&content.attrs)
    .bind(&content.text)
    .bind(&block.plain_text)
    .bind(&block.order_key)
    .bind(&block.source_provider)
    .bind(&block.source_object_id)
    .bind(&block.source_last_edited_time)
    .bind(&block.last_edited_time)
    .bind(&existing.id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("restore notes page history block in place: {e}"))?;
    Ok(())
}

async fn insert_restored_block(
    tx: &mut Transaction<'_, Sqlite>,
    block_id: &str,
    page_id: &str,
    block: &NoteBlockRow,
    placement: &Placement,
    payload: &Value,
) -> Result<(), String> {
    insert_block_row(
        tx,
        BlockRowInsert {
            id: block_id,
            page_id,
            parent_type: placement.parent_type,
            parent_page_id: placement.parent_page_id.as_deref(),
            parent_block_id: placement.parent_block_id.as_deref(),
            block_type: &block.block_type,
            payload,
            plain_text: &block.plain_text,
            order_key: &block.order_key,
            source_provider: block.source_provider.as_deref(),
            source_object_id: block.source_object_id.as_deref(),
            source_last_edited_time: block.source_last_edited_time.as_deref(),
        },
    )
    .await?;
    sqlx::query("UPDATE notes_blocks SET created_time = ?, last_edited_time = ? WHERE id = ?")
        .bind(&block.created_time)
        .bind(&block.last_edited_time)
        .bind(block_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("restore notes page history block times: {e}"))?;
    Ok(())
}

/// Trashes one current block the snapshot does not list, and the page nested under it.
async fn trash_missing_block(
    tx: &mut Transaction<'_, Sqlite>,
    existing: &CurrentBlock,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_blocks
         SET in_trash = 1,
             trash_owner = NULL,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(&existing.id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("trash notes block missing from history snapshot: {e}"))?;
    if existing.nested_page == 0 {
        return Ok(());
    }
    sqlx::query(
        "UPDATE notes_pages
         SET in_trash = 1,
             trash_owner = NULL,
             archived = 0,
             trashed_time = COALESCE(trashed_time, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE child_block_id = ? AND in_trash = 0",
    )
    .bind(&existing.id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("trash notes child page missing from history snapshot: {e}"))?;
    Ok(())
}

fn same_json(stored: &str, restored: &str) -> Result<bool, String> {
    let stored: Value = serde_json::from_str(stored)
        .map_err(|e| format!("parse stored notes block content: {e}"))?;
    let restored: Value = serde_json::from_str(restored)
        .map_err(|e| format!("parse restored notes block content: {e}"))?;
    Ok(stored == restored)
}
