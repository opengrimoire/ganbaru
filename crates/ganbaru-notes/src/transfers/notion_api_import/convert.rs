use crate::models::NoteNotionApiImportDiagnosticDto;
use crate::transfers::import_writer::ImportBlock;
use crate::validation::{validate_block_payload, validate_icon_value, validate_page_cover_value};
use serde_json::{Map, Value, json};

const LOCAL_COLORS: &[&str] = &[
    "default",
    "gray",
    "brown",
    "orange",
    "yellow",
    "green",
    "blue",
    "purple",
    "pink",
    "red",
    "gray_background",
    "brown_background",
    "orange_background",
    "yellow_background",
    "green_background",
    "blue_background",
    "purple_background",
    "pink_background",
    "red_background",
];

const SUPPORTED_PROPERTY_TYPES: &[&str] = &[
    "title",
    "rich_text",
    "number",
    "select",
    "multi_select",
    "status",
    "date",
    "checkbox",
    "url",
    "email",
    "phone_number",
    "files",
    "people",
    "created_time",
    "created_by",
    "last_edited_time",
    "last_edited_by",
    "unique_id",
    "place",
    "relation",
    "rollup",
    "formula",
    "button",
];

const STATUS_GROUPS: &[&str] = &["To-do", "In progress", "Complete"];

#[derive(Default)]
pub(crate) struct NotionConvertOptions {
    pub(crate) keep_external_file_references: bool,
}

#[derive(Default)]
pub(crate) struct NotionConvertStats {
    pub(crate) imported_file_count: i64,
    pub(crate) unsupported_block_count: i64,
}

pub(crate) struct FetchedNotionBlock {
    pub(crate) raw: Value,
    pub(crate) children: Vec<FetchedNotionBlock>,
}

pub(crate) struct ConvertedNotionPage {
    pub(crate) source_id: String,
    pub(crate) title: String,
    pub(crate) icon: Option<Value>,
    pub(crate) cover: Option<Value>,
    pub(crate) url: Option<String>,
    pub(crate) public_url: Option<String>,
    pub(crate) last_edited_time: Option<String>,
    pub(crate) blocks: Vec<ImportBlock>,
}

pub(crate) struct ConvertedNotionDataSource {
    pub(crate) source_id: String,
    pub(crate) title: String,
    pub(crate) icon: Option<Value>,
    pub(crate) url: Option<String>,
    pub(crate) last_edited_time: Option<String>,
    pub(crate) properties: Value,
    pub(crate) property_order: Vec<String>,
}

pub(crate) struct ConvertedNotionRow {
    pub(crate) source_id: String,
    pub(crate) title: String,
    pub(crate) properties: Value,
    pub(crate) last_edited_time: Option<String>,
    pub(crate) blocks: Vec<ImportBlock>,
    pub(crate) comments: Vec<Value>,
}

pub(crate) fn convert_page(
    raw_page: &Value,
    blocks: Vec<FetchedNotionBlock>,
    options: &NotionConvertOptions,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
    stats: &mut NotionConvertStats,
) -> ConvertedNotionPage {
    let source_id = notion_id(raw_page).unwrap_or_else(|| "unknown-page".to_string());
    let title = page_title(raw_page).unwrap_or_else(|| "Untitled".to_string());
    let icon = optional_icon(raw_page.get("icon"), &source_id, diagnostics);
    let cover = optional_cover(raw_page.get("cover"), &source_id, diagnostics);
    let converted_blocks = blocks
        .into_iter()
        .map(|block| convert_block(block, options, diagnostics, stats))
        .collect::<Vec<_>>();
    ConvertedNotionPage {
        source_id,
        title,
        icon,
        cover,
        url: optional_string(raw_page, "url"),
        public_url: optional_string(raw_page, "public_url"),
        last_edited_time: optional_string(raw_page, "last_edited_time"),
        blocks: converted_blocks,
    }
}

pub(crate) fn convert_data_source(
    raw_data_source: &Value,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> ConvertedNotionDataSource {
    let source_id = notion_id(raw_data_source).unwrap_or_else(|| "unknown-data-source".to_string());
    let title = raw_data_source
        .get("title")
        .and_then(Value::as_array)
        .map(|items| rich_text_plain_text(items).trim().to_string())
        .and_then(|value| value.or_else_non_empty())
        .unwrap_or_else(|| "Untitled data source".to_string());
    let icon = optional_icon(raw_data_source.get("icon"), &source_id, diagnostics);
    let (properties, property_order) =
        data_source_properties(raw_data_source.get("properties"), &source_id, diagnostics);
    ConvertedNotionDataSource {
        source_id,
        title,
        icon,
        url: optional_string(raw_data_source, "url"),
        last_edited_time: optional_string(raw_data_source, "last_edited_time"),
        properties,
        property_order,
    }
}

pub(crate) fn convert_data_source_row(
    raw_page: &Value,
    blocks: Vec<FetchedNotionBlock>,
    comments: Vec<Value>,
    schema_properties: &Value,
    options: &NotionConvertOptions,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
    stats: &mut NotionConvertStats,
) -> ConvertedNotionRow {
    let source_id = notion_id(raw_page).unwrap_or_else(|| "unknown-row".to_string());
    let title = page_title(raw_page).unwrap_or_else(|| "Untitled".to_string());
    let properties = row_properties(raw_page.get("properties"), schema_properties, &source_id);
    let converted_blocks = blocks
        .into_iter()
        .map(|block| convert_block(block, options, diagnostics, stats))
        .collect::<Vec<_>>();
    ConvertedNotionRow {
        source_id,
        title,
        properties,
        last_edited_time: optional_string(raw_page, "last_edited_time"),
        blocks: converted_blocks,
        comments,
    }
}

pub(crate) fn comment_rich_text(raw: &Value) -> Vec<Value> {
    let mut diagnostics = Vec::new();
    raw.get("rich_text")
        .and_then(Value::as_array)
        .map(|items| notion_rich_text(items, None, &mut diagnostics))
        .unwrap_or_default()
}

fn convert_block(
    block: FetchedNotionBlock,
    options: &NotionConvertOptions,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
    stats: &mut NotionConvertStats,
) -> ImportBlock {
    let source_id = notion_id(&block.raw);
    let source_last_edited_time = optional_string(&block.raw, "last_edited_time");
    let notion_type = block
        .raw
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unsupported");
    let children = block
        .children
        .into_iter()
        .map(|child| convert_block(child, options, diagnostics, stats))
        .collect::<Vec<_>>();
    let (local_type, payload) = local_block_payload(
        notion_type,
        &block.raw,
        options,
        source_id.as_deref(),
        diagnostics,
        stats,
    );
    let payload = ensure_valid_block_payload(local_type, payload, &block.raw, diagnostics, stats);
    ImportBlock::with_source_time(
        local_type,
        payload,
        source_id,
        source_last_edited_time,
        children,
    )
}

fn local_block_payload(
    notion_type: &str,
    raw_block: &Value,
    options: &NotionConvertOptions,
    source_id: Option<&str>,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
    stats: &mut NotionConvertStats,
) -> (&'static str, Value) {
    let payload = raw_block.get(notion_type).unwrap_or(&Value::Null);
    match notion_type {
        "paragraph" | "heading_1" | "heading_2" | "heading_3" | "bulleted_list_item"
        | "numbered_list_item" | "toggle" | "callout" | "quote" => (
            text_block_type(notion_type),
            text_payload(payload, diagnostics),
        ),
        "to_do" => ("to_do", to_do_payload(payload, diagnostics)),
        "divider" => ("divider", json!({})),
        "code" => ("code", code_payload(payload, diagnostics)),
        "table_of_contents" => (
            "table_of_contents",
            json!({ "color": notion_color(payload.get("color").and_then(Value::as_str)) }),
        ),
        "breadcrumb" => ("breadcrumb", json!({})),
        "table" => ("table", table_payload(payload)),
        "table_row" => ("table_row", table_row_payload(payload, diagnostics)),
        "column_list" => ("column_list", json!({})),
        "column" => ("column", column_payload(payload)),
        "child_page" => ("child_page", child_page_payload(payload)),
        "child_database" => ("child_database", child_database_payload(payload)),
        "bookmark" => ("bookmark", bookmark_payload(payload, diagnostics)),
        "embed" => ("embed", url_payload(payload)),
        "link_preview" => ("link_preview", url_payload(payload)),
        "equation" => ("equation", equation_payload(payload)),
        "image" | "video" | "audio" | "file" | "pdf" => media_payload(
            notion_type,
            payload,
            raw_block,
            options,
            source_id,
            diagnostics,
            stats,
        ),
        "synced_block" => ("synced_block", synced_block_payload(payload)),
        _ => unsupported_payload(
            notion_type,
            raw_block,
            "Notion block type is not editable locally.",
        ),
    }
}

fn ensure_valid_block_payload(
    block_type: &'static str,
    payload: Value,
    raw_block: &Value,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
    stats: &mut NotionConvertStats,
) -> Value {
    if validate_block_payload(block_type, &payload).is_ok() {
        return payload;
    }
    stats.unsupported_block_count += 1;
    diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
        "unsupported_block_payload",
        "warning",
        notion_id(raw_block),
        format!("A Notion {block_type} block was preserved as unsupported."),
    ));
    unsupported_payload_value(
        raw_block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(block_type),
        raw_block,
        "Local validation rejected the converted block payload.",
    )
}

fn text_block_type(notion_type: &str) -> &'static str {
    match notion_type {
        "heading_1" => "heading_1",
        "heading_2" => "heading_2",
        "heading_3" => "heading_3",
        "bulleted_list_item" => "bulleted_list_item",
        "numbered_list_item" => "numbered_list_item",
        "toggle" => "toggle",
        "callout" => "callout",
        "quote" => "quote",
        _ => "paragraph",
    }
}

fn text_payload(payload: &Value, diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>) -> Value {
    let rich_text = payload
        .get("rich_text")
        .and_then(Value::as_array)
        .map(|items| notion_rich_text(items, None, diagnostics))
        .unwrap_or_default();
    let mut output = json!({
        "rich_text": rich_text,
        "color": notion_color(payload.get("color").and_then(Value::as_str))
    });
    if let Some(value) = payload.get("is_toggleable").and_then(Value::as_bool) {
        output["is_toggleable"] = Value::Bool(value);
    }
    if let Some(value) = payload.get("ganbaru_open").and_then(Value::as_bool) {
        output["ganbaru_open"] = Value::Bool(value);
    }
    if let Some(icon) = payload.get("icon") {
        output["icon"] = icon.clone();
    }
    output
}

fn to_do_payload(
    payload: &Value,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> Value {
    let mut output = text_payload(payload, diagnostics);
    output["checked"] = Value::Bool(
        payload
            .get("checked")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    );
    output
}

fn code_payload(payload: &Value, diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>) -> Value {
    json!({
        "rich_text": payload
            .get("rich_text")
            .and_then(Value::as_array)
            .map(|items| notion_rich_text(items, None, diagnostics))
            .unwrap_or_default(),
        "caption": payload
            .get("caption")
            .and_then(Value::as_array)
            .map(|items| notion_rich_text(items, None, diagnostics))
            .unwrap_or_default(),
        "language": payload
            .get("language")
            .and_then(Value::as_str)
            .map(clean_display_text)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "plain text".to_string())
    })
}

fn table_payload(payload: &Value) -> Value {
    json!({
        "table_width": payload
            .get("table_width")
            .and_then(Value::as_i64)
            .filter(|value| (1..=100).contains(value))
            .unwrap_or(1),
        "has_column_header": payload
            .get("has_column_header")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "has_row_header": payload
            .get("has_row_header")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    })
}

fn table_row_payload(
    payload: &Value,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> Value {
    let cells = payload
        .get("cells")
        .and_then(Value::as_array)
        .map(|cells| {
            cells
                .iter()
                .filter_map(Value::as_array)
                .map(|items| Value::Array(notion_rich_text(items, None, diagnostics)))
                .collect::<Vec<_>>()
        })
        .filter(|cells| !cells.is_empty())
        .unwrap_or_else(|| vec![Value::Array(Vec::new())]);
    json!({ "cells": cells })
}

fn column_payload(payload: &Value) -> Value {
    match payload.get("width_ratio").and_then(Value::as_f64) {
        Some(value) if value > 0.0 && value <= 1.0 => json!({ "width_ratio": value }),
        _ => json!({}),
    }
}

fn child_page_payload(payload: &Value) -> Value {
    json!({
        "title": payload
            .get("title")
            .and_then(Value::as_str)
            .map(clean_display_text)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Untitled".to_string())
    })
}

fn child_database_payload(payload: &Value) -> Value {
    json!({
        "title": payload
            .get("title")
            .and_then(Value::as_str)
            .map(clean_display_text)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Untitled database".to_string())
    })
}

fn bookmark_payload(
    payload: &Value,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> Value {
    json!({
        "url": payload
            .get("url")
            .and_then(Value::as_str)
            .map(clean_display_text)
            .unwrap_or_default(),
        "caption": payload
            .get("caption")
            .and_then(Value::as_array)
            .map(|items| notion_rich_text(items, None, diagnostics))
            .unwrap_or_default()
    })
}

fn url_payload(payload: &Value) -> Value {
    json!({
        "url": payload
            .get("url")
            .and_then(Value::as_str)
            .map(clean_display_text)
            .unwrap_or_default()
    })
}

fn equation_payload(payload: &Value) -> Value {
    json!({
        "expression": payload
            .get("expression")
            .and_then(Value::as_str)
            .map(clean_display_text)
            .unwrap_or_default()
    })
}

fn synced_block_payload(payload: &Value) -> Value {
    if let Some(synced_from) = payload.get("synced_from") {
        return json!({ "synced_from": synced_from });
    }
    json!({ "synced_from": null })
}

fn media_payload(
    notion_type: &str,
    payload: &Value,
    raw_block: &Value,
    options: &NotionConvertOptions,
    source_id: Option<&str>,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
    stats: &mut NotionConvertStats,
) -> (&'static str, Value) {
    if !options.keep_external_file_references {
        stats.unsupported_block_count += 1;
        diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
            "external_file_reference_skipped",
            "warning",
            source_id.map(str::to_string),
            "A Notion media block was preserved as unsupported because external file references were disabled.",
        ));
        return unsupported_payload(
            notion_type,
            raw_block,
            "External file references were disabled.",
        );
    }
    let Some(file_type) = payload.get("type").and_then(Value::as_str) else {
        return unsupported_payload(
            notion_type,
            raw_block,
            "Notion media block had no file type.",
        );
    };
    let mut output = json!({
        "type": file_type,
        "caption": payload
            .get("caption")
            .and_then(Value::as_array)
            .map(|items| notion_rich_text(items, None, diagnostics))
            .unwrap_or_default()
    });
    if let Some(name) = payload.get("name").and_then(Value::as_str) {
        output["name"] = Value::String(clean_display_text(name));
    }
    match file_type {
        "external" => output["external"] = payload.get("external").cloned().unwrap_or(json!({})),
        "file" => {
            output["file"] = payload.get("file").cloned().unwrap_or(json!({}));
            diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
                "temporary_notion_file_url",
                "warning",
                source_id.map(str::to_string),
                "A Notion-hosted file URL was imported as an external reference and may expire.",
            ));
        }
        "file_upload" => {
            output["file_upload"] = payload.get("file_upload").cloned().unwrap_or(json!({}));
        }
        _ => {
            return unsupported_payload(notion_type, raw_block, "Unsupported Notion media source.");
        }
    }
    stats.imported_file_count += 1;
    (media_block_type(notion_type), output)
}

fn media_block_type(notion_type: &str) -> &'static str {
    match notion_type {
        "image" => "image",
        "video" => "video",
        "audio" => "audio",
        "pdf" => "pdf",
        _ => "file",
    }
}

fn unsupported_payload(
    notion_type: &str,
    raw_block: &Value,
    warning: &str,
) -> (&'static str, Value) {
    (
        "unsupported",
        unsupported_payload_value(notion_type, raw_block, warning),
    )
}

fn unsupported_payload_value(notion_type: &str, raw_block: &Value, warning: &str) -> Value {
    let raw = raw_block
        .as_object()
        .cloned()
        .map(Value::Object)
        .unwrap_or_else(|| json!({ "value": raw_block }));
    json!({
        "block_type": clean_display_text(notion_type),
        "source_type": "notion_api",
        "raw": raw,
        "warnings": [warning]
    })
}

fn notion_rich_text(
    items: &[Value],
    source_id: Option<&str>,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> Vec<Value> {
    items
        .iter()
        .filter_map(|item| notion_rich_text_item(item, source_id, diagnostics))
        .collect()
}

fn notion_rich_text_item(
    item: &Value,
    source_id: Option<&str>,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> Option<Value> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("text");
    let plain_text = item
        .get("plain_text")
        .and_then(Value::as_str)
        .map(clean_display_text)
        .unwrap_or_default();
    let annotations = rich_text_annotations(item.get("annotations"));
    match item_type {
        "text" => {
            let content = item
                .get("text")
                .and_then(Value::as_object)
                .and_then(|text| text.get("content"))
                .and_then(Value::as_str)
                .map(clean_display_text)
                .unwrap_or(plain_text);
            let href = item
                .get("href")
                .and_then(Value::as_str)
                .map(clean_display_text);
            let link_url = item
                .get("text")
                .and_then(Value::as_object)
                .and_then(|text| text.get("link"))
                .and_then(Value::as_object)
                .and_then(|link| link.get("url"))
                .and_then(Value::as_str)
                .or(href.as_deref())
                .and_then(safe_rich_link);
            Some(json!({
                "type": "text",
                "text": {
                    "content": content,
                    "link": link_url.as_ref().map(|url| json!({ "url": url }))
                },
                "annotations": annotations,
                "plain_text": content,
                "href": link_url
            }))
        }
        "equation" => {
            let expression = item
                .get("equation")
                .and_then(Value::as_object)
                .and_then(|equation| equation.get("expression"))
                .and_then(Value::as_str)
                .map(clean_display_text)
                .unwrap_or_default();
            Some(json!({
                "type": "equation",
                "equation": { "expression": expression },
                "annotations": annotations,
                "plain_text": expression,
                "href": Value::Null
            }))
        }
        "mention" => {
            notion_mention_rich_text(item, plain_text, annotations, source_id, diagnostics)
        }
        _ if !plain_text.is_empty() => Some(plain_text_rich_text(&plain_text, annotations)),
        _ => None,
    }
}

fn notion_mention_rich_text(
    item: &Value,
    plain_text: String,
    annotations: Value,
    source_id: Option<&str>,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> Option<Value> {
    let mention = item.get("mention").and_then(Value::as_object)?;
    if mention.get("type").and_then(Value::as_str) == Some("date") {
        if let Some(date) = mention.get("date") {
            return Some(json!({
                "type": "mention",
                "mention": { "type": "date", "date": date },
                "annotations": annotations,
                "plain_text": plain_text,
                "href": Value::Null
            }));
        }
    }
    diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
        "external_mention_degraded",
        "warning",
        source_id.map(str::to_string),
        "A Notion mention was imported as plain text because it points outside local Ganbaru AI data.",
    ));
    if plain_text.is_empty() {
        None
    } else {
        Some(plain_text_rich_text(&plain_text, annotations))
    }
}

fn plain_text_rich_text(text: &str, annotations: Value) -> Value {
    json!({
        "type": "text",
        "text": {
            "content": text,
            "link": Value::Null
        },
        "annotations": annotations,
        "plain_text": text,
        "href": Value::Null
    })
}

fn rich_text_annotations(value: Option<&Value>) -> Value {
    let object = value.and_then(Value::as_object);
    json!({
        "bold": object
            .and_then(|item| item.get("bold"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "italic": object
            .and_then(|item| item.get("italic"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "strikethrough": object
            .and_then(|item| item.get("strikethrough"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "underline": object
            .and_then(|item| item.get("underline"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "code": object
            .and_then(|item| item.get("code"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "color": notion_color(object
            .and_then(|item| item.get("color"))
            .and_then(Value::as_str))
    })
}

fn page_title(raw_page: &Value) -> Option<String> {
    let properties = raw_page.get("properties")?.as_object()?;
    for value in properties.values() {
        if value.get("type").and_then(Value::as_str) == Some("title") {
            let title = value
                .get("title")
                .and_then(Value::as_array)
                .map(|items| rich_text_plain_text(items))
                .unwrap_or_default();
            return title.trim().to_string().or_else_non_empty();
        }
    }
    None
}

fn data_source_properties(
    value: Option<&Value>,
    source_id: &str,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> (Value, Vec<String>) {
    let mut properties = Map::new();
    let mut order = Vec::new();
    let mut title_found = false;
    if let Some(object) = value.and_then(Value::as_object) {
        for (key, raw_property) in object {
            let (name, property) = data_source_property(key, raw_property, source_id, diagnostics);
            if property.get("type").and_then(Value::as_str) == Some("title") {
                title_found = true;
            }
            let id = property
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(&name)
                .to_string();
            order.push(id);
            properties.insert(name, property);
        }
    }
    if !title_found {
        properties.insert(
            "Name".to_string(),
            json!({
                "id": "title",
                "name": "Name",
                "description": "",
                "type": "title",
                "title": {}
            }),
        );
        order.insert(0, "title".to_string());
        diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
            "missing_title_property",
            "warning",
            Some(source_id.to_string()),
            "Notion data source did not expose a title property, so a local title property was added.",
        ));
    }
    (Value::Object(properties), order)
}

fn data_source_property(
    key: &str,
    raw_property: &Value,
    source_id: &str,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> (String, Value) {
    let raw_type = raw_property
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("rich_text");
    let property_type = if SUPPORTED_PROPERTY_TYPES.contains(&raw_type) {
        raw_type
    } else {
        diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
            "unsupported_property_type",
            "warning",
            Some(source_id.to_string()),
            format!("Notion property type {raw_type} was imported as rich text."),
        ));
        "rich_text"
    };
    let name = raw_property
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(key)
        .trim()
        .to_string()
        .or_else_non_empty()
        .unwrap_or_else(|| "Property".to_string());
    let id = if property_type == "title" {
        "title".to_string()
    } else {
        raw_property
            .get("id")
            .and_then(Value::as_str)
            .map(clean_property_id)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| clean_property_id(&name))
    };
    let config = property_config(property_type, raw_property.get(property_type));
    (
        name.clone(),
        json!({
            "id": id,
            "name": name,
            "description": "",
            "type": property_type,
            property_type: config
        }),
    )
}

fn property_config(property_type: &str, value: Option<&Value>) -> Value {
    match property_type {
        "number" => json!({
            "format": value
                .and_then(Value::as_object)
                .and_then(|object| object.get("format"))
                .and_then(Value::as_str)
                .unwrap_or("number")
        }),
        "select" | "multi_select" => json!({ "options": option_config(value) }),
        "status" => status_config(value),
        "unique_id" => json!({
            "prefix": value
                .and_then(Value::as_object)
                .and_then(|object| object.get("prefix"))
                .cloned()
                .unwrap_or(Value::Null)
        }),
        "relation" => json!({ "target_data_source_id": null, "mode": "one_way" }),
        "rollup" => json!({
            "relation_property_id": null,
            "rollup_property_id": null,
            "function": "show_original"
        }),
        "formula" => json!({ "expression": "" }),
        "button" => json!({ "label": "Button", "actions": [] }),
        _ => json!({}),
    }
}

fn option_config(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_object)
        .and_then(|object| object.get("options"))
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(|option| option.as_object())
                .map(|option| {
                    json!({
                        "id": option
                            .get("id")
                            .and_then(Value::as_str)
                            .map(clean_property_id)
                            .unwrap_or_else(|| "option".to_string()),
                        "name": option
                            .get("name")
                            .and_then(Value::as_str)
                            .map(clean_display_text)
                            .unwrap_or_else(|| "Option".to_string()),
                        "color": option_color(option.get("color").and_then(Value::as_str))
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn status_config(value: Option<&Value>) -> Value {
    let options = option_config(value);
    let option_ids = options
        .iter()
        .filter_map(|option| option.get("id").and_then(Value::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    json!({
        "options": options,
        "groups": STATUS_GROUPS
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let color = ["gray", "blue", "green"][index];
                json!({
                    "id": group,
                    "name": group,
                    "color": color,
                    "option_ids": if index == 0 { option_ids.clone() } else { Vec::<String>::new() }
                })
            })
            .collect::<Vec<_>>()
    })
}

fn row_properties(value: Option<&Value>, schema_properties: &Value, source_id: &str) -> Value {
    let mut output = Map::new();
    let raw_properties = value.and_then(Value::as_object);
    let Some(schema) = schema_properties.as_object() else {
        return Value::Object(output);
    };
    for (name, schema_property) in schema {
        let property_type = schema_property
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("rich_text");
        let property_id = schema_property
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(name);
        let raw = raw_properties.and_then(|properties| properties.get(name));
        if let Some(value) = row_property_value(property_id, property_type, raw, source_id) {
            output.insert(name.clone(), value);
        }
    }
    Value::Object(output)
}

fn row_property_value(
    property_id: &str,
    property_type: &str,
    raw: Option<&Value>,
    source_id: &str,
) -> Option<Value> {
    let payload = match property_type {
        "title" | "rich_text" => raw
            .and_then(|value| value.get(property_type))
            .and_then(Value::as_array)
            .map(|items| {
                let mut diagnostics = Vec::new();
                Value::Array(notion_rich_text(items, Some(source_id), &mut diagnostics))
            })
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "number" | "checkbox" | "date" | "url" | "email" | "phone_number" => raw
            .and_then(|value| value.get(property_type))
            .cloned()
            .unwrap_or(Value::Null),
        "select" | "status" => raw
            .and_then(|value| value.get(property_type))
            .cloned()
            .unwrap_or(Value::Null),
        "multi_select" | "files" | "people" => raw
            .and_then(|value| value.get(property_type))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "created_time" | "created_by" | "last_edited_time" | "last_edited_by" | "unique_id" => raw
            .and_then(|value| value.get(property_type))
            .cloned()
            .unwrap_or(Value::Null),
        "place" => raw
            .and_then(|value| value.get(property_type))
            .cloned()
            .unwrap_or(Value::Null),
        _ => return None,
    };
    Some(json!({
        "id": property_id,
        "type": property_type,
        property_type: payload
    }))
}

fn optional_icon(
    value: Option<&Value>,
    source_id: &str,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> Option<Value> {
    let icon = match value {
        Some(Value::Null) | None => return None,
        Some(value) => value.clone(),
    };
    if validate_icon_value(&icon, "icon").is_ok() {
        return Some(icon);
    }
    diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
        "unsupported_icon",
        "warning",
        Some(source_id.to_string()),
        "Notion icon could not be validated locally and was skipped.",
    ));
    None
}

fn optional_cover(
    value: Option<&Value>,
    source_id: &str,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> Option<Value> {
    let cover = match value {
        Some(Value::Null) | None => return None,
        Some(value) => value.clone(),
    };
    if validate_page_cover_value(&cover).is_ok() {
        return Some(cover);
    }
    diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
        "unsupported_cover",
        "warning",
        Some(source_id.to_string()),
        "Notion cover could not be validated locally and was skipped.",
    ));
    None
}

fn notion_color(value: Option<&str>) -> &'static str {
    LOCAL_COLORS
        .iter()
        .copied()
        .find(|color| Some(*color) == value)
        .unwrap_or("default")
}

fn option_color(value: Option<&str>) -> &'static str {
    match value {
        Some("gray") => "gray",
        Some("brown") => "brown",
        Some("orange") => "orange",
        Some("yellow") => "yellow",
        Some("green") => "green",
        Some("blue") => "blue",
        Some("purple") => "purple",
        Some("pink") => "pink",
        Some("red") => "red",
        _ => "default",
    }
}

fn safe_rich_link(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        Some(clean_display_text(value))
    } else {
        None
    }
}

fn rich_text_plain_text(items: &[Value]) -> String {
    items
        .iter()
        .filter_map(|item| item.get("plain_text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(clean_display_text)
        .filter(|value| !value.trim().is_empty())
}

fn notion_id(value: &Value) -> Option<String> {
    optional_string(value, "id")
}

fn clean_display_text(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || *character == '\n' || *character == '\t')
        .collect()
}

fn clean_property_id(value: &str) -> String {
    clean_display_text(value)
        .chars()
        .filter(|character| *character != '\0')
        .take(80)
        .collect::<String>()
        .trim()
        .to_string()
}

trait NonEmptyString {
    fn or_else_non_empty(self) -> Option<String>;
}

impl NonEmptyString for String {
    fn or_else_non_empty(self) -> Option<String> {
        if self.trim().is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_supported_block_and_preserves_unsupported_child() {
        let raw = json!({
            "object": "block",
            "id": "11111111-1111-1111-1111-111111111111",
            "type": "paragraph",
            "last_edited_time": "2026-07-03T00:00:00.000Z",
            "paragraph": {
                "rich_text": [{
                    "type": "text",
                    "text": { "content": "Hello", "link": null },
                    "annotations": { "bold": true, "italic": false, "strikethrough": false, "underline": false, "code": false, "color": "default" },
                    "plain_text": "Hello",
                    "href": null
                }],
                "color": "default"
            }
        });
        let mut diagnostics = Vec::new();
        let mut stats = NotionConvertStats::default();
        let block = convert_block(
            FetchedNotionBlock {
                raw,
                children: Vec::new(),
            },
            &NotionConvertOptions::default(),
            &mut diagnostics,
            &mut stats,
        );
        assert_eq!(block.block_type, "paragraph");
        assert_eq!(
            block.source_object_id.as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn media_blocks_become_unsupported_when_external_references_are_disabled() {
        let raw = json!({
            "object": "block",
            "id": "22222222-2222-2222-2222-222222222222",
            "type": "image",
            "image": {
                "type": "external",
                "external": { "url": "https://example.com/image.png" },
                "caption": []
            }
        });
        let mut diagnostics = Vec::new();
        let mut stats = NotionConvertStats::default();
        let block = convert_block(
            FetchedNotionBlock {
                raw,
                children: Vec::new(),
            },
            &NotionConvertOptions::default(),
            &mut diagnostics,
            &mut stats,
        );
        assert_eq!(block.block_type, "unsupported");
        assert_eq!(stats.unsupported_block_count, 1);
        let diagnostic = serde_json::to_value(&diagnostics[0]).unwrap();
        assert_eq!(diagnostic["code"], "external_file_reference_skipped");
    }

    #[test]
    fn unsupported_property_types_are_mapped_to_rich_text() {
        let raw = json!({
            "object": "data_source",
            "id": "33333333-3333-3333-3333-333333333333",
            "title": [],
            "properties": {
                "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
                "Mystery": { "id": "abc", "name": "Mystery", "type": "verification", "verification": {} }
            }
        });
        let mut diagnostics = Vec::new();
        let converted = convert_data_source(&raw, &mut diagnostics);
        let property = converted.properties["Mystery"].as_object().unwrap();
        assert_eq!(property["type"], "rich_text");
        let diagnostic = serde_json::to_value(&diagnostics[0]).unwrap();
        assert_eq!(diagnostic["code"], "unsupported_property_type");
    }
}
