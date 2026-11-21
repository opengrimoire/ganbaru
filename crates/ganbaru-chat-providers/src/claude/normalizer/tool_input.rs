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

/// Most links kept from one web search.
const MAX_WEB_LINKS: usize = 24;
/// Byte limit of one kept link title.
const MAX_LINK_TITLE_BYTES: usize = 256;

/// The bounded result of a web search or fetch from the CLI's structured `tool_use_result`:
/// the http or https links a search returned, with their titles, or the URL, status, and size
/// of a fetch. Summaries the CLI wrote about the pages are left out, since the agent's own
/// reply carries what it learned.
pub(super) fn web_result(name: &str, result: Option<&Value>) -> Option<Value> {
    let result = result?.as_object()?;
    match name {
        "WebSearch" => {
            let links = result
                .get("results")?
                .as_array()?
                .iter()
                .filter_map(|entry| entry.get("content").and_then(Value::as_array))
                .flatten()
                .filter_map(web_link)
                .take(MAX_WEB_LINKS)
                .collect::<Vec<_>>();
            Some(json!({ "links": links }))
        }
        "WebFetch" => {
            let url = string_field(result, "url").filter(|url| web_url(url))?;
            let mut fetched = Map::from_iter([(
                "url".to_string(),
                Value::String(bounded_text(url, MAX_FIELD_BYTES)),
            )]);
            for key in ["code", "bytes"] {
                if let Some(number) = result.get(key).and_then(Value::as_u64) {
                    fetched.insert(key.to_string(), Value::from(number));
                }
            }
            Some(Value::Object(fetched))
        }
        _ => None,
    }
}

fn web_link(link: &Value) -> Option<Value> {
    let link = link.as_object()?;
    let url = string_field(link, "url").filter(|url| web_url(url))?;
    let mut kept = Map::from_iter([(
        "url".to_string(),
        Value::String(bounded_text(url, MAX_FIELD_BYTES)),
    )]);
    if let Some(title) = string_field(link, "title") {
        kept.insert(
            "title".to_string(),
            Value::String(bounded_text(title, MAX_LINK_TITLE_BYTES)),
        );
    }
    Some(Value::Object(kept))
}

/// Whether `url` is an http or https URL short enough to keep whole.
fn web_url(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    url.len() <= MAX_FIELD_BYTES && (lower.starts_with("https://") || lower.starts_with("http://"))
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
    fn web_results_keep_web_links_and_drop_summaries() {
        let search = json!({
            "query": "rust release",
            "results": [
                {
                    "tool_use_id": "srvtoolu_1",
                    "content": [
                        { "title": "Rust 1.94.0", "url": "https://blog.rust-lang.org/2026/03/05/Rust-1.94.0" },
                        { "title": "Local", "url": "file:///etc/passwd" },
                        { "url": "http://example.org" },
                    ],
                },
                "The latest release is 1.94.0.",
            ],
            "durationSeconds": 6.1,
            "searchCount": 1,
        });
        assert_eq!(
            web_result("WebSearch", Some(&search)),
            Some(json!({
                "links": [
                    { "url": "https://blog.rust-lang.org/2026/03/05/Rust-1.94.0", "title": "Rust 1.94.0" },
                    { "url": "http://example.org" },
                ],
            }))
        );
        let many = json!({
            "results": [{ "content": (0..40)
                .map(|index| json!({ "title": "t".repeat(1024), "url": format!("https://d{index}.org") }))
                .collect::<Vec<_>>() }],
        });
        let links = web_result("WebSearch", Some(&many)).expect("links");
        let links = links["links"].as_array().expect("links");
        assert_eq!(links.len(), MAX_WEB_LINKS);
        assert!(links[0]["title"].as_str().expect("title").len() <= MAX_LINK_TITLE_BYTES);

        let fetch = json!({
            "bytes": 19632, "code": 200, "codeText": "OK",
            "result": "Python 3.15.2", "durationMs": 1453, "url": "https://docs.python.org/3/",
        });
        assert_eq!(
            web_result("WebFetch", Some(&fetch)),
            Some(json!({ "url": "https://docs.python.org/3/", "code": 200, "bytes": 19632 }))
        );
        assert_eq!(
            web_result("WebFetch", Some(&json!({ "url": "javascript:x" }))),
            None
        );
        assert_eq!(web_result("Bash", Some(&fetch)), None);
        assert_eq!(web_result("WebSearch", None), None);
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
