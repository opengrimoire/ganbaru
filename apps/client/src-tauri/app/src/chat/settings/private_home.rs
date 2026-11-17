//! Private provider homes that Ganbaru owns on this device.
//!
//! Grok prefers a browser login kept in its home over `XAI_API_KEY`, so a Grok instance with an
//! API key launches with a home of its own under the app config folder, one per vault and
//! instance. That home holds no login, only what Grok creates there, and only the launched
//! process uses it.

use crate::chat::credentials::{PlatformCredentialStore, materialize_provider_environment};
use crate::vault;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProviderAuthSource, ProviderInstanceConfig,
    ProviderInstanceId,
};
use ganbaru_chat_providers::ProviderDriverRegistry;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::Manager;

/// Folder under the app config folder that holds private provider homes.
const PRIVATE_HOMES_DIRECTORY: &str = "chat-provider-homes";

/// Resolves `configuration` for a launch: secrets from the credential store, plus a private
/// home when the instance needs one.
///
/// # Errors
///
/// Returns the credential resolution error, or an internal error when the app config folder or
/// the active vault cannot be found.
pub(crate) fn launch_configuration(
    app: &tauri::AppHandle,
    configuration: &ProviderInstanceConfig,
) -> ChatResult<ProviderInstanceConfig> {
    let mut resolved =
        materialize_provider_environment(configuration, &PlatformCredentialStore::default())?;
    if needs_private_home(&resolved) {
        let home = private_home(
            &config_dir(app)?,
            &active_vault_id(app)?,
            &resolved.instance_id,
        );
        resolved.private_home = Some(home.to_string_lossy().into_owned());
    }
    Ok(resolved)
}

/// The private home launches of `configuration` use, found without reading its secrets, so
/// Settings can show the files the provider reads there.
///
/// # Errors
///
/// Returns an internal error when the app config folder or the active vault cannot be found.
pub(crate) fn configured_private_home(
    app: &tauri::AppHandle,
    configuration: &ProviderInstanceConfig,
) -> ChatResult<Option<PathBuf>> {
    if !needs_private_home(&credential_shape(configuration)) {
        return Ok(None);
    }
    Ok(Some(private_home(
        &config_dir(app)?,
        &active_vault_id(app)?,
        &configuration.instance_id,
    )))
}

/// `configuration` with a placeholder for each stored credential, which is enough to tell how
/// the instance authenticates.
fn credential_shape(configuration: &ProviderInstanceConfig) -> ProviderInstanceConfig {
    let mut shape = configuration.clone();
    for name in configuration.credential_references.keys() {
        shape
            .environment
            .insert(name.clone(), "stored-credential".to_string());
    }
    shape
}
/// Removes the private home of a removed provider instance, when it has one.
pub(crate) fn remove_private_home(app: &tauri::AppHandle, instance_id: &ProviderInstanceId) {
    let (Ok(config_dir), Ok(vault_id)) = (config_dir(app), active_vault_id(app)) else {
        return;
    };
    let home = private_home(&config_dir, &vault_id, instance_id);
    if home.exists()
        && let Err(error) = std::fs::remove_dir_all(&home)
    {
        eprintln!(
            "Chat could not remove a private provider home: {}",
            error.kind()
        );
    }
}

/// Whether a resolved configuration launches with a private home: a Grok instance that
/// authenticates with an API key.
fn needs_private_home(configuration: &ProviderInstanceConfig) -> bool {
    ProviderDriverRegistry.auth_source(configuration) == Some(ProviderAuthSource::ApiKey)
}

/// The private home of `instance_id` in `vault_id`. Instance ids may hold characters that are
/// not valid in a file name on every platform, so the folder is named by their SHA-256 digest.
fn private_home(config_dir: &Path, vault_id: &str, instance_id: &ProviderInstanceId) -> PathBuf {
    let digest = Sha256::digest(instance_id.as_str().as_bytes());
    let vault_digest = Sha256::digest(vault_id.as_bytes());
    config_dir
        .join(PRIVATE_HOMES_DIRECTORY)
        .join(format!("{vault_digest:x}"))
        .join(format!("{digest:x}"))
}

fn config_dir(app: &tauri::AppHandle) -> ChatResult<PathBuf> {
    app.path().app_config_dir().map_err(|_| location_error())
}

fn active_vault_id(app: &tauri::AppHandle) -> ChatResult<String> {
    vault::active_vault_id(app).map_err(|_| location_error())
}

fn location_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Chat could not find the private provider home folder",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configuration(family: &str, environment: &[(&str, &str)]) -> ProviderInstanceConfig {
        let mut configuration: ProviderInstanceConfig = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "instanceId": "provider-instance/grok:1",
            "familyId": family,
            "label": "Provider",
            "enabled": true,
            "executable": family,
            "providerHome": null,
            "launchArguments": [],
            "environment": {},
            "credentialReferences": {},
            "visibleModelIds": [],
            "favoriteModelIds": [],
            "providerConfig": { "schemaVersion": 1, "value": {} }
        }))
        .unwrap();
        for (name, value) in environment {
            configuration
                .environment
                .insert((*name).to_string(), (*value).to_string());
        }
        configuration
    }

    #[test]
    fn only_a_grok_instance_with_an_api_key_launches_with_a_private_home() {
        assert!(needs_private_home(&configuration(
            "grok",
            &[("XAI_API_KEY", "xai-test")]
        )));
        assert!(!needs_private_home(&configuration("grok", &[])));
        assert!(!needs_private_home(&configuration(
            "grok",
            &[("XAI_API_KEY", "  ")]
        )));
        assert!(!needs_private_home(&configuration(
            "codex",
            &[("XAI_API_KEY", "xai-test")]
        )));
    }

    #[test]
    fn a_stored_api_key_shows_the_private_home_without_reading_the_secret() {
        let mut stored = configuration("grok", &[]);
        let reference = ganbaru_chat::credentials::provider_credential_reference(
            &stored.instance_id,
            "XAI_API_KEY",
        )
        .unwrap();
        stored
            .credential_references
            .insert("XAI_API_KEY".to_string(), reference);
        assert!(!needs_private_home(&stored));
        assert!(needs_private_home(&credential_shape(&stored)));
        assert!(!needs_private_home(&credential_shape(&configuration(
            "grok",
            &[]
        ))));
    }

    #[test]
    fn private_homes_are_separate_per_vault_and_instance_inside_the_config_folder() {
        let root = Path::new("/config/ganbaru");
        let first = ProviderInstanceId::new("provider-instance/grok:1").unwrap();
        let second = ProviderInstanceId::new("provider-instance/grok:2").unwrap();
        let home = private_home(root, "vault-a", &first);
        assert!(home.starts_with(root.join(PRIVATE_HOMES_DIRECTORY)));
        assert_eq!(home.components().count(), root.components().count() + 3);
        assert_eq!(home, private_home(root, "vault-a", &first));
        assert_ne!(home, private_home(root, "vault-b", &first));
        assert_ne!(home, private_home(root, "vault-a", &second));
        assert!(!home.to_string_lossy().contains("grok:1"));
    }
}
