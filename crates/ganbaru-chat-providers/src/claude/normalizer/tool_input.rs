//! Bounded projection of a Claude tool call's input into an activity title and safe metadata.
//!
//! Only fields that name what the tool acts on are kept (a command, a path, a search query, a
//! URL), so file contents that edit tools carry never reach the timeline.

use super::bounded_text;
use crate::models::VersionedJson;
use serde_json::{Map, Value, json};

/// Byte limit of an activity title, matching the bound other providers use.
const MAX_TITLE_BYTES: usize = 512;
/// Byte limit of one kept input string.
const MAX_FIELD_BYTES: usize = 2048;
/// Most entries kept from one input list, such as the domains a search is limited to.
const MAX_LIST_ENTRIES: usize = 16;

/// Input keys that name a tool's target, in the order the timeline looks them up.
const KEPT_STRING_KEYS: &[&str] = &[
    "command",
    "description",
    "file_path",
    "notebook_path",
    "path",
    "pattern",
    "glob",
    "query",
    "url",
];

/// Input keys that hold lists of short targets.
const KEPT_LIST_KEYS: &[&str] = &["allowed_domains", "blocked_domains"];

/// The activity title of a tool call: the command for the shell, the query or URL for the web
/// tools, and the tool name otherwise, which the timeline reads to classify reads and searches.
pub(super) fn tool_title(name: &str, input: Option<&Map<String, Value>>) -> String {
    let target = match name {
        "Bash" => input.and_then(|input| string_field(input, "command")),
        "WebSearch" => input.and_then(|input| string_field(input, "query")),
        "WebFetch" => input.and_then(|input| string_field(input, "url")),
        _ => None,
    };
    bounded_text(target.unwrap_or(name), MAX_TITLE_BYTES)
}

/// The safe metadata of a tool call: its block type, tool name, and the bounded target fields
/// of its input.
pub(super) fn tool_metadata(
    block_type: &str,
    name: &str,
    input: Option<&Map<String, Value>>,
) -> VersionedJson {
    VersionedJson {
        schema_version: 1,
        value: json!({
            "blockType": block_type,
            "toolName": name,
            "toolInput": input.map(kept_input),
        }),
    }
}

fn kept_input(input: &Map<String, Value>) -> Value {
    let mut kept = Map::new();
    for key in KEPT_STRING_KEYS {
        if let Some(value) = string_field(input, key) {
            kept.insert(
                (*key).to_string(),
                Value::String(bounded_text(value, MAX_FIELD_BYTES)),
            );
        }
    }
    for key in KEPT_LIST_KEYS {
        if let Some(values) = input.get(*key).and_then(Value::as_array) {
            let entries = values
                .iter()
                .filter_map(Value::as_str)
                .take(MAX_LIST_ENTRIES)
                .map(|value| Value::String(bounded_text(value, MAX_FIELD_BYTES)))
                .collect::<Vec<_>>();
            if !entries.is_empty() {
                kept.insert((*key).to_string(), Value::Array(entries));
            }
        }
    }
    Value::Object(kept)
}

fn string_field<'a>(input: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object input")
    }

    #[test]
    fn titles_name_the_command_query_or_url_and_fall_back_to_the_tool() {
        let bash = input(json!({ "command": "pnpm test", "description": "Run tests" }));
        assert_eq!(tool_title("Bash", Some(&bash)), "pnpm test");
        let search = input(json!({ "query": "riemann hypothesis" }));
        assert_eq!(tool_title("WebSearch", Some(&search)), "riemann hypothesis");
        let fetch = input(json!({ "url": "https://example.org/a", "prompt": "summarize" }));
        assert_eq!(
            tool_title("WebFetch", Some(&fetch)),
            "https://example.org/a"
        );
        let read = input(json!({ "file_path": "src/main.rs" }));
        assert_eq!(tool_title("Read", Some(&read)), "Read");
        assert_eq!(tool_title("Bash", None), "Bash");
        assert_eq!(
            tool_title("Bash", Some(&input(json!({ "command": " " })))),
            "Bash"
        );
    }

    #[test]
    fn metadata_keeps_targets_and_drops_contents_and_prompts() {
        let write = input(json!({ "file_path": "notes.md", "content": "secret body" }));
        let metadata = tool_metadata("tool_use", "Write", Some(&write));
        assert_eq!(
            metadata.value,
            json!({
                "blockType": "tool_use",
                "toolName": "Write",
                "toolInput": { "file_path": "notes.md" },
            })
        );
        let fetch = input(json!({ "url": "https://example.org", "prompt": "find the key" }));
        let metadata = tool_metadata("tool_use", "WebFetch", Some(&fetch));
        assert_eq!(
            metadata.value["toolInput"],
            json!({ "url": "https://example.org" })
        );
    }

    #[test]
    fn metadata_bounds_fields_and_domain_lists() {
        let domains = (0..40)
            .map(|index| json!(format!("d{index}.org")))
            .collect::<Vec<_>>();
        let search = input(json!({
            "query": "q".repeat(MAX_FIELD_BYTES * 2),
            "allowed_domains": domains,
            "blocked_domains": [],
        }));
        let metadata = tool_metadata("server_tool_use", "WebSearch", Some(&search));
        let kept = metadata.value["toolInput"].as_object().expect("kept input");
        assert!(kept["query"].as_str().expect("query").len() <= MAX_FIELD_BYTES);
        assert_eq!(
            kept["allowed_domains"].as_array().expect("domains").len(),
            MAX_LIST_ENTRIES
        );
        assert!(!kept.contains_key("blocked_domains"));
    }
}
