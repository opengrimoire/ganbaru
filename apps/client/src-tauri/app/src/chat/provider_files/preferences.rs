//! Typed reads and edits of the teammate preferences in a provider configuration file.
//!
//! Each provider family with an allowlist in
//! [`ganbaru_chat_providers::teammate_preferences`] shows those entries as typed controls. A
//! read reports the value a teammate launch would keep, and an edit changes only that entry of
//! the user's JSON document and keeps every other key and its order.

use ganbaru_chat_contracts::models::{ChatError, ChatResult};
use ganbaru_chat_providers::teammate_preferences::{
    PreferenceLocation, PreferenceType, TeammatePreference, checked_setting, checked_variable,
    insert_at, remove_at, scalar_text, switch_enabled, value_at,
};
use serde::Serialize;
use serde_json::{Map, Value};

/// Object of a Claude settings file that holds environment variables.
const VARIABLES_OBJECT: &str = "env";

/// How the settings page edits a preference.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPreferenceType {
    Switch,
    Boolean,
    Count,
    Percentage,
    Text,
}

/// One allowlisted preference and the value the configuration file gives it.
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
    /// Whether the file sets a value that teammate launches skip because it fails its check.
    pub ignored: bool,
    /// Longest accepted text, for text preferences.
    pub max_bytes: Option<usize>,
    /// Whether a text preference may span lines.
    pub multiline: bool,
    /// Whether an empty text is a meaningful value rather than a missing one.
    pub empty_allowed: bool,
}

/// The teammate preferences of one configuration file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreferencesRead {
    /// Whether the file is missing, empty, or a JSON object, so entries can be edited here.
    pub editable: bool,
    pub entries: Vec<ProviderPreferenceRead>,
}

/// Reads `preferences` from a configuration document's text.
pub(super) fn read_preferences(
    contents: &str,
    preferences: &[TeammatePreference],
) -> ProviderPreferencesRead {
    let document = parse_document(contents);
    let empty = Map::new();
    let object = document.as_ref().unwrap_or(&empty);
    ProviderPreferencesRead {
        editable: document.is_some(),
        entries: preferences
            .iter()
            .map(|preference| read_preference(object, preference))
            .collect(),
    }
}

/// Returns `contents` with the preference `preference_id` set to `value`, or removed when
/// `value` is null or a switch is turned off.
///
/// # Errors
///
/// Returns a validation error when the preference is not allowlisted, the value fails its
/// check, or the document is not a JSON object.
pub(super) fn apply_preference(
    contents: &str,
    preferences: &[TeammatePreference],
    preference_id: &str,
    value: &Value,
) -> ChatResult<String> {
    let preference = preferences
        .iter()
        .find(|preference| preference.id() == preference_id)
        .ok_or_else(|| ChatError::validation("preferenceId", "Preference is not supported"))?;
    let mut document = parse_document(contents).ok_or_else(|| {
        ChatError::validation(
            "providerFile",
            "Fix the configuration file before changing preferences here",
        )
    })?;
    let path = document_path(preference);
    match stored_value(preference.value_type, value)? {
        Some(stored) => {
            if !insert_at(&mut document, &path, stored) {
                return Err(ChatError::validation(
                    "providerFile",
                    "The configuration file holds another value where this preference belongs",
                ));
            }
        }
        None => remove_at(&mut document, &path),
    }
    let mut text = serde_json::to_string_pretty(&Value::Object(document))
        .map_err(|_| ChatError::validation("providerFile", "Preference could not be saved"))?;
    text.push('\n');
    Ok(text)
}

/// The document's top-level object; an empty or blank document counts as an empty object.
fn parse_document(contents: &str) -> Option<Map<String, Value>> {
    if contents.trim().is_empty() {
        return Some(Map::new());
    }
    match serde_json::from_str::<Value>(contents) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

/// Key path of a preference inside the document.
fn document_path(preference: &TeammatePreference) -> Vec<&'static str> {
    match preference.location {
        PreferenceLocation::Setting(path) => path.to_vec(),
        PreferenceLocation::Variable(name) => vec![VARIABLES_OBJECT, name],
    }
}

fn read_preference(
    document: &Map<String, Value>,
    preference: &TeammatePreference,
) -> ProviderPreferenceRead {
    let path = document_path(preference);
    let raw = path
        .split_first()
        .and_then(|(first, rest)| value_at(document.get(*first)?, rest));
    let value = raw.and_then(|raw| shown_value(preference, raw));
    let (max_bytes, multiline, empty_allowed) = match preference.value_type {
        PreferenceType::Text {
            max_bytes,
            single_line,
            empty_allowed,
        } => (Some(max_bytes), !single_line, empty_allowed),
        _ => (None, false, false),
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
    }
}

/// The value the settings page shows for a raw document value, when it passes its check.
fn shown_value(preference: &TeammatePreference, raw: &Value) -> Option<Value> {
    match preference.location {
        PreferenceLocation::Setting(_) => checked_setting(preference.value_type, raw),
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
fn stored_value(value_type: PreferenceType, value: &Value) -> ChatResult<Option<Value>> {
    if value.is_null() {
        return Ok(None);
    }
    let invalid = || ChatError::validation("value", "Preference value is not valid");
    match value_type {
        PreferenceType::Switch => {
            let enabled = value.as_bool().ok_or_else(invalid)?;
            Ok(enabled.then(|| Value::String("1".to_string())))
        }
        PreferenceType::Count | PreferenceType::Percentage => {
            let text = match value {
                Value::String(_) | Value::Number(_) => scalar_text(value),
                _ => None,
            }
            .ok_or_else(invalid)?;
            let text = text.trim().to_string();
            checked_variable(value_type, &text)
                .then_some(Some(Value::String(text)))
                .ok_or_else(invalid)
        }
        PreferenceType::Boolean | PreferenceType::Text { .. } => checked_setting(value_type, value)
            .map(Some)
            .ok_or_else(invalid),
    }
}

fn preference_type(value_type: PreferenceType) -> ProviderPreferenceType {
    match value_type {
        PreferenceType::Switch => ProviderPreferenceType::Switch,
        PreferenceType::Boolean => ProviderPreferenceType::Boolean,
        PreferenceType::Count => ProviderPreferenceType::Count,
        PreferenceType::Percentage => ProviderPreferenceType::Percentage,
        PreferenceType::Text { .. } => ProviderPreferenceType::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_chat_providers::teammate_preferences::{CLAUDE_PREFERENCES, CURSOR_PREFERENCES};
    use serde_json::json;

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
        let read = read_preferences(&contents, CLAUDE_PREFERENCES);
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
        let read = read_preferences("{ \"maxMode\": true,", CURSOR_PREFERENCES);
        assert!(!read.editable);
        assert!(read.entries.iter().all(|entry| entry.value.is_null()));
        assert!(apply_preference("[]", CURSOR_PREFERENCES, "maxMode", &json!(true)).is_err());
        assert!(read_preferences("", CURSOR_PREFERENCES).editable);
    }

    #[test]
    fn edits_change_one_entry_and_keep_other_keys_in_order() {
        let contents = "{\n  \"model\": \"m\",\n  \"permissions\": { \"allow\": [] },\n  \"env\": { \"FOO\": \"bar\" }\n}\n";
        let edited = apply_preference(
            contents,
            CLAUDE_PREFERENCES,
            "CLAUDE_CODE_DISABLE_1M_CONTEXT",
            &json!(true),
        )
        .unwrap();
        let edited =
            apply_preference(&edited, CLAUDE_PREFERENCES, "language", &json!("Japanese")).unwrap();
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
        let edited = apply_preference(
            &contents,
            CURSOR_PREFERENCES,
            "network.useHttp1ForAgent",
            &Value::Null,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&edited).unwrap(),
            json!({ "maxMode": true })
        );
        let contents = json!({ "env": { "DISABLE_TELEMETRY": "1" } }).to_string();
        let edited = apply_preference(
            &contents,
            CLAUDE_PREFERENCES,
            "DISABLE_TELEMETRY",
            &json!(false),
        )
        .unwrap();
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
            assert!(
                apply_preference("{}", CLAUDE_PREFERENCES, id, &value).is_err(),
                "{id}"
            );
        }
        let blocked = json!({ "env": "DISABLE_TELEMETRY=1" }).to_string();
        assert!(
            apply_preference(
                &blocked,
                CLAUDE_PREFERENCES,
                "DISABLE_TELEMETRY",
                &json!(true)
            )
            .is_err()
        );
        let counted =
            apply_preference("", CLAUDE_PREFERENCES, "MAX_THINKING_TOKENS", &json!(8000)).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&counted).unwrap(),
            json!({ "env": { "MAX_THINKING_TOKENS": "8000" } })
        );
    }
}
