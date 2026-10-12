use serde::Serialize;
use std::collections::BTreeMap;
#[cfg(desktop)]
use std::{fs, path::Path};
use tauri::Runtime;

use crate::vault::{VaultAppState, active_vault_id, read_app_state, update_app_state};

const MAX_ROOT_IDS_PER_REQUEST: usize = 1_000;
const MAX_ROOT_ID_BYTES: usize = 200;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalRootBindingStatus {
    Available,
    Missing,
    NeedsRelink,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRootBindingRead {
    pub root_id: String,
    pub folder_path: Option<String>,
    pub status: LocalRootBindingStatus,
}

fn validate_root_id(root_id: &str) -> Result<&str, String> {
    let root_id = root_id.trim();
    if root_id.is_empty() {
        return Err("music root id is required".to_string());
    }
    if root_id.len() > MAX_ROOT_ID_BYTES {
        return Err(format!(
            "music root id exceeds the {MAX_ROOT_ID_BYTES} byte limit"
        ));
    }
    Ok(root_id)
}

fn require_active_vault<R: Runtime>(
    app: &tauri::AppHandle<R>,
    requested_vault_id: &str,
) -> Result<String, String> {
    let requested_vault_id = requested_vault_id.trim();
    if requested_vault_id.is_empty() {
        return Err("vault id is required".to_string());
    }
    let active_vault_id = active_vault_id(app)?;
    if requested_vault_id != active_vault_id {
        return Err("music root bindings can only be changed for the active vault".to_string());
    }
    Ok(active_vault_id)
}

#[cfg(desktop)]
fn canonical_existing_directory(folder_path: &str) -> Result<String, String> {
    let path = std::path::PathBuf::from(folder_path.trim());
    if !path.is_absolute() {
        return Err("music root folder path must be absolute".to_string());
    }
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("failed to inspect music root folder: {error}"))?;
    if !metadata.is_dir() {
        return Err("music root folder path must be a directory".to_string());
    }
    fs::canonicalize(path)
        .map_err(|error| format!("canonicalize music root folder: {error}"))?
        .to_str()
        .ok_or_else(|| "music root folder path contains non-utf8 characters".to_string())
        .map(str::to_string)
}

#[cfg(target_os = "android")]
fn canonical_existing_directory(folder_path: &str) -> Result<String, String> {
    let folder_path = folder_path.trim();
    if !folder_path.starts_with("content://") || folder_path.len() > 8_192 {
        return Err("music root must be a selected Android document folder".to_string());
    }
    Ok(folder_path.to_string())
}

fn root_bindings_for_vault<'a>(
    state: &'a VaultAppState,
    vault_id: &str,
) -> Option<&'a BTreeMap<String, String>> {
    state.music_root_bindings.get(vault_id)
}

#[cfg(desktop)]
fn binding_status(path: Option<&str>) -> LocalRootBindingStatus {
    match path {
        None => LocalRootBindingStatus::NeedsRelink,
        Some(path) if Path::new(path).is_dir() => LocalRootBindingStatus::Available,
        Some(_) => LocalRootBindingStatus::Missing,
    }
}

#[cfg(target_os = "android")]
fn binding_status(path: Option<&str>) -> LocalRootBindingStatus {
    match path {
        Some(path) if path.starts_with("content://") => LocalRootBindingStatus::Available,
        Some(_) => LocalRootBindingStatus::Missing,
        None => LocalRootBindingStatus::NeedsRelink,
    }
}

fn binding_read(state: &VaultAppState, vault_id: &str, root_id: &str) -> LocalRootBindingRead {
    let folder_path = root_bindings_for_vault(state, vault_id)
        .and_then(|bindings| bindings.get(root_id))
        .cloned();
    LocalRootBindingRead {
        root_id: root_id.to_string(),
        status: binding_status(folder_path.as_deref()),
        folder_path,
    }
}

fn set_binding(state: &mut VaultAppState, vault_id: &str, root_id: &str, folder_path: String) {
    state
        .music_root_bindings
        .entry(vault_id.to_string())
        .or_default()
        .insert(root_id.to_string(), folder_path);
}

fn clear_binding(state: &mut VaultAppState, vault_id: &str, root_id: &str) {
    let remove_vault = state
        .music_root_bindings
        .get_mut(vault_id)
        .is_some_and(|bindings| {
            bindings.remove(root_id);
            bindings.is_empty()
        });
    if remove_vault {
        state.music_root_bindings.remove(vault_id);
    }
}

#[tauri::command]
pub fn music_local_root_bindings(
    app: tauri::AppHandle,
    vault_id: String,
    root_ids: Vec<String>,
) -> Result<Vec<LocalRootBindingRead>, String> {
    let vault_id = require_active_vault(&app, &vault_id)?;
    if root_ids.len() > MAX_ROOT_IDS_PER_REQUEST {
        return Err(format!(
            "music root binding request exceeds the {MAX_ROOT_IDS_PER_REQUEST} item limit"
        ));
    }
    let state = read_app_state(&app)?;
    root_ids
        .into_iter()
        .map(|root_id| {
            let root_id = validate_root_id(&root_id)?;
            Ok(binding_read(&state, &vault_id, root_id))
        })
        .collect()
}

#[tauri::command]
pub fn music_set_local_root_binding(
    app: tauri::AppHandle,
    vault_id: String,
    root_id: String,
    folder_path: String,
) -> Result<LocalRootBindingRead, String> {
    let vault_id = require_active_vault(&app, &vault_id)?;
    let root_id = validate_root_id(&root_id)?.to_string();
    let folder_path = canonical_existing_directory(&folder_path)?;
    update_app_state(&app, |state| {
        set_binding(state, &vault_id, &root_id, folder_path);
        Ok(binding_read(state, &vault_id, &root_id))
    })
}

#[tauri::command]
pub fn music_clear_local_root_binding(
    app: tauri::AppHandle,
    vault_id: String,
    root_id: String,
) -> Result<LocalRootBindingRead, String> {
    let vault_id = require_active_vault(&app, &vault_id)?;
    let root_id = validate_root_id(&root_id)?.to_string();
    update_app_state(&app, |state| {
        clear_binding(state, &vault_id, &root_id);
        Ok(binding_read(state, &vault_id, &root_id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> VaultAppState {
        VaultAppState {
            device_id: None,
            active_vault_path: None,
            recent_vault_paths: Vec::new(),
            music_root_bindings: BTreeMap::new(),
            project_working_folders: Default::default(),
            chat: Default::default(),
        }
    }

    #[test]
    fn two_devices_bind_the_same_logical_root_independently() {
        let mut first_device = state();
        let mut second_device = state();

        set_binding(
            &mut first_device,
            "vault-1",
            "soundtracks",
            "/mnt/first/Music".to_string(),
        );
        set_binding(
            &mut second_device,
            "vault-1",
            "soundtracks",
            "/media/second/Soundtracks".to_string(),
        );

        assert_eq!(
            root_bindings_for_vault(&first_device, "vault-1")
                .unwrap()
                .get("soundtracks")
                .map(String::as_str),
            Some("/mnt/first/Music"),
        );
        assert_eq!(
            root_bindings_for_vault(&second_device, "vault-1")
                .unwrap()
                .get("soundtracks")
                .map(String::as_str),
            Some("/media/second/Soundtracks"),
        );
    }

    #[test]
    fn clearing_one_vault_binding_keeps_other_vaults_isolated() {
        let mut app_state = state();
        set_binding(
            &mut app_state,
            "vault-1",
            "soundtracks",
            "/music/one".to_string(),
        );
        set_binding(
            &mut app_state,
            "vault-2",
            "soundtracks",
            "/music/two".to_string(),
        );

        clear_binding(&mut app_state, "vault-1", "soundtracks");

        assert!(!app_state.music_root_bindings.contains_key("vault-1"));
        assert!(app_state.music_root_bindings.contains_key("vault-2"));
    }

    #[test]
    fn absent_and_stale_bindings_have_distinct_repair_states() {
        let app_state = state();
        assert_eq!(
            binding_read(&app_state, "vault-1", "soundtracks").status,
            LocalRootBindingStatus::NeedsRelink,
        );
        assert_eq!(
            binding_status(Some("/a/path/that/does/not/exist")),
            LocalRootBindingStatus::Missing,
        );
    }
}
