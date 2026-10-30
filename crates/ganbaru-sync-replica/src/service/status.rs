//! The sync status a device reports: its state, role, counts, connection, and degraded reasons,
//! in the wire names the frontend reads.

use ganbaru_handoff::dial::{DialAddressClass, validate_dial_endpoint};
use ganbaru_sync::stability::{DeviceAcknowledgement, DeviceStanding};
use ganbaru_sync::{ClockWarning, DeviceNames, DeviceRef, SyncStatus};
use serde::Serialize;
use std::collections::BTreeSet;
use std::net::SocketAddr;

/// What the service is doing for the active vault.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// The vault is not linked to other devices.
    #[default]
    Off,
    /// Current with the hub, or the hub with nothing to do.
    Idle,
    /// An exchange is running.
    Syncing,
    /// The hub cannot be reached; changes queue locally.
    Offline,
    /// Exchanges are paused on this device; sealing continues.
    Paused,
    /// The person key has not reached this device yet.
    WaitingForIdentity,
    /// The last exchange or pass failed.
    Error,
}

/// The side of the exchange this device takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncRole {
    /// The coordinator, which linked devices exchange with.
    Hub,
    /// A linked device.
    Client,
}

/// Stored operations that cannot be applied, by reason.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldView {
    pub newer_format: usize,
    pub newer_manifest: usize,
    pub invalid: usize,
}

/// A count for one replicated table, by name, so the frontend presents it through the
/// registration of the owning domain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCount {
    pub table: String,
    pub count: usize,
}

impl TableCount {
    fn list(counts: &[(&'static str, usize)]) -> Vec<Self> {
        counts
            .iter()
            .map(|(table, count)| Self {
                table: (*table).to_string(),
                count: *count,
            })
            .collect()
    }
}

/// A sync problem the interface explains in its own words rather than with `last_error`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncErrorCode {
    /// The hub refused a new writer of this device, which reached the writers a device or the
    /// vault may have.
    WriterLimit,
    /// The hub compacted operations this device lacks, so it pulls nothing until it receives
    /// a fresh copy of the vault. Local changes still reach the hub.
    BootstrapRequired,
    /// The selected log removed the coordinator this device is linked to, so it exchanges only
    /// authorization records with it and syncs with the other members until it is linked to
    /// another computer or its copy is recovered.
    CoordinatorRemoved,
}

/// Why the coordinator does not count a linked device toward stability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceStandingView {
    /// The device stored nothing for long enough that compaction no longer waits for it.
    Dormant,
    /// The link record has no valid time; the device must be linked again.
    RelinkRequired,
}

/// A linked device the coordinator does not count, with the reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UncountedDevice {
    pub device: DeviceRef,
    pub standing: DeviceStandingView,
}

/// The kind of network a linked device reached its coordinator over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncConnectionNetwork {
    /// A private, link-local, or on-link address of the local network.
    Lan,
    /// The shared address space of overlay networks such as Tailscale.
    Overlay,
}

/// How this linked device reached its coordinator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConnectionView {
    pub network: SyncConnectionNetwork,
    /// The coordinator endpoint, as `address:port`.
    pub address: String,
}

impl SyncConnectionView {
    /// The connection through `endpoint`, or `None` for an endpoint no dial would reach.
    pub fn reached(endpoint: SocketAddr) -> Option<Self> {
        let network = match validate_dial_endpoint(endpoint).ok()? {
            DialAddressClass::Overlay => SyncConnectionNetwork::Overlay,
            DialAddressClass::PrivateLan
            | DialAddressClass::GlobalIpv6
            | DialAddressClass::Loopback => SyncConnectionNetwork::Lan,
        };
        Some(Self {
            network,
            address: endpoint.to_string(),
        })
    }
}

/// What the last relay pass of this app run moved, and what still waits.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayDeliveryView {
    /// When the pass finished, in Unix milliseconds.
    pub last_pass_at_ms: u64,
    /// Sealed records the pass delivered to the relay.
    pub sent: usize,
    /// Sealed records the pass received from the relay.
    pub received: usize,
    /// Operations of this installation's writers the relay does not hold yet.
    pub undelivered: u64,
    pub uploaded_files: usize,
    pub downloaded_files: usize,
    /// Files left for a later pass on a metered network.
    pub deferred_files: usize,
    pub stored_bytes: u64,
    pub quota_bytes: u64,
    /// Why the pass stopped early or what the relay needs from an administrator.
    pub problem: Option<String>,
}

impl RelayDeliveryView {
    /// The counts of `pass`, finished at `now_ms`.
    pub fn of(pass: &crate::relay::RelayPass, now_ms: u64) -> Self {
        Self {
            last_pass_at_ms: now_ms,
            sent: pass.pushed,
            received: pass.pulled,
            undelivered: pass.undelivered,
            uploaded_files: pass.uploaded,
            downloaded_files: pass.downloaded,
            deferred_files: pass.deferred_assets,
            stored_bytes: pass.stored_bytes,
            quota_bytes: pass.quota_bytes,
            problem: pass.problem.clone(),
        }
    }

    /// A pass that could not reach or use the relay at `now_ms`.
    pub fn failed(problem: String, now_ms: u64) -> Self {
        Self {
            last_pass_at_ms: now_ms,
            problem: Some(problem),
            ..Self::default()
        }
    }
}

/// Why this device may sync late or not at all, each with a recovery action the interface
/// offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncDegradedReason {
    /// Android restricts the app's background work.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    BackgroundRestricted,
    /// Android placed the app in the restricted standby bucket.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    StandbyRestricted,
    /// The system refused connections to the local network.
    LocalNetworkDenied,
    /// A firewall blocks peers or discovery until the user allows the local network.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    FirewallAuthorizationRequired,
    /// A firewall blocks other members from reaching this member desktop's listener until the
    /// user allows peers to connect.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    FirewallPeerListenerBlocked,
}

/// The degraded reasons of one pass, without repeats and in a fixed order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DegradedReasons(BTreeSet<SyncDegradedReason>);

impl DegradedReasons {
    /// Adds `reason` when `present` holds.
    pub fn note(&mut self, reason: SyncDegradedReason, present: bool) {
        if present {
            self.0.insert(reason);
        }
    }

    pub fn into_vec(self) -> Vec<SyncDegradedReason> {
        self.0.into_iter().collect()
    }
}

/// The sync status of the active vault.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusView {
    pub state: SyncState,
    pub role: Option<SyncRole>,
    pub paused: bool,
    pub last_exchange_at_ms: Option<u64>,
    pub last_error: Option<String>,
    pub last_error_code: Option<SyncErrorCode>,
    pub pending_local_changes: usize,
    pub waiting: usize,
    pub held: HeldView,
    /// Rows with open conflicts, per table.
    pub conflicts: Vec<TableCount>,
    /// Recovery offers, per table of their root row.
    pub recovery: Vec<TableCount>,
    /// Changes of this device the anchor has not confirmed storing. A removal sequenced before
    /// the anchor stores them excludes them, so the interface asks to reach the anchor.
    pub unconfirmed: usize,
    /// When the oldest unconfirmed change was stored, in Unix milliseconds.
    pub oldest_unconfirmed_at_ms: Option<u64>,
    /// Superseded versions this device keeps until the anchor confirms the changes that
    /// superseded them.
    pub displaced_versions: usize,
    /// On a linked device, since when no pass in this app run reached the anchor, directly or
    /// as the coordinator, in Unix milliseconds. `None` while the anchor answers, on the
    /// coordinator, and before the first pass.
    pub anchor_unreachable_since_ms: Option<u64>,
    /// Devices whose merged edits carry clocks far ahead of this device, so they win
    /// concurrent edits until the clocks agree.
    pub clock_ahead_devices: Vec<DeviceRef>,
    /// On the coordinator, linked devices it does not count toward stability, in device id
    /// order.
    pub uncounted_devices: Vec<UncountedDevice>,
    /// How a linked device last reached its coordinator in this app run. It stays after a
    /// failed dial, so the interface pairs it with `state`.
    pub connection: Option<SyncConnectionView>,
    /// Why connectivity is degraded, in a fixed order.
    pub degraded: Vec<SyncDegradedReason>,
    /// The last relay pass of this app run, while the sync space uses a relay.
    pub relay: Option<RelayDeliveryView>,
}

impl SyncStatusView {
    /// Copies the engine counts into the view.
    pub fn set_counts(&mut self, status: &SyncStatus) {
        self.pending_local_changes = status.pending_captures;
        self.waiting = status.waiting;
        self.held = HeldView {
            newer_format: status.held.newer_format,
            newer_manifest: status.held.newer_manifest,
            invalid: status.held.invalid,
        };
        self.conflicts = TableCount::list(&status.conflicts);
        self.recovery = TableCount::list(&status.recovery);
        self.unconfirmed = status.unconfirmed;
        self.oldest_unconfirmed_at_ms = status.oldest_unconfirmed_ms;
        self.displaced_versions = status.displaced;
    }

    /// Names the linked devices the coordinator does not count, with why, in device id order.
    pub fn set_device_standings(&mut self, devices: &[DeviceAcknowledgement], names: &DeviceNames) {
        let mut uncounted: Vec<(&str, DeviceStandingView)> = devices
            .iter()
            .filter_map(|device| {
                let standing = match device.standing {
                    DeviceStanding::Counted => return None,
                    DeviceStanding::Dormant => DeviceStandingView::Dormant,
                    DeviceStanding::RelinkRequired => DeviceStandingView::RelinkRequired,
                };
                Some((device.device_id.as_str(), standing))
            })
            .collect();
        uncounted.sort();
        self.uncounted_devices = uncounted
            .into_iter()
            .map(|(device_id, standing)| UncountedDevice {
                device: names.describe(device_id),
                standing,
            })
            .collect();
    }

    /// Names the devices of writers with far-ahead clocks, once per device, in device id order.
    /// A writer missing from `status` has no known device and is skipped.
    pub fn set_clock_ahead(
        &mut self,
        status: &SyncStatus,
        warnings: &[ClockWarning],
        devices: &DeviceNames,
    ) {
        let device_ids: BTreeSet<&str> = warnings
            .iter()
            .filter_map(|warning| {
                status
                    .writers
                    .iter()
                    .find(|summary| summary.writer == warning.writer)
                    .map(|summary| summary.device_id.as_str())
            })
            .collect();
        self.clock_ahead_devices = device_ids
            .into_iter()
            .map(|device_id| devices.describe(device_id))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_sync::{WriterState, WriterSummary};
    use ganbaru_sync_contracts::{Hlc, WriterId};
    use std::collections::HashMap;

    fn summary(writer: WriterId, device_id: &str) -> WriterSummary {
        WriterSummary {
            writer,
            device_id: device_id.to_string(),
            state: WriterState::Active,
            stored: 1,
            applied: 1,
            cutoff: None,
        }
    }

    #[test]
    fn far_ahead_writers_name_each_device_once() {
        let laptop_old = WriterId::from_bytes([1; 16]);
        let laptop_new = WriterId::from_bytes([2; 16]);
        let phone = WriterId::from_bytes([3; 16]);
        let unknown = WriterId::from_bytes([4; 16]);
        let status = SyncStatus {
            writers: vec![
                summary(laptop_old, "laptop"),
                summary(laptop_new, "laptop"),
                summary(phone, "phone"),
            ],
            ..SyncStatus::default()
        };
        let clock = Hlc::new(u64::MAX, 0);
        let warnings =
            [laptop_new, unknown, laptop_old].map(|writer| ClockWarning { writer, clock });
        let devices = DeviceNames::new(
            "phone",
            HashMap::from([("laptop".to_string(), "Work laptop".to_string())]),
        );
        let mut view = SyncStatusView::default();
        view.set_clock_ahead(&status, &warnings, &devices);
        assert_eq!(view.clock_ahead_devices, vec![devices.describe("laptop")]);
        assert_eq!(
            view.clock_ahead_devices[0].device_label.as_deref(),
            Some("Work laptop")
        );

        view.set_clock_ahead(&status, &[], &devices);
        assert!(view.clock_ahead_devices.is_empty());
    }

    #[test]
    fn uncounted_devices_are_named_in_device_id_order_with_their_standing() {
        let device = |device_id: &str, standing| DeviceAcknowledgement {
            device_id: device_id.to_string(),
            acknowledged: ganbaru_sync_contracts::VersionVector::new(),
            last_stored_ms: None,
            standing,
        };
        let devices = [
            device("tablet", DeviceStanding::RelinkRequired),
            device("laptop", DeviceStanding::Counted),
            device("phone", DeviceStanding::Dormant),
        ];
        let names = DeviceNames::new(
            "hub",
            HashMap::from([("phone".to_string(), "Old phone".to_string())]),
        );
        let mut view = SyncStatusView::default();
        view.set_device_standings(&devices, &names);
        let wire = serde_json::to_value(&view).expect("serialize status");
        let uncounted = wire["uncountedDevices"]
            .as_array()
            .expect("uncounted devices");
        assert_eq!(uncounted.len(), 2);
        assert_eq!(uncounted[0]["standing"], "dormant");
        assert_eq!(uncounted[0]["device"]["deviceLabel"], "Old phone");
        assert_eq!(uncounted[1]["standing"], "relinkRequired");
        assert_eq!(
            serde_json::to_value(SyncErrorCode::BootstrapRequired).expect("serialize code"),
            "bootstrapRequired"
        );

        view.set_device_standings(&devices[1..2], &names);
        assert!(view.uncounted_devices.is_empty());
    }

    #[test]
    fn connection_and_degraded_reasons_reach_the_frontend_in_its_wire_names() {
        let mut reasons = DegradedReasons::default();
        reasons.note(SyncDegradedReason::FirewallPeerListenerBlocked, true);
        reasons.note(SyncDegradedReason::FirewallAuthorizationRequired, true);
        reasons.note(SyncDegradedReason::StandbyRestricted, false);
        reasons.note(SyncDegradedReason::LocalNetworkDenied, true);
        reasons.note(SyncDegradedReason::BackgroundRestricted, true);
        reasons.note(SyncDegradedReason::LocalNetworkDenied, true);
        let view = SyncStatusView {
            connection: SyncConnectionView::reached("100.64.0.7:43821".parse().expect("endpoint")),
            degraded: reasons.into_vec(),
            ..SyncStatusView::default()
        };
        let wire = serde_json::to_value(&view).expect("serialize status");
        assert_eq!(
            wire["connection"],
            serde_json::json!({ "network": "overlay", "address": "100.64.0.7:43821" })
        );
        assert_eq!(
            wire["degraded"],
            serde_json::json!([
                "backgroundRestricted",
                "localNetworkDenied",
                "firewallAuthorizationRequired",
                "firewallPeerListenerBlocked"
            ])
        );
        let empty = serde_json::to_value(SyncStatusView::default()).expect("serialize status");
        assert_eq!(empty["connection"], serde_json::Value::Null);
        assert_eq!(empty["degraded"], serde_json::json!([]));
        assert_eq!(empty["anchorUnreachableSinceMs"], serde_json::Value::Null);
        assert!(
            empty
                .as_object()
                .is_some_and(|fields| fields.contains_key("anchorUnreachableSinceMs"))
        );
    }

    #[test]
    fn relay_delivery_counts_reach_the_frontend_in_its_wire_names() {
        let pass = crate::relay::RelayPass {
            pushed: 4,
            pulled: 2,
            undelivered: 1,
            uploaded: 1,
            deferred_assets: 3,
            stored_bytes: 4_096,
            quota_bytes: 1_048_576,
            ..Default::default()
        };
        let view = SyncStatusView {
            relay: Some(RelayDeliveryView::of(&pass, 900)),
            ..SyncStatusView::default()
        };
        let wire = serde_json::to_value(&view).expect("serialize status");
        assert_eq!(
            wire["relay"],
            serde_json::json!({
                "lastPassAtMs": 900,
                "sent": 4,
                "received": 2,
                "undelivered": 1,
                "uploadedFiles": 1,
                "downloadedFiles": 0,
                "deferredFiles": 3,
                "storedBytes": 4_096,
                "quotaBytes": 1_048_576,
                "problem": null,
            })
        );
        let failed = RelayDeliveryView::failed("relay unreachable".to_string(), 7);
        assert_eq!(failed.problem.as_deref(), Some("relay unreachable"));
        assert_eq!((failed.sent, failed.last_pass_at_ms), (0, 7));
        let empty = serde_json::to_value(SyncStatusView::default()).expect("serialize status");
        assert_eq!(empty["relay"], serde_json::Value::Null);
    }

    #[test]
    fn sync_status_reports_conflict_and_recovery_counts_per_table() {
        let status = SyncStatus {
            conflicts: vec![("quick_notes", 2), ("quick_note_tags", 1)],
            recovery: vec![("quick_notes", 3)],
            ..SyncStatus::default()
        };
        let mut view = SyncStatusView::default();
        view.set_counts(&status);
        let count = |table: &str, count| TableCount {
            table: table.to_string(),
            count,
        };
        assert_eq!(
            view.conflicts,
            vec![count("quick_notes", 2), count("quick_note_tags", 1)]
        );
        assert_eq!(view.recovery, vec![count("quick_notes", 3)]);
        let wire = serde_json::to_value(&view).expect("serialize status");
        assert_eq!(
            wire["recovery"],
            serde_json::json!([{ "table": "quick_notes", "count": 3 }])
        );

        view.set_counts(&SyncStatus::default());
        assert!(view.conflicts.is_empty() && view.recovery.is_empty());
    }

    #[test]
    fn sync_status_reports_unconfirmed_changes_and_their_age() {
        let status = SyncStatus {
            unconfirmed: 4,
            oldest_unconfirmed_ms: Some(1_800_000_000_000),
            displaced: 3,
            ..SyncStatus::default()
        };
        let mut view = SyncStatusView::default();
        view.set_counts(&status);
        let wire = serde_json::to_value(&view).expect("serialize status");
        assert_eq!(wire["unconfirmed"], serde_json::json!(4));
        assert_eq!(wire["displacedVersions"], serde_json::json!(3));
        assert_eq!(
            wire["oldestUnconfirmedAtMs"],
            serde_json::json!(1_800_000_000_000_u64)
        );

        view.set_counts(&SyncStatus::default());
        let wire = serde_json::to_value(&view).expect("serialize status");
        assert_eq!(wire["unconfirmed"], serde_json::json!(0));
        assert_eq!(wire["oldestUnconfirmedAtMs"], serde_json::Value::Null);
    }
}
