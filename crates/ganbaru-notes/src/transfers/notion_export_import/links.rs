use super::{ExportEntry, ExportTarget};
use std::collections::HashMap;
use std::path::{Component, Path};

pub(super) fn local_page_link(
    source: &ExportEntry,
    target_map: &HashMap<String, ExportTarget>,
    raw_reference: &str,
) -> Option<String> {
    let relative = resolve_relative_link(source, raw_reference)?;
    let target = target_map.get(&relative)?;
    Some(format!(
        "http://localhost:1420/?view=notes#notes?title={}",
        encode_query_component(&target.title)
    ))
}

fn resolve_relative_link(source: &ExportEntry, raw_reference: &str) -> Option<String> {
    if is_external_url(raw_reference) || raw_reference.starts_with('#') {
        return None;
    }
    let decoded = decoded_link_reference(raw_reference);
    if decoded.is_empty() {
        return None;
    }
    let source_parent = Path::new(&source.relative)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    normalize_relative_components(&source_parent.join(decoded))
}

pub(super) fn entry_parent_relative(entry: &ExportEntry, reference: &str) -> String {
    let source_parent = Path::new(&entry.relative)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    normalize_relative_components(&source_parent.join(reference))
        .unwrap_or_else(|| reference.to_string())
}

fn normalize_relative_components(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

pub(super) fn decoded_link_reference(raw: &str) -> String {
    let without_anchor = raw.split(['#', '?']).next().unwrap_or(raw).trim();
    percent_decode(without_anchor)
}

pub(super) fn is_external_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("ganbaru-asset:")
}

pub(super) fn html_media_block_type(prefix: &str) -> Option<&'static str> {
    let tag_start = prefix.rfind('<')?;
    let name = prefix[tag_start + 1..]
        .trim_start_matches('/')
        .split_whitespace()
        .next()?
        .to_ascii_lowercase();
    match name.as_str() {
        "img" => Some("image"),
        "video" => Some("video"),
        "audio" => Some("audio"),
        _ => None,
    }
}

pub(super) fn title_and_source_id(stem: &str, relative: &str) -> (String, String) {
    let trimmed = stem.trim();
    if let Some((title, id)) = split_notion_id_suffix(trimmed) {
        return (title.to_string(), id.to_string());
    }
    (
        trimmed_non_empty(trimmed).unwrap_or_else(|| "Untitled".to_string()),
        relative.to_string(),
    )
}

fn split_notion_id_suffix(value: &str) -> Option<(&str, &str)> {
    let (title, suffix) = value.rsplit_once(' ')?;
    let compact = suffix.replace('-', "");
    if compact.len() == 32 && compact.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some((title.trim(), suffix))
    } else {
        None
    }
}

pub(super) fn normalize_title(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(super) fn trimmed_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub(super) fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) =
                    (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
                {
                    output.push(high * 16 + low);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn encode_query_component(value: &str) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(*byte as char)
            }
            b' ' => output.push('+'),
            byte => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notion_export_titles_strip_id_suffixes() {
        let (title, source_id) = title_and_source_id(
            "Roadmap 0123456789abcdef0123456789abcdef",
            "Roadmap 0123456789abcdef0123456789abcdef.md",
        );
        assert_eq!(title, "Roadmap");
        assert_eq!(source_id, "0123456789abcdef0123456789abcdef");
    }
}
