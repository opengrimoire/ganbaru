use super::*;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

/// How long a scan of installed applications vouches for rule match names before it is rebuilt.
const DESKTOP_CATALOG_INDEX_MAX_AGE: Duration = Duration::from_secs(5 * 60);

/// Process names the installed application catalog lists for each application name.
#[derive(Debug, Default)]
pub(super) struct DesktopCatalogIndex {
    process_names_by_app: HashMap<String, Vec<String>>,
}

impl DesktopCatalogIndex {
    /// Merges candidates that share an application name, such as a packaged and a Flatpak copy.
    pub(super) fn from_candidates(candidates: Vec<DistractionsDesktopAppCandidate>) -> Self {
        let mut process_names_by_app = HashMap::<String, Vec<String>>::new();
        for app in candidates {
            let Some(name) = normalize_app_candidate_name(&app.name) else {
                continue;
            };
            process_names_by_app
                .entry(app_name_key(&name))
                .or_default()
                .extend(app.process_names);
        }
        Self {
            process_names_by_app,
        }
    }

    pub(super) fn process_names(&self, app_name: &str) -> &[String] {
        self.process_names_by_app
            .get(&app_name_key(app_name))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

static DESKTOP_CATALOG_INDEX: Mutex<Option<(Instant, Arc<DesktopCatalogIndex>)>> = Mutex::new(None);

#[cfg(target_os = "linux")]
fn scan_desktop_catalog_index() -> DesktopCatalogIndex {
    DesktopCatalogIndex::from_candidates(linux::list_trusted_desktop_apps())
}

#[cfg(not(target_os = "linux"))]
fn scan_desktop_catalog_index() -> DesktopCatalogIndex {
    DesktopCatalogIndex::from_candidates(list_installed_desktop_apps())
}

/// Returns the cached catalog index, scanning installed applications when it is missing or stale.
/// Scans run on the calling thread, so callers must already be off the async runtime.
pub(super) fn desktop_catalog_index() -> Arc<DesktopCatalogIndex> {
    let mut cached = DESKTOP_CATALOG_INDEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((scanned_at, index)) = cached.as_ref() {
        if scanned_at.elapsed() < DESKTOP_CATALOG_INDEX_MAX_AGE {
            return index.clone();
        }
    }
    let index = Arc::new(scan_desktop_catalog_index());
    *cached = Some((Instant::now(), index.clone()));
    index
}

/// Rescans installed applications so an app the user just picked vouches for its rule at once.
fn refresh_desktop_catalog_index() {
    let index = Arc::new(scan_desktop_catalog_index());
    *DESKTOP_CATALOG_INDEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((Instant::now(), index));
}

#[tauri::command]
pub async fn distractions_list_desktop_apps() -> Result<Vec<DistractionsDesktopAppCandidate>, String>
{
    let generation = DESKTOP_APP_LIST_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        refresh_desktop_catalog_index();
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
