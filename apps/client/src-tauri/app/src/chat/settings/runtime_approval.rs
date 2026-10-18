//! Device confirmation for provider runtime values that portable configuration can carry.
//!
//! Portable provider configuration travels with backups and handoff, so launch arguments,
//! environment entries, and provider settings outside the family allowlists apply only after
//! this device confirms them in a native dialog. Executable and provider home paths are
//! device-local; they need confirmation only when neither a native picker nor this device
//! supplied them.

use super::device_state_error;
use crate::chat::credentials::{PlatformCredentialStore, materialize_provider_environment};
use crate::chat::device_state::{ChatProviderDeviceState, read_active_device_scope};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProviderInstanceConfig, ProviderInstanceId,
};
use ganbaru_chat_providers::runtime_review::{
    DevicePathKind, RuntimeReviewItem, device_path_review_item, is_default_executable,
    portable_runtime_review_items,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// How long a picker selection or test confirmation vouches for a later save.
pub(crate) const RUNTIME_RECEIPT_TTL: Duration = Duration::from_secs(30 * 60);
/// Most picker selections and test confirmations remembered at once.
pub(crate) const MAX_RUNTIME_RECEIPTS: usize = 256;
/// Most review items listed in one native confirmation before summarizing the rest.
pub(crate) const MAX_RUNTIME_REVIEW_DIALOG_ITEMS: usize = 12;

/// Localized text for the native runtime confirmation.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRuntimePrompt {
    pub title: String,
    pub message: String,
    pub allow_label: String,
    pub cancel_label: String,
    /// Suffix for the count of items not listed, with `{count}` replaced natively.
    pub more_label: String,
}

/// One runtime value awaiting confirmation, as shown in Settings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRuntimeReviewRead {
    pub label: String,
    pub widens_permissions: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ReceiptKey {
    PickedPath(DevicePathKindKey, String),
    ConfirmedItem(ProviderInstanceId, String),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DevicePathKindKey {
    Executable,
    ProviderHome,
}

impl From<DevicePathKind> for DevicePathKindKey {
    fn from(kind: DevicePathKind) -> Self {
        match kind {
            DevicePathKind::Executable => Self::Executable,
            DevicePathKind::ProviderHome => Self::ProviderHome,
        }
    }
}

/// In-memory, bounded proof that a value came from a native picker or a test confirmation.
#[derive(Default)]
pub struct RuntimeReceipts {
    entries: Mutex<Vec<(ReceiptKey, Instant)>>,
}

impl RuntimeReceipts {
    pub(crate) fn record_picked_path(&self, kind: DevicePathKind, path: &str) {
        self.record([ReceiptKey::PickedPath(kind.into(), path.to_string())]);
    }

    pub(crate) fn record_confirmed_items(
        &self,
        instance_id: &ProviderInstanceId,
        items: &[RuntimeReviewItem],
    ) {
        self.record(
            items
                .iter()
                .map(|item| ReceiptKey::ConfirmedItem(instance_id.clone(), item.digest.clone())),
        );
    }

    fn record(&self, keys: impl IntoIterator<Item = ReceiptKey>) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        let now = Instant::now();
        entries.retain(|(_, recorded)| now.duration_since(*recorded) < RUNTIME_RECEIPT_TTL);
        for key in keys {
            entries.retain(|(existing, _)| existing != &key);
            entries.push((key, now));
        }
        let overflow = entries.len().saturating_sub(MAX_RUNTIME_RECEIPTS);
        entries.drain(..overflow);
    }

    fn contains(&self, key: &ReceiptKey) -> bool {
        let Ok(entries) = self.entries.lock() else {
            return false;
        };
        let now = Instant::now();
        entries.iter().any(|(existing, recorded)| {
            existing == key && now.duration_since(*recorded) < RUNTIME_RECEIPT_TTL
        })
    }

    fn vouches_for_path(&self, kind: DevicePathKind, path: &str) -> bool {
        self.contains(&ReceiptKey::PickedPath(kind.into(), path.to_string()))
    }

    fn vouches_for_item(&self, instance_id: &ProviderInstanceId, digest: &str) -> bool {
        self.contains(&ReceiptKey::ConfirmedItem(
            instance_id.clone(),
            digest.to_string(),
        ))
    }
}

/// Portable runtime values of `configuration` this device has not confirmed.
pub(crate) fn pending_portable_items(
    configuration: &ProviderInstanceConfig,
    device: Option<&ChatProviderDeviceState>,
) -> Vec<RuntimeReviewItem> {
    portable_runtime_review_items(configuration)
        .into_iter()
        .filter(|item| {
            device.is_none_or(|device| !device.approved_runtime_items.contains(&item.digest))
        })
        .collect()
}

/// Settings view of the values awaiting confirmation.
pub(crate) fn runtime_review_reads(items: &[RuntimeReviewItem]) -> Vec<ProviderRuntimeReviewRead> {
    items
        .iter()
        .map(|item| ProviderRuntimeReviewRead {
            label: item.label.clone(),
            widens_permissions: item.widens_permissions,
        })
        .collect()
}

/// Values a save or test request needs confirmed before they apply.
///
/// Portable values are vouched for by stored approvals or a recent test confirmation. Device
/// paths are vouched for when empty, a bare family default resolved through `PATH`, already
/// stored for this instance on this device, or recently returned by a native picker.
pub(crate) fn ingress_review_items(
    configuration: &ProviderInstanceConfig,
    device: Option<&ChatProviderDeviceState>,
    receipts: &RuntimeReceipts,
) -> Vec<RuntimeReviewItem> {
    let mut items = pending_portable_items(configuration, device)
        .into_iter()
        .filter(|item| !receipts.vouches_for_item(&configuration.instance_id, &item.digest))
        .collect::<Vec<_>>();
    let family = configuration.family_id.as_str();
    let executable = configuration.executable.trim();
    let stored_executable = device.and_then(|device| device.executable_path.as_deref());
    if !executable.is_empty()
        && !is_default_executable(family, executable)
        && stored_executable != Some(executable)
        && !receipts.vouches_for_path(DevicePathKind::Executable, executable)
    {
        let item = device_path_review_item(family, DevicePathKind::Executable, executable);
        if !receipts.vouches_for_item(&configuration.instance_id, &item.digest) {
            items.push(item);
        }
    }
    let home = configuration
        .provider_home
        .as_deref()
        .map(str::trim)
        .filter(|home| !home.is_empty());
    let stored_home = device.and_then(|device| device.provider_home_path.as_deref());
    if let Some(home) = home
        && stored_home != Some(home)
        && !receipts.vouches_for_path(DevicePathKind::ProviderHome, home)
    {
        let item = device_path_review_item(family, DevicePathKind::ProviderHome, home);
        if !receipts.vouches_for_item(&configuration.instance_id, &item.digest) {
            items.push(item);
        }
    }
    items
}

/// Digests to store as this device's approvals once `configuration` is confirmed.
pub(crate) fn approval_digests(configuration: &ProviderInstanceConfig) -> BTreeSet<String> {
    portable_runtime_review_items(configuration)
        .into_iter()
        .map(|item| item.digest)
        .collect()
}

pub(crate) fn runtime_approval_required_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::RuntimeApprovalRequired,
        "Provider runtime settings need confirmation on this device",
        true,
    )
}

/// Fails unless every portable runtime value of `configuration` is confirmed on this device.
pub(crate) fn require_approved_runtime(
    configuration: &ProviderInstanceConfig,
    device: Option<&ChatProviderDeviceState>,
) -> ChatResult<()> {
    if pending_portable_items(configuration, device).is_empty() {
        Ok(())
    } else {
        Err(runtime_approval_required_error())
    }
}

/// Resolves the provider runtime for an operation after the device approval check.
pub(crate) fn approved_runtime(
    app: &tauri::AppHandle,
    configuration: &ProviderInstanceConfig,
) -> ChatResult<ProviderInstanceConfig> {
    require_runtime_approved_on_device(app, configuration)?;
    materialize_provider_environment(configuration, &PlatformCredentialStore::default())
}

/// Device approval check without resolving secrets.
pub(crate) fn require_runtime_approved_on_device(
    app: &tauri::AppHandle,
    configuration: &ProviderInstanceConfig,
) -> ChatResult<()> {
    let scope = read_active_device_scope(app).map_err(device_state_error)?;
    require_approved_runtime(
        configuration,
        scope.provider_instances.get(&configuration.instance_id),
    )
}

pub(crate) fn validate_runtime_prompt(prompt: &ProviderRuntimePrompt) -> ChatResult<()> {
    let fields = [
        ("title", prompt.title.as_str(), 160),
        ("message", prompt.message.as_str(), 2_000),
        ("allowLabel", prompt.allow_label.as_str(), 80),
        ("cancelLabel", prompt.cancel_label.as_str(), 80),
        ("moreLabel", prompt.more_label.as_str(), 80),
    ];
    for (field, value, limit) in fields {
        let text = value.trim();
        let invalid_character = text
            .chars()
            .any(|character| character.is_control() && character != '\n');
        if text.is_empty() || text.len() > limit || invalid_character {
            return Err(ChatError::validation(
                field,
                "Runtime prompt text is invalid",
            ));
        }
    }
    if prompt.allow_label.trim() == prompt.cancel_label.trim() {
        return Err(ChatError::validation(
            "allowLabel",
            "Runtime prompt buttons need distinct labels",
        ));
    }
    Ok(())
}

/// Native dialog body: the localized message followed by natively resolved item labels.
pub(crate) fn runtime_dialog_message(
    prompt: &ProviderRuntimePrompt,
    provider_label: &str,
    items: &[RuntimeReviewItem],
) -> String {
    let mut lines = vec![
        prompt.message.trim().to_string(),
        String::new(),
        provider_label.chars().filter(|c| !c.is_control()).collect(),
    ];
    lines.extend(
        items
            .iter()
            .take(MAX_RUNTIME_REVIEW_DIALOG_ITEMS)
            .map(|item| format!("\u{2022} {}", item.label)),
    );
    let hidden = items.len().saturating_sub(MAX_RUNTIME_REVIEW_DIALOG_ITEMS);
    if hidden > 0 {
        lines.push(
            prompt
                .more_label
                .trim()
                .replace("{count}", &hidden.to_string()),
        );
    }
    lines.join("\n")
}

/// Shows the native confirmation and reports whether the user allowed the listed values.
pub(crate) async fn ask_runtime_approval(
    app: &tauri::AppHandle,
    prompt: &ProviderRuntimePrompt,
    provider_label: &str,
    items: &[RuntimeReviewItem],
) -> ChatResult<bool> {
    let allow_label = prompt.allow_label.trim().to_string();
    let (sender, mut receiver) = tauri::async_runtime::channel(1);
    app.dialog()
        .message(runtime_dialog_message(prompt, provider_label, items))
        .title(prompt.title.trim())
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            allow_label.clone(),
            prompt.cancel_label.trim().to_string(),
        ))
        .show_with_result(move |result| {
            let allowed = match result {
                tauri_plugin_dialog::MessageDialogResult::Ok
                | tauri_plugin_dialog::MessageDialogResult::Yes => true,
                tauri_plugin_dialog::MessageDialogResult::Custom(label) => label == allow_label,
                _ => false,
            };
            let _ = sender.try_send(allowed);
        });
    receiver.recv().await.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Runtime confirmation closed without a result",
            true,
        )
    })
}

#[cfg(test)]
mod tests;
