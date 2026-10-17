pub mod syntax;

use self::syntax::{HtmlElement, HtmlNode, decode_html_entities, parse_html_nodes};
use crate::models::{
    NoteHtmlImportDiagnosticDto, NoteHtmlImportDto, NoteHtmlImportRequest, NoteParent,
};
use crate::transfers::import_writer::{
    ImportBlock, ImportedPageCreate, count_import_blocks, create_imported_page,
    normalized_import_project_id,
};
use crate::validation::{validate_block_payload, validate_parent};
use ammonia::{Builder, UrlRelative};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

const MAX_HTML_IMPORT_CHARS: usize = 1_000_000;
const MAX_HTML_IMPORT_BLOCKS: usize = 1_000;

const ALLOWED_TAGS: &[&str] = &[
    "a",
    "article",
    "aside",
    "audio",
    "b",
    "blockquote",
    "body",
    "br",
    "code",
    "del",
    "details",
    "div",
    "em",
    "figcaption",
    "figure",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "html",
    "hr",
    "i",
    "img",
    "input",
    "li",
    "main",
    "ol",
    "p",
    "pre",
    "s",
    "section",
    "source",
    "span",
    "strike",
    "strong",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "u",
    "ul",
    "video",
];

const CLEAN_CONTENT_TAGS: &[&str] = &[
    "base", "embed", "form", "head", "iframe", "link", "math", "meta", "object", "script", "style",
    "svg", "title",
];

pub async fn import_page(
    pool: &SqlitePool,
    request: NoteHtmlImportRequest,
) -> Result<NoteHtmlImportDto, String> {
    validate_parent(&request.parent)?;
    if matches!(&request.parent, NoteParent::DataSourceId { .. }) {
        return Err("HTML imports cannot create database row pages".to_string());
    }
    if request.html.chars().count() > MAX_HTML_IMPORT_CHARS {
        return Err("HTML import exceeds the 1 MB text limit".to_string());
    }
    let source_name = request
        .source_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let keep_external_file_references = request.keep_external_file_references.unwrap_or(false);
    let project_id = normalized_import_project_id(request.project_id);
    let mut plan = parse_html(&request.html, keep_external_file_references);
    let title = import_title(
        request.title.as_deref(),
        plan.title.as_deref(),
        source_name.as_deref(),
    );
    if plan.blocks.is_empty() {
        plan.blocks
            .push(html_block("paragraph", text_payload(""), 1, Vec::new()));
    }
    let imported_block_count = count_import_blocks(&plan.blocks);
    if imported_block_count > MAX_HTML_IMPORT_BLOCKS {
        return Err(format!(
            "HTML import supports up to {MAX_HTML_IMPORT_BLOCKS} blocks"
        ));
    }

    let page = create_imported_page(
        pool,
        ImportedPageCreate {
            parent: &request.parent,
            after_block_id: request.after_block_id.as_deref(),
            title: &title,
            source_provider: "html",
            source_object_id: source_name.as_deref(),
            source_workspace_id: None,
            source_last_edited_time: None,
            icon: None,
            cover: None,
            url: None,
            public_url: None,
            project_id: project_id.as_deref(),
            blocks: plan.blocks,
        },
    )
    .await?;
    Ok(NoteHtmlImportDto::new(
        page,
        plan.diagnostics,
        imported_block_count as i64,
    ))
}

pub(crate) struct HtmlPlan {
    pub(crate) title: Option<String>,
    pub(crate) blocks: Vec<ImportBlock>,
    pub(crate) diagnostics: Vec<NoteHtmlImportDiagnosticDto>,
}

pub(crate) fn parse_html(html: &str, keep_external_file_references: bool) -> HtmlPlan {
    let normalized = html.replace("\r\n", "\n").replace('\r', "\n");
    let mut diagnostics = scan_html_diagnostics(&normalized);
    let title = extract_title(&normalized);
    let sanitized = sanitize_html_import(&normalized);
    if sanitized != normalized {
        diagnostics.push(NoteHtmlImportDiagnosticDto::new(
            "html_markup_sanitized",
            "info",
            None,
            "Unsafe or unsupported HTML markup was removed before import.",
        ));
    }
    let nodes = parse_html_nodes(&sanitized);
    let mut parser = HtmlImportParser {
        keep_external_file_references,
        diagnostics,
        first_heading: None,
    };
    let blocks = parser.blocks_from_nodes(&nodes);
    HtmlPlan {
        title: title.or(parser.first_heading),
        blocks,
        diagnostics: parser.diagnostics,
    }
}

fn sanitize_html_import(html: &str) -> String {
    let mut tag_attributes = HashMap::new();
    tag_attributes.insert("a", HashSet::from(["href"]));
    tag_attributes.insert("audio", HashSet::from(["src"]));
    tag_attributes.insert("details", HashSet::from(["open"]));
    tag_attributes.insert("img", HashSet::from(["alt", "src", "title"]));
    tag_attributes.insert("input", HashSet::from(["checked", "disabled", "type"]));
    tag_attributes.insert("source", HashSet::from(["src"]));
    tag_attributes.insert("video", HashSet::from(["src"]));

    let mut builder = Builder::empty();
    builder
        .tags(HashSet::from_iter(ALLOWED_TAGS.iter().copied()))
        .tag_attributes(tag_attributes)
        .generic_attributes(HashSet::new())
        .generic_attribute_prefixes(HashSet::new())
        .url_schemes(HashSet::from(["http", "https", "mailto"]))
        .url_relative(UrlRelative::PassThrough)
        .link_rel(None)
        .clean_content_tags(HashSet::from_iter(CLEAN_CONTENT_TAGS.iter().copied()))
        .strip_comments(true);
    builder.clean(html).to_string()
}

struct HtmlImportParser {
    keep_external_file_references: bool,
    diagnostics: Vec<NoteHtmlImportDiagnosticDto>,
    first_heading: Option<String>,
}

impl HtmlImportParser {
    fn blocks_from_nodes(&mut self, nodes: &[HtmlNode]) -> Vec<ImportBlock> {
        let mut blocks = Vec::new();
        let mut inline = Vec::new();
        for node in nodes {
            if is_inline_node(node) {
                inline.push(node.clone());
                continue;
            }
            self.flush_inline_nodes(&mut inline, &mut blocks);
            blocks.extend(self.blocks_from_node(node));
        }
        self.flush_inline_nodes(&mut inline, &mut blocks);
        blocks
    }

    fn flush_inline_nodes(&mut self, inline: &mut Vec<HtmlNode>, blocks: &mut Vec<ImportBlock>) {
        if inline.is_empty() {
            return;
        }
        let line = inline.first().map(node_line).unwrap_or(1);
        let rich_text = self.rich_text_from_nodes(inline);
        if rich_text_plain_text(&rich_text).trim().is_empty() {
            inline.clear();
            return;
        }
        blocks.push(html_block(
            "paragraph",
            json!({
                "rich_text": rich_text,
                "color": "default"
            }),
            line,
            Vec::new(),
        ));
        inline.clear();
    }

    fn blocks_from_node(&mut self, node: &HtmlNode) -> Vec<ImportBlock> {
        match node {
            HtmlNode::Text { text, line } => {
                if text.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![html_block(
                        "paragraph",
                        text_payload(text),
                        *line,
                        Vec::new(),
                    )]
                }
            }
            HtmlNode::Element(element) => self.blocks_from_element(element),
        }
    }

    fn blocks_from_element(&mut self, element: &HtmlElement) -> Vec<ImportBlock> {
        match element.name.as_str() {
            "html" | "body" | "main" | "article" | "section" | "tbody" | "thead" | "tfoot" => {
                self.blocks_from_nodes(&element.children)
            }
            "div" | "figure" => {
                if has_block_children(&element.children) {
                    self.blocks_from_nodes(&element.children)
                } else {
                    vec![self.text_block("paragraph", element)]
                }
            }
            "figcaption" | "p" => vec![self.text_block("paragraph", element)],
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => vec![self.heading_block(element)],
            "ul" => self.list_blocks(element, "bulleted_list_item"),
            "ol" => self.list_blocks(element, "numbered_list_item"),
            "li" => vec![self.list_item_block(element, "bulleted_list_item")],
            "blockquote" => vec![self.text_block("quote", element)],
            "pre" => vec![self.code_block(element)],
            "hr" => vec![html_block("divider", json!({}), element.line, Vec::new())],
            "table" => vec![self.table_block(element)],
            "details" => vec![self.toggle_block(element)],
            "aside" => vec![self.callout_block(element)],
            "img" => vec![self.media_block(element, "image")],
            "video" => vec![self.media_block(element, "video")],
            "audio" => vec![self.media_block(element, "audio")],
            "br" | "input" | "source" | "summary" => Vec::new(),
            _ => {
                self.diagnostic(
                    "html_element_unsupported",
                    "warning",
                    Some(element.line),
                    format!(
                        "HTML <{}> content was preserved as unsupported.",
                        element.name
                    ),
                );
                vec![unsupported_block(
                    element.line,
                    &element.name,
                    "HTML element is not mapped to an editable Notes block.",
                )]
            }
        }
    }

    fn text_block(&mut self, block_type: &'static str, element: &HtmlElement) -> ImportBlock {
        html_block(
            block_type,
            json!({
                "rich_text": self.rich_text_from_nodes(&element.children),
                "color": "default"
            }),
            element.line,
            Vec::new(),
        )
    }

    fn heading_block(&mut self, element: &HtmlElement) -> ImportBlock {
        let block_type = match element.name.as_str() {
            "h1" => "heading_1",
            "h2" => "heading_2",
            "h3" => "heading_3",
            "h4" => "heading_4",
            "h5" => "heading_5",
            _ => "heading_6",
        };
        let rich_text = self.rich_text_from_nodes(&element.children);
        let plain = rich_text_plain_text(&rich_text);
        if self.first_heading.is_none() && !plain.trim().is_empty() {
            self.first_heading = Some(plain.trim().to_string());
        }
        html_block(
            block_type,
            json!({
                "rich_text": rich_text,
                "color": "default"
            }),
            element.line,
            Vec::new(),
        )
    }

    fn list_blocks(&mut self, element: &HtmlElement, block_type: &'static str) -> Vec<ImportBlock> {
        let mut blocks = Vec::new();
        for child in &element.children {
            if let HtmlNode::Element(item) = child {
                if item.name == "li" {
                    blocks.push(self.list_item_block(item, block_type));
                    let nested = item
                        .children
                        .iter()
                        .filter_map(|node| match node {
                            HtmlNode::Element(child)
                                if matches!(child.name.as_str(), "ul" | "ol") =>
                            {
                                Some(child)
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    if !nested.is_empty() {
                        self.diagnostic(
                            "html_nested_list_flattened",
                            "warning",
                            Some(item.line),
                            "Nested HTML lists are imported after their parent item.",
                        );
                    }
                    for nested_list in nested {
                        let nested_type = if nested_list.name == "ol" {
                            "numbered_list_item"
                        } else {
                            "bulleted_list_item"
                        };
                        blocks.extend(self.list_blocks(nested_list, nested_type));
                    }
                }
            }
        }
        blocks
    }

    fn list_item_block(
        &mut self,
        element: &HtmlElement,
        fallback_type: &'static str,
    ) -> ImportBlock {
        let inline_children = element
            .children
            .iter()
            .filter(|node| !matches!(node, HtmlNode::Element(child) if matches!(child.name.as_str(), "ul" | "ol")))
            .cloned()
            .collect::<Vec<_>>();
        let rich_text = self.rich_text_from_nodes(&inline_children);
        if let Some(checked) = checkbox_state(element) {
            return html_block(
                "to_do",
                json!({
                    "rich_text": rich_text,
                    "checked": checked,
                    "color": "default"
                }),
                element.line,
                Vec::new(),
            );
        }
        html_block(
            fallback_type,
            json!({
                "rich_text": rich_text,
                "color": "default"
            }),
            element.line,
            Vec::new(),
        )
    }

    fn code_block(&mut self, element: &HtmlElement) -> ImportBlock {
        html_block(
            "code",
            json!({
                "rich_text": [rich_text(&node_text(&element.children), InlineStyle::default())],
                "caption": [],
                "language": "plain text"
            }),
            element.line,
            Vec::new(),
        )
    }

    fn table_block(&mut self, element: &HtmlElement) -> ImportBlock {
        let rows = table_rows(element);
        if rows.is_empty() {
            self.diagnostic(
                "html_table_empty",
                "warning",
                Some(element.line),
                "An empty HTML table was imported as unsupported content.",
            );
            return unsupported_block(element.line, "table", "HTML table had no rows.");
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let has_column_header = rows
            .first()
            .map(|row| row.iter().any(|cell| cell.header))
            .unwrap_or(false);
        let children = rows
            .into_iter()
            .map(|row| {
                let mut cells = row
                    .into_iter()
                    .map(|cell| self.rich_text_from_nodes(&cell.children))
                    .collect::<Vec<_>>();
                while cells.len() < width {
                    cells.push(vec![rich_text("", InlineStyle::default())]);
                }
                html_block(
                    "table_row",
                    json!({ "cells": cells }),
                    element.line,
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>();
        html_block(
            "table",
            json!({
                "table_width": width,
                "has_column_header": has_column_header,
                "has_row_header": false
            }),
            element.line,
            children,
        )
    }

    fn toggle_block(&mut self, element: &HtmlElement) -> ImportBlock {
        let summary = element.children.iter().find_map(|node| match node {
            HtmlNode::Element(child) if child.name == "summary" => Some(child),
            _ => None,
        });
        let rich_text = summary
            .map(|summary| self.rich_text_from_nodes(&summary.children))
            .unwrap_or_else(|| vec![rich_text("Toggle", InlineStyle::default())]);
        let child_nodes = element
            .children
            .iter()
            .filter(|node| !matches!(node, HtmlNode::Element(child) if child.name == "summary"))
            .cloned()
            .collect::<Vec<_>>();
        html_block(
            "toggle",
            json!({
                "rich_text": rich_text,
                "color": "default",
                "ganbaru_open": element.has_attr("open")
            }),
            element.line,
            self.blocks_from_nodes(&child_nodes),
        )
    }

    fn callout_block(&mut self, element: &HtmlElement) -> ImportBlock {
        html_block(
            "callout",
            json!({
                "rich_text": self.rich_text_from_nodes(&element.children),
                "color": "default",
                "icon": null
            }),
            element.line,
            Vec::new(),
        )
    }

    fn media_block(&mut self, element: &HtmlElement, block_type: &'static str) -> ImportBlock {
        let src = element
            .attr("src")
            .or_else(|| {
                element.children.iter().find_map(|node| match node {
                    HtmlNode::Element(child) if child.name == "source" => child.attr("src"),
                    _ => None,
                })
            })
            .unwrap_or("")
            .trim();
        if src.is_empty() {
            self.diagnostic(
                "html_media_reference_missing",
                "warning",
                Some(element.line),
                "A media element was skipped because it had no source.",
            );
            return unsupported_block(element.line, &element.name, "Media element had no source.");
        }
        if !self.keep_external_file_references {
            self.diagnostic(
                "html_media_reference_skipped",
                "warning",
                Some(element.line),
                "Media references are skipped unless the user chooses to keep HTTPS references.",
            );
            return unsupported_block(
                element.line,
                &element.name,
                "Media reference was skipped by import policy.",
            );
        }
        if !src.to_ascii_lowercase().starts_with("https://") {
            self.diagnostic(
                "html_media_reference_requires_file_policy",
                "warning",
                Some(element.line),
                "Local or non-HTTPS media references require the import file policy flow.",
            );
            return unsupported_block(
                element.line,
                &element.name,
                "Media reference was not imported because it was not an HTTPS URL.",
            );
        }
        let caption = element
            .attr("alt")
            .or_else(|| element.attr("title"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| vec![rich_text(value, InlineStyle::default())])
            .unwrap_or_default();
        let payload = json!({
            "caption": caption,
            "type": "external",
            "external": {
                "url": src
            }
        });
        if let Err(error) = validate_block_payload(block_type, &payload) {
            self.diagnostic(
                "html_media_reference_unsupported",
                "warning",
                Some(element.line),
                error,
            );
            return unsupported_block(
                element.line,
                &element.name,
                "Media reference was not supported by the Notes media block.",
            );
        }
        html_block(block_type, payload, element.line, Vec::new())
    }

    fn rich_text_from_nodes(&mut self, nodes: &[HtmlNode]) -> Vec<Value> {
        let mut items = Vec::new();
        for node in nodes {
            self.append_inline_node(node, InlineStyle::default(), &mut items);
        }
        if items.is_empty() {
            items.push(rich_text("", InlineStyle::default()));
        }
        items
    }

    fn append_inline_node(&mut self, node: &HtmlNode, style: InlineStyle, items: &mut Vec<Value>) {
        match node {
            HtmlNode::Text { text, .. } => {
                if !text.is_empty() {
                    items.push(rich_text(&normalize_inline_text(text), style));
                }
            }
            HtmlNode::Element(element) if element.name == "br" => {
                items.push(rich_text("\n", style));
            }
            HtmlNode::Element(element) if element.name == "input" => {}
            HtmlNode::Element(element) if element.name == "img" => {
                if let Some(alt) = element.attr("alt").filter(|value| !value.trim().is_empty()) {
                    items.push(rich_text(alt.trim(), style));
                }
                self.diagnostic(
                    "html_inline_image_preserved_as_text",
                    "warning",
                    Some(element.line),
                    "Inline images are not editable inline objects. Alt text was preserved when available.",
                );
            }
            HtmlNode::Element(element) => {
                let mut next_style = style;
                match element.name.as_str() {
                    "b" | "strong" => next_style.bold = true,
                    "i" | "em" => next_style.italic = true,
                    "u" => next_style.underline = true,
                    "s" | "strike" | "del" => next_style.strikethrough = true,
                    "code" => next_style.code = true,
                    "a" => {
                        if let Some(href) = element.attr("href") {
                            if is_rich_text_url(href) {
                                next_style.href = Some(href.to_string());
                            } else {
                                self.diagnostic(
                                    "html_link_url_unsupported",
                                    "warning",
                                    Some(element.line),
                                    "Only HTTP, HTTPS, and mailto links are imported as rich text links.",
                                );
                            }
                        }
                    }
                    _ => {}
                }
                for child in &element.children {
                    self.append_inline_node(child, next_style.clone(), items);
                }
            }
        }
    }

    fn diagnostic(
        &mut self,
        code: &str,
        severity: &str,
        line: Option<i64>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(NoteHtmlImportDiagnosticDto::new(
            code, severity, line, message,
        ));
    }
}

#[derive(Clone, Default)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    underline: bool,
    code: bool,
    href: Option<String>,
}

#[derive(Clone)]
struct TableCell {
    header: bool,
    children: Vec<HtmlNode>,
}

fn table_rows(element: &HtmlElement) -> Vec<Vec<TableCell>> {
    let mut rows = Vec::new();
    collect_table_rows(element, &mut rows);
    rows
}

fn collect_table_rows(element: &HtmlElement, rows: &mut Vec<Vec<TableCell>>) {
    if element.name == "tr" {
        let cells = element
            .children
            .iter()
            .filter_map(|node| match node {
                HtmlNode::Element(cell) if matches!(cell.name.as_str(), "td" | "th") => {
                    Some(TableCell {
                        header: cell.name == "th",
                        children: cell.children.clone(),
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if !cells.is_empty() {
            rows.push(cells);
        }
        return;
    }
    for child in &element.children {
        if let HtmlNode::Element(child) = child {
            collect_table_rows(child, rows);
        }
    }
}

fn checkbox_state(element: &HtmlElement) -> Option<bool> {
    element.children.iter().find_map(|node| match node {
        HtmlNode::Element(child) if child.name == "input" => {
            let input_type = child.attr("type").unwrap_or("").to_ascii_lowercase();
            (input_type == "checkbox").then(|| child.has_attr("checked"))
        }
        _ => None,
    })
}

fn has_block_children(nodes: &[HtmlNode]) -> bool {
    nodes.iter().any(|node| match node {
        HtmlNode::Element(element) => is_block_element(&element.name),
        HtmlNode::Text { .. } => false,
    })
}

fn is_inline_node(node: &HtmlNode) -> bool {
    match node {
        HtmlNode::Text { .. } => true,
        HtmlNode::Element(element) => !is_block_element(&element.name),
    }
}

fn is_block_element(name: &str) -> bool {
    matches!(
        name,
        "article"
            | "aside"
            | "audio"
            | "blockquote"
            | "body"
            | "details"
            | "div"
            | "figcaption"
            | "figure"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "html"
            | "img"
            | "li"
            | "main"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "ul"
            | "video"
    )
}

fn html_block(
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

fn text_payload(text: &str) -> Value {
    json!({
        "rich_text": [rich_text(text, InlineStyle::default())],
        "color": "default"
    })
}

fn rich_text(text: &str, style: InlineStyle) -> Value {
    let href = style.href.as_deref();
    json!({
        "type": "text",
        "text": {
            "content": text,
            "link": href.map(|url| json!({ "url": url }))
        },
        "annotations": {
            "bold": style.bold,
            "italic": style.italic,
            "strikethrough": style.strikethrough,
            "underline": style.underline,
            "code": style.code,
            "color": "default"
        },
        "plain_text": text,
        "href": href
    })
}

fn unsupported_block(line: i64, tag: &str, warning: &str) -> ImportBlock {
    html_block(
        "unsupported",
        json!({
            "block_type": tag,
            "source_type": "html_import",
            "raw": {
                "tag": tag,
                "line": line
            },
            "warnings": [warning]
        }),
        line,
        Vec::new(),
    )
}

fn rich_text_plain_text(items: &[Value]) -> String {
    items
        .iter()
        .filter_map(|item| item.get("plain_text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn node_line(node: &HtmlNode) -> i64 {
    match node {
        HtmlNode::Element(element) => element.line,
        HtmlNode::Text { line, .. } => *line,
    }
}

fn node_text(nodes: &[HtmlNode]) -> String {
    let mut text = String::new();
    for node in nodes {
        match node {
            HtmlNode::Text { text: value, .. } => text.push_str(value),
            HtmlNode::Element(element) if element.name == "br" => text.push('\n'),
            HtmlNode::Element(element) => text.push_str(&node_text(&element.children)),
        }
    }
    text
}

fn normalize_inline_text(value: &str) -> String {
    value.replace('\t', " ")
}

fn is_rich_text_url(value: &str) -> bool {
    let trimmed = value.trim();
    let Ok(parsed) = reqwest::Url::parse(trimmed) else {
        return false;
    };
    match parsed.scheme() {
        "http" | "https" => parsed.host_str().is_some(),
        "mailto" => trimmed.contains('@'),
        _ => false,
    }
}

fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let title_start = lower.find("<title")?;
    let content_start = lower[title_start..].find('>')? + title_start + 1;
    let content_end = lower[content_start..].find("</title>")? + content_start;
    let text = strip_tags(&decode_html_entities(&html[content_start..content_end]));
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().take(240).collect())
}

fn strip_tags(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
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
    "Imported HTML".to_string()
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

fn scan_html_diagnostics(html: &str) -> Vec<NoteHtmlImportDiagnosticDto> {
    let mut diagnostics = Vec::new();
    let lower = html.to_ascii_lowercase();
    for pattern in ["<script", "<style", "<iframe", "<svg", "<math"] {
        if let Some(index) = lower.find(pattern) {
            diagnostics.push(NoteHtmlImportDiagnosticDto::new(
                "html_unsafe_markup_removed",
                "warning",
                Some(line_for_offset(html, index)),
                "Unsafe HTML markup was removed before import.",
            ));
        }
    }
    for pattern in ["javascript:", " onerror=", " onclick=", " onload="] {
        if let Some(index) = lower.find(pattern) {
            diagnostics.push(NoteHtmlImportDiagnosticDto::new(
                "html_unsafe_attribute_removed",
                "warning",
                Some(line_for_offset(html, index)),
                "Unsafe HTML attributes or URL schemes were removed before import.",
            ));
        }
    }
    diagnostics.extend(unsupported_tag_diagnostics(html));
    diagnostics
}

fn unsupported_tag_diagnostics(html: &str) -> Vec<NoteHtmlImportDiagnosticDto> {
    let allowed = HashSet::<&str>::from_iter(ALLOWED_TAGS.iter().copied());
    let clean = HashSet::<&str>::from_iter(CLEAN_CONTENT_TAGS.iter().copied());
    let mut seen = HashSet::new();
    let mut diagnostics = Vec::new();
    let mut cursor = 0;
    while let Some(relative_start) = html[cursor..].find('<') {
        let start = cursor + relative_start;
        cursor = start + 1;
        let rest = &html[cursor..];
        if rest.starts_with(['/', '!', '?']) {
            continue;
        }
        let name = rest
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
            .collect::<String>()
            .to_ascii_lowercase();
        if name.is_empty() || allowed.contains(name.as_str()) || clean.contains(name.as_str()) {
            continue;
        }
        if seen.insert(name.clone()) {
            diagnostics.push(NoteHtmlImportDiagnosticDto::new(
                "html_element_unsupported",
                "warning",
                Some(line_for_offset(html, start)),
                format!("HTML <{name}> elements are not mapped to editable Notes blocks."),
            ));
        }
    }
    diagnostics
}

fn line_for_offset(value: &str, offset: usize) -> i64 {
    value[..offset.min(value.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count() as i64
        + 1
}
