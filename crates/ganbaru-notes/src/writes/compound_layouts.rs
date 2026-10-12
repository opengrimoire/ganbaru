//! Canonical child traversal for edits that cannot trust a partially hydrated editor.

use super::{block_commands, block_moves, block_tree, ids};
use crate::models::{
    NoteAppendBlockChildren, NoteBlockUpdate, NoteDuplicateBlocks, NoteDuplicatedBlockId,
    NoteMoveBlock, NoteParent,
};
use crate::validation;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};
use std::collections::HashSet;

const MAX_LAYOUT_CHILDREN: i64 = 20_000;
const MAX_TABLE_WIDTH: usize = 100;
const MAX_LAYOUT_PAYLOAD_BYTES: i64 = 16 * 1024 * 1024;

async fn child_ids(tx: &mut Transaction<'_, Sqlite>, id: &str) -> Result<Vec<String>, String> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM notes_blocks WHERE parent_block_id = ? AND in_trash = 0 ORDER BY sort_order, id LIMIT ?")
        .bind(id).bind(MAX_LAYOUT_CHILDREN + 1).fetch_all(&mut **tx).await
        .map_err(|error| format!("load Notes layout children: {error}"))?;
    if ids.len() > MAX_LAYOUT_CHILDREN as usize {
        return Err("Notes layout exceeds its child limit".to_string());
    }
    Ok(ids)
}

/// Resolve all template content before copying, retaining caller identities for loaded rows.
pub(super) async fn template_children(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    mut request: NoteDuplicateBlocks,
) -> Result<NoteDuplicateBlocks, String> {
    validation::require_uuid(id, "source_block_id")?;
    let block_type: Option<String> =
        sqlx::query_scalar("SELECT type FROM notes_blocks WHERE id = ? AND in_trash = 0")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|error| format!("read Notes template source: {error}"))?;
    if !matches!(block_type.as_deref(), Some("template" | "button")) {
        return Err("Notes template insertion requires an active template or button".to_string());
    }
    request.block_ids = child_ids(tx, id).await?;
    let source: Vec<(String, String)> = sqlx::query_as("WITH RECURSIVE content(id) AS (SELECT id FROM notes_blocks WHERE parent_block_id = ? AND in_trash = 0 UNION SELECT b.id FROM notes_blocks b JOIN content c ON b.parent_block_id = c.id WHERE b.in_trash = 0 LIMIT ?) SELECT b.id, b.type FROM notes_blocks b JOIN content c ON b.id = c.id ORDER BY b.id")
        .bind(id).bind(MAX_LAYOUT_CHILDREN + 1).fetch_all(&mut **tx).await
        .map_err(|error| format!("bound Notes template content: {error}"))?;
    validation::validate_duplicate_block_count(source.len())?;
    if source
        .iter()
        .any(|(_, block_type)| block_type == "child_page")
    {
        return Err("Notes template and button content cannot contain child pages".to_string());
    }
    let provided: HashSet<String> = request
        .duplicated_block_ids
        .iter()
        .map(|pair| pair.source_id.clone())
        .collect();
    let mut reserved: HashSet<String> = request
        .duplicated_block_ids
        .iter()
        .map(|pair| pair.duplicate_id.clone())
        .collect();
    for (source_id, _) in source {
        if !provided.contains(&source_id) {
            request.duplicated_block_ids.push(NoteDuplicatedBlockId {
                source_id,
                duplicate_id: ids::new_note_id(tx, &mut reserved).await?,
            });
        }
    }
    request.include_trashed_sources = Some(false);
    Ok(request)
}

/// Move all canonical children, including unloaded content, before removing a layout item.
pub(super) async fn move_children(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    parent: NoteParent,
    mut after: Option<String>,
) -> Result<Vec<String>, String> {
    let ids = child_ids(tx, id).await?;
    for child_id in &ids {
        block_moves::move_block_tx(
            tx,
            child_id,
            NoteMoveBlock {
                parent: parent.clone(),
                after,
                before: None,
            },
            false,
        )
        .await?;
        after = Some(child_id.clone());
    }
    Ok(ids)
}

/// Change every canonical row with its table width in the enclosing editor transaction.
pub(super) async fn table_column(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    index: usize,
    insert: bool,
    empty_row_id: &str,
) -> Result<Vec<String>, String> {
    let mut ids = child_ids(tx, id).await?;
    let payload_bytes: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM notes_blocks WHERE (parent_block_id = ? OR id = ?) AND in_trash = 0")
        .bind(id).bind(id).fetch_one(&mut **tx).await.map_err(|error| format!("bound Notes table payload: {error}"))?;
    if payload_bytes > MAX_LAYOUT_PAYLOAD_BYTES {
        return Err("Notes table exceeds its payload limit".to_string());
    }
    let table = block_tree::load_block_row_tx(tx, id, false).await?;
    if table.block_type != "table" {
        return Err("Notes column edit requires a table".to_string());
    }
    let mut payload: Value = serde_json::from_str(&table.payload)
        .map_err(|error| format!("decode Notes table: {error}"))?;
    let mut width = payload["table_width"]
        .as_u64()
        .ok_or("Notes table has no valid width")? as usize;
    let mut rows = Vec::with_capacity(ids.len());
    for row_id in &ids {
        let row = block_tree::load_block_row_tx(tx, row_id, false).await?;
        if row.block_type != "table_row" {
            return Err("Notes table contains an invalid row".to_string());
        }
        let row_payload: Value = serde_json::from_str(&row.payload)
            .map_err(|error| format!("decode Notes table row: {error}"))?;
        let cells = row_payload["cells"]
            .as_array()
            .ok_or("Notes table row has no cells")?;
        width = width.max(cells.len());
        rows.push((row_id.clone(), row_payload));
    }
    if width == 0
        || width > MAX_TABLE_WIDTH
        || (insert && width == MAX_TABLE_WIDTH)
        || (!insert && width == 1)
        || index > width
        || (!insert && index == width)
    {
        return Err("Notes column edit is outside the table bounds".to_string());
    }
    let next_width = if insert { width + 1 } else { width - 1 };
    payload["table_width"] = json!(next_width);
    let update: NoteBlockUpdate =
        serde_json::from_value(json!({ "type": "table", "table": payload }))
            .map_err(|error| format!("plan Notes table edit: {error}"))?;
    block_commands::update_block_tx(tx, id, update, false).await?;
    for (row_id, mut row_payload) in rows {
        let cells = row_payload["cells"]
            .as_array_mut()
            .ok_or("Notes table row has no cells")?;
        cells.resize(width, json!([]));
        if insert {
            cells.insert(index, json!([]));
        } else {
            cells.remove(index);
        }
        let update =
            serde_json::from_value(json!({ "type": "table_row", "table_row": row_payload }))
                .map_err(|error| format!("plan Notes row edit: {error}"))?;
        block_commands::update_block_tx(tx, &row_id, update, false).await?;
    }
    if insert && ids.is_empty() {
        let append: NoteAppendBlockChildren = serde_json::from_value(json!({ "parent": { "type": "block_id", "block_id": id }, "children": [{ "id": empty_row_id, "type": "table_row", "table_row": { "cells": vec![json!([]); next_width] } }] })).map_err(|error| format!("plan empty Notes table row: {error}"))?;
        ids.extend(block_commands::append_block_children_tx(tx, append, false).await?);
    }
    Ok(ids)
}
