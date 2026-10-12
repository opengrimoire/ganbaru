pub(crate) fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

pub(crate) fn escape_attr(value: &str) -> String {
    escape_html(value)
}

pub(crate) fn slug(value: &str, fallback: &str) -> String {
    let mut output = String::new();
    let mut last_was_dash = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            output.push(character);
            last_was_dash = false;
        } else if !last_was_dash && !output.is_empty() {
            output.push('-');
            last_was_dash = true;
        }
    }
    while output.ends_with('-') {
        output.pop();
    }
    if output.is_empty() {
        fallback.to_string()
    } else {
        output
    }
}

pub(crate) fn short_id(id: &str) -> String {
    id.chars()
        .filter(|character| *character != '-')
        .take(8)
        .collect()
}

pub(crate) fn page_archive_path(title: &str, id: &str, root: bool) -> String {
    if root {
        "index.html".to_string()
    } else {
        format!(
            "pages/{}-{}/index.html",
            slug(title, "untitled"),
            short_id(id)
        )
    }
}

pub(crate) fn database_archive_path(title: &str, id: &str) -> String {
    format!(
        "databases/{}-{}.json",
        slug(title, "database"),
        short_id(id)
    )
}

pub(crate) fn asset_archive_path(source_path: &str) -> String {
    format!("assets/{}", source_path.trim().trim_start_matches('/'))
}

pub(crate) fn relative_link(from_path: &str, target_path: &str) -> String {
    let mut from_parts = split_path(from_path);
    if !from_parts.is_empty() {
        from_parts.pop();
    }
    let target_parts = split_path(target_path);
    let common = from_parts
        .iter()
        .zip(target_parts.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = Vec::new();
    for _ in common..from_parts.len() {
        parts.push("..".to_string());
    }
    parts.extend(target_parts[common..].iter().cloned());
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

pub(crate) fn json_file_contents(value: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| format!("serialize HTML export JSON: {e}"))
}

fn split_path(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}
