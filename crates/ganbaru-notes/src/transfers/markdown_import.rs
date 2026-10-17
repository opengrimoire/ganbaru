pub mod syntax;

use self::syntax::{
    is_divider, is_https_url, is_reference_definition, is_rich_text_url, is_table_delimiter,
    normalize_table_cells, ordered_list_marker, parse_image, parse_table_cells,
    starts_with_list_marker, todo_marker, unordered_list_marker, unquote_frontmatter_value,
};
use crate::models::{
    NoteMarkdownImportDiagnosticDto, NoteMarkdownImportDto, NoteMarkdownImportRequest, NoteParent,
};
use crate::transfers::import_writer::{
    ImportBlock, ImportedPageCreate, count_import_blocks, create_imported_page,
};
use crate::validation::{validate_block_payload, validate_parent};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::path::Path;

const MAX_MARKDOWN_IMPORT_CHARS: usize = 1_000_000;
const MAX_MARKDOWN_IMPORT_BLOCKS: usize = 1_000;

pub async fn import_page(
    pool: &SqlitePool,
    request: NoteMarkdownImportRequest,
) -> Result<NoteMarkdownImportDto, String> {
    validate_parent(&request.parent)?;
    if matches!(&request.parent, NoteParent::DataSourceId { .. }) {
        return Err("markdown imports cannot create database row pages".to_string());
    }
    if request.markdown.chars().count() > MAX_MARKDOWN_IMPORT_CHARS {
        return Err("markdown import exceeds the 1 MB text limit".to_string());
    }
    let source_name = request
        .source_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let mut plan = parse_markdown(&request.markdown);
    let title = import_title(
        request.title.as_deref(),
        plan.title.as_deref(),
        source_name.as_deref(),
    );
    if plan.blocks.is_empty() {
        plan.blocks
            .push(markdown_block("paragraph", text_payload(""), 1, Vec::new()));
    }
    let imported_block_count = count_import_blocks(&plan.blocks);
    if imported_block_count > MAX_MARKDOWN_IMPORT_BLOCKS {
        return Err(format!(
            "markdown import supports up to {MAX_MARKDOWN_IMPORT_BLOCKS} blocks"
        ));
    }

    let page = create_imported_page(
        pool,
        ImportedPageCreate {
            parent: &request.parent,
            after_block_id: request.after_block_id.as_deref(),
            title: &title,
            source_provider: "markdown",
            source_object_id: source_name.as_deref(),
            source_workspace_id: None,
            source_last_edited_time: None,
            icon: None,
            cover: None,
            url: None,
            public_url: None,
            project_id: None,
            blocks: plan.blocks,
        },
    )
    .await?;
    Ok(NoteMarkdownImportDto::new(
        page,
        plan.diagnostics,
        imported_block_count as i64,
    ))
}

pub(crate) struct MarkdownPlan {
    pub(crate) title: Option<String>,
    pub(crate) blocks: Vec<ImportBlock>,
    pub(crate) diagnostics: Vec<NoteMarkdownImportDiagnosticDto>,
}

fn markdown_block(
    block_type: &'static str,
    payload: Value,
    source_line: i64,
    children: Vec<ImportBlock>,
) -> ImportBlock {
    ImportBlock::new(
        block_type,
        payload,
        Some(format!("line:{source_line}")),
        children,
    )
}

pub(crate) fn parse_markdown(markdown: &str) -> MarkdownPlan {
    let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
    let (frontmatter_title, body, mut diagnostics) = strip_frontmatter(&normalized);
    let lines = body
        .lines()
        .enumerate()
        .map(|(index, text)| MarkdownLine {
            number: index as i64 + 1,
            text: text.to_string(),
        })
        .collect::<Vec<_>>();
    let mut parser = MarkdownParser {
        lines: &lines,
        index: 0,
        diagnostics: Vec::new(),
        paragraph: Vec::new(),
        blocks: Vec::new(),
        first_heading: None,
    };
    parser.parse();
    diagnostics.extend(parser.diagnostics);
    MarkdownPlan {
        title: frontmatter_title.or(parser.first_heading),
        blocks: parser.blocks,
        diagnostics,
    }
}

struct MarkdownLine {
    number: i64,
    text: String,
}

struct MarkdownParser<'a> {
    lines: &'a [MarkdownLine],
    index: usize,
    diagnostics: Vec<NoteMarkdownImportDiagnosticDto>,
    paragraph: Vec<MarkdownLine>,
    blocks: Vec<ImportBlock>,
    first_heading: Option<String>,
}

impl MarkdownParser<'_> {
    fn parse(&mut self) {
        while self.index < self.lines.len() {
            let line = &self.lines[self.index];
            let trimmed = line.text.trim();
            if trimmed.is_empty() {
                self.flush_paragraph();
                self.index += 1;
                continue;
            }
            if let Some(block) = self.parse_fenced_code() {
                self.flush_paragraph();
                self.blocks.push(block);
                continue;
            }
            if self.is_table_start() {
                self.flush_paragraph();
                let table = self.parse_table();
                self.blocks.push(table);
                continue;
            }
            if let Some(block) = self.parse_quote() {
                self.flush_paragraph();
                self.blocks.push(block);
                continue;
            }
            if is_divider(trimmed) {
                self.flush_paragraph();
                self.blocks.push(markdown_block(
                    "divider",
                    json!({}),
                    line.number,
                    Vec::new(),
                ));
                self.index += 1;
                continue;
            }
            if let Some(block) = self.parse_heading() {
                self.flush_paragraph();
                self.blocks.push(block);
                continue;
            }
            if let Some(block) = self.parse_list_item() {
                self.flush_paragraph();
                self.blocks.push(block);
                continue;
            }
            if let Some(block) = self.parse_standalone_image() {
                self.flush_paragraph();
                self.blocks.push(block);
                continue;
            }
            if trimmed.starts_with('<') {
                self.flush_paragraph();
                self.diagnostic(
                    "markdown_html_unsupported",
                    "warning",
                    Some(line.number),
                    "HTML blocks are preserved as unsupported markdown.",
                );
                self.blocks.push(unsupported_block(
                    line.number,
                    &line.text,
                    "HTML blocks are not imported as editable Notes blocks.",
                ));
                self.index += 1;
                continue;
            }
            if is_reference_definition(trimmed) {
                self.flush_paragraph();
                self.diagnostic(
                    "markdown_reference_definition_unsupported",
                    "warning",
                    Some(line.number),
                    "Reference-style links are preserved as unsupported markdown.",
                );
                self.blocks.push(unsupported_block(
                    line.number,
                    &line.text,
                    "Reference-style links are not resolved by the local markdown importer.",
                ));
                self.index += 1;
                continue;
            }
            self.paragraph.push(MarkdownLine {
                number: line.number,
                text: line.text.clone(),
            });
            self.index += 1;
        }
        self.flush_paragraph();
    }

    fn parse_fenced_code(&mut self) -> Option<ImportBlock> {
        let line = &self.lines[self.index];
        let trimmed = line.text.trim_start();
        let fence = if trimmed.starts_with("```") {
            "```"
        } else if trimmed.starts_with("~~~") {
            "~~~"
        } else {
            return None;
        };
        let language = trimmed
            .trim_start_matches(fence)
            .split_whitespace()
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or("plain text")
            .to_string();
        let start_line = line.number;
        self.index += 1;
        let mut code_lines = Vec::new();
        while self.index < self.lines.len() {
            let current = &self.lines[self.index];
            if current.text.trim_start().starts_with(fence) {
                self.index += 1;
                return Some(markdown_block(
                    "code",
                    json!({
                        "rich_text": [rich_text(&code_lines.join("\n"), None)],
                        "caption": [],
                        "language": language
                    }),
                    start_line,
                    Vec::new(),
                ));
            }
            code_lines.push(current.text.clone());
            self.index += 1;
        }
        self.diagnostic(
            "markdown_code_fence_unclosed",
            "warning",
            Some(start_line),
            "A fenced code block was imported through the end of the document because it was not closed.",
        );
        Some(markdown_block(
            "code",
            json!({
                "rich_text": [rich_text(&code_lines.join("\n"), None)],
                "caption": [],
                "language": language
            }),
            start_line,
            Vec::new(),
        ))
    }

    fn is_table_start(&self) -> bool {
        if self.index + 1 >= self.lines.len() {
            return false;
        }
        parse_table_cells(&self.lines[self.index].text).is_some()
            && is_table_delimiter(&self.lines[self.index + 1].text)
    }

    fn parse_table(&mut self) -> ImportBlock {
        let start_line = self.lines[self.index].number;
        let header = parse_table_cells(&self.lines[self.index].text).unwrap_or_default();
        let width = header.len().max(1);
        self.index += 2;
        let mut rows = vec![header];
        while self.index < self.lines.len() {
            let line = &self.lines[self.index];
            if line.text.trim().is_empty() || is_table_delimiter(&line.text) {
                break;
            }
            let Some(cells) = parse_table_cells(&line.text) else {
                break;
            };
            rows.push(normalize_table_cells(cells, width));
            self.index += 1;
        }
        let children = rows
            .into_iter()
            .map(|cells| {
                let rich_cells = cells
                    .into_iter()
                    .map(|cell| inline_rich_text(&cell, start_line, &mut self.diagnostics))
                    .collect::<Vec<_>>();
                markdown_block(
                    "table_row",
                    json!({ "cells": rich_cells }),
                    start_line,
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>();
        markdown_block(
            "table",
            json!({
                "table_width": width,
                "has_column_header": true,
                "has_row_header": false
            }),
            start_line,
            children,
        )
    }

    fn parse_quote(&mut self) -> Option<ImportBlock> {
        let line = &self.lines[self.index];
        if !line.text.trim_start().starts_with('>') {
            return None;
        }
        let start_line = line.number;
        let mut quote_lines = Vec::new();
        while self.index < self.lines.len() {
            let current = &self.lines[self.index];
            let trimmed = current.text.trim_start();
            if !trimmed.starts_with('>') {
                break;
            }
            quote_lines.push(trimmed.trim_start_matches('>').trim_start().to_string());
            self.index += 1;
        }
        let text = quote_lines.join("\n");
        Some(markdown_block(
            "quote",
            text_payload_with_line(&text, start_line, &mut self.diagnostics),
            start_line,
            Vec::new(),
        ))
    }

    fn parse_heading(&mut self) -> Option<ImportBlock> {
        let line = &self.lines[self.index];
        let trimmed = line.text.trim_start();
        let depth = trimmed
            .chars()
            .take_while(|character| *character == '#')
            .count();
        if depth == 0 || depth > 6 || !trimmed.chars().nth(depth).is_some_and(char::is_whitespace) {
            return None;
        }
        let text = trimmed[depth..].trim().trim_end_matches('#').trim();
        if self.first_heading.is_none() && !text.is_empty() {
            self.first_heading = Some(text.to_string());
        }
        let block_type = match depth {
            1 => "heading_1",
            2 => "heading_2",
            3 => "heading_3",
            4 => "heading_4",
            5 => "heading_5",
            _ => "heading_6",
        };
        self.index += 1;
        Some(markdown_block(
            block_type,
            text_payload_with_line(text, line.number, &mut self.diagnostics),
            line.number,
            Vec::new(),
        ))
    }

    fn parse_list_item(&mut self) -> Option<ImportBlock> {
        let line = &self.lines[self.index];
        if line.text.starts_with(' ') || line.text.starts_with('\t') {
            let trimmed = line.text.trim_start();
            if starts_with_list_marker(trimmed) {
                self.diagnostic(
                    "markdown_nested_list_flattened",
                    "warning",
                    Some(line.number),
                    "Nested list indentation is imported as a flat list item.",
                );
            }
        }
        let trimmed = line.text.trim_start();
        if let Some((checked, text)) = todo_marker(trimmed) {
            self.index += 1;
            return Some(markdown_block(
                "to_do",
                json!({
                    "rich_text": inline_rich_text(text, line.number, &mut self.diagnostics),
                    "checked": checked,
                    "color": "default"
                }),
                line.number,
                Vec::new(),
            ));
        }
        if let Some(text) = unordered_list_marker(trimmed) {
            self.index += 1;
            return Some(markdown_block(
                "bulleted_list_item",
                text_payload_with_line(text, line.number, &mut self.diagnostics),
                line.number,
                Vec::new(),
            ));
        }
        if let Some(text) = ordered_list_marker(trimmed) {
            self.index += 1;
            return Some(markdown_block(
                "numbered_list_item",
                text_payload_with_line(text, line.number, &mut self.diagnostics),
                line.number,
                Vec::new(),
            ));
        }
        None
    }

    fn parse_standalone_image(&mut self) -> Option<ImportBlock> {
        let line = &self.lines[self.index];
        let parsed = parse_image(line.text.trim())?;
        self.index += 1;
        if !is_https_url(parsed.url) {
            self.diagnostic(
                "markdown_image_reference_blocked",
                "warning",
                Some(line.number),
                "Only HTTPS image references are imported directly. Use the import file policy flow for local files.",
            );
            return Some(unsupported_block(
                line.number,
                &line.text,
                "Image reference was not imported because it was not an HTTPS URL.",
            ));
        }
        let payload = json!({
            "caption": if parsed.alt.trim().is_empty() {
                Vec::<Value>::new()
            } else {
                vec![rich_text(parsed.alt, None)]
            },
            "type": "external",
            "external": {
                "url": parsed.url
            }
        });
        if let Err(error) = validate_block_payload("image", &payload) {
            self.diagnostic(
                "markdown_image_reference_unsupported",
                "warning",
                Some(line.number),
                error,
            );
            return Some(unsupported_block(
                line.number,
                &line.text,
                "Image reference was not supported by the Notes image block.",
            ));
        }
        Some(markdown_block("image", payload, line.number, Vec::new()))
    }

    fn flush_paragraph(&mut self) {
        if self.paragraph.is_empty() {
            return;
        }
        let start_line = self.paragraph[0].number;
        let text = self
            .paragraph
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if text.contains("![") {
            self.diagnostic(
                "markdown_inline_image_preserved_as_text",
                "warning",
                Some(start_line),
                "Inline images inside paragraphs are preserved as text. Put an image on its own line to create an image block.",
            );
        }
        let payload = text_payload_with_line(&text, start_line, &mut self.diagnostics);
        self.blocks
            .push(markdown_block("paragraph", payload, start_line, Vec::new()));
        self.paragraph.clear();
    }

    fn diagnostic(
        &mut self,
        code: &str,
        severity: &str,
        line: Option<i64>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(NoteMarkdownImportDiagnosticDto::new(
            code, severity, line, message,
        ));
    }
}

fn strip_frontmatter(
    input: &str,
) -> (Option<String>, String, Vec<NoteMarkdownImportDiagnosticDto>) {
    let mut diagnostics = Vec::new();
    let lines = input.lines().collect::<Vec<_>>();
    if lines.first().map(|line| line.trim()) != Some("---") {
        return (None, input.to_string(), diagnostics);
    }
    let Some(end_index) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, line)| (line.trim() == "---").then_some(index))
    else {
        diagnostics.push(NoteMarkdownImportDiagnosticDto::new(
            "markdown_frontmatter_unclosed",
            "warning",
            Some(1),
            "Opening frontmatter marker was preserved as content because it was not closed.",
        ));
        return (None, input.to_string(), diagnostics);
    };
    let mut title = None;
    for (offset, line) in lines[1..end_index].iter().enumerate() {
        let line_number = offset as i64 + 2;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            diagnostics.push(NoteMarkdownImportDiagnosticDto::new(
                "markdown_frontmatter_field_unsupported",
                "warning",
                Some(line_number),
                "Only simple key and value frontmatter fields are imported.",
            ));
            continue;
        };
        if key.trim().eq_ignore_ascii_case("title") {
            title = Some(unquote_frontmatter_value(value.trim()));
        } else {
            diagnostics.push(NoteMarkdownImportDiagnosticDto::new(
                "markdown_frontmatter_field_ignored",
                "info",
                Some(line_number),
                format!("Frontmatter field '{}' was ignored.", key.trim()),
            ));
        }
    }
    let body = if end_index + 1 >= lines.len() {
        String::new()
    } else {
        lines[end_index + 1..].join("\n")
    };
    (
        title.filter(|value| !value.trim().is_empty()),
        body,
        diagnostics,
    )
}

fn import_title(
    request_title: Option<&str>,
    parsed_title: Option<&str>,
    source_name: Option<&str>,
) -> String {
    if let Some(title) = request_title.and_then(non_empty_trimmed) {
        return title.chars().take(240).collect();
    }
    if let Some(title) = parsed_title.and_then(non_empty_trimmed) {
        return title.chars().take(240).collect();
    }
    if let Some(title) = source_name.and_then(source_name_title) {
        return title.chars().take(240).collect();
    }
    "Imported markdown".to_string()
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn source_name_title(value: &str) -> Option<String> {
    let file_name = Path::new(value)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(value)
        .trim();
    (!file_name.is_empty()).then(|| file_name.to_string())
}

fn text_payload(text: &str) -> Value {
    json!({
        "rich_text": [rich_text(text, None)],
        "color": "default"
    })
}

fn text_payload_with_line(
    text: &str,
    line: i64,
    diagnostics: &mut Vec<NoteMarkdownImportDiagnosticDto>,
) -> Value {
    json!({
        "rich_text": inline_rich_text(text, line, diagnostics),
        "color": "default"
    })
}

fn inline_rich_text(
    text: &str,
    line: i64,
    diagnostics: &mut Vec<NoteMarkdownImportDiagnosticDto>,
) -> Vec<Value> {
    let mut items = Vec::new();
    let mut cursor = 0;
    while let Some(relative_start) = text[cursor..].find('[') {
        let start = cursor + relative_start;
        if text[start..].starts_with("![") {
            cursor = start + 2;
            continue;
        }
        let Some(close_label) = text[start + 1..].find(']') else {
            break;
        };
        let label_end = start + 1 + close_label;
        if !text[label_end..].starts_with("](") {
            cursor = label_end + 1;
            continue;
        }
        let url_start = label_end + 2;
        let Some(close_url) = text[url_start..].find(')') else {
            break;
        };
        let url_end = url_start + close_url;
        let label = &text[start + 1..label_end];
        let url = text[url_start..url_end].trim();
        if start > cursor {
            items.push(rich_text(&text[cursor..start], None));
        }
        if is_rich_text_url(url) {
            items.push(rich_text(label, Some(url)));
        } else {
            diagnostics.push(NoteMarkdownImportDiagnosticDto::new(
                "markdown_link_url_unsupported",
                "warning",
                Some(line),
                "Only HTTP, HTTPS, and mailto links are imported as rich text links.",
            ));
            items.push(rich_text(&text[start..url_end + 1], None));
        }
        cursor = url_end + 1;
    }
    if cursor < text.len() {
        items.push(rich_text(&text[cursor..], None));
    }
    if items.is_empty() {
        items.push(rich_text("", None));
    }
    items
}

fn rich_text(text: &str, url: Option<&str>) -> Value {
    json!({
        "type": "text",
        "text": {
            "content": text,
            "link": url.map(|url| json!({ "url": url }))
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
        "href": url
    })
}

fn unsupported_block(line: i64, raw: &str, warning: &str) -> ImportBlock {
    markdown_block(
        "unsupported",
        json!({
            "block_type": "markdown",
            "source_type": "markdown_import",
            "raw": {
                "markdown": raw,
                "line": line
            },
            "warnings": [warning]
        }),
        line,
        Vec::new(),
    )
}
