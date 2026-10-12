//! Tauri commands for the sync status, controls, and recovery offers.

use super::recovery::{self, RecoveryAction, RecoveryChoice, RecoveryEntryView};
use super::service::SyncRuntime;
use super::status::{SyncNotice, SyncStatusView, SyncSubscribers};
use serde::Deserialize;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime, State, Webview};

/// How long a recovery action may wait for the service to run it.
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest subscription id, which fits a UUID.
const MAX_SUBSCRIPTION_ID_BYTES: usize = 64;

/// The latest sync status of the active vault.
#[tauri::command]
pub fn sync_status(runtime: State<'_, SyncRuntime>) -> Result<SyncStatusView, String> {
    runtime.status()
}

/// Streams status and applied-change notices to the calling WebView, replacing its previous
/// subscription.
#[tauri::command]
pub fn sync_subscribe<R: Runtime>(
    webview: Webview<R>,
    subscribers: State<'_, SyncSubscribers>,
    subscription_id: String,
    channel: Channel<SyncNotice>,
) -> Result<(), String> {
    validate_subscription_id(&subscription_id)?;
    subscribers.subscribe(webview.label().to_string(), subscription_id, channel)
}

/// Ends the calling WebView's subscription when it is still the current one.
#[tauri::command]
pub fn sync_unsubscribe<R: Runtime>(
    webview: Webview<R>,
    subscribers: State<'_, SyncSubscribers>,
    subscription_id: String,
) -> Result<(), String> {
    validate_subscription_id(&subscription_id)?;
    subscribers.unsubscribe(webview.label(), &subscription_id)
}

fn validate_subscription_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > MAX_SUBSCRIPTION_ID_BYTES
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("sync subscription id is invalid".to_string());
    }
    Ok(())
}

/// Runs a sync pass now.
#[tauri::command]
pub fn sync_now(runtime: State<'_, SyncRuntime>) {
    runtime.sync_now();
}

/// Pauses or resumes exchanges on this device. Local changes keep sealing while paused.
#[tauri::command]
pub fn sync_set_paused(runtime: State<'_, SyncRuntime>, paused: bool) -> Result<(), String> {
    runtime.set_paused(paused)
}

/// Deleted rows that kept edits their deletion did not see, newest edit first.
#[tauri::command]
pub async fn sync_recovery_list<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<RecoveryEntryView>, String> {
    recovery::list(&app).await
}

/// A recovery offer, by table name and row key.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryTarget {
    table: String,
    row_key: String,
}

/// Restores a recovery offer as a new row and closes the offer on every device. Returns the
/// new row id.
#[tauri::command]
pub async fn sync_recovery_restore(
    runtime: State<'_, SyncRuntime>,
    target: RecoveryTarget,
) -> Result<Option<String>, String> {
    close_recovery(&runtime, target, RecoveryAction::Restore).await
}

/// Discards a recovery offer on every device.
#[tauri::command]
pub async fn sync_recovery_discard(
    runtime: State<'_, SyncRuntime>,
    target: RecoveryTarget,
) -> Result<(), String> {
    close_recovery(&runtime, target, RecoveryAction::Discard)
        .await
        .map(|_| ())
}

async fn close_recovery(
    runtime: &SyncRuntime,
    target: RecoveryTarget,
    action: RecoveryAction,
) -> Result<Option<String>, String> {
    let table = recovery::recoverable_table(&target.table)?;
    let reply = runtime.request_recovery(RecoveryChoice {
        table,
        row_key: target.row_key,
        action,
    })?;
    tokio::time::timeout(RECOVERY_TIMEOUT, reply)
        .await
        .map_err(|_| "sync did not run the recovery action in time".to_string())?
        .map_err(|_| "sync stopped before the recovery action ran".to_string())?
}
