use crate::links::{
    LocalLinkResolver, block_id_from_local_notes_url, page_ids_from_local_notes_url,
};
use serde_json::Value;

pub(super) struct ScannedReference {
    pub(super) target: ScannedTarget,
    pub(super) link_type: &'static str,
    pub(super) label: String,
}

pub(super) enum ScannedTarget {
    Page(String),
    Block(String),
    Database(String),
    LocalObject {
        object_type: &'static str,
        id: String,
    },
    ExternalUrl(String),
}

pub(super) fn collect_json_references(
    value: &Value,
    link_resolver: &LocalLinkResolver,
) -> Vec<ScannedReference> {
    let mut references = Vec::new();
    collect_value_references(value, link_resolver, &mut references);
    references
}

pub(super) fn external_url_reference(
    url: &str,
    link_type: &'static str,
) -> Option<ScannedReference> {
    Some(ScannedReference {
        target: ScannedTarget::ExternalUrl(normalize_external_url(url)?),
        link_type,
        label: url.trim().chars().take(200).collect(),
    })
}

fn collect_value_references(
    value: &Value,
    link_resolver: &LocalLinkResolver,
    references: &mut Vec<ScannedReference>,
) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = mention_reference(object) {
                references.push(reference);
                return;
            }
            let text_label = object
                .get("plain_text")
                .or_else(|| object.get("text").and_then(|text| text.get("content")))
                .and_then(Value::as_str)
                .unwrap_or_default();
            for field in ["href", "url"] {
                if let Some(url) = object.get(field).and_then(Value::as_str) {
                    push_url_references(references, url, text_label, link_resolver);
                }
            }
            if let Some(url) = object
                .get("link")
                .and_then(|link| link.get("url"))
                .and_then(Value::as_str)
            {
                push_url_references(references, url, text_label, link_resolver);
            }
            for nested in object.values() {
                collect_value_references(nested, link_resolver, references);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_value_references(item, link_resolver, references);
            }
        }
        _ => {}
    }
}

fn mention_reference(object: &serde_json::Map<String, Value>) -> Option<ScannedReference> {
    let mention = object.get("mention")?.as_object()?;
    let mention_type = mention.get("type")?.as_str()?;
    let label = object
        .get("plain_text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .chars()
        .take(200)
        .collect::<String>();
    match mention_type {
        "page" => Some(ScannedReference {
            target: ScannedTarget::Page(
                mention.get("page")?.get("id")?.as_str()?.trim().to_string(),
            ),
            link_type: "page_mention",
            label,
        }),
        "database" => Some(ScannedReference {
            target: ScannedTarget::Database(
                mention
                    .get("database")?
                    .get("id")?
                    .as_str()?
                    .trim()
                    .to_string(),
            ),
            link_type: "database_mention",
            label,
        }),
        "ganbaru_object" => {
            let object = mention.get("ganbaru_object")?.as_object()?;
            let object_type = object.get("type")?.as_str()?.trim();
            let object_id = object.get("id")?.as_str()?.trim();
            if object_id.is_empty() {
                return None;
            }
            Some(ScannedReference {
                target: ScannedTarget::LocalObject {
                    object_type: local_object_target_type(object_type)?,
                    id: object_id.to_string(),
                },
                link_type: "local_object_mention",
                label,
            })
        }
        _ => None,
    }
}

fn push_url_references(
    references: &mut Vec<ScannedReference>,
    url: &str,
    label: &str,
    link_resolver: &LocalLinkResolver,
) {
    let label = label.trim().chars().take(200).collect::<String>();
    if url.contains("#notes?") {
        for page_id in page_ids_from_local_notes_url(url, link_resolver) {
            references.push(ScannedReference {
                target: ScannedTarget::Page(page_id),
                link_type: "page_link",
                label: label.clone(),
            });
        }
        if let Some(block_id) = block_id_from_local_notes_url(url) {
            references.push(ScannedReference {
                target: ScannedTarget::Block(block_id),
                link_type: "block_link",
                label,
            });
        }
        return;
    }
    if let Some(url) = normalize_external_url(url) {
        references.push(ScannedReference {
            target: ScannedTarget::ExternalUrl(url),
            link_type: "external_url",
            label,
        });
    }
}

fn local_object_target_type(object_type: &str) -> Option<&'static str> {
    match object_type {
        "project" => Some("project"),
        "project_task" => Some("project_task"),
        "calendar_event" => Some("calendar_event"),
        _ => None,
    }
}

fn normalize_external_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.starts_with("ganbaru-asset:") {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        return Some(trimmed.chars().take(2048).collect());
    }
    None
}
