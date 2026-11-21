//! Preferences the Agent files page edits in the configuration files of providers whose
//! agents read that configuration themselves.
//!
//! Codex, Grok, and OpenCode agents load the user's file natively, so these catalogs only
//! make common entries editable without raw TOML or JSON. Like the Claude and Cursor
//! allowlists, they hold entries that shape answers, limits, history, sharing, or what the CLI
//! reports, and leave out models and effort, which Ganbaru picks per agent, and approvals,
//! sandboxes, permissions, tools, servers, providers, hooks, and credentials.

use ganbaru_chat_providers::agent_preferences::{
    AgentPreference, PreferenceType, agent_preferences, setting,
};

use super::{DocumentSyntax, PreferenceCatalog};
use crate::chat::provider_files::ProviderFileFormat;

/// Codex `config.toml` entries, as the Codex configuration reference documents them.
const CODEX_PREFERENCES: &[AgentPreference] = &[
    setting(
        &["model_reasoning_summary"],
        PreferenceType::Choice(&["auto", "concise", "detailed", "none"]),
    ),
    setting(
        &["model_verbosity"],
        PreferenceType::Choice(&["low", "medium", "high"]),
    ),
    setting(
        &["web_search"],
        PreferenceType::Choice(&["disabled", "cached", "indexed", "live"]),
    ),
    setting(
        &["history", "persistence"],
        PreferenceType::Choice(&["save-all", "none"]),
    ),
    setting(&["hide_agent_reasoning"], PreferenceType::Boolean),
    setting(&["show_raw_agent_reasoning"], PreferenceType::Boolean),
    setting(&["check_for_update_on_startup"], PreferenceType::Boolean),
    setting(&["analytics", "enabled"], PreferenceType::Boolean),
    setting(&["feedback", "enabled"], PreferenceType::Boolean),
    setting(&["model_context_window"], PreferenceType::Count),
    setting(&["model_auto_compact_token_limit"], PreferenceType::Count),
    setting(&["tool_output_token_limit"], PreferenceType::Count),
    setting(&["project_doc_max_bytes"], PreferenceType::Count),
];

/// Grok `config.toml` entries, as the Grok settings reference documents them.
const GROK_PREFERENCES: &[AgentPreference] = &[
    setting(
        &["session", "auto_compact_threshold_percent"],
        PreferenceType::Percentage,
    ),
    setting(&["toolset", "bash", "timeout_secs"], PreferenceType::Count),
    setting(
        &["toolset", "bash", "max_timeout_secs"],
        PreferenceType::Count,
    ),
    setting(
        &["toolset", "bash", "output_byte_limit"],
        PreferenceType::Count,
    ),
    setting(&["tools", "respect_gitignore"], PreferenceType::Boolean),
    setting(&["features", "web_fetch"], PreferenceType::Boolean),
    setting(&["subagents", "enabled"], PreferenceType::Boolean),
    setting(&["memory", "enabled"], PreferenceType::Boolean),
    setting(&["cli", "auto_update"], PreferenceType::Boolean),
    setting(&["ui", "show_thinking_blocks"], PreferenceType::Boolean),
];

/// OpenCode `opencode.json` and `opencode.jsonc` entries, as the OpenCode configuration schema
/// documents them.
const OPENCODE_PREFERENCES: &[AgentPreference] = &[
    setting(
        &["share"],
        PreferenceType::Choice(&["manual", "auto", "disabled"]),
    ),
    setting(
        &["update"],
        PreferenceType::Choice(&["disable", "notify", "auto"]),
    ),
    setting(&["snapshots"], PreferenceType::Boolean),
    setting(&["compaction", "auto"], PreferenceType::Boolean),
    setting(&["compaction", "keep", "tokens"], PreferenceType::Count),
    setting(&["tool_output", "max_lines"], PreferenceType::Count),
    setting(&["tool_output", "max_bytes"], PreferenceType::Count),
];

/// The preferences a provider family's configuration file in `format` offers, and how its text
/// is edited, or `None` when the page edits none.
pub(in crate::chat::provider_files) fn file_preferences(
    family_id: &str,
    format: ProviderFileFormat,
) -> Option<PreferenceCatalog> {
    let (syntax, preferences) = match (family_id, format) {
        ("claude" | "cursor", ProviderFileFormat::Json) => {
            (DocumentSyntax::Json, agent_preferences(family_id))
        }
        ("codex", ProviderFileFormat::Toml) => (DocumentSyntax::Toml, CODEX_PREFERENCES),
        ("grok", ProviderFileFormat::Toml) => (DocumentSyntax::Toml, GROK_PREFERENCES),
        // OpenCode reads both files as JSON with comments.
        ("opencode", ProviderFileFormat::Json | ProviderFileFormat::Jsonc) => {
            (DocumentSyntax::Jsonc, OPENCODE_PREFERENCES)
        }
        _ => return None,
    };
    (!preferences.is_empty()).then_some(PreferenceCatalog {
        syntax,
        preferences,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_unique_settings() {
        for catalog in [CODEX_PREFERENCES, GROK_PREFERENCES, OPENCODE_PREFERENCES] {
            let mut ids = catalog.iter().map(AgentPreference::id).collect::<Vec<_>>();
            let count = ids.len();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), count);
            assert!(catalog.iter().all(|preference| matches!(
                preference.location,
                ganbaru_chat_providers::agent_preferences::PreferenceLocation::Setting(_)
            )));
        }
    }

    #[test]
    fn each_family_edits_only_the_file_format_its_cli_reads() {
        let syntax =
            |family, format| file_preferences(family, format).map(|catalog| catalog.syntax);
        assert_eq!(
            syntax("claude", ProviderFileFormat::Json),
            Some(DocumentSyntax::Json)
        );
        assert_eq!(
            syntax("codex", ProviderFileFormat::Toml),
            Some(DocumentSyntax::Toml)
        );
        assert_eq!(
            syntax("grok", ProviderFileFormat::Toml),
            Some(DocumentSyntax::Toml)
        );
        assert_eq!(
            syntax("opencode", ProviderFileFormat::Json),
            Some(DocumentSyntax::Jsonc)
        );
        assert_eq!(
            syntax("opencode", ProviderFileFormat::Jsonc),
            Some(DocumentSyntax::Jsonc)
        );
        assert_eq!(syntax("codex", ProviderFileFormat::Markdown), None);
        assert_eq!(syntax("claude", ProviderFileFormat::Markdown), None);
        assert_eq!(syntax("acp", ProviderFileFormat::Json), None);
    }
}
