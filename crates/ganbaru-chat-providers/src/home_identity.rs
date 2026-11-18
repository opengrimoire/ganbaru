//! The provider home a configuration signs in with on this device, so two configurations that
//! share one account can be told apart from two that only differ in how they name it.

use ganbaru_chat_contracts::models::ProviderInstanceConfig;
use std::path::PathBuf;

/// Where a family keeps its sign-in when a configuration names no home: the variable the provider
/// reads first, then the folder under the user's home.
struct DefaultHome {
    family_id: &'static str,
    variable: &'static str,
    folder: &'static [&'static str],
}

const DEFAULT_HOMES: &[DefaultHome] = &[
    DefaultHome {
        family_id: "claude",
        variable: "CLAUDE_CONFIG_DIR",
        folder: &[".claude"],
    },
    DefaultHome {
        family_id: "codex",
        variable: "CODEX_HOME",
        folder: &[".codex"],
    },
    DefaultHome {
        family_id: "grok",
        variable: "GROK_HOME",
        folder: &[".grok"],
    },
    DefaultHome {
        family_id: "opencode",
        variable: "OPENCODE_CONFIG_DIR",
        folder: &[".config", "opencode"],
    },
];

/// The canonical home `configuration` launches with: the configured home, or the family's
/// default when it names none. A folder that does not exist yet keeps its absolute spelling, and
/// `None` means the family has no home Ganbaru knows of.
pub fn provider_home_identity(configuration: &ProviderInstanceConfig) -> Option<PathBuf> {
    let configured = configuration
        .provider_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let home = configured.or_else(|| default_home(configuration.family_id.as_str()))?;
    Some(std::fs::canonicalize(&home).unwrap_or(home))
}

fn default_home(family_id: &str) -> Option<PathBuf> {
    let default = DEFAULT_HOMES
        .iter()
        .find(|entry| entry.family_id == family_id)?;
    if let Some(value) = std::env::var_os(default.variable).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(value));
    }
    let user_home = user_home()?;
    Some(
        default
            .folder
            .iter()
            .fold(user_home, |path, part| path.join(part)),
    )
}

fn user_home() -> Option<PathBuf> {
    #[cfg(windows)]
    let names = ["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    let names = ["HOME", "USERPROFILE"];
    names
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configuration(family: &str, home: Option<&str>) -> ProviderInstanceConfig {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "instanceId": "instance",
            "familyId": family,
            "label": "Provider",
            "enabled": true,
            "executable": "",
            "providerHome": home,
            "launchArguments": [],
            "environment": {},
            "credentialReferences": {},
            "providerConfig": { "schemaVersion": 1, "value": {} }
        }))
        .unwrap()
    }

    #[test]
    fn a_configured_home_resolves_to_its_canonical_folder() {
        let folder =
            std::env::temp_dir().join(format!("ganbaru-home-identity-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("nested")).unwrap();
        let spelled = folder.join("nested").join("..");
        let identity = provider_home_identity(&configuration("claude", spelled.to_str()));
        let expected = std::fs::canonicalize(&folder).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(identity, Some(expected));
    }

    #[test]
    fn an_empty_home_uses_the_family_default_and_unknown_families_have_none() {
        assert_eq!(
            provider_home_identity(&configuration("codex", Some("  "))),
            provider_home_identity(&configuration("codex", None)),
        );
        assert!(provider_home_identity(&configuration("codex", None)).is_some());
        assert_eq!(provider_home_identity(&configuration("cursor", None)), None);
    }
}
