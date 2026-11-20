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

#[cfg(desktop)]
mod usage_limits;

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
    /// newest observation wins. A report without usage numbers keeps the numbers the stored
    /// report read for the same windows, as [`usage_limits::carry_usage_numbers`] describes, and
    /// keeps when they were read.
    #[cfg(desktop)]
    pub fn observe_usage_limits(
        &mut self,
        mut status: RateLimitStatusEvent,
        observed_at: UtcTimestamp,
    ) -> bool {
        if self
            .usage_limits
            .as_ref()
            .is_some_and(|current| !is_after(&observed_at, &current.observed_at))
        {
            return false;
        }
        let Some(observed_at_seconds) = epoch_seconds(&observed_at) else {
            return false;
        };
        let previous = self.usage_limits.take();
        let numbers = usage_limits::carry_usage_numbers(
            previous.as_ref().map(|current| &current.status),
            &mut status,
            observed_at_seconds,
        );
        let usage_read_at = if numbers.read {
            Some(observed_at.clone())
        } else if numbers.carried {
            previous.and_then(|current| current.usage_read_at)
        } else {
            None
        };
        self.usage_limits = Some(ProviderUsageLimitsObservation {
            observed_at,
            usage_read_at,
            status,
        });
        true
    }

    /// Whether this device read the instance's usage numbers less than `max_age` before `now`,
    /// from a probe or a turn. A report without numbers, such as a Claude turn's window
    /// statuses, does not count, and neither does a successful probe alone: its limits read is
    /// best effort, so a probe that came back without limits leaves the limits to read again.
    #[cfg(desktop)]
    pub fn has_usage_limits_within(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        max_age: chrono::TimeDelta,
    ) -> bool {
        self.usage_limits
            .as_ref()
            .and_then(|limits| limits.usage_read_at.as_ref())
            .and_then(|read_at| chrono::DateTime::parse_from_rfc3339(read_at.as_str()).ok())
            .is_some_and(|read_at| now.signed_duration_since(read_at) < max_age)
    }
}

/// Usage limits a provider reported, with the time this device observed them.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageLimitsObservation {
    pub observed_at: UtcTimestamp,
    /// When a report last read the usage numbers `status` holds, which a later report without
    /// numbers keeps; `None` when no report read them.
    #[serde(default)]
    pub usage_read_at: Option<UtcTimestamp>,
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

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDeviceScope {
    pub provider_instances: BTreeMap<ProviderInstanceId, ChatProviderDeviceState>,
    pub full_access_trust:
        BTreeMap<ProviderInstanceId, BTreeMap<ProjectWorkingFolderId, UtcTimestamp>>,
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
#[cfg(desktop)]
fn is_after(candidate: &UtcTimestamp, reference: &UtcTimestamp) -> bool {
    let instant = |value: &UtcTimestamp| chrono::DateTime::parse_from_rfc3339(value.as_str()).ok();
    match (instant(candidate), instant(reference)) {
        (Some(candidate), Some(reference)) => candidate > reference,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// The instant `value` names in whole epoch seconds.
#[cfg(desktop)]
fn epoch_seconds(value: &UtcTimestamp) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value.as_str())
        .ok()
        .map(|instant| instant.timestamp())
}

/// Stores a usage limit report for an instance this device keeps; see
/// [`ChatProviderDeviceState::observe_usage_limits`].
#[cfg(desktop)]
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
#[cfg(desktop)]
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
    let updated = update_app_state(app, |state| {
        if state.chat.schema_version != CHAT_DEVICE_STATE_SCHEMA_VERSION {
            return Err("Chat device state schema is unsupported".to_string());
        }
        update(state.chat.scope_mut(&vault_id, &device_id))
    })?;
    // Probes, catalogs, and plan limits land in the device state, so linked devices hear of them.
    #[cfg(desktop)]
    crate::chat::provider_status::wake(app);
    Ok(updated)
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
    /// A Codex-shaped report, which reads its usage numbers itself.
    fn limits_with_numbers() -> RateLimitStatusEvent {
        RateLimitStatusEvent {
            provider_data: Some(ganbaru_chat_contracts::models::VersionedJson {
                schema_version: 1,
                value: serde_json::json!({ "rateLimitsByLimitId": {} }),
            }),
            ..limits(false)
        }
    }

    #[test]
    fn only_a_recent_read_of_usage_numbers_makes_the_limits_fresh() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-04T12:00:00Z")
            .unwrap()
            .to_utc();
        let hour = chrono::TimeDelta::hours(1);
        let mut device = ChatProviderDeviceState::default();
        assert!(
            !device.has_usage_limits_within(now, hour),
            "an instance never read is stale"
        );

        device.last_successful_probe_at = Some(at(11 * 3600 + 59 * 60));
        assert!(
            !device.has_usage_limits_within(now, hour),
            "a recent probe that returned no limits leaves them to read again"
        );

        device.observe_usage_limits(limits_with_numbers(), at(11 * 3600));
        assert!(
            !device.has_usage_limits_within(now, hour),
            "exactly the maximum age is stale"
        );
        device.observe_usage_limits(
            limits_with_numbers(),
            UtcTimestamp::new("2026-08-04T11:00:01Z").unwrap(),
        );
        assert!(device.has_usage_limits_within(now, hour));
    }

    #[test]
    fn a_report_without_numbers_keeps_when_the_numbers_were_read() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-04T12:00:00Z")
            .unwrap()
            .to_utc();
        let hour = chrono::TimeDelta::hours(1);
        let mut device = ChatProviderDeviceState::default();
        device.observe_usage_limits(limits(false), at(11 * 3600 + 59 * 60));
        assert!(
            !device.has_usage_limits_within(now, hour),
            "a recent report without numbers leaves the numbers to read"
        );

        let mut device = ChatProviderDeviceState::default();
        device.observe_usage_limits(limits_with_numbers(), at(10 * 3600));
        device.observe_usage_limits(limits(true), at(11 * 3600 + 59 * 60));
        let stored = device.usage_limits.as_ref().unwrap();
        assert_eq!(stored.usage_read_at, Some(at(10 * 3600)));
        assert!(stored.status.limited, "the newer report's status stands");
        assert!(
            stored.status.provider_data.is_some(),
            "the numbers of the earlier report stay"
        );
        assert!(
            !device.has_usage_limits_within(now, hour),
            "numbers carried from two hours ago are stale"
        );
    }
}
