use super::*;

#[cfg(windows)]
fn collect_windows_shortcuts(
    dir: &Path,
    depth: usize,
    apps: &mut Vec<DistractionsDesktopAppCandidate>,
) {
    if depth > 4 || apps.len() >= 2_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_windows_shortcuts(&path, depth + 1, apps);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("lnk") || extension.eq_ignore_ascii_case("appref-ms")
            })
        {
            if let Some(name) = path.file_stem().and_then(|name| name.to_str()) {
                apps.push(candidate(
                    name.to_string(),
                    "Installed app",
                    Some(path.to_string_lossy().to_string()),
                    vec![name.to_string()],
                ));
            }
        }
        if apps.len() >= 2_000 {
            return;
        }
    }
}

#[cfg(windows)]
pub(super) fn list_installed_desktop_apps() -> Vec<DistractionsDesktopAppCandidate> {
    let mut apps = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        collect_windows_shortcuts(
            &appdata.join("Microsoft\\Windows\\Start Menu\\Programs"),
            0,
            &mut apps,
        );
    }
    if let Some(programdata) = std::env::var_os("PROGRAMDATA").map(PathBuf::from) {
        collect_windows_shortcuts(
            &programdata.join("Microsoft\\Windows\\Start Menu\\Programs"),
            0,
            &mut apps,
        );
    }
    apps
}
