use crate::local_user;
use crate::models::{
    NoteBlockDto, NoteBlockRow, NoteLoadedPage, NotePageDto, NotePageHistoryCopyBlocks,
    NotePageHistorySettingsDto, NotePageHistorySettingsRow, NotePageHistorySettingsUpdate,
    NotePageHistorySnapshotDto, NotePageHistorySnapshotRow, NotePageRow, NotePaginatedBlockList,
};
use crate::validation::{
    plain_text_from_payload, require_uuid, validate_block_payload, validate_sort_order,
};
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::{HashMap, HashSet};

const DEFAULT_BLOCK_SORT_STEP: f64 = 1000.0;
const EDIT_SESSION_SNAPSHOT_WINDOW_MINUTES: i64 = 5;

enum SnapshotInsertMode {
    PreserveAvailableIds,
    CopyWithFreshIds,
}

pub async fn load_page_history_settings(
    pool: &SqlitePool,
) -> Result<NotePageHistorySettingsDto, String> {
    ensure_settings_row(pool).await?;
    let row = sqlx::query_as::<_, NotePageHistorySettingsRow>(
        "SELECT retention_days, updated_at
         FROM notes_page_history_settings
         WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load notes page history settings: {e}"))?;
    Ok(NotePageHistorySettingsDto::new(row))
}

pub async fn update_page_history_settings(
    pool: &SqlitePool,
    update: NotePageHistorySettingsUpdate,
) -> Result<NotePageHistorySettingsDto, String> {
    validate_retention_days(update.retention_days)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page history settings update: {e}"))?;
    ensure_settings_row_tx(&mut tx).await?;
    sqlx::query(
        "UPDATE notes_page_history_settings
         SET retention_days = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = 1",
    )
    .bind(update.retention_days)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update notes page history settings: {e}"))?;
    cleanup_history_retention_tx(&mut tx).await?;
    let inherited_project_ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM projects WHERE notes_history_retention_days IS NULL ORDER BY id",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| format!("list projects using global Notes history retention: {e}"))?;
    for project_id in inherited_project_ids {
        crate::project_history::prune_project_history_tx(&mut tx, &project_id).await?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page history settings update: {e}"))?;
    load_page_history_settings(pool).await
}

pub async fn list_page_history_snapshots(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<Vec<NotePageHistorySnapshotDto>, String> {
    let page_id = normalize_uuid(page_id, "page_id")?;
    let rows = sqlx::query_as::<_, NotePageHistorySnapshotRow>(
        "SELECT *
         FROM notes_page_history_snapshots
         WHERE page_id = ?
         ORDER BY created_time DESC, id DESC",
    )
    .bind(&page_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes page history: {e}"))?;
    rows.into_iter()
        .map(NotePageHistorySnapshotDto::new)
        .collect()
}

pub async fn load_page_history_snapshot(
    pool: &SqlitePool,
    page_id: &str,
    snapshot_id: &str,
) -> Result<NoteLoadedPage, String> {
    let page_id = normalize_uuid(page_id, "page_id")?;
    let snapshot_id = normalize_uuid(snapshot_id, "snapshot_id")?;
    let row = load_snapshot_row(pool, &page_id, &snapshot_id).await?;
    loaded_page_from_snapshot(row)
}

pub async fn restore_page_history_snapshot(
    pool: &SqlitePool,
    page_id: &str,
    snapshot_id: &str,
) -> Result<NoteLoadedPage, String> {
    let page_id = normalize_uuid(page_id, "page_id")?;
    let snapshot_id = normalize_uuid(snapshot_id, "snapshot_id")?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page history restore: {e}"))?;
    ensure_active_page_exists_tx(&mut tx, &page_id).await?;
    let snapshot = load_snapshot_row_tx(&mut tx, &page_id, &snapshot_id).await?;
    record_page_snapshot_tx(&mut tx, &page_id, "restore").await?;
    replace_page_from_snapshot(&mut tx, &page_id, &snapshot).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page history restore: {e}"))?;
    crate::reads::load_page(pool, &page_id).await
}

pub async fn copy_page_history_blocks(
    pool: &SqlitePool,
    page_id: &str,
    snapshot_id: &str,
    request: NotePageHistoryCopyBlocks,
) -> Result<NotePaginatedBlockList, String> {
    let page_id = normalize_uuid(page_id, "page_id")?;
    let snapshot_id = normalize_uuid(snapshot_id, "snapshot_id")?;
    if let Some(after_block_id) = request.after_block_id.as_deref() {
        require_uuid(after_block_id.trim(), "after_block_id")?;
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page history block copy: {e}"))?;
    ensure_active_page_exists_tx(&mut tx, &page_id).await?;
    let snapshot = load_snapshot_row_tx(&mut tx, &page_id, &snapshot_id).await?;
    let blocks = parse_snapshot_blocks(&snapshot)?;
    record_page_snapshot_tx(&mut tx, &page_id, "copy_history_blocks").await?;
    let copied_root_ids = insert_snapshot_blocks(
        &mut tx,
        &page_id,
        &blocks,
        SnapshotInsertMode::CopyWithFreshIds,
        request.after_block_id.as_deref(),
    )
    .await?;
    touch_page(&mut tx, &page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page history block copy: {e}"))?;
    load_blocks_by_ids(pool, copied_root_ids).await
}

pub async fn record_page_snapshot_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    reason: &str,
) -> Result<Option<String>, String> {
    let page_id = normalize_uuid(page_id, "page_id")?;
    let reason = normalize_reason(reason)?;
    ensure_settings_row_tx(tx).await?;
    if !crate::project_history::page_history_enabled_tx(tx, &page_id).await? {
        return Ok(None);
    }
    let page = load_page_row_tx(tx, &page_id).await?;
    let force_checkpoint = matches!(
        reason.as_str(),
        "trash_page"
            | "move_page"
            | "restore"
            | "archive_page"
            | "apply_page_template"
            | "copy_history_blocks"
    );
    crate::project_history::mark_page_dirty_tx(tx, &page_id, &page.title, force_checkpoint).await?;
    let blocks = load_page_block_subtree_rows(tx, &page_id).await?;
    let blocks_payload = serialize_snapshot_blocks(&blocks)?;
    if latest_snapshot_matches(tx, &page, &blocks_payload).await? {
        return Ok(None);
    }
    if recent_edit_session_snapshot_exists(tx, &page.id, &reason).await? {
        return Ok(None);
    }
    let local_user = local_user::current_local_user_tx(tx).await?;
    let snapshot_id = new_note_id(tx, &mut HashSet::new()).await?;
    let block_bundle_hash =
        crate::project_history::store_page_history_blocks_tx(tx, &blocks_payload).await?;
    sqlx::query(
        "INSERT INTO notes_page_history_snapshots (
            id,
            page_id,
            parent_type,
            parent_page_id,
            parent_block_id,
            parent_data_source_id,
            folder_id,
            title,
            properties,
            icon,
            cover,
            in_trash,
            archived,
            blocks,
            block_bundle_hash,
            block_count,
            reason,
            created_by,
            page_created_time,
            page_last_edited_time
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&snapshot_id)
    .bind(&page.id)
    .bind(&page.parent_type)
    .bind(&page.parent_page_id)
    .bind(&page.parent_block_id)
    .bind(&page.parent_data_source_id)
    .bind(&page.folder_id)
    .bind(&page.title)
    .bind(&page.properties)
    .bind(&page.icon)
    .bind(&page.cover)
    .bind(page.in_trash)
    .bind(page.archived)
    .bind("[]")
    .bind(&block_bundle_hash)
    .bind(blocks.len() as i64)
    .bind(&reason)
    .bind(&local_user.id)
    .bind(&page.created_time)
    .bind(&page.last_edited_time)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("record notes page history snapshot: {e}"))?;
    cleanup_history_retention_tx(tx).await?;
    Ok(Some(snapshot_id))
}

async fn replace_page_from_snapshot(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    snapshot: &NotePageHistorySnapshotRow,
) -> Result<(), String> {
    let blocks = parse_snapshot_blocks(snapshot)?;
    let owns_database: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM notes_databases AS database
        JOIN notes_blocks AS block ON block.id = database.id WHERE block.page_id = ?)",
    )
    .bind(page_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("check database ownership before page history restore: {e}"))?;
    let snapshot_has_database = blocks
        .iter()
        .filter(|block| block.block_type == "child_database")
        .map(crate::writes::database_copy::has_database_graph)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .any(|has_graph| has_graph);
    if owns_database || snapshot_has_database {
        return Err("This page contains database content. Restore a project version to recover its complete database graph.".to_string());
    }
    trash_current_child_pages(tx, page_id).await?;
    sqlx::query("DELETE FROM notes_blocks WHERE page_id = ?")
        .bind(page_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear notes page before history restore: {e}"))?;
    sqlx::query(
        "UPDATE notes_pages
         SET title = ?,
             properties = ?,
             icon = ?,
             cover = ?,
             in_trash = 0,
             trashed_time = NULL,
             archived = 0,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(&snapshot.title)
    .bind(&snapshot.properties)
    .bind(&snapshot.icon)
    .bind(&snapshot.cover)
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("restore notes page history metadata: {e}"))?;
    insert_snapshot_blocks(
        tx,
        page_id,
        &blocks,
        SnapshotInsertMode::PreserveAvailableIds,
        None,
    )
    .await?;
    if blocks.is_empty() {
        insert_default_page_block(tx, page_id, &mut HashSet::new()).await?;
    }
    touch_page(tx, page_id).await
}

async fn insert_snapshot_blocks(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    blocks: &[NoteBlockRow],
    mode: SnapshotInsertMode,
    after_block_id: Option<&str>,
) -> Result<Vec<String>, String> {
    if blocks.is_empty() {
        return Ok(Vec::new());
    }
    let root_blocks = blocks
        .iter()
        .filter(|block| block.parent_type == "page_id")
        .collect::<Vec<_>>();
    let root_sort_orders =
        next_page_root_sort_orders(tx, page_id, after_block_id, root_blocks.len()).await?;
    let mut root_sort_order_by_source = root_blocks
        .iter()
        .zip(root_sort_orders)
        .map(|(block, sort_order)| (block.id.as_str(), sort_order))
        .collect::<HashMap<_, _>>();
    let mut reserved_ids = HashSet::new();
    let mut id_map = HashMap::with_capacity(blocks.len());
    for block in blocks {
        let next_id = match mode {
            SnapshotInsertMode::CopyWithFreshIds => new_note_id(tx, &mut reserved_ids).await?,
            SnapshotInsertMode::PreserveAvailableIds => {
                if block_id_exists(tx, &block.id).await? {
                    new_note_id(tx, &mut reserved_ids).await?
                } else {
                    block.id.clone()
                }
            }
        };
        reserved_ids.insert(next_id.clone());
        id_map.insert(block.id.clone(), next_id);
    }

    let mut database_copies = Vec::new();
    let mut budget = crate::writes::copy_budget::CopyBudget::default();
    let mut copy_context = crate::writes::copy_budget::CopyContext {
        reserved_ids: &mut reserved_ids,
        budget: &mut budget,
    };
    if matches!(mode, SnapshotInsertMode::CopyWithFreshIds) {
        let project_id =
            crate::project_history::resolve_project_id_for_page_tx(tx, page_id).await?;
        for block in blocks
            .iter()
            .filter(|block| block.block_type == "child_database")
        {
            if crate::writes::database_copy::has_database_graph(block)? {
                database_copies.push(
                    crate::writes::database_copy::plan_database_copy(
                        tx,
                        block,
                        &id_map[&block.id],
                        &mut copy_context,
                        project_id.as_deref(),
                        false,
                    )
                    .await?,
                );
            }
        }
    }

    let mut root_ids = Vec::with_capacity(root_blocks.len());
    for block in blocks {
        let block_id = id_map
            .get(&block.id)
            .cloned()
            .ok_or_else(|| "snapshot block id mapping is missing".to_string())?;
        let (parent_type, parent_page_id, parent_block_id) = if block.parent_type == "page_id" {
            ("page_id", Some(page_id.to_string()), None)
        } else {
            let source_parent_id = block
                .parent_block_id
                .as_ref()
                .ok_or_else(|| "snapshot child block is missing its parent".to_string())?;
            let parent_block_id = id_map
                .get(source_parent_id)
                .cloned()
                .ok_or_else(|| "snapshot child block parent was not restored".to_string())?;
            ("block_id", None, Some(parent_block_id))
        };
        let sort_order = if block.parent_type == "page_id" {
            root_sort_order_by_source
                .remove(block.id.as_str())
                .ok_or_else(|| "snapshot root sort order is missing".to_string())?
        } else {
            block.sort_order
        };
        validate_sort_order(sort_order)?;
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
                sort_order,
                source_provider,
                source_object_id,
                source_last_edited_time,
                created_time,
                last_edited_time
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&block_id)
        .bind(page_id)
        .bind(parent_type)
        .bind(parent_page_id.as_deref())
        .bind(parent_block_id.as_deref())
        .bind(block.has_children)
        .bind(&block.block_type)
        .bind(&block.payload)
        .bind(&block.plain_text)
        .bind(sort_order)
        .bind(&block.source_provider)
        .bind(&block.source_object_id)
        .bind(&block.source_last_edited_time)
        .bind(&block.created_time)
        .bind(&block.last_edited_time)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert notes page history block: {e}"))?;
        if block.parent_type == "page_id" {
            root_ids.push(block_id.clone());
        }
        if block.block_type == "child_page" {
            upsert_snapshot_child_page(
                tx,
                &block_id,
                page_id,
                parent_type,
                parent_block_id,
                block,
                &mut reserved_ids,
            )
            .await?;
        }
    }
    for database_copy in &database_copies {
        sqlx::query("UPDATE notes_blocks SET payload = ? WHERE id = ?")
            .bind(database_copy.payload.to_string())
            .bind(&database_copy.id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("set copied history database references: {e}"))?;
        crate::writes::database_copy::insert_database_copy(tx, database_copy).await?;
    }
    crate::writes::database_copy::finalize_copies(tx, &database_copies, &[], &id_map).await?;
    Ok(root_ids)
}

async fn upsert_snapshot_child_page(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    containing_page_id: &str,
    block_parent_type: &str,
    block_parent_block_id: Option<String>,
    block: &NoteBlockRow,
    reserved_ids: &mut HashSet<String>,
) -> Result<(), String> {
    let payload: Value = serde_json::from_str(&block.payload)
        .map_err(|e| format!("parse history child page payload: {e}"))?;
    let title = payload
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let page_parent_type = if block_parent_type == "page_id" {
        "page_id"
    } else {
        "block_id"
    };
    let parent_page_id = if page_parent_type == "page_id" {
        Some(containing_page_id)
    } else {
        None
    };
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM notes_pages WHERE id = ?")
        .bind(page_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load history child page: {e}"))?;
    if exists.is_some() {
        sqlx::query(
            "UPDATE notes_pages
             SET parent_type = ?,
                 parent_page_id = ?,
                 parent_block_id = ?,
                 parent_data_source_id = NULL,
                 title = ?,
                 properties = ?,
                 in_trash = 0,
                 trashed_time = NULL,
                 archived = 0,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(page_parent_type)
        .bind(parent_page_id)
        .bind(block_parent_block_id.as_deref())
        .bind(&title)
        .bind(page_title_properties(&title).to_string())
        .bind(page_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("restore history child page: {e}"))?;
    } else {
        sqlx::query(
            "INSERT INTO notes_pages (
                id,
                parent_type,
                parent_page_id,
                parent_block_id,
                title,
                properties
             )
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(page_id)
        .bind(page_parent_type)
        .bind(parent_page_id)
        .bind(block_parent_block_id.as_deref())
        .bind(&title)
        .bind(page_title_properties(&title).to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("create history child page: {e}"))?;
    }
    let body_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks WHERE page_id = ?")
        .bind(page_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("count history child page body blocks: {e}"))?;
    if body_count == 0 {
        insert_default_page_block(tx, page_id, reserved_ids).await?;
    }
    Ok(())
}

async fn load_snapshot_row(
    pool: &SqlitePool,
    page_id: &str,
    snapshot_id: &str,
) -> Result<NotePageHistorySnapshotRow, String> {
    let mut row = sqlx::query_as::<_, NotePageHistorySnapshotRow>(
        "SELECT *
         FROM notes_page_history_snapshots
         WHERE id = ? AND page_id = ?",
    )
    .bind(snapshot_id)
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes page history snapshot: {e}"))?
    .ok_or_else(|| "notes page history snapshot not found".to_string())?;
    if let Some(hash) = row.block_bundle_hash.as_deref() {
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| format!("begin Notes page history bundle load: {e}"))?;
        row.blocks = crate::project_history::load_page_history_blocks_tx(&mut tx, hash).await?;
        tx.commit()
            .await
            .map_err(|e| format!("commit Notes page history bundle load: {e}"))?;
    }
    Ok(row)
}

async fn load_snapshot_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    snapshot_id: &str,
) -> Result<NotePageHistorySnapshotRow, String> {
    let mut row = sqlx::query_as::<_, NotePageHistorySnapshotRow>(
        "SELECT *
         FROM notes_page_history_snapshots
         WHERE id = ? AND page_id = ?",
    )
    .bind(snapshot_id)
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes page history snapshot: {e}"))?
    .ok_or_else(|| "notes page history snapshot not found".to_string())?;
    if let Some(hash) = row.block_bundle_hash.as_deref() {
        row.blocks = crate::project_history::load_page_history_blocks_tx(tx, hash).await?;
    }
    Ok(row)
}

async fn load_page_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<NotePageRow, String> {
    sqlx::query_as::<_, NotePageRow>("SELECT * FROM notes_pages WHERE id = ?")
        .bind(page_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load notes page for history: {e}"))?
        .ok_or_else(|| "notes page not found".to_string())
}

async fn ensure_active_page_exists_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load active notes page: {e}"))?;
    exists
        .map(|_| ())
        .ok_or_else(|| "notes page not found".to_string())
}

async fn load_page_block_subtree_rows(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<Vec<NoteBlockRow>, String> {
    sqlx::query_as::<_, NoteBlockRow>(
        "WITH RECURSIVE subtree(id, path) AS (
            SELECT id, printf('%020.6f:%s', sort_order, id)
            FROM notes_blocks
            WHERE parent_type = 'page_id' AND parent_page_id = ? AND in_trash = 0
            UNION ALL
            SELECT child.id, subtree.path || '/' || printf('%020.6f:%s', child.sort_order, child.id)
            FROM notes_blocks AS child
            JOIN subtree ON child.parent_block_id = subtree.id
            WHERE child.in_trash = 0
         )
         SELECT
            notes_blocks.id,
            notes_blocks.page_id,
            notes_blocks.parent_type,
            notes_blocks.parent_page_id,
            notes_blocks.parent_block_id,
            notes_blocks.has_children,
            notes_blocks.in_trash,
            notes_blocks.type AS block_type,
            notes_blocks.payload,
            notes_blocks.plain_text,
            notes_blocks.sort_order,
            notes_blocks.source_provider,
            notes_blocks.source_object_id,
            notes_blocks.source_last_edited_time,
            notes_blocks.created_time,
            notes_blocks.last_edited_time
         FROM notes_blocks
         JOIN subtree ON subtree.id = notes_blocks.id
         ORDER BY subtree.path ASC",
    )
    .bind(page_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load notes page blocks for history: {e}"))
}

fn serialize_snapshot_blocks(blocks: &[NoteBlockRow]) -> Result<String, String> {
    for block in blocks {
        validate_sort_order(block.sort_order)?;
        let payload: Value = serde_json::from_str(&block.payload)
            .map_err(|e| format!("parse history block payload: {e}"))?;
        validate_block_payload(&block.block_type, &payload)?;
    }
    serde_json::to_string(blocks).map_err(|e| format!("serialize notes page history blocks: {e}"))
}

fn parse_snapshot_blocks(
    snapshot: &NotePageHistorySnapshotRow,
) -> Result<Vec<NoteBlockRow>, String> {
    let blocks: Vec<NoteBlockRow> = serde_json::from_str(&snapshot.blocks)
        .map_err(|e| format!("parse notes page history blocks: {e}"))?;
    for block in &blocks {
        validate_sort_order(block.sort_order)?;
        let payload: Value = serde_json::from_str(&block.payload)
            .map_err(|e| format!("parse notes page history block payload: {e}"))?;
        validate_block_payload(&block.block_type, &payload)?;
    }
    Ok(blocks)
}

async fn latest_snapshot_matches(
    tx: &mut Transaction<'_, Sqlite>,
    page: &NotePageRow,
    blocks_payload: &str,
) -> Result<bool, String> {
    let latest = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<String>,
            i64,
            i64,
            String,
            Option<String>,
        ),
    >(
        "SELECT parent_type,
                parent_page_id,
                parent_block_id,
                parent_data_source_id,
                folder_id,
                title,
                properties,
                icon,
                cover,
                in_trash,
                archived,
                blocks,
                block_bundle_hash
         FROM notes_page_history_snapshots
         WHERE page_id = ?
         ORDER BY created_time DESC, id DESC
         LIMIT 1",
    )
    .bind(&page.id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load latest notes page history snapshot: {e}"))?;
    Ok(latest.is_some_and(
        |(
            parent_type,
            parent_page_id,
            parent_block_id,
            parent_data_source_id,
            folder_id,
            title,
            properties,
            icon,
            cover,
            in_trash,
            archived,
            blocks,
            block_bundle_hash,
        )| {
            parent_type == page.parent_type
                && parent_page_id == page.parent_page_id
                && parent_block_id == page.parent_block_id
                && parent_data_source_id == page.parent_data_source_id
                && folder_id == page.folder_id
                && title == page.title
                && properties == page.properties
                && icon == page.icon
                && cover == page.cover
                && in_trash == page.in_trash
                && archived == page.archived
                && block_bundle_hash.as_deref().map_or_else(
                    || blocks == blocks_payload,
                    |hash| hash == crate::project_history::page_history_blocks_hash(blocks_payload),
                )
        },
    ))
}

fn is_edit_session_snapshot_reason(reason: &str) -> bool {
    matches!(
        reason,
        "append_block_children" | "update_block" | "trash_block" | "trash_blocks"
    )
}

async fn recent_edit_session_snapshot_exists(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    reason: &str,
) -> Result<bool, String> {
    if !is_edit_session_snapshot_reason(reason) {
        return Ok(false);
    }
    let modifier = format!("-{EDIT_SESSION_SNAPSHOT_WINDOW_MINUTES} minutes");
    let latest_reason: Option<String> = sqlx::query_scalar(
        "SELECT reason
         FROM notes_page_history_snapshots
         WHERE page_id = ?
           AND created_time >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)
         ORDER BY created_time DESC, id DESC
         LIMIT 1",
    )
    .bind(page_id)
    .bind(modifier)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load recent notes edit history snapshot: {e}"))?;
    Ok(latest_reason.is_some_and(|latest| is_edit_session_snapshot_reason(&latest)))
}

fn loaded_page_from_snapshot(
    snapshot: NotePageHistorySnapshotRow,
) -> Result<NoteLoadedPage, String> {
    let blocks = parse_snapshot_blocks(&snapshot)?;
    let page = NotePageDto::new(NotePageRow {
        id: snapshot.page_id.clone(),
        parent_type: snapshot.parent_type,
        parent_page_id: snapshot.parent_page_id,
        parent_block_id: snapshot.parent_block_id,
        parent_data_source_id: snapshot.parent_data_source_id,
        folder_id: snapshot.folder_id,
        title: snapshot.title,
        properties: snapshot.properties,
        icon: snapshot.icon,
        cover: snapshot.cover,
        in_trash: snapshot.in_trash,
        archived: snapshot.archived,
        source_provider: None,
        source_object_id: None,
        source_workspace_id: None,
        source_last_edited_time: None,
        url: None,
        public_url: None,
        created_time: snapshot.page_created_time,
        last_edited_time: snapshot.page_last_edited_time,
    })?;
    let block_dtos = blocks
        .into_iter()
        .map(NoteBlockDto::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NoteLoadedPage::new(
        page,
        NotePaginatedBlockList::new(block_dtos, None, false),
    ))
}

async fn trash_current_child_pages(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_pages
         SET in_trash = 1,
             archived = 0,
             trashed_time = COALESCE(trashed_time, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id IN (
             SELECT id FROM notes_blocks
             WHERE page_id = ? AND type = 'child_page' AND in_trash = 0
         )",
    )
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("trash current child pages before history restore: {e}"))?;
    Ok(())
}

async fn next_page_root_sort_orders(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    after_block_id: Option<&str>,
    count: usize,
) -> Result<Vec<f64>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if let Some(after_id) = after_block_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        require_uuid(after_id, "after_block_id")?;
        let after_order: f64 = sqlx::query_scalar(
            "SELECT sort_order
             FROM notes_blocks
             WHERE id = ?
               AND parent_type = 'page_id'
               AND parent_page_id = ?
               AND page_id = ?
               AND in_trash = 0",
        )
        .bind(after_id)
        .bind(page_id)
        .bind(page_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load notes history copy anchor: {e}"))?
        .ok_or_else(|| "after block not found".to_string())?;
        let next_order: Option<f64> = sqlx::query_scalar(
            "SELECT sort_order
             FROM notes_blocks
             WHERE parent_type = 'page_id'
               AND parent_page_id = ?
               AND in_trash = 0
               AND sort_order > ?
             ORDER BY sort_order ASC, id ASC
             LIMIT 1",
        )
        .bind(page_id)
        .bind(after_order)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load next notes history copy sibling: {e}"))?;
        let step = match next_order {
            Some(next_order) if next_order > after_order => {
                (next_order - after_order) / (count as f64 + 1.0)
            }
            _ => DEFAULT_BLOCK_SORT_STEP,
        };
        return Ok((1..=count)
            .map(|index| after_order + step * index as f64)
            .collect());
    }
    let max_order: Option<f64> = sqlx::query_scalar(
        "SELECT MAX(sort_order)
         FROM notes_blocks
         WHERE parent_type = 'page_id'
           AND parent_page_id = ?
           AND in_trash = 0",
    )
    .bind(page_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load notes history root sort order: {e}"))?;
    let base = max_order.unwrap_or(0.0);
    Ok((1..=count)
        .map(|index| base + DEFAULT_BLOCK_SORT_STEP * index as f64)
        .collect())
}

async fn block_id_exists(tx: &mut Transaction<'_, Sqlite>, block_id: &str) -> Result<bool, String> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM notes_blocks WHERE id = ?")
        .bind(block_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("check notes history block id: {e}"))?;
    Ok(exists.is_some())
}

async fn new_note_id(
    tx: &mut Transaction<'_, Sqlite>,
    reserved_ids: &mut HashSet<String>,
) -> Result<String, String> {
    for _ in 0..32 {
        let id: String = sqlx::query_scalar(
            "SELECT lower(hex(randomblob(4))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(6)))",
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("generate notes history id: {e}"))?;
        require_uuid(&id, "generated_id")?;
        if reserved_ids.contains(&id) {
            continue;
        }
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1
             WHERE EXISTS (SELECT 1 FROM notes_pages WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_blocks WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_comment_threads WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_comments WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_page_templates WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_page_history_snapshots WHERE id = ?)",
        )
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("check generated notes history id: {e}"))?;
        if exists.is_none() {
            reserved_ids.insert(id.clone());
            return Ok(id);
        }
    }
    Err("could not generate a unique notes id".to_string())
}

async fn insert_default_page_block(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    reserved_ids: &mut HashSet<String>,
) -> Result<(), String> {
    let block_id = new_note_id(tx, reserved_ids).await?;
    let payload = default_text_payload("");
    sqlx::query(
        "INSERT INTO notes_blocks (
            id,
            page_id,
            parent_type,
            parent_page_id,
            type,
            payload,
            plain_text,
            sort_order
         )
         VALUES (?, ?, 'page_id', ?, 'paragraph', ?, ?, 1000)",
    )
    .bind(block_id)
    .bind(page_id)
    .bind(page_id)
    .bind(payload.to_string())
    .bind(plain_text_from_payload("paragraph", &payload))
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("create notes history fallback block: {e}"))?;
    Ok(())
}

async fn load_blocks_by_ids(
    pool: &SqlitePool,
    ids: Vec<String>,
) -> Result<NotePaginatedBlockList, String> {
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        rows.push(crate::reads::get_block_row(pool, &id, false).await?);
    }
    let block_dtos = rows
        .into_iter()
        .map(NoteBlockDto::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NotePaginatedBlockList::new(block_dtos, None, false))
}

async fn ensure_settings_row(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        "INSERT OR IGNORE INTO notes_page_history_settings (id, retention_days)
         VALUES (1, 30)",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("ensure notes page history settings: {e}"))?;
    Ok(())
}

async fn ensure_settings_row_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<(), String> {
    sqlx::query(
        "INSERT OR IGNORE INTO notes_page_history_settings (id, retention_days)
         VALUES (1, 30)",
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("ensure notes page history settings: {e}"))?;
    Ok(())
}

pub async fn cleanup_history_retention_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<(), String> {
    let retention_days: Option<i64> =
        sqlx::query_scalar("SELECT retention_days FROM notes_page_history_settings WHERE id = 1")
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("load notes page history retention: {e}"))?;
    let retention_days = retention_days.unwrap_or(30);
    validate_retention_days(Some(retention_days))?;
    sqlx::query(
        "WITH RECURSIVE page_ancestors(snapshot_id, page_id, parent_page_id, properties) AS (
             SELECT snapshot.id,
                    page.id,
                    COALESCE(
                        page.parent_page_id,
                        (SELECT block.page_id FROM notes_blocks AS block
                         WHERE block.id = page.parent_block_id),
                        (SELECT COALESCE(database.parent_page_id, database_block.page_id)
                         FROM notes_data_sources AS data_source
                         JOIN notes_databases AS database ON database.id = data_source.database_id
                         LEFT JOIN notes_blocks AS database_block ON database_block.id = database.id
                         WHERE data_source.id = page.parent_data_source_id)
                    ),
                    page.properties
             FROM notes_page_history_snapshots AS snapshot
             JOIN notes_pages AS page ON page.id = snapshot.page_id
             UNION ALL
             SELECT child.snapshot_id,
                    parent.id,
                    COALESCE(
                        parent.parent_page_id,
                        (SELECT block.page_id FROM notes_blocks AS block
                         WHERE block.id = parent.parent_block_id),
                        (SELECT COALESCE(database.parent_page_id, database_block.page_id)
                         FROM notes_data_sources AS data_source
                         JOIN notes_databases AS database ON database.id = data_source.database_id
                         LEFT JOIN notes_blocks AS database_block ON database_block.id = database.id
                         WHERE data_source.id = parent.parent_data_source_id)
                    ),
                    parent.properties
             FROM page_ancestors AS child
             JOIN notes_pages AS parent ON parent.id = child.parent_page_id
         ),
         snapshot_retention(snapshot_id, retention_days) AS (
             SELECT snapshot.id,
                    COALESCE(MAX(project.notes_history_retention_days), ?)
             FROM notes_page_history_snapshots AS snapshot
             LEFT JOIN page_ancestors AS ancestor ON ancestor.snapshot_id = snapshot.id
             LEFT JOIN projects AS project
               ON project.id = trim(json_extract(ancestor.properties, '$.__ganbaru_project_id'))
             GROUP BY snapshot.id
         )
         DELETE FROM notes_page_history_snapshots
         WHERE id IN (
             SELECT snapshot.id
             FROM notes_page_history_snapshots AS snapshot
             JOIN snapshot_retention AS retention ON retention.snapshot_id = snapshot.id
             WHERE retention.retention_days = 0
                OR snapshot.created_time < strftime(
                    '%Y-%m-%dT%H:%M:%fZ',
                    'now',
                    '-' || retention.retention_days || ' days'
                )
         )",
    )
    .bind(retention_days)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("prune notes page history: {e}"))?;
    crate::project_history::garbage_collect_history_storage_tx(tx).await?;
    Ok(())
}

fn validate_retention_days(retention_days: Option<i64>) -> Result<(), String> {
    let Some(days) = retention_days else {
        return Err("retention_days must be 0, 7, 30, 90, 180, or 365".to_string());
    };
    if ![0, 7, 30, 90, 180, 365].contains(&days) {
        return Err("retention_days must be 0, 7, 30, 90, 180, or 365".to_string());
    }
    Ok(())
}

fn normalize_uuid(value: &str, field: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    require_uuid(&value, field)?;
    Ok(value)
}

fn normalize_reason(value: &str) -> Result<String, String> {
    let reason = value.trim();
    if reason.is_empty() {
        return Err("history reason must not be empty".to_string());
    }
    if reason.chars().any(char::is_control) {
        return Err("history reason must not contain control characters".to_string());
    }
    if reason.chars().count() > 80 {
        return Err("history reason must be 80 characters or fewer".to_string());
    }
    Ok(reason.to_string())
}

async fn touch_page(tx: &mut Transaction<'_, Sqlite>, page_id: &str) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_pages
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch notes page after history write: {e}"))?;
    Ok(())
}

fn page_title_properties(title: &str) -> Value {
    json!({
        "title": {
            "id": "title",
            "type": "title",
            "title": [rich_text(title)]
        }
    })
}

fn default_text_payload(text: &str) -> Value {
    json!({
        "rich_text": [rich_text(text)],
        "color": "default"
    })
}

fn rich_text(text: &str) -> Value {
    json!({
        "type": "text",
        "text": {
            "content": text,
            "link": null
        },
        "annotations": {
            "bold": false,
            "italic": false,
            "strikethrough": false,
            "underline": false,
            "code": false,
            "color": "default"
        },
        "plain_text": text,
        "href": null
    })
}
