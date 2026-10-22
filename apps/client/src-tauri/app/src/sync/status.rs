//! The sync status the frontend shows, and the subscription channels that carry it. Channels
//! work in every shell, including Android, which keeps the global event module out of its bundle.

use ganbaru_handoff::dial::{DialAddressClass, validate_dial_endpoint};
use ganbaru_sync::{ClockWarning, DeviceNames, DeviceRef, SyncStatus};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::{Mutex, MutexGuard};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, Runtime};

/// What the service is doing for the active vault.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SyncState {
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
pub(crate) enum SyncRole {
    /// The coordinator, which linked devices exchange with.
    Hub,
    /// A linked device.
    Client,
}

/// Stored operations that cannot be applied, by reason.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HeldView {
    pub newer_format: usize,
    pub newer_manifest: usize,
    pub invalid: usize,
}

/// A count for one replicated table, by name, so the frontend presents it through the
/// registration of the owning domain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TableCount {
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
pub(crate) enum SyncErrorCode {
    /// The hub refused a new writer of this device, which reached the writers a device or the
    /// vault may have.
    WriterLimit,
}

/// The kind of network a linked device reached its coordinator over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SyncConnectionNetwork {
    /// A private, link-local, or on-link address of the local network.
    Lan,
    /// The shared address space of overlay networks such as Tailscale.
    Overlay,
}

/// How this linked device reached its coordinator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConnectionView {
    pub network: SyncConnectionNetwork,
    /// The coordinator endpoint, as `address:port`.
    pub address: String,
}

impl SyncConnectionView {
    /// The connection through `endpoint`, or `None` for an endpoint no dial would reach.
    pub(crate) fn reached(endpoint: SocketAddr) -> Option<Self> {
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

/// Why this device may sync late or not at all, each with a recovery action the interface
/// offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SyncDegradedReason {
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
}

/// The degraded reasons of one pass, without repeats and in a fixed order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DegradedReasons(BTreeSet<SyncDegradedReason>);

impl DegradedReasons {
    /// Adds `reason` when `present` holds.
    pub(crate) fn note(&mut self, reason: SyncDegradedReason, present: bool) {
        if present {
            self.0.insert(reason);
        }
    }

    pub(crate) fn into_vec(self) -> Vec<SyncDegradedReason> {
        self.0.into_iter().collect()
    }
}

/// The sync status of the active vault.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncStatusView {
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
    /// Devices whose merged edits carry clocks far ahead of this device, so they win
    /// concurrent edits until the clocks agree.
    pub clock_ahead_devices: Vec<DeviceRef>,
    /// How a linked device last reached its coordinator in this app run. It stays after a
    /// failed dial, so the interface pairs it with `state`.
    pub connection: Option<SyncConnectionView>,
    /// Why connectivity is degraded, in a fixed order.
    pub degraded: Vec<SyncDegradedReason>,
}

impl SyncStatusView {
    /// Copies the engine counts into the view.
    pub(crate) fn set_counts(&mut self, status: &SyncStatus) {
        self.pending_local_changes = status.pending_captures;
        self.waiting = status.waiting;
        self.held = HeldView {
            newer_format: status.held.newer_format,
            newer_manifest: status.held.newer_manifest,
            invalid: status.held.invalid,
        };
        self.conflicts = TableCount::list(&status.conflicts);
        self.recovery = TableCount::list(&status.recovery);
    }

    /// Names the devices of writers with far-ahead clocks, once per device, in device id order.
    /// A writer missing from `status` has no known device and is skipped.
    pub(crate) fn set_clock_ahead(
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

/// A notice on a sync subscription channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum SyncNotice {
    /// The status changed.
    Status { status: SyncStatusView },
    /// Replicated changes rewrote rows of these tables, for frontend invalidation.
    Applied { tables: Vec<String> },
}

struct Subscriber {
    id: String,
    channel: Channel<SyncNotice>,
}

/// Notice channels by WebView label. Each WebView keeps its latest subscription, so a reload
/// replaces the channel of the previous page.
#[derive(Default)]
pub(crate) struct SyncSubscribers(Mutex<BTreeMap<String, Subscriber>>);

impl SyncSubscribers {
    pub(crate) fn subscribe(
        &self,
        webview: String,
        id: String,
        channel: Channel<SyncNotice>,
    ) -> Result<(), String> {
        self.lock()?.insert(webview, Subscriber { id, channel });
        Ok(())
    }

    /// Removes the subscription of `webview` when `id` is still its current one.
    pub(crate) fn unsubscribe(&self, webview: &str, id: &str) -> Result<(), String> {
        let mut subscribers = self.lock()?;
        if subscribers
            .get(webview)
            .is_some_and(|subscriber| subscriber.id == id)
        {
            subscribers.remove(webview);
        }
        Ok(())
    }

    /// Sends `notice` to every open WebView, dropping subscriptions of closed ones and
    /// channels that fail.
    fn notify<R: Runtime>(&self, app: &AppHandle<R>, notice: &SyncNotice) {
        let Ok(mut subscribers) = self.lock() else {
            eprintln!("sync subscribers are unavailable");
            return;
        };
        subscribers.retain(|webview, subscriber| {
            webview_is_open(app, webview)
                && match subscriber.channel.send(notice.clone()) {
                    Ok(()) => true,
                    Err(error) => {
                        eprintln!("send sync notice to {webview}: {error}");
                        false
                    }
                }
        });
    }

    fn lock(&self) -> Result<MutexGuard<'_, BTreeMap<String, Subscriber>>, String> {
        self.0
            .lock()
            .map_err(|_| "sync subscribers are unavailable".to_string())
    }
}

/// Whether the WebView labeled `label` is still open, including child WebViews of a window.
#[cfg(desktop)]
fn webview_is_open<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    app.get_webview(label).is_some()
}

/// Whether the WebView labeled `label` is still open. Mobile builds have no child WebViews, so
/// every WebView belongs to a webview window.
#[cfg(mobile)]
fn webview_is_open<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    app.get_webview_window(label).is_some()
}

fn notify<R: Runtime>(app: &AppHandle<R>, notice: &SyncNotice) {
    if let Some(subscribers) = app.try_state::<SyncSubscribers>() {
        subscribers.notify(app, notice);
    }
}

pub(crate) fn emit_status<R: Runtime>(app: &AppHandle<R>, view: &SyncStatusView) {
    notify(
        app,
        &SyncNotice::Status {
            status: view.clone(),
        },
    );
}

pub(crate) fn emit_applied<R: Runtime>(app: &AppHandle<R>, tables: Vec<String>) {
    if tables.is_empty() {
        return;
    }
    notify(app, &SyncNotice::Applied { tables });
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
    fn connection_and_degraded_reasons_reach_the_frontend_in_its_wire_names() {
        let mut reasons = DegradedReasons::default();
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
                "firewallAuthorizationRequired"
            ])
        );
        let empty = serde_json::to_value(SyncStatusView::default()).expect("serialize status");
        assert_eq!(empty["connection"], serde_json::Value::Null);
        assert_eq!(empty["degraded"], serde_json::json!([]));
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
}
