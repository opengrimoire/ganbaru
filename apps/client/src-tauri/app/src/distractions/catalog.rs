use super::*;

fn candidate(
    name: impl Into<String>,
    source: &'static str,
    detail: Option<String>,
    process_names: Vec<String>,
) -> DistractionsDesktopAppCandidate {
    let name = name.into();
    DistractionsDesktopAppCandidate {
        process_names: normalize_process_match_names(&name, process_names),
        name,
        source: source.to_string(),
        detail,
    }
}

pub(super) fn sort_and_deduplicate_candidates(
    candidates: Vec<DistractionsDesktopAppCandidate>,
) -> Vec<DistractionsDesktopAppCandidate> {
    let mut by_name = HashMap::<String, DistractionsDesktopAppCandidate>::new();
    for app in candidates {
        if is_protected_desktop_app_candidate(&app) {
            continue;
        }
        if let Some(name) = normalize_app_candidate_name(&app.name) {
            let key = app_name_key(&name);
            by_name
                .entry(key)
                .or_insert_with(|| DistractionsDesktopAppCandidate {
                    process_names: normalize_process_match_names(&name, app.process_names),
                    name,
                    source: app.source,
                    detail: app.detail,
                });
        }
    }
    let mut apps = by_name.into_values().collect::<Vec<_>>();
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    apps
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod unsupported;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux::list_installed_desktop_apps;
#[cfg(target_os = "macos")]
use macos::list_installed_desktop_apps;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
use unsupported::list_installed_desktop_apps;
#[cfg(windows)]
use windows::list_installed_desktop_apps;

#[cfg(all(test, target_os = "linux"))]
pub(super) use linux::{parse_desktop_entry, process_name_from_exec};

#[tauri::command]
pub async fn distractions_list_desktop_apps() -> Result<Vec<DistractionsDesktopAppCandidate>, String>
{
    let generation = DESKTOP_APP_LIST_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let apps = sort_and_deduplicate_candidates(list_installed_desktop_apps());
        if DESKTOP_APP_LIST_GENERATION.load(Ordering::Acquire) != generation {
            Vec::new()
        } else {
            apps
        }
    })
    .await
    .map_err(|error| format!("desktop app worker failed: {error}"))
}
