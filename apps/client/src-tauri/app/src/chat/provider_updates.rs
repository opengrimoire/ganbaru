//! Provider CLI update checks and updates for Settings > Chat.
//!
//! Checks read each family's release source with a configuration that holds no secrets, so
//! the credential store is never opened and no update process ever sees a key. Only an
//! instance whose runtime this device approved is checked or updated, because Grok's check
//! and every update run the configured program.

use super::device_state::read_active_device_scope;
use super::settings::{
    device_state_error, maintenance_configuration, provider_instance_read, read_chat_config,
    read_provider, require_runtime_approved_on_device,
};
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::models::{
    ChatError, ChatResult, ProviderFamilyId, ProviderInstanceConfig, ProviderInstanceId,
    ProviderUpdateRead, ProviderUpdateRunRead,
};
use ganbaru_chat_providers::cli_updates::{
    ReleaseLookups, UpdateCheck, prepare_update_check, run_update, update_command,
};
use ganbaru_chat_providers::{ProviderCliCommand, ProviderDriverFactory, ProviderDriverRegistry};
use tauri::Manager;

/// One update at a time, since two installers replacing the same program would race.
static UPDATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Newer releases for every enabled provider instance whose last probe read a version.
///
/// An instance that cannot be checked is left out, so one unreachable source or broken
/// install never hides the updates of the others.
///
/// # Errors
///
/// Fails only when the Chat settings or this device's provider state cannot be read.
#[tauri::command]
pub async fn chat_check_provider_updates(
    app: tauri::AppHandle,
) -> ChatResult<Vec<ProviderUpdateRead>> {
    let config = read_chat_config(&app)?;
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    let checks = config
        .providers
        .iter()
        .map(|portable| provider_instance_read(portable, &scope.provider_instances))
        .filter(|read| read.configuration.enabled)
        .filter_map(|read| {
            let version = read.last_probe?.version?;
            match prepared_check(&app, &read.configuration, &version) {
                Ok(check) => check,
                Err(error) => {
                    eprintln!(
                        "[chat] provider update check skipped {}: {}",
                        read.configuration.instance_id.as_str(),
                        error.message
                    );
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    let mut lookups = ReleaseLookups::default();
    let mut updates = Vec::new();
    for check in checks {
        let instance_id = check.instance_id().clone();
        match check.run(&mut lookups).await {
            Ok(Some(update)) => updates.push(update),
            Ok(None) => {}
            Err(error) => eprintln!(
                "[chat] provider update check failed {}: {}",
                instance_id.as_str(),
                error.message
            ),
        }
    }
    Ok(updates)
}

/// Runs the provider's own update command for `instance_id`, installing `target_version`
/// where the CLI takes a version.
/// After a successful update, open sessions of that family restart onto the new program once
/// they are idle and quiet, so running work is never interrupted.
///
/// # Errors
///
/// Fails when the instance is unknown or unapproved on this device, `target_version` is not an
/// update from its last probed version, its family has no update command, a package manager
/// owns the install, or the program cannot start in time.
#[tauri::command]
pub async fn chat_run_provider_update(
    app: tauri::AppHandle,
    instance_id: ProviderInstanceId,
    target_version: String,
) -> ChatResult<ProviderUpdateRunRead> {
    let (family_id, command) = resolved_update_command(&app, &instance_id, &target_version)?;
    let _guard = UPDATE_LOCK.lock().await;
    let result = run_update(command).await?;
    if result.successful
        && let Err(error) = app
            .state::<ChatRuntimeRegistry>()
            .restart_family_sessions_after_update(&family_id)
    {
        eprintln!(
            "Chat could not restart sessions after a provider update, code {:?}",
            error.code
        );
    }
    Ok(result)
}

fn prepared_check(
    app: &tauri::AppHandle,
    configuration: &ProviderInstanceConfig,
    version: &str,
) -> ChatResult<Option<UpdateCheck>> {
    require_runtime_approved_on_device(app, configuration)?;
    let driver =
        ProviderDriverRegistry.create_driver(maintenance_configuration(app, configuration)?)?;
    prepare_update_check(driver.as_ref(), version)
}

fn resolved_update_command(
    app: &tauri::AppHandle,
    instance_id: &ProviderInstanceId,
    target_version: &str,
) -> ChatResult<(ProviderFamilyId, ProviderCliCommand)> {
    let provider = read_provider(app, instance_id)?;
    let current_version = provider
        .last_probe
        .and_then(|probe| probe.version)
        .ok_or_else(|| ChatError::unsupported("Check this provider before updating it"))?;
    let configuration = provider.configuration;
    require_runtime_approved_on_device(app, &configuration)?;
    let driver =
        ProviderDriverRegistry.create_driver(maintenance_configuration(app, &configuration)?)?;
    let command = update_command(driver.as_ref(), &current_version, target_version)?;
    Ok((configuration.family_id, command))
}
