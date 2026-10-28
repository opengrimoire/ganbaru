//! Provider-free Chat settings used by mobile clients.

use crate::chat::configuration::{
    read_chat_configuration, update_chat_device_config, with_behavior, write_chat_behavior,
};
use ganbaru_chat::credentials::CredentialStoreAvailability;
use ganbaru_chat_contracts::config::{
    ChatBehaviorPreferences, ChatConfiguration, ChatPanelPreferences,
};
use ganbaru_chat_contracts::models::{
    ChatResult, ChatThreadId, ProviderFamilyMetadataRead, ProviderInstanceConfig,
    ProviderModelCatalog, ProviderProbeResult,
};
use serde::Serialize;

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
    pub configuration: ChatConfiguration,
    pub provider_families: Vec<ProviderFamilyMetadataRead>,
    pub provider_instances: Vec<ProviderInstanceRead>,
    pub credential_store_availability: CredentialStoreAvailability,
    pub last_selected_thread_id: Option<ChatThreadId>,
}

async fn read_settings(app: &tauri::AppHandle) -> ChatResult<ChatSettingsRead> {
    Ok(ChatSettingsRead {
        configuration: read_chat_configuration(app).await?,
        provider_families: Vec::new(),
        provider_instances: Vec::new(),
        credential_store_availability: CredentialStoreAvailability::Unavailable,
        last_selected_thread_id: None,
    })
}

#[tauri::command]
pub async fn chat_read_settings(app: tauri::AppHandle) -> ChatResult<ChatSettingsRead> {
    read_settings(&app).await
}

#[tauri::command]
pub async fn chat_discover_default_providers(
    app: tauri::AppHandle,
) -> ChatResult<ChatSettingsRead> {
    read_settings(&app).await
}

/// Stores the portable Chat behavior preferences, which replicate to the vault's other devices.
#[tauri::command]
pub async fn chat_update_behavior(
    app: tauri::AppHandle,
    behavior: ChatBehaviorPreferences,
) -> ChatResult<ChatConfiguration> {
    write_chat_behavior(&app, &behavior).await?;
    read_chat_configuration(&app).await
}

#[tauri::command]
pub async fn chat_update_panels(
    app: tauri::AppHandle,
    panels: ChatPanelPreferences,
) -> ChatResult<ChatConfiguration> {
    let device = update_chat_device_config(&app, |config| {
        config.panels = panels;
        Ok(())
    })?;
    with_behavior(&app, device).await
}
