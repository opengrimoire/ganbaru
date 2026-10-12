//! Provider-free Chat settings used by mobile clients.

use crate::vault;
use ganbaru_chat::credentials::CredentialStoreAvailability;
use ganbaru_chat_contracts::config::{
    ChatBehaviorPreferences, ChatPanelPreferences, ChatVaultConfig, parse_chat_config_branch,
    replace_chat_config_branch,
};
use ganbaru_chat_contracts::models::{
    ChatResult, ChatThreadId, ProviderFamilyMetadataRead, ProviderInstanceConfig,
    ProviderModelCatalog, ProviderProbeResult,
};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInstanceRead {
    pub configuration: ProviderInstanceConfig,
    pub last_probe: Option<ProviderProbeResult>,
    pub last_successful_probe_at: Option<ganbaru_chat_contracts::models::UtcTimestamp>,
    pub model_catalog: Option<ProviderModelCatalog>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSettingsRead {
    pub configuration: ChatVaultConfig,
    pub provider_families: Vec<ProviderFamilyMetadataRead>,
    pub provider_instances: Vec<ProviderInstanceRead>,
    pub credential_store_availability: CredentialStoreAvailability,
    pub last_selected_thread_id: Option<ChatThreadId>,
}

fn config_error(message: String) -> ganbaru_chat_contracts::models::ChatError {
    ganbaru_chat_contracts::models::ChatError::new(
        ganbaru_chat_contracts::models::ChatErrorCode::ConfigurationInvalid,
        message,
        true,
    )
}

fn read_chat_config(app: &tauri::AppHandle) -> ChatResult<ChatVaultConfig> {
    let raw = vault::vault_read_config(app.clone()).map_err(config_error)?;
    let root: Value = serde_json::from_str(&raw)
        .map_err(|error| config_error(format!("Chat config is invalid: {error}")))?;
    parse_chat_config_branch(&root)
}

fn read_settings(app: &tauri::AppHandle) -> ChatResult<ChatSettingsRead> {
    Ok(ChatSettingsRead {
        configuration: read_chat_config(app)?,
        provider_families: Vec::new(),
        provider_instances: Vec::new(),
        credential_store_availability: CredentialStoreAvailability::Unavailable,
        last_selected_thread_id: None,
    })
}

fn mutate_chat_config(
    app: &tauri::AppHandle,
    mutate: impl FnOnce(&mut ChatVaultConfig),
) -> ChatResult<ChatVaultConfig> {
    vault::mutate_active_vault_config(
        app,
        |root| {
            let mut config = parse_chat_config_branch(root)?;
            mutate(&mut config);
            replace_chat_config_branch(root, config.clone())?;
            Ok(config)
        },
        config_error,
    )
}

#[tauri::command]
pub fn chat_read_settings(app: tauri::AppHandle) -> ChatResult<ChatSettingsRead> {
    read_settings(&app)
}

#[tauri::command]
pub fn chat_discover_default_providers(app: tauri::AppHandle) -> ChatResult<ChatSettingsRead> {
    read_settings(&app)
}

#[tauri::command]
pub fn chat_update_behavior(
    app: tauri::AppHandle,
    behavior: ChatBehaviorPreferences,
) -> ChatResult<ChatVaultConfig> {
    mutate_chat_config(&app, |config| config.behavior = behavior)
}

#[tauri::command]
pub fn chat_update_panels(
    app: tauri::AppHandle,
    panels: ChatPanelPreferences,
) -> ChatResult<ChatVaultConfig> {
    mutate_chat_config(&app, |config| config.panels = panels)
}
