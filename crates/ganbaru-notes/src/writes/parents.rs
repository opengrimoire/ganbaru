use crate::models::{NoteBlockRow, NoteBlockWrite, NoteParent};
use crate::validation::{
    block_payload_supports_children, plain_text_from_payload, require_uuid, validate_sort_order,
};
use crate::{assets, mention_notifications};
use serde_json::Value;

pub struct ParentTarget {
    pub parent_type: &'static str,
    pub parent_page_id: Option<String>,
    pub parent_block_id: Option<String>,
    pub parent_block_type: Option<String>,
    pub page_id: String,
}

pub fn parent_target_from_block_row(row: &NoteBlockRow) -> ParentTarget {
    ParentTarget {
        parent_type: if row.parent_type == "page_id" {
            "page_id"
        } else {
            "block_id"
        },
        parent_page_id: row.parent_page_id.clone(),
        parent_block_id: row.parent_block_id.clone(),
        parent_block_type: None,
        page_id: row.page_id.clone(),
    }
}

pub async fn validate_page_parent_exists(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &NoteParent,
) -> Result<(), String> {
    match parent {
        NoteParent::Workspace { .. } => Ok(()),
        NoteParent::PageId { page_id } => {
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
            )
            .bind(page_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load parent page: {e}"))?;
            exists
                .map(|_| ())
                .ok_or_else(|| "parent page not found".to_string())
        }
        NoteParent::BlockId { block_id } => {
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM notes_blocks WHERE id = ? AND in_trash = 0")
                    .bind(block_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| format!("load parent block: {e}"))?;
            exists
                .map(|_| ())
                .ok_or_else(|| "parent block not found".to_string())
        }
        NoteParent::DataSourceId { data_source_id } => {
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT 1
                 FROM notes_data_sources AS data_source
                 JOIN notes_databases AS database ON database.id = data_source.database_id
                 WHERE data_source.id = ?
                   AND data_source.in_trash = 0
                   AND database.in_trash = 0",
            )
            .bind(data_source_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load parent data source: {e}"))?;
            exists
                .map(|_| ())
                .ok_or_else(|| "parent data source not found".to_string())
        }
    }
}

pub async fn resolve_block_parent(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &NoteParent,
) -> Result<ParentTarget, String> {
    match parent {
        NoteParent::Workspace { .. } => Err("blocks cannot be parented by workspace".to_string()),
        NoteParent::PageId { page_id } => {
            let page_id = page_id.trim();
            require_uuid(page_id, "page_id")?;
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
            )
            .bind(page_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load block parent page: {e}"))?;
            if exists.is_none() {
                return Err("parent page not found".to_string());
            }
            Ok(ParentTarget {
                parent_type: "page_id",
                parent_page_id: Some(page_id.to_string()),
                parent_block_id: None,
                parent_block_type: None,
                page_id: page_id.to_string(),
            })
        }
        NoteParent::BlockId { block_id } => {
            let block_id = block_id.trim();
            require_uuid(block_id, "block_id")?;
            let parent: Option<(String, String, String)> = sqlx::query_as(
                "SELECT page_id, type, payload FROM notes_blocks WHERE id = ? AND in_trash = 0",
            )
            .bind(block_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load block parent block: {e}"))?;
            let (page_id, block_type, payload) =
                parent.ok_or_else(|| "parent block not found".to_string())?;
            let payload: Value = serde_json::from_str(&payload)
                .map_err(|e| format!("parse parent block payload: {e}"))?;
            if !block_payload_supports_children(&block_type, &payload) {
                return Err(format!("{block_type} blocks cannot have children"));
            }
            Ok(ParentTarget {
                parent_type: "block_id",
                parent_page_id: None,
                parent_block_id: Some(block_id.to_string()),
                parent_block_type: Some(block_type),
                page_id,
            })
        }
        NoteParent::DataSourceId { .. } => {
            Err("blocks cannot be parented by data sources".to_string())
        }
    }
}

pub(super) fn validate_children_for_parent(
    parent: &ParentTarget,
    children: &[NoteBlockWrite],
) -> Result<(), String> {
    for child in children {
        let payload = child
            .payload()
            .ok_or_else(|| format!("{} payload is required", child.block_type))?;
        validate_block_for_parent(parent, &child.block_type, payload)?;
    }
    Ok(())
}

pub(crate) fn validate_block_for_parent(
    parent: &ParentTarget,
    block_type: &str,
    payload: &Value,
) -> Result<(), String> {
    validate_block_type_for_parent(parent, block_type)?;
    if block_type == "paragraph"
        && payload.get("icon").is_some()
        && parent.parent_block_type.as_deref() != Some("tab")
    {
        return Err("paragraph.icon is only supported for tab labels".to_string());
    }
    Ok(())
}

pub(super) fn validate_block_type_for_parent(
    parent: &ParentTarget,
    block_type: &str,
) -> Result<(), String> {
    if parent.parent_block_type.as_deref() == Some("column_list") {
        if block_type == "column" {
            return Ok(());
        }
        return Err(format!(
            "{block_type} blocks cannot be children of column_list blocks"
        ));
    }
    if parent.parent_block_type.as_deref() == Some("table") {
        if block_type == "table_row" {
            return Ok(());
        }
        return Err(format!(
            "{block_type} blocks cannot be children of table blocks"
        ));
    }
    if parent.parent_block_type.as_deref() == Some("tab") {
        if block_type == "paragraph" {
            return Ok(());
        }
        return Err(format!(
            "{block_type} blocks cannot be children of tab blocks"
        ));
    }
    if block_type == "table_row" {
        return Err("table_row blocks must be children of table blocks".to_string());
    }
    if block_type == "column" {
        return Err("column blocks must be children of column_list blocks".to_string());
    }
    Ok(())
}

pub(super) async fn validate_block_update_parent(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    current: &NoteBlockRow,
    target_block_type: &str,
    target_payload: &Value,
) -> Result<(), String> {
    let parent_block_type = if let Some(parent_block_id) = &current.parent_block_id {
        sqlx::query_scalar::<_, String>(
            "SELECT type FROM notes_blocks WHERE id = ? AND in_trash = 0",
        )
        .bind(parent_block_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load notes block parent type: {e}"))?
    } else {
        None
    };
    let parent = ParentTarget {
        parent_type: if current.parent_type == "page_id" {
            "page_id"
        } else {
            "block_id"
        },
        parent_page_id: current.parent_page_id.clone(),
        parent_block_id: current.parent_block_id.clone(),
        parent_block_type,
        page_id: current.page_id.clone(),
    };
    validate_block_for_parent(&parent, target_block_type, target_payload)
}

pub(super) async fn validate_block_update_children(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
    current_block_type: &str,
    target_block_type: &str,
    target_payload: &Value,
) -> Result<(), String> {
    if target_block_type == "table" {
        ensure_existing_children_have_type(tx, block_id, "table_row", "table").await?;
    }
    if target_block_type == "column_list" {
        ensure_existing_children_have_type(tx, block_id, "column", "column_list").await?;
    }
    if target_block_type == "tab" {
        ensure_existing_children_have_type(tx, block_id, "paragraph", "tab").await?;
    }
    if current_block_type == "table" && target_block_type != "table" {
        let child_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_blocks
             WHERE parent_block_id = ? AND in_trash = 0",
        )
        .bind(block_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("count notes table rows: {e}"))?;
        if child_count > 0 {
            return Err(format!(
                "table blocks with rows cannot be converted to {target_block_type} blocks"
            ));
        }
    }
    if current_block_type == "column_list" && target_block_type != "column_list" {
        let child_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_blocks
             WHERE parent_block_id = ? AND in_trash = 0",
        )
        .bind(block_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("count notes column children: {e}"))?;
        if child_count > 0 {
            return Err(format!(
                "column_list blocks with columns cannot be converted to {target_block_type} blocks"
            ));
        }
    }
    if current_block_type == "tab" && target_block_type != "tab" {
        let child_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_blocks
             WHERE parent_block_id = ? AND in_trash = 0",
        )
        .bind(block_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("count notes tab children: {e}"))?;
        if child_count > 0 {
            return Err(format!(
                "tab blocks with labels cannot be converted to {target_block_type} blocks"
            ));
        }
    }
    if matches!(
        target_block_type,
        "heading_1" | "heading_2" | "heading_3" | "heading_4" | "heading_5" | "heading_6"
    ) && !block_payload_supports_children(target_block_type, target_payload)
    {
        let child_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_blocks
             WHERE parent_block_id = ? AND in_trash = 0",
        )
        .bind(block_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("count notes heading children: {e}"))?;
        if child_count > 0 {
            return Err(format!(
                "{target_block_type} blocks with children must stay toggleable"
            ));
        }
    }
    if target_block_type == "synced_block"
        && !block_payload_supports_children(target_block_type, target_payload)
    {
        let child_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_blocks
             WHERE parent_block_id = ? AND in_trash = 0",
        )
        .bind(block_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("count notes synced block children: {e}"))?;
        if child_count > 0 {
            return Err("synced_block blocks with children must stay original".to_string());
        }
    }
    Ok(())
}

pub(super) async fn ensure_existing_children_have_type(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
    allowed_child_type: &str,
    parent_type: &str,
) -> Result<(), String> {
    let invalid_child_type: Option<String> = sqlx::query_scalar(
        "SELECT type
         FROM notes_blocks
         WHERE parent_block_id = ?
           AND in_trash = 0
           AND type <> ?
         LIMIT 1",
    )
    .bind(block_id)
    .bind(allowed_child_type)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes {parent_type} child type: {e}"))?;
    if invalid_child_type.is_some() {
        return Err(format!(
            "{parent_type} blocks can only contain {allowed_child_type} blocks"
        ));
    }
    Ok(())
}

pub(super) async fn insert_block(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
    block: &NoteBlockWrite,
    sort_order: f64,
) -> Result<(), String> {
    validate_sort_order(sort_order)?;
    let payload = block
        .payload()
        .ok_or_else(|| format!("{} payload is required", block.block_type))?;
    let plain_text = plain_text_from_payload(&block.block_type, payload);
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
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(block.id.trim())
    .bind(&parent.page_id)
    .bind(parent.parent_type)
    .bind(&parent.parent_page_id)
    .bind(&parent.parent_block_id)
    .bind(block.block_type.trim())
    .bind(payload.to_string())
    .bind(&plain_text)
    .bind(sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert notes block: {e}"))?;
    mention_notifications::sync_block_tx(
        tx,
        block.id.trim(),
        &parent.page_id,
        block.block_type.trim(),
        payload,
        &plain_text,
    )
    .await?;
    assets::sync_block_asset_reference_tx(
        tx,
        block.id.trim(),
        &parent.page_id,
        block.block_type.trim(),
        payload,
    )
    .await?;
    Ok(())
}

pub async fn refresh_parent_has_children(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &ParentTarget,
) -> Result<(), String> {
    let Some(block_id) = &parent.parent_block_id else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE notes_blocks
         SET has_children = CASE
             WHEN EXISTS (
                 SELECT 1 FROM notes_blocks AS child
                 WHERE child.parent_block_id = notes_blocks.id
                   AND child.in_trash = 0
             )
             THEN 1 ELSE 0 END,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(block_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("refresh notes parent has_children: {e}"))?;
    Ok(())
}

pub async fn touch_page(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_pages
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch notes page: {e}"))?;
    Ok(())
}
