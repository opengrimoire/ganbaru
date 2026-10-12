pub mod archive;
pub mod format;
pub mod render;

use self::format::{
    asset_archive_path, database_archive_path, json_file_contents, page_archive_path, slug,
};
use self::render::{HtmlRenderInput, render_page};
use crate::links;
use crate::models::{
    NoteBlockRow, NoteCommentAnchorRow, NoteCommentRow, NoteCommentThreadRow,
    NoteHtmlExportAssetDto, NoteHtmlExportDiagnosticDto, NoteHtmlExportDto, NoteHtmlExportFileDto,
    NoteHtmlExportRequest, NotePageRow,
};
use crate::validation::require_uuid;
use serde_json::{Value, json};
use sqlx::{FromRow, SqlitePool};
use std::collections::{HashMap, HashSet};

const EXPORT_CSS_PATH: &str = "assets/ganbaru-notes-export.css";
const MANIFEST_PATH: &str = "manifest.json";
const PAGE_TREE_DEPTH_LIMIT: i64 = 64;

pub async fn export_page(
    pool: &SqlitePool,
    request: NoteHtmlExportRequest,
) -> Result<NoteHtmlExportDto, String> {
    Ok(build_archive(pool, request).await?.dto)
}

pub async fn export_archive(
    pool: &SqlitePool,
    request: NoteHtmlExportRequest,
) -> Result<HtmlArchive, String> {
    build_archive(pool, request).await
}

#[derive(Clone)]
pub(crate) struct ExportPage {
    pub(crate) row: NotePageRow,
    pub(crate) title: String,
    pub(crate) path: String,
}

pub(crate) struct ExportBlock {
    pub(crate) row: NoteBlockRow,
    pub(crate) payload: Value,
    pub(crate) children: Vec<ExportBlock>,
    pub(crate) child_page_id: Option<String>,
}

pub(crate) struct ExportCommentThread {
    pub(crate) anchor_label: Option<String>,
    pub(crate) comments: Vec<ExportComment>,
}

pub(crate) struct ExportComment {
    pub(crate) id: String,
    pub(crate) rich_text: String,
    pub(crate) author_name: String,
}

pub(crate) struct ExportDatabase {
    pub(crate) path: String,
    value: Value,
    view_count: i64,
}

pub struct HtmlArchive {
    pub dto: NoteHtmlExportDto,
    pub default_file_name: String,
}

#[derive(FromRow)]
struct AssetRow {
    id: String,
    source_path: String,
    content_type: String,
    byte_size: i64,
    sha256: String,
    storage_state: String,
}

async fn build_archive(
    pool: &SqlitePool,
    request: NoteHtmlExportRequest,
) -> Result<HtmlArchive, String> {
    let page_id = request.page_id.trim().to_string();
    require_uuid(&page_id, "page_id")?;
    let include_page_tree = request.include_page_tree.unwrap_or(true);
    let include_comments = request.include_comments.unwrap_or(false);
    let include_resolved_comments = request.include_resolved_comments.unwrap_or(false);
    let include_assets = request.include_assets.unwrap_or(true);
    let include_database_views = request.include_database_views.unwrap_or(true);

    let page_rows = load_pages(pool, &page_id, include_page_tree).await?;
    if page_rows.is_empty() {
        return Err("notes page not found".to_string());
    }
    let mut pages = page_rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let title = if row.title.trim().is_empty() {
                "Untitled".to_string()
            } else {
                row.title.clone()
            };
            let path = page_archive_path(&title, &row.id, index == 0);
            ExportPage { row, title, path }
        })
        .collect::<Vec<_>>();
    pages.sort_by(|left, right| {
        if left.row.id == page_id {
            return std::cmp::Ordering::Less;
        }
        if right.row.id == page_id {
            return std::cmp::Ordering::Greater;
        }
        left.path.cmp(&right.path)
    });

    let page_ids = pages
        .iter()
        .map(|page| page.row.id.clone())
        .collect::<Vec<_>>();
    let block_rows = load_blocks(pool, &page_ids).await?;
    let block_page_ids = block_rows
        .iter()
        .map(|row| (row.id.clone(), row.page_id.clone()))
        .collect::<HashMap<_, _>>();
    let child_page_by_block = pages
        .iter()
        .filter_map(|page| {
            page.row
                .parent_block_id
                .as_ref()
                .map(|block_id| (block_id.clone(), page.row.id.clone()))
        })
        .collect::<HashMap<_, _>>();
    let blocks_by_page = build_blocks_by_page(block_rows, &child_page_by_block)?;
    let child_pages_by_page = child_pages_by_page(&pages, &block_page_ids);
    let comments = if include_comments {
        load_comments(pool, &page_ids, include_resolved_comments).await?
    } else {
        HashMap::new()
    };
    let asset_paths = collect_asset_paths(&pages, &blocks_by_page, &comments);
    let mut diagnostics = Vec::new();
    let assets = load_assets(pool, &asset_paths, include_assets, &mut diagnostics).await?;
    let asset_archive_paths = assets
        .iter()
        .filter(|asset| asset.exported)
        .map(|asset| (asset.source_path.clone(), asset.archive_path.clone()))
        .collect::<HashMap<_, _>>();
    let databases = if include_database_views {
        export_databases(pool, blocks_by_page.values().flatten()).await?
    } else {
        HashMap::new()
    };
    let database_view_count = databases
        .values()
        .map(|database| database.view_count)
        .sum::<i64>();
    let database_files = databases.values().map(database_file).collect::<Vec<_>>();
    let page_paths = pages
        .iter()
        .map(|page| (page.row.id.clone(), page.path.clone()))
        .collect::<HashMap<_, _>>();
    let local_link_resolver = links::local_link_resolver(pool).await?;
    let mut files = Vec::new();
    let mut exported_block_count = 0_i64;
    let mut exported_comment_count = 0_i64;
    for page in &pages {
        let blocks = blocks_by_page
            .get(&page.row.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let child_pages = child_pages_by_page
            .get(&page.row.id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .filter_map(|child_id| pages.iter().find(|candidate| candidate.row.id == *child_id))
            .collect::<Vec<_>>();
        let page_comments = comments.get(&page.row.id).map(Vec::as_slice).unwrap_or(&[]);
        let rendered = render_page(HtmlRenderInput {
            page,
            blocks,
            child_pages: &child_pages,
            comments: page_comments,
            databases: &databases,
            page_paths: &page_paths,
            asset_paths: &asset_archive_paths,
            local_link_resolver: &local_link_resolver,
        });
        diagnostics.extend(rendered.diagnostics);
        exported_block_count += rendered.exported_block_count;
        exported_comment_count += rendered.exported_comment_count;
        files.push(NoteHtmlExportFileDto::new(
            page.path.clone(),
            "text/html; charset=utf-8",
            rendered.html,
        ));
    }
    files.extend(database_files);
    files.push(NoteHtmlExportFileDto::new(
        EXPORT_CSS_PATH,
        "text/css; charset=utf-8",
        export_css(),
    ));
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let exported_asset_count = assets.iter().filter(|asset| asset.exported).count() as i64;
    let manifest_json = manifest_json(ManifestInput {
        root_page_id: &page_id,
        pages: &pages,
        files: &files,
        assets: &assets,
        diagnostics: &diagnostics,
        options: ManifestOptions {
            include_page_tree,
            include_comments,
            include_resolved_comments,
            include_assets,
            include_database_views,
        },
        counts: ManifestCounts {
            exported_block_count,
            exported_comment_count,
            exported_database_view_count: database_view_count,
        },
    })?;
    files.push(NoteHtmlExportFileDto::new(
        MANIFEST_PATH,
        "application/json; charset=utf-8",
        manifest_json.clone(),
    ));
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let default_file_name = format!("{}-html.zip", slug(&pages[0].title, "notes"));
    let dto = NoteHtmlExportDto {
        object: "notes_html_archive_export",
        root_page_id: page_id,
        files,
        assets,
        diagnostics,
        manifest_json,
        exported_page_count: pages.len() as i64,
        exported_block_count,
        exported_asset_count,
        exported_comment_count,
        exported_database_view_count: database_view_count,
    };
    Ok(HtmlArchive {
        dto,
        default_file_name,
    })
}

async fn load_pages(
    pool: &SqlitePool,
    page_id: &str,
    include_tree: bool,
) -> Result<Vec<NotePageRow>, String> {
    if !include_tree {
        return sqlx::query_as::<_, NotePageRow>(
            "SELECT *
             FROM notes_pages
             WHERE id = ?
               AND in_trash = 0
               AND archived = 0",
        )
        .bind(page_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load notes page for HTML export: {e}"));
    }
    sqlx::query_as::<_, NotePageRow>(
        "WITH RECURSIVE page_tree(id, depth) AS (
            SELECT id, 0
            FROM notes_pages
            WHERE id = ?
              AND in_trash = 0
              AND archived = 0
            UNION
            SELECT child.id, page_tree.depth + 1
            FROM notes_pages AS child
            JOIN page_tree ON page_tree.depth < ?
            LEFT JOIN notes_blocks AS parent_block
              ON child.parent_type = 'block_id'
             AND child.parent_block_id = parent_block.id
            WHERE child.in_trash = 0
              AND child.archived = 0
              AND (
                  (child.parent_type = 'page_id' AND child.parent_page_id = page_tree.id)
                  OR (
                      child.parent_type = 'block_id'
                      AND parent_block.page_id = page_tree.id
                      AND parent_block.in_trash = 0
                  )
              )
         )
         SELECT page.*
         FROM notes_pages AS page
         JOIN page_tree ON page_tree.id = page.id
         ORDER BY page_tree.depth ASC, page.title COLLATE NOCASE ASC, page.id ASC",
    )
    .bind(page_id)
    .bind(PAGE_TREE_DEPTH_LIMIT)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes page tree for HTML export: {e}"))
}

async fn load_blocks(pool: &SqlitePool, page_ids: &[String]) -> Result<Vec<NoteBlockRow>, String> {
    if page_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = placeholders(page_ids.len());
    let sql = format!(
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
         WHERE page_id IN ({placeholders})
           AND in_trash = 0
         ORDER BY page_id ASC, sort_order ASC, id ASC"
    );
    let mut query = sqlx::query_as::<_, NoteBlockRow>(&sql);
    for page_id in page_ids {
        query = query.bind(page_id);
    }
    query
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load notes blocks for HTML export: {e}"))
}

fn build_blocks_by_page(
    rows: Vec<NoteBlockRow>,
    child_page_by_block: &HashMap<String, String>,
) -> Result<HashMap<String, Vec<ExportBlock>>, String> {
    let mut by_page = HashMap::<String, Vec<NoteBlockRow>>::new();
    for row in rows {
        by_page.entry(row.page_id.clone()).or_default().push(row);
    }
    let mut output = HashMap::new();
    for (page_id, page_rows) in by_page {
        output.insert(
            page_id.clone(),
            build_block_tree(&page_id, page_rows, child_page_by_block)?,
        );
    }
    Ok(output)
}

fn build_block_tree(
    page_id: &str,
    rows: Vec<NoteBlockRow>,
    child_page_by_block: &HashMap<String, String>,
) -> Result<Vec<ExportBlock>, String> {
    let mut blocks_by_parent = HashMap::<String, Vec<ExportBlock>>::new();
    for row in rows {
        let parent_key = match row.parent_type.as_str() {
            "page_id" => row
                .parent_page_id
                .as_deref()
                .map(page_key)
                .unwrap_or_else(|| page_key("missing")),
            "block_id" => row
                .parent_block_id
                .as_deref()
                .map(block_key)
                .unwrap_or_else(|| block_key("missing")),
            _ => continue,
        };
        let payload = serde_json::from_str(&row.payload)
            .map_err(|e| format!("parse HTML export block payload: {e}"))?;
        let child_page_id = child_page_by_block.get(&row.id).cloned();
        blocks_by_parent
            .entry(parent_key)
            .or_default()
            .push(ExportBlock {
                row,
                payload,
                children: Vec::new(),
                child_page_id,
            });
    }
    let mut roots = blocks_by_parent
        .remove(&page_key(page_id))
        .unwrap_or_default();
    attach_children(&mut roots, &mut blocks_by_parent);
    Ok(roots)
}

fn attach_children(
    blocks: &mut [ExportBlock],
    blocks_by_parent: &mut HashMap<String, Vec<ExportBlock>>,
) {
    for block in blocks {
        block.children = blocks_by_parent
            .remove(&block_key(&block.row.id))
            .unwrap_or_default();
        attach_children(&mut block.children, blocks_by_parent);
    }
}

fn child_pages_by_page(
    pages: &[ExportPage],
    block_page_ids: &HashMap<String, String>,
) -> HashMap<String, Vec<String>> {
    let mut output = HashMap::<String, Vec<String>>::new();
    for page in pages {
        if let Some(parent_page_id) = page.row.parent_page_id.as_ref() {
            output
                .entry(parent_page_id.clone())
                .or_default()
                .push(page.row.id.clone());
        } else if let Some(parent_block_id) = page.row.parent_block_id.as_ref() {
            if let Some(parent_page_id) = block_page_ids.get(parent_block_id) {
                output
                    .entry(parent_page_id.clone())
                    .or_default()
                    .push(page.row.id.clone());
            }
        }
    }
    for child_ids in output.values_mut() {
        child_ids.sort();
    }
    output
}

async fn load_comments(
    pool: &SqlitePool,
    page_ids: &[String],
    include_resolved: bool,
) -> Result<HashMap<String, Vec<ExportCommentThread>>, String> {
    let mut by_page = HashMap::new();
    for page_id in page_ids {
        by_page.insert(
            page_id.clone(),
            load_page_comments(pool, page_id, include_resolved).await?,
        );
    }
    Ok(by_page)
}

async fn load_page_comments(
    pool: &SqlitePool,
    page_id: &str,
    include_resolved: bool,
) -> Result<Vec<ExportCommentThread>, String> {
    let status_clause = if include_resolved {
        ""
    } else {
        "AND status = 'open'"
    };
    let sql = format!(
        "SELECT *
         FROM notes_comment_threads
         WHERE page_id = ?
           {status_clause}
           AND EXISTS (
               SELECT 1
               FROM notes_comments AS comment
               WHERE comment.thread_id = notes_comment_threads.id
                 AND comment.deleted_at IS NULL
           )
         ORDER BY created_time ASC, id ASC"
    );
    let thread_rows = sqlx::query_as::<_, NoteCommentThreadRow>(&sql)
        .bind(page_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load notes comment threads for HTML export: {e}"))?;
    let mut threads = Vec::new();
    for thread in thread_rows {
        let anchor = sqlx::query_as::<_, NoteCommentAnchorRow>(
            "SELECT *
             FROM notes_comment_thread_anchors
             WHERE thread_id = ?",
        )
        .bind(&thread.id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load notes comment anchor for HTML export: {e}"))?;
        let rows = sqlx::query_as::<_, NoteCommentRow>(
            "SELECT *
             FROM notes_comments
             WHERE thread_id = ?
               AND deleted_at IS NULL
             ORDER BY created_time ASC, id ASC",
        )
        .bind(&thread.id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load notes comments for HTML export: {e}"))?;
        let comments = rows
            .into_iter()
            .map(|row| ExportComment {
                id: row.id,
                rich_text: row.rich_text,
                author_name: display_name(&row.display_name),
            })
            .collect();
        threads.push(ExportCommentThread {
            anchor_label: anchor.map(|anchor| anchor.anchor_text),
            comments,
        });
    }
    Ok(threads)
}

fn collect_asset_paths(
    pages: &[ExportPage],
    blocks_by_page: &HashMap<String, Vec<ExportBlock>>,
    comments: &HashMap<String, Vec<ExportCommentThread>>,
) -> HashSet<String> {
    let mut paths = HashSet::new();
    for page in pages {
        for raw in [
            &page.row.properties,
            page.row.icon.as_deref().unwrap_or(""),
            page.row.cover.as_deref().unwrap_or(""),
        ] {
            if let Ok(value) = serde_json::from_str::<Value>(raw) {
                collect_asset_paths_from_value(&value, &mut paths);
            }
        }
    }
    for blocks in blocks_by_page.values() {
        collect_asset_paths_from_blocks(blocks, &mut paths);
    }
    for threads in comments.values() {
        for thread in threads {
            for comment in &thread.comments {
                if let Ok(value) = serde_json::from_str::<Value>(&comment.rich_text) {
                    collect_asset_paths_from_value(&value, &mut paths);
                }
            }
        }
    }
    paths
}

fn collect_asset_paths_from_blocks(blocks: &[ExportBlock], paths: &mut HashSet<String>) {
    for block in blocks {
        collect_asset_paths_from_value(&block.payload, paths);
        collect_asset_paths_from_blocks(&block.children, paths);
    }
}

fn collect_asset_paths_from_value(value: &Value, paths: &mut HashSet<String>) {
    match value {
        Value::String(text) => {
            if let Some(path) = text.strip_prefix("ganbaru-asset:") {
                paths.insert(path.to_string());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_asset_paths_from_value(item, paths);
            }
        }
        Value::Object(map) => {
            if let Some(path) = map.get("ganbaru_asset_path").and_then(Value::as_str) {
                paths.insert(path.to_string());
            }
            for item in map.values() {
                collect_asset_paths_from_value(item, paths);
            }
        }
        _ => {}
    }
}

async fn load_assets(
    pool: &SqlitePool,
    asset_paths: &HashSet<String>,
    include_assets: bool,
    diagnostics: &mut Vec<NoteHtmlExportDiagnosticDto>,
) -> Result<Vec<NoteHtmlExportAssetDto>, String> {
    if asset_paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut sorted_paths = asset_paths.iter().cloned().collect::<Vec<_>>();
    sorted_paths.sort();
    let sql = format!(
        "SELECT id,
                asset_path AS source_path,
                content_type,
                byte_size,
                sha256,
                storage_state
         FROM notes_assets
         WHERE asset_path IN ({})
         ORDER BY asset_path ASC",
        placeholders(sorted_paths.len())
    );
    let mut query = sqlx::query_as::<_, AssetRow>(&sql);
    for path in &sorted_paths {
        query = query.bind(path);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load notes assets for HTML export: {e}"))?;
    let found = rows
        .iter()
        .map(|row| row.source_path.clone())
        .collect::<HashSet<_>>();
    for path in sorted_paths.iter().filter(|path| !found.contains(*path)) {
        diagnostics.push(NoteHtmlExportDiagnosticDto::new(
            "html_export_asset_metadata_missing",
            "warning",
            None::<String>,
            None::<String>,
            None::<String>,
            None::<String>,
            format!("Managed asset metadata was missing for {path}"),
        ));
    }
    Ok(rows
        .into_iter()
        .map(|row| {
            let exported = include_assets && row.storage_state == "available";
            if include_assets && !exported {
                diagnostics.push(NoteHtmlExportDiagnosticDto::new(
                    "html_export_asset_unavailable",
                    "warning",
                    None::<String>,
                    None::<String>,
                    Some(row.id.clone()),
                    None::<String>,
                    "Managed asset is not available on disk and was not exported",
                ));
            }
            NoteHtmlExportAssetDto {
                id: row.id,
                archive_path: asset_archive_path(&row.source_path),
                source_path: row.source_path,
                content_type: row.content_type,
                byte_size: row.byte_size,
                sha256: row.sha256,
                storage_state: row.storage_state,
                exported,
            }
        })
        .collect())
}

async fn export_databases<'a, I>(
    pool: &SqlitePool,
    blocks: I,
) -> Result<HashMap<String, ExportDatabase>, String>
where
    I: Iterator<Item = &'a ExportBlock>,
{
    let mut databases = HashMap::new();
    for block in blocks {
        collect_database_blocks(pool, block, &mut databases).await?;
    }
    Ok(databases)
}

async fn collect_database_blocks(
    pool: &SqlitePool,
    block: &ExportBlock,
    databases: &mut HashMap<String, ExportDatabase>,
) -> Result<(), String> {
    if block.row.block_type == "child_database" {
        if let Some(database) = load_database_export(pool, block).await? {
            databases.insert(block.row.id.clone(), database);
        }
    }
    for child in &block.children {
        Box::pin(collect_database_blocks(pool, child, databases)).await?;
    }
    Ok(())
}

async fn load_database_export(
    pool: &SqlitePool,
    block: &ExportBlock,
) -> Result<Option<ExportDatabase>, String> {
    let Some(database_id) = block
        .payload
        .get("database_id")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Ok(None);
    };
    let title = block
        .payload
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Untitled database");
    let data_source_id = block
        .payload
        .get("data_source_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let views = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            Option<String>,
        ),
    >(
        "SELECT id,
                database_id,
                data_source_id,
                name,
                filter,
                sorts,
                configuration
         FROM notes_database_views
         WHERE database_id = ?
         ORDER BY sort_order ASC, id ASC",
    )
    .bind(&database_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load database views for HTML export: {e}"))?;
    let view_count = views.len() as i64;
    let rows = if data_source_id.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT id, title, properties, last_edited_time
             FROM notes_pages
             WHERE parent_type = 'data_source_id'
               AND parent_data_source_id = ?
               AND in_trash = 0
               AND archived = 0
             ORDER BY title COLLATE NOCASE ASC, id ASC",
        )
        .bind(data_source_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load database rows for HTML export: {e}"))?
    };
    let path = database_archive_path(title, &database_id);
    let value = json!({
        "object": "notes_database_html_export",
        "database_id": database_id,
        "block_id": block.row.id,
        "title": title,
        "data_source_id": data_source_id,
        "views": views.into_iter().map(|view| json!({
            "id": view.0,
            "database_id": view.1,
            "data_source_id": view.2,
            "name": view.3,
            "filter": view.4.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()),
            "sorts": serde_json::from_str::<Value>(&view.5).unwrap_or_else(|_| Value::Array(Vec::new())),
            "configuration": view.6.and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        })).collect::<Vec<_>>(),
        "rows": rows.into_iter().map(|row| json!({
            "id": row.0,
            "title": row.1,
            "properties": serde_json::from_str::<Value>(&row.2).unwrap_or_else(|_| Value::Object(Default::default())),
            "last_edited_time": row.3
        })).collect::<Vec<_>>()
    });
    Ok(Some(ExportDatabase {
        path,
        value,
        view_count,
    }))
}

fn database_file(database: &ExportDatabase) -> NoteHtmlExportFileDto {
    NoteHtmlExportFileDto::new(
        database.path.clone(),
        "application/json; charset=utf-8",
        json_file_contents(&database.value).unwrap_or_else(|_| "{}".to_string()),
    )
}

struct ManifestInput<'a> {
    root_page_id: &'a str,
    pages: &'a [ExportPage],
    files: &'a [NoteHtmlExportFileDto],
    assets: &'a [NoteHtmlExportAssetDto],
    diagnostics: &'a [NoteHtmlExportDiagnosticDto],
    options: ManifestOptions,
    counts: ManifestCounts,
}

struct ManifestOptions {
    include_page_tree: bool,
    include_comments: bool,
    include_resolved_comments: bool,
    include_assets: bool,
    include_database_views: bool,
}

struct ManifestCounts {
    exported_block_count: i64,
    exported_comment_count: i64,
    exported_database_view_count: i64,
}

fn manifest_json(input: ManifestInput<'_>) -> Result<String, String> {
    let value = json!({
        "object": "notes_html_archive_manifest",
        "schema_version": 1,
        "source": "ganbaru-ai-notes",
        "root_page_id": input.root_page_id,
        "options": {
            "include_page_tree": input.options.include_page_tree,
            "include_comments": input.options.include_comments,
            "include_resolved_comments": input.options.include_resolved_comments,
            "include_assets": input.options.include_assets,
            "include_database_views": input.options.include_database_views
        },
        "counts": {
            "pages": input.pages.len(),
            "blocks": input.counts.exported_block_count,
            "assets": input.assets.iter().filter(|asset| asset.exported).count(),
            "comments": input.counts.exported_comment_count,
            "database_views": input.counts.exported_database_view_count,
            "warnings": input.diagnostics.len()
        },
        "pages": input.pages.iter().map(|page| json!({
            "id": page.row.id,
            "title": page.title,
            "path": page.path,
            "parent_type": page.row.parent_type,
            "parent_page_id": page.row.parent_page_id,
            "parent_block_id": page.row.parent_block_id,
            "created_time": page.row.created_time,
            "last_edited_time": page.row.last_edited_time
        })).collect::<Vec<_>>(),
        "files": input.files.iter().map(|file| json!({
            "path": file.path,
            "content_type": file.content_type,
            "byte_size": file.byte_size
        })).collect::<Vec<_>>(),
        "assets": input.assets,
        "diagnostics": input.diagnostics
    });
    json_file_contents(&value)
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(",")
}

fn page_key(page_id: &str) -> String {
    format!("page:{page_id}")
}

fn block_key(block_id: &str) -> String {
    format!("block:{block_id}")
}

fn display_name(raw: &str) -> String {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("resolved_name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn export_css() -> &'static str {
    "body{margin:0;background:#fafafa;color:#1f2328;font:16px/1.55 system-ui,sans-serif}.page{max-width:860px;margin:0 auto;padding:48px 24px}.page-header{border-bottom:1px solid #d8dee4;margin-bottom:28px}.page-header h1{font-size:2.2rem;line-height:1.1;margin:0 0 8px}.page-meta{color:#6e7781;font-size:.9rem}.block{margin:.65rem 0}.blocks :is(h1,h2,h3,h4,h5,h6){font-weight:600;line-height:1.3}.blocks h1{font-size:1.875rem}.blocks h2{font-size:1.5rem}.blocks h3{font-size:1.375rem}.blocks h4{font-size:1.25rem}.blocks h5{font-size:1.125rem}.blocks h6{font-size:1rem}.subpages,.comments,.database,.callout,.warning{border:1px solid #d8dee4;border-radius:8px;background:#fff;padding:12px 14px;margin:16px 0}.subpages ul{margin:.5rem 0 0;padding-left:1.2rem}.callout{display:flex;gap:10px;background:#fff8c5}.warning{color:#9a6700;background:#fff8c5}a{color:#0969da}pre{overflow:auto;border-radius:8px;background:#24292f;color:#f6f8fa;padding:14px}code{font-family:ui-monospace,Menlo,Consolas,monospace}blockquote{border-left:4px solid #d0d7de;margin:1rem 0;padding-left:1rem;color:#57606a}table{border-collapse:collapse;width:100%;margin:1rem 0}td,th{border:1px solid #d8dee4;padding:6px 8px;text-align:left}img,video{max-width:100%;height:auto}.mention{background:#ddf4ff;border-radius:4px;padding:0 3px}.equation{font-family:ui-monospace,Menlo,Consolas,monospace;background:#f6f8fa;border-radius:4px;padding:0 4px}.todo input{margin-right:6px}[data-color$='_background']{background:#fff8c5}[data-color='red']{color:#cf222e}[data-color='blue']{color:#0969da}[data-color='green']{color:#1a7f37}"
}
