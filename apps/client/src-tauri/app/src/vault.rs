//! Vault filesystem layer.
//!
//! The vault holds portable user data. The Tauri app config directory stores
//! only the active vault pointer and device-local runtime files.
//!
//! Writes are atomic: serialize to an exclusively created, randomly named
//! temporary file, fsync, rename. A crash mid-write leaves either the previous
//! good file or an ignored temporary file. Vault entries the app manages must
//! not be symlinks; see `reject_symlinked_vault_entries`.

use chrono::{DateTime, SecondsFormat, Utc};
use ganbaru_atomic_files::FileAccess;
#[cfg(target_os = "android")]
use ganbaru_mobile_documents::MobileDocumentsExt;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{Manager, Runtime};
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, FilePath};

static APP_STATE: std::sync::Mutex<Option<CachedAppState>> = std::sync::Mutex::new(None);
static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod config;
pub(crate) use config::read_active_config_bounded;
mod documents;

// Keep Tauri commands and their generated wrappers at the vault facade.
pub use config::*;
pub use documents::*;

pub(crate) mod backup;
pub(crate) mod database_rollback;
pub(crate) mod handoff;
pub(crate) mod ownership;
pub(crate) mod preferences;
#[cfg(any(mobile, test))]
pub(crate) mod process_managers;
pub(crate) mod quiescence;
pub(crate) mod runtime_lifecycle;
pub(crate) mod vault_lock;

pub const APP_SQLITE_FILE: &str = ganbaru_sync_replica::backup::DATABASE_FILE;
const PRODUCTION_VAULT_NAME: &str = "Ganbaru";
const DEVELOPMENT_VAULT_NAME: &str = "Ganbaru Dev";
const APP_STATE_FILE: &str = "app-state.json";
const VAULT_MANIFEST_FILE: &str = ganbaru_sync_replica::backup::VAULT_MANIFEST_FILE;
const CONFIG_FILE: &str = ganbaru_sync_replica::backup::CONFIG_FILE;
const VAULT_APP_MARKER: &str = "ganbaru";
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
    #[serde(default)]
    pub music_path_grants: crate::music::path_authority::MusicPathGrantState,
    /// Local soundscape file paths by vault ID and sound ID; the vault keeps only the identities.
    #[serde(default)]
    pub music_soundscape_paths: BTreeMap<String, BTreeMap<String, String>>,
    /// Android starter vault this installation created and the user has not entered yet. Only
    /// native code sets it, and selecting any other vault or entering the app clears it.
    #[serde(default)]
    pub starter_vault_id: Option<String>,
}

/// Whether the active vault is the untouched starter vault, which a first link may replace
/// without a portable backup.
fn is_untouched_starter(state: &VaultAppState, active_vault_id: &str) -> bool {
    state.starter_vault_id.as_deref() == Some(active_vault_id)
}

/// Forgets the starter vault once another vault is selected, so the record never returns.
fn forget_starter_unless(state: &mut VaultAppState, selected_vault_id: &str) {
    state
        .starter_vault_id
        .take_if(|starter| starter != selected_vault_id);
}

/// Whether the active vault is the untouched Android starter vault. Desktop never has one.
pub(crate) fn untouched_starter_vault<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<bool, String> {
    if !cfg!(mobile) {
        return Ok(false);
    }
    let state = read_app_state(app)?;
    if state.starter_vault_id.is_none() {
        return Ok(false);
    }
    Ok(is_untouched_starter(&state, &active_vault_id(app)?))
}

/// Records that the user entered the app with the starter vault, which makes it user data.
#[tauri::command]
pub fn vault_release_starter(app: tauri::AppHandle) -> Result<(), String> {
    update_app_state(&app, |state| {
        state.starter_vault_id = None;
        Ok(())
    })
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(deserializer)
}

pub(crate) fn ensure_device_id<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<String, String> {
    ensure_device_id_in(&app_config_dir(app)?)
}

/// The device id stored in the app config directory `config_dir`, created on first use.
pub(crate) fn ensure_device_id_in(config_dir: &Path) -> Result<String, String> {
    // Most calls find the identity already stored; avoid cloning device state for an update.
    if let Some(device_id) = read_app_state_in(config_dir)?
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
    update_app_state_in(config_dir, |state| {
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

fn app_config_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

fn app_state_path_in(config_dir: &Path) -> PathBuf {
    config_dir.join(APP_STATE_FILE)
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
    write_text_file_atomically(path, json, FileAccess::OwnerOnly)
}

/// Keeps the app config directory, which holds device identity, usage spools, and runtime
/// snapshots, accessible only by the current user.
#[cfg(desktop)]
pub(crate) fn secure_app_config_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let path = app.path().app_config_dir().map_err(|e| e.to_string())?;
    make_directory_private(&path)
}

/// Creates `path` with owner-only access, or removes group and other access from an existing
/// directory on Unix. A symlinked directory is left as the user arranged it. On Windows the
/// per-user profile already limits access.
#[cfg(any(desktop, test))]
fn make_directory_private(path: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(path)
        .map_err(|e| format!("create app config dir: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata =
            fs::symlink_metadata(path).map_err(|e| format!("inspect app config dir: {e}"))?;
        let mode = metadata.permissions().mode();
        if metadata.is_dir() && mode & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(mode & !0o077))
                .map_err(|e| format!("restrict app config dir: {e}"))?;
        }
    }
    Ok(())
}

pub(crate) fn read_app_state<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Arc<VaultAppState>, String> {
    read_app_state_in(&app_config_dir(app)?)
}

/// The device state stored in the app config directory `config_dir`.
fn read_app_state_in(config_dir: &Path) -> Result<Arc<VaultAppState>, String> {
    let mut cache = APP_STATE
        .lock()
        .map_err(|_| "app state lock is unavailable".to_string())?;
    cached_app_state(&mut cache, &app_state_path_in(config_dir))
}

pub(crate) fn update_app_state<R: Runtime, T>(
    app: &tauri::AppHandle<R>,
    update: impl FnOnce(&mut VaultAppState) -> Result<T, String>,
) -> Result<T, String> {
    update_app_state_in(&app_config_dir(app)?, update)
}

/// Updates the device state stored in the app config directory `config_dir`.
fn update_app_state_in<T>(
    config_dir: &Path,
    update: impl FnOnce(&mut VaultAppState) -> Result<T, String>,
) -> Result<T, String> {
    let mut cache = APP_STATE
        .lock()
        .map_err(|_| "app state lock is unavailable".to_string())?;
    let path = app_state_path_in(config_dir);
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

pub(crate) fn now_utc() -> DateTime<Utc> {
    std::time::SystemTime::now().into()
}

fn require_absolute_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("Ganbaru folder path must be absolute".to_string());
    }
    let metadata =
        fs::metadata(path).map_err(|e| format!("failed to inspect Ganbaru folder path: {e}"))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err("Ganbaru path must be a folder".to_string())
    }
}

fn canonical_vault_path(path: PathBuf) -> Result<PathBuf, String> {
    require_absolute_directory(&path)?;
    fs::canonicalize(&path).map_err(|e| format!("canonicalize Ganbaru folder path: {e}"))
}

fn folder_is_empty(path: &Path) -> Result<bool, String> {
    let mut entries = fs::read_dir(path).map_err(|e| format!("read Ganbaru folder: {e}"))?;
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

/// Largest `vault.json` accepted from an archive or a vault folder. The marker holds a few
/// short fields.
const MAX_VAULT_MANIFEST_BYTES: u64 = 64 * 1024;

/// Largest `config.json` any reader accepts. Writes that would exceed it are refused, so a
/// configuration the app saved always reads back.
pub(crate) const MAX_VAULT_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

fn read_vault_manifest(path: &Path) -> Result<VaultManifest, String> {
    let contents = ganbaru_atomic_files::read_regular_file_capped(
        &manifest_path(path),
        MAX_VAULT_MANIFEST_BYTES,
        ganbaru_atomic_files::FinalSymlink::Refuse,
    )
    .map_err(|e| format!("read Ganbaru folder marker: {e}"))?;
    parse_vault_manifest(&contents)
}

fn parse_vault_manifest(contents: &[u8]) -> Result<VaultManifest, String> {
    let manifest: VaultManifest = serde_json::from_slice(contents)
        .map_err(|e| format!("parse Ganbaru folder marker: {e}"))?;
    if manifest.app != VAULT_APP_MARKER {
        return Err("selected folder is not a Ganbaru folder".to_string());
    }
    if manifest.schema_version != VAULT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported Ganbaru folder schema version {}",
            manifest.schema_version
        ));
    }
    // The identity names key store accounts and key files, so it keeps to the wire identifier
    // alphabet before anything uses it.
    ganbaru_handoff::protocol::validate_identifier("Ganbaru folder id", &manifest.vault_id)?;
    Ok(manifest)
}

fn vault_info_from_manifest(path: &Path, manifest: VaultManifest) -> Result<VaultInfo, String> {
    Ok(VaultInfo {
        path: path_to_string(path, "Ganbaru folder")?,
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
    reject_symlinked_vault_entries(&path)?;
    vault_info_from_manifest(&path, read_vault_manifest(&path)?)
}

/// SQLite companion files that live next to the vault database.
const DATABASE_COMPANION_SUFFIXES: &[&str] = &["-wal", "-shm", "-journal"];

/// Directories under the vault root that the app writes into by fixed name.
const MANAGED_VAULT_DIRECTORIES: &[&str] = &["assets", "projects"];

/// Refuse a vault whose fixed entries are symlinks. A link shipped inside a
/// shared or restored vault could otherwise redirect app writes to any file
/// the user can write. The vault root itself may be reached through a link.
fn reject_symlinked_vault_entries(path: &Path) -> Result<(), String> {
    let database = APP_SQLITE_FILE.to_string();
    let entries = [
        VAULT_MANIFEST_FILE.to_string(),
        CONFIG_FILE.to_string(),
        vault_lock::VAULT_LOCK_FILE.to_string(),
    ]
    .into_iter()
    .chain(
        DATABASE_COMPANION_SUFFIXES
            .iter()
            .map(|suffix| format!("{database}{suffix}")),
    )
    .chain(std::iter::once(database.clone()))
    .chain(
        MANAGED_VAULT_DIRECTORIES
            .iter()
            .map(|name| name.to_string()),
    );
    for entry in entries {
        ganbaru_atomic_files::reject_symlink(&path.join(&entry))
            .map_err(|error| format!("check Ganbaru folder entry '{entry}': {error}"))?;
    }
    Ok(())
}

/// Directories every vault holds, relative to its root.
const VAULT_SKELETON_DIRECTORIES: &[&str] = &[
    "notes/daily",
    "notes/projects",
    "diary/morning",
    "diary/evening",
    "projects",
    "reports",
    "assets",
    "templates",
];

fn ensure_vault_skeleton(path: &Path) -> Result<(), String> {
    for relative in VAULT_SKELETON_DIRECTORIES {
        ganbaru_atomic_files::create_directories_within(path, &path.join(relative))
            .map_err(|e| format!("create Ganbaru folder path '{relative}': {e}"))?;
    }
    if !config_path(path).exists() {
        write_text_file_atomically(&config_path(path), "{}\n", FileAccess::OwnerOnly)?;
    }
    match ganbaru_atomic_files::create_private_file(&database_path(path)) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!("create ganbaru.sqlite: {e}")),
    }
}

/// Validates an existing vault folder before it may become active, then completes its skeleton.
///
/// The folder, its fixed entries, and its marker are checked first, so a folder with a foreign,
/// malformed, or unsupported marker is refused before anything is written into it. The database
/// is not opened here: it opens after selection, and a failure there is reported without
/// deleting or recreating it.
fn validate_vault_candidate(path: &Path) -> Result<VaultInfo, String> {
    let info = vault_info_from_path(path)?;
    ensure_vault_skeleton(Path::new(&info.path))?;
    Ok(info)
}

fn initialize_vault(path: &Path) -> Result<VaultInfo, String> {
    let path = canonical_vault_path(path.to_path_buf())?;
    reject_symlinked_vault_entries(&path)?;
    if manifest_path(&path).exists() {
        return validate_vault_candidate(&path);
    }
    if !folder_is_empty(&path)? {
        return Err("selected folder is not empty and is not a Ganbaru folder".to_string());
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
    write_text_file_atomically(&manifest_path(&path), &json, FileAccess::OwnerOnly)?;
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
    let quiescence = quiescence::begin_reserved_quiescence(
        app,
        transition,
        quiescence::RunningTerminals::Continue,
    )
    .await?;
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

/// Persists selection while the caller retains the native and write barriers. The selected
/// vault is fenced first, which releases the previous vault's lock, so a folder another Ganbaru
/// process has open is refused before it becomes active.
fn select_quiesced_vault<R: Runtime>(
    app: &tauri::AppHandle<R>,
    info: &VaultInfo,
    _quiescence: &quiescence::SnapshotQuiescence,
) -> Result<(), String> {
    vault_lock::process_vault_lock().acquire(Path::new(&info.path))?;
    update_app_state(app, |state| {
        state.active_vault_path = Some(info.path.clone());
        state
            .recent_vault_paths
            .retain(|path| path != &info.path && !path.trim().is_empty());
        state.recent_vault_paths.insert(0, info.path.clone());
        state.recent_vault_paths.truncate(MAX_RECENT_VAULTS);
        forget_starter_unless(state, &info.vault_id);
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
        .music_soundscape_paths
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
    preferences::reset(app);
    let focus = crate::pomodoro::resume_after_vault_handoff(app);
    let music = crate::music::session::resume_after_vault_handoff(app);
    #[cfg(desktop)]
    let distractions = crate::distractions::runtime::resume_after_vault_handoff(app);
    #[cfg(target_os = "android")]
    let distractions = crate::distractions::android::runtime::resume_after_vault_handoff(app);
    #[cfg(target_os = "ios")]
    let distractions = Ok(());
    let sync = crate::sync::resume_after_vault_handoff(app);
    #[cfg(desktop)]
    app.state::<crate::chat::terminal::ChatTerminalRegistry>()
        .resume_creation();
    #[cfg(desktop)]
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = crate::vault::handoff::start_desktop_when_reachable(&app).await {
                eprintln!("vault handoff coordinator is unavailable: {error}");
            }
        });
    }
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
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
    Ok(VaultDefaultLocation {
        path: path_to_string(&path, "Ganbaru folder")?,
        parent_path: path_to_string(parent, "Ganbaru folder parent")?,
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
        ganbaru_atomic_files::create_private_directories(&path)
            .map_err(|e| format!("create Ganbaru folder: {e}"))?;
        initialize_vault(&path)
    })
    .await
    .map_err(|error| format!("vault creation worker failed: {error}"))??;
    select_reserved_vault(app, &info, transition).await?;
    Ok(info)
}

#[tauri::command]
pub fn vault_default_location(app: tauri::AppHandle) -> Result<VaultDefaultLocation, String> {
    default_location_from_path(default_vault_path(&app)?)
}

#[tauri::command]
pub async fn vault_use_default(app: tauri::AppHandle) -> Result<VaultInfo, String> {
    let path = default_vault_path(&app)?;
    let creates_vault = !manifest_path(&path).exists();
    let info = create_and_select_vault(&app, path).await?;
    if cfg!(mobile) && creates_vault {
        update_app_state(&app, |state| {
            state.starter_vault_id = Some(info.vault_id.clone());
            Ok(())
        })?;
    }
    Ok(info)
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
        "Import Ganbaru folder",
        existing_documents_directory(&app),
    )
    .await?
    else {
        return Ok(None);
    };
    let info = validate_vault_candidate(&path)?;
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
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create app data directory: {error}"))?;
    let staging = parent.join(format!(".{}.import", default_vault_name()));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("remove stale folder import: {error}"))?;
    }
    if target.exists() {
        if !folder_is_empty(&target)? {
            return Err(
                "the private Ganbaru folder already exists; use it or remove its test data first"
                    .to_string(),
            );
        }
        fs::remove_dir(&target).map_err(|error| format!("remove empty Ganbaru folder: {error}"))?;
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
        validate_vault_candidate(&staging)?;
        fs::rename(&staging, &target)
            .map_err(|error| format!("activate imported Ganbaru folder: {error}"))?;
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

/// The active vault root of the app config directory `config_dir`.
pub(crate) fn active_vault_path_in(config_dir: &Path) -> Result<PathBuf, String> {
    active_vault_in(config_dir).map(|(path, _)| path)
}

/// Resolve the active folder and its validated marker with one marker read.
fn active_vault<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(PathBuf, VaultManifest), String> {
    active_vault_in(&app_config_dir(app)?)
}

/// Resolve the active folder of the app config directory `config_dir` and its validated marker
/// with one marker read.
fn active_vault_in(config_dir: &Path) -> Result<(PathBuf, VaultManifest), String> {
    let Some(path) = read_app_state_in(config_dir)?.active_vault_path.clone() else {
        return Err("no active Ganbaru folder selected".to_string());
    };
    let path = canonical_vault_path(PathBuf::from(path))?;
    reject_symlinked_vault_entries(&path)?;
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
        .map_err(|error| format!("inspect selected Ganbaru folder: {error}"))?
    {
        return Ok(None);
    }
    let path = canonical_vault_path(path)?;
    reject_symlinked_vault_entries(&path)?;
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

/// The active vault root for writing assets of the replicated sync `domain`, which a linked
/// stable replica may write as well as the owner when the domain allows replica writes.
pub(crate) fn active_replicated_asset_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    domain: &str,
) -> Result<WritableVaultPath, String> {
    replicated_asset_path_in(
        &app_config_dir(app)?,
        app.state::<ownership::VaultOwnershipManager>().inner(),
        domain,
    )
}

/// [`active_replicated_asset_path`] for the app config directory `config_dir`, with the permit
/// taken from `ownership`.
pub(crate) fn replicated_asset_path_in(
    config_dir: &Path,
    ownership: &ownership::VaultOwnershipManager,
    domain: &str,
) -> Result<WritableVaultPath, String> {
    let (path, manifest) = active_vault_in(config_dir)?;
    let permit = ownership.acquire_replicated_write(&manifest.vault_id, domain)?;
    Ok(WritableVaultPath {
        path,
        _permit: permit,
    })
}

pub(crate) fn active_vault_id<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<String, String> {
    active_vault(app).map(|(_, manifest)| manifest.vault_id)
}

/// The active vault id of the app config directory `config_dir`.
pub(crate) fn active_vault_id_in(config_dir: &Path) -> Result<String, String> {
    active_vault_in(config_dir).map(|(_, manifest)| manifest.vault_id)
}

pub fn active_database_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(database_path(&active_vault_path(app)?))
}

/// The active vault database of the app config directory `config_dir`.
pub(crate) fn active_database_path_in(config_dir: &Path) -> Result<PathBuf, String> {
    Ok(database_path(&active_vault_path_in(config_dir)?))
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

/// Creates a vault with id `vault_id` in the empty folder `vault_path` and makes it the active
/// vault of device `device_id` in the app config directory `config_dir`. Returns the path of
/// its database, which is created empty and migrates on first open.
#[cfg(test)]
pub(crate) fn activate_test_vault(
    config_dir: &Path,
    vault_path: &Path,
    vault_id: &str,
    device_id: &str,
) -> Result<PathBuf, String> {
    fs::create_dir_all(vault_path).map_err(|e| format!("create test vault: {e}"))?;
    let path = canonical_vault_path(vault_path.to_path_buf())?;
    let manifest = VaultManifest {
        app: VAULT_APP_MARKER.to_string(),
        schema_version: VAULT_SCHEMA_VERSION,
        vault_id: vault_id.to_string(),
        display_name: display_name_from_path(&path),
        created_at: now_utc().to_rfc3339_opts(SecondsFormat::Millis, true),
    };
    ensure_vault_skeleton(&path)?;
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    write_text_file_atomically(&manifest_path(&path), &json, FileAccess::OwnerOnly)?;
    let active = path.to_string_lossy().into_owned();
    update_app_state_in(config_dir, |state| {
        state.device_id = Some(device_id.to_string());
        state.active_vault_path = Some(active);
        Ok(())
    })?;
    Ok(database_path(&path))
}

/// Replace a UTF-8 text file through a synced, exclusively created temporary
/// file and a rename, so an interrupted write cannot truncate the target and a
/// symlink planted next to it cannot redirect the bytes.
fn write_text_file_atomically(
    path: &Path,
    contents: &str,
    access: FileAccess,
) -> Result<(), String> {
    require_absolute_path(path)?;
    ganbaru_atomic_files::write_file_atomically(path, contents.as_bytes(), access)
        .map_err(|e| e.to_string())
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
    Err("opening Ganbaru folders is not implemented for this platform".to_string())
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
        .map_err(|e| format!("open Ganbaru folder: {e}"))
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
        path.push(format!("ganbaru-vault-test-{pid}-{nanos}-{seq}-{suffix}"));
        path
    }

    #[cfg(unix)]
    #[test]
    fn app_config_directory_is_created_and_kept_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_path("private-config");
        let created = root.join("created");
        make_directory_private(&created).unwrap();
        let mode = fs::metadata(&created).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        let shared = root.join("shared");
        fs::create_dir_all(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
        make_directory_private(&shared).unwrap();
        let mode = fs::metadata(&shared).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_text_file_atomically_rejects_relative_paths() {
        let err = write_text_file_atomically(
            Path::new("relative/path.txt"),
            "data",
            FileAccess::OwnerOnly,
        )
        .unwrap_err();
        assert_eq!(err, "path must be absolute");
    }

    #[test]
    fn write_text_file_atomically_leaves_only_the_target() {
        let parent = unique_path("write");
        fs::create_dir_all(&parent).unwrap();
        let path = parent.join("write.txt");

        write_text_file_atomically(&path, "payload", FileAccess::OwnerOnly)
            .expect("write should succeed");

        assert_eq!(fs::read_to_string(&path).unwrap(), "payload");
        let names = fs::read_dir(&parent)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, vec![std::ffi::OsString::from("write.txt")]);
        fs::remove_dir_all(parent).unwrap();
    }

    /// The config writer used by `mutate_active_vault_config` never writes
    /// through a temporary-name link planted in a shared vault.
    #[cfg(unix)]
    #[test]
    fn config_writes_ignore_a_planted_temporary_symlink() {
        let vault = unique_path("planted-temporary");
        fs::create_dir_all(&vault).unwrap();
        initialize_vault(&vault).unwrap();
        let sentinel = unique_path("planted-temporary-sentinel");
        fs::write(&sentinel, "untouched").unwrap();
        std::os::unix::fs::symlink(&sentinel, vault.join("config.json.tmp")).unwrap();

        write_text_file_atomically(
            &config_path(&vault),
            "{\"preferences\":{}}",
            FileAccess::OwnerOnly,
        )
        .unwrap();

        assert_eq!(fs::read_to_string(&sentinel).unwrap(), "untouched");
        let metadata = fs::symlink_metadata(config_path(&vault)).unwrap();
        assert!(metadata.file_type().is_file());
        fs::remove_dir_all(vault).unwrap();
        fs::remove_file(sentinel).unwrap();
    }

    #[test]
    fn an_oversized_vault_marker_is_refused_before_parsing() {
        let vault = unique_path("oversized-marker");
        fs::create_dir_all(&vault).unwrap();
        initialize_vault(&vault).unwrap();
        let mut contents = fs::read(manifest_path(&vault)).unwrap();
        let limit = usize::try_from(MAX_VAULT_MANIFEST_BYTES).unwrap();
        contents.resize(limit + 1, b' ');
        fs::write(manifest_path(&vault), contents).unwrap();
        let error = vault_info_from_path(&vault).unwrap_err();
        assert!(error.contains("read Ganbaru folder marker"), "{error}");
        let _ = fs::remove_dir_all(vault);
    }

    #[cfg(unix)]
    #[test]
    fn opening_a_vault_rejects_symlinked_fixed_entries() {
        for entry in [
            "config.json",
            "ganbaru.sqlite",
            "ganbaru.sqlite-wal",
            "vault.lock",
            "assets",
        ] {
            let vault = unique_path("symlinked-entry");
            fs::create_dir_all(&vault).unwrap();
            initialize_vault(&vault).unwrap();
            let target = unique_path("symlinked-entry-target");
            fs::create_dir_all(&target).unwrap();
            let link = vault.join(entry);
            if link.is_dir() {
                fs::remove_dir_all(&link).unwrap();
            } else if link.exists() {
                fs::remove_file(&link).unwrap();
            }
            std::os::unix::fs::symlink(target.join("elsewhere"), &link).unwrap();

            let error = vault_info_from_path(&vault).unwrap_err();
            assert!(error.contains(entry), "{entry}: {error}");
            assert!(initialize_vault(&vault).is_err(), "{entry}");
            fs::remove_dir_all(vault).unwrap();
            fs::remove_dir_all(target).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn opening_a_vault_through_a_symlinked_root_still_works() {
        let vault = unique_path("linked-root");
        fs::create_dir_all(&vault).unwrap();
        let created = initialize_vault(&vault).unwrap();
        let alias = unique_path("linked-root-alias");
        std::os::unix::fs::symlink(&vault, &alias).unwrap();

        let opened = vault_info_from_path(&alias).unwrap();
        assert_eq!(opened.vault_id, created.vault_id);
        fs::remove_file(alias).unwrap();
        fs::remove_dir_all(vault).unwrap();
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
        assert!(!path.join(".yjs").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(path.join(APP_SQLITE_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

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
    fn vault_markers_refuse_ids_that_could_name_other_key_files_or_accounts() {
        let marker = |vault_id: &str| {
            serde_json::json!({
                "app": VAULT_APP_MARKER,
                "schemaVersion": VAULT_SCHEMA_VERSION,
                "vaultId": vault_id,
                "displayName": "Vault",
                "createdAt": "2026-10-20T00:00:00Z",
            })
            .to_string()
        };
        let generated = new_vault_id();
        assert_eq!(
            parse_vault_manifest(marker(&generated).as_bytes())
                .unwrap()
                .vault_id,
            generated
        );
        for vault_id in [
            "",
            "../other",
            "a/b",
            "a\\b",
            "vault:other",
            "vault\nid",
            &"v".repeat(161),
        ] {
            assert!(
                parse_vault_manifest(marker(vault_id).as_bytes()).is_err(),
                "{vault_id:?}"
            );
        }
    }

    #[test]
    fn initialize_vault_rejects_non_empty_non_vault_folders() {
        let path = unique_path("non-empty");
        fs::create_dir_all(&path).expect("create vault folder");
        fs::write(path.join("random.txt"), "existing file").expect("seed file");

        let err = initialize_vault(&path).unwrap_err();

        assert_eq!(
            err,
            "selected folder is not empty and is not a Ganbaru folder"
        );
        let _ = fs::remove_dir_all(&path);
    }

    #[test]
    fn folders_with_a_foreign_or_unsupported_marker_are_refused_before_any_write() {
        let markers = [
            (
                serde_json::json!({
                    "app": "another-app",
                    "schemaVersion": VAULT_SCHEMA_VERSION,
                    "vaultId": "vault",
                    "displayName": "Vault",
                    "createdAt": "2026-10-20T00:00:00Z",
                }),
                "selected folder is not a Ganbaru folder".to_string(),
            ),
            (
                serde_json::json!({
                    "app": VAULT_APP_MARKER,
                    "schemaVersion": VAULT_SCHEMA_VERSION + 1,
                    "vaultId": "vault",
                    "displayName": "Vault",
                    "createdAt": "2026-10-20T00:00:00Z",
                }),
                format!(
                    "unsupported Ganbaru folder schema version {}",
                    VAULT_SCHEMA_VERSION + 1
                ),
            ),
        ];
        for (marker, expected) in markers {
            let path = unique_path("foreign-marker");
            fs::create_dir_all(&path).expect("create folder");
            fs::write(path.join(VAULT_MANIFEST_FILE), marker.to_string()).expect("seed marker");

            assert_eq!(initialize_vault(&path).unwrap_err(), expected);
            assert_eq!(validate_vault_candidate(&path).unwrap_err(), expected);

            let entries = fs::read_dir(&path)
                .expect("read folder")
                .map(|entry| entry.expect("folder entry").file_name())
                .collect::<Vec<_>>();
            assert_eq!(entries, [std::ffi::OsString::from(VAULT_MANIFEST_FILE)]);
            let _ = fs::remove_dir_all(&path);
        }
    }

    #[test]
    fn validating_an_existing_vault_completes_its_skeleton() {
        let path = unique_path("existing-vault");
        fs::create_dir_all(&path).expect("create vault folder");
        let created = initialize_vault(&path).expect("initialize vault");
        fs::remove_dir_all(path.join("assets")).expect("remove assets");
        fs::remove_file(path.join(CONFIG_FILE)).expect("remove config");

        let opened = validate_vault_candidate(&path).expect("validate vault");

        assert_eq!(opened.vault_id, created.vault_id);
        assert!(path.join("assets").is_dir());
        assert!(path.join(CONFIG_FILE).is_file());
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
                .music_soundscape_paths
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
            state.music_soundscape_paths.keys().collect::<Vec<_>>(),
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
            active_vault_path: Some("/tmp/ganbaru-vault".to_string()),
            recent_vault_paths: vec!["/tmp/ganbaru-vault".to_string()],
            music_root_bindings,
            project_working_folders: Default::default(),
            chat: Default::default(),
            music_path_grants: Default::default(),
            music_soundscape_paths: Default::default(),
            starter_vault_id: None,
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
    fn starter_record_names_one_vault_and_never_returns_after_another_is_selected() {
        let mut state = VaultAppState::default();
        assert!(!is_untouched_starter(&state, "vault-starter"));

        state.starter_vault_id = Some("vault-starter".to_string());
        assert!(is_untouched_starter(&state, "vault-starter"));
        assert!(!is_untouched_starter(&state, "vault-imported"));

        forget_starter_unless(&mut state, "vault-starter");
        assert!(is_untouched_starter(&state, "vault-starter"));

        forget_starter_unless(&mut state, "vault-imported");
        assert_eq!(state.starter_vault_id, None);
        forget_starter_unless(&mut state, "vault-starter");
        assert!(!is_untouched_starter(&state, "vault-starter"));
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
        state.active_vault_path = Some("/tmp/ganbaru-external-vault".to_string());
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
