use super::block_comments::duplicate_block_comment_threads;
use super::block_tree::{
    load_block_subtree_rows, load_block_subtree_rows_including_trashed, load_blocks_by_ids,
    normalize_selection_root_ids, refresh_duplicated_has_children,
};
use super::copy_budget::{CopyBudget, CopyContext};
use super::database_copy::{insert_database_copy, plan_database_copy};
use super::page_duplicates::{insert_child_page_copy, plan_child_page_copy};
use super::parents::{
    ParentTarget, refresh_parent_has_children, resolve_block_parent, touch_page,
    validate_block_for_parent,
};
use super::sort::{next_sort_orders, sort_orders_before};
use crate::models::{
    NoteBlockDto, NoteDuplicateBlock, NoteDuplicateBlocks, NoteDuplicatedBlockId,
    NotePaginatedBlockList, NoteParent,
};
use crate::validation::{
    require_uuid, validate_duplicate_block_count, validate_parent, validate_sort_order,
};
use crate::{page_history, project_history, reads};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

pub async fn duplicate_block(
    pool: &SqlitePool,
    block_id: &str,
    request: NoteDuplicateBlock,
) -> Result<NoteBlockDto, String> {
    let block_id = block_id.trim();
    require_uuid(block_id, "block_id")?;
    let source = reads::get_block_row(pool, block_id, false).await?;
    project_history::ensure_page_baseline_for_mutation(pool, &source.page_id).await?;
    validate_duplicate_block_count(request.duplicated_block_ids.len())?;
    let mut duplicate_ids = HashMap::with_capacity(request.duplicated_block_ids.len());
    let mut seen_duplicate_ids = HashSet::with_capacity(request.duplicated_block_ids.len());
    for pair in request.duplicated_block_ids {
        let source_id = pair.source_id.trim().to_string();
        let duplicate_id = pair.duplicate_id.trim().to_string();
        require_uuid(&source_id, "source_id")?;
        require_uuid(&duplicate_id, "duplicate_id")?;
        if !seen_duplicate_ids.insert(duplicate_id.clone()) {
            return Err("duplicate_id values must be unique".to_string());
        }
        if duplicate_ids.insert(source_id, duplicate_id).is_some() {
            return Err("source_id values must be unique".to_string());
        }
    }
    let duplicate_root_id = duplicate_ids
        .get(block_id)
        .cloned()
        .ok_or_else(|| "duplicated_block_ids must include the source block".to_string())?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin duplicate notes block: {e}"))?;
    let mut budget = CopyBudget::default();
    let source_rows = load_block_subtree_rows(&mut tx, block_id, &mut budget).await?;
    if source_rows.is_empty() {
        return Err("notes block not found".to_string());
    }
    if source_rows.len() != duplicate_ids.len() {
        return Err("duplicated_block_ids must match the source block subtree".to_string());
    }
    let source_id_set = source_rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<HashSet<_>>();
    for source_id in duplicate_ids.keys() {
        if !source_id_set.contains(source_id.as_str()) {
            return Err("duplicated_block_ids must match the source block subtree".to_string());
        }
    }
    for duplicate_id in &seen_duplicate_ids {
        if source_id_set.contains(duplicate_id.as_str()) {
            return Err("duplicate_id values must not match source_id values".to_string());
        }
    }
    let source_root = source_rows
        .first()
        .ok_or_else(|| "notes block not found".to_string())?;
    let root_parent = ParentTarget {
        parent_type: if source_root.parent_type == "page_id" {
            "page_id"
        } else {
            "block_id"
        },
        parent_page_id: source_root.parent_page_id.clone(),
        parent_block_id: source_root.parent_block_id.clone(),
        parent_block_type: None,
        page_id: source_root.page_id.clone(),
    };
    let root_sort_order =
        next_sort_orders(&mut tx, &root_parent, Some(source_root.id.as_str()), 1).await?[0];
    page_history::record_page_snapshot_tx(&mut tx, &source_root.page_id, "duplicate_block").await?;
    let mut reserved_ids = seen_duplicate_ids.clone();
    let mut context = CopyContext {
        reserved_ids: &mut reserved_ids,
        budget: &mut budget,
    };
    let mut page_copies = Vec::new();
    let destination_project_id =
        project_history::resolve_project_id_for_page_tx(&mut tx, &source_root.page_id).await?;
    for row in source_rows
        .iter()
        .filter(|row| row.block_type == "child_page")
    {
        let parent = if row.id == source_root.id {
            if let Some(id) = &root_parent.parent_page_id {
                NoteParent::PageId {
                    page_id: id.clone(),
                }
            } else {
                NoteParent::BlockId {
                    block_id: root_parent
                        .parent_block_id
                        .clone()
                        .ok_or("missing block parent")?,
                }
            }
        } else {
            NoteParent::BlockId {
                block_id: duplicate_ids
                    .get(
                        row.parent_block_id
                            .as_deref()
                            .ok_or("missing block parent")?,
                    )
                    .cloned()
                    .ok_or("missing duplicate parent")?,
            }
        };
        page_copies.push(
            plan_child_page_copy(
                &mut tx,
                &row.id,
                &duplicate_ids[&row.id],
                parent,
                false,
                &mut context,
                destination_project_id.as_deref(),
            )
            .await?,
        );
    }
    let mut databases = Vec::new();
    for row in source_rows
        .iter()
        .filter(|row| row.block_type == "child_database")
    {
        if super::database_copy::has_database_graph(row)? {
            databases.push(
                plan_database_copy(
                    &mut tx,
                    row,
                    &duplicate_ids[&row.id],
                    &mut context,
                    destination_project_id.as_deref(),
                    false,
                )
                .await?,
            );
        }
    }
    for row in &source_rows {
        let duplicate_id = duplicate_ids.get(&row.id).ok_or_else(|| {
            "duplicated_block_ids must match the source block subtree".to_string()
        })?;
        let (parent_type, parent_page_id, parent_block_id, sort_order) =
            if row.id == source_root.id {
                (
                    root_parent.parent_type,
                    root_parent.parent_page_id.clone(),
                    root_parent.parent_block_id.clone(),
                    root_sort_order,
                )
            } else {
                let source_parent_id = row.parent_block_id.as_ref().ok_or_else(|| {
                    "duplicated descendant block is missing its parent".to_string()
                })?;
                let duplicate_parent_id = duplicate_ids.get(source_parent_id).ok_or_else(|| {
                    "duplicated_block_ids must include every descendant parent".to_string()
                })?;
                (
                    "block_id",
                    None,
                    Some(duplicate_parent_id.clone()),
                    row.sort_order,
                )
            };
        validate_sort_order(sort_order)?;
        let mut payload: Value = serde_json::from_str(&row.payload)
            .map_err(|e| format!("parse copied block payload: {e}"))?;
        super::database_copy::strip_trash_metadata(&mut payload);
        sqlx::query(
            "INSERT INTO notes_blocks (
                id,
                page_id,
                parent_type,
                parent_page_id,
                parent_block_id,
                has_children,
                type,
                payload,
                plain_text,
                sort_order
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(duplicate_id)
        .bind(&source_root.page_id)
        .bind(parent_type)
        .bind(parent_page_id)
        .bind(parent_block_id)
        .bind(row.has_children)
        .bind(&row.block_type)
        .bind(payload.to_string())
        .bind(&row.plain_text)
        .bind(sort_order)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("duplicate notes block: {e}"))?;
    }
    for copy in &page_copies {
        insert_child_page_copy(&mut tx, copy).await?;
    }
    for database in &databases {
        sqlx::query("UPDATE notes_blocks SET payload = ? WHERE id = ?")
            .bind(database.payload.to_string())
            .bind(&database.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("set duplicated database identities: {e}"))?;
        insert_database_copy(&mut tx, database).await?;
    }
    super::database_copy::finalize_copies(&mut tx, &databases, &page_copies, &duplicate_ids)
        .await?;
    refresh_duplicated_has_children(&mut tx, &seen_duplicate_ids).await?;
    duplicate_block_comment_threads(&mut tx, &duplicate_ids, &source_root.page_id).await?;
    touch_page(&mut tx, &source_root.page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit duplicate notes block: {e}"))?;
    reads::get_block(pool, &duplicate_root_id, false).await
}

pub async fn duplicate_blocks(
    pool: &SqlitePool,
    request: NoteDuplicateBlocks,
) -> Result<NotePaginatedBlockList, String> {
    project_history::ensure_blocks_baseline_for_mutation(pool, &request.block_ids).await?;
    project_history::ensure_parent_baseline_for_mutation(pool, &request.parent).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin duplicate notes blocks: {e}"))?;
    let ids = duplicate_blocks_tx(&mut tx, request, true, &mut CopyBudget::default()).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit duplicate notes blocks: {e}"))?;
    load_blocks_by_ids(pool, ids).await
}

/// Copy complete canonical page and database graphs in the caller's transaction.
pub(super) async fn duplicate_blocks_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    request: NoteDuplicateBlocks,
    record_history: bool,
    budget: &mut CopyBudget,
) -> Result<Vec<String>, String> {
    validate_parent(&request.parent)?;
    if request.after.is_some() && request.before.is_some() {
        return Err("duplicate request cannot include both after and before".to_string());
    }
    validate_duplicate_block_count(request.duplicated_block_ids.len())?;
    let (duplicate_ids, seen_duplicate_ids) = duplicate_block_id_map(request.duplicated_block_ids)?;
    let include_trashed_sources = request.include_trashed_sources.unwrap_or(false);
    let root_ids =
        normalize_selection_root_ids(tx, &request.block_ids, include_trashed_sources).await?;
    let destination_parent = resolve_block_parent(tx, &request.parent).await?;
    let mut source_rows = Vec::new();
    let mut root_rows = Vec::with_capacity(root_ids.len());
    for block_id in &root_ids {
        let subtree_rows = load_block_subtree_rows_including_trashed(
            tx,
            block_id,
            include_trashed_sources,
            budget,
        )
        .await?;
        let source_root = subtree_rows
            .first()
            .ok_or_else(|| "notes block not found".to_string())?;
        let payload: Value = serde_json::from_str(&source_root.payload)
            .map_err(|e| format!("parse duplicated block payload: {e}"))?;
        validate_block_for_parent(&destination_parent, &source_root.block_type, &payload)?;
        root_rows.push(source_root.clone());
        source_rows.extend(subtree_rows);
    }
    if source_rows.len() != duplicate_ids.len() {
        return Err("duplicated_block_ids must match the source block subtrees".to_string());
    }
    let source_id_set = source_rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<HashSet<_>>();
    for source_id in duplicate_ids.keys() {
        if !source_id_set.contains(source_id.as_str()) {
            return Err("duplicated_block_ids must match the source block subtrees".to_string());
        }
    }
    for duplicate_id in &seen_duplicate_ids {
        if source_id_set.contains(duplicate_id.as_str()) {
            return Err("duplicate_id values must not match source_id values".to_string());
        }
    }
    let root_id_set = root_ids.iter().map(String::as_str).collect::<HashSet<_>>();
    let root_sort_orders = if let Some(before) = request.before.as_deref() {
        sort_orders_before(tx, &destination_parent, before, root_rows.len()).await?
    } else {
        next_sort_orders(
            tx,
            &destination_parent,
            request.after.as_deref(),
            root_rows.len(),
        )
        .await?
    };
    let root_sort_order_by_source = root_rows
        .iter()
        .zip(root_sort_orders)
        .map(|(row, sort_order)| (row.id.as_str(), sort_order))
        .collect::<HashMap<_, _>>();
    if record_history {
        page_history::record_page_snapshot_tx(tx, &destination_parent.page_id, "duplicate_blocks")
            .await?;
    }
    let mut reserved_ids = seen_duplicate_ids.clone();
    let mut context = CopyContext {
        reserved_ids: &mut reserved_ids,
        budget,
    };
    let mut page_copies = Vec::new();
    let destination_project_id =
        project_history::resolve_project_id_for_page_tx(tx, &destination_parent.page_id).await?;
    for row in source_rows
        .iter()
        .filter(|row| row.block_type == "child_page")
    {
        let parent = if root_id_set.contains(row.id.as_str()) {
            request.parent.clone()
        } else {
            NoteParent::BlockId {
                block_id: duplicate_ids
                    .get(
                        row.parent_block_id
                            .as_deref()
                            .ok_or("missing block parent")?,
                    )
                    .cloned()
                    .ok_or("missing duplicate parent")?,
            }
        };
        page_copies.push(
            plan_child_page_copy(
                tx,
                &row.id,
                &duplicate_ids[&row.id],
                parent,
                include_trashed_sources,
                &mut context,
                destination_project_id.as_deref(),
            )
            .await?,
        );
    }
    let mut databases = Vec::new();
    for row in source_rows
        .iter()
        .filter(|row| row.block_type == "child_database")
    {
        if super::database_copy::has_database_graph(row)? {
            databases.push(
                plan_database_copy(
                    tx,
                    row,
                    &duplicate_ids[&row.id],
                    &mut context,
                    destination_project_id.as_deref(),
                    include_trashed_sources,
                )
                .await?,
            );
        }
    }
    for row in &source_rows {
        let duplicate_id = duplicate_ids.get(&row.id).ok_or_else(|| {
            "duplicated_block_ids must match the source block subtrees".to_string()
        })?;
        let (parent_type, parent_page_id, parent_block_id, sort_order) =
            if root_id_set.contains(row.id.as_str()) {
                (
                    destination_parent.parent_type,
                    destination_parent.parent_page_id.clone(),
                    destination_parent.parent_block_id.clone(),
                    *root_sort_order_by_source
                        .get(row.id.as_str())
                        .ok_or_else(|| "duplicated root sort order is missing".to_string())?,
                )
            } else {
                let source_parent_id = row.parent_block_id.as_ref().ok_or_else(|| {
                    "duplicated descendant block is missing its parent".to_string()
                })?;
                let duplicate_parent_id = duplicate_ids.get(source_parent_id).ok_or_else(|| {
                    "duplicated_block_ids must include every descendant parent".to_string()
                })?;
                (
                    "block_id",
                    None,
                    Some(duplicate_parent_id.clone()),
                    row.sort_order,
                )
            };
        validate_sort_order(sort_order)?;
        let mut payload: Value = serde_json::from_str(&row.payload)
            .map_err(|e| format!("parse pasted block payload: {e}"))?;
        super::database_copy::strip_trash_metadata(&mut payload);
        sqlx::query(
            "INSERT INTO notes_blocks (
                id,
                page_id,
                parent_type,
                parent_page_id,
                parent_block_id,
                has_children,
                type,
                payload,
                plain_text,
                sort_order
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(duplicate_id)
        .bind(&destination_parent.page_id)
        .bind(parent_type)
        .bind(parent_page_id)
        .bind(parent_block_id)
        .bind(row.has_children)
        .bind(&row.block_type)
        .bind(payload.to_string())
        .bind(&row.plain_text)
        .bind(sort_order)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("duplicate notes block: {e}"))?;
    }
    for copy in &page_copies {
        insert_child_page_copy(tx, copy).await?;
    }
    for database in &databases {
        sqlx::query("UPDATE notes_blocks SET payload = ? WHERE id = ?")
            .bind(database.payload.to_string())
            .bind(&database.id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("set pasted database identities: {e}"))?;
        insert_database_copy(tx, database).await?;
    }
    super::database_copy::finalize_copies(tx, &databases, &page_copies, &duplicate_ids).await?;
    refresh_duplicated_has_children(tx, &seen_duplicate_ids).await?;
    duplicate_block_comment_threads(tx, &duplicate_ids, &destination_parent.page_id).await?;
    refresh_parent_has_children(tx, &destination_parent).await?;
    touch_page(tx, &destination_parent.page_id).await?;
    let all_duplicate_ids = source_rows
        .iter()
        .map(|row| {
            duplicate_ids
                .get(&row.id)
                .cloned()
                .ok_or_else(|| "duplicated block id is missing".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(all_duplicate_ids)
}

pub(super) fn duplicate_block_id_map(
    pairs: Vec<NoteDuplicatedBlockId>,
) -> Result<(HashMap<String, String>, HashSet<String>), String> {
    let mut duplicate_ids = HashMap::with_capacity(pairs.len());
    let mut seen_duplicate_ids = HashSet::with_capacity(pairs.len());
    for pair in pairs {
        let source_id = pair.source_id.trim().to_string();
        let duplicate_id = pair.duplicate_id.trim().to_string();
        require_uuid(&source_id, "source_id")?;
        require_uuid(&duplicate_id, "duplicate_id")?;
        if !seen_duplicate_ids.insert(duplicate_id.clone()) {
            return Err("duplicate_id values must be unique".to_string());
        }
        if duplicate_ids.insert(source_id, duplicate_id).is_some() {
            return Err("source_id values must be unique".to_string());
        }
    }
    Ok((duplicate_ids, seen_duplicate_ids))
}
