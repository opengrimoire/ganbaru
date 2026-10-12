use super::*;

#[cfg(target_os = "macos")]
fn collect_macos_apps(dir: &Path, depth: usize, apps: &mut Vec<DistractionsDesktopAppCandidate>) {
    if depth > 2 || apps.len() >= 2_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "app") {
            if let Some(name) = path.file_stem().and_then(|name| name.to_str()) {
                apps.push(candidate(
                    name.to_string(),
                    "Installed app",
                    Some(path.to_string_lossy().to_string()),
                    vec![name.to_string()],
                ));
            }
        } else if path.is_dir() {
            collect_macos_apps(&path, depth + 1, apps);
        }
        if apps.len() >= 2_000 {
            return;
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn list_installed_desktop_apps() -> Vec<DistractionsDesktopAppCandidate> {
    let mut apps = Vec::new();
    collect_macos_apps(Path::new("/Applications"), 0, &mut apps);
    collect_macos_apps(Path::new("/System/Applications"), 0, &mut apps);
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        collect_macos_apps(&home.join("Applications"), 0, &mut apps);
    }
    apps
}
