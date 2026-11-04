//! Device-local file paths for local soundscape loops.
//!
//! The vault stores each sound's identity, while the absolute path of its audio file stays in
//! this device's app state, keyed by vault ID and sound ID. A copied, backed up, or handed off
//! vault therefore never carries another device's paths, and no webview-supplied device ID can
//! select which paths are read.

use std::collections::BTreeMap;

use tauri::Runtime;

use crate::vault::{VaultAppState, read_app_state, update_app_state};

/// Paths this device uses for the vault's local loops, keyed by sound ID.
pub(crate) fn paths<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
) -> Result<BTreeMap<String, String>, String> {
    let state = read_app_state(app)?;
    Ok(paths_for_vault(&state, vault_id))
}

/// Records the file a local loop uses on this device.
pub(crate) fn set_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    soundscape_id: &str,
    path: &str,
) -> Result<(), String> {
    update_app_state(app, |state| {
        insert_path(state, vault_id, soundscape_id, path);
        Ok(())
    })
}

/// Forgets the file of a removed local loop.
pub(crate) fn clear_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    soundscape_id: &str,
) -> Result<(), String> {
    update_app_state(app, |state| {
        remove_path(state, vault_id, soundscape_id);
        Ok(())
    })
}

fn paths_for_vault(state: &VaultAppState, vault_id: &str) -> BTreeMap<String, String> {
    state
        .music_soundscape_paths
        .get(vault_id)
        .cloned()
        .unwrap_or_default()
}

fn insert_path(state: &mut VaultAppState, vault_id: &str, soundscape_id: &str, path: &str) {
    state
        .music_soundscape_paths
        .entry(vault_id.to_string())
        .or_default()
        .insert(soundscape_id.to_string(), path.to_string());
}

fn remove_path(state: &mut VaultAppState, vault_id: &str, soundscape_id: &str) {
    let remove_vault = state
        .music_soundscape_paths
        .get_mut(vault_id)
        .is_some_and(|paths| {
            paths.remove(soundscape_id);
            paths.is_empty()
        });
    if remove_vault {
        state.music_soundscape_paths.remove(vault_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundscape_paths_are_scoped_to_their_vault() {
        let mut state = VaultAppState::default();
        insert_path(&mut state, "vault-1", "local-rain", "/music/rain.wav");
        insert_path(&mut state, "vault-2", "local-rain", "/other/rain.wav");

        assert_eq!(
            paths_for_vault(&state, "vault-1"),
            BTreeMap::from([("local-rain".to_string(), "/music/rain.wav".to_string())])
        );
        assert!(paths_for_vault(&state, "vault-3").is_empty());

        remove_path(&mut state, "vault-1", "local-rain");
        assert!(!state.music_soundscape_paths.contains_key("vault-1"));
        assert_eq!(
            paths_for_vault(&state, "vault-2")
                .get("local-rain")
                .map(String::as_str),
            Some("/other/rain.wav")
        );
    }

    #[test]
    fn soundscape_paths_survive_an_app_state_round_trip() {
        let mut state = VaultAppState::default();
        insert_path(&mut state, "vault-1", "local-rain", "/music/rain.wav");
        let json = serde_json::to_string(&state).unwrap();
        let restored: VaultAppState = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.music_soundscape_paths,
            state.music_soundscape_paths
        );
    }
}
