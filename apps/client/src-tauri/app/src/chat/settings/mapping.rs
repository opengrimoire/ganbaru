use super::{device_state_error, provider_not_found_error, store::read_chat_config};
use crate::chat::device_state::{ChatProviderDeviceState, read_active_device_scope};
use crate::chat::settings_commands::ProviderInstanceRead;
use ganbaru_chat_contracts::config::ChatPortableProviderConfig;
use ganbaru_chat_contracts::models::{
    ChatResult, ProviderInstanceConfig, ProviderInstanceId, ProviderModelCatalog,
};
use std::collections::BTreeMap;

pub(crate) fn read_provider(
    app: &tauri::AppHandle,
    instance_id: &ProviderInstanceId,
) -> ChatResult<ProviderInstanceRead> {
    let config = read_chat_config(app)?;
    let portable = config
        .providers
        .iter()
        .find(|candidate| &candidate.instance_id == instance_id)
        .ok_or_else(provider_not_found_error)?;
    let scope = read_active_device_scope(app).map_err(device_state_error)?;
    Ok(provider_instance_read(portable, &scope.provider_instances))
}

pub(crate) fn provider_instance_read(
    portable: &ChatPortableProviderConfig,
    device_instances: &BTreeMap<ProviderInstanceId, ChatProviderDeviceState>,
) -> ProviderInstanceRead {
    let device = device_instances.get(&portable.instance_id);
    ProviderInstanceRead {
        configuration: ProviderInstanceConfig {
            schema_version: portable.schema_version,
            instance_id: portable.instance_id.clone(),
            family_id: portable.family_id.clone(),
            label: portable.label.clone(),
            enabled: portable.enabled,
            executable: device
                .and_then(|entry| entry.executable_path.clone())
                .unwrap_or_default(),
            provider_home: device.and_then(|entry| entry.provider_home_path.clone()),
            launch_arguments: portable.launch_arguments.clone(),
            environment: portable.environment.clone(),
            credential_references: portable.credential_references.clone(),
            visible_model_ids: portable.visible_model_ids.clone(),
            favorite_model_ids: portable.favorite_model_ids.clone(),
            provider_config: portable.provider_config.clone(),
            internal_mcp: None,
            unknown_fields: portable.unknown_fields.clone(),
        },
        last_probe: device.and_then(|entry| entry.last_probe.clone()),
        last_successful_probe_at: device.and_then(|entry| entry.last_successful_probe_at.clone()),
        model_catalog: device
            .and_then(|entry| entry.model_catalog.clone())
            .map(ProviderModelCatalog::without_deprecated_models),
    }
}

pub(crate) fn portable_configuration(
    config: &ProviderInstanceConfig,
) -> ChatPortableProviderConfig {
    ChatPortableProviderConfig {
        schema_version: config.schema_version,
        instance_id: config.instance_id.clone(),
        family_id: config.family_id.clone(),
        label: config.label.clone(),
        enabled: config.enabled,
        launch_arguments: config.launch_arguments.clone(),
        environment: config.environment.clone(),
        credential_references: config.credential_references.clone(),
        visible_model_ids: config.visible_model_ids.clone(),
        favorite_model_ids: config.favorite_model_ids.clone(),
        provider_config: config.provider_config.clone(),
        unknown_fields: config.unknown_fields.clone(),
    }
}

pub(crate) fn device_configuration(config: &ProviderInstanceConfig) -> ChatProviderDeviceState {
    ChatProviderDeviceState {
        executable_path: (!config.executable.trim().is_empty())
            .then(|| config.executable.trim().to_string()),
        provider_home_path: config
            .provider_home
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        last_probe: None,
        last_successful_probe_at: None,
        model_catalog: None,
    }
}

pub(crate) fn provider_runtime_changed(
    previous: &ProviderInstanceConfig,
    next: &ProviderInstanceConfig,
) -> bool {
    previous.family_id != next.family_id
        || previous.executable != next.executable
        || previous.provider_home != next.provider_home
        || previous.launch_arguments != next.launch_arguments
        || previous.environment != next.environment
        || previous.credential_references != next.credential_references
        || previous.provider_config != next.provider_config
}
