use crate::links::{
    LocalLinkResolver, block_id_from_local_notes_url, page_ids_from_local_notes_url,
};
use crate::models::NoteHtmlExportDiagnosticDto;
use crate::transfers::html_export::format::{escape_attr, escape_html, relative_link};
use crate::transfers::html_export::{ExportBlock, ExportCommentThread, ExportDatabase, ExportPage};
use crate::validation::rich_text_items_plain_text;
use serde_json::Value;
use std::collections::HashMap;

pub(crate) struct HtmlRenderInput<'a> {
    pub(crate) page: &'a ExportPage,
    pub(crate) blocks: &'a [ExportBlock],
    pub(crate) child_pages: &'a [&'a ExportPage],
    pub(crate) comments: &'a [ExportCommentThread],
    pub(crate) databases: &'a HashMap<String, ExportDatabase>,
    pub(crate) page_paths: &'a HashMap<String, String>,
    pub(crate) asset_paths: &'a HashMap<String, String>,
    pub(crate) local_link_resolver: &'a LocalLinkResolver,
}

pub(crate) struct HtmlRenderOutput {
    pub(crate) html: String,
    pub(crate) diagnostics: Vec<NoteHtmlExportDiagnosticDto>,
    pub(crate) exported_block_count: i64,
    pub(crate) exported_comment_count: i64,
}

pub(crate) fn render_page(input: HtmlRenderInput<'_>) -> HtmlRenderOutput {
    let mut renderer = HtmlRenderer {
        current_page_path: &input.page.path,
        page_paths: input.page_paths,
        asset_paths: input.asset_paths,
        databases: input.databases,
        local_link_resolver: input.local_link_resolver,
        diagnostics: Vec::new(),
        exported_block_count: 0,
        exported_comment_count: 0,
    };
    if input.page.row.cover.is_some() {
        renderer.warn_source(
            Some(&input.page.row.id), None, None, None,
            "page_cover_omitted",
            "Page cover appearance is not included in HTML. Use graph export to preserve editable cover metadata.",
        );
    }
    let css = relative_link(&input.page.path, "assets/ganbaru-notes-export.css");
    let body = renderer.render_blocks(input.blocks);
    let child_pages = renderer.render_child_page_list(input.child_pages);
    let comments = renderer.render_comments(input.comments);
    let title = escape_html(&input.page.title);
    let html = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{title}</title>\n<link rel=\"stylesheet\" href=\"{}\">\n</head>\n<body>\n<main class=\"page\">\n<header class=\"page-header\">\n<h1>{title}</h1>\n<p class=\"page-meta\">Created {}. Edited {}.</p>\n</header>\n{}\n<section class=\"blocks\">\n{}\n</section>\n{}\n</main>\n</body>\n</html>\n",
        escape_attr(&css),
        escape_html(&input.page.row.created_time),
        escape_html(&input.page.row.last_edited_time),
        child_pages,
        body,
        comments,
    );
    HtmlRenderOutput {
        html,
        diagnostics: renderer.diagnostics,
        exported_block_count: renderer.exported_block_count,
        exported_comment_count: renderer.exported_comment_count,
    }
}

struct HtmlRenderer<'a> {
    current_page_path: &'a str,
    page_paths: &'a HashMap<String, String>,
    asset_paths: &'a HashMap<String, String>,
    databases: &'a HashMap<String, ExportDatabase>,
    local_link_resolver: &'a LocalLinkResolver,
    diagnostics: Vec<NoteHtmlExportDiagnosticDto>,
    exported_block_count: i64,
    exported_comment_count: i64,
}

impl HtmlRenderer<'_> {
    fn render_blocks(&mut self, blocks: &[ExportBlock]) -> String {
        blocks
            .iter()
            .map(|block| self.render_block(block))
            .filter(|html| !html.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_block(&mut self, block: &ExportBlock) -> String {
        let rendered = self.render_block_content(block);
        let indent = block
            .payload
            .get("ganbaru_indent")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if indent == 0 {
            rendered
        } else {
            format!("<div style=\"margin-inline-start:calc({indent} * 1.25rem)\">{rendered}</div>")
        }
    }

    fn render_block_content(&mut self, block: &ExportBlock) -> String {
        self.exported_block_count += 1;
        let inner = match block.row.block_type.as_str() {
            "paragraph" => tag("p", &self.rich_text_payload(block, &block.payload)),
            "heading_1" | "heading_2" | "heading_3" | "heading_4" | "heading_5" | "heading_6" => {
                self.heading(block)
            }
            "bulleted_list_item" => self.list_item(block, "ul"),
            "numbered_list_item" => self.list_item(block, "ol"),
            "to_do" => self.todo(block),
            "toggle" => self.toggle(block),
            "callout" => self.callout(block),
            "quote" => tag("blockquote", &self.rich_text_payload(block, &block.payload)),
            "code" => self.code(block),
            "divider" => "<hr>".to_string(),
            "equation" => self.equation(block),
            "image" | "video" | "audio" | "file" | "pdf" => self.media(block),
            "bookmark" | "embed" | "link_preview" => self.external_reference(block),
            "child_page" => self.child_page(block),
            "child_database" => self.child_database(block),
            "table" => self.table(block),
            "table_row" => self.table_row(block),
            "column_list" | "column" | "tab" | "synced_block" | "template" | "button" => {
                self.container(block)
            }
            "breadcrumb" | "table_of_contents" => String::new(),
            "unsupported" => self.unsupported(block, "html_export_unsupported_block"),
            _ => self.unsupported(block, "html_export_unknown_block"),
        };
        if block.children.is_empty() || matches!(block.row.block_type.as_str(), "table") {
            anchor_block(&block.row.id, &inner)
        } else {
            anchor_block(
                &block.row.id,
                &format!("{}\n{}", inner, self.render_blocks(&block.children)),
            )
        }
    }

    fn heading(&mut self, block: &ExportBlock) -> String {
        let level = match block.row.block_type.as_str() {
            "heading_1" => 1,
            "heading_2" => 2,
            "heading_3" => 3,
            "heading_4" => 4,
            "heading_5" => 5,
            _ => 6,
        };
        tag(
            &format!("h{level}"),
            &self.rich_text_payload(block, &block.payload),
        )
    }

    fn list_item(&mut self, block: &ExportBlock, list_tag: &str) -> String {
        format!(
            "<{list_tag}><li>{}</li></{list_tag}>",
            self.rich_text_payload(block, &block.payload)
        )
    }

    fn todo(&mut self, block: &ExportBlock) -> String {
        let checked = if block
            .payload
            .get("checked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            " checked"
        } else {
            ""
        };
        format!(
            "<p class=\"todo\"><input type=\"checkbox\" disabled{checked}> {}</p>",
            self.rich_text_payload(block, &block.payload)
        )
    }

    fn toggle(&mut self, block: &ExportBlock) -> String {
        let summary = self.rich_text_payload(block, &block.payload);
        let children = self.render_blocks(&block.children);
        format!("<details open><summary>{summary}</summary>\n{children}\n</details>")
    }

    fn callout(&mut self, block: &ExportBlock) -> String {
        let icon = block
            .payload
            .get("icon")
            .and_then(|icon| icon.get("emoji"))
            .and_then(Value::as_str)
            .map(escape_html)
            .unwrap_or_default();
        format!(
            "<aside class=\"callout\"><span>{}</span><div>{}</div></aside>",
            icon,
            self.rich_text_payload(block, &block.payload)
        )
    }

    fn code(&mut self, block: &ExportBlock) -> String {
        let language = block
            .payload
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let code = block
            .payload
            .get("rich_text")
            .and_then(Value::as_array)
            .map(|items| rich_text_items_plain_text(items))
            .unwrap_or_default();
        format!(
            "<pre><code data-language=\"{}\">{}</code></pre>",
            escape_attr(language),
            escape_html(&code)
        )
    }

    fn equation(&mut self, block: &ExportBlock) -> String {
        let expression = block
            .payload
            .get("expression")
            .and_then(Value::as_str)
            .unwrap_or_default();
        format!("<div class=\"equation\">{}</div>", escape_html(expression))
    }

    fn media(&mut self, block: &ExportBlock) -> String {
        let label = media_label(&block.payload, &block.row.block_type);
        let Some(source) = media_source(&block.payload) else {
            self.warn_block(
                block,
                "html_export_media_missing_reference",
                "Media block has no exportable reference",
            );
            return tag("p", &escape_html(&label));
        };
        let url = self.export_media_url(block, &source);
        match block.row.block_type.as_str() {
            "image" => format!(
                "<figure><img src=\"{}\" alt=\"{}\"><figcaption>{}</figcaption></figure>",
                escape_attr(&url),
                escape_attr(&label),
                escape_html(&label)
            ),
            "video" => format!(
                "<video controls src=\"{}\">{}</video>",
                escape_attr(&url),
                escape_html(&label)
            ),
            "audio" => format!(
                "<audio controls src=\"{}\">{}</audio>",
                escape_attr(&url),
                escape_html(&label)
            ),
            _ => format!(
                "<p><a href=\"{}\">{}</a></p>",
                escape_attr(&url),
                escape_html(&label)
            ),
        }
    }

    fn external_reference(&mut self, block: &ExportBlock) -> String {
        let url = block
            .payload
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !safe_href(url) {
            self.warn_block(
                block,
                "html_export_external_reference_skipped",
                "External reference URL was not safe to export",
            );
            return tag("p", &escape_html(&block.row.block_type));
        }
        let label = block
            .payload
            .get("caption")
            .and_then(Value::as_array)
            .map(|items| rich_text_items_plain_text(items))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| url.to_string());
        format!(
            "<p><a href=\"{}\">{}</a></p>",
            escape_attr(url),
            escape_html(&label)
        )
    }

    fn child_page(&mut self, block: &ExportBlock) -> String {
        let title = block
            .payload
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Untitled");
        if let Some(page_id) = block.child_page_id.as_deref() {
            if let Some(path) = self.page_paths.get(page_id) {
                let link = relative_link(self.current_page_path, path);
                return format!(
                    "<p class=\"child-page\"><a href=\"{}\">{}</a></p>",
                    escape_attr(&link),
                    escape_html(title)
                );
            }
        }
        self.warn_block(
            block,
            "html_export_child_page_unresolved",
            "Child page link did not resolve inside this archive",
        );
        format!("<p class=\"child-page\">{}</p>", escape_html(title))
    }

    fn child_database(&mut self, block: &ExportBlock) -> String {
        let title = block
            .payload
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Untitled database");
        let Some(database) = self.databases.get(&block.row.id) else {
            self.warn_block(
                block,
                "html_export_database_metadata_missing",
                "Database view metadata was not available",
            );
            return format!(
                "<section class=\"database\"><h2>{}</h2></section>",
                escape_html(title)
            );
        };
        let link = relative_link(self.current_page_path, &database.path);
        format!(
            "<section class=\"database\"><h2>{}</h2><p><a href=\"{}\">Database manifest</a></p></section>",
            escape_html(title),
            escape_attr(&link)
        )
    }

    fn table(&mut self, block: &ExportBlock) -> String {
        let rows = block
            .children
            .iter()
            .filter(|child| child.row.block_type == "table_row")
            .collect::<Vec<_>>();
        let body = rows
            .iter()
            .map(|row| self.table_row(row))
            .collect::<Vec<_>>()
            .join("\n");
        format!("<table><tbody>{body}</tbody></table>")
    }

    fn table_row(&mut self, block: &ExportBlock) -> String {
        let cells = block
            .payload
            .get("cells")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let row = cells
            .iter()
            .map(|cell| {
                let html = cell
                    .as_array()
                    .map(|items| self.rich_text(items, Some(&block.row.id), None))
                    .unwrap_or_default();
                format!("<td>{html}</td>")
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<tr>{row}</tr>")
    }

    fn container(&mut self, block: &ExportBlock) -> String {
        let label = self.rich_text_payload(block, &block.payload);
        let children = self.render_blocks(&block.children);
        format!("<section>{label}\n{children}</section>")
    }

    fn unsupported(&mut self, block: &ExportBlock, code: &'static str) -> String {
        self.warn_block(
            block,
            code,
            "Block content was preserved as a readable warning",
        );
        let label = block
            .payload
            .get("block_type")
            .and_then(Value::as_str)
            .unwrap_or(&block.row.block_type);
        format!(
            "<div class=\"warning\">Unsupported block: {}</div>",
            escape_html(label)
        )
    }

    fn rich_text_payload(&mut self, block: &ExportBlock, payload: &Value) -> String {
        payload
            .get("rich_text")
            .and_then(Value::as_array)
            .map(|items| self.rich_text(items, Some(&block.row.id), None))
            .unwrap_or_default()
    }

    fn rich_text(
        &mut self,
        items: &[Value],
        block_id: Option<&str>,
        comment_id: Option<&str>,
    ) -> String {
        items
            .iter()
            .map(|item| self.rich_text_item(item, block_id, comment_id))
            .collect::<Vec<_>>()
            .join("")
    }

    fn rich_text_item(
        &mut self,
        item: &Value,
        block_id: Option<&str>,
        comment_id: Option<&str>,
    ) -> String {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("text");
        let mut rendered = match kind {
            "text" => escape_html(
                item.get("text")
                    .and_then(|text| text.get("content"))
                    .and_then(Value::as_str)
                    .or_else(|| item.get("plain_text").and_then(Value::as_str))
                    .unwrap_or_default(),
            ),
            "mention" => self.mention(item, block_id, comment_id),
            "equation" => {
                let expression = item
                    .get("equation")
                    .and_then(|equation| equation.get("expression"))
                    .and_then(Value::as_str)
                    .or_else(|| item.get("plain_text").and_then(Value::as_str))
                    .unwrap_or_default();
                format!(
                    "<span class=\"equation\">{}</span>",
                    escape_html(expression)
                )
            }
            _ => {
                self.warn_source(
                    block_id,
                    None,
                    None,
                    comment_id,
                    "html_export_rich_text_unsupported",
                    "Unsupported rich text item was exported as plain text",
                );
                escape_html(
                    item.get("plain_text")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
            }
        };
        rendered = apply_annotations(rendered, item.get("annotations"));
        if let Some(href) = rich_text_href(item).filter(|href| !href.trim().is_empty()) {
            rendered = self.linked_text(rendered, href, block_id, comment_id);
        }
        rendered
    }

    fn mention(
        &mut self,
        item: &Value,
        block_id: Option<&str>,
        comment_id: Option<&str>,
    ) -> String {
        let label = item
            .get("plain_text")
            .and_then(Value::as_str)
            .unwrap_or("Mention");
        if item
            .get("mention")
            .and_then(|mention| mention.get("type"))
            .and_then(Value::as_str)
            == Some("page")
        {
            let page_id = item
                .get("mention")
                .and_then(|mention| mention.get("page"))
                .and_then(|page| page.get("id"))
                .and_then(Value::as_str);
            if let Some(page_id) = page_id {
                if let Some(path) = self.page_paths.get(page_id) {
                    let link = relative_link(self.current_page_path, path);
                    return format!(
                        "<a class=\"mention\" href=\"{}\">{}</a>",
                        escape_attr(&link),
                        escape_html(label)
                    );
                }
            }
            self.warn_source(
                page_id,
                block_id,
                None,
                comment_id,
                "html_export_page_mention_external",
                "Page mention target was outside this archive",
            );
        }
        format!("<span class=\"mention\">{}</span>", escape_html(label))
    }

    fn linked_text(
        &mut self,
        rendered: String,
        href: &str,
        block_id: Option<&str>,
        comment_id: Option<&str>,
    ) -> String {
        if href.contains("#notes?") {
            if let Some(page_id) =
                page_ids_from_local_notes_url(href, self.local_link_resolver).first()
            {
                if let Some(path) = self.page_paths.get(page_id) {
                    let mut link = relative_link(self.current_page_path, path);
                    if let Some(block_id) = block_id_from_local_notes_url(href) {
                        link.push('#');
                        link.push_str(&block_id);
                    }
                    return format!("<a href=\"{}\">{rendered}</a>", escape_attr(&link));
                }
            }
            self.warn_source(
                None,
                block_id,
                None,
                comment_id,
                "html_export_local_link_external",
                "Local Notes link target was outside this archive",
            );
            return rendered;
        }
        if !safe_href(href) {
            self.warn_source(
                None,
                block_id,
                None,
                comment_id,
                "html_export_unsafe_link_skipped",
                "Unsafe rich text link was omitted from HTML export",
            );
            return rendered;
        }
        format!("<a href=\"{}\">{rendered}</a>", escape_attr(href))
    }

    fn render_child_page_list(&mut self, child_pages: &[&ExportPage]) -> String {
        if child_pages.is_empty() {
            return String::new();
        }
        let items = child_pages
            .iter()
            .filter_map(|page| {
                self.page_paths.get(&page.row.id).map(|path| {
                    format!(
                        "<li><a href=\"{}\">{}</a></li>",
                        escape_attr(&relative_link(self.current_page_path, path)),
                        escape_html(&page.title)
                    )
                })
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<nav class=\"subpages\"><h2>Subpages</h2><ul>{items}</ul></nav>")
    }

    fn render_comments(&mut self, threads: &[ExportCommentThread]) -> String {
        if threads.is_empty() {
            return String::new();
        }
        let mut output = String::from("<section class=\"comments\"><h2>Comments</h2>");
        for thread in threads {
            output.push_str("<article class=\"comment-thread\">");
            output.push_str(&format!(
                "<h3>{}</h3>",
                escape_html(thread.anchor_label.as_deref().unwrap_or("Page discussion"))
            ));
            for comment in &thread.comments {
                self.exported_comment_count += 1;
                let items = serde_json::from_str::<Value>(&comment.rich_text)
                    .ok()
                    .and_then(|value| value.as_array().cloned())
                    .unwrap_or_default();
                output.push_str(&format!(
                    "<p><strong>{}</strong>: {}</p>",
                    escape_html(&comment.author_name),
                    self.rich_text(&items, None, Some(&comment.id))
                ));
            }
            output.push_str("</article>");
        }
        output.push_str("</section>");
        output
    }

    fn export_media_url(&mut self, block: &ExportBlock, source: &str) -> String {
        if let Some(asset_path) = source.strip_prefix("ganbaru-asset:") {
            if let Some(archive_path) = self.asset_paths.get(asset_path) {
                return relative_link(self.current_page_path, archive_path);
            }
            self.warn_block(
                block,
                "html_export_local_asset_not_included",
                "Local managed asset was not included in this archive",
            );
        }
        source.to_string()
    }

    fn warn_block(&mut self, block: &ExportBlock, code: &'static str, message: &'static str) {
        self.warn_source(
            Some(&block.row.page_id),
            Some(&block.row.id),
            None,
            None,
            code,
            message,
        );
    }

    fn warn_source(
        &mut self,
        page_id: Option<&str>,
        block_id: Option<&str>,
        asset_id: Option<&str>,
        comment_id: Option<&str>,
        code: &'static str,
        message: &'static str,
    ) {
        self.diagnostics.push(NoteHtmlExportDiagnosticDto::new(
            code,
            "warning",
            page_id.map(str::to_string),
            block_id.map(str::to_string),
            asset_id.map(str::to_string),
            comment_id.map(str::to_string),
            message,
        ));
    }
}

fn tag(name: &str, contents: &str) -> String {
    format!("<{name}>{contents}</{name}>")
}

fn anchor_block(id: &str, contents: &str) -> String {
    format!(
        "<div class=\"block\" id=\"{}\">{contents}</div>",
        escape_attr(id)
    )
}

fn media_label(payload: &Value, block_type: &str) -> String {
    payload
        .get("caption")
        .and_then(Value::as_array)
        .map(|items| rich_text_items_plain_text(items))
        .filter(|caption| !caption.trim().is_empty())
        .or_else(|| {
            payload
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| block_type.to_string())
}

fn media_source(payload: &Value) -> Option<String> {
    match payload.get("type").and_then(Value::as_str)? {
        "external" => payload
            .get("external")?
            .get("url")?
            .as_str()
            .map(str::to_string),
        "file" => payload
            .get("file")?
            .get("url")?
            .as_str()
            .map(str::to_string),
        "file_upload" => payload
            .get("file_upload")?
            .get("id")?
            .as_str()
            .map(|id| format!("file-upload:{id}")),
        _ => None,
    }
}

fn rich_text_href(item: &Value) -> Option<&str> {
    item.get("href")
        .and_then(Value::as_str)
        .or_else(|| item.get("text")?.get("link")?.get("url")?.as_str())
}

fn apply_annotations(mut rendered: String, annotations: Option<&Value>) -> String {
    let Some(annotations) = annotations else {
        return rendered;
    };
    if annotations
        .get("code")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        rendered = tag("code", &rendered);
    }
    if annotations
        .get("bold")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        rendered = tag("strong", &rendered);
    }
    if annotations
        .get("italic")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        rendered = tag("em", &rendered);
    }
    if annotations
        .get("strikethrough")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        rendered = tag("s", &rendered);
    }
    if annotations
        .get("underline")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        rendered = tag("u", &rendered);
    }
    let color = annotations
        .get("color")
        .and_then(Value::as_str)
        .unwrap_or("default");
    if color != "default" {
        rendered = format!(
            "<span data-color=\"{}\">{rendered}</span>",
            escape_attr(color)
        );
    }
    rendered
}

fn safe_href(url: &str) -> bool {
    let trimmed = url.trim().to_ascii_lowercase();
    trimmed.starts_with("https://")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("mailto:")
}
