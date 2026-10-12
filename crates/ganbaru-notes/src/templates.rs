use crate::models::{
    NoteBlockRow, NoteLoadedPage, NotePageTemplateApply, NotePageTemplateBlockRow,
    NotePageTemplateCreateFromPage, NotePageTemplateDto, NotePageTemplateDuplicate,
    NotePageTemplateRow, NotePageTemplateUpdate, NoteParent, parent_columns,
};
use crate::validation::{
    plain_text_from_payload, require_uuid, validate_block_payload, validate_parent,
    validate_sort_order,
};
use crate::{page_history, reads};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

const DEFAULT_BLOCK_SORT_STEP: f64 = 1000.0;

struct TemplatePageParent {
    parent_page_id: Option<String>,
}

pub async fn list_page_templates(pool: &SqlitePool) -> Result<Vec<NotePageTemplateDto>, String> {
    let rows = sqlx::query_as::<_, NotePageTemplateRow>(
        "SELECT
            template.id,
            template.name,
            template.source_page_id,
            template.properties,
            template.icon,
            template.cover,
            COUNT(block.id) AS block_count,
            template.created_time,
            template.last_edited_time
         FROM notes_page_templates AS template
         LEFT JOIN notes_page_template_blocks AS block ON block.template_id = template.id
         GROUP BY template.id
         ORDER BY template.last_edited_time DESC, template.name COLLATE NOCASE ASC, template.id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes page templates: {e}"))?;
    rows.into_iter().map(NotePageTemplateDto::new).collect()
}

pub async fn create_page_template_from_page(
    pool: &SqlitePool,
    request: NotePageTemplateCreateFromPage,
) -> Result<NotePageTemplateDto, String> {
    let template_id = normalize_uuid(&request.id, "id")?;
    let source_page_id = normalize_uuid(&request.source_page_id, "source_page_id")?;
    let name = normalize_template_name(&request.name)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page template create: {e}"))?;
    let source_page = load_active_page_snapshot(&mut tx, &source_page_id).await?;
    sqlx::query(
        "INSERT INTO notes_page_templates (
            id,
            name,
            source_page_id,
            properties,
            icon,
            cover
         )
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&template_id)
    .bind(&name)
    .bind(&source_page_id)
    .bind(&source_page.properties)
    .bind(&source_page.icon)
    .bind(&source_page.cover)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create notes page template: {e}"))?;
    replace_template_blocks_from_page(&mut tx, &template_id, &source_page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page template create: {e}"))?;
    get_page_template(pool, &template_id).await
}

pub async fn apply_page_template(
    pool: &SqlitePool,
    template_id: &str,
    request: NotePageTemplateApply,
) -> Result<NoteLoadedPage, String> {
    let template_id = normalize_uuid(template_id, "template_id")?;
    validate_parent(&request.parent)?;
    if matches!(
        &request.parent,
        NoteParent::BlockId { .. } | NoteParent::DataSourceId { .. }
    ) {
        return Err("page templates can only create workspace pages or subpages".to_string());
    }
    let title_override = request
        .title
        .as_deref()
        .map(normalize_optional_page_title)
        .transpose()?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page template apply: {e}"))?;
    let template = load_template_row(&mut tx, &template_id).await?;
    let blocks = load_template_block_rows(&mut tx, &template_id).await?;
    let parent = resolve_page_template_parent(&mut tx, &request.parent).await?;
    if let Some(parent) = &parent {
        if let Some(parent_page_id) = &parent.parent_page_id {
            page_history::record_page_snapshot_tx(&mut tx, parent_page_id, "apply_page_template")
                .await?;
        }
    }
    let mut reserved_ids = HashSet::new();
    let page_id = new_note_id(&mut tx, &mut reserved_ids).await?;
    let title = title_override.unwrap_or_else(|| template.name.clone());
    let properties = properties_with_title(&template.properties, &title)?;
    let (parent_type, parent_page_id, parent_block_id) = parent_columns(&request.parent);
    sqlx::query(
        "INSERT INTO notes_pages (
            id,
            parent_type,
            parent_page_id,
            parent_block_id,
            title,
            properties,
            icon,
            cover
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&page_id)
    .bind(parent_type)
    .bind(parent_page_id)
    .bind(parent_block_id)
    .bind(&title)
    .bind(properties.to_string())
    .bind(&template.icon)
    .bind(&template.cover)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create notes page from template: {e}"))?;
    if let Some(parent) = &parent {
        insert_parent_child_page_block(&mut tx, &page_id, &title, parent).await?;
    }
    if blocks.is_empty() {
        insert_default_page_block(&mut tx, &page_id, &mut reserved_ids).await?;
    } else {
        insert_template_blocks_as_page_blocks(&mut tx, &page_id, &blocks, &mut reserved_ids)
            .await?;
    }
    if let Some(parent) = &parent {
        touch_page(&mut tx, &parent.parent_page_id).await?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page template apply: {e}"))?;
    reads::load_page(pool, &page_id).await
}

pub async fn update_page_template(
    pool: &SqlitePool,
    template_id: &str,
    update: NotePageTemplateUpdate,
) -> Result<NotePageTemplateDto, String> {
    let template_id = normalize_uuid(template_id, "template_id")?;
    let name = update
        .name
        .as_deref()
        .map(normalize_template_name)
        .transpose()?;
    let source_page_id = update
        .source_page_id
        .as_deref()
        .map(|value| normalize_uuid(value, "source_page_id"))
        .transpose()?;
    if name.is_none() && source_page_id.is_none() {
        return Err("template update must include name or source_page_id".to_string());
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page template update: {e}"))?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM notes_page_templates WHERE id = ?")
        .bind(&template_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("load notes page template before update: {e}"))?;
    if exists.is_none() {
        return Err("notes page template not found".to_string());
    }
    if let Some(name) = &name {
        sqlx::query(
            "UPDATE notes_page_templates
             SET name = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(name)
        .bind(&template_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("rename notes page template: {e}"))?;
    }
    if let Some(source_page_id) = &source_page_id {
        let source_page = load_active_page_snapshot(&mut tx, source_page_id).await?;
        sqlx::query(
            "UPDATE notes_page_templates
             SET source_page_id = ?,
                 properties = ?,
                 icon = ?,
                 cover = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(source_page_id)
        .bind(&source_page.properties)
        .bind(&source_page.icon)
        .bind(&source_page.cover)
        .bind(&template_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes page template snapshot: {e}"))?;
        replace_template_blocks_from_page(&mut tx, &template_id, source_page_id).await?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page template update: {e}"))?;
    get_page_template(pool, &template_id).await
}

pub async fn duplicate_page_template(
    pool: &SqlitePool,
    template_id: &str,
    request: NotePageTemplateDuplicate,
) -> Result<NotePageTemplateDto, String> {
    let template_id = normalize_uuid(template_id, "template_id")?;
    let duplicate_id = normalize_uuid(&request.id, "id")?;
    let duplicate_name = normalize_template_name(&request.name)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes page template duplicate: {e}"))?;
    let source = load_template_row(&mut tx, &template_id).await?;
    let blocks = load_template_block_rows(&mut tx, &template_id).await?;
    sqlx::query(
        "INSERT INTO notes_page_templates (
            id,
            name,
            source_page_id,
            properties,
            icon,
            cover
         )
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&duplicate_id)
    .bind(&duplicate_name)
    .bind(&source.source_page_id)
    .bind(&source.properties)
    .bind(&source.icon)
    .bind(&source.cover)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("duplicate notes page template: {e}"))?;
    for block in &blocks {
        insert_template_block_snapshot(&mut tx, &duplicate_id, block).await?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit notes page template duplicate: {e}"))?;
    get_page_template(pool, &duplicate_id).await
}

pub async fn delete_page_template(pool: &SqlitePool, template_id: &str) -> Result<String, String> {
    let template_id = normalize_uuid(template_id, "template_id")?;
    let result = sqlx::query("DELETE FROM notes_page_templates WHERE id = ?")
        .bind(&template_id)
        .execute(pool)
        .await
        .map_err(|e| format!("delete notes page template: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("notes page template not found".to_string());
    }
    Ok(template_id)
}

async fn get_page_template(
    pool: &SqlitePool,
    template_id: &str,
) -> Result<NotePageTemplateDto, String> {
    let row = sqlx::query_as::<_, NotePageTemplateRow>(
        "SELECT
            template.id,
            template.name,
            template.source_page_id,
            template.properties,
            template.icon,
            template.cover,
            COUNT(block.id) AS block_count,
            template.created_time,
            template.last_edited_time
         FROM notes_page_templates AS template
         LEFT JOIN notes_page_template_blocks AS block ON block.template_id = template.id
         WHERE template.id = ?
         GROUP BY template.id",
    )
    .bind(template_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes page template: {e}"))?
    .ok_or_else(|| "notes page template not found".to_string())?;
    NotePageTemplateDto::new(row)
}

async fn load_template_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    template_id: &str,
) -> Result<NotePageTemplateRow, String> {
    sqlx::query_as::<_, NotePageTemplateRow>(
        "SELECT
            template.id,
            template.name,
            template.source_page_id,
            template.properties,
            template.icon,
            template.cover,
            COUNT(block.id) AS block_count,
            template.created_time,
            template.last_edited_time
         FROM notes_page_templates AS template
         LEFT JOIN notes_page_template_blocks AS block ON block.template_id = template.id
         WHERE template.id = ?
         GROUP BY template.id",
    )
    .bind(template_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes page template: {e}"))?
    .ok_or_else(|| "notes page template not found".to_string())
}

async fn load_active_page_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
) -> Result<SourcePageSnapshot, String> {
    sqlx::query_as::<_, SourcePageSnapshot>(
        "SELECT properties, icon, cover
         FROM notes_pages
         WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes page for template: {e}"))?
    .ok_or_else(|| "source page not found".to_string())
}

async fn replace_template_blocks_from_page(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    template_id: &str,
    source_page_id: &str,
) -> Result<(), String> {
    sqlx::query("DELETE FROM notes_page_template_blocks WHERE template_id = ?")
        .bind(template_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear notes page template blocks: {e}"))?;
    let source_blocks = load_page_block_subtree_rows(tx, source_page_id).await?;
    for block in &source_blocks {
        validate_block_payload(
            &block.block_type,
            &serde_json::from_str(&block.payload)
                .map_err(|e| format!("parse notes source block payload: {e}"))?,
        )?;
        insert_source_block_snapshot(tx, template_id, block).await?;
    }
    Ok(())
}

async fn insert_source_block_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    template_id: &str,
    block: &NoteBlockRow,
) -> Result<(), String> {
    validate_sort_order(block.sort_order)?;
    let (parent_type, parent_block_id): (&str, Option<&str>) = if block.parent_type == "page_id" {
        ("template", None)
    } else {
        (
            "block_id",
            Some(
                block
                    .parent_block_id
                    .as_deref()
                    .ok_or_else(|| "template block parent is missing".to_string())?,
            ),
        )
    };
    sqlx::query(
        "INSERT INTO notes_page_template_blocks (
            template_id,
            id,
            parent_type,
            parent_block_id,
            has_children,
            type,
            payload,
            plain_text,
            sort_order
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(template_id)
    .bind(&block.id)
    .bind(parent_type)
    .bind(parent_block_id)
    .bind(block.has_children)
    .bind(&block.block_type)
    .bind(&block.payload)
    .bind(&block.plain_text)
    .bind(block.sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("snapshot notes page template block: {e}"))?;
    Ok(())
}

async fn insert_template_block_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    template_id: &str,
    block: &NotePageTemplateBlockRow,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO notes_page_template_blocks (
            template_id,
            id,
            parent_type,
            parent_block_id,
            has_children,
            type,
            payload,
            plain_text,
            sort_order
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(template_id)
    .bind(&block.id)
    .bind(&block.parent_type)
    .bind(&block.parent_block_id)
    .bind(block.has_children)
    .bind(&block.block_type)
    .bind(&block.payload)
    .bind(&block.plain_text)
    .bind(block.sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("copy notes page template block: {e}"))?;
    Ok(())
}

async fn insert_template_blocks_as_page_blocks(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
    blocks: &[NotePageTemplateBlockRow],
    reserved_ids: &mut HashSet<String>,
) -> Result<(), String> {
    let mut block_ids = HashMap::with_capacity(blocks.len());
    for block in blocks {
        let duplicate_id = new_note_id(tx, reserved_ids).await?;
        block_ids.insert(block.id.clone(), duplicate_id);
    }
    for block in blocks {
        let duplicate_id = block_ids
            .get(&block.id)
            .cloned()
            .ok_or_else(|| "template block id mapping is missing".to_string())?;
        let (parent_type, parent_page_id, parent_block_id) = if block.parent_type == "template" {
            ("page_id", Some(page_id.to_string()), None)
        } else {
            let source_parent_id = block
                .parent_block_id
                .as_ref()
                .ok_or_else(|| "template child block is missing its parent".to_string())?;
            let duplicate_parent_id = block_ids
                .get(source_parent_id)
                .cloned()
                .ok_or_else(|| "template child block parent was not cloned".to_string())?;
            ("block_id", None, Some(duplicate_parent_id))
        };
        validate_sort_order(block.sort_order)?;
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
        .bind(&duplicate_id)
        .bind(page_id)
        .bind(parent_type)
        .bind(parent_page_id)
        .bind(parent_block_id.as_deref())
        .bind(block.has_children)
        .bind(&block.block_type)
        .bind(&block.payload)
        .bind(&block.plain_text)
        .bind(block.sort_order)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("apply notes page template block: {e}"))?;
        if block.block_type == "child_page" {
            insert_applied_child_page(
                tx,
                &duplicate_id,
                page_id,
                parent_type,
                parent_block_id,
                block,
                reserved_ids,
            )
            .await?;
        }
    }
    Ok(())
}

async fn insert_applied_child_page(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
    containing_page_id: &str,
    block_parent_type: &str,
    block_parent_block_id: Option<String>,
    block: &NotePageTemplateBlockRow,
    reserved_ids: &mut HashSet<String>,
) -> Result<(), String> {
    let payload: Value = serde_json::from_str(&block.payload)
        .map_err(|e| format!("parse template child page payload: {e}"))?;
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
    .map_err(|e| format!("create template child page: {e}"))?;
    insert_default_page_block(tx, page_id, reserved_ids).await
}

async fn insert_default_page_block(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
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
    .map_err(|e| format!("create template page initial block: {e}"))?;
    Ok(())
}

async fn insert_parent_child_page_block(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &str,
    title: &str,
    parent: &TemplatePageParent,
) -> Result<(), String> {
    let sort_order = next_page_child_sort_order(tx, &parent.parent_page_id).await?;
    validate_sort_order(sort_order)?;
    let payload = child_page_payload(title);
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
         VALUES (?, ?, 'page_id', ?, 'child_page', ?, ?, ?)",
    )
    .bind(page_id)
    .bind(&parent.parent_page_id)
    .bind(&parent.parent_page_id)
    .bind(payload.to_string())
    .bind(plain_text_from_payload("child_page", &payload))
    .bind(sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("create template page child block: {e}"))?;
    Ok(())
}

async fn resolve_page_template_parent(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent: &NoteParent,
) -> Result<Option<TemplatePageParent>, String> {
    match parent {
        NoteParent::Workspace { .. } => Ok(None),
        NoteParent::PageId { page_id } => {
            let page_id = normalize_uuid(page_id, "parent.page_id")?;
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
            )
            .bind(&page_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load notes page template parent: {e}"))?;
            if exists.is_none() {
                return Err("parent page not found".to_string());
            }
            Ok(Some(TemplatePageParent {
                parent_page_id: Some(page_id),
            }))
        }
        NoteParent::BlockId { .. } => {
            Err("page templates can only create workspace pages or subpages".to_string())
        }
        NoteParent::DataSourceId { .. } => {
            Err("page templates can only create workspace pages or subpages".to_string())
        }
    }
}

async fn load_template_block_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    template_id: &str,
) -> Result<Vec<NotePageTemplateBlockRow>, String> {
    sqlx::query_as::<_, NotePageTemplateBlockRow>(
        "WITH RECURSIVE subtree(id, path) AS (
            SELECT id, printf('%020.6f:%s', sort_order, id)
            FROM notes_page_template_blocks
            WHERE template_id = ? AND parent_type = 'template'
            UNION ALL
            SELECT child.id, subtree.path || '/' || printf('%020.6f:%s', child.sort_order, child.id)
            FROM notes_page_template_blocks AS child
            JOIN subtree ON child.parent_block_id = subtree.id
            WHERE child.template_id = ?
         )
         SELECT
            block.template_id,
            block.id,
            block.parent_type,
            block.parent_block_id,
            block.has_children,
            block.type AS block_type,
            block.payload,
            block.plain_text,
            block.sort_order,
            block.created_time,
            block.last_edited_time
         FROM notes_page_template_blocks AS block
         JOIN subtree ON subtree.id = block.id
         WHERE block.template_id = ?
         ORDER BY subtree.path ASC",
    )
    .bind(template_id)
    .bind(template_id)
    .bind(template_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load notes page template blocks: {e}"))
}

async fn load_page_block_subtree_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
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
    .map_err(|e| format!("load notes page blocks for template: {e}"))
}

async fn next_page_child_sort_order(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    parent_page_id: &Option<String>,
) -> Result<f64, String> {
    let max_order: Option<f64> = sqlx::query_scalar(
        "SELECT MAX(sort_order)
         FROM notes_blocks
         WHERE parent_type = 'page_id'
           AND parent_page_id = ?
           AND in_trash = 0",
    )
    .bind(parent_page_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load notes template parent sort order: {e}"))?;
    Ok(max_order.unwrap_or(0.0) + DEFAULT_BLOCK_SORT_STEP)
}

async fn touch_page(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    page_id: &Option<String>,
) -> Result<(), String> {
    let Some(page_id) = page_id else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE notes_pages
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch notes template parent page: {e}"))?;
    Ok(())
}

async fn new_note_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
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
        .map_err(|e| format!("generate notes template id: {e}"))?;
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
                OR EXISTS (SELECT 1 FROM notes_page_templates WHERE id = ?)",
        )
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("check generated notes template id: {e}"))?;
        if exists.is_none() {
            reserved_ids.insert(id.clone());
            return Ok(id);
        }
    }
    Err("could not generate a unique notes id".to_string())
}

fn normalize_uuid(value: &str, field: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    require_uuid(&value, field)?;
    Ok(value)
}

fn normalize_template_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() {
        return Err("template name must not be empty".to_string());
    }
    if name.chars().any(char::is_control) {
        return Err("template name must not contain control characters".to_string());
    }
    if name.chars().count() > 200 {
        return Err("template name must be 200 characters or fewer".to_string());
    }
    Ok(name.to_string())
}

fn normalize_optional_page_title(value: &str) -> Result<String, String> {
    let title = value.trim();
    if title.chars().any(char::is_control) {
        return Err("page title must not contain control characters".to_string());
    }
    Ok(title.to_string())
}

fn properties_with_title(properties: &str, title: &str) -> Result<Value, String> {
    let mut value: Value = serde_json::from_str(properties)
        .map_err(|e| format!("parse page template properties: {e}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "page template properties must be an object".to_string())?;
    object.insert(
        "title".to_string(),
        json!({
            "id": "title",
            "type": "title",
            "title": [rich_text(title)]
        }),
    );
    Ok(value)
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

fn child_page_payload(title: &str) -> Value {
    json!({
        "title": title
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

#[derive(sqlx::FromRow)]
struct SourcePageSnapshot {
    properties: String,
    icon: Option<String>,
    cover: Option<String>,
}
