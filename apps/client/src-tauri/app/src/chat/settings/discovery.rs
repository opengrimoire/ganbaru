//! Pure provider discovery configuration and executable resolution.

use super::{
    config_io_error, device_state_error, discovery_state_error,
    mapping::portable_configuration,
    providers::operation_context,
    store::{mutate_chat_config, read_chat_config},
};
use crate::chat::device_state::{ChatProviderDeviceState, update_active_device_scope};
use crate::chat::settings_commands::ChatSettingsState;
use crate::vault;
use ganbaru_chat_contracts::config::ChatVaultConfig;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProbeState, ProviderFamilyId, ProviderFamilyMetadataRead,
    ProviderImplementationStatus, ProviderInstanceConfig, ProviderInstanceId, ProviderModelCatalog,
    ProviderProbeResult, VersionedJson,
};
use ganbaru_chat_providers::{ProviderDriverFactory, ProviderDriverRegistry};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) fn provider_family_is_discoverable(metadata: &ProviderFamilyMetadataRead) -> bool {
    metadata.implementation_status == ProviderImplementationStatus::Available
        && metadata
            .supported_platforms
            .iter()
            .any(|platform| platform == std::env::consts::OS)
}

pub(crate) fn should_discover_default_provider(
    config: &ChatVaultConfig,
    family_id: &ProviderFamilyId,
) -> bool {
    !config.automatic_provider_setup_disabled.contains(family_id)
        && !config
            .providers
            .iter()
            .any(|provider| &provider.family_id == family_id)
}

pub(crate) fn automatic_provider_instance_id(
    config: &ChatVaultConfig,
    family_id: &ProviderFamilyId,
) -> ChatResult<ProviderInstanceId> {
    let base = family_id.as_str();
    let candidates = std::iter::once(base.to_string())
        .chain(std::iter::once(format!("{base}-local")))
        .chain((2..=100).map(|suffix| format!("{base}-local-{suffix}")));
    for candidate in candidates {
        let instance_id = ProviderInstanceId::new(candidate)
            .map_err(|_| default_provider_configuration_error())?;
        if !config
            .providers
            .iter()
            .any(|provider| provider.instance_id == instance_id)
        {
            return Ok(instance_id);
        }
    }
    Err(default_provider_configuration_error())
}

pub(crate) fn default_provider_configuration(
    metadata: &ProviderFamilyMetadataRead,
    instance_id: ProviderInstanceId,
    executable: String,
) -> ChatResult<ProviderInstanceConfig> {
    let provider_config = match metadata.family_id.as_str() {
        "opencode" => serde_json::json!({ "mode": "local" }),
        _ => serde_json::json!({}),
    };
    Ok(ProviderInstanceConfig {
        schema_version: 1,
        instance_id,
        family_id: metadata.family_id.clone(),
        label: metadata.display_name.clone(),
        enabled: true,
        executable,
        provider_home: None,
        launch_arguments: Vec::new(),
        environment: BTreeMap::new(),
        credential_references: BTreeMap::new(),
        visible_model_ids: Vec::new(),
        favorite_model_ids: Vec::new(),
        provider_config: VersionedJson {
            schema_version: metadata.configuration_schema_version,
            value: provider_config,
        },
        internal_mcp: None,
        unknown_fields: BTreeMap::new(),
    })
}

pub(crate) fn executable_search_directories() -> Vec<PathBuf> {
    let mut directories = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(home) = platform_user_home() {
        directories.extend(fallback_executable_directories(&home));
    }
    let mut seen = BTreeSet::new();
    directories
        .into_iter()
        .filter(|directory| seen.insert(directory.clone()))
        .collect()
}

pub(crate) fn installed_provider_executables(
    metadata: &ProviderFamilyMetadataRead,
    search_directories: &[PathBuf],
) -> Vec<String> {
    let mut found = BTreeSet::new();
    let mut executables = Vec::new();
    for name in &metadata.default_executable_candidates {
        for directory in search_directories {
            for candidate_name in executable_candidate_names(name) {
                let candidate = directory.join(candidate_name);
                if !candidate.is_file()
                    || !is_executable(&candidate)
                    || !found.insert(candidate.clone())
                {
                    continue;
                }
                if let Some(path) = candidate.to_str() {
                    executables.push(path.to_string());
                }
            }
        }
    }
    executables
}

pub(crate) fn replacement_provider_configurations(
    metadata: &ProviderFamilyMetadataRead,
    configuration: &ProviderInstanceConfig,
    search_directories: &[PathBuf],
) -> Vec<ProviderInstanceConfig> {
    installed_provider_executables(metadata, search_directories)
        .into_iter()
        .filter(|executable| executable != &configuration.executable)
        .map(|executable| ProviderInstanceConfig {
            executable,
            ..configuration.clone()
        })
        .collect()
}

#[cfg(windows)]
fn executable_candidate_names(name: &str) -> Vec<String> {
    if Path::new(name).extension().is_some() {
        return vec![name.to_string()];
    }
    let extensions = std::env::var("PATHEXT")
        .ok()
        .map(|value| value.split(';').map(str::to_ascii_lowercase).collect())
        .unwrap_or_else(|| vec![".exe".to_string(), ".cmd".to_string(), ".bat".to_string()]);
    extensions
        .into_iter()
        .map(|extension| format!("{name}{extension}"))
        .collect()
}

#[cfg(not(windows))]
fn executable_candidate_names(name: &str) -> Vec<String> {
    vec![name.to_string()]
}

fn platform_user_home() -> Option<PathBuf> {
    #[cfg(windows)]
    let variable = "USERPROFILE";
    #[cfg(not(windows))]
    let variable = "HOME";
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn fallback_executable_directories(home: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            home.join("AppData").join("Roaming").join("npm"),
            home.join("AppData").join("Local").join("pnpm"),
            home.join(".local").join("bin"),
            home.join(".bun").join("bin"),
            home.join(".cargo").join("bin"),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![
            home.join(".local").join("bin"),
            home.join(".local").join("share").join("pnpm"),
            home.join(".npm-global").join("bin"),
            home.join(".bun").join("bin"),
            home.join(".cargo").join("bin"),
            home.join(".volta").join("bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
        ]
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn default_provider_configuration_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ConfigurationInvalid,
        "Default Chat provider configuration is invalid",
        false,
    )
}

#[derive(Default)]
pub(crate) struct ProviderDiscoveryRun {
    pub(crate) families_scanned: u32,
    pub(crate) discovered_probes: BTreeMap<ProviderInstanceId, ProviderProbeResult>,
}

pub(crate) async fn discover_default_providers_once(
    app: &tauri::AppHandle,
    state: &ChatSettingsState,
) -> ChatResult<()> {
    let vault_id = vault::active_vault_id(app).map_err(config_io_error)?;
    if discovery_finished(state, &vault_id)? {
        return Ok(());
    }

    let _guard = state.discovery_lock.lock().await;
    if discovery_finished(state, &vault_id)? {
        return Ok(());
    }
    discover_default_providers(app, state).await?;
    mark_discovery_finished(state, vault_id)
}

pub(crate) async fn discover_default_providers(
    app: &tauri::AppHandle,
    state: &ChatSettingsState,
) -> ChatResult<ProviderDiscoveryRun> {
    let metadata = ProviderDriverRegistry
        .list_metadata()
        .into_iter()
        .filter(provider_family_is_discoverable)
        .collect::<Vec<_>>();
    let mut run = ProviderDiscoveryRun {
        families_scanned: metadata.len() as u32,
        ..ProviderDiscoveryRun::default()
    };
    let mut current = read_chat_config(app)?;
    let search_directories = executable_search_directories();

    for family in metadata {
        if !should_discover_default_provider(&current, &family.family_id) {
            continue;
        }
        let Some((configuration, probe, model_catalog)) =
            probe_installed_provider(&family, &current, &search_directories).await?
        else {
            continue;
        };

        let portable = portable_configuration(&configuration);
        let mut inserted = false;
        mutate_chat_config(app, state, |config| {
            if !should_discover_default_provider(config, &family.family_id)
                || config
                    .providers
                    .iter()
                    .any(|provider| provider.instance_id == portable.instance_id)
            {
                return Ok(());
            }
            config.providers.push(portable.clone());
            inserted = true;
            Ok(())
        })?;
        if !inserted {
            continue;
        }

        update_active_device_scope(app, |scope| {
            scope.provider_instances.insert(
                configuration.instance_id.clone(),
                ChatProviderDeviceState {
                    executable_path: Some(configuration.executable.clone()),
                    provider_home_path: None,
                    last_successful_probe_at: (probe.state == ProbeState::Healthy)
                        .then(|| probe.checked_at.clone()),
                    last_probe: Some(probe.clone()),
                    model_catalog,
                },
            );
            Ok(())
        })
        .map_err(device_state_error)?;
        current.providers.push(portable);
        run.discovered_probes
            .insert(configuration.instance_id, probe);
    }

    Ok(run)
}

pub(crate) async fn probe_installed_provider(
    metadata: &ProviderFamilyMetadataRead,
    config: &ChatVaultConfig,
    search_directories: &[PathBuf],
) -> ChatResult<
    Option<(
        ProviderInstanceConfig,
        ProviderProbeResult,
        Option<ProviderModelCatalog>,
    )>,
> {
    let instance_id = automatic_provider_instance_id(config, &metadata.family_id)?;
    let mut fallback = None;
    for executable in installed_provider_executables(metadata, search_directories) {
        let configuration =
            default_provider_configuration(metadata, instance_id.clone(), executable)?;
        let mut driver = ProviderDriverRegistry.create_driver(configuration.clone())?;
        let Ok(probe) = driver
            .probe(&operation_context("discover-default-provider"))
            .await
        else {
            continue;
        };
        if probe.state == ProbeState::ExecutableMissing {
            continue;
        }
        let model_catalog = driver
            .cached_model_catalog()
            .map(ProviderModelCatalog::without_deprecated_models);
        let candidate = (configuration, probe.clone(), model_catalog);
        if matches!(
            probe.state,
            ProbeState::Healthy | ProbeState::AuthenticationRequired
        ) {
            return Ok(Some(candidate));
        }
        fallback.get_or_insert(candidate);
    }
    Ok(fallback)
}

pub(crate) fn discovery_finished(state: &ChatSettingsState, vault_id: &str) -> ChatResult<bool> {
    state
        .discovered_vaults
        .lock()
        .map(|vaults| vaults.contains(vault_id))
        .map_err(|_| discovery_state_error())
}

pub(crate) fn mark_discovery_finished(
    state: &ChatSettingsState,
    vault_id: String,
) -> ChatResult<()> {
    state
        .discovered_vaults
        .lock()
        .map(|mut vaults| {
            vaults.insert(vault_id);
        })
        .map_err(|_| discovery_state_error())
}
