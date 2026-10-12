use super::*;

#[cfg(target_os = "linux")]
fn decode_desktop_entry_value(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }
        match chars.next() {
            Some('s') => decoded.push(' '),
            Some('n') => decoded.push('\n'),
            Some('t') => decoded.push('\t'),
            Some('\\') => decoded.push('\\'),
            Some(other) => decoded.push(other),
            None => decoded.push('\\'),
        }
    }
    decoded
}

#[cfg(target_os = "linux")]
fn first_exec_token(exec: &str) -> Option<String> {
    let mut token = String::new();
    let mut in_quote = false;
    let mut quote_char = '\0';
    let mut escaped = false;
    for ch in exec.trim().chars() {
        if escaped {
            token.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if in_quote {
            if ch == quote_char {
                in_quote = false;
            } else {
                token.push(ch);
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_quote = true;
            quote_char = ch;
            continue;
        }
        if ch.is_whitespace() {
            if token.is_empty() {
                continue;
            }
            break;
        }
        token.push(ch);
    }
    if token.is_empty() { None } else { Some(token) }
}

#[cfg(target_os = "linux")]
pub(in crate::distractions) fn process_name_from_exec(exec: &str) -> Option<String> {
    let token = first_exec_token(exec)?;
    normalize_process_match_name(&token)
}

#[cfg(target_os = "linux")]
fn decode_desktop_entry_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(decode_desktop_entry_value)
        .collect()
}

#[cfg(target_os = "linux")]
fn current_desktop_names() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split([':', ';'])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(target_os = "linux")]
fn linux_entry_matches_current_desktop(only_show_in: &[String], not_show_in: &[String]) -> bool {
    let current_desktops = current_desktop_names();
    if !not_show_in.is_empty()
        && current_desktops.iter().any(|desktop| {
            not_show_in
                .iter()
                .any(|blocked| blocked.eq_ignore_ascii_case(desktop))
        })
    {
        return false;
    }
    only_show_in.is_empty()
        || current_desktops.iter().any(|desktop| {
            only_show_in
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(desktop))
        })
}

#[cfg(target_os = "linux")]
fn linux_desktop_entry_is_system_utility(name: &str, categories: &[String], detail: &str) -> bool {
    let detail_key = app_name_key(detail);
    if detail_key.starts_with("gnome-") && detail_key.contains("-panel.desktop") {
        return true;
    }
    if detail_key.starts_with("xdg-desktop-portal")
        || detail_key.starts_with("gcr-")
        || detail_key.starts_with("nm-")
        || detail_key.starts_with("org.freedesktop.ibus.")
        || detail_key.starts_with("org.gnome.shell.")
        || detail_key.starts_with("org.gnome.settings.")
    {
        return true;
    }
    if is_protected_desktop_app_name(name) {
        return true;
    }

    let has_category = |category: &str| {
        categories
            .iter()
            .any(|item| item.eq_ignore_ascii_case(category))
    };
    let has_any_category = |items: &[&str]| items.iter().any(|item| has_category(item));

    if has_any_category(&[
        "Calculator",
        "Clock",
        "ConsoleOnly",
        "Core",
        "DesktopSettings",
        "Documentation",
        "FileManager",
        "HardwareSettings",
        "Monitor",
        "Security",
        "Settings",
        "TerminalEmulator",
        "X-GNOME-Settings-Panel",
        "X-GNOME-Utilities",
        "X-XFCE-SettingsDialog",
        "X-Unity-Settings-Panel",
    ]) {
        return true;
    }

    has_category("System")
        && !has_any_category(&[
            "AudioVideo",
            "Development",
            "Game",
            "Graphics",
            "Network",
            "Office",
            "Player",
            "Recorder",
            "WebBrowser",
        ])
}

#[cfg(target_os = "linux")]
pub(in crate::distractions) fn parse_desktop_entry(
    contents: &str,
    detail: String,
) -> Option<DistractionsDesktopAppCandidate> {
    let mut in_desktop_entry = false;
    let mut entry_type: Option<String> = None;
    let mut name: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut hidden = false;
    let mut no_display = false;
    let mut categories = Vec::new();
    let mut only_show_in = Vec::new();
    let mut not_show_in = Vec::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "Type" => entry_type = Some(value.trim().to_string()),
            "Name" => name = Some(decode_desktop_entry_value(value.trim())),
            "Exec" => exec = Some(decode_desktop_entry_value(value.trim())),
            "Hidden" => hidden = value.trim().eq_ignore_ascii_case("true"),
            "NoDisplay" => no_display = value.trim().eq_ignore_ascii_case("true"),
            "Categories" => categories = decode_desktop_entry_list(value.trim()),
            "OnlyShowIn" => only_show_in = decode_desktop_entry_list(value.trim()),
            "NotShowIn" => not_show_in = decode_desktop_entry_list(value.trim()),
            _ => {}
        }
    }

    if hidden
        || no_display
        || entry_type
            .as_deref()
            .is_some_and(|value| value != "Application")
        || !linux_entry_matches_current_desktop(&only_show_in, &not_show_in)
    {
        return None;
    }
    let name = normalize_app_candidate_name(name.as_deref()?)?;
    if linux_desktop_entry_is_system_utility(&name, &categories, &detail) {
        return None;
    }
    let mut process_names = exec
        .as_deref()
        .and_then(process_name_from_exec)
        .into_iter()
        .collect::<Vec<_>>();
    process_names.push(detail.clone());
    if let Some(stem) = Path::new(&detail)
        .file_stem()
        .and_then(|name| name.to_str())
    {
        process_names.push(stem.to_string());
    }
    Some(candidate(
        name,
        "Installed app",
        Some(detail),
        process_names,
    ))
}

#[cfg(target_os = "linux")]
fn collect_desktop_entry_files(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth > 3 || files.len() >= 2_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_desktop_entry_files(&path, depth + 1, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "desktop")
        {
            files.push(path);
            if files.len() >= 2_000 {
                return;
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".local/share/applications"));
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    }
    if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        dirs.push(xdg_data_home.join("applications"));
    }
    let xdg_data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    for dir in xdg_data_dirs.split(':').filter(|dir| !dir.is_empty()) {
        dirs.push(PathBuf::from(dir).join("applications"));
    }
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));

    let mut seen = std::collections::HashSet::new();
    dirs.into_iter()
        .filter(|dir| seen.insert(dir.clone()))
        .collect()
}

#[cfg(target_os = "linux")]
pub(super) fn list_installed_desktop_apps() -> Vec<DistractionsDesktopAppCandidate> {
    let mut files = Vec::new();
    for dir in linux_application_dirs() {
        collect_desktop_entry_files(&dir, 0, &mut files);
    }
    let mut candidates = Vec::new();
    for path in files {
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > 128 * 1024 {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let detail = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        if let Some(app) = parse_desktop_entry(&contents, detail) {
            candidates.push(app);
        }
    }
    candidates
}
