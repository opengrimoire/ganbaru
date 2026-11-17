//! Typed reads and edits of the preferences in a provider configuration file.
//!
//! Each provider family with a catalog shows those entries as typed controls: the Claude and
//! Cursor agent allowlists in [`ganbaru_chat_providers::agent_preferences`], and the
//! [`catalogs`] of providers whose agents read the user's configuration themselves. A read
//! reports the value the provider keeps, and an edit changes only that entry of the user's
//! document and keeps every other key and its order. TOML and JSON with comments also keep their
//! comments and formatting.

mod catalogs;
mod jsonc;
mod toml_document;

use ganbaru_chat_contracts::models::{ChatError, ChatResult};
use ganbaru_chat_providers::agent_preferences::{
    AgentPreference, PreferenceLocation, PreferenceType, checked_setting, checked_variable,
    insert_at, remove_at, scalar_text, switch_enabled, value_at,
};
use serde::Serialize;
use serde_json::{Map, Value};

pub(super) use catalogs::file_preferences;
use jsonc::JsoncDocument;
use toml_document::TomlDocument;

/// Object of a Claude settings file that holds environment variables.
const VARIABLES_OBJECT: &str = "env";

/// How a configuration file's text is parsed and edited.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DocumentSyntax {
    /// Strict JSON, written back pretty-printed.
    Json,
    /// JSON with comments and trailing commas, edited in place.
    Jsonc,
    /// TOML, edited in place.
    Toml,
}

/// The preferences of one configuration file and how its text is edited.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PreferenceCatalog {
    pub syntax: DocumentSyntax,
    pub preferences: &'static [AgentPreference],
}

/// A configuration document whose entries a preference reads and edits.
trait PreferenceDocument: Sized {
    /// The document in `contents`, where blank text is an empty document, or `None` when the
    /// text is not valid.
    fn parse(contents: &str) -> Option<Self>;
    /// The value at `path`, in its JSON form.
    fn value_at(&self, path: &[&str]) -> Option<Value>;
    /// Sets the value at `path`, creating missing parents. Returns false without changes when a
    /// value that is not a table holds a parent's place, or a table holds the value's place.
    fn insert_at(&mut self, path: &[&str], value: Value) -> bool;
    /// Removes the value at `path` and every parent left empty.
    fn remove_at(&mut self, path: &[&str]);
    /// The document's text.
    fn render(self) -> Option<String>;
}

/// A strict JSON document, which loses its formatting when edited.
struct JsonDocument(Map<String, Value>);

impl PreferenceDocument for JsonDocument {
    fn parse(contents: &str) -> Option<Self> {
        if contents.trim().is_empty() {
            return Some(Self(Map::new()));
        }
        match serde_json::from_str::<Value>(contents) {
            Ok(Value::Object(object)) => Some(Self(object)),
            _ => None,
        }
    }

    fn value_at(&self, path: &[&str]) -> Option<Value> {
        let (first, rest) = path.split_first()?;
        value_at(self.0.get(*first)?, rest).cloned()
    }

    fn insert_at(&mut self, path: &[&str], value: Value) -> bool {
        insert_at(&mut self.0, path, value)
    }

    fn remove_at(&mut self, path: &[&str]) {
        remove_at(&mut self.0, path);
    }

    fn render(self) -> Option<String> {
        let mut text = serde_json::to_string_pretty(&Value::Object(self.0)).ok()?;
        text.push('\n');
        Some(text)
    }
}

/// How the settings page edits a preference.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPreferenceType {
    Switch,
    Boolean,
    Count,
    Percentage,
    Choice,
    Text,
}

/// One catalog preference and the value the configuration file gives it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreferenceRead {
    /// Dotted key path or variable name.
    pub id: String,
    pub value_type: ProviderPreferenceType,
    /// Whether the preference is an environment variable rather than a settings key.
    pub variable: bool,
    /// A boolean for switches and booleans, a string otherwise, or null when unset.
    pub value: Value,
    /// Whether the file sets a value that fails its check, so the provider or agent launch
    /// may not keep it.
    pub ignored: bool,
    /// Longest accepted text, for text preferences.
    pub max_bytes: Option<usize>,
    /// Whether a text preference may span lines.
    pub multiline: bool,
    /// Whether an empty text is a meaningful value rather than a missing one.
    pub empty_allowed: bool,
    /// Accepted values, for choice preferences.
    pub options: Vec<&'static str>,
}

/// The preferences of one configuration file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreferencesRead {
    /// Whether the file is missing, empty, or a valid document, so entries can be edited here.
    pub editable: bool,
    pub entries: Vec<ProviderPreferenceRead>,
}

/// Reads the catalog's preferences from a configuration document's text.
pub(super) fn read_preferences(
    contents: &str,
    catalog: PreferenceCatalog,
) -> ProviderPreferencesRead {
    match catalog.syntax {
        DocumentSyntax::Json => read_document::<JsonDocument>(contents, catalog.preferences),
        DocumentSyntax::Jsonc => read_document::<JsoncDocument>(contents, catalog.preferences),
        DocumentSyntax::Toml => read_document::<TomlDocument>(contents, catalog.preferences),
    }
}

/// Returns `contents` with the preference `preference_id` set to `value`, or removed when
/// `value` is null or a switch is turned off.
///
/// # Errors
///
/// Returns a validation error when the preference is not in the catalog, the value fails its
/// check, or the document is not valid.
pub(super) fn apply_preference(
    contents: &str,
    catalog: PreferenceCatalog,
    preference_id: &str,
    value: &Value,
) -> ChatResult<String> {
    let preferences = catalog.preferences;
    match catalog.syntax {
        DocumentSyntax::Json => {
            apply_to_document::<JsonDocument>(contents, preferences, preference_id, value)
        }
        DocumentSyntax::Jsonc => {
            apply_to_document::<JsoncDocument>(contents, preferences, preference_id, value)
        }
        DocumentSyntax::Toml => {
            apply_to_document::<TomlDocument>(contents, preferences, preference_id, value)
        }
    }
}

fn read_document<D: PreferenceDocument>(
    contents: &str,
    preferences: &[AgentPreference],
) -> ProviderPreferencesRead {
    let document = D::parse(contents);
    ProviderPreferencesRead {
        editable: document.is_some(),
        entries: preferences
            .iter()
            .map(|preference| {
                let raw = document
                    .as_ref()
                    .and_then(|document| document.value_at(&document_path(preference)));
                read_preference(preference, raw.as_ref())
            })
            .collect(),
    }
}

fn apply_to_document<D: PreferenceDocument>(
    contents: &str,
    preferences: &[AgentPreference],
    preference_id: &str,
    value: &Value,
) -> ChatResult<String> {
    let preference = preferences
        .iter()
        .find(|preference| preference.id() == preference_id)
        .ok_or_else(|| ChatError::validation("preferenceId", "Preference is not supported"))?;
    let mut document = D::parse(contents).ok_or_else(|| {
        ChatError::validation(
            "providerFile",
            "Fix the configuration file before changing preferences here",
        )
    })?;
    let path = document_path(preference);
    match stored_value(preference, value)? {
        Some(stored) => {
            if !document.insert_at(&path, stored) {
                return Err(ChatError::validation(
                    "providerFile",
                    "The configuration file holds another value where this preference belongs",
                ));
            }
        }
        None => document.remove_at(&path),
    }
    document
        .render()
        .ok_or_else(|| ChatError::validation("providerFile", "Preference could not be saved"))
}

/// Key path of a preference inside the document.
fn document_path(preference: &AgentPreference) -> Vec<&'static str> {
    match preference.location {
        PreferenceLocation::Setting(path) => path.to_vec(),
        PreferenceLocation::Variable(name) => vec![VARIABLES_OBJECT, name],
    }
}

fn read_preference(preference: &AgentPreference, raw: Option<&Value>) -> ProviderPreferenceRead {
    let value = raw.and_then(|raw| shown_value(preference, raw));
    let (max_bytes, multiline, empty_allowed) = match preference.value_type {
        PreferenceType::Text {
            max_bytes,
            single_line,
            empty_allowed,
        } => (Some(max_bytes), !single_line, empty_allowed),
        _ => (None, false, false),
    };
    let options = match preference.value_type {
        PreferenceType::Choice(options) => options.to_vec(),
        _ => Vec::new(),
    };
    ProviderPreferenceRead {
        id: preference.id(),
        value_type: preference_type(preference.value_type),
        variable: matches!(preference.location, PreferenceLocation::Variable(_)),
        ignored: raw.is_some() && value.is_none(),
        value: value.unwrap_or(Value::Null),
        max_bytes,
        multiline,
        empty_allowed,
        options,
    }
}

/// The value the settings page shows for a raw document value, when it passes its check.
/// Counts and shares are shown as text, as the page edits them in text fields.
fn shown_value(preference: &AgentPreference, raw: &Value) -> Option<Value> {
    match preference.location {
        PreferenceLocation::Setting(_) => {
            let checked = checked_setting(preference.value_type, raw)?;
            Some(match checked {
                Value::Number(number) => Value::String(number.to_string()),
                other => other,
            })
        }
        PreferenceLocation::Variable(_) => {
            let text = scalar_text(raw)?;
            if !checked_variable(preference.value_type, &text) {
                return None;
            }
            Some(if preference.value_type == PreferenceType::Switch {
                Value::Bool(switch_enabled(&text))
            } else {
                Value::String(text)
            })
        }
    }
}

/// The document value for an edit, or `None` to remove the entry.
fn stored_value(preference: &AgentPreference, value: &Value) -> ChatResult<Option<Value>> {
    if value.is_null() {
        return Ok(None);
    }
    let invalid = || ChatError::validation("value", "Preference value is not valid");
    let value_type = preference.value_type;
    match (value_type, preference.location) {
        (PreferenceType::Switch, _) => {
            let enabled = value.as_bool().ok_or_else(invalid)?;
            Ok(enabled.then(|| Value::String("1".to_string())))
        }
        (PreferenceType::Count | PreferenceType::Percentage, location) => {
            let text = match value {
                Value::String(_) | Value::Number(_) => scalar_text(value),
                _ => None,
            }
            .ok_or_else(invalid)?;
            let text = text.trim();
            match location {
                PreferenceLocation::Variable(_) => checked_variable(value_type, text)
                    .then(|| Some(Value::String(text.to_string())))
                    .ok_or_else(invalid),
                PreferenceLocation::Setting(_) => {
                    let number = text.parse::<u64>().map_err(|_| invalid())?;
                    checked_setting(value_type, &Value::from(number))
                        .map(Some)
                        .ok_or_else(invalid)
                }
            }
        }
        (PreferenceType::Boolean | PreferenceType::Choice(_) | PreferenceType::Text { .. }, _) => {
            checked_setting(value_type, value)
                .map(Some)
                .ok_or_else(invalid)
        }
    }
}

fn preference_type(value_type: PreferenceType) -> ProviderPreferenceType {
    match value_type {
        PreferenceType::Switch => ProviderPreferenceType::Switch,
        PreferenceType::Boolean => ProviderPreferenceType::Boolean,
        PreferenceType::Count => ProviderPreferenceType::Count,
        PreferenceType::Percentage => ProviderPreferenceType::Percentage,
        PreferenceType::Choice(_) => ProviderPreferenceType::Choice,
        PreferenceType::Text { .. } => ProviderPreferenceType::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::provider_files::ProviderFileFormat;
    use ganbaru_chat_providers::agent_preferences::{CLAUDE_PREFERENCES, CURSOR_PREFERENCES};
    use serde_json::json;

    const CLAUDE: PreferenceCatalog = PreferenceCatalog {
        syntax: DocumentSyntax::Json,
        preferences: CLAUDE_PREFERENCES,
    };
    const CURSOR: PreferenceCatalog = PreferenceCatalog {
        syntax: DocumentSyntax::Json,
        preferences: CURSOR_PREFERENCES,
    };

    fn catalog(family: &str, format: ProviderFileFormat) -> PreferenceCatalog {
        file_preferences(family, format).unwrap()
    }

    fn entry<'a>(read: &'a ProviderPreferencesRead, id: &str) -> &'a ProviderPreferenceRead {
        read.entries.iter().find(|entry| entry.id == id).unwrap()
    }

    #[test]
    fn reads_report_kept_values_and_flag_ignored_ones() {
        let contents = json!({
            "language": "Spanish",
            "attribution": { "commit": "" },
            "showThinkingSummaries": "yes",
            "env": {
                "CLAUDE_CODE_DISABLE_1M_CONTEXT": 1,
                "DISABLE_TELEMETRY": "0",
                "MAX_THINKING_TOKENS": "lots",
                "API_TIMEOUT_MS": 60000,
            },
        })
        .to_string();
        let read = read_preferences(&contents, CLAUDE);
        assert!(read.editable);
        assert_eq!(entry(&read, "language").value, json!("Spanish"));
        assert_eq!(entry(&read, "attribution.commit").value, json!(""));
        assert_eq!(entry(&read, "attribution.pr").value, Value::Null);
        assert!(!entry(&read, "attribution.pr").ignored);
        assert!(entry(&read, "attribution.commit").multiline);
        assert!(entry(&read, "attribution.commit").empty_allowed);
        assert!(!entry(&read, "language").empty_allowed);
        assert!(entry(&read, "showThinkingSummaries").ignored);
        assert_eq!(
            entry(&read, "CLAUDE_CODE_DISABLE_1M_CONTEXT").value,
            json!(true)
        );
        assert_eq!(entry(&read, "DISABLE_TELEMETRY").value, json!(false));
        assert!(entry(&read, "MAX_THINKING_TOKENS").ignored);
        assert_eq!(entry(&read, "API_TIMEOUT_MS").value, json!("60000"));
        assert!(entry(&read, "API_TIMEOUT_MS").variable);
    }

    #[test]
    fn unparsable_documents_are_shown_but_not_editable() {
        let read = read_preferences("{ \"maxMode\": true,", CURSOR);
        assert!(!read.editable);
        assert!(read.entries.iter().all(|entry| entry.value.is_null()));
        assert!(apply_preference("[]", CURSOR, "maxMode", &json!(true)).is_err());
        assert!(read_preferences("", CURSOR).editable);
        let codex = catalog("codex", ProviderFileFormat::Toml);
        assert!(!read_preferences("web_search = ", codex).editable);
        assert!(apply_preference("web_search = ", codex, "web_search", &json!("live")).is_err());
    }

    #[test]
    fn edits_change_one_entry_and_keep_other_keys_in_order() {
        let contents = "{\n  \"model\": \"m\",\n  \"permissions\": { \"allow\": [] },\n  \"env\": { \"FOO\": \"bar\" }\n}\n";
        let edited = apply_preference(
            contents,
            CLAUDE,
            "CLAUDE_CODE_DISABLE_1M_CONTEXT",
            &json!(true),
        )
        .unwrap();
        let edited = apply_preference(&edited, CLAUDE, "language", &json!("Japanese")).unwrap();
        let document: Value = serde_json::from_str(&edited).unwrap();
        assert_eq!(
            document,
            json!({
                "model": "m",
                "permissions": { "allow": [] },
                "env": { "FOO": "bar", "CLAUDE_CODE_DISABLE_1M_CONTEXT": "1" },
                "language": "Japanese",
            })
        );
        let keys = document.as_object().unwrap().keys().collect::<Vec<_>>();
        assert_eq!(keys, ["model", "permissions", "env", "language"]);
        assert!(edited.ends_with("}\n"));
    }

    #[test]
    fn turning_switches_off_and_clearing_values_remove_entries_and_empty_parents() {
        let contents = json!({
            "maxMode": true,
            "network": { "useHttp1ForAgent": true },
        })
        .to_string();
        let edited =
            apply_preference(&contents, CURSOR, "network.useHttp1ForAgent", &Value::Null).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&edited).unwrap(),
            json!({ "maxMode": true })
        );
        let contents = json!({ "env": { "DISABLE_TELEMETRY": "1" } }).to_string();
        let edited =
            apply_preference(&contents, CLAUDE, "DISABLE_TELEMETRY", &json!(false)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&edited).unwrap(), json!({}));
    }

    #[test]
    fn edits_reject_values_outside_the_allowlist_or_their_type() {
        for (id, value) in [
            ("permissions.allow", json!(["Bash(*)"])),
            ("MAX_THINKING_TOKENS", json!("-1")),
            ("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE", json!(101)),
            ("language", json!("a\nb")),
            ("language", json!("")),
            ("alwaysThinkingEnabled", json!("true")),
            ("DISABLE_TELEMETRY", json!("1")),
        ] {
            assert!(apply_preference("{}", CLAUDE, id, &value).is_err(), "{id}");
        }
        let blocked = json!({ "env": "DISABLE_TELEMETRY=1" }).to_string();
        assert!(apply_preference(&blocked, CLAUDE, "DISABLE_TELEMETRY", &json!(true)).is_err());
        let counted = apply_preference("", CLAUDE, "MAX_THINKING_TOKENS", &json!(8000)).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&counted).unwrap(),
            json!({ "env": { "MAX_THINKING_TOKENS": "8000" } })
        );
    }

    #[test]
    fn codex_toml_edits_keep_comments_and_store_typed_values() {
        let codex = catalog("codex", ProviderFileFormat::Toml);
        let contents = "# Mine\nmodel = \"gpt\"\nweb_search = \"cached\" # quick\n\n[mcp_servers.docs]\ncommand = \"docs\"\n";
        let edited = apply_preference(contents, codex, "web_search", &json!("live")).unwrap();
        let edited = apply_preference(
            &edited,
            codex,
            "model_auto_compact_token_limit",
            &json!("200000"),
        )
        .unwrap();
        let edited = apply_preference(&edited, codex, "analytics.enabled", &json!(false)).unwrap();
        assert_eq!(
            edited,
            "# Mine\nmodel = \"gpt\"\nweb_search = \"live\" # quick\nmodel_auto_compact_token_limit = 200000\n\n[mcp_servers.docs]\ncommand = \"docs\"\n\n[analytics]\nenabled = false\n"
        );
        let read = read_preferences(&edited, codex);
        assert_eq!(entry(&read, "web_search").value, json!("live"));
        assert_eq!(
            entry(&read, "web_search").options,
            ["disabled", "cached", "indexed", "live"]
        );
        assert_eq!(
            entry(&read, "web_search").value_type,
            ProviderPreferenceType::Choice
        );
        assert_eq!(
            entry(&read, "model_auto_compact_token_limit").value,
            json!("200000")
        );
        assert_eq!(entry(&read, "analytics.enabled").value, json!(false));
        let cleared = apply_preference(&edited, codex, "analytics.enabled", &Value::Null).unwrap();
        assert!(!cleared.contains("[analytics]"));
    }

    #[test]
    fn typed_settings_reject_values_outside_their_bounds() {
        let codex = catalog("codex", ProviderFileFormat::Toml);
        let grok = catalog("grok", ProviderFileFormat::Toml);
        for (catalog, id, value) in [
            (codex, "web_search", json!("everything")),
            (codex, "web_search", json!(true)),
            (codex, "model_context_window", json!("0")),
            (codex, "model_context_window", json!("-5")),
            (codex, "model_context_window", json!("9223372036854775808")),
            (codex, "hide_agent_reasoning", json!("true")),
            (codex, "approval_policy", json!("never")),
            (grok, "session.auto_compact_threshold_percent", json!(101)),
            (grok, "permission.mode", json!("yolo")),
        ] {
            assert!(
                apply_preference("", catalog, id, &value).is_err(),
                "{id} {value}"
            );
        }
        let read = read_preferences("model_context_window = 0\nweb_search = \"LIVE\"\n", codex);
        assert!(entry(&read, "model_context_window").ignored);
        assert!(entry(&read, "web_search").ignored);
        let shared = apply_preference(
            "",
            grok,
            "session.auto_compact_threshold_percent",
            &json!(" 80 "),
        )
        .unwrap();
        assert_eq!(shared, "[session]\nauto_compact_threshold_percent = 80\n");
    }

    #[test]
    fn opencode_edits_keep_comments_in_both_files() {
        for format in [ProviderFileFormat::Json, ProviderFileFormat::Jsonc] {
            let opencode = catalog("opencode", format);
            let contents = "{\n  // Mine\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"share\": \"manual\" // links\n}\n";
            let edited = apply_preference(contents, opencode, "share", &json!("disabled")).unwrap();
            let edited =
                apply_preference(&edited, opencode, "compaction.keep.tokens", &json!(15000))
                    .unwrap();
            let read = read_preferences(&edited, opencode);
            assert_eq!(entry(&read, "share").value, json!("disabled"));
            assert_eq!(entry(&read, "compaction.keep.tokens").value, json!("15000"));
            assert!(edited.starts_with("{\n  // Mine\n"));
            assert!(edited.contains("\"share\": \"disabled\", // links\n"));
            let cleared =
                apply_preference(&edited, opencode, "compaction.keep.tokens", &Value::Null)
                    .unwrap();
            assert_eq!(
                cleared,
                "{\n  // Mine\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"share\": \"disabled\" // links\n}\n"
            );
        }
    }
}
