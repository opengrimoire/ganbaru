//! Device-local Chat bindings and provider runtime metadata.

use crate::vault::{active_vault_id, ensure_device_id, read_app_state, update_app_state};
use ganbaru_chat::scheduling::QuietPeriodPreferences;
use ganbaru_chat_contracts::events::RateLimitStatusEvent;
use ganbaru_chat_contracts::models::{
    ChatThreadId, ProjectWorkingFolderId, ProviderInstanceId, ProviderModelCatalog,
    ProviderProbeResult, UtcTimestamp,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
    /// The newest usage limits the provider reported on this device, from a probe or a turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_limits: Option<ProviderUsageLimitsObservation>,
    /// When a turn last started on this instance on this device. Only the order across
    /// instances matters, so it is written only when the instance becomes the most recent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<UtcTimestamp>,
}

impl ChatProviderDeviceState {
    /// Stores the usage limits a provider reported unless a newer report is already stored, and
    /// reports whether it was stored. A probe and a turn can report in either order, so the
    /// newest observation wins.
    pub fn observe_usage_limits(
        &mut self,
        status: RateLimitStatusEvent,
        observed_at: UtcTimestamp,
    ) -> bool {
        if self
            .usage_limits
            .as_ref()
            .is_some_and(|current| !is_after(&observed_at, &current.observed_at))
        {
            return false;
        }
        self.usage_limits = Some(ProviderUsageLimitsObservation {
            observed_at,
            status,
        });
        true
    }
}

/// Usage limits a provider reported, with the time this device observed them.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageLimitsObservation {
    pub observed_at: UtcTimestamp,
    pub status: RateLimitStatusEvent,
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

/// This device's consent to send one scheduled message with exactly one request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledMessageConsent {
    /// Hex consent digest of the scheduled message ID and its stored request.
    pub digest: String,
    /// When the consent was given.
    pub recorded_at: UtcTimestamp,
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
    /// Consents to send pending scheduled messages, keyed by scheduled message ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scheduled_message_consents: BTreeMap<String, ScheduledMessageConsent>,
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
    /// Quiet windows during which this device holds repeating messages. They never sync:
    /// quiet hours describe one device's place in a person's day.
    #[serde(default)]
    pub quiet_periods: QuietPeriodPreferences,
    /// Schedule proposals this device dismissed, keyed by proposal item ID, with when. A
    /// proposal is a transient suggestion, so dismissing it stays on this device.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dismissed_schedule_proposals: BTreeMap<String, UtcTimestamp>,
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

/// Most scheduled message consents one device scope keeps, apart from assignment acceptances; a
/// dropped consent at worst makes the message's work ask for local acceptance.
#[cfg(desktop)]
pub const MAX_SCHEDULED_MESSAGE_CONSENTS: usize = 4096;

/// Records this device's consent to send the scheduled message `scheduled_message_id` with the
/// request whose consent digest is `digest`, replacing an earlier consent to that message and
/// dropping the oldest consent past [`MAX_SCHEDULED_MESSAGE_CONSENTS`].
#[cfg(desktop)]
pub fn record_scheduled_message_consent(
    scope: &mut ChatDeviceScope,
    scheduled_message_id: String,
    digest: String,
    now: UtcTimestamp,
) {
    scope.scheduled_message_consents.insert(
        scheduled_message_id.clone(),
        ScheduledMessageConsent {
            digest,
            recorded_at: now,
        },
    );
    if scope.scheduled_message_consents.len() <= MAX_SCHEDULED_MESSAGE_CONSENTS {
        return;
    }
    let oldest = scope
        .scheduled_message_consents
        .iter()
        .filter(|(kept, _)| **kept != scheduled_message_id)
        .min_by_key(|(_, consent)| consent.recorded_at.as_str())
        .map(|(id, _)| id.clone());
    if let Some(oldest) = oldest {
        scope.scheduled_message_consents.remove(&oldest);
    }
}

/// Whether this device consented to send `scheduled_message_id` with the request of `digest`.
#[cfg(desktop)]
pub fn scheduled_message_is_consented(
    scope: &ChatDeviceScope,
    scheduled_message_id: &str,
    digest: &str,
) -> bool {
    scope
        .scheduled_message_consents
        .get(scheduled_message_id)
        .is_some_and(|consent| consent.digest == digest)
}

/// Drops the consent to `scheduled_message_id` when it still covers the request of `digest`, so
/// a consent recorded again for a changed request stays.
#[cfg(desktop)]
pub fn forget_scheduled_message_consent(
    scope: &mut ChatDeviceScope,
    scheduled_message_id: &str,
    digest: &str,
) {
    if scheduled_message_is_consented(scope, scheduled_message_id, digest) {
        scope
            .scheduled_message_consents
            .remove(scheduled_message_id);
    }
}

/// Most dismissed schedule proposals one device scope keeps; the oldest beyond it is dropped.
pub const MAX_DISMISSED_SCHEDULE_PROPOSALS: usize = 1024;

/// Records that this device dismissed the schedule proposal `proposal_item_id`, dropping the
/// oldest dismissal past [`MAX_DISMISSED_SCHEDULE_PROPOSALS`].
pub fn record_dismissed_schedule_proposal(
    scope: &mut ChatDeviceScope,
    proposal_item_id: String,
    now: UtcTimestamp,
) {
    scope
        .dismissed_schedule_proposals
        .insert(proposal_item_id.clone(), now);
    while scope.dismissed_schedule_proposals.len() > MAX_DISMISSED_SCHEDULE_PROPOSALS {
        let oldest = scope
            .dismissed_schedule_proposals
            .iter()
            .filter(|(kept, _)| **kept != proposal_item_id)
            .min_by_key(|(_, dismissed_at)| dismissed_at.as_str())
            .map(|(id, _)| id.clone());
        let Some(oldest) = oldest else {
            break;
        };
        scope.dismissed_schedule_proposals.remove(&oldest);
    }
}

/// Orders two stored timestamps by instant; text order differs across fraction precisions.
fn is_after(candidate: &UtcTimestamp, reference: &UtcTimestamp) -> bool {
    let instant = |value: &UtcTimestamp| chrono::DateTime::parse_from_rfc3339(value.as_str()).ok();
    match (instant(candidate), instant(reference)) {
        (Some(candidate), Some(reference)) => candidate > reference,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// Stores a usage limit report for an instance this device keeps; see
/// [`ChatProviderDeviceState::observe_usage_limits`].
pub fn record_provider_usage_limits(
    scope: &mut ChatDeviceScope,
    instance_id: &ProviderInstanceId,
    status: RateLimitStatusEvent,
    observed_at: UtcTimestamp,
) -> bool {
    scope
        .provider_instances
        .get_mut(instance_id)
        .is_some_and(|device| device.observe_usage_limits(status, observed_at))
}

/// Marks an instance as the most recently used on this device and reports whether the order
/// across instances changed. An instance that is already the most recent keeps its older
/// time, so consecutive turns on one provider never rewrite device state.
pub fn record_provider_use(
    scope: &mut ChatDeviceScope,
    instance_id: &ProviderInstanceId,
    used_at: UtcTimestamp,
) -> bool {
    let Some(current) = scope.provider_instances.get(instance_id) else {
        return false;
    };
    let already_most_recent = current.last_used_at.as_ref().is_some_and(|own| {
        scope
            .provider_instances
            .iter()
            .filter(|(id, _)| *id != instance_id)
            .filter_map(|(_, other)| other.last_used_at.as_ref())
            .all(|other| !is_after(other, own))
    });
    if already_most_recent {
        return false;
    }
    if let Some(device) = scope.provider_instances.get_mut(instance_id) {
        device.last_used_at = Some(used_at);
    }
    true
}

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
    fn dismissed_proposals_past_their_cap_drop_the_oldest_and_keep_the_newest() {
        let mut scope = ChatDeviceScope::default();
        for index in 0..MAX_DISMISSED_SCHEDULE_PROPOSALS {
            record_dismissed_schedule_proposal(
                &mut scope,
                format!("proposal:{index}"),
                at(index + 2),
            );
        }

        record_dismissed_schedule_proposal(&mut scope, "proposal:new".to_owned(), at(1));

        assert_eq!(
            scope.dismissed_schedule_proposals.len(),
            MAX_DISMISSED_SCHEDULE_PROPOSALS
        );
        assert!(
            scope
                .dismissed_schedule_proposals
                .contains_key("proposal:new")
        );
        assert!(
            !scope
                .dismissed_schedule_proposals
                .contains_key("proposal:0")
        );
        assert!(
            scope
                .dismissed_schedule_proposals
                .contains_key("proposal:1")
        );
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
    fn a_forgotten_scheduled_consent_leaves_the_others_in_place() {
        let mut scope = ChatDeviceScope::default();
        for (id, digest, second) in [("used", "digest:used", 1), ("kept", "digest:kept", 2)] {
            record_scheduled_message_consent(
                &mut scope,
                id.to_owned(),
                digest.to_owned(),
                at(second),
            );
        }

        forget_scheduled_message_consent(&mut scope, "used", "digest:used");
        forget_scheduled_message_consent(&mut scope, "kept", "digest:other");
        forget_scheduled_message_consent(&mut scope, "unknown", "digest:unknown");

        assert!(!scheduled_message_is_consented(
            &scope,
            "used",
            "digest:used"
        ));
        assert!(scheduled_message_is_consented(
            &scope,
            "kept",
            "digest:kept"
        ));
    }

    #[test]
    fn scheduled_consents_past_their_cap_drop_the_oldest_and_keep_the_newest() {
        let mut scope = ChatDeviceScope::default();
        for index in 0..=MAX_SCHEDULED_MESSAGE_CONSENTS {
            record_scheduled_message_consent(
                &mut scope,
                format!("scheduled:{index}"),
                format!("digest:{index}"),
                at(index + 1),
            );
        }

        assert_eq!(
            scope.scheduled_message_consents.len(),
            MAX_SCHEDULED_MESSAGE_CONSENTS
        );
        assert!(!scheduled_message_is_consented(
            &scope,
            "scheduled:0",
            "digest:0"
        ));
        assert!(scheduled_message_is_consented(
            &scope,
            &format!("scheduled:{MAX_SCHEDULED_MESSAGE_CONSENTS}"),
            &format!("digest:{MAX_SCHEDULED_MESSAGE_CONSENTS}")
        ));
    }

    fn instance(name: &str) -> ProviderInstanceId {
        ProviderInstanceId::new(name.to_string()).unwrap()
    }

    fn scope_with(instances: &[&str]) -> ChatDeviceScope {
        let mut scope = ChatDeviceScope::default();
        for name in instances {
            scope
                .provider_instances
                .insert(instance(name), ChatProviderDeviceState::default());
        }
        scope
    }

    fn limits(limited: bool) -> RateLimitStatusEvent {
        RateLimitStatusEvent {
            limited,
            resets_at: None,
            detail: None,
            provider_data: None,
        }
    }

    #[test]
    fn usage_limits_keep_the_newest_observation_across_precisions() {
        let mut scope = scope_with(&["codex"]);
        let codex = instance("codex");
        let later = UtcTimestamp::new("2026-08-04T10:00:00.500Z").unwrap();
        let earlier_without_fraction = UtcTimestamp::new("2026-08-04T10:00:00Z").unwrap();

        assert!(record_provider_usage_limits(
            &mut scope,
            &codex,
            limits(true),
            later.clone()
        ));
        assert!(
            !record_provider_usage_limits(
                &mut scope,
                &codex,
                limits(false),
                earlier_without_fraction
            ),
            "an older report that arrives late never replaces a newer one"
        );
        assert!(!record_provider_usage_limits(
            &mut scope,
            &codex,
            limits(false),
            later.clone()
        ));
        let stored = scope.provider_instances[&codex]
            .usage_limits
            .clone()
            .unwrap();
        assert!(stored.status.limited);
        assert_eq!(stored.observed_at, later);

        assert!(record_provider_usage_limits(
            &mut scope,
            &codex,
            limits(false),
            at(36_001)
        ));
        assert!(
            !record_provider_usage_limits(
                &mut scope,
                &instance("removed"),
                limits(true),
                at(36_002)
            ),
            "a report for an instance this device no longer keeps is dropped"
        );
    }

    #[test]
    fn provider_use_is_written_only_when_the_most_recent_instance_changes() {
        let mut scope = scope_with(&["claude", "codex"]);
        let claude = instance("claude");
        let codex = instance("codex");

        assert!(record_provider_use(&mut scope, &claude, at(10)));
        assert!(
            !record_provider_use(&mut scope, &claude, at(20)),
            "repeated turns on the most recent instance do not rewrite device state"
        );
        assert_eq!(scope.provider_instances[&claude].last_used_at, Some(at(10)));
        assert!(record_provider_use(&mut scope, &codex, at(30)));
        assert!(record_provider_use(&mut scope, &claude, at(40)));
        assert!(!record_provider_use(
            &mut scope,
            &instance("removed"),
            at(50)
        ));
    }
}
