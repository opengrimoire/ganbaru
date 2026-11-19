//! Chat provider configuration and discovery domain.

use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode};

mod activity;
mod credentials;
mod discovery;
mod duplicates;
mod mapping;
mod picker;
mod preferences;
mod private_home;
mod providers;
mod runtime_approval;
mod store;

pub(crate) use crate::chat::configuration::{config_io_error, read_chat_config};
pub(crate) use activity::{
    CHAT_PROVIDER_ACTIVITY_EVENT, ProviderActivity, record_provider_activity,
};
pub(crate) use credentials::invalidate_provider_state_for_credential;
#[cfg(test)]
pub(crate) use discovery::{
    default_provider_configuration, provider_family_is_discoverable,
    should_discover_default_provider,
};
pub(crate) use discovery::{
    discover_default_providers, discover_default_providers_once, executable_search_directories,
    mark_discovery_finished, replacement_provider_configurations,
};
#[cfg(all(test, unix))]
pub(crate) use discovery::{drop_met_install_requirements, installed_provider_executables};
pub(crate) use duplicates::{duplicate_configuration_error, find_duplicate};
pub(crate) use mapping::{
    device_configuration, portable_configuration, provider_instance_read, provider_runtime_changed,
    read_provider,
};
pub(crate) use picker::{pick_local_path, validate_picker_title};
pub(crate) use preferences::{remember_composer_selection, set_working_folder_provider_preference};
pub(crate) use private_home::{
    configured_private_home, launch_configuration, maintenance_configuration, remove_private_home,
};
pub(crate) use providers::{apply_provider_probe, operation_context};
pub(crate) use runtime_approval::{
    ProviderRuntimePrompt, ProviderRuntimeReviewRead, RuntimeReceipts, approval_digests,
    approved_runtime, ask_runtime_approval, ingress_review_items, pending_portable_items,
    require_runtime_approved_on_device, validate_runtime_prompt,
};
pub(crate) use store::{
    mutate_chat_config, mutate_chat_configuration, provider_mut, read_settings,
};

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

pub(crate) fn credential_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat credential operation failed",
        true,
    )
}
