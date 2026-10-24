use super::parents::{
    resolve_block_parent, touch_page, touch_parent_block, validate_page_parent_exists,
};
use super::payloads::{child_page_payload, default_text_payload};
use super::sort::next_order_keys;
use crate::block_storage::{
    BlockRowInsert, BlockViewState, compose_block_payload, insert_block_row, write_block_content,
};
use crate::models::{
    NoteChildPageFromBlockCreate, NoteLoadedPage, NotePageCreate, NotePageDto, NotePageRow,
    NotePageUpdate, NoteParent, OptionalJsonValue, page_parent_columns, parent_columns,
};
use crate::validation::{
    plain_text_from_payload, require_uuid, validate_page_create, validate_page_update,
};
use crate::{assets, mention_notifications, page_history, project_history, reads};
use ganbaru_sync_contracts::OrderKey;
use serde_json::Value;
use sqlx::{Row, SqliteConnection, SqlitePool};

pub async fn create_page(
    pool: &SqlitePool,
    page: NotePageCreate,
) -> Result<NoteLoadedPage, String> {
    validate_page_create(&page)?;
    if matches!(&page.parent, NoteParent::DataSourceId { .. }) {
        return Err("data source pages must be created with the row page command".to_string());
    }
    let title = page.title.trim().to_string();
    validate_extra_page_properties(page.properties.as_ref())?;
    let requested_project_id = page
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(existing) =
        load_idempotent_page_create(pool, &page, &title, requested_project_id).await?
    {
        return Ok(existing);
    }
    if let Some(project_id) = requested_project_id {
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
    let project_id = match &child_page_parent {
        None => {
            if let Some(project_id) = requested_project_id {
                crate::structure::require_page_project_tx(&mut tx, project_id).await?;
            }
            requested_project_id.map(str::to_string)
        }
        Some(parent) => {
            let inherited = crate::structure::page_project_id_tx(&mut tx, &parent.page_id).await?;
            if requested_project_id
                .is_some_and(|project_id| inherited.as_deref() != Some(project_id))
            {
                return Err("nested notes pages inherit their parent project".to_string());
            }
            inherited
        }
    };
    let child_page_order_key = if let Some(parent) = &child_page_parent {
        Some(
            next_order_keys(&mut tx, parent, page.after_block_id.as_deref(), 1)
                .await?
                .remove(0),
        )
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
            child_block_id,
            folder_id,
            project_id,
            title
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(page.id.trim())
    .bind(parent_type)
    .bind(parent_page_id)
    .bind(parent_block_id)
    .bind(crate::structure::nested_page_block_id(
        parent_type,
        page.id.trim(),
    ))
    .bind(folder_id)
    .bind(crate::structure::stored_page_project(
        parent_type,
        project_id.as_deref(),
    ))
    .bind(&title)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create notes page: {e}"))?;
    crate::structure::validate_page_folder_tx(&mut tx, page.id.trim()).await?;
    insert_initial_block(
        &mut tx,
        page.first_block_id.trim(),
        page.id.trim(),
        &first_payload,
        &first_plain_text,
    )
    .await
    .map_err(|e| format!("create initial notes block: {e}"))?;
    crate::project_history::mark_page_dirty_tx(&mut tx, page.id.trim(), &title, false).await?;
    if let Some(parent) = child_page_parent {
        let child_payload = child_page_payload(&title);
        let child_plain_text = plain_text_from_payload("child_page", &child_payload);
        let order_key = child_page_order_key
            .ok_or_else(|| "child page order key was not prepared".to_string())?;
        insert_block_row(
            &mut tx,
            BlockRowInsert {
                id: page.id.trim(),
                page_id: &parent.page_id,
                parent_type: parent.parent_type,
                parent_page_id: parent.parent_page_id.as_deref(),
                parent_block_id: parent.parent_block_id.as_deref(),
                block_type: "child_page",
                payload: &child_payload,
                plain_text: &child_plain_text,
                order_key: &order_key,
                source_provider: None,
                source_object_id: None,
                source_last_edited_time: None,
            },
        )
        .await
        .map_err(|e| format!("create notes child page block: {e}"))?;
        touch_parent_block(&mut tx, &parent).await?;
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
    requested_project_id: Option<&str>,
) -> Result<Option<NoteLoadedPage>, String> {
    let page_id = page.id.trim();
    let existing = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT parent_type, parent_page_id, parent_block_id, folder_id, {} AS project_id, title
         FROM notes_pages WHERE id = ?",
        crate::structure::page_project_sql("notes_pages")
    )))
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
    let stored_project_id: Option<String> = existing
        .try_get("project_id")
        .map_err(|e| format!("read existing notes page project: {e}"))?;
    let stored_title: String = existing
        .try_get("title")
        .map_err(|e| format!("read existing notes page title: {e}"))?;
    let page_matches = stored_parent_type == parent_type
        && stored_parent_page_id.as_deref() == parent_page_id
        && stored_parent_block_id.as_deref() == parent_block_id
        && stored_folder_id.as_deref() == page.folder_id.as_deref().map(str::trim)
        && stored_title == title
        && project_matches(
            &page.parent,
            stored_project_id.as_deref(),
            requested_project_id,
        );
    if !page_matches {
        return Err("notes page create id already belongs to different data".to_string());
    }

    let initial_payload = default_text_payload("");
    let initial = sqlx::query(
        "SELECT page_id, parent_type, parent_page_id, type, attrs, text, order_key
         FROM notes_blocks WHERE id = ?",
    )
    .bind(page.first_block_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("check existing initial notes block: {e}"))?;
    let Some(initial) = initial else {
        return Err("existing notes page is missing its initial block".to_string());
    };
    let stored_attrs: String = initial
        .try_get("attrs")
        .map_err(|e| format!("read existing initial notes block attributes: {e}"))?;
    let stored_text: String = initial
        .try_get("text")
        .map_err(|e| format!("read existing initial notes block text: {e}"))?;
    let stored_payload = compose_block_payload(
        "paragraph",
        &stored_attrs,
        &stored_text,
        BlockViewState::default(),
        None,
    )?;
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
    let stored_order_key: String = initial
        .try_get("order_key")
        .map_err(|e| format!("read existing initial block order key: {e}"))?;
    let block_matches = stored_page_id == page_id
        && stored_parent_type == "page_id"
        && stored_parent_page_id.as_deref() == Some(page_id)
        && stored_type == "paragraph"
        && stored_order_key == OrderKey::first().as_str()
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
    validate_extra_page_properties(request.properties.as_ref())?;
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
            child_block_id,
            title
         )
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(block_id)
    .bind(page_parent_type)
    .bind(&current.parent_page_id)
    .bind(&current.parent_block_id)
    .bind(block_id)
    .bind(&title)
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
        insert_initial_block(
            &mut tx,
            request.first_block_id.trim(),
            block_id,
            &first_payload,
            &first_plain_text,
        )
        .await
        .map_err(|e| format!("create initial notes child page block: {e}"))?;
    } else {
        sqlx::query(
            "WITH RECURSIVE subtree(id, depth) AS (
                SELECT id, 1 FROM notes_blocks WHERE parent_block_id = ? AND in_trash = 0
                UNION
                SELECT child.id, subtree.depth + 1
                FROM notes_blocks AS child
                JOIN subtree ON child.parent_block_id = subtree.id
                WHERE child.in_trash = 0 AND subtree.depth < ?
             )
             UPDATE notes_blocks
             SET page_id = ?
             WHERE id IN (SELECT id FROM subtree)",
        )
        .bind(block_id)
        .bind(crate::writes::MAX_BLOCK_DEPTH)
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

    write_block_content(
        &mut tx,
        block_id,
        "child_page",
        &child_payload,
        &child_plain_text,
    )
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

/// Compare a stored page project with a retried create request.
///
/// Nested pages inherit their project, so a retry may omit it.
fn project_matches(parent: &NoteParent, stored: Option<&str>, requested: Option<&str>) -> bool {
    match parent {
        NoteParent::Workspace { .. } => stored == requested,
        _ => requested.is_none() || stored == requested,
    }
}

/// Validates the extra properties of a new page outside a data source. Only rows store property
/// values, so a page outside a data source keeps its title and drops the rest.
fn validate_extra_page_properties(extra_properties: Option<&Value>) -> Result<(), String> {
    match extra_properties {
        Some(extra_properties) => crate::validation::validate_page_properties(extra_properties),
        None => Ok(()),
    }
}

/// Applies the fields set in `update` to an active page and returns the updated page.
///
/// `properties` replaces the page's stored values as a whole: a value whose property the object
/// omits is deleted, so a caller that edits one property sends the full object it read. Relation
/// entries in the object are ignored and the page's relation links stay as they are; relations
/// change through the row property and relation commands. See
/// [`write_page_properties`](crate::page_property_storage::write_page_properties).
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
                 child_block_id = ?,
                 folder_id = NULL,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0 AND archived = 0",
        )
        .bind(parent_type)
        .bind(parent_page_id)
        .bind(parent_block_id)
        .bind(parent_data_source_id)
        .bind(crate::structure::nested_page_block_id(parent_type, page_id))
        .bind(page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes page parent: {e}"))?;
        crate::structure::detach_row_hierarchy_tx(&mut tx, page_id).await?;
    }
    if let Some(title) = update.title {
        let title = title.trim().to_string();
        sqlx::query(
            "UPDATE notes_pages
             SET title = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0 AND archived = 0",
        )
        .bind(&title)
        .bind(page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes page title: {e}"))?;
        let child_payload = child_page_payload(&title);
        sqlx::query(
            "UPDATE notes_blocks
             SET plain_text = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND type = 'child_page'",
        )
        .bind(plain_text_from_payload("child_page", &child_payload))
        .bind(page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes child page block title: {e}"))?;
    }
    if let Some(properties) = &update.properties {
        let touched = sqlx::query(
            "UPDATE notes_pages
             SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0 AND archived = 0",
        )
        .bind(page_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes page properties: {e}"))?;
        if touched.rows_affected() > 0 {
            crate::page_property_storage::write_page_properties(&mut tx, page_id, properties)
                .await?;
        }
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
    sqlx::query_as::<_, NotePageRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {} FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
        crate::structure::page_row_columns("notes_pages")
    )))
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes page row: {e}"))?
    .ok_or_else(|| "notes page not found".to_string())
}

/// Inserts the first paragraph of a new page at the first order key.
async fn insert_initial_block(
    conn: &mut SqliteConnection,
    block_id: &str,
    page_id: &str,
    payload: &Value,
    plain_text: &str,
) -> Result<(), String> {
    let order_key = OrderKey::first().into_string();
    insert_block_row(
        conn,
        BlockRowInsert {
            id: block_id,
            page_id,
            parent_type: "page_id",
            parent_page_id: Some(page_id),
            parent_block_id: None,
            block_type: "paragraph",
            payload,
            plain_text,
            order_key: &order_key,
            source_provider: None,
            source_object_id: None,
            source_last_edited_time: None,
        },
    )
    .await
}
