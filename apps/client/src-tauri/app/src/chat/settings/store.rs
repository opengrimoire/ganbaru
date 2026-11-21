use super::{
    device_state_error,
    discovery::{drop_met_install_requirements, installed_family_executables},
    duplicates::duplicate_configurations,
    executable_search_directories,
    mapping::provider_instance_read,
    provider_not_found_error,
};
use crate::chat::configuration::{
    on_blocking_pool, read_chat_configuration, update_chat_device_config, with_behavior,
};
use crate::chat::credentials::{CredentialStore, PlatformCredentialStore};
use crate::chat::device_state::read_active_device_scope;
use crate::chat::settings_commands::{ChatSettingsRead, ChatSettingsState};
use ganbaru_chat_contracts::config::{
    ChatConfiguration, ChatDeviceConfig, ChatPortableProviderConfig,
};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult, ProviderInstanceId};
use ganbaru_chat_providers::ProviderDriverRegistry;
use tauri::Manager;

pub(crate) async fn read_settings(app: &tauri::AppHandle) -> ChatResult<ChatSettingsRead> {
    let configuration = read_chat_configuration(app).await?;
    let scope = read_active_device_scope(app).map_err(device_state_error)?;
    let mut provider_instances: Vec<_> = configuration
        .device
        .providers
        .iter()
        .map(|portable| provider_instance_read(portable, &scope.provider_instances))
        .collect();
    let search_directories = executable_search_directories();
    let configurations: Vec<_> = provider_instances
        .iter()
        .map(|read| read.configuration.clone())
        .collect();
    let mut duplicates = duplicate_configurations(&configurations, &search_directories);
    for read in &mut provider_instances {
        read.duplicate_of = duplicates.remove(&read.configuration.instance_id);
    }
    let mut provider_families = ProviderDriverRegistry.list_metadata();
    drop_met_install_requirements(&mut provider_families, &search_directories);
    let installed_executables =
        installed_family_executables(&provider_families, &search_directories);
    Ok(ChatSettingsRead {
        configuration,
        provider_families,
        provider_instances,
        installed_executables,
        credential_store_availability: PlatformCredentialStore::default().availability(),
        last_selected_thread_id: scope.preferences.last_selected_thread_id,
    })
}

/// Applies `mutate` to the device Chat settings under the settings mutation lock, which keeps
/// read-modify-write command sequences on this device in order.
pub(crate) fn mutate_chat_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &ChatSettingsState,
    mutate: impl FnOnce(&mut ChatDeviceConfig) -> ChatResult<()>,
) -> ChatResult<ChatDeviceConfig> {
    let _guard = state.mutation_lock.lock().map_err(|_| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Chat settings are unavailable",
            true,
        )
    })?;
    update_chat_device_config(app, mutate)
}

/// Applies `mutate` like [`mutate_chat_config`] on the blocking pool, so an async command never
/// holds the async runtime during the lock and file I/O, and returns the device settings with
/// the behavior preferences.
pub(crate) async fn mutate_chat_configuration(
    app: &tauri::AppHandle,
    mutate: impl FnOnce(&mut ChatDeviceConfig) -> ChatResult<()> + Send + 'static,
) -> ChatResult<ChatConfiguration> {
    let handle = app.clone();
    let device = on_blocking_pool(move || {
        let state = handle.state::<ChatSettingsState>();
        mutate_chat_config(&handle, &state, mutate)
    })
    .await?;
    with_behavior(app, device).await
}

pub(crate) fn provider_mut<'a>(
    config: &'a mut ChatDeviceConfig,
    instance_id: &ProviderInstanceId,
) -> ChatResult<&'a mut ChatPortableProviderConfig> {
    config
        .providers
        .iter_mut()
        .find(|provider| &provider.instance_id == instance_id)
        .ok_or_else(provider_not_found_error)
}
