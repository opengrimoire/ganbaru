use super::block_tree::load_child_page_block_row_including_trashed;
use super::pages::load_page_row;
use super::parents::{
    ParentTarget, parent_target_from_block_row, refresh_parent_has_children, touch_page,
};
use super::payloads::child_page_payload;
use super::sort::next_sort_orders;
use crate::models::{
    NoteBlockRow, NoteLoadedPage, NoteMovePage, NotePageDto, NoteParent, page_parent_columns,
};
use crate::validation::{
    plain_text_from_payload, require_uuid, validate_parent, validate_sort_order,
};
use crate::{data_sources, page_history, reads};
use sqlx::SqlitePool;
use std::collections::HashSet;

const TRASH_RETENTION_DAYS: i64 = 7;

pub(super) type PageParentColumns = (String, Option<String>, Option<String>, Option<String>);

pub async fn move_page(
    pool: &SqlitePool,
    page_id: &str,
    request: NoteMovePage,
) -> Result<NoteLoadedPage, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    validate_parent(&request.parent)?;
    let folder_id = request.folder_id.as_deref().map(str::trim);
    if let Some(folder_id) = folder_id {
        require_uuid(folder_id, "folder_id")?;
        if !matches!(&request.parent, NoteParent::Workspace { workspace: true }) {
            return Err("folder_id requires a workspace parent".to_string());
        }
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin move notes page: {e}"))?;
    let source_page = load_page_row(&mut tx, page_id).await?;
    let child_block = load_child_page_block_row_including_trashed(&mut tx, page_id).await?;
    let old_parent = child_block
        .as_ref()
        .filter(|block| block.in_trash == 0)
        .map(parent_target_from_block_row);
    let new_parent = resolve_page_move_parent(&mut tx, page_id, &request.parent).await?;
    let (parent_type, parent_page_id, parent_block_id, parent_data_source_id) =
        page_parent_columns(&request.parent);
    let mut history_page_ids = HashSet::from([page_id.to_string()]);
    if let Some(parent) = &old_parent {
        history_page_ids.insert(parent.page_id.clone());
    }
    if let Some(parent) = &new_parent {
        history_page_ids.insert(parent.page_id.clone());
    }
    for history_page_id in history_page_ids {
        page_history::record_page_snapshot_tx(&mut tx, &history_page_id, "move_page").await?;
    }
    sqlx::query(
        "UPDATE notes_pages
         SET parent_type = ?,
             parent_page_id = ?,
             parent_block_id = ?,
             parent_data_source_id = ?,
             folder_id = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(parent_type)
    .bind(parent_page_id)
    .bind(parent_block_id)
    .bind(parent_data_source_id)
    .bind(folder_id)
    .bind(page_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("move notes page: {e}"))?;
    match &new_parent {
        Some(parent) => {
            upsert_moved_child_page_block(
                &mut tx,
                page_id,
                &source_page.title,
                parent,
                child_block,
            )
            .await?;
            refresh_parent_has_children(&mut tx, parent).await?;
            touch_page(&mut tx, &parent.page_id).await?;
        }
        None => {
            if child_block
                .as_ref()
                .is_some_and(|block| block.in_trash == 0)
            {
                sqlx::query(
                    "UPDATE notes_blocks
                     SET in_trash = 1,
                         last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ? AND type = 'child_page'",
                )
                .bind(page_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("hide moved notes child page block: {e}"))?;
            }
        }
    }
    if let Some(parent) = &old_parent {
        refresh_parent_has_children(&mut tx, parent).await?;
        touch_page(&mut tx, &parent.page_id).await?;
    }
    touch_page(&mut tx, page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit move notes page: {e}"))?;
    reads::load_page(pool, page_id).await
}

pub async fn trash_page(
    pool: &SqlitePool,
    page_id: &str,
    in_trash: bool,
) -> Result<NotePageDto, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin trash notes page: {e}"))?;
    let page_exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM notes_pages WHERE id = ?")
        .bind(page_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("load notes page before trash: {e}"))?;
    if page_exists.is_none() {
        return Err("notes page not found".to_string());
    }
    page_history::record_page_snapshot_tx(&mut tx, page_id, "trash_page").await?;
    let child_parents = load_external_child_page_block_parents(&mut tx, page_id).await?;
    let root_child_page_block_visible = if in_trash {
        false
    } else {
        repair_page_parent_for_active_restore(&mut tx, page_id).await?
    };
    super::database_lifecycle::set_page_trash(&mut tx, page_id, in_trash).await?;
    if in_trash {
        sqlx::query("UPDATE notes_pages SET archived = 0 WHERE id = ?")
            .bind(page_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("clear trashed Notes page archive state: {e}"))?;
    }
    set_root_child_page_block_visibility(&mut tx, page_id, root_child_page_block_visible).await?;
    for parent in child_parents {
        refresh_parent_has_children(&mut tx, &parent).await?;
        touch_page(&mut tx, &parent.page_id).await?;
    }
    data_sources::rollups::invalidate_rollup_cache_for_page_tx(&mut tx, page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit trash notes page: {e}"))?;
    reads::get_page(pool, page_id, true).await
}

pub async fn purge_expired_trashed_pages(pool: &SqlitePool) -> Result<Vec<String>, String> {
    let retention_modifier = format!("-{TRASH_RETENTION_DAYS} days");
    let expired_page_ids: Vec<String> = sqlx::query_scalar(
        "SELECT id
         FROM notes_pages
         WHERE in_trash = 1
           AND trashed_time IS NOT NULL
           AND trashed_time <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)
         ORDER BY trashed_time ASC, id ASC",
    )
    .bind(retention_modifier)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list expired trashed notes pages: {e}"))?;
    let mut deleted_page_ids = Vec::new();
    for page_id in expired_page_ids {
        let still_trashed: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 1")
                .bind(&page_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("check expired trashed notes page: {e}"))?;
        if still_trashed.is_none() {
            continue;
        }
        deleted_page_ids.extend(permanently_delete_page(pool, &page_id).await?);
    }
    deleted_page_ids.sort();
    deleted_page_ids.dedup();
    Ok(deleted_page_ids)
}

pub async fn permanently_delete_page(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<Vec<String>, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin permanent notes page delete: {e}"))?;
    let root_trash_state: Option<i64> =
        sqlx::query_scalar("SELECT in_trash FROM notes_pages WHERE id = ?")
            .bind(page_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| format!("load notes page before permanent delete: {e}"))?;
    match root_trash_state {
        Some(1) => {}
        Some(_) => return Err("notes page must be in trash before permanent delete".to_string()),
        None => return Err("notes page not found".to_string()),
    }
    let deleted_page_ids: Vec<String> = sqlx::query_scalar(
        "WITH RECURSIVE page_subtree(id) AS (
            SELECT id FROM notes_pages WHERE id = ?
            UNION
            SELECT child.id
            FROM notes_pages AS child
            JOIN page_subtree AS parent ON child.parent_page_id = parent.id
            UNION
            SELECT child.id
            FROM notes_pages AS child
            JOIN notes_blocks AS parent_block ON child.parent_block_id = parent_block.id
            JOIN page_subtree AS parent ON parent_block.page_id = parent.id
            UNION
            SELECT child_block.id
            FROM notes_blocks AS child_block
            JOIN page_subtree AS parent ON child_block.page_id = parent.id
            WHERE child_block.type = 'child_page'
         )
         SELECT id FROM page_subtree ORDER BY id ASC",
    )
    .bind(page_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| format!("load permanent notes page delete subtree: {e}"))?;
    let child_parents = load_external_child_page_block_parents(&mut tx, page_id).await?;
    data_sources::rollups::invalidate_rollup_cache_for_page_tx(&mut tx, page_id).await?;
    sqlx::query(
        "WITH RECURSIVE page_subtree(id) AS (
            SELECT id FROM notes_pages WHERE id = ?
            UNION
            SELECT child.id
            FROM notes_pages AS child
            JOIN page_subtree AS parent ON child.parent_page_id = parent.id
            UNION
            SELECT child.id
            FROM notes_pages AS child
            JOIN notes_blocks AS parent_block ON child.parent_block_id = parent_block.id
            JOIN page_subtree AS parent ON parent_block.page_id = parent.id
            UNION
            SELECT child_block.id
            FROM notes_blocks AS child_block
            JOIN page_subtree AS parent ON child_block.page_id = parent.id
            WHERE child_block.type = 'child_page'
         )
         DELETE FROM notes_blocks
         WHERE type = 'child_page' AND id IN (SELECT id FROM page_subtree)",
    )
    .bind(page_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("delete notes child page blocks permanently: {e}"))?;
    sqlx::query(
        "WITH RECURSIVE page_subtree(id) AS (
            SELECT id FROM notes_pages WHERE id = ?
            UNION
            SELECT child.id
            FROM notes_pages AS child
            JOIN page_subtree AS parent ON child.parent_page_id = parent.id
            UNION
            SELECT child.id
            FROM notes_pages AS child
            JOIN notes_blocks AS parent_block ON child.parent_block_id = parent_block.id
            JOIN page_subtree AS parent ON parent_block.page_id = parent.id
            UNION
            SELECT child_block.id
            FROM notes_blocks AS child_block
            JOIN page_subtree AS parent ON child_block.page_id = parent.id
            WHERE child_block.type = 'child_page'
         )
         DELETE FROM notes_pages
         WHERE id IN (SELECT id FROM page_subtree)",
    )
    .bind(page_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("delete notes pages permanently: {e}"))?;
    for parent in child_parents {
        refresh_parent_has_children(&mut tx, &parent).await?;
        touch_page(&mut tx, &parent.page_id).await?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit permanent notes page delete: {e}"))?;
    Ok(deleted_page_ids)
}

pub(super) async fn repair_page_parent_for_active_restore(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
) -> Result<bool, String> {
    let row: Option<PageParentColumns> = sqlx::query_as(
        "SELECT parent_type, parent_page_id, parent_block_id, parent_data_source_id
         FROM notes_pages
         WHERE id = ?",
    )
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load restored notes page parent: {e}"))?;
    let Some((parent_type, parent_page_id, parent_block_id, parent_data_source_id)) = row else {
        return Err("notes page not found".to_string());
    };
    let parent_is_active = match parent_type.as_str() {
        "workspace" => return Ok(false),
        "page_id" => {
            let Some(parent_page_id) = parent_page_id else {
                move_page_to_workspace_parent(tx, page_id).await?;
                return Ok(false);
            };
            active_page_parent_exists(tx, &parent_page_id).await?
        }
        "block_id" => {
            let Some(parent_block_id) = parent_block_id else {
                move_page_to_workspace_parent(tx, page_id).await?;
                return Ok(false);
            };
            active_block_parent_exists(tx, &parent_block_id).await?
        }
        "data_source_id" => {
            let Some(parent_data_source_id) = parent_data_source_id else {
                move_page_to_workspace_parent(tx, page_id).await?;
                return Ok(false);
            };
            active_data_source_parent_exists(tx, &parent_data_source_id).await?
        }
        _ => false,
    };
    if parent_is_active {
        return Ok(true);
    }
    move_page_to_workspace_parent(tx, page_id).await?;
    Ok(false)
}

pub(super) async fn active_page_parent_exists(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent_page_id: &str,
) -> Result<bool, String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1
         FROM notes_pages
         WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(parent_page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("check restored notes page parent: {e}"))?;
    Ok(exists.is_some())
}

pub(super) async fn active_block_parent_exists(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent_block_id: &str,
) -> Result<bool, String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1
         FROM notes_blocks AS block
         JOIN notes_pages AS page ON page.id = block.page_id
         WHERE block.id = ?
           AND block.in_trash = 0
           AND page.in_trash = 0
           AND page.archived = 0",
    )
    .bind(parent_block_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("check restored notes block parent: {e}"))?;
    Ok(exists.is_some())
}

pub(super) async fn active_data_source_parent_exists(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent_data_source_id: &str,
) -> Result<bool, String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1
         FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE data_source.id = ?
           AND data_source.in_trash = 0
           AND database.in_trash = 0",
    )
    .bind(parent_data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("check restored notes data source parent: {e}"))?;
    Ok(exists.is_some())
}

pub(super) async fn move_page_to_workspace_parent(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_pages
         SET parent_type = 'workspace',
             parent_page_id = NULL,
             parent_block_id = NULL,
             parent_data_source_id = NULL,
             folder_id = NULL,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("move restored notes page to workspace: {e}"))?;
    Ok(())
}

pub(super) async fn set_root_child_page_block_visibility(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
    visible: bool,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_blocks
         SET in_trash = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND type = 'child_page'",
    )
    .bind(if visible { 0_i64 } else { 1_i64 })
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("sync restored notes child page block: {e}"))?;
    Ok(())
}

pub(super) async fn load_external_child_page_block_parents(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
) -> Result<Vec<ParentTarget>, String> {
    let rows = sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
        "WITH RECURSIVE page_subtree(id) AS (
            SELECT id FROM notes_pages WHERE id = ?
            UNION
            SELECT child.id
            FROM notes_pages AS child
            JOIN page_subtree AS parent ON child.parent_page_id = parent.id
            UNION
            SELECT child.id
            FROM notes_pages AS child
            JOIN notes_blocks AS parent_block ON child.parent_block_id = parent_block.id
            JOIN page_subtree AS parent ON parent_block.page_id = parent.id
            UNION
            SELECT child_block.id
            FROM notes_blocks AS child_block
            JOIN page_subtree AS parent ON child_block.page_id = parent.id
            WHERE child_block.type = 'child_page'
         )
         SELECT DISTINCT block.page_id,
                         block.parent_type,
                         block.parent_page_id,
                         block.parent_block_id
         FROM notes_blocks AS block
         WHERE block.type = 'child_page'
           AND block.id IN (SELECT id FROM page_subtree)
           AND block.page_id NOT IN (SELECT id FROM page_subtree)",
    )
    .bind(page_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load external notes child page block parents: {e}"))?;
    Ok(rows
        .into_iter()
        .map(
            |(parent_page_id, parent_type, parent_parent_page_id, parent_block_id)| ParentTarget {
                parent_type: if parent_type == "page_id" {
                    "page_id"
                } else {
                    "block_id"
                },
                parent_page_id: parent_parent_page_id,
                parent_block_id,
                parent_block_type: None,
                page_id: parent_page_id,
            },
        )
        .collect())
}

pub async fn archive_page(
    pool: &SqlitePool,
    page_id: &str,
    archived: bool,
) -> Result<NotePageDto, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin archive notes page: {e}"))?;
    let child_block = load_child_page_block_row_including_trashed(&mut tx, page_id).await?;
    let child_parent = child_block.as_ref().map(parent_target_from_block_row);
    page_history::record_page_snapshot_tx(&mut tx, page_id, "archive_page").await?;
    let root_child_page_block_visible = if archived {
        false
    } else {
        repair_page_parent_for_active_restore(&mut tx, page_id).await?
    };
    let result = sqlx::query(
        "UPDATE notes_pages
         SET archived = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND in_trash = 0",
    )
    .bind(if archived { 1_i64 } else { 0_i64 })
    .bind(page_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("archive notes page: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("notes page not found".to_string());
    }
    sqlx::query(
        "UPDATE notes_blocks
         SET in_trash = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND type = 'child_page'",
    )
    .bind(if root_child_page_block_visible {
        0_i64
    } else {
        1_i64
    })
    .bind(page_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("archive notes child page block: {e}"))?;
    if let Some(parent) = child_parent {
        refresh_parent_has_children(&mut tx, &parent).await?;
        touch_page(&mut tx, &parent.page_id).await?;
    }
    data_sources::rollups::invalidate_rollup_cache_for_page_tx(&mut tx, page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit archive notes page: {e}"))?;
    reads::get_page(pool, page_id, true).await
}

pub(super) async fn resolve_page_move_parent(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
    parent: &NoteParent,
) -> Result<Option<ParentTarget>, String> {
    match parent {
        NoteParent::Workspace { .. } => Ok(None),
        NoteParent::PageId {
            page_id: parent_page_id,
        } => {
            let parent_page_id = parent_page_id.trim();
            require_uuid(parent_page_id, "parent.page_id")?;
            if parent_page_id == page_id {
                return Err("page cannot be moved under itself".to_string());
            }
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
            )
            .bind(parent_page_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load moved page parent: {e}"))?;
            if exists.is_none() {
                return Err("parent page not found".to_string());
            }
            ensure_page_not_moved_under_descendant(tx, page_id, parent_page_id).await?;
            Ok(Some(ParentTarget {
                parent_type: "page_id",
                parent_page_id: Some(parent_page_id.to_string()),
                parent_block_id: None,
                parent_block_type: None,
                page_id: parent_page_id.to_string(),
            }))
        }
        NoteParent::BlockId { .. } => {
            Err("pages can only be moved to workspace or another page".to_string())
        }
        NoteParent::DataSourceId { .. } => {
            Err("pages can only be moved to workspace or another page".to_string())
        }
    }
}

pub(super) async fn ensure_page_not_moved_under_descendant(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
    target_page_id: &str,
) -> Result<(), String> {
    let is_descendant: Option<i64> = sqlx::query_scalar(
        "WITH RECURSIVE page_subtree(id) AS (
            SELECT id FROM notes_pages WHERE id = ?
            UNION ALL
            SELECT child.id
            FROM notes_pages AS child
            LEFT JOIN notes_blocks AS parent_block ON parent_block.id = child.parent_block_id
            JOIN page_subtree ON (
                child.parent_type = 'page_id'
                AND child.parent_page_id = page_subtree.id
            ) OR (
                child.parent_type = 'block_id'
                AND parent_block.page_id = page_subtree.id
            )
            WHERE child.in_trash = 0
         )
         SELECT 1 FROM page_subtree WHERE id = ? AND id <> ? LIMIT 1",
    )
    .bind(page_id)
    .bind(target_page_id)
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("check moved page descendants: {e}"))?;
    if is_descendant.is_some() {
        return Err("page cannot be moved under its descendant".to_string());
    }
    Ok(())
}

pub(super) async fn upsert_moved_child_page_block(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
    title: &str,
    parent: &ParentTarget,
    existing_block: Option<NoteBlockRow>,
) -> Result<(), String> {
    let sort_order = next_sort_orders(tx, parent, None, 1).await?[0];
    validate_sort_order(sort_order)?;
    let payload = child_page_payload(title);
    let plain_text = plain_text_from_payload("child_page", &payload);
    if let Some(block) = existing_block {
        if block.block_type != "child_page" {
            return Err("page block id is not a child_page block".to_string());
        }
        sqlx::query(
            "UPDATE notes_blocks
             SET page_id = ?,
                 parent_type = 'page_id',
                 parent_page_id = ?,
                 parent_block_id = NULL,
                 has_children = 0,
                 in_trash = 0,
                 type = 'child_page',
                 payload = ?,
                 plain_text = ?,
                 sort_order = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(&parent.page_id)
        .bind(&parent.parent_page_id)
        .bind(payload.to_string())
        .bind(plain_text)
        .bind(sort_order)
        .bind(page_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("move notes child page block: {e}"))?;
    } else {
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
             VALUES (?, ?, 'page_id', ?, NULL, 0, 'child_page', ?, ?, ?)",
        )
        .bind(page_id)
        .bind(&parent.page_id)
        .bind(&parent.parent_page_id)
        .bind(payload.to_string())
        .bind(plain_text)
        .bind(sort_order)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("create moved notes child page block: {e}"))?;
    }
    Ok(())
}
