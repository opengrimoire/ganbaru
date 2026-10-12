//! Desktop application name normalization, protected-application checks, and foreground and
//! process rule matching.

use crate::contracts::{
    DesktopRuleMatcher, DistractionsDesktopAppCandidate, DistractionsDesktopAppRuleInput,
    DistractionsDesktopRuleIdentity, DistractionsForegroundDesktopAppExpectation,
    DistractionsForegroundDesktopAppStatus,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const PROTECTED_DESKTOP_APP_NAMES: &[&str] = &[
    "Ganbaru AI",
    "ganbaru-ai",
    "ganbaru-ai-dev",
    "Ganbaru AI Dev",
    "Ganbaru AI (dev)",
    "org.opengrimoire.ganbaruai",
    "org.opengrimoire.ganbaruai.dev",
    "Activity Monitor",
    "Advanced Network Configuration",
    "Calculator",
    "Characters",
    "Clocks",
    "Command Prompt",
    "Console",
    "Control Panel",
    "Disk Utility",
    "Disk Usage Analyzer",
    "Disks",
    "Event Viewer",
    "Extension Manager",
    "Extensions",
    "File Explorer",
    "Files",
    "Finder",
    "Fonts",
    "GDebi Package Installer",
    "GNOME System Monitor",
    "Help",
    "Htop",
    "IBus Preferences",
    "Input Method",
    "Keychain Access",
    "Language Support",
    "Logs",
    "Notepad",
    "Passwords and Keys",
    "Power Statistics",
    "PowerShell",
    "Settings",
    "Startup Applications",
    "System Monitor",
    "System Preferences",
    "System Settings",
    "Task Manager",
    "Terminal",
    "Text Editor",
    "UXTerm",
    "Windows Explorer",
    "Windows PowerShell",
    "XTerm",
];
const PROTECTED_DESKTOP_PROCESS_NAMES: &[&str] = &[
    "Activity Monitor",
    "bash",
    "cmd",
    "cmd.exe",
    "conhost.exe",
    "ControlCenter",
    "csrss.exe",
    "dash",
    "dbus-broker",
    "dbus-daemon",
    "dllhost.exe",
    "Dock",
    "dwm.exe",
    "electron",
    "explorer.exe",
    "Finder",
    "fish",
    "flatpak",
    "gnome-control-center",
    "gnome-keyring-daemon",
    "gnome-shell",
    "gnome-terminal",
    "gnome-terminal-server",
    "ibus-daemon",
    "java",
    "javaw",
    "javaw.exe",
    "kitty",
    "konsole",
    "kwin_wayland",
    "kwin_x11",
    "launchd",
    "loginwindow",
    "mmc.exe",
    "mutter",
    "node",
    "plasmashell",
    "PowerShell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
    "python",
    "python3",
    "python3.11",
    "python3.12",
    "pythonw.exe",
    "regedit.exe",
    "rundll32.exe",
    "services.exe",
    "sh",
    "ShellExperienceHost.exe",
    "sihost.exe",
    "snap",
    "StartMenuExperienceHost.exe",
    "svchost.exe",
    "System Settings",
    "SystemUIServer",
    "taskmgr.exe",
    "Terminal",
    "wezterm",
    "winlogon.exe",
    "WindowsTerminal.exe",
    "WindowServer",
    "wt.exe",
    "wscript.exe",
    "xdg-desktop-portal",
    "xdg-desktop-portal-gnome",
    "xdg-desktop-portal-gtk",
    "Xorg",
    "XTerm",
    "Xwayland",
    "zsh",
];

pub fn app_name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

pub fn is_protected_desktop_app_name(name: &str) -> bool {
    let key = app_name_key(name);
    PROTECTED_DESKTOP_APP_NAMES
        .iter()
        .chain(PROTECTED_DESKTOP_PROCESS_NAMES.iter())
        .any(|protected_name| app_name_key(protected_name) == key)
}

pub fn is_protected_desktop_app_candidate(app: &DistractionsDesktopAppCandidate) -> bool {
    is_protected_desktop_app_name(&app.name)
        || app
            .process_names
            .iter()
            .any(|name| is_protected_desktop_app_name(name))
}

pub fn normalize_app_candidate_name(name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().any(char::is_control) {
        return None;
    }
    Some(name.chars().take(80).collect())
}

pub fn normalize_process_match_name(name: &str) -> Option<String> {
    let trimmed = name.trim().trim_matches('"').trim_matches('\'');
    let basename = Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(trimmed);
    normalize_app_candidate_name(basename)
}

pub fn normalize_process_match_name_aliases(name: &str) -> Vec<String> {
    let Some(primary) = normalize_process_match_name(name) else {
        return Vec::new();
    };
    let mut aliases = vec![primary.clone()];
    let lower = primary.to_lowercase();
    for suffix in [".exe", ".desktop"] {
        if lower.ends_with(suffix) {
            if let Some(alias) =
                normalize_app_candidate_name(&primary[..primary.len() - suffix.len()])
            {
                aliases.push(alias);
            }
        }
    }
    aliases
}

pub fn normalize_process_match_names(name: &str, process_names: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for candidate in std::iter::once(name.to_string()).chain(process_names) {
        for process_name in normalize_process_match_name_aliases(&candidate) {
            let key = app_name_key(&process_name);
            if is_protected_desktop_app_name(&process_name) || seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            normalized.push(process_name);
        }
    }
    normalized
}

pub fn unavailable_foreground_desktop_app_status(
    reason: impl Into<String>,
) -> DistractionsForegroundDesktopAppStatus {
    DistractionsForegroundDesktopAppStatus {
        available: false,
        app_name: None,
        process_name: None,
        process_id: None,
        process_identity: None,
        match_names: Vec::new(),
        reason: Some(reason.into()),
    }
}

pub fn foreground_status_from_parts(
    app_name: impl Into<String>,
    process_name: Option<String>,
    process_id: Option<u32>,
    match_names: Vec<String>,
) -> DistractionsForegroundDesktopAppStatus {
    let raw_app_name = app_name.into();
    let app_name = normalize_app_candidate_name(&raw_app_name);
    let process_name = process_name.and_then(|name| {
        normalize_process_match_name_aliases(&name)
            .into_iter()
            .next()
    });
    let Some(app_name) = app_name else {
        return unavailable_foreground_desktop_app_status("foreground app name is unavailable");
    };
    let mut raw_match_names = match_names;
    if let Some(process_name) = &process_name {
        raw_match_names.push(process_name.clone());
    }
    DistractionsForegroundDesktopAppStatus {
        match_names: observed_match_names(&app_name, raw_match_names),
        available: true,
        app_name: Some(app_name),
        process_name,
        process_id,
        process_identity: None,
        reason: None,
    }
}

/// Retain protected aliases in observations so the close boundary can reject the whole identity.
fn observed_match_names(name: &str, names: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    std::iter::once(name.to_owned())
        .chain(names)
        .flat_map(|name| normalize_process_match_name_aliases(&name))
        .filter(|name| seen.insert(app_name_key(name)))
        .collect()
}

pub fn foreground_status_match_names(
    status: &DistractionsForegroundDesktopAppStatus,
) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(app_name) = &status.app_name {
        names.push(app_name.clone());
    }
    if let Some(process_name) = &status.process_name {
        names.push(process_name.clone());
    }
    names.extend(status.match_names.iter().cloned());
    observed_match_names(status.app_name.as_deref().unwrap_or(""), names)
}

pub fn foreground_expectation_matches(
    status: &DistractionsForegroundDesktopAppStatus,
    expected: &DistractionsForegroundDesktopAppExpectation,
) -> bool {
    if !status.available {
        return false;
    }
    if status.process_id != expected.process_id
        || status.process_identity != expected.process_identity
    {
        return false;
    }
    #[cfg(target_os = "linux")]
    if status.process_id.is_some() && status.process_identity.is_none() {
        return false;
    }
    if status.app_name.as_ref().map(|name| app_name_key(name))
        != expected.app_name.as_ref().map(|name| app_name_key(name))
        || status.process_name.as_ref().map(|name| app_name_key(name))
            != expected
                .process_name
                .as_ref()
                .map(|name| app_name_key(name))
    {
        return false;
    }
    let status_names = foreground_status_match_names(status)
        .into_iter()
        .map(|name| app_name_key(&name))
        .collect::<HashSet<_>>();
    let mut expected_names = Vec::new();
    if let Some(app_name) = &expected.app_name {
        expected_names.push(app_name.clone());
    }
    if let Some(process_name) = &expected.process_name {
        expected_names.push(process_name.clone());
    }
    expected_names.extend(expected.match_names.iter().cloned());
    normalize_process_match_names(expected.app_name.as_deref().unwrap_or(""), expected_names)
        .into_iter()
        .any(|name| status_names.contains(&app_name_key(&name)))
}

pub fn validate_foreground_status_is_closeable(
    status: &DistractionsForegroundDesktopAppStatus,
) -> Result<(), String> {
    let names = foreground_status_match_names(status);
    if names.is_empty() {
        return Err("foreground app is unavailable".to_string());
    }
    if status
        .app_name
        .as_deref()
        .is_some_and(is_protected_desktop_app_name)
    {
        return Err("refusing to close protected foreground app".to_string());
    }
    if names.iter().any(|name| is_protected_desktop_app_name(name)) {
        return Err("refusing to close protected foreground app".to_string());
    }
    Ok(())
}

pub fn desktop_rule_matchers(
    apps: Vec<DistractionsDesktopAppRuleInput>,
) -> HashMap<String, DesktopRuleMatcher> {
    let mut matchers = HashMap::new();
    for app in apps {
        let Some(app_name) = normalize_app_candidate_name(&app.name) else {
            continue;
        };
        if is_protected_desktop_app_name(&app_name) {
            continue;
        }
        let rule_identity = match &app.rule_identity {
            DistractionsDesktopRuleIdentity::DesktopApp { rule_id } => {
                let Some(rule_id) = normalize_app_candidate_name(rule_id) else {
                    continue;
                };
                if app_name_key(&rule_id) != app_name_key(&app_name) {
                    continue;
                }
                DistractionsDesktopRuleIdentity::DesktopApp { rule_id }
            }
            DistractionsDesktopRuleIdentity::UsageLimit { rule_id, entry_id }
                if !rule_id.trim().is_empty() && !entry_id.trim().is_empty() =>
            {
                DistractionsDesktopRuleIdentity::UsageLimit {
                    rule_id: rule_id.clone(),
                    entry_id: entry_id.clone(),
                }
            }
            DistractionsDesktopRuleIdentity::UsageLimit { .. } => continue,
        };
        for match_name in normalize_process_match_names(&app_name, app.match_names) {
            matchers
                .entry(app_name_key(&match_name))
                .or_insert_with(|| DesktopRuleMatcher {
                    app_name: app_name.clone(),
                    rule_identity: rule_identity.clone(),
                });
        }
    }
    matchers
}

#[cfg(test)]
mod tests;
