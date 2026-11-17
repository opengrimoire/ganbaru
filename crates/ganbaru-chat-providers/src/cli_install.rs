//! The installers each provider documents for its CLI.
//!
//! Commands are the providers' own, copied from their installation pages, and are shown for
//! the person to run in a terminal, with the command each CLI documents for signing in to the
//! provider account; Ganbaru never runs an installer or a sign-in itself. Each command
//! installs a release inside the family's supported range, so OpenCode names its version 1
//! installer and package rather than the version 2 line Ganbaru has no runtime for.

use crate::models::{ProviderInstallGuide, ProviderInstallShell};

/// One provider's installers by platform, with the page that explains them.
struct InstallDefinition {
    family_id: &'static str,
    /// The command for Linux and macOS shells.
    unix: &'static str,
    /// The command for PowerShell on Windows.
    windows: &'static str,
    /// The command that signs in to the provider account, the same on every platform.
    sign_in: &'static str,
    docs_url: &'static str,
}

const INSTALLERS: &[InstallDefinition] = &[
    InstallDefinition {
        family_id: "claude",
        unix: "curl -fsSL https://claude.ai/install.sh | bash",
        windows: "irm https://claude.ai/install.ps1 | iex",
        sign_in: "claude auth login",
        docs_url: "https://code.claude.com/docs/en/setup",
    },
    InstallDefinition {
        family_id: "codex",
        unix: "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
        windows: "irm https://chatgpt.com/codex/install.ps1 | iex",
        sign_in: "codex login",
        docs_url: "https://developers.openai.com/codex/cli",
    },
    InstallDefinition {
        family_id: "cursor",
        unix: "curl https://cursor.com/install -fsS | bash",
        windows: "irm 'https://cursor.com/install?win32=true' | iex",
        sign_in: "cursor-agent login",
        docs_url: "https://cursor.com/docs/cli/installation",
    },
    InstallDefinition {
        family_id: "grok",
        unix: "curl -fsSL https://x.ai/cli/install.sh | bash",
        windows: "irm https://x.ai/cli/install.ps1 | iex",
        sign_in: "grok login",
        docs_url: "https://docs.x.ai/build/overview",
    },
    InstallDefinition {
        family_id: "opencode",
        unix: "curl -fsSL https://opencode.ai/install | bash",
        // OpenCode documents no PowerShell installer; its version 1 npm package installs
        // on Windows wherever Node.js is present.
        windows: "npm install -g opencode-ai@1",
        sign_in: "opencode auth login",
        docs_url: "https://opencode.ai/docs/",
    },
];

/// How to install `family_id`'s CLI on this platform, or `None` for a family without a
/// documented installer, such as a generic ACP agent.
pub(crate) fn install_guide(family_id: &str) -> Option<ProviderInstallGuide> {
    install_guide_for(family_id, cfg!(windows))
}

fn install_guide_for(family_id: &str, windows: bool) -> Option<ProviderInstallGuide> {
    let definition = INSTALLERS
        .iter()
        .find(|definition| definition.family_id == family_id)?;
    let (shell, command) = if windows {
        (ProviderInstallShell::PowerShell, definition.windows)
    } else {
        (ProviderInstallShell::Terminal, definition.unix)
    };
    Some(ProviderInstallGuide {
        shell,
        command: command.to_string(),
        sign_in_command: definition.sign_in.to_string(),
        docs_url: definition.docs_url.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_available_family_names_an_https_installer_and_page() {
        for family in ["claude", "codex", "cursor", "grok", "opencode"] {
            for windows in [false, true] {
                let guide = install_guide_for(family, windows).expect(family);
                assert!(guide.docs_url.starts_with("https://"), "{family}");
                assert!(guide.command.contains("https://") || windows, "{family}");
            }
        }
    }

    #[test]
    fn the_platform_picks_the_shell_of_the_command() {
        let unix = install_guide_for("claude", false).unwrap();
        let windows = install_guide_for("claude", true).unwrap();
        assert_eq!(unix.shell, ProviderInstallShell::Terminal);
        assert_eq!(windows.shell, ProviderInstallShell::PowerShell);
        assert!(unix.command.starts_with("curl "));
        assert!(windows.command.starts_with("irm "));
        assert_eq!(unix.docs_url, windows.docs_url);
    }

    #[test]
    fn every_sign_in_command_runs_the_family_default_executable() {
        for definition in INSTALLERS {
            let metadata = crate::registry::PROVIDER_METADATA
                .iter()
                .find(|metadata| metadata.family_id == definition.family_id)
                .expect(definition.family_id);
            let program = definition.sign_in.split_whitespace().next();
            assert_eq!(
                program,
                metadata.default_executable_candidates.first().copied(),
                "{}",
                definition.family_id
            );
        }
    }

    #[test]
    fn opencode_installs_the_supported_major_line() {
        let unix = install_guide_for("opencode", false).unwrap();
        let windows = install_guide_for("opencode", true).unwrap();
        assert!(!unix.command.contains("/v2/"));
        assert!(windows.command.ends_with("opencode-ai@1"));
    }

    #[test]
    fn the_desktop_opener_allows_every_install_page() {
        let capability = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/client/src-tauri/capabilities/desktop.json");
        let capability = std::fs::read_to_string(capability).expect("desktop capability");
        for definition in INSTALLERS {
            let entry = format!("\"url\": \"{}\"", definition.docs_url);
            assert!(capability.contains(&entry), "{}", definition.docs_url);
        }
    }

    #[test]
    fn a_family_without_an_installer_gets_none() {
        assert_eq!(install_guide_for("acp", false), None);
        assert_eq!(install_guide_for("unknown", true), None);
    }
}
