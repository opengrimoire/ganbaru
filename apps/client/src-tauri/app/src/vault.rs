//! Vault filesystem layer.
//!
//! The vault holds portable user data. The Tauri app config directory stores
//! only the active vault pointer and device-local runtime files.
//!
//! Writes are atomic: serialize to `.tmp`, fsync, rename. A crash mid-write
//! leaves either the previous good file or the temp file, which is ignored
//! on next read.

use chrono::{DateTime, SecondsFormat, Utc};
#[cfg(target_os = "android")]
use ganbaru_mobile_documents::MobileDocumentsExt;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{Manager, Runtime};
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, FilePath};

static APP_STATE: std::sync::Mutex<Option<CachedAppState>> = std::sync::Mutex::new(None);
static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod config;
#[cfg(desktop)]
pub(crate) use config::read_active_config_bounded;
mod documents;

// Keep Tauri commands and their generated wrappers at the vault facade.
pub use config::*;
pub use documents::*;

pub(crate) mod backup;
pub(crate) mod handoff;
pub(crate) mod ownership;
pub(crate) mod quiescence;
pub(crate) mod runtime_lifecycle;

pub const APP_SQLITE_FILE: &str = "ganbaru-ai.sqlite";
const PRODUCTION_VAULT_NAME: &str = "Ganbaru AI";
const DEVELOPMENT_VAULT_NAME: &str = "Ganbaru AI Dev";
const APP_STATE_FILE: &str = "app-state.json";
const VAULT_MANIFEST_FILE: &str = "vault.json";
const CONFIG_FILE: &str = "config.json";
const VAULT_APP_MARKER: &str = "ganbaru-ai";
const VAULT_SCHEMA_VERSION: u32 = 1;
const MAX_RECENT_VAULTS: usize = 8;
#[cfg(target_os = "android")]
const MOBILE_VAULT_IMPORT_MAX_FILES: u32 = 100_000;
#[cfg(target_os = "android")]
const MOBILE_VAULT_IMPORT_MAX_BYTES: u64 = 100 * 1024 * 1024 * 1024;
#[cfg(target_os = "android")]
const MOBILE_VAULT_IMPORT_MAX_DEPTH: u32 = 64;

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultAppState {
    #[serde(deserialize_with = "required_nullable")]
    pub device_id: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub active_vault_path: Option<String>,
    pub recent_vault_paths: Vec<String>,
    pub music_root_bindings: BTreeMap<String, BTreeMap<String, String>>,
    pub project_working_folders: ganbaru_working_folders::WorkingFolderDeviceState,
    pub chat: crate::chat::device_state::ChatDeviceState,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(deserializer)
}

#[tauri::command]
pub(crate) fn vault_device_id<R: Runtime>(app: tauri::AppHandle<R>) -> Result<String, String> {
    ensure_device_id(&app)
}

pub(crate) fn ensure_device_id<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<String, String> {
    // Most calls find the identity already stored; avoid cloning device state for an update.
    if let Some(device_id) = read_app_state(app)?
        .device_id
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(device_id.clone());
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("create device id timestamp: {error}"))?
        .as_nanos();
    let candidate = format!("device-{timestamp:x}-{:x}", std::process::id());
    update_app_state(app, |state| {
        if let Some(device_id) = state
            .device_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(device_id.clone());
        }
        state.device_id = Some(candidate.clone());
        Ok(candidate)
    })
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultManifest {
    app: String,
    schema_version: u32,
    vault_id: String,
    display_name: String,
    created_at: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub path: String,
    pub config_path: String,
    pub database_path: String,
    pub vault_id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultDefaultLocation {
    pub path: String,
    pub parent_path: String,
    pub folder_name: String,
    pub development_build: bool,
}

fn app_state_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    let mut path = app.path().app_config_dir().map_err(|e| e.to_string())?;
    path.push(APP_STATE_FILE);
    Ok(path)
}

fn read_app_state_from_path(path: &Path) -> Result<VaultAppState, String> {
    if !path.exists() {
        return Ok(VaultAppState::default());
    }
    let contents = fs::read_to_string(path).map_err(|e| format!("read app state: {e}"))?;
    serde_json::from_str(&contents).map_err(|e| format!("parse app state: {e}"))
}

/// File identity used to detect app state changes made outside this process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AppStateStamp {
    len: u64,
    modified: std::time::SystemTime,
}

/// Parsed app state shared by readers until its file changes.
struct CachedAppState {
    path: PathBuf,
    stamp: Option<AppStateStamp>,
    state: Arc<VaultAppState>,
}

fn app_state_stamp(path: &Path) -> Result<Option<AppStateStamp>, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("inspect app state: {error}")),
    };
    let modified = metadata
        .modified()
        .map_err(|error| format!("inspect app state modification time: {error}"))?;
    Ok(Some(AppStateStamp {
        len: metadata.len(),
        modified,
    }))
}

/// Return the cached state, parsing the file only when it is new or changed.
///
/// Background owners resolve the active folder several times per second, so
/// reparsing the whole device state on each lookup is not acceptable.
fn cached_app_state(
    cache: &mut Option<CachedAppState>,
    path: &Path,
) -> Result<Arc<VaultAppState>, String> {
    // Stamp before reading so a concurrent external write is detected on the next lookup.
    let stamp = app_state_stamp(path)?;
    if let Some(cached) = cache
        .as_ref()
        .filter(|cached| cached.path == path && cached.stamp == stamp)
    {
        return Ok(Arc::clone(&cached.state));
    }
    let state = Arc::new(read_app_state_from_path(path)?);
    *cache = Some(CachedAppState {
        path: path.to_path_buf(),
        stamp,
        state: Arc::clone(&state),
    });
    Ok(state)
}

#[cfg(test)]
fn write_app_state_to_path(path: &Path, state: &VaultAppState) -> Result<(), String> {
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    write_app_state_json(path, &json)
}

fn write_app_state_json(path: &Path, json: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create app config dir: {e}"))?;
    }
    write_text_file_atomically(path, json)
}

pub(crate) fn read_app_state<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Arc<VaultAppState>, String> {
    let mut cache = APP_STATE
        .lock()
        .map_err(|_| "app state lock is unavailable".to_string())?;
    cached_app_state(&mut cache, &app_state_path(app)?)
}

pub(crate) fn update_app_state<R: Runtime, T>(
    app: &tauri::AppHandle<R>,
    update: impl FnOnce(&mut VaultAppState) -> Result<T, String>,
) -> Result<T, String> {
    let mut cache = APP_STATE
        .lock()
        .map_err(|_| "app state lock is unavailable".to_string())?;
    let path = app_state_path(app)?;
    let current = cached_app_state(&mut cache, &path)?;
    let mut state = VaultAppState::clone(&current);
    let result = update(&mut state)?;
    // Idempotent updates must not rewrite and fsync the file.
    let stored = cache.as_ref().is_some_and(|cached| cached.stamp.is_some());
    if stored && state == *current {
        return Ok(result);
    }
    let json = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    write_app_state_json(&path, &json)?;
    *cache = match app_state_stamp(&path) {
        Ok(stamp) => Some(CachedAppState {
            path,
            stamp,
            state: Arc::new(state),
        }),
        // The write succeeded; the next lookup reparses the file instead of trusting a stale stamp.
        Err(_) => None,
    };
    Ok(result)
}

fn path_to_string(path: &Path, label: &str) -> Result<String, String> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("{label} path contains non-utf8 characters"))
}

fn display_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(default_vault_name())
        .to_string()
}

fn is_development_build() -> bool {
    cfg!(debug_assertions)
}

fn default_vault_name() -> &'static str {
    if is_development_build() {
        DEVELOPMENT_VAULT_NAME
    } else {
        PRODUCTION_VAULT_NAME
    }
}

fn new_vault_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("vault-{:x}-{:x}", std::process::id(), nanos)
}

fn now_utc() -> DateTime<Utc> {
    std::time::SystemTime::now().into()
}

fn require_absolute_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("Ganbaru AI folder path must be absolute".to_string());
    }
    let metadata =
        fs::metadata(path).map_err(|e| format!("failed to inspect Ganbaru AI folder path: {e}"))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err("Ganbaru AI path must be a folder".to_string())
    }
}

fn canonical_vault_path(path: PathBuf) -> Result<PathBuf, String> {
    require_absolute_directory(&path)?;
    fs::canonicalize(&path).map_err(|e| format!("canonicalize Ganbaru AI folder path: {e}"))
}

fn folder_is_empty(path: &Path) -> Result<bool, String> {
    let mut entries = fs::read_dir(path).map_err(|e| format!("read Ganbaru AI folder: {e}"))?;
    Ok(entries.next().is_none())
}

fn manifest_path(path: &Path) -> PathBuf {
    path.join(VAULT_MANIFEST_FILE)
}

/// Resolve the portable configuration path beneath an already authorized vault root.
pub(crate) fn config_path(path: &Path) -> PathBuf {
    path.join(CONFIG_FILE)
}

fn database_path(path: &Path) -> PathBuf {
    path.join(APP_SQLITE_FILE)
}

fn read_vault_manifest(path: &Path) -> Result<VaultManifest, String> {
    let manifest_path = manifest_path(path);
    let contents = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read Ganbaru AI folder marker: {e}"))?;
    let manifest: VaultManifest = serde_json::from_str(&contents)
        .map_err(|e| format!("parse Ganbaru AI folder marker: {e}"))?;
    if manifest.app != VAULT_APP_MARKER {
        return Err("selected folder is not a Ganbaru AI folder".to_string());
    }
    if manifest.schema_version != VAULT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported Ganbaru AI folder schema version {}",
            manifest.schema_version
        ));
    }
    Ok(manifest)
}

fn vault_info_from_manifest(path: &Path, manifest: VaultManifest) -> Result<VaultInfo, String> {
    Ok(VaultInfo {
        path: path_to_string(path, "Ganbaru AI folder")?,
        config_path: path_to_string(&config_path(path), "config")?,
        database_path: path_to_string(&database_path(path), "database")?,
        vault_id: manifest.vault_id,
        display_name: if manifest.display_name.trim().is_empty() {
            display_name_from_path(path)
        } else {
            manifest.display_name
        },
    })
}

fn vault_info_from_path(path: &Path) -> Result<VaultInfo, String> {
    let path = canonical_vault_path(path.to_path_buf())?;
    vault_info_from_manifest(&path, read_vault_manifest(&path)?)
}

fn ensure_vault_skeleton(path: &Path) -> Result<(), String> {
    for relative in [
        "notes/daily",
        "notes/projects",
        "diary/morning",
        "diary/evening",
        "projects",
        "reports",
        "assets",
        "templates",
        ".yjs",
    ] {
        fs::create_dir_all(path.join(relative))
            .map_err(|e| format!("create Ganbaru AI folder path '{relative}': {e}"))?;
    }
    if !config_path(path).exists() {
        write_text_file_atomically(&config_path(path), "{}\n")?;
    }
    if !database_path(path).exists() {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(database_path(path))
            .map_err(|e| format!("create ganbaru-ai.sqlite: {e}"))?;
    }
    Ok(())
}

fn initialize_vault(path: &Path) -> Result<VaultInfo, String> {
    let path = canonical_vault_path(path.to_path_buf())?;
    if manifest_path(&path).exists() {
        ensure_vault_skeleton(&path)?;
        return vault_info_from_path(&path);
    }
    if !folder_is_empty(&path)? {
        return Err("selected folder is not empty and is not a Ganbaru AI folder".to_string());
    }
    let manifest = VaultManifest {
        app: VAULT_APP_MARKER.to_string(),
        schema_version: VAULT_SCHEMA_VERSION,
        vault_id: new_vault_id(),
        display_name: display_name_from_path(&path),
        created_at: now_utc().to_rfc3339_opts(SecondsFormat::Millis, true),
    };
    ensure_vault_skeleton(&path)?;
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    write_text_file_atomically(&manifest_path(&path), &json)?;
    vault_info_from_manifest(&path, manifest)
}

/// Changes the active folder only after every native owner has drained.
#[cfg(desktop)]
async fn select_vault<R: Runtime>(
    app: &tauri::AppHandle<R>,
    info: &VaultInfo,
) -> Result<(), String> {
    select_reserved_vault(app, info, quiescence::reserve_vault_transition()?).await
}

async fn select_reserved_vault<R: Runtime>(
    app: &tauri::AppHandle<R>,
    info: &VaultInfo,
    transition: quiescence::VaultTransition,
) -> Result<(), String> {
    let quiescence = quiescence::begin_reserved_quiescence(app, transition).await?;
    let selection_app = app.clone();
    let info = info.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let selected = select_quiesced_vault(&selection_app, &info, &quiescence);
        let resumed = quiescence.finish();
        selected.and(resumed)
    })
    .await
    .map_err(|error| format!("vault selection worker failed: {error}"))?
}

/// Persists selection while the caller retains the native and write barriers.
fn select_quiesced_vault<R: Runtime>(
    app: &tauri::AppHandle<R>,
    info: &VaultInfo,
    _quiescence: &quiescence::SnapshotQuiescence,
) -> Result<(), String> {
    update_app_state(app, |state| {
        state.active_vault_path = Some(info.path.clone());
        state
            .recent_vault_paths
            .retain(|path| path != &info.path && !path.trim().is_empty());
        state.recent_vault_paths.insert(0, info.path.clone());
        state.recent_vault_paths.truncate(MAX_RECENT_VAULTS);
        if let Some(mut known) = recent_vault_ids(&state.recent_vault_paths) {
            known.insert(info.vault_id.clone());
            retain_vault_scopes(state, &known);
        }
        Ok(())
    })
}

/// Collect the vault IDs found at recent folders, or `None` when any folder is unreachable.
///
/// A missing folder may be a vault on a disconnected drive, so its ID is unknown and
/// no scope can safely be called stale. A reachable folder without a valid marker is
/// no longer a vault and contributes no ID.
fn recent_vault_ids(paths: &[String]) -> Option<BTreeSet<String>> {
    let mut known = BTreeSet::new();
    for path in paths.iter().map(Path::new) {
        if !path.try_exists().unwrap_or(false) {
            return None;
        }
        if let Ok(manifest) = read_vault_manifest(path) {
            known.insert(manifest.vault_id);
        }
    }
    Some(known)
}

/// Forget device-local settings for vaults that are known to be gone.
///
/// Scopes are keyed by vault ID, so a deleted or replaced vault would otherwise
/// keep its folder bindings and Chat provider settings forever.
fn retain_vault_scopes(state: &mut VaultAppState, known_vault_ids: &BTreeSet<String>) {
    state
        .music_root_bindings
        .retain(|vault_id, _| known_vault_ids.contains(vault_id));
    state
        .project_working_folders
        .vaults
        .retain(|vault_id, _| known_vault_ids.contains(vault_id));
    state
        .chat
        .vaults
        .retain(|vault_id, _| known_vault_ids.contains(vault_id));
}

/// Reconciles native execution owners with the newly active, authorized vault.
pub(crate) fn resume_native_runtimes<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let focus = crate::pomodoro::resume_after_vault_handoff(app);
    let music = crate::music::session::resume_after_vault_handoff(app);
    #[cfg(desktop)]
    let distractions = crate::distractions::runtime::resume_after_vault_handoff(app);
    #[cfg(target_os = "android")]
    let distractions = crate::distractions::android::runtime::resume_after_vault_handoff(app);
    #[cfg(target_os = "ios")]
    let distractions = Ok(());
    let sync = crate::sync::resume_after_vault_handoff(app);
    focus.and(music).and(distractions).and(sync)
}

#[cfg(desktop)]
async fn pick_folder(
    app: &tauri::AppHandle,
    title: &str,
    start_directory: Option<PathBuf>,
) -> Result<Option<PathBuf>, String> {
    let mut picker = app.dialog().file().set_title(title);
    if let Some(directory) = start_directory {
        picker = picker.set_directory(directory);
    }
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    picker.pick_folder(move |path| {
        let result = path.map(dialog_path).transpose();
        let _ = tx.try_send(result);
    });
    rx.recv()
        .await
        .ok_or_else(|| "folder picker closed without returning a result".to_string())?
}

#[cfg(desktop)]
fn existing_documents_directory(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().document_dir().ok().filter(|path| path.is_dir())
}

#[cfg(desktop)]
fn default_data_parent(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .document_dir()
        .map_err(|e| format!("find Documents folder: {e}"))
}

#[cfg(mobile)]
fn default_data_parent(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("find private application data folder: {e}"))
}

fn default_vault_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(default_data_parent(app)?.join(default_vault_name()))
}

fn default_location_from_path(path: PathBuf) -> Result<VaultDefaultLocation, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Ganbaru AI folder has no parent directory".to_string())?;
    Ok(VaultDefaultLocation {
        path: path_to_string(&path, "Ganbaru AI folder")?,
        parent_path: path_to_string(parent, "Ganbaru AI folder parent")?,
        folder_name: default_vault_name().to_string(),
        development_build: is_development_build(),
    })
}

async fn create_and_select_vault(
    app: &tauri::AppHandle,
    path: PathBuf,
) -> Result<VaultInfo, String> {
    let transition = quiescence::reserve_vault_transition()?;
    let creation_transition = transition.clone();
    let info = tauri::async_runtime::spawn_blocking(move || {
        let _transition = creation_transition;
        fs::create_dir_all(&path).map_err(|e| format!("create Ganbaru AI folder: {e}"))?;
        initialize_vault(&path)
    })
    .await
    .map_err(|error| format!("vault creation worker failed: {error}"))??;
    select_reserved_vault(app, &info, transition).await?;
    Ok(info)
}

/// Folder selection fields exposed to the WebView. Device-local Chat, music, and
/// working-folder state stays native.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultAppStateSummary {
    pub active_vault_path: Option<String>,
    pub recent_vault_paths: Vec<String>,
}

#[tauri::command]
pub fn vault_read_app_state(app: tauri::AppHandle) -> Result<VaultAppStateSummary, String> {
    let state = read_app_state(&app)?;
    Ok(VaultAppStateSummary {
        active_vault_path: state.active_vault_path.clone(),
        recent_vault_paths: state.recent_vault_paths.clone(),
    })
}

#[tauri::command]
pub fn vault_default_location(app: tauri::AppHandle) -> Result<VaultDefaultLocation, String> {
    default_location_from_path(default_vault_path(&app)?)
}

#[tauri::command]
pub async fn vault_use_default(app: tauri::AppHandle) -> Result<VaultInfo, String> {
    let path = default_vault_path(&app)?;
    create_and_select_vault(&app, path).await
}

#[tauri::command]
pub fn vault_active_info(app: tauri::AppHandle) -> Result<Option<VaultInfo>, String> {
    let Some(path) = read_app_state(&app)?.active_vault_path.clone() else {
        return Ok(None);
    };
    vault_info_from_path(&PathBuf::from(path)).map(Some)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn vault_pick_create(app: tauri::AppHandle) -> Result<Option<VaultInfo>, String> {
    let Some(path) =
        pick_folder(&app, "Select a folder", existing_documents_directory(&app)).await?
    else {
        return Ok(None);
    };
    let info = create_and_select_vault(&app, path).await?;
    Ok(Some(info))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn vault_pick_open(app: tauri::AppHandle) -> Result<Option<VaultInfo>, String> {
    let Some(path) = pick_folder(
        &app,
        "Import Ganbaru AI folder",
        existing_documents_directory(&app),
    )
    .await?
    else {
        return Ok(None);
    };
    let info = vault_info_from_path(&path)?;
    ensure_vault_skeleton(&PathBuf::from(&info.path))?;
    select_vault(&app, &info).await?;
    Ok(Some(info))
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn vault_pick_open(app: tauri::AppHandle) -> Result<Option<VaultInfo>, String> {
    let transition = quiescence::reserve_vault_transition()?;
    let import_transition = transition.clone();
    let import_app = app.clone();
    let info = tauri::async_runtime::spawn_blocking(move || {
        let _transition = import_transition;
        pick_mobile_vault(&import_app)
    })
    .await
    .map_err(|error| format!("Android vault import worker failed: {error}"))??;
    if let Some(info) = &info {
        select_reserved_vault(&app, info, transition).await?;
    }
    Ok(info)
}

#[cfg(target_os = "android")]
fn pick_mobile_vault(app: &tauri::AppHandle) -> Result<Option<VaultInfo>, String> {
    let target = default_vault_path(app)?;
    let parent = target
        .parent()
        .ok_or_else(|| "Ganbaru AI folder has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create app data directory: {error}"))?;
    let staging = parent.join(format!(".{}.import", default_vault_name()));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("remove stale folder import: {error}"))?;
    }
    if target.exists() {
        if !folder_is_empty(&target)? {
            return Err(
                "the private Ganbaru AI folder already exists; use it or remove its test data first"
                    .to_string(),
            );
        }
        fs::remove_dir(&target)
            .map_err(|error| format!("remove empty Ganbaru AI folder: {error}"))?;
    }

    let staging_path = path_to_string(&staging, "folder import staging")?;
    let selected = app.mobile_documents().pick_vault_tree_to_path(
        &staging_path,
        MOBILE_VAULT_IMPORT_MAX_FILES,
        MOBILE_VAULT_IMPORT_MAX_BYTES,
        MOBILE_VAULT_IMPORT_MAX_DEPTH,
    )?;
    if selected.is_none() {
        return Ok(None);
    }

    let result = (|| {
        let imported = vault_info_from_path(&staging)?;
        ensure_vault_skeleton(Path::new(&imported.path))?;
        fs::rename(&staging, &target)
            .map_err(|error| format!("activate imported Ganbaru AI folder: {error}"))?;
        let info = vault_info_from_path(&target)?;
        Ok(Some(info))
    })();
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

#[cfg(desktop)]
#[tauri::command]
pub fn vault_reveal_active(app: tauri::AppHandle) -> Result<(), String> {
    let path = active_vault_path(&app)?;
    reveal_vault_folder(&path)
}

pub fn active_vault_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    active_vault(app).map(|(path, _)| path)
}

/// Resolve the active folder and its validated marker with one marker read.
fn active_vault<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(PathBuf, VaultManifest), String> {
    let Some(path) = read_app_state(app)?.active_vault_path.clone() else {
        return Err("no active Ganbaru AI folder selected".to_string());
    };
    let path = canonical_vault_path(PathBuf::from(path))?;
    let manifest = read_vault_manifest(&path)?;
    Ok((path, manifest))
}

/// Background observation waits during onboarding or after a selected folder was deleted.
/// Invalid manifests and filesystem access errors remain explicit failures.
#[cfg(desktop)]
pub(crate) fn available_active_vault_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<PathBuf>, String> {
    available_vault_path(
        read_app_state(app)?
            .active_vault_path
            .as_ref()
            .map(PathBuf::from),
    )
}

#[cfg(desktop)]
fn available_vault_path(path: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    let Some(path) = path else { return Ok(None) };
    if !path
        .try_exists()
        .map_err(|error| format!("inspect selected Ganbaru AI folder: {error}"))?
    {
        return Ok(None);
    }
    let path = canonical_vault_path(path)?;
    read_vault_manifest(&path)?;
    Ok(Some(path))
}

pub(crate) struct WritableVaultPath {
    path: PathBuf,
    _permit: ownership::ManagedVaultWritePermit,
}

impl WritableVaultPath {
    pub(crate) fn join(mut self, path: impl AsRef<Path>) -> Self {
        self.path.push(path);
        self
    }
}

impl std::ops::Deref for WritableVaultPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for WritableVaultPath {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

pub(crate) fn active_writable_vault_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<WritableVaultPath, String> {
    let (path, manifest) = active_vault(app)?;
    let vault_id = manifest.vault_id;
    let permit = app
        .state::<ownership::VaultOwnershipManager>()
        .acquire_managed_write(&vault_id)?;
    Ok(WritableVaultPath {
        path,
        _permit: permit,
    })
}

pub(crate) fn active_vault_id<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<String, String> {
    active_vault(app).map(|(_, manifest)| manifest.vault_id)
}

pub fn active_database_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(database_path(&active_vault_path(app)?))
}

fn require_absolute_path(path: &Path) -> Result<(), String> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err("path must be absolute".to_string())
    }
}

#[cfg(desktop)]
fn dialog_path(path: FilePath) -> Result<PathBuf, String> {
    path.into_path()
        .map_err(|e| format!("selected path is not a local file: {e}"))
}

/// Write a UTF-8 text file atomically via `.tmp` plus rename so an
/// interrupted write cannot leave the target file truncated.
fn write_text_file_atomically(path: &Path, contents: &str) -> Result<(), String> {
    require_absolute_path(path)?;
    let target = path;
    let parent = target
        .parent()
        .ok_or_else(|| "target has no parent directory".to_string())?
        .to_path_buf();
    let file_name = target
        .file_name()
        .ok_or_else(|| "target has no file name".to_string())?
        .to_string_lossy()
        .into_owned();
    let temporary_path = parent.join(format!("{file_name}.tmp"));
    {
        let mut file = fs::File::create(&temporary_path).map_err(|e| e.to_string())?;
        file.write_all(contents.as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&temporary_path, target).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[cfg(desktop)]
fn reveal_vault_folder(path: &Path) -> Result<(), String> {
    spawn_file_manager_command("xdg-open", [path.as_os_str()])
}

#[cfg(target_os = "macos")]
#[cfg(desktop)]
fn reveal_vault_folder(path: &Path) -> Result<(), String> {
    spawn_file_manager_command("open", [path.as_os_str()])
}

#[cfg(windows)]
#[cfg(desktop)]
fn reveal_vault_folder(path: &Path) -> Result<(), String> {
    spawn_file_manager_command("explorer.exe", [path.as_os_str()])
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
#[cfg(desktop)]
fn reveal_vault_folder(_path: &Path) -> Result<(), String> {
    Err("opening Ganbaru AI folders is not implemented for this platform".to_string())
}

#[cfg(desktop)]
fn spawn_file_manager_command<I, S>(program: &str, args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("open Ganbaru AI folder: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    pub(super) fn unique_path(suffix: &str) -> PathBuf {
        // Salt paths with process, time, and sequence values so filesystem
        // tests can share this helper without colliding.
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "ganbaru-ai-vault-test-{pid}-{nanos}-{seq}-{suffix}"
        ));
        path
    }

    #[test]
    fn write_text_file_atomically_rejects_relative_paths() {
        let err = write_text_file_atomically(Path::new("relative/path.txt"), "data").unwrap_err();
        assert_eq!(err, "path must be absolute");
    }

    #[test]
    fn write_text_file_atomically_writes_atomically_to_absolute_path() {
        let path = unique_path("write.txt");
        let parent = path.parent().unwrap().to_path_buf();
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        let temporary_sibling = parent.join(format!("{file_name}.tmp"));

        write_text_file_atomically(&path, "payload").expect("write should succeed");

        let on_disk = fs::read_to_string(&path).expect("file should exist");
        assert_eq!(on_disk, "payload");
        // The .tmp sibling must not survive a successful write.
        assert!(
            !temporary_sibling.exists(),
            "tmp file should have been renamed away"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn initialize_vault_creates_manifest_config_database_and_dirs() {
        let path = unique_path("portable-vault");
        fs::create_dir_all(&path).expect("create vault folder");

        let info = initialize_vault(&path).expect("initialize vault");

        assert_eq!(
            info.database_path,
            path.join(APP_SQLITE_FILE).to_string_lossy()
        );
        assert!(path.join(VAULT_MANIFEST_FILE).exists());
        assert!(path.join(CONFIG_FILE).exists());
        assert!(path.join(APP_SQLITE_FILE).exists());
        assert!(path.join("notes").join("daily").is_dir());
        assert!(path.join("diary").join("morning").is_dir());
        assert!(path.join(".yjs").is_dir());

        let _ = fs::remove_dir_all(&path);
    }

    #[test]
    #[cfg(desktop)]
    fn background_vault_resolution_waits_for_selection_but_rejects_invalid_manifests() {
        let path = unique_path("background-vault");
        assert_eq!(available_vault_path(None).unwrap(), None);
        assert_eq!(available_vault_path(Some(path.clone())).unwrap(), None);
        fs::create_dir_all(&path).unwrap();
        assert!(available_vault_path(Some(path.clone())).is_err());
        initialize_vault(&path).unwrap();
        assert_eq!(
            available_vault_path(Some(path.clone())).unwrap(),
            Some(path.clone())
        );
        fs::write(path.join(VAULT_MANIFEST_FILE), "invalid json").unwrap();
        assert!(available_vault_path(Some(path.clone())).is_err());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn initialize_vault_rejects_non_empty_non_vault_folders() {
        let path = unique_path("non-empty");
        fs::create_dir_all(&path).expect("create vault folder");
        fs::write(path.join("random.txt"), "existing file").expect("seed file");

        let err = initialize_vault(&path).unwrap_err();

        assert_eq!(
            err,
            "selected folder is not empty and is not a Ganbaru AI folder"
        );
        let _ = fs::remove_dir_all(&path);
    }

    #[test]
    fn default_vault_name_tracks_build_mode() {
        if cfg!(debug_assertions) {
            assert_eq!(default_vault_name(), DEVELOPMENT_VAULT_NAME);
        } else {
            assert_eq!(default_vault_name(), PRODUCTION_VAULT_NAME);
        }
    }

    #[test]
    fn recent_vault_ids_refuse_to_guess_while_a_folder_is_unreachable() {
        let vault = unique_path("recent-vault");
        fs::create_dir_all(&vault).expect("create vault folder");
        let info = initialize_vault(&vault).expect("initialize vault");
        let plain = unique_path("recent-plain");
        fs::create_dir_all(&plain).expect("create plain folder");
        let reachable = vec![
            vault.to_string_lossy().into_owned(),
            plain.to_string_lossy().into_owned(),
        ];

        assert_eq!(
            recent_vault_ids(&reachable),
            Some(BTreeSet::from([info.vault_id]))
        );
        let mut with_missing = reachable.clone();
        with_missing.push(unique_path("recent-missing").to_string_lossy().into_owned());
        assert_eq!(recent_vault_ids(&with_missing), None);
        let _ = fs::remove_dir_all(&vault);
        let _ = fs::remove_dir_all(&plain);
    }

    #[test]
    fn vault_scopes_keep_only_known_vaults() {
        let mut state = VaultAppState::default();
        for vault_id in ["vault-live", "vault-deleted"] {
            state
                .music_root_bindings
                .insert(vault_id.to_string(), BTreeMap::new());
            state
                .project_working_folders
                .vaults
                .insert(vault_id.to_string(), BTreeMap::new());
            state
                .chat
                .vaults
                .insert(vault_id.to_string(), BTreeMap::new());
        }

        retain_vault_scopes(&mut state, &BTreeSet::from(["vault-live".to_string()]));

        let live = ["vault-live"];
        assert_eq!(
            state.music_root_bindings.keys().collect::<Vec<_>>(),
            live.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            state
                .project_working_folders
                .vaults
                .keys()
                .collect::<Vec<_>>(),
            live.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            state.chat.vaults.keys().collect::<Vec<_>>(),
            live.iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn app_state_round_trips_active_and_recent_vault_paths() {
        let path = unique_path("app-state.json");
        let mut music_root_bindings = BTreeMap::new();
        music_root_bindings.insert(
            "vault-1".to_string(),
            BTreeMap::from([(
                "soundtracks".to_string(),
                "/mnt/music/soundtracks".to_string(),
            )]),
        );
        let state = VaultAppState {
            device_id: Some("device-test".to_string()),
            active_vault_path: Some("/tmp/ganbaru-ai-vault".to_string()),
            recent_vault_paths: vec!["/tmp/ganbaru-ai-vault".to_string()],
            music_root_bindings,
            project_working_folders: Default::default(),
            chat: Default::default(),
        };

        write_app_state_to_path(&path, &state).expect("write app state");
        let saved = read_app_state_from_path(&path).expect("read app state");

        assert_eq!(saved.active_vault_path, state.active_vault_path);
        assert_eq!(saved.device_id, state.device_id);
        assert_eq!(saved.recent_vault_paths, state.recent_vault_paths);
        assert_eq!(saved.music_root_bindings, state.music_root_bindings);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn app_state_cache_reuses_parse_until_the_file_changes() {
        let path = unique_path("cached-app-state.json");
        let mut cache = None;

        let missing = cached_app_state(&mut cache, &path).expect("read missing app state");
        assert_eq!(*missing, VaultAppState::default());

        let mut state = VaultAppState {
            device_id: Some("device-test".to_string()),
            ..VaultAppState::default()
        };
        write_app_state_to_path(&path, &state).expect("write app state");
        let first = cached_app_state(&mut cache, &path).expect("read written app state");
        let second = cached_app_state(&mut cache, &path).expect("read cached app state");
        assert_eq!(first.device_id.as_deref(), Some("device-test"));
        assert!(Arc::ptr_eq(&first, &second));

        // A different length changes the stamp even on coarse-timestamp filesystems.
        state.active_vault_path = Some("/tmp/ganbaru-ai-external-vault".to_string());
        write_app_state_to_path(&path, &state).expect("write external change");
        let changed = cached_app_state(&mut cache, &path).expect("read changed app state");
        assert_eq!(changed.active_vault_path, state.active_vault_path);
        assert!(!Arc::ptr_eq(&first, &changed));

        fs::remove_file(&path).expect("remove app state");
        let removed = cached_app_state(&mut cache, &path).expect("read removed app state");
        assert_eq!(*removed, VaultAppState::default());
    }

    #[test]
    fn app_state_rejects_missing_current_device_state_sections() {
        let path = unique_path("incomplete-app-state.json");
        fs::write(
            &path,
            r#"{"activeVaultPath":"/tmp/vault","recentVaultPaths":[]}"#,
        )
        .expect("write incomplete state");

        let state = read_app_state_from_path(&path);

        assert!(state.is_err());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn app_state_requires_current_nullable_fields() {
        let current = serde_json::to_value(VaultAppState::default()).expect("serialize app state");
        assert!(serde_json::from_value::<VaultAppState>(current.clone()).is_ok());

        for field in ["deviceId", "activeVaultPath"] {
            let mut incomplete = current.clone();
            incomplete.as_object_mut().unwrap().remove(field);

            assert!(
                serde_json::from_value::<VaultAppState>(incomplete).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn provider_probe_requires_current_authority_support_fields() {
        let path = unique_path("incomplete-provider-probe-app-state.json");
        fs::write(
            &path,
            r#"{
  "deviceId": "device-test",
  "activeVaultPath": null,
  "recentVaultPaths": [],
  "musicRootBindings": {},
  "projectWorkingFolders": { "schemaVersion": 1, "vaults": {} },
  "chat": {
    "schemaVersion": 1,
    "vaults": {
      "vault-test": {
        "device-test": {
          "providerInstances": {
            "codex": {
              "executablePath": null,
              "providerHomePath": null,
              "lastProbe": {
                "instanceId": "codex",
                "state": "healthy",
                "version": "1.0.0",
                "negotiatedProtocolVersion": null,
                "accountLabel": null,
                "capabilities": { "entries": [] },
                "authoritySupport": {
                  "internalHostTools": true,
                  "denyShell": true,
                  "readOnlyRoot": true,
                  "writableRoot": true,
                  "confinedCommands": true,
                  "networkBoundary": true,
                  "classifiedPublish": true
                },
                "checkedAt": "2026-08-22T00:00:00Z",
                "detail": null
              },
              "lastSuccessfulProbeAt": null,
              "modelCatalog": null
            }
          },
          "fullAccessTrust": {},
          "preferences": {
            "lastSelectedThreadId": null
          },
          "diagnostics": { "captureEnabled": false, "retentionDays": 7 },
          "executionEnvironmentPaths": {}
        }
      }
    }
  }
}"#,
        )
        .expect("write incomplete provider probe state");

        let state = read_app_state_from_path(&path);

        assert!(state.is_err());
        let _ = fs::remove_file(&path);
    }
}
