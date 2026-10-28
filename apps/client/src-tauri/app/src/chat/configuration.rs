//! Chat settings storage shared by desktop and mobile.
//!
//! Behavior preferences are portable `chat.behavior.*` vault preferences that the preferences
//! sync domain replicates. Provider entries, remembered selections, working folder provider
//! preferences, and panel layout stay in this device's preference file under a native entry
//! that generic preference patches never reach.

use crate::vault::VaultConfigPatch;
use crate::vault::preferences::{
    CHAT_DEVICE_ENTRY, apply_patches, effective_view, read_native_device_entry,
    update_native_device_entry, value_at,
};
use ganbaru_chat_contracts::config::{
    ChatBehaviorPreferences, ChatConfiguration, ChatDeviceConfig,
};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};

/// The registry path whose portable keys hold the Chat behavior preferences.
pub(crate) const CHAT_BEHAVIOR_PREFERENCE_PATH: &str = "chat.behavior";

/// Maps a storage failure to the public Chat error without exposing local paths.
pub(crate) fn config_io_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat settings could not be persisted",
        true,
    )
}

/// Runs device preference file `work` on the blocking pool, so an async command never holds
/// the async runtime during file I/O.
pub(crate) async fn on_blocking_pool<T: Send + 'static>(
    work: impl FnOnce() -> ChatResult<T> + Send + 'static,
) -> ChatResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| config_io_error(format!("Chat settings worker stopped: {error}")))?
}

/// The device Chat settings with the portable behavior preferences of the active vault.
pub(crate) async fn read_chat_configuration<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> ChatResult<ChatConfiguration> {
    let handle = app.clone();
    let device = on_blocking_pool(move || read_chat_config(&handle)).await?;
    with_behavior(app, device).await
}

/// Applies `mutate` to the device Chat settings on the blocking pool, see
/// [`update_chat_device_config`], and returns them with the portable behavior preferences.
/// Desktop commands go through the settings mutation lock instead.
#[cfg(mobile)]
pub(crate) async fn update_chat_configuration<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    mutate: impl FnOnce(&mut ChatDeviceConfig) -> ChatResult<()> + Send + 'static,
) -> ChatResult<ChatConfiguration> {
    let handle = app.clone();
    let device = on_blocking_pool(move || update_chat_device_config(&handle, mutate)).await?;
    with_behavior(app, device).await
}

/// `device` with the portable behavior preferences of the active vault.
pub(crate) async fn with_behavior<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    device: ChatDeviceConfig,
) -> ChatResult<ChatConfiguration> {
    Ok(ChatConfiguration {
        device,
        behavior: read_chat_behavior(app).await?,
    })
}

/// Stores every field of `behavior` as its portable `chat.behavior.*` preference, which the
/// preferences sync domain replicates to the other devices of the vault.
pub(crate) async fn write_chat_behavior<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    behavior: &ChatBehaviorPreferences,
) -> ChatResult<()> {
    behavior.validate()?;
    let patches = behavior_patches(behavior)?;
    apply_patches(app, patches).await.map_err(config_io_error)
}

/// One preference patch per field of `behavior`, at `chat.behavior.<field>`.
pub(crate) fn behavior_patches(
    behavior: &ChatBehaviorPreferences,
) -> ChatResult<Vec<VaultConfigPatch>> {
    let serialized = serde_json::to_value(behavior)
        .map_err(|error| config_io_error(format!("encode Chat behavior: {error}")))?;
    let serde_json::Value::Object(fields) = serialized else {
        return Err(config_io_error("Chat behavior is not an object".to_owned()));
    };
    let base = CHAT_BEHAVIOR_PREFERENCE_PATH
        .split('.')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    Ok(fields
        .into_iter()
        .map(|(field, value)| VaultConfigPatch {
            path: base.iter().cloned().chain(std::iter::once(field)).collect(),
            remove: false,
            value,
        })
        .collect())
}

/// The Chat behavior preferences of the active vault.
pub(crate) async fn read_chat_behavior<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> ChatResult<ChatBehaviorPreferences> {
    let view = effective_view(app).await.map_err(config_io_error)?;
    Ok(behavior_from_view(&view))
}

/// The Chat behavior preferences held in a composed preference view. The view holds only
/// values the registry accepts, so a missing key reads as its default.
pub(crate) fn behavior_from_view(view: &serde_json::Value) -> ChatBehaviorPreferences {
    value_at(view, CHAT_BEHAVIOR_PREFERENCE_PATH)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

/// Applies `mutate` to the device Chat settings in the device preference file and stores them
/// when they still validate. The device file lock serializes it with other device writes.
pub(crate) fn update_chat_device_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    mutate: impl FnOnce(&mut ChatDeviceConfig) -> ChatResult<()>,
) -> ChatResult<ChatDeviceConfig> {
    update_native_device_entry(
        app,
        CHAT_DEVICE_ENTRY,
        |current| {
            let mut config = ChatDeviceConfig::from_stored(current)?;
            mutate(&mut config)?;
            Ok((config.to_stored()?, config))
        },
        config_io_error,
    )
}

/// The device Chat settings of the active vault, or defaults when none are stored yet.
pub(crate) fn read_chat_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> ChatResult<ChatDeviceConfig> {
    let stored = read_native_device_entry(app, CHAT_DEVICE_ENTRY).map_err(config_io_error)?;
    ChatDeviceConfig::from_stored(stored.as_ref())
}
