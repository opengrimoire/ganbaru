use crate::models::{
    NoteBlockRow, NoteDataSourceTemplateApply, NoteDataSourceTemplateBlockRow,
    NoteDataSourceTemplateCreateFromRow, NoteDataSourceTemplateDto,
    NoteDataSourceTemplateDuplicate, NoteDataSourceTemplateRow, NoteDataSourceTemplateUpdate,
    NoteLoadedPage, NotePageRow, NoteParent,
};
use crate::validation::{plain_text_from_payload, require_uuid, validate_block_payload};
use crate::{data_sources, reads, writes};
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::{HashMap, HashSet};

const MAX_TEMPLATE_NAME_CHARS: usize = 120;

pub async fn list_data_source_templates(
    pool: &SqlitePool,
    data_source_id: &str,
) -> Result<Vec<NoteDataSourceTemplateDto>, String> {
    require_uuid(data_source_id, "data_source_id")?;
    let rows = sqlx::query_as::<_, NoteDataSourceTemplateRow>(
        "SELECT
            template.id,
            template.data_source_id,
            template.source_page_id,
            template.name,
            template.properties,
            template.is_default,
            COUNT(block.id) AS block_count,
            template.created_time,
            template.last_edited_time
         FROM notes_data_source_templates AS template
         LEFT JOIN notes_data_source_template_blocks AS block ON block.template_id = template.id
         WHERE template.data_source_id = ?
         GROUP BY template.id
         ORDER BY template.is_default DESC,
                  template.last_edited_time DESC,
                  template.name COLLATE NOCASE ASC,
                  template.id ASC",
    )
    .bind(data_source_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes data source templates: {e}"))?;
    rows.into_iter()
        .map(NoteDataSourceTemplateDto::new)
        .collect()
}

pub async fn create_data_source_template_from_row(
    pool: &SqlitePool,
    data_source_id: &str,
    request: NoteDataSourceTemplateCreateFromRow,
) -> Result<NoteDataSourceTemplateDto, String> {
    let data_source_id = data_source_id.trim();
    require_uuid(data_source_id, "data_source_id")?;
    let template_id = normalize_uuid(&request.id, "id")?;
    let source_page_id = normalize_uuid(&request.source_page_id, "source_page_id")?;
    let name = normalize_template_name(&request.name)?;
    let is_default = request.is_default.unwrap_or(false);
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source template create: {e}"))?;
    let data_source = load_active_data_source_tx(&mut tx, data_source_id).await?;
    let row = load_active_row_page_tx(&mut tx, data_source_id, &source_page_id).await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let row_properties = parse_json(&row.properties, "row page properties")?;
    let (_, properties) = data_sources::rows::row_page_properties(
        &schema_properties,
        &row.title,
        Some(&row_properties_for_schema(
            &schema_properties,
            &row_properties,
        )?),
    )?;
    if is_default {
        clear_default_template_tx(&mut tx, data_source_id).await?;
    }
    sqlx::query(
        "INSERT INTO notes_data_source_templates (
            id,
            data_source_id,
            source_page_id,
            name,
            properties,
            is_default
         )
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&template_id)
    .bind(data_source_id)
    .bind(&source_page_id)
    .bind(&name)
    .bind(properties.to_string())
    .bind(if is_default { 1 } else { 0 })
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create notes data source template: {e}"))?;
    replace_template_blocks_from_page(&mut tx, &template_id, &source_page_id).await?;
    touch_data_source_tx(&mut tx, data_source_id, &data_source.database_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source template create: {e}"))?;
    get_data_source_template(pool, data_source_id, &template_id).await
}

/// Apply a template with an optional caller-reserved page ID and fresh body block IDs.
pub async fn apply_data_source_template(
    pool: &SqlitePool,
    data_source_id: &str,
    template_id: &str,
    request: NoteDataSourceTemplateApply,
) -> Result<NoteLoadedPage, String> {
    let data_source_id = data_source_id.trim();
    let template_id = normalize_uuid(template_id, "template_id")?;
    require_uuid(data_source_id, "data_source_id")?;
    let requested_id = request
        .id
        .as_deref()
        .map(|id| normalize_uuid(id, "id"))
        .transpose()?;
    let title_override = request
        .title
        .as_deref()
        .map(normalize_row_title)
        .transpose()?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source template apply: {e}"))?;
    let data_source = load_active_data_source_tx(&mut tx, data_source_id).await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let template = load_template_row_tx(&mut tx, data_source_id, &template_id).await?;
    let template_properties = parse_json(&template.properties, "data source template properties")?;
    let title = title_override.unwrap_or_else(|| template.name.clone());
    let mut replay_properties =
        row_properties_for_schema(&schema_properties, &template_properties)?;
    remove_title_property(&schema_properties, &mut replay_properties)?;
    let (title, properties) = data_sources::rows::row_page_properties(
        &schema_properties,
        &title,
        Some(&replay_properties),
    )?;
    let blocks = load_template_block_rows_tx(&mut tx, &template_id).await?;
    let page_id = if let Some(id) = requested_id {
        id
    } else {
        data_sources::views::generate_uuid_tx(
            &mut tx,
            "generate data source template page id",
            "generated_data_source_template_page_id",
        )
        .await?
    };
    sqlx::query(
        "INSERT INTO notes_pages (
            id,
            parent_type,
            parent_data_source_id,
            title,
            properties
         )
         VALUES (?, 'data_source_id', ?, ?, ?)",
    )
    .bind(&page_id)
    .bind(data_source_id)
    .bind(&title)
    .bind(properties.to_string())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create notes row from data source template: {e}"))?;
    let mut reserved_ids = HashSet::from([page_id.clone()]);
    if blocks.is_empty() {
        insert_default_row_block(&mut tx, &page_id, &mut reserved_ids).await?;
    } else {
        insert_template_blocks_as_page_blocks(&mut tx, &page_id, &blocks, &mut reserved_ids)
            .await?;
    }
    data_sources::relations::replace_row_relation_links_tx(
        &mut tx,
        data_source_id,
        &page_id,
        &schema_properties,
        &properties,
        true,
    )
    .await?;
    data_sources::rollups::invalidate_rollup_cache_for_data_source_tx(&mut tx, data_source_id)
        .await?;
    touch_data_source_tx(&mut tx, data_source_id, &data_source.database_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source template apply: {e}"))?;
    reads::load_page(pool, &page_id).await
}

pub async fn update_data_source_template(
    pool: &SqlitePool,
    data_source_id: &str,
    template_id: &str,
    update: NoteDataSourceTemplateUpdate,
) -> Result<NoteDataSourceTemplateDto, String> {
    let data_source_id = data_source_id.trim();
    let template_id = normalize_uuid(template_id, "template_id")?;
    require_uuid(data_source_id, "data_source_id")?;
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
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    if name.is_none() && source_page_id.is_none() && update.is_default.is_none() {
        return Err("template update must include a changed field".to_string());
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source template update: {e}"))?;
    let data_source = load_active_data_source_tx(&mut tx, data_source_id).await?;
    load_template_row_tx(&mut tx, data_source_id, &template_id).await?;
    if let Some(is_default) = update.is_default {
        if is_default {
            clear_default_template_tx(&mut tx, data_source_id).await?;
        }
        sqlx::query(
            "UPDATE notes_data_source_templates
             SET is_default = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND data_source_id = ?",
        )
        .bind(if is_default { 1 } else { 0 })
        .bind(&template_id)
        .bind(data_source_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes data source template default: {e}"))?;
    }
    if let Some(name) = &name {
        sqlx::query(
            "UPDATE notes_data_source_templates
             SET name = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND data_source_id = ?",
        )
        .bind(name)
        .bind(&template_id)
        .bind(data_source_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("rename notes data source template: {e}"))?;
    }
    if let Some(source_page_id) = &source_page_id {
        let row = load_active_row_page_tx(&mut tx, data_source_id, source_page_id).await?;
        let schema_properties = parse_json(&data_source.properties, "data source properties")?;
        let row_properties = parse_json(&row.properties, "row page properties")?;
        let (_, properties) = data_sources::rows::row_page_properties(
            &schema_properties,
            &row.title,
            Some(&row_properties_for_schema(
                &schema_properties,
                &row_properties,
            )?),
        )?;
        sqlx::query(
            "UPDATE notes_data_source_templates
             SET source_page_id = ?,
                 properties = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND data_source_id = ?",
        )
        .bind(source_page_id)
        .bind(properties.to_string())
        .bind(&template_id)
        .bind(data_source_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes data source template snapshot: {e}"))?;
        replace_template_blocks_from_page(&mut tx, &template_id, source_page_id).await?;
    }
    touch_data_source_tx(&mut tx, data_source_id, &data_source.database_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source template update: {e}"))?;
    get_data_source_template(pool, data_source_id, &template_id).await
}

pub async fn duplicate_data_source_template(
    pool: &SqlitePool,
    data_source_id: &str,
    template_id: &str,
    request: NoteDataSourceTemplateDuplicate,
) -> Result<NoteDataSourceTemplateDto, String> {
    let data_source_id = data_source_id.trim();
    let template_id = normalize_uuid(template_id, "template_id")?;
    let duplicate_id = normalize_uuid(&request.id, "id")?;
    let duplicate_name = normalize_template_name(&request.name)?;
    require_uuid(data_source_id, "data_source_id")?;
    let is_default = request.is_default.unwrap_or(false);
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source template duplicate: {e}"))?;
    let data_source = load_active_data_source_tx(&mut tx, data_source_id).await?;
    let source = load_template_row_tx(&mut tx, data_source_id, &template_id).await?;
    let blocks = load_template_block_rows_tx(&mut tx, &template_id).await?;
    if is_default {
        clear_default_template_tx(&mut tx, data_source_id).await?;
    }
    sqlx::query(
        "INSERT INTO notes_data_source_templates (
            id,
            data_source_id,
            source_page_id,
            name,
            properties,
            is_default
         )
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&duplicate_id)
    .bind(data_source_id)
    .bind(&source.source_page_id)
    .bind(&duplicate_name)
    .bind(&source.properties)
    .bind(if is_default { 1 } else { 0 })
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("duplicate notes data source template: {e}"))?;
    for block in &blocks {
        insert_template_block_snapshot(&mut tx, &duplicate_id, block).await?;
    }
    touch_data_source_tx(&mut tx, data_source_id, &data_source.database_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source template duplicate: {e}"))?;
    get_data_source_template(pool, data_source_id, &duplicate_id).await
}

pub async fn delete_data_source_template(
    pool: &SqlitePool,
    data_source_id: &str,
    template_id: &str,
) -> Result<String, String> {
    require_uuid(data_source_id, "data_source_id")?;
    let template_id = normalize_uuid(template_id, "template_id")?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source template delete: {e}"))?;
    crate::project_history::mark_data_source_dirty_tx(
        &mut tx,
        data_source_id,
        "Deleted database template",
        true,
    )
    .await?;
    let result = sqlx::query(
        "DELETE FROM notes_data_source_templates
         WHERE id = ? AND data_source_id = ?",
    )
    .bind(&template_id)
    .bind(data_source_id.trim())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("delete notes data source template: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("notes data source template not found".to_string());
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source template delete: {e}"))?;
    Ok(template_id)
}

async fn get_data_source_template(
    pool: &SqlitePool,
    data_source_id: &str,
    template_id: &str,
) -> Result<NoteDataSourceTemplateDto, String> {
    let row = sqlx::query_as::<_, NoteDataSourceTemplateRow>(
        "SELECT
            template.id,
            template.data_source_id,
            template.source_page_id,
            template.name,
            template.properties,
            template.is_default,
            COUNT(block.id) AS block_count,
            template.created_time,
            template.last_edited_time
         FROM notes_data_source_templates AS template
         LEFT JOIN notes_data_source_template_blocks AS block ON block.template_id = template.id
         WHERE template.id = ? AND template.data_source_id = ?
         GROUP BY template.id",
    )
    .bind(template_id)
    .bind(data_source_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes data source template: {e}"))?
    .ok_or_else(|| "notes data source template not found".to_string())?;
    NoteDataSourceTemplateDto::new(row)
}

async fn load_template_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    template_id: &str,
) -> Result<NoteDataSourceTemplateRow, String> {
    sqlx::query_as::<_, NoteDataSourceTemplateRow>(
        "SELECT
            template.id,
            template.data_source_id,
            template.source_page_id,
            template.name,
            template.properties,
            template.is_default,
            COUNT(block.id) AS block_count,
            template.created_time,
            template.last_edited_time
         FROM notes_data_source_templates AS template
         LEFT JOIN notes_data_source_template_blocks AS block ON block.template_id = template.id
         WHERE template.id = ? AND template.data_source_id = ?
         GROUP BY template.id",
    )
    .bind(template_id)
    .bind(data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes data source template: {e}"))?
    .ok_or_else(|| "notes data source template not found".to_string())
}

struct DataSourceTemplateOwner {
    database_id: String,
    properties: String,
}

impl<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> for DataSourceTemplateOwner {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            database_id: row.try_get("database_id")?,
            properties: row.try_get("properties")?,
        })
    }
}

async fn load_active_data_source_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<DataSourceTemplateOwner, String> {
    sqlx::query_as::<_, DataSourceTemplateOwner>(
        "SELECT data_source.database_id,
                data_source.properties
         FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE data_source.id = ?
           AND data_source.in_trash = 0
           AND database.in_trash = 0",
    )
    .bind(data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes data source for template: {e}"))?
    .ok_or_else(|| "data source not found".to_string())
}

async fn load_active_row_page_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    page_id: &str,
) -> Result<NotePageRow, String> {
    sqlx::query_as::<_, NotePageRow>(
        "SELECT page.*
         FROM notes_pages AS page
         WHERE page.id = ?
           AND page.parent_type = 'data_source_id'
           AND page.parent_data_source_id = ?
           AND page.in_trash = 0
           AND page.archived = 0",
    )
    .bind(page_id)
    .bind(data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes data source template row page: {e}"))?
    .ok_or_else(|| "row page not found".to_string())
}

async fn replace_template_blocks_from_page(
    tx: &mut Transaction<'_, Sqlite>,
    template_id: &str,
    source_page_id: &str,
) -> Result<(), String> {
    sqlx::query("DELETE FROM notes_data_source_template_blocks WHERE template_id = ?")
        .bind(template_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear notes data source template blocks: {e}"))?;
    let source_blocks = load_page_block_rows_tx(tx, source_page_id).await?;
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

async fn load_page_block_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<Vec<NoteBlockRow>, String> {
    sqlx::query_as::<_, NoteBlockRow>(
        "SELECT id,
                page_id,
                parent_type,
                parent_page_id,
                parent_block_id,
                has_children,
                in_trash,
                type AS block_type,
                payload,
                plain_text,
                sort_order,
                source_provider,
                source_object_id,
                source_last_edited_time,
                created_time,
                last_edited_time
         FROM notes_blocks
         WHERE page_id = ? AND in_trash = 0
         ORDER BY CASE parent_type WHEN 'page_id' THEN 0 ELSE 1 END,
                  sort_order ASC,
                  id ASC",
    )
    .bind(page_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load notes data source template source blocks: {e}"))
}

async fn insert_source_block_snapshot(
    tx: &mut Transaction<'_, Sqlite>,
    template_id: &str,
    block: &NoteBlockRow,
) -> Result<(), String> {
    let mut payload: Value = serde_json::from_str(&block.payload)
        .map_err(|e| format!("parse template source block: {e}"))?;
    if block.block_type == "child_page" {
        payload[writes::database_copy::TEMPLATE_PAGE_REFERENCE] = block.id.clone().into();
    }
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
        "INSERT INTO notes_data_source_template_blocks (
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
    .bind(payload.to_string())
    .bind(&block.plain_text)
    .bind(block.sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("snapshot notes data source template block: {e}"))?;
    Ok(())
}

async fn insert_template_block_snapshot(
    tx: &mut Transaction<'_, Sqlite>,
    template_id: &str,
    block: &NoteDataSourceTemplateBlockRow,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO notes_data_source_template_blocks (
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
    .map_err(|e| format!("copy notes data source template block: {e}"))?;
    Ok(())
}

async fn load_template_block_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    template_id: &str,
) -> Result<Vec<NoteDataSourceTemplateBlockRow>, String> {
    sqlx::query_as::<_, NoteDataSourceTemplateBlockRow>(
        "SELECT template_id,
                id,
                parent_type,
                parent_block_id,
                has_children,
                type AS block_type,
                payload,
                plain_text,
                sort_order,
                created_time,
                last_edited_time
         FROM notes_data_source_template_blocks
         WHERE template_id = ?
         ORDER BY CASE parent_type WHEN 'template' THEN 0 ELSE 1 END,
                  sort_order ASC,
                  id ASC",
    )
    .bind(template_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load notes data source template blocks: {e}"))
}

async fn insert_template_blocks_as_page_blocks(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    blocks: &[NoteDataSourceTemplateBlockRow],
    reserved_ids: &mut HashSet<String>,
) -> Result<(), String> {
    let mut block_ids = HashMap::with_capacity(blocks.len());
    for block in blocks {
        let duplicate_id = next_reserved_uuid(tx, reserved_ids).await?;
        block_ids.insert(block.id.clone(), duplicate_id);
    }
    let project_id = crate::project_history::resolve_project_id_for_page_tx(tx, page_id).await?;
    let mut databases = Vec::new();
    let mut page_copies = Vec::new();
    let mut copied_child_pages = HashSet::new();
    let mut budget = writes::copy_budget::CopyBudget::default();
    let mut copy_context = writes::copy_budget::CopyContext {
        reserved_ids,
        budget: &mut budget,
    };
    for block in blocks {
        let id = &block_ids[&block.id];
        let payload: Value = serde_json::from_str(&block.payload)
            .map_err(|e| format!("parse applied template graph: {e}"))?;
        if block.block_type == "child_database" && payload.get("database_id").is_some() {
            let source = writes::database_copy::load_reference_source(tx, &payload).await?;
            databases.push(
                writes::database_copy::plan_database_copy(
                    tx,
                    &source,
                    id,
                    &mut copy_context,
                    project_id.as_deref(),
                    false,
                )
                .await?,
            );
        } else if block.block_type == "child_page" {
            if let Some(source) = payload
                .get(writes::database_copy::TEMPLATE_PAGE_REFERENCE)
                .and_then(Value::as_str)
            {
                let parent = if block.parent_type == "template" {
                    NoteParent::PageId {
                        page_id: page_id.to_string(),
                    }
                } else {
                    NoteParent::BlockId {
                        block_id: block_ids
                            .get(
                                block
                                    .parent_block_id
                                    .as_deref()
                                    .ok_or("template note has no parent")?,
                            )
                            .cloned()
                            .ok_or("template note parent was not copied")?,
                    }
                };
                page_copies.push(
                    writes::plan_child_page_copy(
                        tx,
                        source,
                        id,
                        parent,
                        false,
                        &mut copy_context,
                        project_id.as_deref(),
                    )
                    .await?,
                );
                copied_child_pages.insert(id.clone());
            }
        }
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
        .map_err(|e| format!("apply notes data source template block: {e}"))?;
        if block.block_type == "child_page" && !copied_child_pages.contains(&duplicate_id) {
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
    for graph in &page_copies {
        writes::insert_child_page_copy(tx, graph).await?;
    }
    for database in &databases {
        sqlx::query("UPDATE notes_blocks SET payload = ? WHERE id = ?")
            .bind(database.payload.to_string())
            .bind(&database.id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("set applied template database identities: {e}"))?;
        writes::database_copy::insert_database_copy(tx, database).await?;
    }
    writes::database_copy::finalize_copies(tx, &databases, &page_copies, &block_ids).await?;
    Ok(())
}

async fn insert_applied_child_page(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    containing_page_id: &str,
    block_parent_type: &str,
    block_parent_block_id: Option<String>,
    block: &NoteDataSourceTemplateBlockRow,
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
    .map_err(|e| format!("create data source template child page: {e}"))?;
    insert_default_row_block(tx, page_id, reserved_ids).await
}

async fn insert_default_row_block(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    reserved_ids: &mut HashSet<String>,
) -> Result<(), String> {
    let block_id = next_reserved_uuid(tx, reserved_ids).await?;
    let payload = writes::default_text_payload("");
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
    .map_err(|e| format!("create data source template row initial block: {e}"))?;
    Ok(())
}

async fn next_reserved_uuid(
    tx: &mut Transaction<'_, Sqlite>,
    reserved_ids: &mut HashSet<String>,
) -> Result<String, String> {
    for _ in 0..32 {
        let id = data_sources::views::generate_uuid_tx(
            tx,
            "generate data source template block id",
            "generated_data_source_template_block_id",
        )
        .await?;
        if reserved_ids.insert(id.clone()) {
            return Ok(id);
        }
    }
    Err("could not reserve a unique note id".to_string())
}

async fn clear_default_template_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_data_source_templates
         SET is_default = 0,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE data_source_id = ? AND is_default = 1",
    )
    .bind(data_source_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("clear notes data source default template: {e}"))?;
    Ok(())
}

async fn touch_data_source_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: &str,
) -> Result<(), String> {
    crate::project_history::mark_data_source_dirty_tx(
        tx,
        data_source_id,
        "Database template",
        false,
    )
    .await?;
    sqlx::query(
        "UPDATE notes_data_sources
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(data_source_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch notes data source after template change: {e}"))?;
    sqlx::query(
        "UPDATE notes_databases
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(database_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch notes database after template change: {e}"))?;
    Ok(())
}

fn row_properties_for_schema(
    schema_properties: &Value,
    row_properties: &Value,
) -> Result<Value, String> {
    let schema = schema_properties
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let row = row_properties
        .as_object()
        .ok_or_else(|| "row properties must be an object".to_string())?;
    let mut filtered = serde_json::Map::new();
    for (key, schema_value) in schema {
        if let Some(value) = row
            .get(key)
            .filter(|value| property_value_matches_schema(schema_value, value))
            .or_else(|| {
                row.values()
                    .find(|value| property_value_matches_schema(schema_value, value))
            })
        {
            filtered.insert(key.clone(), value.clone());
        }
    }
    Ok(Value::Object(filtered))
}

fn remove_title_property(
    schema_properties: &Value,
    row_properties: &mut Value,
) -> Result<(), String> {
    let schema = schema_properties
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let row = row_properties
        .as_object_mut()
        .ok_or_else(|| "row properties must be an object".to_string())?;
    for (key, schema_value) in schema {
        if schema_value.get("type").and_then(Value::as_str) == Some("title") {
            row.remove(key);
        }
    }
    Ok(())
}

fn property_value_matches_schema(schema_value: &Value, value: &Value) -> bool {
    let Some(schema) = schema_value.as_object() else {
        return false;
    };
    let Some(row_value) = value.as_object() else {
        return false;
    };
    row_value.get("id").and_then(Value::as_str) == schema.get("id").and_then(Value::as_str)
        && row_value.get("type").and_then(Value::as_str)
            == schema.get("type").and_then(Value::as_str)
}

fn normalize_uuid(value: &str, label: &str) -> Result<String, String> {
    let trimmed = value.trim();
    require_uuid(trimmed, label)?;
    Ok(trimmed.to_string())
}

fn normalize_template_name(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("template name is required".to_string());
    }
    if trimmed.chars().any(char::is_control) {
        return Err("template name must not contain control characters".to_string());
    }
    if trimmed.chars().count() > MAX_TEMPLATE_NAME_CHARS {
        return Err("template name is too long".to_string());
    }
    Ok(trimmed.to_string())
}

fn normalize_row_title(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.chars().any(char::is_control) {
        return Err("title must not contain control characters".to_string());
    }
    Ok(trimmed.to_string())
}

fn page_title_properties(title: &str) -> Value {
    json!({
        "title": {
            "id": "title",
            "type": "title",
            "title": [writes::rich_text(title)]
        }
    })
}

fn parse_json(value: &str, label: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|e| format!("parse {label}: {e}"))
}
