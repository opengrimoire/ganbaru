//! Persistent per-working-folder web data for preview webviews.
//!
//! Each working folder that opens a preview gets its own device-local data directory under the
//! app config directory, so cookies, storage, service workers, and caches never cross folders or
//! reach the app's own webviews. The directory name is a random generation token kept in Chat
//! device state. Discarding a folder's data rotates the token instead of reusing the directory,
//! because Linux keeps every WebKit web context alive until the app exits and a reused directory
//! would bring back its in-memory session.

use super::*;
use crate::chat::device_state::{read_active_device_scope, update_active_device_scope};
use crate::chat::internal_mcp::secure_random_hex;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

const PREVIEW_DATA_DIRECTORY: &str = "chat-previews";
const GENERATION_BYTES: usize = 16;
const VAULT_DIRECTORY_BYTES: usize = 16;

/// Vault data roots already swept for orphaned directories in this app run.
#[derive(Clone, Default)]
pub(super) struct PreviewDataSweeps(Arc<Mutex<HashSet<String>>>);

/// Reports whether a stored generation token is safe to use as a directory name.
pub(super) fn is_generation_token(value: &str) -> bool {
    value.len() == GENERATION_BYTES * 2
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Preview data root for one vault. The vault ID is hashed so the directory name is always
/// filesystem-safe and does not reveal the ID.
pub(super) fn vault_preview_root(config_dir: &Path, vault_id: &str) -> PathBuf {
    let digest = Sha256::digest(vault_id.as_bytes());
    let name = digest[..VAULT_DIRECTORY_BYTES]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    config_dir.join(PREVIEW_DATA_DIRECTORY).join(name)
}

/// Directory names under a vault's preview root that no working folder references.
pub(super) fn orphaned_entries(
    entries: impl IntoIterator<Item = String>,
    referenced: &BTreeSet<String>,
) -> Vec<String> {
    entries
        .into_iter()
        .filter(|name| !referenced.contains(name))
        .collect()
}

/// Resolves a thread's working folder and the data directory its preview webviews use,
/// creating the directory when needed.
pub(super) async fn preview_data_directory(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<(ProjectWorkingFolderId, PathBuf)> {
    let working_folder_id = repository::thread_working_folder(pool, thread_id).await?;
    let root = active_preview_root(app)?;
    let fresh = secure_random_hex(GENERATION_BYTES)?;
    let generation = update_active_device_scope(app, |scope| {
        let entry = scope
            .preview_data_stores
            .entry(working_folder_id.clone())
            .or_default();
        if !is_generation_token(entry) {
            *entry = fresh;
        }
        Ok(entry.clone())
    })
    .map_err(|_| data_store_error())?;
    let sweeps = app.state::<ChatBrowserManager>().data_sweeps.clone();
    let mut swept = sweeps.0.lock().map_err(|_| browser_state_error())?;
    let root_key = root.to_string_lossy().into_owned();
    if !swept.contains(&root_key) {
        remove_orphaned_directories(app, &root);
        swept.insert(root_key);
    }
    let directory = root.join(generation);
    std::fs::create_dir_all(&directory).map_err(|_| data_store_error())?;
    Ok((working_folder_id, directory))
}

/// Discards the preview data of the given working folders, or of every folder in the active
/// vault when `folders` is `None`. Open preview tabs keep working with their data cleared.
pub(crate) fn discard_preview_data<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    folders: Option<&[ProjectWorkingFolderId]>,
) -> ChatResult<()> {
    let root = active_preview_root(app)?;
    let removed = update_active_device_scope(app, |scope| {
        let removed = match folders {
            Some(folders) => folders
                .iter()
                .filter_map(|folder| scope.preview_data_stores.remove_entry(folder))
                .collect::<Vec<_>>(),
            None => std::mem::take(&mut scope.preview_data_stores)
                .into_iter()
                .collect(),
        };
        Ok(removed)
    })
    .map_err(|_| data_store_error())?;
    app.state::<ChatBrowserManager>()
        .clear_open_tab_data(app, |folder| {
            folders.is_none_or(|folders| folders.contains(folder))
        });
    for (_, generation) in removed {
        if is_generation_token(&generation) {
            remove_directory(&root.join(generation));
        }
    }
    Ok(())
}

fn active_preview_root<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> ChatResult<PathBuf> {
    let vault_id = vault::active_vault_id(app).map_err(|_| data_store_error())?;
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|_| data_store_error())?;
    Ok(vault_preview_root(&config_dir, &vault_id))
}

/// Removes directories left by discards that failed while a webview still held their files, or
/// by a crash between rotating a token and deleting its directory. It runs before the first
/// preview of a vault in each app run, when no web context can be using them.
fn remove_orphaned_directories<R: tauri::Runtime>(app: &tauri::AppHandle<R>, root: &Path) {
    let Ok(scope) = read_active_device_scope(app) else {
        return;
    };
    let referenced = scope
        .preview_data_stores
        .into_values()
        .collect::<BTreeSet<_>>();
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let names = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    for name in orphaned_entries(names, &referenced) {
        remove_directory(&root.join(name));
    }
}

fn remove_directory(directory: &Path) {
    if let Err(error) = std::fs::remove_dir_all(directory)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("Could not remove a Chat preview data directory: {error}");
    }
}

fn data_store_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Browser preview data could not be prepared",
        true,
    )
}

/// Clears every working folder's preview browsing data in the active vault.
#[tauri::command]
pub fn chat_browser_clear_data(app: tauri::AppHandle) -> ChatResult<()> {
    discard_preview_data(&app, None)
}
