//! User preferences that Claude teammate runs keep although they load no settings files.
//!
//! Teammate launches pass `--restricted` and `--setting-sources ""`, so nothing in the user
//! settings file applies to them. The allowlist below names the settings keys and `env`
//! variables that only shape answers, thinking, limits, or what the CLI reports, and never widen
//! tools, folders, servers, hooks, credentials, or where requests go. Ganbaru copies them from
//! the user settings file into the launch: keys through `--settings` and variables through the
//! process environment. Each value is checked against its type, and a value of the wrong type is
//! left out, as the CLI would reject it.

use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Largest user settings file read for preferences.
const MAX_USER_SETTINGS_BYTES: u64 = 1024 * 1024;
/// Longest string kept from a settings key.
const MAX_SETTING_TEXT_BYTES: usize = 4096;
/// Longest response language name kept.
const MAX_LANGUAGE_BYTES: usize = 64;
/// Longest variable value kept.
const MAX_VARIABLE_BYTES: usize = 32;

/// How a preference variable's value is checked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PreferenceVariableKind {
    /// A switch the CLI reads as set or unset, such as `1` or `true`.
    Switch,
    /// A count of tokens, milliseconds, or characters.
    Count,
    /// A share of the context window from 1 to 100.
    Percentage,
}

/// Environment variables a Claude process honors from the host and from the `env` of the user
/// settings file. Model and effort variables are left out because Ganbaru's model picker owns
/// them, and so is anything that changes credentials, endpoints, telemetry exporters, or the
/// shell.
pub(super) const PREFERENCE_VARIABLES: &[(&str, PreferenceVariableKind)] = &[
    (
        "CLAUDE_CODE_DISABLE_1M_CONTEXT",
        PreferenceVariableKind::Switch,
    ),
    (
        "CLAUDE_CODE_DISABLE_THINKING",
        PreferenceVariableKind::Switch,
    ),
    (
        "CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING",
        PreferenceVariableKind::Switch,
    ),
    (
        "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
        PreferenceVariableKind::Count,
    ),
    ("MAX_THINKING_TOKENS", PreferenceVariableKind::Count),
    (
        "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
        PreferenceVariableKind::Percentage,
    ),
    ("API_TIMEOUT_MS", PreferenceVariableKind::Count),
    ("BASH_DEFAULT_TIMEOUT_MS", PreferenceVariableKind::Count),
    ("BASH_MAX_TIMEOUT_MS", PreferenceVariableKind::Count),
    ("BASH_MAX_OUTPUT_LENGTH", PreferenceVariableKind::Count),
    ("DISABLE_TELEMETRY", PreferenceVariableKind::Switch),
    ("DISABLE_ERROR_REPORTING", PreferenceVariableKind::Switch),
    (
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
        PreferenceVariableKind::Switch,
    ),
    (
        "CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY",
        PreferenceVariableKind::Switch,
    ),
];

/// Settings keys a teammate launch copies from the user settings file into `--settings`.
pub(super) const PREFERENCE_SETTINGS: &[&str] = &[
    "language",
    "alwaysThinkingEnabled",
    "showThinkingSummaries",
    "attribution",
];

/// The preferences found in one user settings file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ClaudeUserPreferences {
    /// Allowlisted `env` variables, as the CLI would read them.
    pub environment: BTreeMap<String, String>,
    /// Allowlisted settings keys with checked values.
    pub settings: Map<String, Value>,
}

/// Reads the preferences in `settings.json` of the Claude configuration directory. A missing,
/// oversized, or malformed file yields none, because the CLI then applies none either.
pub(super) fn user_preferences(config_directory: &Path) -> ClaudeUserPreferences {
    ganbaru_atomic_files::read_regular_file_capped(
        &config_directory.join("settings.json"),
        MAX_USER_SETTINGS_BYTES,
        ganbaru_atomic_files::FinalSymlink::Follow,
    )
    .map(|bytes| preferences_from_settings(&bytes))
    .unwrap_or_default()
}

/// The allowlisted preferences of a Claude settings document.
pub(super) fn preferences_from_settings(bytes: &[u8]) -> ClaudeUserPreferences {
    let Ok(Value::Object(settings)) = serde_json::from_slice::<Value>(bytes) else {
        return ClaudeUserPreferences::default();
    };
    let environment = settings
        .get("env")
        .and_then(Value::as_object)
        .map(|env| {
            PREFERENCE_VARIABLES
                .iter()
                .filter_map(|(name, kind)| {
                    let value = scalar_text(env.get(*name)?)?;
                    preference_variable_value(*kind, &value).then(|| ((*name).to_string(), value))
                })
                .collect()
        })
        .unwrap_or_default();
    let settings = PREFERENCE_SETTINGS
        .iter()
        .filter_map(|key| {
            let value = checked_setting(key, settings.get(*key)?)?;
            Some(((*key).to_string(), value))
        })
        .collect();
    ClaudeUserPreferences {
        environment,
        settings,
    }
}

/// Whether a host value of the preference variable `name` passes that variable's check.
pub(super) fn host_preference_variable(name: &str, value: &str) -> bool {
    PREFERENCE_VARIABLES
        .iter()
        .find(|(known, _)| *known == name)
        .is_some_and(|(_, kind)| preference_variable_value(*kind, value))
}

/// Whether `value` fits a variable of `kind`.
fn preference_variable_value(kind: PreferenceVariableKind, value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_VARIABLE_BYTES || value.contains('\0') {
        return false;
    }
    match kind {
        PreferenceVariableKind::Switch => value.chars().all(|c| c.is_ascii_alphanumeric()),
        PreferenceVariableKind::Count => value.parse::<u64>().is_ok(),
        PreferenceVariableKind::Percentage => value
            .parse::<u8>()
            .is_ok_and(|share| (1..=100).contains(&share)),
    }
}

/// String, number, and boolean values as the CLI reads them from `env`.
fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

/// The value of an allowlisted settings key, when it has the documented type.
fn checked_setting(key: &str, value: &Value) -> Option<Value> {
    match key {
        "language" => value
            .as_str()
            .filter(|language| {
                !language.trim().is_empty()
                    && language.len() <= MAX_LANGUAGE_BYTES
                    && !language.chars().any(char::is_control)
            })
            .map(|language| Value::String(language.to_string())),
        "alwaysThinkingEnabled" | "showThinkingSummaries" => value.as_bool().map(Value::Bool),
        "attribution" => {
            let attribution = value.as_object()?;
            let kept = ["commit", "pr"]
                .iter()
                .filter_map(|field| {
                    let text = attribution.get(*field)?.as_str()?;
                    (text.len() <= MAX_SETTING_TEXT_BYTES && !text.contains('\0'))
                        .then(|| ((*field).to_string(), Value::String(text.to_string())))
                })
                .collect::<Map<_, _>>();
            (!kept.is_empty()).then_some(Value::Object(kept))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read(settings: Value) -> ClaudeUserPreferences {
        preferences_from_settings(settings.to_string().as_bytes())
    }

    #[test]
    fn allowlisted_variables_keep_scalar_values_of_their_type() {
        let preferences = read(json!({ "env": {
            "CLAUDE_CODE_DISABLE_1M_CONTEXT": 1,
            "DISABLE_TELEMETRY": true,
            "MAX_THINKING_TOKENS": "8000",
            "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "80",
        } }));
        assert_eq!(
            preferences.environment,
            BTreeMap::from([
                (
                    "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE".to_string(),
                    "80".to_string()
                ),
                (
                    "CLAUDE_CODE_DISABLE_1M_CONTEXT".to_string(),
                    "1".to_string()
                ),
                ("DISABLE_TELEMETRY".to_string(), "true".to_string()),
                ("MAX_THINKING_TOKENS".to_string(), "8000".to_string()),
            ])
        );
    }

    #[test]
    fn variables_outside_the_allowlist_or_their_type_are_left_out() {
        let preferences = read(json!({ "env": {
            "ANTHROPIC_BASE_URL": "https://example.invalid",
            "ANTHROPIC_MODEL": "claude-opus-5-5",
            "CLAUDE_CODE_SHELL_PREFIX": "/bin/sh",
            "NODE_OPTIONS": "--require /tmp/x.js",
            "MAX_THINKING_TOKENS": "lots",
            "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "0",
            "API_TIMEOUT_MS": "-5",
            "CLAUDE_CODE_DISABLE_1M_CONTEXT": ["1"],
            "DISABLE_TELEMETRY": "1; rm",
        } }));
        assert!(preferences.environment.is_empty());
    }

    #[test]
    fn allowlisted_settings_keep_documented_types_only() {
        let preferences = read(json!({
            "language": "Spanish",
            "alwaysThinkingEnabled": false,
            "showThinkingSummaries": "yes",
            "attribution": { "commit": "", "pr": 4, "extra": "x" },
            "hooks": { "Stop": [] },
            "permissions": { "allow": ["Bash(*)"] },
            "apiKeyHelper": "/bin/key",
            "model": "claude-opus-5-5",
        }));
        assert_eq!(
            Value::Object(preferences.settings),
            json!({
                "language": "Spanish",
                "alwaysThinkingEnabled": false,
                "attribution": { "commit": "" },
            })
        );
    }

    #[test]
    fn malformed_documents_yield_no_preferences() {
        assert_eq!(
            preferences_from_settings(b"{ not json"),
            ClaudeUserPreferences::default()
        );
        assert_eq!(read(json!(["language"])), ClaudeUserPreferences::default());
        assert!(
            read(json!({ "env": "DISABLE_TELEMETRY=1" }))
                .environment
                .is_empty()
        );
        assert!(read(json!({ "language": "a\nb" })).settings.is_empty());
    }

    #[test]
    fn host_variables_pass_the_same_check() {
        assert!(host_preference_variable("BASH_MAX_TIMEOUT_MS", "600000"));
        assert!(!host_preference_variable("BASH_MAX_TIMEOUT_MS", "ten"));
        assert!(!host_preference_variable("ANTHROPIC_API_KEY", "1"));
    }
}
