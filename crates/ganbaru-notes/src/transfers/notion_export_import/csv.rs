use crate::writes;
use serde_json::{Map, Value, json};

pub(super) const MAX_CSV_ROWS: usize = 1_000;
pub(super) const MAX_CSV_COLUMNS: usize = 100;

pub(super) fn csv_property_schema(headers: &[String]) -> Value {
    let mut properties = Map::new();
    let title_name = headers
        .first()
        .filter(|value| !value.trim().is_empty())
        .map_or("Name", String::as_str);
    properties.insert(
        title_name.to_string(),
        json!({
            "id": "title",
            "name": title_name,
            "description": "",
            "type": "title",
            "title": {}
        }),
    );
    for (index, header) in headers.iter().enumerate().skip(1) {
        let name = unique_property_name(header, index);
        let id = format!("csv_{index}");
        properties.insert(
            name.clone(),
            json!({
                "id": id,
                "name": name,
                "description": "",
                "type": "rich_text",
                "rich_text": {}
            }),
        );
    }
    Value::Object(properties)
}

pub(super) fn csv_property_order(headers: &[String]) -> Vec<String> {
    let mut order = vec!["title".to_string()];
    order.extend((1..headers.len()).map(|index| format!("csv_{index}")));
    order
}

pub(super) fn csv_row_properties(headers: &[String], row: &[String]) -> Value {
    let mut properties = Map::new();
    let title_name = headers
        .first()
        .filter(|value| !value.trim().is_empty())
        .map_or("Name", String::as_str);
    let title = row.first().map(String::as_str).unwrap_or("").trim();
    properties.insert(
        title_name.to_string(),
        json!({
            "id": "title",
            "type": "title",
            "title": [writes::rich_text(if title.is_empty() { "Untitled" } else { title })]
        }),
    );
    for (index, header) in headers.iter().enumerate().skip(1) {
        let value = row.get(index).map(String::as_str).unwrap_or("").trim();
        if value.is_empty() {
            continue;
        }
        let name = unique_property_name(header, index);
        let id = format!("csv_{index}");
        properties.insert(
            name,
            json!({
                "id": id,
                "type": "rich_text",
                "rich_text": [writes::rich_text(value)]
            }),
        );
    }
    Value::Object(properties)
}

pub(super) fn parse_csv(input: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut chars = input.chars().peekable();
    let mut in_quotes = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                row.push(cell.trim().to_string());
                cell.clear();
            }
            '\n' if !in_quotes => {
                row.push(cell.trim().to_string());
                cell.clear();
                rows.push(row);
                row = Vec::new();
            }
            '\r' if !in_quotes => {}
            _ => cell.push(ch),
        }
    }
    if in_quotes {
        return Err("CSV export contains an unterminated quoted cell".to_string());
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell.trim().to_string());
        rows.push(row);
    }
    Ok(rows)
}

fn unique_property_name(header: &str, index: usize) -> String {
    super::links::trimmed_non_empty(header).unwrap_or_else(|| format!("Column {index}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_parser_handles_quotes_and_commas() {
        let rows = parse_csv("Name,Notes\nAlpha,\"one, two\"\nBeta,\"said \"\"ok\"\"\"").unwrap();
        assert_eq!(rows[1][1], "one, two");
        assert_eq!(rows[2][1], "said \"ok\"");
    }
}
