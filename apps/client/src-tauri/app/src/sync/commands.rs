//! Tauri commands for the sync status, controls, recovery offers, quarantine, and the
//! authority actions of an admin desktop.

use super::host::SyncHost;
use super::recovery::{self, RecoveryAction, RecoveryChoice, RecoveryEntryView};
#[cfg(desktop)]
use super::service::{AuthorityAction, AuthorityAnswer};
use super::service::{SyncRuntime, blocking};
use super::status::{SyncNotice, SyncStatusView, SyncSubscribers};
use ganbaru_sync_replica::quarantine::Quarantine;
use serde::Deserialize;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime, State, Webview};

/// How long a recovery action may wait for the service to run it.
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long Reset sync state may wait for the service to run it.
#[cfg(desktop)]
const RESET_TIMEOUT: Duration = Duration::from_secs(60);
/// How long an authority action may wait for the service to run it.
#[cfg(desktop)]
const AUTHORITY_TIMEOUT: Duration = Duration::from_secs(60);
/// Serializes changes of the quarantine file, so two commands never drop each other's change.
static QUARANTINE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
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

/// Resets sync state on the coordinating computer: unlinks every device linked for the vault,
/// clears the engine tables and this installation's writer state, and founds a new space that
/// republishes every current row. Unlinked devices pair again. A client refuses it.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_reset_state(runtime: State<'_, SyncRuntime>) -> Result<(), String> {
    let reply = runtime.request_reset()?;
    tokio::time::timeout(RESET_TIMEOUT, reply)
        .await
        .map_err(|_| "sync did not reset its state in time".to_string())?
        .map_err(|_| "sync stopped before resetting its state".to_string())?
}

/// The devices this installation quarantined in the active vault.
#[tauri::command]
pub async fn sync_quarantine_list<R: Runtime>(app: AppHandle<R>) -> Result<Vec<String>, String> {
    let path = quarantine_path(&app)?;
    let quarantine = blocking(move || Quarantine::load(&path)).await?;
    Ok(quarantine.devices().iter().cloned().collect())
}

/// Quarantines another device of the active vault on this installation at once: its
/// connections are refused and the operations its writers sealed are not stored. On an admin
/// desktop it also proposes removing the device from the vault, which is permanent once the
/// anchor sequences it.
#[tauri::command]
pub async fn sync_quarantine_device<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, SyncRuntime>,
    device_id: String,
) -> Result<(), String> {
    if device_id == app.device_id()? {
        return Err("this device cannot quarantine itself".to_string());
    }
    let path = quarantine_path(&app)?;
    {
        let _guard = QUARANTINE_LOCK.lock().await;
        let target = device_id.clone();
        blocking(move || Quarantine::load(&path)?.add(&target)).await?;
    }
    #[cfg(desktop)]
    runtime.request_authority_detached(AuthorityAction::ProposeRemoval(device_id))?;
    runtime.sync_now();
    Ok(())
}

/// Lifts the quarantine of a device on this installation, so its operations arrive again. A
/// removal the anchor already sequenced stays.
#[tauri::command]
pub async fn sync_quarantine_lift<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, SyncRuntime>,
    device_id: String,
) -> Result<(), String> {
    let path = quarantine_path(&app)?;
    {
        let _guard = QUARANTINE_LOCK.lock().await;
        blocking(move || Quarantine::load(&path)?.lift(&device_id)).await?;
    }
    runtime.sync_now();
    Ok(())
}

/// Seals a handover that makes this linked admin desktop the anchor of the vault's sync space,
/// after an exchange that leaves it current with the coordinator. Returns the authorization
/// revision of the handover.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_take_anchor(runtime: State<'_, SyncRuntime>) -> Result<u64, String> {
    match run_authority(&runtime, AuthorityAction::TakeAnchor).await? {
        AuthorityAnswer::Anchored(revision) => Ok(revision),
        _ => Err("sync answered taking the anchor with something else".to_string()),
    }
}

/// On the coordinating computer, proposes making a linked desktop an admin with the admin key
/// it sent at pairing, so it can take the anchor.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_promote_device(
    runtime: State<'_, SyncRuntime>,
    device_id: String,
) -> Result<(), String> {
    run_authority(&runtime, AuthorityAction::Promote(device_id))
        .await
        .map(|_| ())
}

/// Replaces the vault's recovery kit on an admin desktop and returns the new kit code, which is
/// shown once. The old kit stops signing once the anchor sequences the change.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_regenerate_recovery_kit(
    runtime: State<'_, SyncRuntime>,
) -> Result<String, String> {
    match run_authority(&runtime, AuthorityAction::RegenerateRecoveryKit).await? {
        AuthorityAnswer::RecoveryKit(code) => Ok(code),
        _ => Err("sync answered the recovery kit request with something else".to_string()),
    }
}

#[cfg(desktop)]
async fn run_authority(
    runtime: &SyncRuntime,
    action: AuthorityAction,
) -> Result<AuthorityAnswer, String> {
    let reply = runtime.request_authority(action)?;
    tokio::time::timeout(AUTHORITY_TIMEOUT, reply)
        .await
        .map_err(|_| "sync did not run the request in time".to_string())?
        .map_err(|_| "sync stopped before the request ran".to_string())?
}

fn quarantine_path<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, String> {
    let vault_id = app.active_vault_id()?;
    Ok(app.sync_files(&vault_id)?.quarantine_path())
}

async fn close_recovery(
    runtime: &SyncRuntime,
    target: RecoveryTarget,
    action: RecoveryAction,
) -> Result<Option<String>, String> {
    let table = recovery::restorers().table(&target.table)?;
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
