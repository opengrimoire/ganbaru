use crate::models::{
    NoteLoadedPage, NoteNotionApiImportDiagnosticDto, NoteNotionApiImportedObjectDto,
};
use crate::transfers::import_writer::{
    ImportBlock, ImportedPageCreate, ImportedRowPageCreate, count_import_blocks,
    create_imported_page, create_imported_row_page,
};
use crate::transfers::notion_api_import::convert::{
    ConvertedNotionDataSource, ConvertedNotionRow, comment_rich_text,
};
use crate::validation::{
    plain_text_from_payload, validate_block_payload, validate_comment_rich_text,
};
use crate::{assets, data_sources, search, writes};
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::collections::{HashMap, HashSet};

const NOTION_SOURCE_PROVIDER: &str = "notion";

pub(crate) async fn create_imported_notion_page(
    pool: &SqlitePool,
    parent: &crate::models::NoteParent,
    source_workspace_id: Option<&str>,
    project_id: Option<&str>,
    page: crate::transfers::notion_api_import::convert::ConvertedNotionPage,
) -> Result<NoteLoadedPage, String> {
    create_imported_page(
        pool,
        ImportedPageCreate {
            parent,
            after_block_id: None,
            title: &page.title,
            source_provider: NOTION_SOURCE_PROVIDER,
            source_object_id: Some(&page.source_id),
            source_workspace_id,
            source_last_edited_time: page.last_edited_time.as_deref(),
            icon: page.icon.as_ref(),
            cover: page.cover.as_ref(),
            url: page.url.as_deref(),
            public_url: page.public_url.as_deref(),
            project_id,
            blocks: page.blocks,
        },
    )
    .await
}

pub(crate) async fn create_imported_notion_data_source(
    pool: &SqlitePool,
    parent: &crate::models::NoteParent,
    source_provider: &str,
    source_workspace_id: Option<&str>,
    project_id: Option<&str>,
    data_source: ConvertedNotionDataSource,
    rows: Vec<ConvertedNotionRow>,
) -> Result<(Vec<NoteLoadedPage>, NoteNotionApiImportedObjectDto, i64), String> {
    let block = ImportBlock::with_source_time(
        "child_database",
        json!({ "title": data_source.title }),
        Some(data_source.source_id.clone()),
        data_source.last_edited_time.clone(),
        Vec::new(),
    );
    let wrapper_page = create_imported_page(
        pool,
        ImportedPageCreate {
            parent,
            after_block_id: None,
            title: &data_source.title,
            source_provider,
            source_object_id: Some(&data_source.source_id),
            source_workspace_id,
            source_last_edited_time: data_source.last_edited_time.as_deref(),
            icon: data_source.icon.as_ref(),
            cover: None,
            url: data_source.url.as_deref(),
            public_url: None,
            project_id,
            blocks: vec![block],
        },
    )
    .await?;
    let database_block =
        load_imported_database_block(pool, source_provider, &data_source.source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notion data source import persist: {e}"))?;
    let mut reserved_ids = HashSet::new();
    let local_data_source_id = writes::new_note_id(&mut tx, &mut reserved_ids).await?;
    let local_view_id = writes::new_note_id(&mut tx, &mut reserved_ids).await?;
    insert_database_objects(
        &mut tx,
        &database_block,
        &local_data_source_id,
        &local_view_id,
        source_workspace_id,
        &data_source,
    )
    .await?;
    update_child_database_payload(
        &mut tx,
        &database_block.id,
        &data_source.title,
        &local_data_source_id,
        &local_view_id,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit Notion data source import persist: {e}"))?;

    let mut imported_pages = vec![wrapper_page];
    let mut imported_block_count = 1;
    for row in rows {
        let (title, properties) = data_sources::rows::row_page_properties(
            &data_source.properties,
            &row.title,
            Some(&row.properties),
        )?;
        imported_block_count += count_import_blocks(&row.blocks) as i64;
        let imported_row = create_imported_row_page(
            pool,
            ImportedRowPageCreate {
                data_source_id: &local_data_source_id,
                title: &title,
                properties: &properties,
                source_provider,
                source_object_id: Some(&row.source_id),
                source_workspace_id,
                source_last_edited_time: row.last_edited_time.as_deref(),
                blocks: row.blocks,
            },
        )
        .await?;
        imported_pages.push(imported_row);
    }
    refresh_data_source_after_rows(
        pool,
        &local_data_source_id,
        &database_block.id,
        &data_source.properties,
    )
    .await?;
    Ok((
        imported_pages,
        NoteNotionApiImportedObjectDto::new(
            "data_source",
            data_source.source_id,
            local_data_source_id,
            data_source.title,
        ),
        imported_block_count,
    ))
}

pub(crate) async fn import_comments(
    pool: &SqlitePool,
    page_source_id: &str,
    source_workspace_id: Option<&str>,
    comments: &[Value],
    keep_external_file_references: bool,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> Result<i64, String> {
    if comments.is_empty() {
        return Ok(0);
    }
    let local_page_id = local_page_id_for_source(pool, page_source_id).await?;
    let block_map = local_block_ids_for_source_page(pool, &local_page_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notion comments import: {e}"))?;
    let mut reserved_ids = HashSet::new();
    let mut discussion_map: HashMap<String, String> = HashMap::new();
    let mut imported = 0;
    for comment in comments {
        let Some(source_comment_id) = comment.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(source_discussion_id) = comment.get("discussion_id").and_then(Value::as_str)
        else {
            continue;
        };
        let parent = comment_parent(&local_page_id, &block_map, comment);
        let thread_id = if let Some(thread_id) = discussion_map.get(source_discussion_id) {
            thread_id.clone()
        } else {
            let thread_id = writes::new_note_id(&mut tx, &mut reserved_ids).await?;
            insert_imported_comment_thread(
                &mut tx,
                &thread_id,
                &local_page_id,
                &parent,
                source_discussion_id,
                source_workspace_id,
                comment.get("last_edited_time").and_then(Value::as_str),
            )
            .await?;
            discussion_map.insert(source_discussion_id.to_string(), thread_id.clone());
            thread_id
        };
        let rich_text = comment_rich_text(comment);
        if validate_comment_rich_text(&rich_text).is_err() {
            diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
                "empty_comment_skipped",
                "warning",
                Some(source_comment_id.to_string()),
                "A Notion comment had no importable text and was skipped.",
            ));
            continue;
        }
        let attachments = comment_attachments(
            comment,
            keep_external_file_references,
            diagnostics,
            source_comment_id,
        );
        let comment_id = writes::new_note_id(&mut tx, &mut reserved_ids).await?;
        insert_imported_comment(
            &mut tx,
            ImportedNotionComment {
                comment_id: &comment_id,
                thread_id: &thread_id,
                raw_comment: comment,
                rich_text: &rich_text,
                attachments: &attachments,
                source_comment_id,
                source_workspace_id,
            },
        )
        .await?;
        imported += 1;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit Notion comments import: {e}"))?;
    Ok(imported)
}

pub(crate) async fn rebuild_indexes_after_import(pool: &SqlitePool) -> Result<(), String> {
    search::rebuild_index(pool).await?;
    crate::links::backlinks::rebuild_index(pool).await?;
    crate::links::facts::rebuild_index(pool).await?;
    Ok(())
}

struct ImportedDatabaseBlock {
    id: String,
    parent_type: String,
    parent_page_id: Option<String>,
    parent_block_id: Option<String>,
}

struct CommentParent {
    parent_type: &'static str,
    parent_page_id: Option<String>,
    parent_block_id: Option<String>,
}

struct ImportedNotionComment<'a> {
    comment_id: &'a str,
    thread_id: &'a str,
    raw_comment: &'a Value,
    rich_text: &'a [Value],
    attachments: &'a [Value],
    source_comment_id: &'a str,
    source_workspace_id: Option<&'a str>,
}

async fn load_imported_database_block(
    pool: &SqlitePool,
    source_provider: &str,
    source_object_id: &str,
) -> Result<ImportedDatabaseBlock, String> {
    let row = sqlx::query(
        "SELECT id, parent_type, parent_page_id, parent_block_id
         FROM notes_blocks
         WHERE source_provider = ?
           AND source_object_id = ?
           AND type = 'child_database'
           AND in_trash = 0
         ORDER BY created_time DESC, id DESC
         LIMIT 1",
    )
    .bind(source_provider)
    .bind(source_object_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load imported Notion database block: {e}"))?
    .ok_or_else(|| "imported Notion database block not found".to_string())?;
    Ok(ImportedDatabaseBlock {
        id: row.try_get("id").map_err(|e| e.to_string())?,
        parent_type: row.try_get("parent_type").map_err(|e| e.to_string())?,
        parent_page_id: row.try_get("parent_page_id").map_err(|e| e.to_string())?,
        parent_block_id: row.try_get("parent_block_id").map_err(|e| e.to_string())?,
    })
}

async fn insert_database_objects(
    tx: &mut Transaction<'_, Sqlite>,
    block: &ImportedDatabaseBlock,
    data_source_id: &str,
    view_id: &str,
    source_workspace_id: Option<&str>,
    data_source: &ConvertedNotionDataSource,
) -> Result<(), String> {
    let title_rich_text = rich_text_array(&data_source.title);
    let description = Value::Array(Vec::new());
    let icon = data_source.icon.as_ref().map(Value::to_string);
    sqlx::query(
        "INSERT INTO notes_databases (
            id,
            parent_type,
            parent_page_id,
            parent_block_id,
            title,
            title_rich_text,
            description,
            icon,
            is_inline,
            source_provider,
            source_object_id,
            source_workspace_id,
            source_last_edited_time,
            url
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, 'notion', ?, ?, ?, ?)",
    )
    .bind(&block.id)
    .bind(&block.parent_type)
    .bind(&block.parent_page_id)
    .bind(&block.parent_block_id)
    .bind(&data_source.title)
    .bind(title_rich_text.to_string())
    .bind(description.to_string())
    .bind(icon.clone())
    .bind(&data_source.source_id)
    .bind(source_workspace_id)
    .bind(data_source.last_edited_time.as_deref())
    .bind(data_source.url.as_deref())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert imported Notion database: {e}"))?;

    sqlx::query(
        "INSERT INTO notes_data_sources (
            id,
            database_id,
            title,
            title_rich_text,
            description,
            icon,
            properties,
            source_provider,
            source_object_id,
            source_workspace_id,
            source_last_edited_time
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, 'notion', ?, ?, ?)",
    )
    .bind(data_source_id)
    .bind(&block.id)
    .bind(&data_source.title)
    .bind(title_rich_text.to_string())
    .bind(description.to_string())
    .bind(icon)
    .bind(data_source.properties.to_string())
    .bind(&data_source.source_id)
    .bind(source_workspace_id)
    .bind(data_source.last_edited_time.as_deref())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert imported Notion data source: {e}"))?;

    sqlx::query(
        "INSERT INTO notes_database_views (
            id,
            database_id,
            data_source_id,
            name,
            type,
            sorts,
            configuration,
            sort_order,
            source_provider,
            source_object_id,
            source_workspace_id,
            source_last_edited_time,
            url
         )
         VALUES (?, ?, ?, 'Table', 'table', ?, ?, 1000, 'notion', ?, ?, ?, ?)",
    )
    .bind(view_id)
    .bind(&block.id)
    .bind(data_source_id)
    .bind(Value::Array(Vec::new()).to_string())
    .bind(table_view_configuration(&data_source.property_order).to_string())
    .bind(&data_source.source_id)
    .bind(source_workspace_id)
    .bind(data_source.last_edited_time.as_deref())
    .bind(data_source.url.as_deref())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert imported Notion database view: {e}"))?;
    Ok(())
}

async fn update_child_database_payload(
    tx: &mut Transaction<'_, Sqlite>,
    block_id: &str,
    title: &str,
    data_source_id: &str,
    view_id: &str,
) -> Result<(), String> {
    let payload = json!({
        "title": title,
        "database_id": block_id,
        "data_source_id": data_source_id,
        "view_id": view_id
    });
    validate_block_payload("child_database", &payload)?;
    let plain_text = plain_text_from_payload("child_database", &payload);
    sqlx::query(
        "UPDATE notes_blocks
         SET payload = ?,
             plain_text = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(payload.to_string())
    .bind(plain_text)
    .bind(block_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("update imported Notion database block: {e}"))?;
    Ok(())
}

async fn refresh_data_source_after_rows(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: &str,
    schema_properties: &Value,
) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notion data source refresh: {e}"))?;
    assets::sync_data_source_property_asset_references_tx(
        &mut tx,
        data_source_id,
        schema_properties,
    )
    .await?;
    data_sources::rollups::invalidate_rollup_cache_for_data_source_tx(&mut tx, data_source_id)
        .await?;
    sqlx::query(
        "UPDATE notes_data_sources
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(data_source_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("touch imported Notion data source: {e}"))?;
    sqlx::query(
        "UPDATE notes_databases
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(database_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("touch imported Notion database: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit Notion data source refresh: {e}"))
}

async fn local_page_id_for_source(pool: &SqlitePool, source_id: &str) -> Result<String, String> {
    sqlx::query_scalar(
        "SELECT id
         FROM notes_pages
         WHERE source_provider = 'notion'
           AND source_object_id = ?
         ORDER BY created_time DESC, id DESC
         LIMIT 1",
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load imported Notion page: {e}"))?
    .ok_or_else(|| "imported Notion page not found".to_string())
}

async fn local_block_ids_for_source_page(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<HashMap<String, String>, String> {
    let rows = sqlx::query(
        "SELECT source_object_id, id
         FROM notes_blocks
         WHERE page_id = ?
           AND source_provider = 'notion'
           AND source_object_id IS NOT NULL
           AND in_trash = 0",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load imported Notion block map: {e}"))?;
    let mut map = HashMap::new();
    for row in rows {
        let source_object_id: String =
            row.try_get("source_object_id").map_err(|e| e.to_string())?;
        let id: String = row.try_get("id").map_err(|e| e.to_string())?;
        map.insert(source_object_id, id);
    }
    Ok(map)
}

fn comment_parent(
    local_page_id: &str,
    block_map: &HashMap<String, String>,
    comment: &Value,
) -> CommentParent {
    let parent = comment.get("parent").and_then(Value::as_object);
    if parent
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        == Some("block_id")
    {
        if let Some(source_block_id) = parent
            .and_then(|value| value.get("block_id"))
            .and_then(Value::as_str)
        {
            if let Some(local_block_id) = block_map.get(source_block_id) {
                return CommentParent {
                    parent_type: "block_id",
                    parent_page_id: None,
                    parent_block_id: Some(local_block_id.clone()),
                };
            }
        }
    }
    CommentParent {
        parent_type: "page_id",
        parent_page_id: Some(local_page_id.to_string()),
        parent_block_id: None,
    }
}

async fn insert_imported_comment_thread(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    page_id: &str,
    parent: &CommentParent,
    source_discussion_id: &str,
    source_workspace_id: Option<&str>,
    source_last_edited_time: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO notes_comment_threads (
            id,
            page_id,
            parent_type,
            parent_page_id,
            parent_block_id,
            source_provider,
            source_object_id,
            source_workspace_id,
            source_last_edited_time
         )
         VALUES (?, ?, ?, ?, ?, 'notion', ?, ?, ?)",
    )
    .bind(thread_id)
    .bind(page_id)
    .bind(parent.parent_type)
    .bind(parent.parent_page_id.as_deref())
    .bind(parent.parent_block_id.as_deref())
    .bind(source_discussion_id)
    .bind(source_workspace_id)
    .bind(source_last_edited_time)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert imported Notion comment thread: {e}"))?;
    Ok(())
}

async fn insert_imported_comment(
    tx: &mut Transaction<'_, Sqlite>,
    comment: ImportedNotionComment<'_>,
) -> Result<(), String> {
    let plain_text = crate::validation::rich_text_items_plain_text(comment.rich_text);
    let created_by = comment
        .raw_comment
        .get("created_by")
        .and_then(Value::as_object)
        .and_then(|user| user.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("notion-user");
    let display_name = comment
        .raw_comment
        .get("display_name")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "user", "resolved_name": created_by }));
    sqlx::query(
        "INSERT INTO notes_comments (
            id,
            thread_id,
            rich_text,
            plain_text,
            created_by,
            display_name,
            attachments,
            source_provider,
            source_object_id,
            source_workspace_id,
            source_last_edited_time
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, 'notion', ?, ?, ?)",
    )
    .bind(comment.comment_id)
    .bind(comment.thread_id)
    .bind(Value::Array(comment.rich_text.to_vec()).to_string())
    .bind(plain_text)
    .bind(created_by)
    .bind(display_name.to_string())
    .bind(Value::Array(comment.attachments.to_vec()).to_string())
    .bind(comment.source_comment_id)
    .bind(comment.source_workspace_id)
    .bind(
        comment
            .raw_comment
            .get("last_edited_time")
            .and_then(Value::as_str),
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert imported Notion comment: {e}"))?;
    Ok(())
}

fn comment_attachments(
    raw_comment: &Value,
    keep_external_file_references: bool,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
    source_comment_id: &str,
) -> Vec<Value> {
    let Some(attachments) = raw_comment.get("attachments").and_then(Value::as_array) else {
        return Vec::new();
    };
    if keep_external_file_references {
        return attachments
            .iter()
            .filter(|attachment| attachment.is_object())
            .take(100)
            .cloned()
            .collect();
    }
    if !attachments.is_empty() {
        diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
            "comment_attachment_skipped",
            "warning",
            Some(source_comment_id.to_string()),
            "Notion comment attachments were skipped because external file references were disabled.",
        ));
    }
    Vec::new()
}

fn table_view_configuration(property_order: &[String]) -> Value {
    json!({
        "type": "table",
        "table": {
            "property_order": property_order,
            "hidden_property_ids": [],
            "column_widths": {},
            "row_open_mode": "full_page"
        }
    })
}

fn rich_text_array(text: &str) -> Value {
    json!([{
        "type": "text",
        "text": {
            "content": text,
            "link": Value::Null
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
        "href": Value::Null
    }])
}
