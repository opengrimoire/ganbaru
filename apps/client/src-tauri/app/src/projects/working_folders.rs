//! Device-local persistence adapter for project working-folder state.

use crate::vault::{active_vault_id, read_app_state, update_app_state, vault_device_id};
use tauri::Runtime;

#[cfg(desktop)]
pub use ganbaru_working_folders::ProjectWorkingFolderBindingState;
pub use ganbaru_working_folders::{
    WORKING_FOLDER_DEVICE_STATE_SCHEMA_VERSION, WorkingFolderDeviceScope,
};

pub fn read_active_working_folder_scope<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<WorkingFolderDeviceScope, String> {
    let vault_id = active_vault_id(app)?;
    let device_id = vault_device_id(app.clone())?;
    let state = read_app_state(app)?;
    Ok(state
        .project_working_folders
        .scope(&vault_id, &device_id)
        .cloned()
        .unwrap_or_default())
}

pub fn update_active_working_folder_scope<R: Runtime, T>(
    app: &tauri::AppHandle<R>,
    update: impl FnOnce(&mut WorkingFolderDeviceScope) -> Result<T, String>,
) -> Result<T, String> {
    let vault_id = active_vault_id(app)?;
    let device_id = vault_device_id(app.clone())?;
    update_app_state(app, |state| {
        if state.project_working_folders.schema_version
            != WORKING_FOLDER_DEVICE_STATE_SCHEMA_VERSION
        {
            return Err("project working-folder device state is unsupported".to_string());
        }
        update(
            state
                .project_working_folders
                .scope_mut(&vault_id, &device_id),
        )
    })
}
