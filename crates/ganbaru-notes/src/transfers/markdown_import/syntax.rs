pub(crate) fn is_divider(trimmed: &str) -> bool {
    matches!(trimmed, "---" | "***" | "___")
}

pub(crate) fn starts_with_list_marker(trimmed: &str) -> bool {
    todo_marker(trimmed).is_some()
        || unordered_list_marker(trimmed).is_some()
        || ordered_list_marker(trimmed).is_some()
}

pub(crate) fn todo_marker(trimmed: &str) -> Option<(bool, &str)> {
    for marker in ["- [ ] ", "* [ ] ", "+ [ ] "] {
        if let Some(text) = trimmed.strip_prefix(marker) {
            return Some((false, text));
        }
    }
    for marker in ["- [x] ", "- [X] ", "* [x] ", "* [X] ", "+ [x] ", "+ [X] "] {
        if let Some(text) = trimmed.strip_prefix(marker) {
            return Some((true, text));
        }
    }
    None
}

pub(crate) fn unordered_list_marker(trimmed: &str) -> Option<&str> {
    ["- ", "* ", "+ "]
        .into_iter()
        .find_map(|marker| trimmed.strip_prefix(marker))
}

pub(crate) fn ordered_list_marker(trimmed: &str) -> Option<&str> {
    let marker_end = trimmed
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_digit())
        .last()
        .map(|(index, character)| index + character.len_utf8())?;
    if marker_end == 0 || marker_end + 2 > trimmed.len() {
        return None;
    }
    let rest = &trimmed[marker_end..];
    if rest.starts_with(". ") || rest.starts_with(") ") {
        return Some(rest[2..].trim_start());
    }
    None
}

pub(crate) fn parse_image(trimmed: &str) -> Option<ParsedImage<'_>> {
    let rest = trimmed.strip_prefix("![")?;
    let label_end = rest.find("](")?;
    let alt = &rest[..label_end];
    let url_start = label_end + 2;
    let url_end = rest[url_start..].find(')')? + url_start;
    if !rest[url_end + 1..].trim().is_empty() {
        return None;
    }
    Some(ParsedImage {
        alt,
        url: rest[url_start..url_end].trim(),
    })
}

pub(crate) struct ParsedImage<'a> {
    pub(crate) alt: &'a str,
    pub(crate) url: &'a str,
}

pub(crate) fn parse_table_cells(line: &str) -> Option<Vec<String>> {
    if !line.contains('|') {
        return None;
    }
    let trimmed = line.trim().trim_matches('|');
    let cells = trimmed
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect::<Vec<_>>();
    (!cells.is_empty()).then_some(cells)
}

pub(crate) fn is_table_delimiter(line: &str) -> bool {
    let Some(cells) = parse_table_cells(line) else {
        return false;
    };
    cells.iter().all(|cell| {
        let trimmed = cell.trim();
        trimmed
            .chars()
            .filter(|character| *character == '-')
            .count()
            >= 3
            && trimmed
                .chars()
                .all(|character| matches!(character, '-' | ':' | ' '))
    })
}

pub(crate) fn normalize_table_cells(mut cells: Vec<String>, width: usize) -> Vec<String> {
    cells.truncate(width);
    while cells.len() < width {
        cells.push(String::new());
    }
    cells
}

pub(crate) fn is_reference_definition(trimmed: &str) -> bool {
    trimmed.starts_with('[') && trimmed.contains("]:")
}

pub(crate) fn is_https_url(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("https://")
}

pub(crate) fn is_rich_text_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

pub(crate) fn unquote_frontmatter_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0];
        let last = trimmed.as_bytes()[trimmed.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return trimmed[1..trimmed.len() - 1].trim().to_string();
        }
    }
    trimmed.to_string()
}
