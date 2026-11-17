//! Provider preferences that teammate launches keep although they skip the user's own
//! provider configuration.
//!
//! Claude teammates load no settings files and confined Cursor teammates read a configuration
//! Ganbaru writes, so each of those providers has an allowlist of preferences copied from the
//! user's file. An entry only shapes answers, thinking, limits, transport, or what the CLI
//! reports, and never widens tools, folders, servers, hooks, credentials, or where requests go.
//! The same catalog drives the launch and the typed controls of the Agent files settings page.

use serde_json::{Map, Value};

/// How a preference's value is checked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreferenceType {
    /// A variable the CLI reads as set or unset, such as `1` or `true`.
    Switch,
    /// A JSON boolean.
    Boolean,
    /// A count of tokens, milliseconds, or characters. A setting holds a positive TOML or JSON
    /// integer; a variable may also hold zero.
    Count,
    /// A share from 1 to 100.
    Percentage,
    /// One of a fixed list of strings.
    Choice(&'static [&'static str]),
    /// A JSON string of at most `max_bytes` bytes.
    Text {
        /// Longest value kept.
        max_bytes: usize,
        /// Whether the value must stay on one line without control characters.
        single_line: bool,
        /// Whether an empty string is a meaningful value rather than a missing one.
        empty_allowed: bool,
    },
}

/// Where a preference lives in the provider's user configuration file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreferenceLocation {
    /// A key, nested along the path, of the configuration document.
    Setting(&'static [&'static str]),
    /// A variable of the process environment, or of the `env` object of a Claude settings file.
    Variable(&'static str),
}

/// One allowlisted preference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TeammatePreference {
    /// Where the preference lives.
    pub location: PreferenceLocation,
    /// How its value is checked.
    pub value_type: PreferenceType,
}

impl TeammatePreference {
    /// The stable identifier of the preference: the dotted key path or the variable name.
    pub fn id(&self) -> String {
        match self.location {
            PreferenceLocation::Setting(path) => path.join("."),
            PreferenceLocation::Variable(name) => name.to_string(),
        }
    }
}

/// Longest variable value kept.
const MAX_VARIABLE_BYTES: usize = 32;
/// Largest count setting kept, the largest integer TOML holds.
const MAX_SETTING_COUNT: u64 = i64::MAX as u64;
/// Longest response language name kept.
const MAX_LANGUAGE_BYTES: usize = 64;
/// Longest attribution text kept.
const MAX_ATTRIBUTION_BYTES: usize = 4096;

/// A preference at the key path `path` of the configuration document.
pub const fn setting(
    path: &'static [&'static str],
    value_type: PreferenceType,
) -> TeammatePreference {
    TeammatePreference {
        location: PreferenceLocation::Setting(path),
        value_type,
    }
}

const fn variable(name: &'static str, value_type: PreferenceType) -> TeammatePreference {
    TeammatePreference {
        location: PreferenceLocation::Variable(name),
        value_type,
    }
}

const ATTRIBUTION_TEXT: PreferenceType = PreferenceType::Text {
    max_bytes: MAX_ATTRIBUTION_BYTES,
    single_line: false,
    empty_allowed: true,
};

/// Claude preferences: settings keys join `--settings` and variables join the process
/// environment. Model and effort variables are left out because Ganbaru's model picker owns
/// them, and so is anything that changes credentials, endpoints, telemetry exporters, or the
/// shell.
pub const CLAUDE_PREFERENCES: &[TeammatePreference] = &[
    setting(
        &["language"],
        PreferenceType::Text {
            max_bytes: MAX_LANGUAGE_BYTES,
            single_line: true,
            empty_allowed: false,
        },
    ),
    setting(&["alwaysThinkingEnabled"], PreferenceType::Boolean),
    setting(&["showThinkingSummaries"], PreferenceType::Boolean),
    setting(&["attribution", "commit"], ATTRIBUTION_TEXT),
    setting(&["attribution", "pr"], ATTRIBUTION_TEXT),
    variable("CLAUDE_CODE_DISABLE_1M_CONTEXT", PreferenceType::Switch),
    variable("CLAUDE_CODE_DISABLE_THINKING", PreferenceType::Switch),
    variable(
        "CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING",
        PreferenceType::Switch,
    ),
    variable("CLAUDE_CODE_MAX_OUTPUT_TOKENS", PreferenceType::Count),
    variable("MAX_THINKING_TOKENS", PreferenceType::Count),
    variable(
        "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
        PreferenceType::Percentage,
    ),
    variable("API_TIMEOUT_MS", PreferenceType::Count),
    variable("BASH_DEFAULT_TIMEOUT_MS", PreferenceType::Count),
    variable("BASH_MAX_TIMEOUT_MS", PreferenceType::Count),
    variable("BASH_MAX_OUTPUT_LENGTH", PreferenceType::Count),
    variable("DISABLE_TELEMETRY", PreferenceType::Switch),
    variable("DISABLE_ERROR_REPORTING", PreferenceType::Switch),
    variable(
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
        PreferenceType::Switch,
    ),
    variable(
        "CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY",
        PreferenceType::Switch,
    ),
];

/// Cursor CLI configuration keys a confined launch copies. They change model tier, transport,
/// or commit trailers only, never permissions, approval, the sandbox, or what runs.
pub const CURSOR_PREFERENCES: &[TeammatePreference] = &[
    setting(&["maxMode"], PreferenceType::Boolean),
    setting(&["network", "useHttp1ForAgent"], PreferenceType::Boolean),
    setting(
        &["attribution", "attributeCommitsToAgent"],
        PreferenceType::Boolean,
    ),
    setting(
        &["attribution", "attributePRsToAgent"],
        PreferenceType::Boolean,
    ),
];

/// The allowlist of a provider family; empty for families whose teammates read the user's
/// configuration natively.
pub fn teammate_preferences(family_id: &str) -> &'static [TeammatePreference] {
    match family_id {
        "claude" => CLAUDE_PREFERENCES,
        "cursor" => CURSOR_PREFERENCES,
        _ => &[],
    }
}

/// Whether `value` fits a variable of `value_type`.
pub fn checked_variable(value_type: PreferenceType, value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_VARIABLE_BYTES || value.contains('\0') {
        return false;
    }
    match value_type {
        PreferenceType::Switch => value.chars().all(|c| c.is_ascii_alphanumeric()),
        PreferenceType::Count => value.parse::<u64>().is_ok(),
        PreferenceType::Percentage => value
            .parse::<u8>()
            .is_ok_and(|share| (1..=100).contains(&share)),
        PreferenceType::Boolean | PreferenceType::Text { .. } | PreferenceType::Choice(_) => false,
    }
}

/// Whether a switch variable's value turns it on, as Claude Code reads `1`, `true`, `yes`, and
/// `on` in any case.
pub fn switch_enabled(value: &str) -> bool {
    ["1", "true", "yes", "on"]
        .iter()
        .any(|enabled| value.trim().eq_ignore_ascii_case(enabled))
}

/// The value of a setting of `value_type`, when it has that type and passes its bounds.
pub fn checked_setting(value_type: PreferenceType, value: &Value) -> Option<Value> {
    match value_type {
        PreferenceType::Boolean => value.as_bool().map(Value::Bool),
        PreferenceType::Text {
            max_bytes,
            single_line,
            empty_allowed,
        } => {
            let text = value.as_str()?;
            let fits = text.len() <= max_bytes
                && !text.contains('\0')
                && (!single_line || !text.chars().any(char::is_control))
                && (empty_allowed || !text.trim().is_empty());
            fits.then(|| Value::String(text.to_string()))
        }
        PreferenceType::Count => value
            .as_u64()
            .filter(|count| (1..=MAX_SETTING_COUNT).contains(count))
            .map(Value::from),
        PreferenceType::Percentage => value
            .as_u64()
            .filter(|share| (1..=100).contains(share))
            .map(Value::from),
        PreferenceType::Choice(options) => {
            let choice = value.as_str()?;
            options
                .contains(&choice)
                .then(|| Value::String(choice.to_string()))
        }
        PreferenceType::Switch => None,
    }
}

/// String, number, and boolean values as a CLI reads them from an `env` object.
pub fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

/// The value at `path` of a JSON document.
pub fn value_at<'a>(document: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(document, |value, key| value.get(*key))
}

/// Sets `value` at `path`, creating missing parent objects. Returns false and changes nothing
/// when a parent exists but is not an object.
pub fn insert_at(document: &mut Map<String, Value>, path: &[&str], value: Value) -> bool {
    let Some((leaf, parents)) = path.split_last() else {
        return false;
    };
    if path_blocked(document, parents) {
        return false;
    }
    let mut target = document;
    for key in parents {
        let entry = target
            .entry((*key).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let Some(object) = entry.as_object_mut() else {
            return false;
        };
        target = object;
    }
    target.insert((*leaf).to_string(), value);
    true
}

/// Whether a parent along `parents` exists and is not an object.
fn path_blocked(document: &Map<String, Value>, parents: &[&str]) -> bool {
    let mut target = document;
    for key in parents {
        match target.get(*key) {
            None => return false,
            Some(Value::Object(object)) => target = object,
            Some(_) => return true,
        }
    }
    false
}

/// Removes the value at `path` and every parent object the removal leaves empty.
pub fn remove_at(document: &mut Map<String, Value>, path: &[&str]) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    if rest.is_empty() {
        document.remove(*first);
        return;
    }
    if let Some(Value::Object(child)) = document.get_mut(*first) {
        remove_at(child, rest);
        if child.is_empty() {
            document.remove(*first);
        }
    }
}

/// The allowlisted settings of `document`, nested as in the document, keeping only values that
/// pass their check.
pub fn allowlisted_settings(
    document: &Value,
    preferences: &[TeammatePreference],
) -> Map<String, Value> {
    let mut kept = Map::new();
    for preference in preferences {
        let PreferenceLocation::Setting(path) = preference.location else {
            continue;
        };
        if let Some(value) =
            value_at(document, path).and_then(|value| checked_setting(preference.value_type, value))
        {
            insert_at(&mut kept, path, value);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identifiers_are_dotted_paths_or_variable_names_and_unique() {
        for catalog in [CLAUDE_PREFERENCES, CURSOR_PREFERENCES] {
            let mut ids = catalog
                .iter()
                .map(TeammatePreference::id)
                .collect::<Vec<_>>();
            let count = ids.len();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), count);
        }
        assert_eq!(CLAUDE_PREFERENCES[3].id(), "attribution.commit");
        assert!(teammate_preferences("codex").is_empty());
    }

    #[test]
    fn settings_keep_documented_types_and_bounds_only() {
        let language = CLAUDE_PREFERENCES[0].value_type;
        assert_eq!(
            checked_setting(language, &json!("Spanish")),
            Some(json!("Spanish"))
        );
        assert_eq!(checked_setting(language, &json!("a\nb")), None);
        assert_eq!(checked_setting(language, &json!("  ")), None);
        assert_eq!(checked_setting(language, &json!("x".repeat(65))), None);
        assert_eq!(
            checked_setting(ATTRIBUTION_TEXT, &json!("")),
            Some(json!(""))
        );
        assert_eq!(
            checked_setting(ATTRIBUTION_TEXT, &json!("Line\n\nTrailer")),
            Some(json!("Line\n\nTrailer"))
        );
        assert_eq!(checked_setting(ATTRIBUTION_TEXT, &json!("a\0b")), None);
        assert_eq!(
            checked_setting(PreferenceType::Boolean, &json!("true")),
            None
        );
    }

    #[test]
    fn numeric_and_choice_settings_keep_values_in_bounds_only() {
        let count = PreferenceType::Count;
        assert_eq!(checked_setting(count, &json!(8000)), Some(json!(8000)));
        for rejected in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("8000"),
            json!(u64::MAX),
        ] {
            assert_eq!(checked_setting(count, &rejected), None, "{rejected}");
        }
        let share = PreferenceType::Percentage;
        assert_eq!(checked_setting(share, &json!(85)), Some(json!(85)));
        assert_eq!(checked_setting(share, &json!(0)), None);
        assert_eq!(checked_setting(share, &json!(101)), None);
        let choice = PreferenceType::Choice(&["low", "high"]);
        assert_eq!(checked_setting(choice, &json!("low")), Some(json!("low")));
        assert_eq!(checked_setting(choice, &json!("LOW")), None);
        assert_eq!(checked_setting(choice, &json!(1)), None);
        assert!(!checked_variable(choice, "low"));
    }

    #[test]
    fn variables_keep_values_of_their_type_only() {
        assert!(checked_variable(PreferenceType::Switch, "1"));
        assert!(!checked_variable(PreferenceType::Switch, "1; rm"));
        assert!(checked_variable(PreferenceType::Count, "8000"));
        assert!(!checked_variable(PreferenceType::Count, "-5"));
        assert!(checked_variable(PreferenceType::Percentage, "100"));
        assert!(!checked_variable(PreferenceType::Percentage, "0"));
        assert!(!checked_variable(PreferenceType::Count, ""));
    }

    #[test]
    fn nested_values_are_inserted_and_removed_with_their_empty_parents() {
        let mut document = json!({ "attribution": { "pr": "x" }, "model": "m" })
            .as_object()
            .cloned()
            .unwrap();
        assert!(insert_at(
            &mut document,
            &["attribution", "commit"],
            json!("")
        ));
        assert!(insert_at(
            &mut document,
            &["network", "useHttp1ForAgent"],
            json!(true)
        ));
        assert!(!insert_at(&mut document, &["model", "nested"], json!(true)));
        assert_eq!(
            Value::Object(document.clone()),
            json!({
                "attribution": { "pr": "x", "commit": "" },
                "model": "m",
                "network": { "useHttp1ForAgent": true },
            })
        );
        remove_at(&mut document, &["network", "useHttp1ForAgent"]);
        remove_at(&mut document, &["attribution", "commit"]);
        remove_at(&mut document, &["missing", "key"]);
        assert_eq!(
            Value::Object(document),
            json!({ "attribution": { "pr": "x" }, "model": "m" })
        );
    }

    #[test]
    fn allowlisted_settings_skip_other_keys_and_wrong_types() {
        let document = json!({
            "maxMode": true,
            "network": { "useHttp1ForAgent": "yes", "proxy": "http://example.invalid" },
            "attribution": { "attributePRsToAgent": false },
            "permissions": { "allow": ["Shell(*)"] },
        });
        assert_eq!(
            Value::Object(allowlisted_settings(&document, CURSOR_PREFERENCES)),
            json!({ "maxMode": true, "attribution": { "attributePRsToAgent": false } })
        );
    }
}
