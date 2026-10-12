use super::block_comments::update_block_comment_thread_pages;
use super::block_tree::{load_block_row_tx, load_blocks_by_ids, normalize_selection_root_ids};
use super::parents::{
    ParentTarget, parent_target_from_block_row, refresh_parent_has_children, resolve_block_parent,
    touch_page, validate_block_for_parent,
};
use super::sort::{next_sort_orders, sort_order_before, sort_orders_before};
use crate::models::{NoteBlockDto, NoteMoveBlock, NoteMoveBlocks, NotePaginatedBlockList};
use crate::validation::{require_uuid, validate_parent, validate_sort_order};
use crate::{page_history, project_history, reads};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashSet;

pub async fn move_block(
    pool: &SqlitePool,
    block_id: &str,
    request: NoteMoveBlock,
) -> Result<NoteBlockDto, String> {
    let current = reads::get_block_row(pool, block_id, false).await?;
    project_history::ensure_page_baseline_for_mutation(pool, &current.page_id).await?;
    project_history::ensure_parent_baseline_for_mutation(pool, &request.parent).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin move notes block: {e}"))?;
    move_block_tx(&mut tx, block_id, request, true).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit move notes block: {e}"))?;
    reads::get_block(pool, block_id, false).await
}

/// Move a canonical block and its graph within an existing editor transaction.
pub(super) async fn move_block_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
    request: NoteMoveBlock,
    record_history: bool,
) -> Result<(), String> {
    require_uuid(block_id, "block_id")?;
    validate_parent(&request.parent)?;
    let current = load_block_row_tx(tx, block_id, false).await?;
    let old_parent = ParentTarget {
        parent_type: if current.parent_type == "page_id" {
            "page_id"
        } else {
            "block_id"
        },
        parent_page_id: current.parent_page_id.clone(),
        parent_block_id: current.parent_block_id.clone(),
        parent_block_type: None,
        page_id: current.page_id.clone(),
    };
    let new_parent = resolve_block_parent(tx, &request.parent).await?;
    let current_payload: Value = serde_json::from_str(&current.payload)
        .map_err(|e| format!("parse moved block payload: {e}"))?;
    validate_block_for_parent(&new_parent, &current.block_type, &current_payload)?;
    ensure_not_moving_into_self(tx, block_id, &new_parent).await?;
    ensure_not_moving_into_subtree_page(tx, block_id, &new_parent.page_id).await?;
    if request.after.is_some() && request.before.is_some() {
        return Err("move request cannot include both after and before".to_string());
    }
    let sort_order = if let Some(before) = request.before.as_deref() {
        sort_order_before(tx, &new_parent, before).await?
    } else {
        next_sort_orders(tx, &new_parent, request.after.as_deref(), 1).await?[0]
    };
    validate_sort_order(sort_order)?;
    if record_history {
        page_history::record_page_snapshot_tx(tx, &current.page_id, "move_block").await?;
    }
    if current.page_id != new_parent.page_id {
        if record_history {
            page_history::record_page_snapshot_tx(tx, &new_parent.page_id, "move_block").await?;
        }
    }
    sqlx::query(
        "UPDATE notes_blocks
         SET page_id = ?,
             parent_type = ?,
             parent_page_id = ?,
             parent_block_id = ?,
             sort_order = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND in_trash = 0",
    )
    .bind(&new_parent.page_id)
    .bind(new_parent.parent_type)
    .bind(&new_parent.parent_page_id)
    .bind(&new_parent.parent_block_id)
    .bind(sort_order)
    .bind(block_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("move notes block: {e}"))?;
    sqlx::query(
        "WITH RECURSIVE subtree(id) AS (
            SELECT id FROM notes_blocks WHERE parent_block_id = ?
            UNION ALL
            SELECT notes_blocks.id
            FROM notes_blocks
            JOIN subtree ON notes_blocks.parent_block_id = subtree.id
         )
         UPDATE notes_blocks
         SET page_id = ?
         WHERE id IN (SELECT id FROM subtree)",
    )
    .bind(block_id)
    .bind(&new_parent.page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("move notes block descendants: {e}"))?;
    update_block_comment_thread_pages(tx, block_id, &new_parent.page_id).await?;
    if current.block_type == "child_page" {
        sqlx::query(
            "UPDATE notes_pages
             SET parent_type = ?,
                 parent_page_id = ?,
                 parent_block_id = ?,
                 parent_data_source_id = NULL,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(new_parent.parent_type)
        .bind(&new_parent.parent_page_id)
        .bind(&new_parent.parent_block_id)
        .bind(block_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("move notes child page parent: {e}"))?;
    }
    refresh_moved_database_parents(tx, block_id).await?;
    let project_id =
        project_history::resolve_project_id_for_page_tx(tx, &new_parent.page_id).await?;
    super::database_lifecycle::adopt_block_project(tx, block_id, project_id.as_deref()).await?;
    refresh_parent_has_children(tx, &old_parent).await?;
    refresh_parent_has_children(tx, &new_parent).await?;
    touch_page(tx, &new_parent.page_id).await?;
    if old_parent.page_id != new_parent.page_id {
        touch_page(tx, &old_parent.page_id).await?;
    }
    Ok(())
}

/// Move existing identities, optionally restoring their cut-owned graph in the same transaction.
pub async fn move_blocks(
    pool: &SqlitePool,
    request: NoteMoveBlocks,
) -> Result<NotePaginatedBlockList, String> {
    validate_parent(&request.parent)?;
    project_history::ensure_blocks_baseline_for_mutation(pool, &request.block_ids).await?;
    project_history::ensure_parent_baseline_for_mutation(pool, &request.parent).await?;
    if request.after.is_some() && request.before.is_some() {
        return Err("move request cannot include both after and before".to_string());
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin move notes blocks: {e}"))?;
    let include_trashed = request.include_trashed_sources.unwrap_or(false);
    let root_ids =
        normalize_selection_root_ids(&mut tx, &request.block_ids, include_trashed).await?;
    let mut root_rows = Vec::with_capacity(root_ids.len());
    let mut old_parents = Vec::with_capacity(root_ids.len());
    for block_id in &root_ids {
        let row = load_block_row_tx(&mut tx, block_id, include_trashed).await?;
        if row.in_trash != 0 {
            super::database_lifecycle::set_block_trash(&mut tx, block_id, false).await?;
        }
        old_parents.push(parent_target_from_block_row(&row));
        root_rows.push(row);
    }
    let new_parent = resolve_block_parent(&mut tx, &request.parent).await?;
    ensure_insert_anchor_outside_selection(
        &mut tx,
        request.after.as_deref().or(request.before.as_deref()),
        &root_ids,
    )
    .await?;
    for row in &root_rows {
        let payload: Value = serde_json::from_str(&row.payload)
            .map_err(|e| format!("parse moved block payload: {e}"))?;
        validate_block_for_parent(&new_parent, &row.block_type, &payload)?;
        ensure_not_moving_into_self(&mut tx, &row.id, &new_parent).await?;
        ensure_not_moving_into_subtree_page(&mut tx, &row.id, &new_parent.page_id).await?;
    }
    let sort_orders = if let Some(before) = request.before.as_deref() {
        sort_orders_before(&mut tx, &new_parent, before, root_rows.len()).await?
    } else {
        next_sort_orders(
            &mut tx,
            &new_parent,
            request.after.as_deref(),
            root_rows.len(),
        )
        .await?
    };
    let mut history_page_ids = HashSet::from([new_parent.page_id.clone()]);
    for row in &root_rows {
        history_page_ids.insert(row.page_id.clone());
    }
    for page_id in history_page_ids {
        page_history::record_page_snapshot_tx(&mut tx, &page_id, "move_blocks").await?;
    }
    for (row, sort_order) in root_rows.iter().zip(sort_orders) {
        validate_sort_order(sort_order)?;
        sqlx::query(
            "UPDATE notes_blocks
             SET page_id = ?,
                 parent_type = ?,
                 parent_page_id = ?,
                 parent_block_id = ?,
                 sort_order = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0",
        )
        .bind(&new_parent.page_id)
        .bind(new_parent.parent_type)
        .bind(&new_parent.parent_page_id)
        .bind(&new_parent.parent_block_id)
        .bind(sort_order)
        .bind(&row.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("move notes block: {e}"))?;
        sqlx::query(
            "WITH RECURSIVE subtree(id) AS (
                SELECT id FROM notes_blocks WHERE parent_block_id = ?
                UNION ALL
                SELECT notes_blocks.id
                FROM notes_blocks
                JOIN subtree ON notes_blocks.parent_block_id = subtree.id
             )
             UPDATE notes_blocks
             SET page_id = ?
             WHERE id IN (SELECT id FROM subtree)",
        )
        .bind(&row.id)
        .bind(&new_parent.page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("move notes block descendants: {e}"))?;
        update_block_comment_thread_pages(&mut tx, &row.id, &new_parent.page_id).await?;
        if row.block_type == "child_page" {
            sqlx::query(
                "UPDATE notes_pages
                 SET parent_type = ?,
                     parent_page_id = ?,
                     parent_block_id = ?,
                     parent_data_source_id = NULL,
                     last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?",
            )
            .bind(new_parent.parent_type)
            .bind(&new_parent.parent_page_id)
            .bind(&new_parent.parent_block_id)
            .bind(&row.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("move notes child page parent: {e}"))?;
        }
    }
    for parent in &old_parents {
        refresh_parent_has_children(&mut tx, parent).await?;
        if parent.page_id != new_parent.page_id {
            touch_page(&mut tx, &parent.page_id).await?;
        }
    }
    let project_id =
        project_history::resolve_project_id_for_page_tx(&mut tx, &new_parent.page_id).await?;
    for root in &root_ids {
        refresh_moved_database_parents(&mut tx, root).await?;
        super::database_lifecycle::adopt_block_project(&mut tx, root, project_id.as_deref())
            .await?;
    }
    refresh_parent_has_children(&mut tx, &new_parent).await?;
    touch_page(&mut tx, &new_parent.page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit move notes blocks: {e}"))?;
    load_blocks_by_ids(pool, root_ids).await
}

pub(super) async fn ensure_not_moving_into_self(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
    parent: &ParentTarget,
) -> Result<(), String> {
    let Some(parent_block_id) = &parent.parent_block_id else {
        return Ok(());
    };
    if parent_block_id == block_id {
        return Err("block cannot be moved under itself".to_string());
    }
    let descendant: Option<i64> = sqlx::query_scalar(
        "WITH RECURSIVE subtree(id) AS (
            SELECT id FROM notes_blocks WHERE parent_block_id = ?
            UNION ALL
            SELECT notes_blocks.id
            FROM notes_blocks
            JOIN subtree ON notes_blocks.parent_block_id = subtree.id
         )
         SELECT 1 FROM subtree WHERE id = ? LIMIT 1",
    )
    .bind(block_id)
    .bind(parent_block_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("check notes move cycle: {e}"))?;
    if descendant.is_some() {
        return Err("block cannot be moved under its descendant".to_string());
    }
    Ok(())
}

pub(super) async fn ensure_insert_anchor_outside_selection(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    anchor_id: Option<&str>,
    root_ids: &[String],
) -> Result<(), String> {
    let Some(anchor_id) = anchor_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    require_uuid(anchor_id, "anchor")?;
    for root_id in root_ids {
        let inside_subtree: Option<i64> = sqlx::query_scalar(
            "WITH RECURSIVE subtree(id) AS (
                SELECT id FROM notes_blocks WHERE id = ?
                UNION ALL
                SELECT notes_blocks.id
                FROM notes_blocks
                JOIN subtree ON notes_blocks.parent_block_id = subtree.id
             )
             SELECT 1 FROM subtree WHERE id = ? LIMIT 1",
        )
        .bind(root_id)
        .bind(anchor_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("check notes selection anchor: {e}"))?;
        if inside_subtree.is_some() {
            return Err("insert anchor cannot be inside the selected block subtree".to_string());
        }
    }
    Ok(())
}

pub(super) async fn ensure_not_moving_into_subtree_page(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
    destination_page_id: &str,
) -> Result<(), String> {
    let is_subtree_page: Option<i64> = sqlx::query_scalar(
        "WITH RECURSIVE subtree(kind, id) AS (
            SELECT 'block', id FROM notes_blocks WHERE id = ?
            UNION
            SELECT 'block', block.id FROM notes_blocks AS block JOIN subtree AS parent
                ON (parent.kind = 'block' AND block.parent_block_id = parent.id)
                OR (parent.kind = 'page' AND block.page_id = parent.id)
            UNION
            SELECT 'page', page.id FROM notes_pages AS page JOIN subtree AS parent
                ON (parent.kind = 'block' AND (page.id = parent.id OR page.parent_block_id = parent.id))
                OR (parent.kind = 'page' AND page.parent_page_id = parent.id)
                OR (parent.kind = 'source' AND page.parent_data_source_id = parent.id)
            UNION
            SELECT 'source', source.id FROM notes_data_sources AS source JOIN subtree AS parent
                ON parent.kind = 'block' AND source.database_id = parent.id
         )
         SELECT 1 FROM subtree WHERE kind = 'page' AND id = ? LIMIT 1",
    )
    .bind(block_id)
    .bind(destination_page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("check notes block subtree pages: {e}"))?;
    if is_subtree_page.is_some() {
        return Err("block cannot be moved into a page contained by its subtree".to_string());
    }
    Ok(())
}

async fn refresh_moved_database_parents(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    root_id: &str,
) -> Result<(), String> {
    sqlx::query("WITH RECURSIVE subtree(id) AS (
        SELECT id FROM notes_blocks WHERE id = ? UNION
        SELECT block.id FROM notes_blocks AS block JOIN subtree ON block.parent_block_id = subtree.id
    ) UPDATE notes_databases SET
        parent_type = (SELECT parent_type FROM notes_blocks WHERE id = notes_databases.id),
        parent_page_id = (SELECT parent_page_id FROM notes_blocks WHERE id = notes_databases.id),
        parent_block_id = (SELECT parent_block_id FROM notes_blocks WHERE id = notes_databases.id),
        last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
        WHERE id IN (SELECT id FROM subtree)")
        .bind(root_id).execute(&mut **tx).await.map_err(|e| format!("refresh moved Notes database placement: {e}"))?;
    Ok(())
}
