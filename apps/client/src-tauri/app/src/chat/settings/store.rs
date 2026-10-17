use super::{
    config_io_error, config_shape_error, device_state_error, mapping::provider_instance_read,
    provider_not_found_error,
};
use crate::chat::credentials::{CredentialStore, PlatformCredentialStore};
use crate::chat::device_state::read_active_device_scope;
use crate::chat::settings_commands::{ChatSettingsRead, ChatSettingsState};
use crate::vault;
use ganbaru_chat_contracts::config::{
    ChatPortableProviderConfig, ChatVaultConfig, parse_chat_config_branch,
    replace_chat_config_branch,
};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult, ProviderInstanceId};
use ganbaru_chat_providers::ProviderDriverRegistry;
use serde_json::Value;

pub(crate) fn read_settings(app: &tauri::AppHandle) -> ChatResult<ChatSettingsRead> {
    let configuration = read_chat_config(app)?;
    let scope = read_active_device_scope(app).map_err(device_state_error)?;
    let provider_instances = configuration
        .providers
        .iter()
        .map(|portable| provider_instance_read(portable, &scope.provider_instances))
        .collect();
    Ok(ChatSettingsRead {
        configuration,
        provider_families: ProviderDriverRegistry.list_metadata(),
        provider_instances,
        credential_store_availability: PlatformCredentialStore::default().availability(),
        last_selected_thread_id: scope.preferences.last_selected_thread_id,
    })
}

pub(crate) fn mutate_chat_config(
    app: &tauri::AppHandle,
    state: &ChatSettingsState,
    mutate: impl FnOnce(&mut ChatVaultConfig) -> ChatResult<()>,
) -> ChatResult<ChatVaultConfig> {
    let _guard = state.mutation_lock.lock().map_err(|_| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Chat settings are unavailable",
            true,
        )
    })?;
    vault::mutate_active_vault_config(
        app,
        |root| {
            let mut config = parse_chat_config_branch(root)?;
            mutate(&mut config)?;
            config.validate()?;
            replace_chat_config_branch(root, config.clone())?;
            Ok(config)
        },
        config_io_error,
    )
}

pub(crate) fn read_chat_config(app: &tauri::AppHandle) -> ChatResult<ChatVaultConfig> {
    let raw = vault::vault_read_config(app.clone()).map_err(config_io_error)?;
    let root: Value = serde_json::from_str(&raw).map_err(|_| config_shape_error())?;
    parse_chat_config_branch(&root)
}

pub(crate) fn provider_mut<'a>(
    config: &'a mut ChatVaultConfig,
    instance_id: &ProviderInstanceId,
) -> ChatResult<&'a mut ChatPortableProviderConfig> {
    config
        .providers
        .iter_mut()
        .find(|provider| &provider.instance_id == instance_id)
        .ok_or_else(provider_not_found_error)
}
