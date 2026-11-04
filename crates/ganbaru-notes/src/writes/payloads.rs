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
