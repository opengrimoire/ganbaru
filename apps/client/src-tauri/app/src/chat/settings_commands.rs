//! Narrow provider and Chat preference commands for the existing Settings UI.

use super::credentials::{
    CredentialStore, CredentialStoreAvailability, PlatformCredentialStore, SecretValue,
    materialize_provider_environment,
};
#[cfg(test)]
use super::device_state::ChatProviderDeviceState;
use super::device_state::{read_active_device_scope, update_active_device_scope};
pub(crate) use super::settings::read_provider;
use super::settings::{
    apply_provider_probe, config_io_error, credential_error, device_configuration,
    device_state_error, discover_default_providers, discover_default_providers_once,
    executable_search_directories, invalidate_provider_state_for_credential,
    mark_discovery_finished, mutate_chat_config, operation_context, pick_local_path,
    portable_configuration, provider_instance_read, provider_mut, provider_not_found_error,
    provider_runtime_changed, read_chat_config, read_settings, remember_composer_selection,
    replacement_provider_configurations, set_working_folder_provider_preference, unique_model_ids,
    validate_picker_title,
};
#[cfg(test)]
use super::settings::{
    default_provider_configuration, installed_provider_executables,
    provider_family_is_discoverable, should_discover_default_provider,
};
use crate::vault;
use ganbaru_chat_contracts::config::{
    ChatBehaviorPreferences, ChatPanelPreferences, ChatVaultConfig, RememberedComposerSelection,
};
#[cfg(test)]
use ganbaru_chat_contracts::models::ProviderFamilyId;
use ganbaru_chat_contracts::models::{
    ChatErrorCode, ChatResult, ChatThreadId, CredentialReferenceId, ModelId, ProbeState,
    ProjectWorkingFolderId, ProviderFamilyMetadataRead, ProviderInstanceConfig, ProviderInstanceId,
    ProviderModelCatalog, ProviderProbeResult,
};
use ganbaru_chat_providers::{ProviderDriverFactory, ProviderDriverRegistry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Mutex;

#[derive(Default)]
pub struct ChatSettingsState {
    pub(crate) mutation_lock: Mutex<()>,
    pub(crate) discovery_lock: tokio::sync::Mutex<()>,
    pub(crate) discovered_vaults: Mutex<BTreeSet<String>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderInstanceRequest {
    pub configuration: ProviderInstanceConfig,
}

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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveProviderResult {
    pub removed: bool,
    pub credential_cleanup_failed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSetupTestRead {
    pub probe: ProviderProbeResult,
    pub model_catalog: Option<ProviderModelCatalog>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRefreshResult {
    pub families_scanned: u32,
    pub providers_checked: u32,
    pub providers_discovered: u32,
    pub issues: u32,
}

#[tauri::command]
pub async fn chat_read_settings(app: tauri::AppHandle) -> ChatResult<ChatSettingsRead> {
    read_settings(&app)
}

#[tauri::command]
pub async fn chat_discover_default_providers(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
) -> ChatResult<ChatSettingsRead> {
    discover_default_providers_once(&app, &state).await?;
    read_settings(&app)
}

#[tauri::command]
pub async fn chat_refresh_all_providers(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
) -> ChatResult<ProviderRefreshResult> {
    let _guard = state.discovery_lock.lock().await;
    let discovery = discover_default_providers(&app, &state).await?;
    let vault_id = vault::active_vault_id(&app).map_err(config_io_error)?;
    mark_discovery_finished(&state, vault_id)?;

    let provider_ids = read_chat_config(&app)?
        .providers
        .into_iter()
        .map(|provider| provider.instance_id)
        .collect::<Vec<_>>();
    let mut issues = 0;
    for instance_id in &provider_ids {
        let probe = match discovery.discovered_probes.get(instance_id) {
            Some(probe) => Ok(probe.clone()),
            None => chat_probe_provider(app.clone(), instance_id.clone()).await,
        };
        if probe.is_err() || probe.is_ok_and(|probe| probe.state != ProbeState::Healthy) {
            issues += 1;
        }
    }

    Ok(ProviderRefreshResult {
        families_scanned: discovery.families_scanned,
        providers_checked: provider_ids.len() as u32,
        providers_discovered: discovery.discovered_probes.len() as u32,
        issues,
    })
}

#[tauri::command]
pub fn chat_set_last_selected_thread(
    app: tauri::AppHandle,
    thread_id: Option<ChatThreadId>,
) -> ChatResult<()> {
    update_active_device_scope(&app, |scope| {
        scope.preferences.last_selected_thread_id = thread_id;
        Ok(())
    })
    .map_err(device_state_error)
}

#[tauri::command]
pub fn chat_save_provider(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    request: SaveProviderInstanceRequest,
) -> ChatResult<ProviderInstanceRead> {
    ProviderDriverRegistry.create_driver(request.configuration.clone())?;
    let previous = match read_provider(&app, &request.configuration.instance_id) {
        Ok(read) => Some(read.configuration),
        Err(error) if error.code == ChatErrorCode::NotFound => None,
        Err(error) => return Err(error),
    };
    let runtime_changed = previous
        .as_ref()
        .is_none_or(|previous| provider_runtime_changed(previous, &request.configuration));
    let portable = portable_configuration(&request.configuration);
    let device = device_configuration(&request.configuration);
    mutate_chat_config(&app, &state, |config| {
        config
            .automatic_provider_setup_disabled
            .remove(&portable.family_id);
        if let Some(existing) = config
            .providers
            .iter_mut()
            .find(|candidate| candidate.instance_id == portable.instance_id)
        {
            *existing = portable.clone();
        } else {
            config.providers.push(portable.clone());
        }
        Ok(())
    })?;
    update_active_device_scope(&app, |scope| {
        let existing = scope
            .provider_instances
            .entry(request.configuration.instance_id.clone())
            .or_default();
        existing.executable_path = device.executable_path.clone();
        existing.provider_home_path = device.provider_home_path.clone();
        if runtime_changed {
            existing.last_probe = None;
            if let Some(catalog) = existing.model_catalog.as_mut() {
                catalog.stale = true;
            }
        }
        Ok(())
    })
    .map_err(device_state_error)?;
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    Ok(provider_instance_read(&portable, &scope.provider_instances))
}

#[tauri::command]
pub fn chat_set_provider_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    instance_id: ProviderInstanceId,
    enabled: bool,
) -> ChatResult<ProviderInstanceRead> {
    mutate_chat_config(&app, &state, |config| {
        provider_mut(config, &instance_id)?.enabled = enabled;
        Ok(())
    })?;
    read_provider(&app, &instance_id)
}

#[tauri::command]
pub fn chat_remove_provider(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    instance_id: ProviderInstanceId,
) -> ChatResult<RemoveProviderResult> {
    let current = read_chat_config(&app)?;
    let provider = current
        .providers
        .iter()
        .find(|candidate| candidate.instance_id == instance_id)
        .cloned()
        .ok_or_else(provider_not_found_error)?;
    mutate_chat_config(&app, &state, |config| {
        config
            .providers
            .retain(|candidate| candidate.instance_id != instance_id);
        if !config
            .providers
            .iter()
            .any(|candidate| candidate.family_id == provider.family_id)
        {
            config
                .automatic_provider_setup_disabled
                .insert(provider.family_id.clone());
        }
        config
            .remembered_selections
            .retain(|selection| selection.provider_instance_id != instance_id);
        config
            .working_folder_provider_preferences
            .retain(|_, provider_id| provider_id != &instance_id);
        Ok(())
    })?;
    update_active_device_scope(&app, |scope| {
        scope.provider_instances.remove(&instance_id);
        Ok(())
    })
    .map_err(device_state_error)?;
    let store = PlatformCredentialStore::default();
    let mut credential_cleanup_failed = false;
    for reference in provider.credential_references.values() {
        if store.remove(reference).is_err() {
            credential_cleanup_failed = true;
        }
    }
    Ok(RemoveProviderResult {
        removed: true,
        credential_cleanup_failed,
    })
}

#[tauri::command]
pub async fn chat_test_provider(
    request: SaveProviderInstanceRequest,
) -> ChatResult<ProviderSetupTestRead> {
    let configuration = materialize_provider_environment(
        &request.configuration,
        &PlatformCredentialStore::default(),
    )?;
    let mut driver = ProviderDriverRegistry.create_driver(configuration)?;
    let probe = driver.probe(&operation_context("test-provider")).await?;
    let model_catalog = if probe.state == ProbeState::Healthy {
        Some(
            driver
                .discover_models(&operation_context("test-provider-models"))
                .await?
                .without_deprecated_models(),
        )
    } else {
        None
    };
    Ok(ProviderSetupTestRead {
        probe,
        model_catalog,
    })
}

#[tauri::command]
pub async fn chat_probe_provider(
    app: tauri::AppHandle,
    instance_id: ProviderInstanceId,
) -> ChatResult<ProviderProbeResult> {
    let read = read_provider(&app, &instance_id)?;
    let original_configuration = read.configuration;
    let original =
        probe_provider_configuration(original_configuration.clone(), "probe-provider").await?;
    let selected = if original.probe.state == ProbeState::ExecutableMissing {
        discover_replacement_provider_executable(&original_configuration)
            .await?
            .unwrap_or(original)
    } else {
        original
    };
    let probe = selected.probe;
    let model_catalog = selected.model_catalog;
    update_active_device_scope(&app, |scope| {
        let device = scope.provider_instances.entry(instance_id).or_default();
        let executable = selected.configuration.executable.trim();
        device.executable_path = (!executable.is_empty()).then(|| executable.to_string());
        apply_provider_probe(device, &probe, model_catalog.clone());
        Ok(())
    })
    .map_err(device_state_error)?;
    Ok(probe)
}

struct ProviderProbeAttempt {
    configuration: ProviderInstanceConfig,
    probe: ProviderProbeResult,
    model_catalog: Option<ProviderModelCatalog>,
}

async fn probe_provider_configuration(
    configuration: ProviderInstanceConfig,
    operation_id: &str,
) -> ChatResult<ProviderProbeAttempt> {
    let materialized =
        materialize_provider_environment(&configuration, &PlatformCredentialStore::default())?;
    let mut driver = ProviderDriverRegistry.create_driver(materialized)?;
    let probe = driver.probe(&operation_context(operation_id)).await?;
    Ok(ProviderProbeAttempt {
        configuration,
        probe,
        model_catalog: driver.cached_model_catalog(),
    })
}

async fn discover_replacement_provider_executable(
    configuration: &ProviderInstanceConfig,
) -> ChatResult<Option<ProviderProbeAttempt>> {
    let Some(metadata) = ProviderDriverRegistry
        .list_metadata()
        .into_iter()
        .find(|metadata| metadata.family_id == configuration.family_id)
    else {
        return Ok(None);
    };
    let replacements = replacement_provider_configurations(
        &metadata,
        configuration,
        &executable_search_directories(),
    );
    let mut fallback = None;
    for replacement in replacements {
        let Ok(attempt) =
            probe_provider_configuration(replacement, "rediscover-provider-executable").await
        else {
            continue;
        };
        if attempt.probe.state == ProbeState::ExecutableMissing {
            continue;
        }
        if matches!(
            attempt.probe.state,
            ProbeState::Healthy | ProbeState::AuthenticationRequired
        ) {
            return Ok(Some(attempt));
        }
        fallback.get_or_insert(attempt);
    }
    Ok(fallback)
}

#[tauri::command]
pub async fn chat_refresh_provider_models(
    app: tauri::AppHandle,
    instance_id: ProviderInstanceId,
) -> ChatResult<ProviderModelCatalog> {
    let read = read_provider(&app, &instance_id)?;
    let configuration =
        materialize_provider_environment(&read.configuration, &PlatformCredentialStore::default())?;
    let mut driver = ProviderDriverRegistry.create_driver(configuration)?;
    let catalog = driver
        .discover_models(&operation_context("discover-provider-models"))
        .await?
        .without_deprecated_models();
    update_active_device_scope(&app, |scope| {
        scope
            .provider_instances
            .entry(instance_id)
            .or_default()
            .model_catalog = Some(catalog.clone());
        Ok(())
    })
    .map_err(device_state_error)?;
    Ok(catalog)
}

#[tauri::command]
pub fn chat_update_provider_models(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    instance_id: ProviderInstanceId,
    visible_model_ids: Vec<ModelId>,
    favorite_model_ids: Vec<ModelId>,
) -> ChatResult<ProviderInstanceRead> {
    let visible = unique_model_ids(visible_model_ids);
    let favorites = unique_model_ids(favorite_model_ids);
    mutate_chat_config(&app, &state, |config| {
        let provider = provider_mut(config, &instance_id)?;
        provider.visible_model_ids = visible;
        provider.favorite_model_ids = favorites;
        Ok(())
    })?;
    read_provider(&app, &instance_id)
}

#[tauri::command]
pub fn chat_update_behavior(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    behavior: ChatBehaviorPreferences,
) -> ChatResult<ChatVaultConfig> {
    mutate_chat_config(&app, &state, |config| {
        config.behavior = behavior;
        Ok(())
    })
}

#[tauri::command]
pub fn chat_update_panels(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    panels: ChatPanelPreferences,
) -> ChatResult<ChatVaultConfig> {
    mutate_chat_config(&app, &state, |config| {
        config.panels = panels;
        Ok(())
    })
}

#[tauri::command]
pub fn chat_set_working_folder_provider_preference(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    working_folder_id: ProjectWorkingFolderId,
    instance_id: Option<ProviderInstanceId>,
) -> ChatResult<ChatVaultConfig> {
    mutate_chat_config(&app, &state, |config| {
        set_working_folder_provider_preference(config, working_folder_id, instance_id)
    })
}

#[tauri::command]
pub fn chat_remember_composer_selection(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    selection: RememberedComposerSelection,
) -> ChatResult<ChatVaultConfig> {
    mutate_chat_config(&app, &state, |config| {
        remember_composer_selection(config, selection)
    })
}

#[tauri::command]
pub fn chat_replace_credential(
    app: tauri::AppHandle,
    reference_id: CredentialReferenceId,
    secret: String,
) -> ChatResult<()> {
    let secret = SecretValue::new(secret).map_err(credential_error)?;
    PlatformCredentialStore::default()
        .replace(&reference_id, &secret)
        .map_err(credential_error)?;
    invalidate_provider_state_for_credential(&app, &reference_id)
}

#[tauri::command]
pub fn chat_remove_credential(
    app: tauri::AppHandle,
    reference_id: CredentialReferenceId,
) -> ChatResult<bool> {
    let removed = PlatformCredentialStore::default()
        .remove(&reference_id)
        .map_err(credential_error)?;
    invalidate_provider_state_for_credential(&app, &reference_id)?;
    Ok(removed)
}

#[tauri::command]
pub async fn chat_pick_provider_executable(
    app: tauri::AppHandle,
    title: String,
) -> ChatResult<Option<String>> {
    pick_local_path(&app, false, validate_picker_title(&title)?).await
}

#[tauri::command]
pub async fn chat_pick_provider_home(
    app: tauri::AppHandle,
    title: String,
) -> ChatResult<Option<String>> {
    pick_local_path(&app, true, validate_picker_title(&title)?).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_configurations_use_the_installed_cli_and_native_account_home() {
        for metadata in ProviderDriverRegistry
            .list_metadata()
            .into_iter()
            .filter(provider_family_is_discoverable)
        {
            let instance_id = ProviderInstanceId::new(metadata.family_id.as_str()).unwrap();
            let executable = format!("/installed/{}", metadata.family_id.as_str());
            let configuration =
                default_provider_configuration(&metadata, instance_id, executable.clone()).unwrap();

            assert_eq!(
                configuration.instance_id.as_str(),
                metadata.family_id.as_str()
            );
            assert_eq!(configuration.family_id, metadata.family_id);
            assert_eq!(configuration.label, metadata.display_name);
            assert_eq!(configuration.executable, executable);
            assert_eq!(configuration.provider_home, None);
            assert!(configuration.environment.is_empty());
            assert!(configuration.credential_references.is_empty());
            ProviderDriverRegistry
                .create_driver(configuration)
                .expect("each built-in provider configuration must remain valid");
        }
    }

    #[test]
    fn an_existing_family_prevents_automatic_replacement() {
        let mut config = ChatVaultConfig::default();
        let metadata = ProviderDriverRegistry
            .list_metadata()
            .into_iter()
            .find(|metadata| metadata.family_id.as_str() == "claude")
            .unwrap();
        assert!(should_discover_default_provider(
            &config,
            &metadata.family_id
        ));

        config.providers.push(portable_configuration(
            &default_provider_configuration(
                &metadata,
                ProviderInstanceId::new("claude-custom").unwrap(),
                "/installed/claude".to_string(),
            )
            .unwrap(),
        ));

        assert!(!should_discover_default_provider(
            &config,
            &metadata.family_id
        ));
    }

    #[test]
    fn removing_the_default_family_can_disable_automatic_replacement() {
        let mut config = ChatVaultConfig::default();
        config
            .automatic_provider_setup_disabled
            .insert(ProviderFamilyId::new("claude").unwrap());

        assert!(!should_discover_default_provider(
            &config,
            &ProviderFamilyId::new("claude").unwrap()
        ));
    }

    #[test]
    fn successful_provider_probe_persists_its_model_catalog() {
        let probe: ProviderProbeResult = serde_json::from_value(serde_json::json!({
            "instanceId": "claude",
            "state": "healthy",
            "version": "2.1.218",
            "accountLabel": null,
            "capabilities": { "entries": [] },
            "authoritySupport": ganbaru_chat_contracts::models::ProviderAuthoritySupport::default(),
            "checkedAt": "2026-07-23T03:18:50.240Z",
            "detail": null
        }))
        .unwrap();
        let catalog: ProviderModelCatalog = serde_json::from_value(serde_json::json!({
            "instanceId": "claude",
            "models": [],
            "source": "provider",
            "discoveredAt": "2026-07-23T03:18:50.240Z",
            "stale": false
        }))
        .unwrap();
        let mut device = ChatProviderDeviceState::default();

        apply_provider_probe(&mut device, &probe, Some(catalog.clone()));

        assert_eq!(device.last_probe, Some(probe.clone()));
        assert_eq!(device.last_successful_probe_at, Some(probe.checked_at));
        assert_eq!(device.model_catalog, Some(catalog));
    }

    #[test]
    fn successful_provider_probe_drops_deprecated_models() {
        let probe: ProviderProbeResult = serde_json::from_value(serde_json::json!({
            "instanceId": "codex",
            "state": "healthy",
            "version": "1.0.0",
            "accountLabel": null,
            "capabilities": { "entries": [] },
            "authoritySupport": ganbaru_chat_contracts::models::ProviderAuthoritySupport::default(),
            "checkedAt": "2026-07-23T03:18:50.240Z",
            "detail": null
        }))
        .unwrap();
        let catalog: ProviderModelCatalog = serde_json::from_value(serde_json::json!({
            "instanceId": "codex",
            "models": [{
                "id": "gpt-current",
                "displayName": "GPT Current",
                "description": null,
                "contextLimit": null,
                "availability": "available",
                "capabilities": [],
                "options": [],
                "custom": false
            }, {
                "id": "gpt-old",
                "displayName": "GPT Old",
                "description": null,
                "contextLimit": null,
                "availability": "deprecated",
                "capabilities": [],
                "options": [],
                "custom": false
            }],
            "source": "provider",
            "discoveredAt": "2026-07-23T03:18:50.240Z",
            "stale": false
        }))
        .unwrap();
        let mut device = ChatProviderDeviceState::default();

        apply_provider_probe(&mut device, &probe, Some(catalog));

        let models = &device.model_catalog.as_ref().unwrap().models;
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id.as_str(), "gpt-current");
    }

    #[cfg(unix)]
    #[test]
    fn installed_provider_search_uses_metadata_candidates_in_order() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "ganbaru-provider-discovery-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let second = directory.join("second-provider");
        let first = directory.join("first-provider");
        for executable in [&second, &first] {
            std::fs::write(executable, "#!/bin/sh\nexit 0\n").unwrap();
            let mut permissions = std::fs::metadata(executable).unwrap().permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(executable, permissions).unwrap();
        }
        let mut metadata = ProviderDriverRegistry.list_metadata().remove(0);
        metadata.default_executable_candidates =
            vec!["first-provider".to_string(), "second-provider".to_string()];

        let found = installed_provider_executables(&metadata, std::slice::from_ref(&directory));

        assert_eq!(
            found,
            vec![
                first.to_string_lossy().into_owned(),
                second.to_string_lossy().into_owned()
            ]
        );

        let mut configuration = default_provider_configuration(
            &metadata,
            ProviderInstanceId::new("provider-custom").unwrap(),
            "/missing/provider".to_string(),
        )
        .unwrap();
        configuration.provider_home = Some("/custom/provider-home".to_string());
        configuration.launch_arguments = vec!["custom-argument".to_string()];
        let replacements = replacement_provider_configurations(
            &metadata,
            &configuration,
            std::slice::from_ref(&directory),
        );
        let expected = found
            .into_iter()
            .map(|executable| ProviderInstanceConfig {
                executable,
                ..configuration.clone()
            })
            .collect::<Vec<_>>();

        assert_eq!(replacements, expected);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
