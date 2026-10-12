//! Device-local Chat bindings and provider runtime metadata.

#[cfg(desktop)]
use crate::vault::{active_vault_id, read_app_state, update_app_state, vault_device_id};
use ganbaru_chat_contracts::models::{
    ChatThreadId, ProjectWorkingFolderId, ProviderInstanceId, ProviderModelCatalog,
    ProviderProbeResult, UtcTimestamp,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(desktop)]
use tauri::Runtime;

pub const CHAT_DEVICE_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatProviderDeviceState {
    pub executable_path: Option<String>,
    pub provider_home_path: Option<String>,
    pub last_probe: Option<ProviderProbeResult>,
    pub last_successful_probe_at: Option<UtcTimestamp>,
    pub model_catalog: Option<ProviderModelCatalog>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMachinePreferences {
    pub last_selected_thread_id: Option<ChatThreadId>,
}

pub const DEFAULT_DIAGNOSTIC_RETENTION_DAYS: u16 = 7;
#[cfg(desktop)]
pub const MAX_DIAGNOSTIC_RETENTION_DAYS: u16 = 30;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDiagnosticPreferences {
    pub capture_enabled: bool,
    pub retention_days: u16,
}

impl Default for ChatDiagnosticPreferences {
    fn default() -> Self {
        Self {
            capture_enabled: false,
            retention_days: DEFAULT_DIAGNOSTIC_RETENTION_DAYS,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDeviceScope {
    pub provider_instances: BTreeMap<ProviderInstanceId, ChatProviderDeviceState>,
    pub full_access_trust:
        BTreeMap<ProviderInstanceId, BTreeMap<ProjectWorkingFolderId, UtcTimestamp>>,
    pub preferences: ChatMachinePreferences,
    pub diagnostics: ChatDiagnosticPreferences,
    pub execution_environment_paths: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDeviceState {
    pub schema_version: u32,
    pub vaults: BTreeMap<String, BTreeMap<String, ChatDeviceScope>>,
}

impl Default for ChatDeviceState {
    fn default() -> Self {
        Self {
            schema_version: CHAT_DEVICE_STATE_SCHEMA_VERSION,
            vaults: BTreeMap::new(),
        }
    }
}

#[cfg(desktop)]
impl ChatDeviceState {
    pub fn scope(&self, vault_id: &str, device_id: &str) -> Option<&ChatDeviceScope> {
        self.vaults
            .get(vault_id)
            .and_then(|devices| devices.get(device_id))
    }

    pub fn scope_mut(&mut self, vault_id: &str, device_id: &str) -> &mut ChatDeviceScope {
        self.vaults
            .entry(vault_id.to_string())
            .or_default()
            .entry(device_id.to_string())
            .or_default()
    }
}

#[cfg(desktop)]
pub fn full_access_is_trusted(
    scope: &ChatDeviceScope,
    provider_instance_id: &ProviderInstanceId,
    working_folder_id: &ProjectWorkingFolderId,
) -> bool {
    scope
        .full_access_trust
        .get(provider_instance_id)
        .is_some_and(|workspaces| workspaces.contains_key(working_folder_id))
}

#[cfg(desktop)]
pub fn set_full_access_trust(
    scope: &mut ChatDeviceScope,
    provider_instance_id: ProviderInstanceId,
    working_folder_id: ProjectWorkingFolderId,
    trusted_at: Option<UtcTimestamp>,
) {
    if let Some(timestamp) = trusted_at {
        scope
            .full_access_trust
            .entry(provider_instance_id)
            .or_default()
            .insert(working_folder_id, timestamp);
        return;
    }
    let Some(workspaces) = scope.full_access_trust.get_mut(&provider_instance_id) else {
        return;
    };
    workspaces.remove(&working_folder_id);
    if workspaces.is_empty() {
        scope.full_access_trust.remove(&provider_instance_id);
    }
}

#[cfg(desktop)]
pub fn read_active_device_scope<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<ChatDeviceScope, String> {
    let vault_id = active_vault_id(app)?;
    let device_id = vault_device_id(app.clone())?;
    let state = read_app_state(app)?;
    Ok(state
        .chat
        .scope(&vault_id, &device_id)
        .cloned()
        .unwrap_or_default())
}

#[cfg(desktop)]
pub fn update_active_device_scope<R: Runtime, T>(
    app: &tauri::AppHandle<R>,
    update: impl FnOnce(&mut ChatDeviceScope) -> Result<T, String>,
) -> Result<T, String> {
    let vault_id = active_vault_id(app)?;
    let device_id = vault_device_id(app.clone())?;
    update_app_state(app, |state| {
        if state.chat.schema_version != CHAT_DEVICE_STATE_SCHEMA_VERSION {
            return Err("Chat device state schema is unsupported".to_string());
        }
        update(state.chat.scope_mut(&vault_id, &device_id))
    })
}
