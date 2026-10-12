//! The sync status the frontend shows, and the subscription channels that carry it. Channels
//! work in every shell, including Android, which keeps the global event module out of its bundle.

use ganbaru_sync::SyncStatus;
use serde::Serialize;
use std::collections::BTreeMap;
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

/// Rows with an open conflict in one table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictCount {
    pub table: String,
    pub count: usize,
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
    pub pending_local_changes: usize,
    pub waiting: usize,
    pub held: HeldView,
    pub conflicts: Vec<ConflictCount>,
    pub recovery_count: usize,
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
        self.conflicts = status
            .conflicts
            .iter()
            .map(|(table, count)| ConflictCount {
                table: (*table).to_string(),
                count: *count,
            })
            .collect();
        self.recovery_count = status.recovery;
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
