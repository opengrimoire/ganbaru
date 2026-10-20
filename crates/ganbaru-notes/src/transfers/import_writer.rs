use crate::models::{NoteLoadedPage, NoteParent, parent_columns};
use crate::validation::{
    plain_text_from_payload, validate_block_payload, validate_parent, validate_sort_order,
};
use crate::{assets, reads, writes};
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::HashSet;

#[derive(Clone)]
pub(crate) struct ImportBlock {
    pub(crate) id: String,
    pub(crate) block_type: &'static str,
    pub(crate) payload: Value,
    pub(crate) source_object_id: Option<String>,
    pub(crate) source_last_edited_time: Option<String>,
    pub(crate) children: Vec<ImportBlock>,
}

impl ImportBlock {
    pub(crate) fn new(
        block_type: &'static str,
        payload: Value,
        source_object_id: Option<String>,
        children: Vec<ImportBlock>,
    ) -> Self {
        Self::with_source_time(block_type, payload, source_object_id, None, children)
    }

    pub(crate) fn with_source_time(
        block_type: &'static str,
        payload: Value,
        source_object_id: Option<String>,
        source_last_edited_time: Option<String>,
        children: Vec<ImportBlock>,
    ) -> Self {
        Self {
            id: String::new(),
            block_type,
            payload,
            source_object_id,
            source_last_edited_time,
            children,
        }
    }
}

pub(crate) struct ImportedPageCreate<'a> {
    pub(crate) parent: &'a NoteParent,
    pub(crate) after_block_id: Option<&'a str>,
    pub(crate) title: &'a str,
    pub(crate) source_provider: &'a str,
    pub(crate) source_object_id: Option<&'a str>,
    pub(crate) source_workspace_id: Option<&'a str>,
    pub(crate) source_last_edited_time: Option<&'a str>,
    pub(crate) icon: Option<&'a Value>,
    pub(crate) cover: Option<&'a Value>,
    pub(crate) url: Option<&'a str>,
    pub(crate) public_url: Option<&'a str>,
    pub(crate) project_id: Option<&'a str>,
    pub(crate) blocks: Vec<ImportBlock>,
}

pub(crate) struct ImportedRowPageCreate<'a> {
    pub(crate) data_source_id: &'a str,
    pub(crate) title: &'a str,
    pub(crate) properties: &'a Value,
    pub(crate) source_provider: &'a str,
    pub(crate) source_object_id: Option<&'a str>,
    pub(crate) source_workspace_id: Option<&'a str>,
    pub(crate) source_last_edited_time: Option<&'a str>,
    pub(crate) blocks: Vec<ImportBlock>,
}

pub(crate) async fn create_imported_page(
    pool: &SqlitePool,
    mut request: ImportedPageCreate<'_>,
) -> Result<NoteLoadedPage, String> {
    validate_parent(request.parent)?;
    if matches!(request.parent, NoteParent::DataSourceId { .. }) {
        return Err("document imports cannot create database row pages".to_string());
    }
    validate_import_block_depth(&request.blocks)?;
    if request.blocks.is_empty() {
        request.blocks.push(ImportBlock::new(
            "paragraph",
            json!({
                "rich_text": [empty_rich_text()],
                "color": "default"
            }),
            None,
            Vec::new(),
        ));
    }
    if let Some(project_id) = request.project_id {
        crate::project_history::ensure_project_baseline_for_mutation(pool, project_id).await?;
    } else {
        crate::project_history::ensure_parent_baseline_for_mutation(pool, request.parent).await?;
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin {} import: {e}", request.source_provider))?;
    writes::validate_page_parent_exists(&mut tx, request.parent).await?;
    let mut reserved_ids = HashSet::new();
    let page_id = writes::new_note_id(&mut tx, &mut reserved_ids).await?;
    let block_ids = new_note_ids(
        &mut tx,
        &mut reserved_ids,
        count_import_blocks(&request.blocks),
    )
    .await?;
    let mut block_ids = block_ids.into_iter();
    assign_block_ids(&mut request.blocks, &mut block_ids);
    insert_import_page(&mut tx, &page_id, &request).await?;
    insert_import_child_page_block(&mut tx, &page_id, &request).await?;
    let flattened_blocks = flatten_blocks(&page_id, &request.blocks);
    for block in flattened_blocks {
        insert_raw_block_with_id(&mut tx, &request, block).await?;
    }
    sync_import_block_assets(&mut tx, &request.blocks, &page_id).await?;
    crate::project_history::mark_page_dirty_tx(&mut tx, &page_id, request.title, true).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit {} import: {e}", request.source_provider))?;
    reads::load_page(pool, &page_id).await
}

pub(crate) async fn create_imported_row_page(
    pool: &SqlitePool,
    mut request: ImportedRowPageCreate<'_>,
) -> Result<NoteLoadedPage, String> {
    validate_import_block_depth(&request.blocks)?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, request.data_source_id)
        .await?;
    if request.blocks.is_empty() {
        request.blocks.push(ImportBlock::new(
            "paragraph",
            json!({
                "rich_text": [empty_rich_text()],
                "color": "default"
            }),
            None,
            Vec::new(),
        ));
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin {} row import: {e}", request.source_provider))?;
    ensure_data_source_exists(&mut tx, request.data_source_id).await?;
    let mut reserved_ids = HashSet::new();
    let page_id = writes::new_note_id(&mut tx, &mut reserved_ids).await?;
    let block_ids = new_note_ids(
        &mut tx,
        &mut reserved_ids,
        count_import_blocks(&request.blocks),
    )
    .await?;
    let mut block_ids = block_ids.into_iter();
    assign_block_ids(&mut request.blocks, &mut block_ids);
    insert_import_row_page(&mut tx, &page_id, &request).await?;
    let flattened_blocks = flatten_blocks(&page_id, &request.blocks);
    let workspace_parent = NoteParent::Workspace { workspace: true };
    let page_request = ImportedPageCreate {
        parent: &workspace_parent,
        after_block_id: None,
        title: request.title,
        source_provider: request.source_provider,
        source_object_id: request.source_object_id,
        source_workspace_id: request.source_workspace_id,
        source_last_edited_time: request.source_last_edited_time,
        icon: None,
        cover: None,
        url: None,
        public_url: None,
        project_id: None,
        blocks: Vec::new(),
    };
    for block in flattened_blocks {
        insert_raw_block_with_id(&mut tx, &page_request, block).await?;
    }
    sync_import_block_assets(&mut tx, &request.blocks, &page_id).await?;
    crate::project_history::mark_data_source_dirty_tx(
        &mut tx,
        request.data_source_id,
        request.title,
        true,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit {} row import: {e}", request.source_provider))?;
    reads::load_page(pool, &page_id).await
}

/// Refuses an imported block tree deeper than [`writes::MAX_BLOCK_DEPTH`]
/// without recursing, so later recursive passes stay within that bound.
pub(crate) fn validate_import_block_depth(blocks: &[ImportBlock]) -> Result<(), String> {
    let mut pending = vec![(blocks, 1_i64)];
    while let Some((level, depth)) = pending.pop() {
        if level.is_empty() {
            continue;
        }
        if depth > writes::MAX_BLOCK_DEPTH {
            return Err(writes::block_nesting_error());
        }
        pending.extend(
            level
                .iter()
                .map(|block| (block.children.as_slice(), depth + 1)),
        );
    }
    Ok(())
}

pub(crate) fn count_import_blocks(blocks: &[ImportBlock]) -> usize {
    blocks
        .iter()
        .map(|block| 1 + count_import_blocks(&block.children))
        .sum()
}

async fn new_note_ids(
    tx: &mut Transaction<'_, Sqlite>,
    reserved_ids: &mut HashSet<String>,
    count: usize,
) -> Result<Vec<String>, String> {
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
        ids.push(writes::new_note_id(tx, reserved_ids).await?);
    }
    Ok(ids)
}

fn assign_block_ids(blocks: &mut [ImportBlock], ids: &mut impl Iterator<Item = String>) {
    for block in blocks {
        block.id = ids
            .next()
            .expect("generated import block ids must cover every parsed block");
        assign_block_ids(&mut block.children, ids);
    }
}

async fn sync_import_block_assets(
    tx: &mut Transaction<'_, Sqlite>,
    blocks: &[ImportBlock],
    page_id: &str,
) -> Result<(), String> {
    let mut stack = blocks.iter().collect::<Vec<_>>();
    while let Some(block) = stack.pop() {
        assets::sync_block_asset_reference_tx(
            tx,
            &block.id,
            page_id,
            block.block_type,
            &block.payload,
        )
        .await?;
        stack.extend(block.children.iter());
    }
    Ok(())
}

async fn ensure_data_source_exists(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<(), String> {
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
    .map_err(|e| format!("load row import data source: {e}"))?;
    if exists.is_some() {
        Ok(())
    } else {
        Err("data source not found".to_string())
    }
}

async fn insert_import_page(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    request: &ImportedPageCreate<'_>,
) -> Result<(), String> {
    if let Some(project_id) = request.project_id {
        ensure_project_exists(tx, project_id).await?;
    }
    let (parent_type, parent_page_id, parent_block_id) = parent_columns(request.parent);
    sqlx::query(
        "INSERT INTO notes_pages (
            id,
            parent_type,
            parent_page_id,
            parent_block_id,
            title,
            properties,
            source_provider,
            source_object_id,
            source_workspace_id,
            source_last_edited_time,
            icon,
            cover,
            url,
            public_url
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(page_id)
    .bind(parent_type)
    .bind(parent_page_id)
    .bind(parent_block_id)
    .bind(request.title)
    .bind(import_page_properties(request.title, request.project_id)?.to_string())
    .bind(request.source_provider)
    .bind(request.source_object_id)
    .bind(request.source_workspace_id)
    .bind(request.source_last_edited_time)
    .bind(request.icon.map(Value::to_string))
    .bind(request.cover.map(Value::to_string))
    .bind(request.url)
    .bind(request.public_url)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("create {} import page: {e}", request.source_provider))?;
    Ok(())
}

async fn ensure_project_exists(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: &str,
) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1
         FROM projects
         WHERE id = ?",
    )
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load import destination project: {e}"))?;
    if exists.is_some() {
        Ok(())
    } else {
        Err("import destination project not found".to_string())
    }
}

pub(crate) fn normalized_import_project_id(project_id: Option<String>) -> Option<String> {
    project_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn import_page_properties(title: &str, project_id: Option<&str>) -> Result<Value, String> {
    let mut properties = writes::page_title_properties(title);
    let Some(project_id) = project_id else {
        return Ok(properties);
    };
    let object = properties
        .as_object_mut()
        .ok_or_else(|| "page properties must be an object".to_string())?;
    object.insert(
        crate::PAGE_PROJECT_ID_PROPERTY.to_string(),
        Value::String(project_id.to_string()),
    );
    Ok(properties)
}

async fn insert_import_row_page(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    request: &ImportedRowPageCreate<'_>,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO notes_pages (
            id,
            parent_type,
            parent_data_source_id,
            title,
            properties,
            source_provider,
            source_object_id,
            source_workspace_id,
            source_last_edited_time
         )
         VALUES (?, 'data_source_id', ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(page_id)
    .bind(request.data_source_id)
    .bind(request.title)
    .bind(request.properties.to_string())
    .bind(request.source_provider)
    .bind(request.source_object_id)
    .bind(request.source_workspace_id)
    .bind(request.source_last_edited_time)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("create {} import row page: {e}", request.source_provider))?;
    Ok(())
}

async fn insert_import_child_page_block(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    request: &ImportedPageCreate<'_>,
) -> Result<(), String> {
    if matches!(request.parent, NoteParent::Workspace { .. }) {
        return Ok(());
    }
    let parent = writes::resolve_block_parent(tx, request.parent).await?;
    let sort_order = writes::next_sort_orders(tx, &parent, request.after_block_id, 1).await?[0];
    insert_raw_block_with_id(
        tx,
        request,
        RawBlockInsert {
            id: page_id.to_string(),
            page_id: page_id.to_string(),
            parent: RawBlockParent {
                parent_type: parent.parent_type,
                parent_page_id: parent.parent_page_id.clone(),
                parent_block_id: parent.parent_block_id.clone(),
            },
            block_type: "child_page",
            payload: json!({ "title": request.title }),
            sort_order,
            has_children: false,
            source_object_id: request.source_object_id.map(str::to_string),
            source_last_edited_time: request.source_last_edited_time.map(str::to_string),
        },
    )
    .await?;
    writes::refresh_parent_has_children(tx, &parent).await?;
    writes::touch_page(tx, &parent.page_id).await
}

fn flatten_blocks(page_id: &str, blocks: &[ImportBlock]) -> Vec<RawBlockInsert> {
    let mut flattened = Vec::new();
    flatten_children(
        page_id,
        RawBlockParent {
            parent_type: "page_id",
            parent_page_id: Some(page_id.to_string()),
            parent_block_id: None,
        },
        blocks,
        &mut flattened,
    );
    flattened
}

fn flatten_children(
    page_id: &str,
    parent: RawBlockParent,
    blocks: &[ImportBlock],
    output: &mut Vec<RawBlockInsert>,
) {
    for (index, block) in blocks.iter().enumerate() {
        output.push(RawBlockInsert {
            id: block.id.clone(),
            page_id: page_id.to_string(),
            parent: parent.clone(),
            block_type: block.block_type,
            payload: block.payload.clone(),
            sort_order: 1000.0 + index as f64 * 1000.0,
            has_children: !block.children.is_empty(),
            source_object_id: block.source_object_id.clone(),
            source_last_edited_time: block.source_last_edited_time.clone(),
        });
        flatten_children(
            page_id,
            RawBlockParent {
                parent_type: "block_id",
                parent_page_id: None,
                parent_block_id: Some(block.id.clone()),
            },
            &block.children,
            output,
        );
    }
}

async fn insert_raw_block_with_id(
    tx: &mut Transaction<'_, Sqlite>,
    request: &ImportedPageCreate<'_>,
    block: RawBlockInsert,
) -> Result<(), String> {
    validate_sort_order(block.sort_order)?;
    validate_block_payload(block.block_type, &block.payload)?;
    let plain_text = plain_text_from_payload(block.block_type, &block.payload);
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
            source_last_edited_time
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(block.id)
    .bind(block.page_id)
    .bind(block.parent.parent_type)
    .bind(block.parent.parent_page_id)
    .bind(block.parent.parent_block_id)
    .bind(if block.has_children { 1 } else { 0 })
    .bind(block.block_type)
    .bind(block.payload.to_string())
    .bind(plain_text)
    .bind(block.sort_order)
    .bind(request.source_provider)
    .bind(block.source_object_id)
    .bind(block.source_last_edited_time)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert {} import block: {e}", request.source_provider))?;
    Ok(())
}

fn empty_rich_text() -> Value {
    json!({
        "type": "text",
        "text": {
            "content": "",
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
        "plain_text": "",
        "href": null
    })
}

#[derive(Clone)]
struct RawBlockParent {
    parent_type: &'static str,
    parent_page_id: Option<String>,
    parent_block_id: Option<String>,
}

struct RawBlockInsert {
    id: String,
    page_id: String,
    parent: RawBlockParent,
    block_type: &'static str,
    payload: Value,
    sort_order: f64,
    has_children: bool,
    source_object_id: Option<String>,
    source_last_edited_time: Option<String>,
}
