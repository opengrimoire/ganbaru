//! Device-local Chat bindings and provider runtime metadata.

#[cfg(desktop)]
use crate::vault::{active_vault_id, ensure_device_id, read_app_state, update_app_state};
use ganbaru_chat_contracts::models::{
    ChatThreadId, ProjectWorkingFolderId, ProviderInstanceId, ProviderModelCatalog,
    ProviderProbeResult, UtcTimestamp,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(desktop)]
use tauri::Runtime;

pub const CHAT_DEVICE_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatProviderDeviceState {
    pub executable_path: Option<String>,
    pub provider_home_path: Option<String>,
    pub last_probe: Option<ProviderProbeResult>,
    pub last_successful_probe_at: Option<UtcTimestamp>,
    pub model_catalog: Option<ProviderModelCatalog>,
    /// Digests of portable runtime values this device confirmed for the instance.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub approved_runtime_items: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMachinePreferences {
    pub last_selected_thread_id: Option<ChatThreadId>,
}

pub const DEFAULT_DIAGNOSTIC_RETENTION_DAYS: u16 = 7;
#[cfg(desktop)]
pub const MAX_DIAGNOSTIC_RETENTION_DAYS: u16 = 30;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDiagnosticPreferences {
    pub capture_enabled: bool,
    pub retention_days: u16,
}

impl Default for ChatDiagnosticPreferences {
    fn default() -> Self {
        Self {
            capture_enabled: false,
            retention_days: DEFAULT_DIAGNOSTIC_RETENTION_DAYS,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDeviceScope {
    pub provider_instances: BTreeMap<ProviderInstanceId, ChatProviderDeviceState>,
    pub full_access_trust:
        BTreeMap<ProviderInstanceId, BTreeMap<ProjectWorkingFolderId, UtcTimestamp>>,
    /// Local acceptances of synced Chat work, keyed by hex acceptance digest.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assignment_acceptances: BTreeMap<String, UtcTimestamp>,
    pub preferences: ChatMachinePreferences,
    pub diagnostics: ChatDiagnosticPreferences,
    pub execution_environment_paths: BTreeMap<String, String>,
    /// Preview origins the user allowed, keyed by Chat thread ID: external sites the model may
    /// use and loopback origins the thread may navigate to.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub preview_model_origins: BTreeMap<String, BTreeSet<String>>,
    /// Generation token naming each working folder's preview web data directory under the app
    /// config directory. Discarding the data replaces the token.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub preview_data_stores: BTreeMap<ProjectWorkingFolderId, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDeviceState {
    pub schema_version: u32,
    pub vaults: BTreeMap<String, BTreeMap<String, ChatDeviceScope>>,
}

impl Default for ChatDeviceState {
    fn default() -> Self {
        Self {
            schema_version: CHAT_DEVICE_STATE_SCHEMA_VERSION,
            vaults: BTreeMap::new(),
        }
    }
}

#[cfg(desktop)]
impl ChatDeviceState {
    pub fn scope(&self, vault_id: &str, device_id: &str) -> Option<&ChatDeviceScope> {
        self.vaults
            .get(vault_id)
            .and_then(|devices| devices.get(device_id))
    }

    pub fn scope_mut(&mut self, vault_id: &str, device_id: &str) -> &mut ChatDeviceScope {
        self.vaults
            .entry(vault_id.to_string())
            .or_default()
            .entry(device_id.to_string())
            .or_default()
    }
}

#[cfg(desktop)]
pub fn full_access_is_trusted(
    scope: &ChatDeviceScope,
    provider_instance_id: &ProviderInstanceId,
    working_folder_id: &ProjectWorkingFolderId,
) -> bool {
    scope
        .full_access_trust
        .get(provider_instance_id)
        .is_some_and(|workspaces| workspaces.contains_key(working_folder_id))
}

#[cfg(desktop)]
pub fn set_full_access_trust(
    scope: &mut ChatDeviceScope,
    provider_instance_id: ProviderInstanceId,
    working_folder_id: ProjectWorkingFolderId,
    trusted_at: Option<UtcTimestamp>,
) {
    if let Some(timestamp) = trusted_at {
        scope
            .full_access_trust
            .entry(provider_instance_id)
            .or_default()
            .insert(working_folder_id, timestamp);
        return;
    }
    let Some(workspaces) = scope.full_access_trust.get_mut(&provider_instance_id) else {
        return;
    };
    workspaces.remove(&working_folder_id);
    if workspaces.is_empty() {
        scope.full_access_trust.remove(&provider_instance_id);
    }
}

/// Most local acceptances one device scope keeps; a dropped acceptance at worst asks again.
#[cfg(desktop)]
pub const MAX_ASSIGNMENT_ACCEPTANCES: usize = 4096;

/// Whether this device accepted the synced Chat work with the acceptance digest `digest`.
#[cfg(desktop)]
pub fn assignment_is_accepted(scope: &ChatDeviceScope, digest: &str) -> bool {
    scope.assignment_acceptances.contains_key(digest)
}

/// Records the local acceptance of `digest` at `now`, dropping the oldest acceptance past
/// [`MAX_ASSIGNMENT_ACCEPTANCES`].
#[cfg(desktop)]
pub fn record_assignment_acceptance(
    scope: &mut ChatDeviceScope,
    digest: String,
    now: UtcTimestamp,
) {
    scope.assignment_acceptances.insert(digest.clone(), now);
    if scope.assignment_acceptances.len() <= MAX_ASSIGNMENT_ACCEPTANCES {
        return;
    }
    let oldest = scope
        .assignment_acceptances
        .iter()
        .filter(|(kept, _)| **kept != digest)
        .min_by_key(|(_, accepted_at)| accepted_at.as_str())
        .map(|(digest, _)| digest.clone());
    if let Some(oldest) = oldest {
        scope.assignment_acceptances.remove(&oldest);
    }
}

/// Drops the local acceptance of `digest`, once what it covered is used up.
#[cfg(desktop)]
pub fn forget_assignment_acceptance(scope: &mut ChatDeviceScope, digest: &str) {
    scope.assignment_acceptances.remove(digest);
}

/// Records this device's consent to send the scheduled message `scheduled_message_id` with the
/// request whose consent digest is `digest`.
#[cfg(desktop)]
pub fn record_scheduled_message_consent(
    scope: &mut ChatDeviceScope,
    _scheduled_message_id: String,
    digest: String,
    now: UtcTimestamp,
) {
    record_assignment_acceptance(scope, digest, now);
}

/// Whether this device consented to send `scheduled_message_id` with the request of `digest`.
#[cfg(desktop)]
pub fn scheduled_message_is_consented(
    scope: &ChatDeviceScope,
    _scheduled_message_id: &str,
    digest: &str,
) -> bool {
    assignment_is_accepted(scope, digest)
}

#[cfg(desktop)]
pub fn read_active_device_scope<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<ChatDeviceScope, String> {
    let vault_id = active_vault_id(app)?;
    let device_id = ensure_device_id(app)?;
    let state = read_app_state(app)?;
    Ok(state
        .chat
        .scope(&vault_id, &device_id)
        .cloned()
        .unwrap_or_default())
}

#[cfg(desktop)]
pub fn update_active_device_scope<R: Runtime, T>(
    app: &tauri::AppHandle<R>,
    update: impl FnOnce(&mut ChatDeviceScope) -> Result<T, String>,
) -> Result<T, String> {
    let vault_id = active_vault_id(app)?;
    let device_id = ensure_device_id(app)?;
    update_app_state(app, |state| {
        if state.chat.schema_version != CHAT_DEVICE_STATE_SCHEMA_VERSION {
            return Err("Chat device state schema is unsupported".to_string());
        }
        update(state.chat.scope_mut(&vault_id, &device_id))
    })
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    fn at(second: usize) -> UtcTimestamp {
        UtcTimestamp::new(format!(
            "2026-08-04T{:02}:{:02}:{:02}.000Z",
            second / 3600,
            second / 60 % 60,
            second % 60
        ))
        .unwrap()
    }

    #[test]
    fn acceptances_past_the_cap_drop_the_oldest_and_keep_the_newest() {
        let mut scope = ChatDeviceScope::default();
        for index in 0..=MAX_ASSIGNMENT_ACCEPTANCES {
            record_assignment_acceptance(&mut scope, format!("digest:{index}"), at(index + 1));
        }

        assert_eq!(
            scope.assignment_acceptances.len(),
            MAX_ASSIGNMENT_ACCEPTANCES
        );
        assert!(!assignment_is_accepted(&scope, "digest:0"));
        assert!(assignment_is_accepted(&scope, "digest:1"));
        assert!(assignment_is_accepted(
            &scope,
            &format!("digest:{MAX_ASSIGNMENT_ACCEPTANCES}")
        ));
    }

    #[test]
    fn a_new_acceptance_survives_the_cap_when_its_clock_runs_behind() {
        let mut scope = ChatDeviceScope::default();
        for index in 0..MAX_ASSIGNMENT_ACCEPTANCES {
            record_assignment_acceptance(&mut scope, format!("digest:{index}"), at(index + 2));
        }

        record_assignment_acceptance(&mut scope, "digest:new".to_owned(), at(1));

        assert!(assignment_is_accepted(&scope, "digest:new"));
    }

    #[test]
    fn scheduled_consents_leave_assignment_acceptances_in_place() {
        let mut scope = ChatDeviceScope::default();
        for index in 0..MAX_ASSIGNMENT_ACCEPTANCES {
            record_assignment_acceptance(&mut scope, format!("digest:{index}"), at(index + 1));
        }

        for index in 0..8 {
            record_scheduled_message_consent(
                &mut scope,
                format!("scheduled:{index}"),
                format!("consent:{index}"),
                at(MAX_ASSIGNMENT_ACCEPTANCES + index + 1),
            );
        }

        for index in 0..8 {
            assert!(
                assignment_is_accepted(&scope, &format!("digest:{index}")),
                "scheduled consent {index} evicted an assignment acceptance"
            );
            assert!(scheduled_message_is_consented(
                &scope,
                &format!("scheduled:{index}"),
                &format!("consent:{index}")
            ));
        }
    }

    #[test]
    fn a_forgotten_acceptance_leaves_the_others_in_place() {
        let mut scope = ChatDeviceScope::default();
        record_assignment_acceptance(&mut scope, "digest:used".to_owned(), at(1));
        record_assignment_acceptance(&mut scope, "digest:kept".to_owned(), at(2));

        forget_assignment_acceptance(&mut scope, "digest:used");
        forget_assignment_acceptance(&mut scope, "digest:unknown");

        assert!(!assignment_is_accepted(&scope, "digest:used"));
        assert!(assignment_is_accepted(&scope, "digest:kept"));
    }
}
