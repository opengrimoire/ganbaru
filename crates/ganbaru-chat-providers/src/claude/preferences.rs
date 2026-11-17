//! User preferences that Claude teammate runs keep although they load no settings files.
//!
//! Teammate launches pass `--restricted` and `--setting-sources ""`, so nothing in the user
//! settings file applies to them. Ganbaru copies the entries of
//! [`CLAUDE_PREFERENCES`](crate::teammate_preferences::CLAUDE_PREFERENCES) from the user
//! settings file into the launch: settings keys through `--settings` and `env` variables through
//! the process environment. Each value is checked against its type, and a value of the wrong
//! type is left out, as the CLI would reject it.

use crate::teammate_preferences::{
    CLAUDE_PREFERENCES, PreferenceLocation, allowlisted_settings, checked_variable, scalar_text,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Largest user settings file read for preferences.
const MAX_USER_SETTINGS_BYTES: u64 = 1024 * 1024;

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
    let Ok(document @ Value::Object(_)) = serde_json::from_slice::<Value>(bytes) else {
        return ClaudeUserPreferences::default();
    };
    let environment = document
        .get("env")
        .and_then(Value::as_object)
        .map(|env| {
            CLAUDE_PREFERENCES
                .iter()
                .filter_map(|preference| {
                    let PreferenceLocation::Variable(name) = preference.location else {
                        return None;
                    };
                    let value = scalar_text(env.get(name)?)?;
                    checked_variable(preference.value_type, &value)
                        .then(|| (name.to_string(), value))
                })
                .collect()
        })
        .unwrap_or_default();
    ClaudeUserPreferences {
        environment,
        settings: allowlisted_settings(&document, CLAUDE_PREFERENCES),
    }
}

/// Names of the preference variables a Claude process may inherit from the host.
pub(super) fn preference_variable_names() -> impl Iterator<Item = &'static str> {
    CLAUDE_PREFERENCES
        .iter()
        .filter_map(|preference| match preference.location {
            PreferenceLocation::Variable(name) => Some(name),
            PreferenceLocation::Setting(_) => None,
        })
}

/// Whether a host value of the preference variable `name` passes that variable's check.
pub(super) fn host_preference_variable(name: &str, value: &str) -> bool {
    CLAUDE_PREFERENCES
        .iter()
        .find(|preference| matches!(preference.location, PreferenceLocation::Variable(known) if known == name))
        .is_some_and(|preference| checked_variable(preference.value_type, value))
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
