use crate::models::NotePageRow;
use serde_json::{Value, json};

pub fn page_title_properties(title: &str) -> Value {
    json!({
        "title": {
            "id": "title",
            "type": "title",
            "title": [rich_text(title)]
        }
    })
}

pub(super) fn page_row_properties_for_title(
    page: &NotePageRow,
    title: &str,
) -> Result<String, String> {
    if page.parent_type == "data_source_id" {
        data_source_page_properties_with_title(&page.properties, title)
    } else {
        normal_page_properties_with_title(&page.properties, title)
    }
}

fn normal_page_properties_with_title(properties: &str, title: &str) -> Result<String, String> {
    let mut value: Value =
        serde_json::from_str(properties).map_err(|e| format!("parse page properties: {e}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "page properties must be an object".to_string())?;
    let title_value = page_title_properties(title)
        .get("title")
        .cloned()
        .ok_or_else(|| "page title properties are missing the title key".to_string())?;
    object.insert("title".to_string(), title_value);
    Ok(value.to_string())
}

pub(super) fn data_source_page_properties_with_title(
    properties: &str,
    title: &str,
) -> Result<String, String> {
    let mut value: Value =
        serde_json::from_str(properties).map_err(|e| format!("parse row page properties: {e}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "row page properties must be an object".to_string())?;
    for property in object.values_mut() {
        let Some(property_object) = property.as_object_mut() else {
            continue;
        };
        if property_object.get("type").and_then(Value::as_str) == Some("title") {
            property_object.insert("title".to_string(), Value::Array(vec![rich_text(title)]));
            return Ok(value.to_string());
        }
    }
    Err("row page properties are missing a title property".to_string())
}

pub(super) fn child_page_payload(title: &str) -> Value {
    json!({
        "title": title
    })
}

pub fn default_text_payload(text: &str) -> Value {
    json!({
        "rich_text": [rich_text(text)],
        "color": "default"
    })
}

pub fn rich_text(text: &str) -> Value {
    json!({
        "type": "text",
        "text": {
            "content": text,
            "link": null
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
        "href": null
    })
}
