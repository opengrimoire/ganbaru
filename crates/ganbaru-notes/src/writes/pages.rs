use super::parents::{
    refresh_parent_has_children, resolve_block_parent, touch_page, validate_page_parent_exists,
};
use super::payloads::{
    child_page_payload, default_text_payload, page_row_properties_for_title, page_title_properties,
};
use super::sort::next_sort_orders;
use crate::models::{
    NoteChildPageFromBlockCreate, NoteLoadedPage, NotePageCreate, NotePageDto, NotePageRow,
    NotePageUpdate, NoteParent, OptionalJsonValue, page_parent_columns, parent_columns,
};
use crate::validation::{
    plain_text_from_payload, require_uuid, validate_page_create, validate_page_update,
    validate_sort_order,
};
use crate::{assets, mention_notifications, page_history, project_history, reads};
use serde_json::Value;
use sqlx::{Row, SqlitePool};

pub async fn create_page(
    pool: &SqlitePool,
    page: NotePageCreate,
) -> Result<NoteLoadedPage, String> {
    validate_page_create(&page)?;
    if matches!(&page.parent, NoteParent::DataSourceId { .. }) {
        return Err("data source pages must be created with the row page command".to_string());
    }
    let title = page.title.trim().to_string();
    let properties = page_properties_for_create(&title, page.properties.as_ref())?;
    if let Some(existing) = load_idempotent_page_create(pool, &page, &title, &properties).await? {
        return Ok(existing);
    }
    if let Some(project_id) = properties
        .get("__ganbaru_project_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        project_history::ensure_project_baseline_for_mutation(pool, project_id).await?;
    } else {
        project_history::ensure_parent_baseline_for_mutation(pool, &page.parent).await?;
    }
    let folder_id = page.folder_id.as_deref().map(str::trim);
    let (parent_type, parent_page_id, parent_block_id) = parent_columns(&page.parent);
    let first_payload = default_text_payload("");
    let first_plain_text = plain_text_from_payload("paragraph", &first_payload);
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page create: {e}"))?;
    validate_page_parent_exists(&mut tx, &page.parent).await?;
    let child_page_parent = match &page.parent {
        NoteParent::Workspace { .. } => None,
        _ => Some(resolve_block_parent(&mut tx, &page.parent).await?),
    };
    let child_page_sort_order = if let Some(parent) = &child_page_parent {
        Some(next_sort_orders(&mut tx, parent, page.after_block_id.as_deref(), 1).await?[0])
    } else {
        None
    };
    if let Some(parent) = &child_page_parent {
        page_history::record_page_snapshot_tx(&mut tx, &parent.page_id, "create_child_page")
            .await?;
    }
    sqlx::query(
        "INSERT INTO notes_pages (
            id,
            parent_type,
            parent_page_id,
            parent_block_id,
            folder_id,
            title,
            properties
         )
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(page.id.trim())
    .bind(parent_type)
    .bind(parent_page_id)
    .bind(parent_block_id)
    .bind(folder_id)
    .bind(&title)
    .bind(properties.to_string())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create notes page: {e}"))?;
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
    .bind(page.first_block_id.trim())
    .bind(page.id.trim())
    .bind(page.id.trim())
    .bind(first_payload.to_string())
    .bind(first_plain_text)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create initial notes block: {e}"))?;
    crate::project_history::mark_page_dirty_tx(&mut tx, page.id.trim(), &title, false).await?;
    if let Some(parent) = child_page_parent {
        let child_payload = child_page_payload(&title);
        let child_plain_text = plain_text_from_payload("child_page", &child_payload);
        let sort_order = child_page_sort_order
            .ok_or_else(|| "child page sort order was not prepared".to_string())?;
        validate_sort_order(sort_order)?;
        sqlx::query(
            "INSERT INTO notes_blocks (
                id,
                page_id,
                parent_type,
                parent_page_id,
                parent_block_id,
                type,
                payload,
                plain_text,
                sort_order
             )
             VALUES (?, ?, ?, ?, ?, 'child_page', ?, ?, ?)",
        )
        .bind(page.id.trim())
        .bind(&parent.page_id)
        .bind(parent.parent_type)
        .bind(&parent.parent_page_id)
        .bind(&parent.parent_block_id)
        .bind(child_payload.to_string())
        .bind(child_plain_text)
        .bind(sort_order)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("create notes child page block: {e}"))?;
        refresh_parent_has_children(&mut tx, &parent).await?;
        touch_page(&mut tx, &parent.page_id).await?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page create: {e}"))?;
    reads::load_page(pool, page.id.trim()).await
}

async fn load_idempotent_page_create(
    pool: &SqlitePool,
    page: &NotePageCreate,
    title: &str,
    properties: &Value,
) -> Result<Option<NoteLoadedPage>, String> {
    let page_id = page.id.trim();
    let existing = sqlx::query(
        "SELECT parent_type, parent_page_id, parent_block_id, folder_id, title, properties
         FROM notes_pages WHERE id = ?",
    )
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("check existing notes page create: {e}"))?;
    let Some(existing) = existing else {
        let first_block_exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks WHERE id = ?")
                .bind(page.first_block_id.trim())
                .fetch_one(pool)
                .await
                .map_err(|e| format!("check existing initial notes block: {e}"))?;
        if first_block_exists != 0 {
            return Err("notes page create ids already belong to different data".to_string());
        }
        return Ok(None);
    };

    let (parent_type, parent_page_id, parent_block_id) = parent_columns(&page.parent);
    let stored_properties: String = existing
        .try_get("properties")
        .map_err(|e| format!("read existing notes page properties: {e}"))?;
    let stored_properties: Value = serde_json::from_str(&stored_properties)
        .map_err(|e| format!("parse existing notes page properties: {e}"))?;
    let stored_parent_type: String = existing
        .try_get("parent_type")
        .map_err(|e| format!("read existing notes page parent type: {e}"))?;
    let stored_parent_page_id: Option<String> = existing
        .try_get("parent_page_id")
        .map_err(|e| format!("read existing notes page parent page: {e}"))?;
    let stored_parent_block_id: Option<String> = existing
        .try_get("parent_block_id")
        .map_err(|e| format!("read existing notes page parent block: {e}"))?;
    let stored_folder_id: Option<String> = existing
        .try_get("folder_id")
        .map_err(|e| format!("read existing notes page folder: {e}"))?;
    let stored_title: String = existing
        .try_get("title")
        .map_err(|e| format!("read existing notes page title: {e}"))?;
    let page_matches = stored_parent_type == parent_type
        && stored_parent_page_id.as_deref() == parent_page_id
        && stored_parent_block_id.as_deref() == parent_block_id
        && stored_folder_id.as_deref() == page.folder_id.as_deref().map(str::trim)
        && stored_title == title
        && stored_properties == *properties;
    if !page_matches {
        return Err("notes page create id already belongs to different data".to_string());
    }

    let initial_payload = default_text_payload("");
    let initial = sqlx::query(
        "SELECT page_id, parent_type, parent_page_id, type, payload, sort_order
         FROM notes_blocks WHERE id = ?",
    )
    .bind(page.first_block_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("check existing initial notes block: {e}"))?;
    let Some(initial) = initial else {
        return Err("existing notes page is missing its initial block".to_string());
    };
    let stored_payload: String = initial
        .try_get("payload")
        .map_err(|e| format!("read existing initial notes block payload: {e}"))?;
    let stored_payload: Value = serde_json::from_str(&stored_payload)
        .map_err(|e| format!("parse existing initial notes block payload: {e}"))?;
    let stored_page_id: String = initial
        .try_get("page_id")
        .map_err(|e| format!("read existing initial block page: {e}"))?;
    let stored_parent_type: String = initial
        .try_get("parent_type")
        .map_err(|e| format!("read existing initial block parent type: {e}"))?;
    let stored_parent_page_id: Option<String> = initial
        .try_get("parent_page_id")
        .map_err(|e| format!("read existing initial block parent page: {e}"))?;
    let stored_type: String = initial
        .try_get("type")
        .map_err(|e| format!("read existing initial block type: {e}"))?;
    let stored_sort_order: f64 = initial
        .try_get("sort_order")
        .map_err(|e| format!("read existing initial block sort order: {e}"))?;
    let block_matches = stored_page_id == page_id
        && stored_parent_type == "page_id"
        && stored_parent_page_id.as_deref() == Some(page_id)
        && stored_type == "paragraph"
        && stored_sort_order == 1000.0
        && stored_payload == initial_payload;
    if !block_matches {
        return Err("initial notes block id already belongs to different data".to_string());
    }

    reads::load_page(pool, page_id).await.map(Some)
}

pub async fn create_child_page_from_block(
    pool: &SqlitePool,
    block_id: &str,
    request: NoteChildPageFromBlockCreate,
) -> Result<NoteLoadedPage, String> {
    let block_id = block_id.trim();
    require_uuid(block_id, "block_id")?;
    require_uuid(&request.first_block_id, "first_block_id")?;
    if block_id == request.first_block_id.trim() {
        return Err("first_block_id must not match block_id".to_string());
    }
    let current = reads::get_block_row(pool, block_id, false).await?;
    if current.block_type == "child_page" {
        return reads::load_page(pool, block_id).await;
    }
    project_history::ensure_page_baseline_for_mutation(pool, &current.page_id).await?;
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .unwrap_or_else(|| current.plain_text.trim())
        .to_string();
    let properties = page_properties_for_create(&title, request.properties.as_ref())?;
    let page_parent_type = if current.parent_type == "page_id" {
        "page_id"
    } else {
        "block_id"
    };
    let child_payload = child_page_payload(&title);
    let child_plain_text = plain_text_from_payload("child_page", &child_payload);
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes child page create: {e}"))?;
    let existing_page: Option<i64> = sqlx::query_scalar("SELECT 1 FROM notes_pages WHERE id = ?")
        .bind(block_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("check notes child page id: {e}"))?;
    if existing_page.is_some() {
        return Err("notes page already exists for block".to_string());
    }
    page_history::record_page_snapshot_tx(
        &mut tx,
        &current.page_id,
        "create_child_page_from_block",
    )
    .await?;
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
    .bind(block_id)
    .bind(page_parent_type)
    .bind(&current.parent_page_id)
    .bind(&current.parent_block_id)
    .bind(&title)
    .bind(properties.to_string())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create notes child page: {e}"))?;

    let child_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notes_blocks WHERE parent_block_id = ? AND in_trash = 0",
    )
    .bind(block_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| format!("count notes child page block children: {e}"))?;
    if child_count == 0 {
        let first_payload = default_text_payload("");
        let first_plain_text = plain_text_from_payload("paragraph", &first_payload);
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
        .bind(request.first_block_id.trim())
        .bind(block_id)
        .bind(block_id)
        .bind(first_payload.to_string())
        .bind(first_plain_text)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("create initial notes child page block: {e}"))?;
    } else {
        sqlx::query(
            "WITH RECURSIVE subtree(id) AS (
                SELECT id FROM notes_blocks WHERE parent_block_id = ? AND in_trash = 0
                UNION ALL
                SELECT child.id
                FROM notes_blocks AS child
                JOIN subtree ON child.parent_block_id = subtree.id
                WHERE child.in_trash = 0
             )
             UPDATE notes_blocks
             SET page_id = ?
             WHERE id IN (SELECT id FROM subtree)",
        )
        .bind(block_id)
        .bind(block_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("move notes child page descendant blocks: {e}"))?;
        sqlx::query(
            "UPDATE notes_blocks
             SET parent_type = 'page_id',
                 parent_page_id = ?,
                 parent_block_id = NULL
             WHERE parent_block_id = ? AND in_trash = 0",
        )
        .bind(block_id)
        .bind(block_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("move notes child page root blocks: {e}"))?;
    }

    sqlx::query(
        "UPDATE notes_blocks
         SET has_children = 0,
             type = 'child_page',
             payload = ?,
             plain_text = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND in_trash = 0",
    )
    .bind(child_payload.to_string())
    .bind(child_plain_text)
    .bind(block_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("convert notes block to child page: {e}"))?;
    touch_page(&mut tx, &current.page_id).await?;
    touch_page(&mut tx, block_id).await?;
    // Earlier block snapshots cannot replay a paragraph over the new page identity.
    sqlx::query("DELETE FROM notes_undo_state WHERE page_id = ?")
        .bind(&current.page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("clear notes history before child page conversion: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes child page create: {e}"))?;
    reads::load_page(pool, block_id).await
}

fn page_properties_for_create(
    title: &str,
    extra_properties: Option<&Value>,
) -> Result<Value, String> {
    let mut properties = page_title_properties(title);
    let Some(extra_properties) = extra_properties else {
        return Ok(properties);
    };
    let extra_object = extra_properties
        .as_object()
        .ok_or_else(|| "properties must be an object".to_string())?;
    let properties_object = properties
        .as_object_mut()
        .ok_or_else(|| "page properties must be an object".to_string())?;
    for (key, value) in extra_object {
        if key == "title" {
            continue;
        }
        properties_object.insert(key.clone(), value.clone());
    }
    Ok(properties)
}

pub async fn update_page(
    pool: &SqlitePool,
    page_id: &str,
    update: NotePageUpdate,
) -> Result<NotePageDto, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    validate_page_update(&update)?;
    project_history::ensure_page_baseline_for_mutation(pool, page_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page update: {e}"))?;
    page_history::record_page_snapshot_tx(&mut tx, page_id, "update_page").await?;
    let page_properties_changed = update.properties.is_some();
    let page_title_changed = update.title.is_some();
    if let Some(parent) = &update.parent {
        validate_page_parent_exists(&mut tx, parent).await?;
        let (parent_type, parent_page_id, parent_block_id, parent_data_source_id) =
            page_parent_columns(parent);
        sqlx::query(
            "UPDATE notes_pages
             SET parent_type = ?,
                 parent_page_id = ?,
                 parent_block_id = ?,
                 parent_data_source_id = ?,
                 folder_id = NULL,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0 AND archived = 0",
        )
        .bind(parent_type)
        .bind(parent_page_id)
        .bind(parent_block_id)
        .bind(parent_data_source_id)
        .bind(page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes page parent: {e}"))?;
    }
    if let Some(title) = update.title {
        let title = title.trim().to_string();
        let current_page = load_page_row(&mut tx, page_id).await?;
        let properties = match update.properties.clone() {
            Some(properties) => properties.to_string(),
            None => page_row_properties_for_title(&current_page, &title)?,
        };
        sqlx::query(
            "UPDATE notes_pages
             SET title = ?,
                 properties = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0 AND archived = 0",
        )
        .bind(&title)
        .bind(properties)
        .bind(page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes page title: {e}"))?;
        let child_payload = child_page_payload(&title);
        sqlx::query(
            "UPDATE notes_blocks
             SET payload = ?,
                 plain_text = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND type = 'child_page'",
        )
        .bind(child_payload.to_string())
        .bind(plain_text_from_payload("child_page", &child_payload))
        .bind(page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes child page block title: {e}"))?;
    } else if let Some(properties) = update.properties {
        sqlx::query(
            "UPDATE notes_pages
             SET properties = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0 AND archived = 0",
        )
        .bind(properties.to_string())
        .bind(page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes page properties: {e}"))?;
    }
    if update.icon.is_set() || update.cover.is_set() {
        let icon_reference_value = update.icon.value().cloned();
        let cover_reference_value = update.cover.value().cloned();
        let (icon_is_set, icon) = json_field_update(&update.icon);
        let (cover_is_set, cover) = json_field_update(&update.cover);
        sqlx::query(
            "UPDATE notes_pages
             SET icon = CASE WHEN ? THEN ? ELSE icon END,
                 cover = CASE WHEN ? THEN ? ELSE cover END,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0 AND archived = 0",
        )
        .bind(icon_is_set)
        .bind(icon)
        .bind(cover_is_set)
        .bind(cover)
        .bind(page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes page media: {e}"))?;
        assets::sync_page_asset_references_tx(
            &mut tx,
            page_id,
            icon_is_set != 0,
            icon_reference_value.as_ref(),
            cover_is_set != 0,
            cover_reference_value.as_ref(),
        )
        .await?;
    }
    if page_properties_changed || page_title_changed {
        mention_notifications::sync_page_properties_tx(&mut tx, page_id).await?;
    }
    if page_properties_changed {
        let updated_page = load_page_row(&mut tx, page_id).await?;
        if updated_page.parent_type == "data_source_id" {
            if let Some(data_source_id) = updated_page.parent_data_source_id.as_deref() {
                assets::sync_current_data_source_property_asset_references_tx(
                    &mut tx,
                    data_source_id,
                )
                .await?;
            }
        }
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page update: {e}"))?;
    reads::get_page(pool, page_id, false).await
}

pub(super) fn json_field_update(value: &OptionalJsonValue) -> (i64, Option<String>) {
    if value.is_set() {
        (1, value.storage_value())
    } else {
        (0, None)
    }
}

pub(super) async fn load_page_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
) -> Result<NotePageRow, String> {
    sqlx::query_as::<_, NotePageRow>(
        "SELECT * FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes page row: {e}"))?
    .ok_or_else(|| "notes page not found".to_string())
}
