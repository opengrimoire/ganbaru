//! The installers each provider documents for its CLI.
//!
//! Commands are the providers' own, copied from their installation pages, and are shown for
//! the person to run in a terminal, with the command each CLI documents for signing in to the
//! provider account; Ganbaru never runs an installer or a sign-in itself. The sign-in shown
//! right after an install names the program by its folder when the installer leaves that
//! folder off `PATH`, and on Windows it calls npm and its shims through their `.cmd` files,
//! which the default PowerShell execution policy allows. Each command
//! installs a release inside the family's supported range, so OpenCode names its version 1
//! installer and package rather than the version 2 line Ganbaru has no runtime for. Each
//! definition also names the folders its installer puts the program in, which discovery
//! searches because a running app keeps the `PATH` it started with.

use crate::models::{ProviderInstallGuide, ProviderInstallRequirement, ProviderInstallShell};
use std::path::{Path, PathBuf};

/// One provider's installers by platform, with the page that explains them.
struct InstallDefinition {
    family_id: &'static str,
    /// The command for Linux and macOS shells.
    unix: &'static str,
    /// The command for PowerShell on Windows.
    windows: &'static str,
    /// The command that signs in to the provider account, the same on every platform.
    sign_in: &'static str,
    /// The sign-in command for a new Linux or macOS terminal right after the installer ran.
    unix_sign_in_after_install: &'static str,
    /// The sign-in command for a new PowerShell window right after the installer ran.
    windows_sign_in_after_install: &'static str,
    docs_url: &'static str,
    /// A tool the Windows installer needs before it can run.
    windows_requirement: Option<&'static Requirement>,
    /// Where the Linux and macOS installers put the program, as folder names under the home folder.
    unix_directories: &'static [&'static [&'static str]],
    /// Where the Windows installer puts the program, as folder names under a per-user folder.
    windows_directories: &'static [(WindowsRoot, &'static [&'static str])],
}

/// A tool an installer needs, with the command that installs it.
struct Requirement {
    name: &'static str,
    /// The program discovery looks for.
    program: &'static str,
    command: &'static str,
}

/// Node.js through the Windows package manager, which puts `node` and the folder npm installs
/// programs in on `PATH` for new shells.
const NODE_JS: Requirement = Requirement {
    name: "Node.js",
    program: "node",
    command: "winget install OpenJS.NodeJS.LTS",
};

/// The per-user Windows folder an installer writes under.
#[derive(Clone, Copy)]
enum WindowsRoot {
    /// `%USERPROFILE%`.
    Home,
    /// `%LOCALAPPDATA%`.
    LocalAppData,
    /// `%APPDATA%`.
    RoamingAppData,
}

const INSTALLERS: &[InstallDefinition] = &[
    InstallDefinition {
        family_id: "claude",
        unix: "curl -fsSL https://claude.ai/install.sh | bash",
        windows: "irm https://claude.ai/install.ps1 | iex",
        sign_in: "claude auth login",
        // The installer leaves its folder off `PATH` on every platform and only prints how to add it.
        unix_sign_in_after_install: "~/.local/bin/claude auth login",
        windows_sign_in_after_install: r#"& "$HOME\.local\bin\claude.exe" auth login"#,
        docs_url: "https://code.claude.com/docs/en/setup",
        windows_requirement: None,
        unix_directories: &[&[".local", "bin"]],
        windows_directories: &[(WindowsRoot::Home, &[".local", "bin"])],
    },
    InstallDefinition {
        family_id: "codex",
        unix: "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
        windows: "irm https://chatgpt.com/codex/install.ps1 | iex",
        sign_in: "codex login",
        unix_sign_in_after_install: "codex login",
        windows_sign_in_after_install: "codex login",
        docs_url: "https://developers.openai.com/codex/cli",
        windows_requirement: None,
        unix_directories: &[&[".local", "bin"]],
        windows_directories: &[(
            WindowsRoot::LocalAppData,
            &["Programs", "OpenAI", "Codex", "bin"],
        )],
    },
    InstallDefinition {
        family_id: "cursor",
        unix: "curl https://cursor.com/install -fsS | bash",
        windows: "irm 'https://cursor.com/install?win32=true' | iex",
        sign_in: "cursor-agent login",
        // The Linux and macOS installer only prints how to add its folder to `PATH`.
        unix_sign_in_after_install: "~/.local/bin/cursor-agent login",
        windows_sign_in_after_install: "cursor-agent login",
        docs_url: "https://cursor.com/docs/cli/installation",
        windows_requirement: None,
        unix_directories: &[&[".local", "bin"]],
        windows_directories: &[(WindowsRoot::LocalAppData, &["cursor-agent"])],
    },
    InstallDefinition {
        family_id: "grok",
        unix: "curl -fsSL https://x.ai/cli/install.sh | bash",
        windows: "irm https://x.ai/cli/install.ps1 | iex",
        sign_in: "grok login",
        unix_sign_in_after_install: "grok login",
        windows_sign_in_after_install: "grok login",
        docs_url: "https://docs.x.ai/build/overview",
        windows_requirement: None,
        // The program lives in `.grok/bin`, linked from `.local/bin` only when that is on `PATH`.
        unix_directories: &[&[".grok", "bin"], &[".local", "bin"]],
        windows_directories: &[(WindowsRoot::Home, &[".grok", "bin"])],
    },
    InstallDefinition {
        family_id: "opencode",
        unix: "curl -fsSL https://opencode.ai/install | bash",
        // OpenCode documents no PowerShell installer; its version 1 npm package installs
        // on Windows wherever Node.js is present.
        // `npm.cmd` and `opencode.cmd` skip the `.ps1` shims that the default PowerShell
        // execution policy refuses to run.
        windows: "npm.cmd install -g opencode-ai@1",
        sign_in: "opencode auth login",
        unix_sign_in_after_install: "opencode auth login",
        windows_sign_in_after_install: "opencode.cmd auth login",
        docs_url: "https://opencode.ai/docs/",
        windows_requirement: Some(&NODE_JS),
        unix_directories: &[&[".opencode", "bin"]],
        windows_directories: &[(WindowsRoot::RoamingAppData, &["npm"])],
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
    let (shell, command, sign_in_after_install) = if windows {
        (
            ProviderInstallShell::PowerShell,
            definition.windows,
            definition.windows_sign_in_after_install,
        )
    } else {
        (
            ProviderInstallShell::Terminal,
            definition.unix,
            definition.unix_sign_in_after_install,
        )
    };
    Some(ProviderInstallGuide {
        shell,
        command: command.to_string(),
        sign_in_command: definition.sign_in.to_string(),
        sign_in_after_install_command: sign_in_after_install.to_string(),
        docs_url: definition.docs_url.to_string(),
        requirement: windows
            .then_some(definition.windows_requirement)
            .flatten()
            .map(|requirement| ProviderInstallRequirement {
                name: requirement.name.to_string(),
                program: requirement.program.to_string(),
                command: requirement.command.to_string(),
            }),
    })
}

/// The per-user folders the installers write under, which the caller reads from its environment.
pub struct InstallRoots<'a> {
    /// The home folder: `$HOME`, or `%USERPROFILE%` on Windows.
    pub home: &'a Path,
    /// `%LOCALAPPDATA%` on Windows.
    pub local_app_data: Option<&'a Path>,
    /// `%APPDATA%` on Windows.
    pub roaming_app_data: Option<&'a Path>,
}

/// The folders the documented installers put provider programs in on this platform, without
/// duplicates. A running app keeps the `PATH` it started with, so searching these folders finds
/// a program installed afterwards, even by an installer that only adds its folder to `PATH` for
/// new shells.
pub fn installer_executable_directories(roots: &InstallRoots<'_>) -> Vec<PathBuf> {
    installer_executable_directories_for(roots, cfg!(windows))
}

fn installer_executable_directories_for(roots: &InstallRoots<'_>, windows: bool) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut push = |base: &Path, names: &[&str]| {
        let directory = names
            .iter()
            .fold(base.to_path_buf(), |path, name| path.join(name));
        if !directories.contains(&directory) {
            directories.push(directory);
        }
    };
    for definition in INSTALLERS {
        if windows {
            for (root, names) in definition.windows_directories {
                let base = match root {
                    WindowsRoot::Home => Some(roots.home),
                    WindowsRoot::LocalAppData => roots.local_app_data,
                    WindowsRoot::RoamingAppData => roots.roaming_app_data,
                };
                if let Some(base) = base {
                    push(base, names);
                }
            }
        } else {
            for names in definition.unix_directories {
                push(roots.home, names);
            }
        }
    }
    directories
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
    fn every_sign_in_after_install_runs_the_family_program_with_the_same_arguments() {
        for definition in INSTALLERS {
            let (_, arguments) = definition
                .sign_in
                .split_once(' ')
                .expect(definition.family_id);
            let program = definition.sign_in.split_whitespace().next().unwrap();
            for command in [
                definition.unix_sign_in_after_install,
                definition.windows_sign_in_after_install,
            ] {
                let (invocation, rest) = command
                    .rsplit_once(&format!(" {arguments}"))
                    .expect(command);
                assert!(rest.is_empty(), "{command}");
                let name = invocation
                    .trim_start_matches("& ")
                    .trim_matches('"')
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap();
                let name = name
                    .strip_suffix(".exe")
                    .or_else(|| name.strip_suffix(".cmd"))
                    .unwrap_or(name);
                assert_eq!(name, program, "{command}");
            }
        }
    }

    #[test]
    fn opencode_on_windows_avoids_the_powershell_script_shims() {
        let windows = install_guide_for("opencode", true).unwrap();
        assert!(windows.command.starts_with("npm.cmd "));
        assert!(
            windows
                .sign_in_after_install_command
                .starts_with("opencode.cmd ")
        );
    }

    #[test]
    fn only_the_windows_npm_installer_needs_node() {
        let node = install_guide_for("opencode", true)
            .unwrap()
            .requirement
            .unwrap();
        assert_eq!(node.program, "node");
        assert!(node.command.starts_with("winget install "));
        assert_eq!(
            install_guide_for("opencode", false).unwrap().requirement,
            None
        );
        for family in ["claude", "codex", "cursor", "grok"] {
            for windows in [false, true] {
                assert_eq!(
                    install_guide_for(family, windows).unwrap().requirement,
                    None
                );
            }
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
    fn installer_folders_cover_every_family_on_every_platform() {
        for definition in INSTALLERS {
            assert!(
                !definition.unix_directories.is_empty(),
                "{}",
                definition.family_id
            );
            assert!(
                !definition.windows_directories.is_empty(),
                "{}",
                definition.family_id
            );
        }
    }

    #[test]
    fn unix_installer_folders_sit_under_home_once_each() {
        let home = Path::new("/home/person");
        let roots = InstallRoots {
            home,
            local_app_data: None,
            roaming_app_data: None,
        };
        let directories = installer_executable_directories_for(&roots, false);
        assert_eq!(
            directories,
            [".local/bin", ".grok/bin", ".opencode/bin"]
                .map(|relative| home.join(relative))
                .to_vec()
        );
    }

    #[test]
    fn windows_installer_folders_skip_an_unknown_app_data_folder() {
        let home = Path::new("/users/person");
        let local = Path::new("/users/person/local");
        let without = InstallRoots {
            home,
            local_app_data: None,
            roaming_app_data: None,
        };
        let with = InstallRoots {
            home,
            local_app_data: Some(local),
            roaming_app_data: None,
        };
        let codex = local
            .join("Programs")
            .join("OpenAI")
            .join("Codex")
            .join("bin");
        assert!(!installer_executable_directories_for(&without, true).contains(&codex));
        let directories = installer_executable_directories_for(&with, true);
        assert!(directories.contains(&codex));
        assert!(directories.contains(&local.join("cursor-agent")));
        assert!(directories.contains(&home.join(".grok").join("bin")));
        assert!(
            !directories
                .iter()
                .any(|directory| directory.ends_with("npm"))
        );
    }

    #[test]
    fn a_family_without_an_installer_gets_none() {
        assert_eq!(install_guide_for("acp", false), None);
        assert_eq!(install_guide_for("unknown", true), None);
    }
}
