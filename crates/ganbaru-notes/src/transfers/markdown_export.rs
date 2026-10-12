pub mod format;

use self::format::{
    append_children, append_indented_children, block_separator, callout_icon_text, code_fence,
    display_name, escape_dollar_math, escape_html_comment, escape_inline, escape_link_label,
    escape_url, indent, inline_code, media_url, normalize_cells, parse_json_array, quote_lines,
    synced_block_label, table_cells,
};
use crate::models::{
    NoteBlockRow, NoteCommentAnchorRow, NoteCommentRow, NoteCommentThreadRow,
    NoteMarkdownExportDiagnosticDto, NoteMarkdownExportDto, NoteMarkdownExportRequest,
};
use crate::validation::{require_uuid, rich_text_items_plain_text};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;

const MAX_EXPORTED_COMMENT_TEXT_CHARS: usize = 2000;
const MAX_BLOCK_INDENT_BYTES: usize = 1024 * 1024;

pub async fn export_page(
    pool: &SqlitePool,
    request: NoteMarkdownExportRequest,
) -> Result<NoteMarkdownExportDto, String> {
    let page_id = request.page_id.trim().to_string();
    require_uuid(&page_id, "page_id")?;
    let (title, cover) = load_active_page_header(pool, &page_id).await?;
    let rows = load_active_page_blocks(pool, &page_id).await?;
    let mut diagnostics = Vec::new();
    if cover.is_some() {
        diagnostics.push(NoteMarkdownExportDiagnosticDto::new(
            "page_cover_omitted", "warning", None::<String>, None::<String>,
            "Page covers are not represented in Markdown. Use graph export to preserve editable cover metadata.",
        ));
    }
    let blocks = build_block_tree(&page_id, rows, &mut diagnostics)?;
    let mut renderer = MarkdownRenderer::new(diagnostics);
    let mut markdown = String::new();
    if request.include_page_title.unwrap_or(true) {
        markdown.push_str("# ");
        markdown.push_str(&escape_inline(&title));
        markdown.push_str("\n\n");
    }
    let body = renderer.render_blocks(&blocks, 0);
    if !body.is_empty() {
        markdown.push_str(&body);
        markdown.push('\n');
    }

    let mut exported_comment_count = 0_i64;
    if request.include_comments.unwrap_or(false) {
        let comments = load_export_comments(
            pool,
            &page_id,
            request.include_resolved_comments.unwrap_or(false),
        )
        .await?;
        exported_comment_count = comments
            .iter()
            .map(|thread| thread.comments.len() as i64)
            .sum();
        let comments_markdown = renderer.render_comment_threads(&comments);
        if !comments_markdown.is_empty() {
            if !markdown.ends_with("\n\n") {
                if !markdown.ends_with('\n') {
                    markdown.push('\n');
                }
                markdown.push('\n');
            }
            markdown.push_str("## Comments\n\n");
            markdown.push_str(&comments_markdown);
            markdown.push('\n');
        }
    }

    Ok(NoteMarkdownExportDto::new(
        page_id,
        markdown,
        renderer.diagnostics,
        renderer.exported_block_count,
        exported_comment_count,
    ))
}

/// Load cover presence with the title so lossy exports can report omitted presentation.
async fn load_active_page_header(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<(String, Option<String>), String> {
    sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT title, cover
         FROM notes_pages
         WHERE id = ?
           AND in_trash = 0
           AND archived = 0",
    )
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes page for markdown export: {e}"))?
    .ok_or_else(|| "notes page not found".to_string())
}

async fn load_active_page_blocks(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<Vec<NoteBlockRow>, String> {
    sqlx::query_as::<_, NoteBlockRow>(
        "SELECT
            id,
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
         WHERE page_id = ?
           AND in_trash = 0
         ORDER BY sort_order ASC, id ASC",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes blocks for markdown export: {e}"))
}

fn build_block_tree(
    page_id: &str,
    rows: Vec<NoteBlockRow>,
    diagnostics: &mut Vec<NoteMarkdownExportDiagnosticDto>,
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
            _ => {
                diagnostics.push(NoteMarkdownExportDiagnosticDto::new(
                    "markdown_export_invalid_parent",
                    "warning",
                    Some(row.id.clone()),
                    None::<String>,
                    "Block has a parent shape that cannot be represented in page markdown",
                ));
                continue;
            }
        };
        let payload = serde_json::from_str(&row.payload)
            .map_err(|e| format!("parse markdown export block payload: {e}"))?;
        blocks_by_parent
            .entry(parent_key)
            .or_default()
            .push(ExportBlock {
                row,
                payload,
                children: Vec::new(),
            });
    }

    let mut roots = blocks_by_parent
        .remove(&page_key(page_id))
        .unwrap_or_default();
    attach_children(&mut roots, &mut blocks_by_parent);
    if !blocks_by_parent.is_empty() {
        diagnostics.push(NoteMarkdownExportDiagnosticDto::new(
            "markdown_export_orphaned_children",
            "warning",
            None::<String>,
            None::<String>,
            "Some child blocks were not reachable from the exported page root",
        ));
    }
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

fn page_key(page_id: &str) -> String {
    format!("page:{page_id}")
}

fn block_key(block_id: &str) -> String {
    format!("block:{block_id}")
}

struct ExportBlock {
    row: NoteBlockRow,
    payload: Value,
    children: Vec<ExportBlock>,
}

struct MarkdownRenderer {
    diagnostics: Vec<NoteMarkdownExportDiagnosticDto>,
    exported_block_count: i64,
}

impl MarkdownRenderer {
    fn new(diagnostics: Vec<NoteMarkdownExportDiagnosticDto>) -> Self {
        Self {
            diagnostics,
            exported_block_count: 0,
        }
    }

    fn render_blocks(&mut self, blocks: &[ExportBlock], depth: usize) -> String {
        let mut output = String::new();
        let mut previous_type: Option<&str> = None;
        for block in blocks {
            let mut rendered = self.render_block(block, depth);
            let extra_indent = block
                .payload
                .get("ganbaru_indent")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if extra_indent > 0 {
                let width = usize::try_from(extra_indent)
                    .ok()
                    .and_then(|value| value.checked_mul(2));
                if let Some(width) = width.filter(|width| {
                    width.saturating_mul(rendered.lines().count()) <= MAX_BLOCK_INDENT_BYTES
                }) {
                    let prefix = " ".repeat(width);
                    rendered = rendered
                        .lines()
                        .map(|line| format!("{prefix}{line}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                } else {
                    self.warn_block(block, "markdown_export_indentation_too_large", "Indentation exceeds the Markdown whitespace export budget; text is retained");
                }
            }
            if rendered.trim().is_empty() {
                continue;
            }
            if !output.is_empty() {
                output.push_str(block_separator(previous_type, &block.row.block_type));
            }
            output.push_str(&rendered);
            previous_type = Some(&block.row.block_type);
        }
        output
    }

    fn render_block(&mut self, block: &ExportBlock, depth: usize) -> String {
        self.exported_block_count += 1;
        match block.row.block_type.as_str() {
            "paragraph" => self.render_paragraph(block, depth),
            "heading_1" | "heading_2" | "heading_3" | "heading_4" | "heading_5" | "heading_6" => {
                self.render_heading(block, depth)
            }
            "bulleted_list_item" => self.render_list_item(block, depth, "-"),
            "numbered_list_item" => self.render_list_item(block, depth, "1."),
            "to_do" => self.render_todo(block, depth),
            "toggle" => self.render_toggle(block, depth, None),
            "callout" => self.render_callout(block, depth),
            "quote" => self.render_quote(block, depth),
            "code" => self.render_code(block),
            "divider" => "---".to_string(),
            "equation" => self.render_equation(block),
            "image" | "video" | "audio" | "file" | "pdf" => self.render_media(block),
            "bookmark" | "embed" | "link_preview" => self.render_external_reference(block),
            "child_page" => self.render_child_page(block),
            "child_database" => self.render_unsupported_block(
                block,
                "markdown_export_child_database_unsupported",
                "Child databases export as placeholders until database markdown export exists",
            ),
            "table" => self.render_table(block),
            "table_row" => self.render_table_row(block),
            "column_list" | "column" | "tab" | "synced_block" | "template" | "button" => {
                self.render_container_fallback(block, depth)
            }
            "breadcrumb" | "table_of_contents" => {
                self.warn_block(
                    block,
                    "markdown_export_generated_block_skipped",
                    "Generated navigation blocks are skipped in markdown export",
                );
                String::new()
            }
            "unsupported" => self.render_unsupported_block(
                block,
                "markdown_export_unsupported_block",
                "Unsupported block content was preserved as an export placeholder",
            ),
            _ => self.render_unsupported_block(
                block,
                "markdown_export_unknown_block",
                "Unknown block type was preserved as an export placeholder",
            ),
        }
    }

    fn render_paragraph(&mut self, block: &ExportBlock, depth: usize) -> String {
        let text = self.render_payload_rich_text(block, &block.payload);
        append_children(text, self.render_blocks(&block.children, depth + 1), depth)
    }

    fn render_heading(&mut self, block: &ExportBlock, depth: usize) -> String {
        let text = self.render_payload_rich_text(block, &block.payload);
        let level = match block.row.block_type.as_str() {
            "heading_1" => 1,
            "heading_2" => 2,
            "heading_3" => 3,
            "heading_4" => 4,
            "heading_5" => 5,
            _ => 6,
        };
        let heading = format!("{} {}", "#".repeat(level), text);
        if block
            .payload
            .get("is_toggleable")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return self.render_toggle(block, depth, Some(heading));
        }
        append_children(
            heading,
            self.render_blocks(&block.children, depth + 1),
            depth,
        )
    }

    fn render_list_item(&mut self, block: &ExportBlock, depth: usize, marker: &str) -> String {
        let text = self.render_payload_rich_text(block, &block.payload);
        let line = format!("{}{marker} {text}", indent(depth));
        append_indented_children(line, self.render_blocks(&block.children, depth + 1))
    }

    fn render_todo(&mut self, block: &ExportBlock, depth: usize) -> String {
        let checked = block
            .payload
            .get("checked")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let marker = if checked { "- [x]" } else { "- [ ]" };
        let text = self.render_payload_rich_text(block, &block.payload);
        let line = format!("{}{marker} {text}", indent(depth));
        append_indented_children(line, self.render_blocks(&block.children, depth + 1))
    }

    fn render_toggle(
        &mut self,
        block: &ExportBlock,
        depth: usize,
        summary: Option<String>,
    ) -> String {
        let summary =
            summary.unwrap_or_else(|| self.render_payload_rich_text(block, &block.payload));
        let children = self.render_blocks(&block.children, depth + 1);
        if children.is_empty() {
            return format!("<details><summary>{summary}</summary></details>");
        }
        format!("<details><summary>{summary}</summary>\n\n{children}\n\n</details>")
    }

    fn render_callout(&mut self, block: &ExportBlock, depth: usize) -> String {
        let mut text = self.render_payload_rich_text(block, &block.payload);
        if let Some(icon) = callout_icon_text(&block.payload) {
            text = format!("{icon} {text}");
        }
        let children = self.render_blocks(&block.children, depth + 1);
        let combined = if children.is_empty() {
            format!("[!NOTE] {text}")
        } else {
            format!("[!NOTE] {text}\n\n{children}")
        };
        quote_lines(&combined)
    }

    fn render_quote(&mut self, block: &ExportBlock, depth: usize) -> String {
        let text = self.render_payload_rich_text(block, &block.payload);
        let children = self.render_blocks(&block.children, depth + 1);
        let combined = if children.is_empty() {
            text
        } else if text.is_empty() {
            children
        } else {
            format!("{text}\n\n{children}")
        };
        quote_lines(&combined)
    }

    fn render_code(&mut self, block: &ExportBlock) -> String {
        let language = block
            .payload
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let code = block
            .payload
            .get("rich_text")
            .and_then(Value::as_array)
            .map(|items| rich_text_items_plain_text(items))
            .unwrap_or_default();
        let fence = code_fence(&code);
        format!("{fence}{language}\n{code}\n{fence}")
    }

    fn render_equation(&mut self, block: &ExportBlock) -> String {
        let expression = block
            .payload
            .get("expression")
            .and_then(Value::as_str)
            .unwrap_or_default();
        format!("$$\n{expression}\n$$")
    }

    fn render_media(&mut self, block: &ExportBlock) -> String {
        let label = media_label(
            &block.payload,
            &block.row.block_type,
            self,
            Some(&block.row.id),
        );
        let Some(url) = media_url(&block.payload) else {
            self.warn_block(
                block,
                "markdown_export_media_missing_reference",
                "Media block has no exportable reference",
            );
            return format!("[{label}]");
        };
        if url.starts_with("ganbaru-asset:") {
            self.warn_block(
                block,
                "markdown_export_local_asset_reference",
                "Local managed asset is exported as a Ganbaru asset reference",
            );
        }
        if block.row.block_type == "image" {
            format!("![{}]({})", escape_link_label(&label), escape_url(&url))
        } else {
            format!("[{}]({})", escape_link_label(&label), escape_url(&url))
        }
    }

    fn render_external_reference(&mut self, block: &ExportBlock) -> String {
        let url = block
            .payload
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if url.is_empty() {
            self.warn_block(
                block,
                "markdown_export_external_reference_missing_url",
                "External reference block has no URL",
            );
            return format!("[{}]", block.row.block_type);
        }
        let label = block
            .payload
            .get("caption")
            .and_then(Value::as_array)
            .map(|items| rich_text_items_plain_text(items))
            .filter(|caption| !caption.trim().is_empty())
            .unwrap_or_else(|| url.to_string());
        format!("[{}]({})", escape_link_label(&label), escape_url(url))
    }

    fn render_child_page(&mut self, block: &ExportBlock) -> String {
        let title = block
            .payload
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Untitled");
        self.warn_block(
            block,
            "markdown_export_child_page_reference",
            "Child page body is not inlined into this page markdown export",
        );
        format!("[[{}]]", escape_inline(title))
    }

    fn render_table(&mut self, block: &ExportBlock) -> String {
        let rows = block
            .children
            .iter()
            .filter(|child| child.row.block_type == "table_row")
            .collect::<Vec<_>>();
        if rows.is_empty() {
            self.warn_block(
                block,
                "markdown_export_empty_table",
                "Table has no rows to export",
            );
            return String::new();
        }
        let width = rows
            .iter()
            .map(|row| table_cells(&row.payload).len())
            .max()
            .unwrap_or(0);
        if width == 0 {
            self.warn_block(
                block,
                "markdown_export_empty_table",
                "Table rows have no cells to export",
            );
            return String::new();
        }
        let mut lines = Vec::new();
        let header = normalize_cells(table_cells(&rows[0].payload), width)
            .into_iter()
            .map(|cell| self.render_table_cell(&cell, Some(&rows[0].row.id)))
            .collect::<Vec<_>>();
        lines.push(format!("| {} |", header.join(" | ")));
        lines.push(format!(
            "| {} |",
            std::iter::repeat_n("---", width)
                .collect::<Vec<_>>()
                .join(" | ")
        ));
        for row in rows.iter().skip(1) {
            let cells = normalize_cells(table_cells(&row.payload), width)
                .into_iter()
                .map(|cell| self.render_table_cell(&cell, Some(&row.row.id)))
                .collect::<Vec<_>>();
            lines.push(format!("| {} |", cells.join(" | ")));
        }
        for child in &block.children {
            if child.row.block_type != "table_row" {
                self.warn_block(
                    child,
                    "markdown_export_table_child_unsupported",
                    "Non-row table child was not included in the markdown table",
                );
            } else {
                self.exported_block_count += 1;
            }
        }
        lines.join("\n")
    }

    fn render_table_row(&mut self, block: &ExportBlock) -> String {
        self.warn_block(
            block,
            "markdown_export_table_row_without_table",
            "Table row appeared outside a table and was exported as a single row",
        );
        let cells = table_cells(&block.payload)
            .into_iter()
            .map(|cell| self.render_table_cell(&cell, Some(&block.row.id)))
            .collect::<Vec<_>>();
        format!("| {} |", cells.join(" | "))
    }

    fn render_container_fallback(&mut self, block: &ExportBlock, depth: usize) -> String {
        self.warn_block(
            block,
            "markdown_export_container_approximated",
            "Container block was approximated by exporting its visible child content",
        );
        let label = match block.row.block_type.as_str() {
            "synced_block" => synced_block_label(&block.payload),
            "template" | "button" => self.render_payload_rich_text(block, &block.payload),
            _ => String::new(),
        };
        let children = self.render_blocks(&block.children, depth + 1);
        match (label.is_empty(), children.is_empty()) {
            (true, true) => String::new(),
            (false, true) => label,
            (true, false) => children,
            (false, false) => format!("{label}\n\n{children}"),
        }
    }

    fn render_unsupported_block(
        &mut self,
        block: &ExportBlock,
        code: &'static str,
        message: &'static str,
    ) -> String {
        self.warn_block(block, code, message);
        let label = block
            .payload
            .get("block_type")
            .and_then(Value::as_str)
            .unwrap_or(&block.row.block_type);
        format!(
            "<!-- unsupported notes block: {} -->",
            escape_html_comment(label)
        )
    }

    fn render_payload_rich_text(&mut self, block: &ExportBlock, payload: &Value) -> String {
        payload
            .get("rich_text")
            .and_then(Value::as_array)
            .map(|items| self.render_rich_text(items, Some(&block.row.id), None))
            .unwrap_or_default()
    }

    fn render_table_cell(&mut self, items: &[Value], block_id: Option<&str>) -> String {
        self.render_rich_text(items, block_id, None)
            .replace('|', "\\|")
            .replace('\n', "<br>")
    }

    fn render_comment_threads(&mut self, threads: &[ExportCommentThread]) -> String {
        let mut lines = Vec::new();
        for thread in threads {
            let target = if let Some(block_id) = thread.row.parent_block_id.as_deref() {
                if let Some(anchor) = thread.anchor.as_ref() {
                    format!(
                        "Block `{}` on \"{}\"",
                        block_id,
                        escape_inline(&anchor.anchor_text)
                    )
                } else {
                    format!("Block `{block_id}`")
                }
            } else {
                "Page discussion".to_string()
            };
            lines.push(format!(
                "- {} ({}, {})",
                target, thread.row.status, thread.row.created_time
            ));
            for comment in &thread.comments {
                let author = display_name(&comment.display_name);
                let rich_text = parse_json_array(&comment.rich_text).unwrap_or_default();
                let mut text = self.render_rich_text(&rich_text, None, Some(&comment.id));
                if text.chars().count() > MAX_EXPORTED_COMMENT_TEXT_CHARS {
                    text = text
                        .chars()
                        .take(MAX_EXPORTED_COMMENT_TEXT_CHARS)
                        .collect::<String>();
                    self.warn_comment(
                        comment,
                        "markdown_export_comment_truncated",
                        "Long comment text was truncated in markdown export",
                    );
                }
                lines.push(format!("  - {}: {}", escape_inline(&author), text));
            }
        }
        lines.join("\n")
    }

    fn render_rich_text(
        &mut self,
        items: &[Value],
        block_id: Option<&str>,
        comment_id: Option<&str>,
    ) -> String {
        items
            .iter()
            .map(|item| self.render_rich_text_item(item, block_id, comment_id))
            .collect::<Vec<_>>()
            .join("")
    }

    fn render_rich_text_item(
        &mut self,
        item: &Value,
        block_id: Option<&str>,
        comment_id: Option<&str>,
    ) -> String {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("text");
        let raw = match kind {
            "text" => item
                .get("text")
                .and_then(|text| text.get("content"))
                .and_then(Value::as_str)
                .or_else(|| item.get("plain_text").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string(),
            "mention" => item
                .get("plain_text")
                .and_then(Value::as_str)
                .unwrap_or("Mention")
                .to_string(),
            "equation" => {
                let expression = item
                    .get("equation")
                    .and_then(|equation| equation.get("expression"))
                    .and_then(Value::as_str)
                    .or_else(|| item.get("plain_text").and_then(Value::as_str))
                    .unwrap_or_default();
                return format!("${}$", escape_dollar_math(expression));
            }
            _ => {
                self.warn_source(
                    block_id,
                    comment_id,
                    "markdown_export_rich_text_unsupported",
                    "Unsupported rich text item was exported as plain text",
                );
                item.get("plain_text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            }
        };
        let mut rendered = apply_annotations(
            escape_inline(&raw),
            item.get("annotations"),
            self,
            block_id,
            comment_id,
        );
        let href = item
            .get("href")
            .and_then(Value::as_str)
            .or_else(|| item.get("text")?.get("link")?.get("url")?.as_str());
        if let Some(url) = href.filter(|url| !url.trim().is_empty()) {
            rendered = format!("[{}]({})", escape_link_label(&rendered), escape_url(url));
        }
        rendered
    }

    fn warn_block(&mut self, block: &ExportBlock, code: &'static str, message: &'static str) {
        self.warn_source(Some(&block.row.id), None, code, message);
    }

    fn warn_comment(
        &mut self,
        comment: &NoteCommentRow,
        code: &'static str,
        message: &'static str,
    ) {
        self.warn_source(None, Some(&comment.id), code, message);
    }

    fn warn_source(
        &mut self,
        block_id: Option<&str>,
        comment_id: Option<&str>,
        code: &'static str,
        message: &'static str,
    ) {
        self.diagnostics.push(NoteMarkdownExportDiagnosticDto::new(
            code,
            "warning",
            block_id.map(str::to_string),
            comment_id.map(str::to_string),
            message,
        ));
    }
}

fn apply_annotations(
    escaped: String,
    annotations: Option<&Value>,
    renderer: &mut MarkdownRenderer,
    block_id: Option<&str>,
    comment_id: Option<&str>,
) -> String {
    let Some(annotations) = annotations else {
        return escaped;
    };
    let mut rendered = if annotations
        .get("code")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        inline_code(&escaped)
    } else {
        escaped
    };
    if annotations
        .get("bold")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        rendered = format!("**{rendered}**");
    }
    if annotations
        .get("italic")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        rendered = format!("*{rendered}*");
    }
    if annotations
        .get("strikethrough")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        rendered = format!("~~{rendered}~~");
    }
    if annotations
        .get("underline")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        renderer.warn_source(
            block_id,
            comment_id,
            "markdown_export_annotation_approximated",
            "Underline annotation was exported as inline HTML",
        );
        rendered = format!("<u>{rendered}</u>");
    }
    let color = annotations
        .get("color")
        .and_then(Value::as_str)
        .unwrap_or("default");
    if color != "default" {
        renderer.warn_source(
            block_id,
            comment_id,
            "markdown_export_annotation_unsupported",
            "Rich text color cannot be represented in portable markdown",
        );
    }
    rendered
}

fn media_label(
    payload: &Value,
    block_type: &str,
    renderer: &mut MarkdownRenderer,
    block_id: Option<&str>,
) -> String {
    let caption = payload
        .get("caption")
        .and_then(Value::as_array)
        .map(|items| rich_text_items_plain_text(items))
        .unwrap_or_default();
    if !caption.trim().is_empty() {
        return caption;
    }
    if let Some(name) = payload.get("name").and_then(Value::as_str) {
        if !name.trim().is_empty() {
            return name.to_string();
        }
    }
    if payload.get("type").and_then(Value::as_str) == Some("file_upload") {
        renderer.warn_source(
            block_id,
            None,
            "markdown_export_file_upload_reference",
            "File upload object was exported as an opaque reference",
        );
    }
    block_type.to_string()
}

struct ExportCommentThread {
    row: NoteCommentThreadRow,
    anchor: Option<NoteCommentAnchorRow>,
    comments: Vec<NoteCommentRow>,
}

async fn load_export_comments(
    pool: &SqlitePool,
    page_id: &str,
    include_resolved: bool,
) -> Result<Vec<ExportCommentThread>, String> {
    let thread_rows = if include_resolved {
        sqlx::query_as::<_, NoteCommentThreadRow>(
            "SELECT *
             FROM notes_comment_threads
             WHERE page_id = ?
               AND EXISTS (
                   SELECT 1
                   FROM notes_comments AS comment
                   WHERE comment.thread_id = notes_comment_threads.id
                     AND comment.deleted_at IS NULL
               )
             ORDER BY created_time ASC, id ASC",
        )
        .bind(page_id)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, NoteCommentThreadRow>(
            "SELECT *
             FROM notes_comment_threads
             WHERE page_id = ?
               AND status = 'open'
               AND EXISTS (
                   SELECT 1
                   FROM notes_comments AS comment
                   WHERE comment.thread_id = notes_comment_threads.id
                     AND comment.deleted_at IS NULL
               )
             ORDER BY created_time ASC, id ASC",
        )
        .bind(page_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| format!("load notes comment threads for markdown export: {e}"))?;

    let mut threads = Vec::with_capacity(thread_rows.len());
    for row in thread_rows {
        let anchor = sqlx::query_as::<_, NoteCommentAnchorRow>(
            "SELECT *
             FROM notes_comment_thread_anchors
             WHERE thread_id = ?",
        )
        .bind(&row.id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load notes comment anchor for markdown export: {e}"))?;
        let comments = sqlx::query_as::<_, NoteCommentRow>(
            "SELECT *
             FROM notes_comments
             WHERE thread_id = ?
               AND deleted_at IS NULL
             ORDER BY created_time ASC, id ASC",
        )
        .bind(&row.id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load notes comments for markdown export: {e}"))?;
        threads.push(ExportCommentThread {
            row,
            anchor,
            comments,
        });
    }
    Ok(threads)
}
