use super::*;

pub fn plain_text_from_payload(block_type: &str, payload: &Value) -> String {
    if matches!(
        block_type,
        "breadcrumb" | "table_of_contents" | "column_list" | "column" | "table" | "tab" | "divider"
    ) {
        return String::new();
    }
    if block_type == "table_row" {
        return table_row_plain_text(payload);
    }
    if matches!(block_type, "child_page" | "child_database") {
        return payload
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
    }
    if block_type == "bookmark" {
        return bookmark_plain_text(payload);
    }
    if matches!(block_type, "embed" | "link_preview") {
        return payload
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
    }
    if block_type == "synced_block" {
        return synced_block_plain_text(payload);
    }
    if block_type == "button" {
        return button_plain_text(payload);
    }
    if block_type == "equation" {
        return payload
            .get("expression")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
    }
    if matches!(block_type, "image" | "video" | "audio" | "file" | "pdf") {
        return media_plain_text(payload);
    }
    if block_type == "unsupported" {
        return unsupported_plain_text(payload);
    }
    payload
        .get("rich_text")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("plain_text")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("text")?.get("content")?.as_str())
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

pub fn media_plain_text(payload: &Value) -> String {
    let caption = payload
        .get("caption")
        .and_then(Value::as_array)
        .map(|items| rich_text_items_plain_text(items))
        .unwrap_or_default();
    let name = payload
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source = match payload.get("type").and_then(Value::as_str) {
        Some("external") => payload
            .get("external")
            .and_then(|external| external.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        Some("file") => payload
            .get("file")
            .and_then(|file| file.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        Some("file_upload") => payload
            .get("file_upload")
            .and_then(|file_upload| file_upload.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        _ => "",
    };
    [caption.as_str(), name, source]
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn unsupported_plain_text(payload: &Value) -> String {
    let block_type = payload
        .get("block_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source_type = payload
        .get("source_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let warnings = payload
        .get("warnings")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|warning| !warning.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let raw_status = match payload.get("raw") {
        Some(Value::Object(_)) => "raw payload preserved",
        _ => "",
    };
    [
        block_type.trim(),
        source_type.trim(),
        warnings.trim(),
        raw_status,
    ]
    .iter()
    .filter(|part| !part.is_empty())
    .copied()
    .collect::<Vec<_>>()
    .join(" ")
}

pub fn synced_block_plain_text(payload: &Value) -> String {
    match payload.get("synced_from") {
        Some(Value::Null) => "Synced block".to_string(),
        Some(Value::Object(synced_from)) => synced_from
            .get("block_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

pub fn button_plain_text(payload: &Value) -> String {
    payload
        .get("rich_text")
        .and_then(Value::as_array)
        .map(|items| rich_text_items_plain_text(items))
        .unwrap_or_else(|| "Button".to_string())
}

pub fn table_row_plain_text(payload: &Value) -> String {
    payload
        .get("cells")
        .and_then(Value::as_array)
        .map(|cells| {
            cells
                .iter()
                .filter_map(Value::as_array)
                .map(|items| rich_text_items_plain_text(items))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .unwrap_or_default()
}

pub fn bookmark_plain_text(payload: &Value) -> String {
    let caption = payload
        .get("caption")
        .and_then(Value::as_array)
        .map(|items| rich_text_items_plain_text(items))
        .unwrap_or_default();
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match (caption.trim().is_empty(), url.trim().is_empty()) {
        (true, true) => String::new(),
        (true, false) => url.to_string(),
        (false, true) => caption,
        (false, false) => format!("{caption} {url}"),
    }
}

pub fn rich_text_items_plain_text(items: &[Value]) -> String {
    items
        .iter()
        .filter_map(|item| {
            item.get("plain_text")
                .and_then(Value::as_str)
                .or_else(|| item.get("text")?.get("content")?.as_str())
        })
        .collect::<Vec<_>>()
        .join("")
}
