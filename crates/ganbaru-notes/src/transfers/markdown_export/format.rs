use serde_json::Value;

pub(crate) fn block_separator(previous: Option<&str>, current: &str) -> &'static str {
    if previous.is_some_and(|previous| is_list_like(previous) && is_list_like(current)) {
        "\n"
    } else {
        "\n\n"
    }
}

fn is_list_like(block_type: &str) -> bool {
    matches!(
        block_type,
        "bulleted_list_item" | "numbered_list_item" | "to_do"
    )
}

pub(crate) fn append_children(line: String, children: String, depth: usize) -> String {
    if children.is_empty() {
        return line;
    }
    if line.is_empty() {
        return children;
    }
    format!("{line}\n\n{}{}", indent(depth + 1), children)
}

pub(crate) fn append_indented_children(line: String, children: String) -> String {
    if children.is_empty() {
        line
    } else {
        format!("{line}\n{}", indent_multiline(&children, 2))
    }
}

pub(crate) fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

fn indent_multiline(text: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn quote_lines(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn inline_code(text: &str) -> String {
    if text.contains('`') {
        format!("`` {text} ``")
    } else {
        format!("`{text}`")
    }
}

pub(crate) fn code_fence(code: &str) -> &'static str {
    if code.contains("```") { "````" } else { "```" }
}

pub(crate) fn table_cells(payload: &Value) -> Vec<Vec<Value>> {
    payload
        .get("cells")
        .and_then(Value::as_array)
        .map(|cells| {
            cells
                .iter()
                .filter_map(Value::as_array)
                .map(|items| items.to_vec())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn normalize_cells(mut cells: Vec<Vec<Value>>, width: usize) -> Vec<Vec<Value>> {
    cells.truncate(width);
    while cells.len() < width {
        cells.push(Vec::new());
    }
    cells
}

pub(crate) fn media_url(payload: &Value) -> Option<String> {
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

pub(crate) fn callout_icon_text(payload: &Value) -> Option<String> {
    let icon = payload.get("icon")?;
    match icon.get("type").and_then(Value::as_str)? {
        "emoji" => icon.get("emoji")?.as_str().map(str::to_string),
        "icon" => icon
            .get("icon")?
            .get("name")?
            .as_str()
            .map(|name| format!("[{name}]")),
        _ => None,
    }
}

pub(crate) fn synced_block_label(payload: &Value) -> String {
    match payload.get("synced_from") {
        Some(Value::Null) => "Synced block".to_string(),
        Some(Value::Object(source)) => source
            .get("block_id")
            .and_then(Value::as_str)
            .map(|block_id| format!("Synced block from `{block_id}`"))
            .unwrap_or_else(|| "Synced block".to_string()),
        _ => "Synced block".to_string(),
    }
}

pub(crate) fn display_name(raw: &str) -> String {
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

pub(crate) fn parse_json_array(raw: &str) -> Option<Vec<Value>> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| value.as_array().cloned())
}

pub(crate) fn escape_inline(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(
            character,
            '\\' | '*' | '_' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '!' | '|'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

pub(crate) fn escape_link_label(text: &str) -> String {
    text.replace('[', "\\[").replace(']', "\\]")
}

pub(crate) fn escape_url(url: &str) -> String {
    url.trim().replace(')', "%29")
}

pub(crate) fn escape_dollar_math(expression: &str) -> String {
    expression.replace('$', "\\$")
}

pub(crate) fn escape_html_comment(value: &str) -> String {
    value.replace("--", " - ")
}
