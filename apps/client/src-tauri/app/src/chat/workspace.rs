//! Tauri device-state integration for project working folders.

pub mod commands;
pub mod files;
pub mod observer;

use crate::projects::working_folders::{
    ProjectWorkingFolderBindingState, read_active_working_folder_scope,
    update_active_working_folder_scope,
};
use crate::vault;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use std::path::Path;
use tauri::Runtime;

pub use ganbaru_chat::workspace::*;

pub fn ensure_managed_working_folder_binding<R: Runtime>(
    app: &tauri::AppHandle<R>,
    working_folder: &ProjectWorkingFolder,
) -> ChatResult<()> {
    if working_folder.kind != WorkingFolderKind::Managed {
        return Ok(());
    }
    let existing = read_active_working_folder_scope(app)
        .map_err(device_state_error)?
        .bindings
        .contains_key(&working_folder.id);
    if existing {
        return Ok(());
    }
    let relative_path = working_folder
        .managed_relative_path
        .as_deref()
        .ok_or_else(path_validation_error)?;
    let expected =
        ganbaru_working_folders::managed_project_relative_path(working_folder.project_id.as_str())
            .map_err(|_| path_validation_error())?;
    if relative_path != expected {
        return Err(path_validation_error());
    }
    let vault_root = vault::active_writable_vault_path(app).map_err(device_state_error)?;
    let folder_path = ganbaru_working_folders::resolve_managed_project_folder(
        &vault_root,
        working_folder.project_id.as_str(),
    )
    .map_err(|_| {
        ChatError::new(
            ChatErrorCode::Permission,
            "The managed project working folder could not be created inside the active Ganbaru folder",
            true,
        )
    })?;
    let (_, binding) = prepare_workspace_binding(working_folder, &folder_path)?;
    store_active_device_binding(app, &working_folder.id, binding)
}

pub fn validate_external_folder_outside_vault<R: Runtime>(
    app: &tauri::AppHandle<R>,
    selected_path: &Path,
) -> ChatResult<()> {
    let selected = canonical_existing_directory(selected_path)?;
    let vault_root = vault::active_vault_path(app).map_err(device_state_error)?;
    if paths_overlap(&selected, &vault_root) {
        return Err(ChatError::validation(
            "workingFolderPath",
            "External project working folders cannot overlap the active Ganbaru folder",
        ));
    }
    Ok(())
}

pub fn store_active_device_binding<R: Runtime>(
    app: &tauri::AppHandle<R>,
    working_folder_id: &ProjectWorkingFolderId,
    binding: ProjectWorkingFolderBindingState,
) -> ChatResult<()> {
    update_active_working_folder_scope(app, |scope| {
        let previous = scope.bindings.insert(working_folder_id.clone(), binding);
        let rebound = previous.is_some_and(|previous| {
            scope
                .bindings
                .get(working_folder_id)
                .is_some_and(|current| {
                    current.canonical_path != previous.canonical_path
                        || current.filesystem_identity != previous.filesystem_identity
                })
        });
        if rebound {
            scope.trust.remove(working_folder_id);
        }
        Ok(())
    })
    .map_err(device_state_error)
}

pub fn remove_active_device_binding<R: Runtime>(
    app: &tauri::AppHandle<R>,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<()> {
    update_active_working_folder_scope(app, |scope| {
        scope.bindings.remove(working_folder_id);
        scope.trust.remove(working_folder_id);
        Ok(())
    })
    .map_err(device_state_error)?;
    // An unbound folder's preview logins and site data go with its binding. The unbind has
    // already happened, so a cleanup failure is reported without undoing it.
    #[cfg(desktop)]
    if let Err(error) = crate::chat::preview::data_store::discard_preview_data(
        app,
        Some(std::slice::from_ref(working_folder_id)),
    ) {
        eprintln!(
            "Could not discard Chat preview data for an unbound working folder: {}",
            error.message
        );
    }
    Ok(())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn path_validation_error() -> ChatError {
    ChatError::validation(
        "relativePath",
        "Workspace paths must be normalized relative paths",
    )
}

fn device_state_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat device state could not be updated",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::paths_overlap;
    use std::path::Path;

    #[test]
    fn external_folder_overlap_rejects_vault_ancestors_descendants_and_identity() {
        let vault = Path::new("data").join("Ganbaru");
        let vault = vault.as_path();
        assert!(paths_overlap(vault, vault));
        assert!(paths_overlap(Path::new("data"), vault));
        assert!(paths_overlap(&vault.join("projects").join("repo"), vault));
        assert!(!paths_overlap(Path::new("work/repo"), vault));
    }
}
