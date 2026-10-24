use crate::block_storage::{
    BlockRowInsert, MAX_BLOCK_ATTRS_BYTES, MAX_BLOCK_TEXT_BYTES, insert_block_row,
    split_rich_text_to_bounds,
};
use crate::models::{NoteLoadedPage, NoteParent, parent_columns};
use crate::validation::{plain_text_from_payload, validate_block_payload, validate_parent};
use crate::{assets, reads, writes};
use ganbaru_sync_contracts::OrderKey;
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
    request.blocks = split_oversized_import_blocks(std::mem::take(&mut request.blocks))?;
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
    let flattened_blocks = flatten_blocks(&page_id, &request.blocks)?;
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
    request.blocks = split_oversized_import_blocks(std::mem::take(&mut request.blocks))?;
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
    let flattened_blocks = flatten_blocks(&page_id, &request.blocks)?;
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

/// Splits every block whose rich text is above the stored text bound into consecutive blocks of
/// the same type, so long imported text becomes several blocks instead of failing the import.
///
/// The first block keeps the source identity and its own attributes. The continuations follow it
/// in text order with neutral attributes, and the last one takes the children so they still come
/// after all of the text. Blocks whose oversized text lives in `cells` or `caption` are left whole
/// and are refused when stored.
pub(crate) fn split_oversized_import_blocks(
    blocks: Vec<ImportBlock>,
) -> Result<Vec<ImportBlock>, String> {
    let mut output = Vec::with_capacity(blocks.len());
    for mut block in blocks {
        block.children = split_oversized_import_blocks(std::mem::take(&mut block.children))?;
        if block.block_type == "unsupported" {
            bound_unsupported_raw(&mut block.payload);
        }
        let Some(rich_text) = block.payload.get("rich_text").and_then(Value::as_array) else {
            output.push(block);
            continue;
        };
        let encoded = Value::Array(rich_text.clone()).to_string().len();
        if encoded <= MAX_BLOCK_TEXT_BYTES / 2 {
            output.push(block);
            continue;
        }
        let mut chunks = split_rich_text_to_bounds(rich_text)?.into_iter();
        let first = chunks.next().unwrap_or_default();
        let mut continuations: Vec<ImportBlock> = chunks
            .map(|chunk| {
                ImportBlock::new(
                    block.block_type,
                    continuation_payload(block.block_type, &block.payload, chunk),
                    None,
                    Vec::new(),
                )
            })
            .collect();
        if let Some(last) = continuations.last_mut() {
            if !block.children.is_empty() {
                last.children = std::mem::take(&mut block.children);
                if let Some(toggleable) = block.payload.get("is_toggleable") {
                    last.payload["is_toggleable"] = toggleable.clone();
                }
            }
        }
        block.payload["rich_text"] = Value::Array(first);
        output.push(block);
        output.extend(continuations);
    }
    Ok(output)
}

/// Payload for a continuation of a split block: the text chunk, the presentation every chunk
/// shares (`color`, indent, code `language`), the attributes the block type requires (a callout
/// or button `icon` and button `actions`), and the neutral value of any required state, so state
/// such as a to-do's `checked` or a code caption stays on the first block.
fn continuation_payload(block_type: &str, source: &Value, chunk: Vec<Value>) -> Value {
    let mut payload = json!({ "rich_text": chunk });
    for key in ["color", "ganbaru_indent", "language"] {
        if let Some(value) = source.get(key) {
            payload[key] = value.clone();
        }
    }
    match block_type {
        "to_do" => payload["checked"] = Value::Bool(false),
        "code" => payload["caption"] = Value::Array(Vec::new()),
        "callout" => payload["icon"] = source.get("icon").cloned().unwrap_or(Value::Null),
        "button" => {
            payload["icon"] = source.get("icon").cloned().unwrap_or(Value::Null);
            if let Some(actions) = source.get("actions") {
                payload["actions"] = actions.clone();
            }
        }
        _ => {}
    }
    payload
}

/// Largest raw source an importer should keep on an `unsupported` block, leaving room in the
/// attribute bound for JSON escaping and the block's other keys.
pub(crate) const MAX_UNSUPPORTED_RAW_BYTES: usize = MAX_BLOCK_ATTRS_BYTES / 2;

/// Replaces the `raw` source of an `unsupported` block whose payload is above the attribute bound
/// with its size and a warning. The raw source is diagnostic context, not editable content.
fn bound_unsupported_raw(payload: &mut Value) {
    let encoded = payload.to_string().len();
    if encoded <= MAX_BLOCK_ATTRS_BYTES {
        return;
    }
    let raw_bytes = payload.get("raw").map_or(0, |raw| raw.to_string().len());
    payload["raw"] = json!({ "omitted_bytes": raw_bytes });
    let warning = Value::String(
        "The raw source was larger than a block can store and was not kept.".to_string(),
    );
    match payload.get_mut("warnings").and_then(Value::as_array_mut) {
        Some(warnings) => warnings.push(warning),
        None => payload["warnings"] = Value::Array(vec![warning]),
    }
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
            child_block_id,
            project_id,
            title,
            source_provider,
            source_object_id,
            source_workspace_id,
            source_last_edited_time,
            icon,
            cover,
            url,
            public_url
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(page_id)
    .bind(parent_type)
    .bind(parent_page_id)
    .bind(parent_block_id)
    .bind(crate::structure::nested_page_block_id(parent_type, page_id))
    .bind(crate::structure::stored_page_project(
        parent_type,
        request.project_id,
    ))
    .bind(request.title)
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
            source_provider,
            source_object_id,
            source_workspace_id,
            source_last_edited_time
         )
         VALUES (?, 'data_source_id', ?, ?, ?, ?, ?, ?)",
    )
    .bind(page_id)
    .bind(request.data_source_id)
    .bind(request.title)
    .bind(request.source_provider)
    .bind(request.source_object_id)
    .bind(request.source_workspace_id)
    .bind(request.source_last_edited_time)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("create {} import row page: {e}", request.source_provider))?;
    crate::page_property_storage::write_page_properties(tx, page_id, request.properties).await
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
    let order_key = writes::next_order_keys(tx, &parent, request.after_block_id, 1)
        .await?
        .remove(0);
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
            order_key,
            source_object_id: request.source_object_id.map(str::to_string),
            source_last_edited_time: request.source_last_edited_time.map(str::to_string),
        },
    )
    .await?;
    writes::touch_parent_block(tx, &parent).await?;
    writes::touch_page(tx, &parent.page_id).await
}

fn flatten_blocks(page_id: &str, blocks: &[ImportBlock]) -> Result<Vec<RawBlockInsert>, String> {
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
    )?;
    Ok(flattened)
}

fn flatten_children(
    page_id: &str,
    parent: RawBlockParent,
    blocks: &[ImportBlock],
    output: &mut Vec<RawBlockInsert>,
) -> Result<(), String> {
    for (index, block) in blocks.iter().enumerate() {
        let position =
            u32::try_from(index).map_err(|_| "too many imported sibling blocks".to_string())?;
        let order_key = OrderKey::rank(position)
            .map_err(|error| format!("order imported block: {error}"))?
            .into_string();
        output.push(RawBlockInsert {
            id: block.id.clone(),
            page_id: page_id.to_string(),
            parent: parent.clone(),
            block_type: block.block_type,
            payload: block.payload.clone(),
            order_key,
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
        )?;
    }
    Ok(())
}

async fn insert_raw_block_with_id(
    tx: &mut Transaction<'_, Sqlite>,
    request: &ImportedPageCreate<'_>,
    block: RawBlockInsert,
) -> Result<(), String> {
    validate_block_payload(block.block_type, &block.payload)?;
    let plain_text = plain_text_from_payload(block.block_type, &block.payload);
    insert_block_row(
        tx,
        BlockRowInsert {
            id: &block.id,
            page_id: &block.page_id,
            parent_type: block.parent.parent_type,
            parent_page_id: block.parent.parent_page_id.as_deref(),
            parent_block_id: block.parent.parent_block_id.as_deref(),
            block_type: block.block_type,
            payload: &block.payload,
            plain_text: &plain_text,
            order_key: &block.order_key,
            source_provider: Some(request.source_provider),
            source_object_id: block.source_object_id.as_deref(),
            source_last_edited_time: block.source_last_edited_time.as_deref(),
        },
    )
    .await
    .map_err(|e| format!("insert {} import block: {e}", request.source_provider))
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
    order_key: String,
    source_object_id: Option<String>,
    source_last_edited_time: Option<String>,
}
