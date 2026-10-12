use crate::assets::files::{NotesFileAssetDto, copy_local_import_file_for_block};
use crate::models::{
    NoteNotionExportImportDiagnosticDto, NoteNotionExportImportDto, NoteNotionExportImportRequest,
    NoteNotionExportImportSummary, NoteParent,
};
use crate::transfers::html_import::parse_html;
use crate::transfers::import_writer::{
    ImportBlock, ImportedPageCreate, count_import_blocks, create_imported_page,
    normalized_import_project_id,
};
use crate::transfers::markdown_import::parse_markdown;
use crate::transfers::notion_api_import::convert::{ConvertedNotionDataSource, ConvertedNotionRow};
use crate::transfers::notion_api_import::writer::{
    create_imported_notion_data_source, rebuild_indexes_after_import,
};
use crate::validation::validate_parent;
use csv::{
    MAX_CSV_COLUMNS, MAX_CSV_ROWS, csv_property_order, csv_property_schema, csv_row_properties,
    parse_csv,
};
use links::{
    decoded_link_reference, entry_parent_relative, html_media_block_type, is_external_url,
    local_page_link, normalize_title, relative_path, title_and_source_id, trimmed_non_empty,
};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

mod csv;
mod links;

const SOURCE_PROVIDER: &str = "notion_export";
const MAX_EXPORT_FILES: usize = 2_000;
const MAX_EXPORT_DEPTH: usize = 16;
const MAX_TEXT_BYTES: u64 = 1_000_000;
const MAX_CSV_BYTES: u64 = 512 * 1024;

pub async fn import_folder(
    pool: &SqlitePool,
    vault_root: &Path,
    request: NoteNotionExportImportRequest,
) -> Result<NoteNotionExportImportDto, String> {
    import_folder_inner(pool, Some(vault_root), request).await
}

#[cfg(test)]
pub(crate) async fn import_folder_without_file_copy(
    pool: &SqlitePool,
    request: NoteNotionExportImportRequest,
) -> Result<NoteNotionExportImportDto, String> {
    import_folder_inner(pool, None, request).await
}

async fn import_folder_inner(
    pool: &SqlitePool,
    vault_root: Option<&Path>,
    request: NoteNotionExportImportRequest,
) -> Result<NoteNotionExportImportDto, String> {
    let prepared = PreparedExportImport::new(request)?;
    let mut diagnostics = Vec::new();
    let entries = scan_export_entries(&prepared.root, &prepared, &mut diagnostics)?;
    if entries.is_empty() {
        return Err(
            "Notion export folder did not contain importable Markdown, HTML, or CSV files"
                .to_string(),
        );
    }
    let target_map = export_target_map(&entries);
    let mut imported_pages = Vec::new();
    let mut imported_data_sources = Vec::new();
    let mut imported_block_count = 0;
    let mut stats = ImportStats::default();
    let mut consumed_documents = HashSet::new();
    let runtime = RuntimeImportContext { pool, vault_root };
    let lookup = ImportLookup {
        entries: &entries,
        target_map: &target_map,
    };

    for entry in entries
        .iter()
        .filter(|entry| entry.kind == ExportEntryKind::Csv)
    {
        let csv = read_text_entry(entry, MAX_CSV_BYTES)?;
        let parsed = parse_csv(&csv)?;
        let (data_source, rows, consumed) = csv_data_source_from_export(
            &runtime,
            &prepared,
            entry,
            parsed,
            &mut diagnostics,
            &lookup,
            &mut stats,
        )
        .await?;
        consumed_documents.extend(consumed);
        let (pages, data_source_object, block_count) = create_imported_notion_data_source(
            pool,
            &prepared.parent,
            SOURCE_PROVIDER,
            prepared.source_workspace_id.as_deref(),
            prepared.project_id.as_deref(),
            data_source,
            rows,
        )
        .await?;
        imported_block_count += block_count;
        imported_pages.extend(pages);
        imported_data_sources.push(data_source_object);
    }

    for entry in entries.iter().filter(|entry| {
        matches!(
            entry.kind,
            ExportEntryKind::Markdown | ExportEntryKind::Html
        )
    }) {
        if consumed_documents.contains(&entry.relative) {
            continue;
        }
        let ParsedDocument {
            title,
            blocks,
            block_count,
        } = parse_document_entry(
            &runtime,
            &prepared,
            entry,
            &target_map,
            &mut diagnostics,
            &mut stats,
        )
        .await?;
        let page = create_imported_page(
            pool,
            ImportedPageCreate {
                parent: &prepared.parent,
                after_block_id: None,
                title: &title,
                source_provider: SOURCE_PROVIDER,
                source_object_id: Some(&entry.source_id),
                source_workspace_id: prepared.source_workspace_id.as_deref(),
                source_last_edited_time: None,
                icon: None,
                cover: None,
                url: None,
                public_url: None,
                project_id: prepared.project_id.as_deref(),
                blocks,
            },
        )
        .await?;
        imported_block_count += block_count;
        imported_pages.push(page);
    }

    rebuild_indexes_after_import(pool).await?;
    Ok(NoteNotionExportImportDto::new(
        NoteNotionExportImportSummary {
            imported_pages,
            imported_data_sources,
            diagnostics,
            imported_block_count,
            imported_file_count: stats.imported_file_count,
            skipped_file_count: stats.skipped_file_count,
            unsupported_block_count: stats.unsupported_block_count,
        },
    ))
}

struct PreparedExportImport {
    parent: NoteParent,
    root: PathBuf,
    source_workspace_id: Option<String>,
    project_id: Option<String>,
    keep_external_file_references: bool,
    copy_local_file_references: bool,
    import_markdown: bool,
    import_html: bool,
    import_csv: bool,
}

impl PreparedExportImport {
    fn new(request: NoteNotionExportImportRequest) -> Result<Self, String> {
        validate_parent(&request.parent)?;
        if matches!(request.parent, NoteParent::DataSourceId { .. }) {
            return Err(
                "Notion export folders cannot be imported into a database row page".to_string(),
            );
        }
        let root_input = PathBuf::from(request.export_root_path.trim());
        if !root_input.is_absolute() {
            return Err("Notion export folder path must be absolute".to_string());
        }
        let root = fs::canonicalize(&root_input)
            .map_err(|e| format!("inspect Notion export folder: {e}"))?;
        if !root.is_dir() {
            return Err("Notion export path must point to a folder".to_string());
        }
        Ok(Self {
            parent: request.parent,
            root,
            source_workspace_id: request
                .source_workspace_id
                .and_then(|value| trimmed_non_empty(&value)),
            project_id: normalized_import_project_id(request.project_id),
            keep_external_file_references: request.keep_external_file_references.unwrap_or(false),
            copy_local_file_references: request.copy_local_file_references.unwrap_or(true),
            import_markdown: request.import_markdown.unwrap_or(true),
            import_html: request.import_html.unwrap_or(true),
            import_csv: request.import_csv.unwrap_or(true),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExportEntryKind {
    Markdown,
    Html,
    Csv,
}

#[derive(Clone, Debug)]
struct ExportEntry {
    path: PathBuf,
    relative: String,
    source_id: String,
    title: String,
    kind: ExportEntryKind,
}

#[derive(Clone, Debug)]
struct ExportTarget {
    title: String,
}

struct ParsedDocument {
    title: String,
    blocks: Vec<ImportBlock>,
    block_count: i64,
}

struct RuntimeImportContext<'a> {
    pool: &'a SqlitePool,
    vault_root: Option<&'a Path>,
}

struct ImportLookup<'a> {
    entries: &'a [ExportEntry],
    target_map: &'a HashMap<String, ExportTarget>,
}

#[derive(Default)]
struct ImportStats {
    imported_file_count: i64,
    skipped_file_count: i64,
    unsupported_block_count: i64,
}

#[derive(Default)]
struct AssetRewrite {
    by_url: HashMap<String, NotesFileAssetDto>,
    counter: usize,
}

impl AssetRewrite {
    fn placeholder_url(&mut self, asset: NotesFileAssetDto) -> String {
        self.counter += 1;
        let extension = asset
            .relative_path
            .rsplit_once('.')
            .map(|(_, extension)| extension)
            .unwrap_or("bin");
        let url = format!(
            "https://ganbaru.local/notion-export/asset-{}.{}",
            self.counter, extension
        );
        self.by_url.insert(url.clone(), asset);
        url
    }
}

async fn parse_document_entry(
    runtime: &RuntimeImportContext<'_>,
    prepared: &PreparedExportImport,
    entry: &ExportEntry,
    target_map: &HashMap<String, ExportTarget>,
    diagnostics: &mut Vec<NoteNotionExportImportDiagnosticDto>,
    stats: &mut ImportStats,
) -> Result<ParsedDocument, String> {
    let text = read_text_entry(entry, MAX_TEXT_BYTES)?;
    let mut assets = AssetRewrite::default();
    let rewritten = match entry.kind {
        ExportEntryKind::Markdown => {
            rewrite_markdown_references(
                runtime,
                prepared,
                entry,
                target_map,
                diagnostics,
                &mut assets,
            )
            .await
        }
        ExportEntryKind::Html => {
            rewrite_html_references(
                runtime,
                prepared,
                entry,
                target_map,
                diagnostics,
                &mut assets,
            )
            .await
        }
        ExportEntryKind::Csv => text,
    };
    let (title, mut blocks) = match entry.kind {
        ExportEntryKind::Markdown => {
            let plan = parse_markdown(&rewritten);
            push_markdown_diagnostics(&entry.relative, plan.diagnostics, diagnostics);
            (
                plan.title.unwrap_or_else(|| entry.title.clone()),
                plan.blocks,
            )
        }
        ExportEntryKind::Html => {
            let plan = parse_html(&rewritten, prepared.keep_external_file_references);
            push_html_diagnostics(&entry.relative, plan.diagnostics, diagnostics);
            (
                plan.title.unwrap_or_else(|| entry.title.clone()),
                plan.blocks,
            )
        }
        ExportEntryKind::Csv => unreachable!("CSV entries are not parsed as document pages"),
    };
    let copied = apply_asset_rewrites(&mut blocks, &assets.by_url);
    stats.imported_file_count += copied;
    stats.unsupported_block_count += count_unsupported_blocks(&blocks);
    stats.skipped_file_count += skipped_local_file_count(diagnostics, &entry.relative);
    Ok(ParsedDocument {
        title,
        block_count: count_import_blocks(&blocks) as i64,
        blocks,
    })
}

async fn csv_data_source_from_export(
    runtime: &RuntimeImportContext<'_>,
    prepared: &PreparedExportImport,
    entry: &ExportEntry,
    rows: Vec<Vec<String>>,
    diagnostics: &mut Vec<NoteNotionExportImportDiagnosticDto>,
    lookup: &ImportLookup<'_>,
    stats: &mut ImportStats,
) -> Result<
    (
        ConvertedNotionDataSource,
        Vec<ConvertedNotionRow>,
        HashSet<String>,
    ),
    String,
> {
    if rows.is_empty() {
        return Err(format!("CSV export file {} is empty", entry.relative));
    }
    if rows.len() > MAX_CSV_ROWS + 1 {
        return Err(format!(
            "CSV export file {} exceeds the {MAX_CSV_ROWS} row import limit",
            entry.relative
        ));
    }
    let headers = rows[0]
        .iter()
        .map(|header| header.trim().to_string())
        .collect::<Vec<_>>();
    if headers.len() > MAX_CSV_COLUMNS {
        return Err(format!(
            "CSV export file {} exceeds the {MAX_CSV_COLUMNS} column import limit",
            entry.relative
        ));
    }
    let properties = csv_property_schema(&headers);
    let property_order = csv_property_order(&headers);
    let body_candidates = markdown_or_html_entries_near(entry, lookup.entries);
    let mut consumed = HashSet::new();
    let mut converted_rows = Vec::new();
    for (index, row) in rows.iter().skip(1).enumerate() {
        if row.iter().all(|cell| cell.trim().is_empty()) {
            diagnostics.push(NoteNotionExportImportDiagnosticDto::new(
                "empty_csv_row_skipped",
                "info",
                Some(entry.relative.clone()),
                "An empty CSV row was skipped.",
            ));
            continue;
        }
        let title = row
            .first()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Untitled")
            .to_string();
        let row_blocks = if let Some(body_entry) = matching_body_entry(&title, &body_candidates) {
            consumed.insert(body_entry.relative.clone());
            parse_document_entry(
                runtime,
                prepared,
                body_entry,
                lookup.target_map,
                diagnostics,
                stats,
            )
            .await?
            .blocks
        } else {
            Vec::new()
        };
        converted_rows.push(ConvertedNotionRow {
            source_id: format!("{}#row-{}", entry.source_id, index + 2),
            title: title.clone(),
            properties: csv_row_properties(&headers, row),
            last_edited_time: None,
            blocks: row_blocks,
            comments: Vec::new(),
        });
    }
    Ok((
        ConvertedNotionDataSource {
            source_id: entry.source_id.clone(),
            title: entry.title.clone(),
            icon: None,
            url: None,
            last_edited_time: None,
            properties,
            property_order,
        },
        converted_rows,
        consumed,
    ))
}

fn scan_export_entries(
    root: &Path,
    prepared: &PreparedExportImport,
    diagnostics: &mut Vec<NoteNotionExportImportDiagnosticDto>,
) -> Result<Vec<ExportEntry>, String> {
    let mut entries = Vec::new();
    collect_export_entries(root, root, prepared, diagnostics, 0, &mut entries)?;
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(entries)
}

fn collect_export_entries(
    root: &Path,
    dir: &Path,
    prepared: &PreparedExportImport,
    diagnostics: &mut Vec<NoteNotionExportImportDiagnosticDto>,
    depth: usize,
    entries: &mut Vec<ExportEntry>,
) -> Result<(), String> {
    if depth > MAX_EXPORT_DEPTH {
        diagnostics.push(NoteNotionExportImportDiagnosticDto::new(
            "folder_depth_skipped",
            "warning",
            Some(relative_path(root, dir)),
            "A nested folder was skipped because it exceeded the Notion export import depth limit.",
        ));
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("read Notion export folder: {e}"))? {
        let entry = entry.map_err(|e| format!("read Notion export entry: {e}"))?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if name.starts_with('.') {
            diagnostics.push(NoteNotionExportImportDiagnosticDto::new(
                "hidden_entry_skipped",
                "info",
                Some(relative_path(root, &path)),
                "Hidden files and folders are skipped during Notion export import.",
            ));
            continue;
        }
        if path.is_dir() {
            collect_export_entries(root, &path, prepared, diagnostics, depth + 1, entries)?;
            continue;
        }
        if entries.len() >= MAX_EXPORT_FILES {
            return Err(format!(
                "Notion export folder import supports up to {MAX_EXPORT_FILES} importable files"
            ));
        }
        let Some(kind) = entry_kind(&path, prepared) else {
            continue;
        };
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("index.html"))
        {
            diagnostics.push(NoteNotionExportImportDiagnosticDto::new(
                "sitemap_skipped",
                "info",
                Some(relative_path(root, &path)),
                "The Notion workspace sitemap was skipped instead of being imported as a note.",
            ));
            continue;
        }
        let relative = relative_path(root, &path);
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Untitled");
        let (title, source_id) = title_and_source_id(stem, &relative);
        entries.push(ExportEntry {
            path,
            relative,
            source_id,
            title,
            kind,
        });
    }
    Ok(())
}

fn entry_kind(path: &Path, prepared: &PreparedExportImport) -> Option<ExportEntryKind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "md" | "markdown" if prepared.import_markdown => Some(ExportEntryKind::Markdown),
        "html" | "htm" if prepared.import_html => Some(ExportEntryKind::Html),
        "csv" if prepared.import_csv => Some(ExportEntryKind::Csv),
        _ => None,
    }
}

fn export_target_map(entries: &[ExportEntry]) -> HashMap<String, ExportTarget> {
    entries
        .iter()
        .map(|entry| {
            (
                entry.relative.clone(),
                ExportTarget {
                    title: entry.title.clone(),
                },
            )
        })
        .collect()
}

fn read_text_entry(entry: &ExportEntry, max_bytes: u64) -> Result<String, String> {
    let metadata = fs::metadata(&entry.path)
        .map_err(|e| format!("inspect Notion export file {}: {e}", entry.relative))?;
    if !metadata.is_file() {
        return Err(format!(
            "Notion export entry {} is not a file",
            entry.relative
        ));
    }
    if metadata.len() > max_bytes {
        return Err(format!(
            "Notion export file {} exceeds the import size limit",
            entry.relative
        ));
    }
    fs::read_to_string(&entry.path)
        .map_err(|e| format!("read Notion export file {}: {e}", entry.relative))
}

async fn rewrite_markdown_references(
    runtime: &RuntimeImportContext<'_>,
    prepared: &PreparedExportImport,
    entry: &ExportEntry,
    target_map: &HashMap<String, ExportTarget>,
    diagnostics: &mut Vec<NoteNotionExportImportDiagnosticDto>,
    assets: &mut AssetRewrite,
) -> String {
    let Ok(text) = read_text_entry(entry, MAX_TEXT_BYTES) else {
        return String::new();
    };
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(open_bracket) = text[cursor..].find('[') {
        let start = cursor + open_bracket;
        output.push_str(&text[cursor..start]);
        let is_image = start > 0 && text.as_bytes()[start - 1] == b'!';
        if is_image && output.ends_with('!') {
            output.pop();
        }
        let Some(label_end_offset) = text[start..].find("](") else {
            output.push_str(&text[start..start + 1]);
            cursor = start + 1;
            continue;
        };
        let label_end = start + label_end_offset;
        let url_start = label_end + 2;
        let Some(url_end_offset) = text[url_start..].find(')') else {
            output.push_str(&text[start..start + 1]);
            cursor = start + 1;
            continue;
        };
        let url_end = url_start + url_end_offset;
        let label = &text[start + 1..label_end];
        let url = text[url_start..url_end].trim();
        if is_image {
            let next = rewrite_media_reference(
                runtime,
                prepared,
                entry,
                url,
                "image",
                diagnostics,
                assets,
            )
            .await;
            output.push_str(&format!(
                "![{label}]({})",
                next.unwrap_or_else(|| url.to_string())
            ));
        } else if let Some(local_url) = local_page_link(entry, target_map, url) {
            output.push_str(&format!("[{label}]({local_url})"));
        } else {
            output.push_str(&text[start..url_end + 1]);
        }
        cursor = url_end + 1;
    }
    output.push_str(&text[cursor..]);
    output
}

async fn rewrite_html_references(
    runtime: &RuntimeImportContext<'_>,
    prepared: &PreparedExportImport,
    entry: &ExportEntry,
    target_map: &HashMap<String, ExportTarget>,
    diagnostics: &mut Vec<NoteNotionExportImportDiagnosticDto>,
    assets: &mut AssetRewrite,
) -> String {
    let Ok(text) = read_text_entry(entry, MAX_TEXT_BYTES) else {
        return String::new();
    };
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let Some(attr_start_offset) = text[index..].find(['h', 's']) else {
            output.push_str(&text[index..]);
            break;
        };
        let attr_start = index + attr_start_offset;
        output.push_str(&text[index..attr_start]);
        let rest = &text[attr_start..];
        let attr = if rest.starts_with("href=") {
            "href"
        } else if rest.starts_with("src=") {
            "src"
        } else {
            output.push_str(&text[attr_start..attr_start + 1]);
            index = attr_start + 1;
            continue;
        };
        let quote_index = attr_start + attr.len() + 1;
        let Some(quote) = text.as_bytes().get(quote_index).copied() else {
            output.push_str(&text[attr_start..]);
            break;
        };
        if quote != b'"' && quote != b'\'' {
            output.push_str(&text[attr_start..quote_index]);
            index = quote_index;
            continue;
        }
        let value_start = quote_index + 1;
        let Some(value_end_offset) = text[value_start..].find(quote as char) else {
            output.push_str(&text[attr_start..]);
            break;
        };
        let value_end = value_start + value_end_offset;
        let value = &text[value_start..value_end];
        let rewritten = if attr == "href" {
            local_page_link(entry, target_map, value)
        } else {
            let block_type = html_media_block_type(&text[..attr_start]);
            match block_type {
                Some(block_type) => {
                    rewrite_media_reference(
                        runtime,
                        prepared,
                        entry,
                        value,
                        block_type,
                        diagnostics,
                        assets,
                    )
                    .await
                }
                None => None,
            }
        };
        output.push_str(attr);
        output.push('=');
        output.push(quote as char);
        output.push_str(rewritten.as_deref().unwrap_or(value));
        output.push(quote as char);
        index = value_end + 1;
    }
    output
}

async fn rewrite_media_reference(
    runtime: &RuntimeImportContext<'_>,
    prepared: &PreparedExportImport,
    entry: &ExportEntry,
    reference: &str,
    block_type: &str,
    diagnostics: &mut Vec<NoteNotionExportImportDiagnosticDto>,
    assets: &mut AssetRewrite,
) -> Option<String> {
    if is_external_url(reference) || !prepared.copy_local_file_references {
        return None;
    }
    let Some(vault_root) = runtime.vault_root else {
        diagnostics.push(NoteNotionExportImportDiagnosticDto::new(
            "local_file_reference_skipped",
            "warning",
            Some(entry.relative.clone()),
            "A local file reference was skipped because this import path cannot copy files.",
        ));
        return None;
    };
    let reference = decoded_link_reference(reference);
    let copied = copy_local_import_file_for_block(
        runtime.pool,
        vault_root,
        &prepared.root,
        &entry_parent_relative(entry, &reference),
        block_type,
        Path::new(&reference)
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned),
    )
    .await;
    for diagnostic in &copied.diagnostics {
        diagnostics.push(NoteNotionExportImportDiagnosticDto::new(
            diagnostic.code.clone(),
            diagnostic.severity.clone(),
            Some(entry.relative.clone()),
            diagnostic.message.clone(),
        ));
    }
    if copied.action == "copied_asset" {
        copied.asset.map(|asset| assets.placeholder_url(asset))
    } else {
        None
    }
}

fn apply_asset_rewrites(
    blocks: &mut [ImportBlock],
    assets: &HashMap<String, NotesFileAssetDto>,
) -> i64 {
    let mut copied = 0;
    for block in blocks {
        if let Some(asset) =
            media_external_url(&block.payload).and_then(|url| assets.get(url).cloned())
        {
            block.payload = managed_file_payload(&block.payload, &asset);
            copied += 1;
        }
        copied += apply_asset_rewrites(&mut block.children, assets);
    }
    copied
}

fn media_external_url(payload: &Value) -> Option<&str> {
    (payload.get("type").and_then(Value::as_str) == Some("external"))
        .then(|| payload.get("external")?.get("url")?.as_str())
        .flatten()
}

fn managed_file_payload(existing: &Value, asset: &NotesFileAssetDto) -> Value {
    let caption = existing
        .get("caption")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    json!({
        "caption": caption,
        "type": "file",
        "file": {
            "url": format!("ganbaru-asset:{}", asset.relative_path),
            "name": asset.original_name.clone(),
            "ganbaru_asset_path": asset.relative_path,
            "content_type": asset.content_type,
            "byte_size": asset.byte_size,
            "sha256": asset.sha256
        }
    })
}

fn markdown_or_html_entries_near<'a>(
    csv: &ExportEntry,
    entries: &'a [ExportEntry],
) -> Vec<&'a ExportEntry> {
    let csv_parent = csv.path.parent();
    entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.kind,
                ExportEntryKind::Markdown | ExportEntryKind::Html
            )
        })
        .filter(|entry| entry.path.parent() == csv_parent)
        .collect()
}

fn matching_body_entry<'a>(title: &str, candidates: &'a [&ExportEntry]) -> Option<&'a ExportEntry> {
    let normalized = normalize_title(title);
    candidates
        .iter()
        .copied()
        .find(|entry| normalize_title(&entry.title) == normalized)
}

fn push_markdown_diagnostics(
    source_path: &str,
    diagnostics: Vec<crate::models::NoteMarkdownImportDiagnosticDto>,
    output: &mut Vec<NoteNotionExportImportDiagnosticDto>,
) {
    for diagnostic in diagnostics {
        push_serialized_diagnostic(source_path, "markdown", diagnostic, output);
    }
}

fn push_html_diagnostics(
    source_path: &str,
    diagnostics: Vec<crate::models::NoteHtmlImportDiagnosticDto>,
    output: &mut Vec<NoteNotionExportImportDiagnosticDto>,
) {
    for diagnostic in diagnostics {
        push_serialized_diagnostic(source_path, "html", diagnostic, output);
    }
}

fn push_serialized_diagnostic(
    source_path: &str,
    prefix: &str,
    diagnostic: impl serde::Serialize,
    output: &mut Vec<NoteNotionExportImportDiagnosticDto>,
) {
    let value = serde_json::to_value(diagnostic).unwrap_or_else(|_| json!({}));
    let code = value
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("diagnostic")
        .to_string();
    let severity = value
        .get("severity")
        .and_then(Value::as_str)
        .unwrap_or("warning")
        .to_string();
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Import diagnostic.")
        .to_string();
    output.push(NoteNotionExportImportDiagnosticDto::new(
        format!("{prefix}_{code}"),
        severity,
        Some(source_path.to_string()),
        message,
    ));
}

fn skipped_local_file_count(
    diagnostics: &[NoteNotionExportImportDiagnosticDto],
    source_path: &str,
) -> i64 {
    diagnostics
        .iter()
        .filter(|diagnostic| {
            serde_json::to_value(diagnostic)
                .ok()
                .and_then(|value| {
                    Some(
                        value.get("source_path")?.as_str()? == source_path
                            && value.get("code")?.as_str()?.contains("skipped"),
                    )
                })
                .unwrap_or(false)
        })
        .count() as i64
}

fn count_unsupported_blocks(blocks: &[ImportBlock]) -> i64 {
    blocks
        .iter()
        .map(|block| {
            i64::from(block.block_type == "unsupported") + count_unsupported_blocks(&block.children)
        })
        .sum()
}
