//! Tauri commands for the sync status, controls, recovery offers, quarantine, the authority
//! read models, and the authority actions of an admin desktop.

use super::authority::{self, AuthorityView, LateChangeView, OutboxEntryView};
use super::host::SyncHost;
use super::recovery::{self, RecoveryAction, RecoveryChoice, RecoveryEntryView};
#[cfg(desktop)]
use super::service::{AuthorityAction, AuthorityAnswer, LateDecision, PendingRecoveryKit};
use super::service::{SyncRuntime, blocking};
use super::status::{SyncNotice, SyncStatusView, SyncSubscribers};
#[cfg(desktop)]
use ganbaru_sync_contracts::LateResolution;
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

/// The recovery kit whose seed this desktop still holds because the user has not confirmed
/// saving it, such as the kit founding a charter creates, or none.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_recovery_kit_pending(
    runtime: State<'_, SyncRuntime>,
) -> Result<Option<PendingRecoveryKit>, String> {
    match run_authority(&runtime, AuthorityAction::PendingRecoveryKit).await? {
        AuthorityAnswer::PendingRecoveryKit(kit) => Ok(kit),
        _ => Err("sync answered the recovery kit request with something else".to_string()),
    }
}

/// Deletes the recovery seed after the user confirmed saving the kit, so no device keeps it.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_recovery_kit_saved(runtime: State<'_, SyncRuntime>) -> Result<(), String> {
    run_authority(&runtime, AuthorityAction::RecoveryKitSaved)
        .await
        .map(|_| ())
}

/// Uses a recovery kit on this enrolled desktop: proposes, signed with the recovery admin key
/// the code derives, making this desktop an admin, so it can take the anchor once the anchor
/// sequences the proposal. The derived key is never stored.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_use_recovery_kit(
    runtime: State<'_, SyncRuntime>,
    code: String,
) -> Result<(), String> {
    run_authority(&runtime, AuthorityAction::UseRecoveryKit(code))
        .await
        .map(|_| ())
}

/// The selected authorization state of the active vault's sync space: the anchor, every
/// enrolled device with its role, and whether this installation is an admin. `None` before the
/// vault joined a sync space.
#[tauri::command]
pub async fn sync_authority<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<AuthorityView>, String> {
    authority::read(&app).await
}

/// The authorization proposals this device made that wait for the anchor or that the anchor
/// refused, newest first.
#[tauri::command]
pub async fn sync_outbox_list<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<OutboxEntryView>, String> {
    authority::outbox(&app).await
}

/// Rows removed devices changed after their removal cutoff that no admin decided yet.
#[tauri::command]
pub async fn sync_late_changes<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<LateChangeView>, String> {
    authority::late_changes(&app).await
}

/// A late change, by removal revision, table name, and row key.
#[cfg(desktop)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LateChangeTarget {
    removal_revision: u64,
    table: String,
    row_key: String,
}

/// On an admin desktop, applies a late change with the local writer and proposes restoring it,
/// so every device records the decision once the anchor sequences it.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_late_change_restore(
    runtime: State<'_, SyncRuntime>,
    target: LateChangeTarget,
) -> Result<(), String> {
    resolve_late(&runtime, target, LateResolution::Restore).await
}

/// On an admin desktop, proposes discarding a late change for good.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_late_change_discard(
    runtime: State<'_, SyncRuntime>,
    target: LateChangeTarget,
) -> Result<(), String> {
    resolve_late(&runtime, target, LateResolution::Discard).await
}

/// On an admin desktop, proposes removing another enrolled device from the vault. The removal
/// is permanent once the anchor sequences it; the anchor itself cannot be removed.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_remove_device(
    runtime: State<'_, SyncRuntime>,
    device_id: String,
) -> Result<(), String> {
    run_authority(&runtime, AuthorityAction::Remove(device_id)).await?;
    runtime.sync_now();
    Ok(())
}

#[cfg(desktop)]
async fn resolve_late(
    runtime: &SyncRuntime,
    target: LateChangeTarget,
    resolution: LateResolution,
) -> Result<(), String> {
    let decision = late_decision(target, resolution)?;
    run_authority(runtime, AuthorityAction::ResolveLate(decision)).await?;
    runtime.sync_now();
    Ok(())
}

/// Resolves the table name of a late change target against the vault manifest.
#[cfg(desktop)]
fn late_decision(
    target: LateChangeTarget,
    resolution: LateResolution,
) -> Result<LateDecision, String> {
    let table = ganbaru_sync::Engine::vault()
        .manifest()
        .table_named(&target.table)
        .ok_or_else(|| "the late change names an unknown table".to_string())?
        .id;
    if target.removal_revision == 0 {
        return Err("the late change names no removal".to_string());
    }
    Ok(LateDecision {
        removal_revision: target.removal_revision,
        table,
        row_key: target.row_key,
        resolution,
    })
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

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    fn target(removal_revision: u64, table: &str) -> LateChangeTarget {
        LateChangeTarget {
            removal_revision,
            table: table.to_string(),
            row_key: "note-late".to_string(),
        }
    }

    #[test]
    fn sync_late_decision_resolves_the_table_by_name() {
        let decision =
            late_decision(target(3, "quick_notes"), LateResolution::Discard).expect("decision");
        let table = ganbaru_sync::Engine::vault()
            .manifest()
            .table_named("quick_notes")
            .expect("quick notes table")
            .id;
        assert_eq!(
            decision,
            LateDecision {
                removal_revision: 3,
                table,
                row_key: "note-late".to_string(),
                resolution: LateResolution::Discard,
            }
        );
    }

    #[test]
    fn sync_late_decision_refuses_unknown_tables_and_missing_removals() {
        assert!(late_decision(target(3, "sync_ops"), LateResolution::Restore).is_err());
        assert!(late_decision(target(0, "quick_notes"), LateResolution::Restore).is_err());
    }
}
