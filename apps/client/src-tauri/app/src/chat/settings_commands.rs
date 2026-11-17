//! Narrow provider and Chat preference commands for the existing Settings UI.

use super::credentials::{
    CredentialStore, CredentialStoreAvailability, PlatformCredentialStore, SecretValue,
    is_provider_credential_reference, provider_credential_reference,
};
#[cfg(test)]
use super::device_state::ChatProviderDeviceState;
use super::device_state::{read_active_device_scope, update_active_device_scope};
#[cfg(all(test, unix))]
use super::settings::installed_provider_executables;
pub(crate) use super::settings::read_provider;
use super::settings::{
    CHAT_PROVIDER_ACTIVITY_EVENT, apply_provider_probe, config_io_error, credential_error,
    device_configuration, device_state_error, discover_default_providers,
    discover_default_providers_once, executable_search_directories,
    invalidate_provider_state_for_credential, mark_discovery_finished, mutate_chat_config,
    mutate_chat_configuration, operation_context, pick_local_path, portable_configuration,
    provider_instance_read, provider_mut, provider_not_found_error, provider_runtime_changed,
    read_chat_config, read_settings, remember_composer_selection,
    replacement_provider_configurations, set_working_folder_provider_preference, unique_model_ids,
    validate_picker_title,
};
use super::settings::{
    ProviderRuntimePrompt, ProviderRuntimeReviewRead, RuntimeReceipts, approval_digests,
    approved_runtime, ask_runtime_approval, ingress_review_items, launch_configuration,
    pending_portable_items, remove_private_home, require_runtime_approved_on_device,
    validate_runtime_prompt,
};
#[cfg(test)]
use super::settings::{
    default_provider_configuration, provider_family_is_discoverable,
    should_discover_default_provider,
};
use crate::chat::configuration::{read_chat_configuration, write_chat_behavior};
use crate::vault;
use ganbaru_chat_contracts::config::{
    ChatBehaviorPreferences, ChatConfiguration, ChatPanelPreferences, RememberedComposerSelection,
};
use ganbaru_chat_contracts::models::{
    ChatErrorCode, ChatResult, ChatThreadId, CredentialReferenceId, ModelId, ProbeState,
    ProjectWorkingFolderId, ProviderFamilyId, ProviderFamilyMetadataRead, ProviderInstanceConfig,
    ProviderInstanceId, ProviderModelCatalog, ProviderProbeResult,
};
use ganbaru_chat_providers::runtime_review::DevicePathKind;
use ganbaru_chat_providers::{ProviderDriverFactory, ProviderDriverRegistry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

#[derive(Default)]
pub struct ChatSettingsState {
    pub(crate) mutation_lock: Mutex<()>,
    pub(crate) discovery_lock: tokio::sync::Mutex<()>,
    pub(crate) discovered_vaults: Mutex<BTreeSet<String>>,
    pub(crate) runtime_receipts: RuntimeReceipts,
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
    pub runtime_review: Vec<ProviderRuntimeReviewRead>,
    pub last_probe: Option<ProviderProbeResult>,
    pub last_successful_probe_at: Option<ganbaru_chat_contracts::models::UtcTimestamp>,
    pub model_catalog: Option<ProviderModelCatalog>,
    /// The newest usage limits this device observed for the instance.
    pub usage_limits: Option<crate::chat::device_state::ProviderUsageLimitsObservation>,
    /// When a turn last made the instance the most recently used on this device.
    pub last_used_at: Option<ganbaru_chat_contracts::models::UtcTimestamp>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSettingsRead {
    pub configuration: ChatConfiguration,
    pub provider_families: Vec<ProviderFamilyMetadataRead>,
    pub provider_instances: Vec<ProviderInstanceRead>,
    /// The CLI programs found on this device for each family it could set up, by family; a
    /// family appears with an empty list when its search found nothing, and is absent when
    /// this device does not search for it.
    pub installed_executables: BTreeMap<ProviderFamilyId, Vec<String>>,
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
    read_settings(&app).await
}

/// Age after which opening Chat rereads a provider's status and plan limits; matches when the
/// limits views call a reading out of date.
const PROVIDER_READING_MAX_AGE: chrono::TimeDelta = chrono::TimeDelta::hours(1);

#[tauri::command]
pub async fn chat_discover_default_providers(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
) -> ChatResult<ChatSettingsRead> {
    if discover_default_providers_once(&app, &state).await? {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { refresh_stale_provider_readings(app).await });
    }
    read_settings(&app).await
}

/// Probes, one at a time, each enabled provider this device has not heard from within
/// [`PROVIDER_READING_MAX_AGE`], so plan limits and sign-in state are current without a
/// visit to Settings. It runs once per vault and app session, beside discovery, and stops
/// when the active vault changes.
async fn refresh_stale_provider_readings(app: tauri::AppHandle) {
    let state = app.state::<ChatSettingsState>();
    let _guard = state.discovery_lock.lock().await;
    let (Ok(vault_id), Ok(config), Ok(scope)) = (
        vault::active_vault_id(&app),
        read_chat_config(&app),
        read_active_device_scope(&app),
    ) else {
        eprintln!("Chat could not read providers to refresh their status");
        return;
    };
    let now = chrono::Utc::now();
    let stale = config.providers.into_iter().filter(|provider| {
        provider.enabled
            && !scope
                .provider_instances
                .get(&provider.instance_id)
                .is_some_and(|device| device.has_reading_within(now, PROVIDER_READING_MAX_AGE))
    });
    let mut refreshed = false;
    for provider in stale {
        if vault::active_vault_id(&app).ok().as_ref() != Some(&vault_id) {
            break;
        }
        match chat_probe_provider(app.clone(), provider.instance_id).await {
            Ok(_) => refreshed = true,
            Err(error) => eprintln!("Chat provider status refresh failed: {error}"),
        }
    }
    if refreshed && app.emit(CHAT_PROVIDER_ACTIVITY_EVENT, ()).is_err() {
        eprintln!("Chat provider activity notification failed");
    }
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

/// Saves a provider after this device confirms any runtime values nothing has vouched for.
///
/// Returns `None` when the user cancels the native confirmation.
#[tauri::command]
pub async fn chat_save_provider(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    request: SaveProviderInstanceRequest,
    prompt: ProviderRuntimePrompt,
) -> ChatResult<Option<ProviderInstanceRead>> {
    validate_runtime_prompt(&prompt)?;
    ProviderDriverRegistry.create_driver(request.configuration.clone())?;
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    let pending = ingress_review_items(
        &request.configuration,
        scope
            .provider_instances
            .get(&request.configuration.instance_id),
        &state.runtime_receipts,
    );
    if !pending.is_empty()
        && !ask_runtime_approval(&app, &prompt, &request.configuration.label, &pending).await?
    {
        return Ok(None);
    }
    save_confirmed_provider(&app, &state, &request.configuration).map(Some)
}

fn save_confirmed_provider(
    app: &tauri::AppHandle,
    state: &ChatSettingsState,
    configuration: &ProviderInstanceConfig,
) -> ChatResult<ProviderInstanceRead> {
    let previous = match read_provider(app, &configuration.instance_id) {
        Ok(read) => Some(read.configuration),
        Err(error) if error.code == ChatErrorCode::NotFound => None,
        Err(error) => return Err(error),
    };
    let runtime_changed = previous
        .as_ref()
        .is_none_or(|previous| provider_runtime_changed(previous, configuration));
    let portable = portable_configuration(configuration);
    let device = device_configuration(configuration);
    let approvals = approval_digests(configuration);
    mutate_chat_config(app, state, |config| {
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
    update_active_device_scope(app, |scope| {
        let existing = scope
            .provider_instances
            .entry(configuration.instance_id.clone())
            .or_default();
        existing.executable_path = device.executable_path.clone();
        existing.provider_home_path = device.provider_home_path.clone();
        existing.approved_runtime_items = approvals.clone();
        if runtime_changed {
            existing.last_probe = None;
            existing.usage_limits = None;
            if let Some(catalog) = existing.model_catalog.as_mut() {
                catalog.stale = true;
            }
        }
        Ok(())
    })
    .map_err(device_state_error)?;
    let scope = read_active_device_scope(app).map_err(device_state_error)?;
    Ok(provider_instance_read(&portable, &scope.provider_instances))
}

/// Confirms a stored provider's portable runtime values on this device.
///
/// Returns `None` when the user cancels; approvals stay unchanged in that case.
#[tauri::command]
pub async fn chat_confirm_provider_runtime(
    app: tauri::AppHandle,
    instance_id: ProviderInstanceId,
    prompt: ProviderRuntimePrompt,
) -> ChatResult<Option<ProviderInstanceRead>> {
    validate_runtime_prompt(&prompt)?;
    let read = read_provider(&app, &instance_id)?;
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    let pending = pending_portable_items(
        &read.configuration,
        scope.provider_instances.get(&instance_id),
    );
    if pending.is_empty() {
        return Ok(Some(read));
    }
    if !ask_runtime_approval(&app, &prompt, &read.configuration.label, &pending).await? {
        return Ok(None);
    }
    let approvals = approval_digests(&read.configuration);
    update_active_device_scope(&app, |scope| {
        scope
            .provider_instances
            .entry(instance_id.clone())
            .or_default()
            .approved_runtime_items = approvals.clone();
        Ok(())
    })
    .map_err(device_state_error)?;
    read_provider(&app, &instance_id).map(Some)
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
    remove_private_home(&app, &instance_id);
    let store = PlatformCredentialStore::default();
    let mut credential_cleanup_failed = false;
    for (name, reference) in &provider.credential_references {
        if !is_provider_credential_reference(&provider.instance_id, name, reference) {
            continue;
        }
        if store.remove(reference).is_err() {
            credential_cleanup_failed = true;
        }
    }
    Ok(RemoveProviderResult {
        removed: true,
        credential_cleanup_failed,
    })
}

/// Tests an unsaved provider after this device confirms any runtime values nothing has vouched for.
///
/// A confirmation here is remembered briefly in memory so the following save does not ask again;
/// it never becomes a stored approval. Returns `None` when the user cancels.
#[tauri::command]
pub async fn chat_test_provider(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    request: SaveProviderInstanceRequest,
    prompt: ProviderRuntimePrompt,
) -> ChatResult<Option<ProviderSetupTestRead>> {
    validate_runtime_prompt(&prompt)?;
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    let pending = ingress_review_items(
        &request.configuration,
        scope
            .provider_instances
            .get(&request.configuration.instance_id),
        &state.runtime_receipts,
    );
    if !pending.is_empty() {
        if !ask_runtime_approval(&app, &prompt, &request.configuration.label, &pending).await? {
            return Ok(None);
        }
        state
            .runtime_receipts
            .record_confirmed_items(&request.configuration.instance_id, &pending);
    }
    let configuration = launch_configuration(&app, &request.configuration)?;
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
    Ok(Some(ProviderSetupTestRead {
        probe,
        model_catalog,
    }))
}

#[tauri::command]
pub async fn chat_probe_provider(
    app: tauri::AppHandle,
    instance_id: ProviderInstanceId,
) -> ChatResult<ProviderProbeResult> {
    let read = read_provider(&app, &instance_id)?;
    let original_configuration = read.configuration;
    require_runtime_approved_on_device(&app, &original_configuration)?;
    let original =
        probe_provider_configuration(&app, original_configuration.clone(), "probe-provider")
            .await?;
    let selected = if original.probe.state == ProbeState::ExecutableMissing {
        discover_replacement_provider_executable(&app, &original_configuration)
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
    app: &tauri::AppHandle,
    configuration: ProviderInstanceConfig,
    operation_id: &str,
) -> ChatResult<ProviderProbeAttempt> {
    let materialized = launch_configuration(app, &configuration)?;
    let mut driver = ProviderDriverRegistry.create_driver(materialized)?;
    let probe = driver.probe(&operation_context(operation_id)).await?;
    Ok(ProviderProbeAttempt {
        configuration,
        probe,
        model_catalog: driver.cached_model_catalog(),
    })
}

async fn discover_replacement_provider_executable(
    app: &tauri::AppHandle,
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
            probe_provider_configuration(app, replacement, "rediscover-provider-executable").await
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
    let configuration = approved_runtime(&app, &read.configuration)?;
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

/// Stores the Chat behavior preferences. Portable fields replicate to the vault's other devices.
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
    mutate_chat_configuration(&app, |config| {
        config.panels = panels;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn chat_set_working_folder_provider_preference(
    app: tauri::AppHandle,
    working_folder_id: ProjectWorkingFolderId,
    instance_id: Option<ProviderInstanceId>,
) -> ChatResult<ChatConfiguration> {
    mutate_chat_configuration(&app, |config| {
        set_working_folder_provider_preference(config, working_folder_id, instance_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_remember_composer_selection(
    app: tauri::AppHandle,
    selection: RememberedComposerSelection,
) -> ChatResult<ChatConfiguration> {
    mutate_chat_configuration(&app, |config| {
        remember_composer_selection(config, selection)
    })
    .await
}

/// Store a provider secret under the reference Rust derives for this instance and variable.
#[tauri::command]
pub fn chat_replace_credential(
    app: tauri::AppHandle,
    instance_id: ProviderInstanceId,
    environment_name: String,
    secret: String,
) -> ChatResult<CredentialReferenceId> {
    let reference_id = provider_credential_reference(&instance_id, &environment_name)?;
    let secret = SecretValue::new(secret).map_err(credential_error)?;
    PlatformCredentialStore::default()
        .replace(&reference_id, &secret)
        .map_err(credential_error)?;
    invalidate_provider_state_for_credential(&app, &reference_id)?;
    Ok(reference_id)
}

#[tauri::command]
pub fn chat_remove_credential(
    app: tauri::AppHandle,
    instance_id: ProviderInstanceId,
    environment_name: String,
) -> ChatResult<bool> {
    let reference_id = provider_credential_reference(&instance_id, &environment_name)?;
    let removed = PlatformCredentialStore::default()
        .remove(&reference_id)
        .map_err(credential_error)?;
    invalidate_provider_state_for_credential(&app, &reference_id)?;
    Ok(removed)
}

#[tauri::command]
pub async fn chat_pick_provider_executable(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    title: String,
) -> ChatResult<Option<String>> {
    let picked = pick_local_path(&app, false, validate_picker_title(&title)?).await?;
    if let Some(path) = &picked {
        state
            .runtime_receipts
            .record_picked_path(DevicePathKind::Executable, path);
    }
    Ok(picked)
}

#[tauri::command]
pub async fn chat_pick_provider_home(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatSettingsState>,
    title: String,
) -> ChatResult<Option<String>> {
    let picked = pick_local_path(&app, true, validate_picker_title(&title)?).await?;
    if let Some(path) = &picked {
        state
            .runtime_receipts
            .record_picked_path(DevicePathKind::ProviderHome, path);
    }
    Ok(picked)
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
        let mut config = ganbaru_chat_contracts::config::ChatDeviceConfig::default();
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
        let mut config = ganbaru_chat_contracts::config::ChatDeviceConfig::default();
        config
            .automatic_provider_setup_disabled
            .insert(ProviderFamilyId::new("claude").unwrap());

        assert!(!should_discover_default_provider(
            &config,
            &ProviderFamilyId::new("claude").unwrap()
        ));
    }

    #[test]
    fn provider_read_reports_the_current_driver_authority_over_a_stored_probe() {
        let configuration: ProviderInstanceConfig = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "instanceId": "claude",
            "familyId": "claude",
            "label": "Claude",
            "enabled": true,
            "executable": "claude",
            "providerHome": null,
            "launchArguments": [],
            "environment": {},
            "credentialReferences": {},
            "visibleModelIds": [],
            "favoriteModelIds": [],
            "providerConfig": { "schemaVersion": 1, "value": {} }
        }))
        .unwrap();
        let stale_probe: ProviderProbeResult = serde_json::from_value(serde_json::json!({
            "instanceId": "claude",
            "state": "healthy",
            "version": "2.1.323",
            "accountLabel": null,
            "capabilities": { "entries": [] },
            "authoritySupport": ganbaru_chat_contracts::models::ProviderAuthoritySupport::default(),
            "checkedAt": "2026-07-23T03:18:50.240Z",
            "detail": null
        }))
        .unwrap();
        let device = ChatProviderDeviceState {
            last_probe: Some(stale_probe.clone()),
            ..ChatProviderDeviceState::default()
        };
        let devices =
            std::collections::BTreeMap::from([(configuration.instance_id.clone(), device)]);

        let read = provider_instance_read(&portable_configuration(&configuration), &devices);
        let probe = read.last_probe.expect("stored probe stays visible");

        assert!(probe.authority_support.read_only_root);
        assert!(probe.authority_support.writable_root);
        assert!(probe.authority_support.person_approved_commands);
        assert_eq!(probe.version, stale_probe.version);
        assert_eq!(probe.checked_at, stale_probe.checked_at);
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

    #[cfg(unix)]
    #[test]
    fn installed_provider_search_skips_programs_other_users_can_replace() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "ganbaru-provider-discovery-shared-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let private = root.join("private");
        let shared = root.join("shared");
        std::fs::create_dir_all(&private).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        let set_mode = |path: &std::path::Path, mode: u32| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        set_mode(&private, 0o775);
        for (directory, name, mode) in [
            (&private, "group-provider", 0o775),
            (&private, "world-provider", 0o777),
            (&shared, "group-provider", 0o755),
        ] {
            let executable = directory.join(name);
            std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
            set_mode(&executable, mode);
        }
        set_mode(&shared, 0o1777);
        let mut metadata = ProviderDriverRegistry.list_metadata().remove(0);
        metadata.default_executable_candidates =
            vec!["world-provider".to_string(), "group-provider".to_string()];
        let path = std::env::join_paths([
            std::path::PathBuf::from("relative"),
            std::path::PathBuf::new(),
            shared.clone(),
            private.clone(),
        ])
        .unwrap();
        let directories = ganbaru_chat_providers::executable_search::path_search_directories(&path);

        let found = installed_provider_executables(&metadata, &directories);

        assert_eq!(directories, vec![shared.clone(), private.clone()]);
        assert_eq!(
            found,
            vec![
                private
                    .join("group-provider")
                    .to_string_lossy()
                    .into_owned()
            ]
        );
        set_mode(&shared, 0o755);
        std::fs::remove_dir_all(root).unwrap();
    }
}
