//! Chat provider configuration and discovery domain.

use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode};

mod credentials;
mod discovery;
mod mapping;
mod picker;
mod preferences;
mod providers;
mod store;

pub(crate) use credentials::invalidate_provider_state_for_credential;
#[cfg(test)]
pub(crate) use discovery::{
    default_provider_configuration, installed_provider_executables,
    provider_family_is_discoverable, should_discover_default_provider,
};
pub(crate) use discovery::{
    discover_default_providers, discover_default_providers_once, executable_search_directories,
    mark_discovery_finished, replacement_provider_configurations,
};
pub(crate) use mapping::{
    device_configuration, portable_configuration, provider_instance_read, provider_runtime_changed,
    read_provider,
};
pub(crate) use picker::{pick_local_path, validate_picker_title};
pub(crate) use preferences::{remember_composer_selection, set_working_folder_provider_preference};
pub(crate) use providers::{apply_provider_probe, operation_context, unique_model_ids};
pub(crate) use store::{mutate_chat_config, provider_mut, read_chat_config, read_settings};

pub(crate) fn provider_not_found_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::NotFound,
        "Chat provider instance was not found",
        true,
    )
}

pub(crate) fn discovery_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Chat provider discovery state is unavailable",
        true,
    )
}

pub(crate) fn device_state_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat device state could not be updated",
        true,
    )
}

pub(crate) fn config_io_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat settings could not be persisted",
        true,
    )
}

pub(crate) fn config_shape_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat settings file is invalid",
        false,
    )
}

pub(crate) fn credential_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat credential operation failed",
        true,
    )
}
