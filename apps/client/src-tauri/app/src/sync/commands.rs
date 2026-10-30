//! Tauri commands for the sync status, controls, recovery offers, quarantine, the authority
//! read models, and the authority actions of an admin desktop.

use super::authority::{self, AuthorityView, LateChangesView, OutboxEntryView};
use super::contests::{self, ContestsDto};
use super::host::SyncHost;
use super::recovery::{self, RecoveryAction, RecoveryChoice, RecoveryEntryView};
use super::recovery_kit::{self, KIT_LOCK, RecoveryKitState};
#[cfg(desktop)]
use super::service::{AuthorityAction, AuthorityAnswer, ForceRequest, LateDecision};
use super::service::{SyncRuntime, blocking};
use super::status::{SyncNotice, SyncStatusView, SyncSubscribers};
#[cfg(desktop)]
use ganbaru_sync::ContestEntry;
#[cfg(desktop)]
use ganbaru_sync_contracts::bounds::{MAX_DEVICE_ID_BYTES, MAX_WRITERS_PER_SPACE};
#[cfg(desktop)]
use ganbaru_sync_contracts::ids::parse_hex;
#[cfg(desktop)]
use ganbaru_sync_contracts::{Digest32, LateResolution};
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
/// Most devices a recovery kit's device review may mark. Every enrolled device certifies at
/// least one writer, so a space never enrolls more devices than it holds writers.
#[cfg(desktop)]
const MAX_REVIEW_REMOVALS: usize = MAX_WRITERS_PER_SPACE;

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

/// Reports whether the phone app is in the foreground. Going to the background stops the
/// member listener before the command returns, so the phone listens only while it is visible;
/// coming to the foreground runs a pass, which starts the listener when this phone is a linked
/// member.
#[cfg(any(mobile, test))]
#[tauri::command]
pub async fn sync_set_foreground<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, SyncRuntime>,
    visible: bool,
) -> Result<(), String> {
    runtime.set_foreground(visible);
    if visible {
        runtime.sync_now();
    } else {
        app.stop_serving_members().await;
    }
    Ok(())
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

/// Replaces the vault's recovery kit on an admin desktop. The new kit waits for the user to
/// save it, as [`sync_recovery_kit_state`] shows; the old kit stops signing once the anchor
/// sequences the change and opens no data encrypted after it.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_regenerate_recovery_kit(runtime: State<'_, SyncRuntime>) -> Result<(), String> {
    let _kit = KIT_LOCK.lock().await;
    match run_authority(&runtime, AuthorityAction::RegenerateRecoveryKit).await? {
        AuthorityAnswer::RecoveryKit => Ok(()),
        _ => Err("sync answered the recovery kit request with something else".to_string()),
    }
}

/// The recovery kit of the active vault: whether the vault keeps a kit, and the kit whose seed
/// this device still holds because the user has not confirmed saving it.
#[tauri::command]
pub async fn sync_recovery_kit_state<R: Runtime>(
    app: AppHandle<R>,
) -> Result<RecoveryKitState, String> {
    let _kit = KIT_LOCK.lock().await;
    let Some(pool) = crate::db::connect_active_vault_for_sync(&app).await? else {
        return Ok(RecoveryKitState {
            stored: false,
            pending: None,
        });
    };
    let mut conn = acquire_kit_connection(&pool).await?;
    recovery_kit::state(&mut conn, &app.key_store()?, &app.active_vault_id()?).await
}

/// Creates or replaces the recovery kit of an active vault that does not sync yet; its public
/// keys found the charter when the vault first links.
#[tauri::command]
pub async fn sync_create_recovery_kit<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let _kit = KIT_LOCK.lock().await;
    let pool = writable_kit_vault(&app).await?;
    let mut conn = acquire_kit_connection(&pool).await?;
    recovery_kit::create(
        &mut conn,
        &app.key_store()?,
        &app.active_vault_id()?,
        super::now_ms()?,
    )
    .await
}

/// Confirms that the user saved the waiting recovery kit by typing back the last group of its
/// code, then deletes its seed, so no device keeps it. Returns false when the group does not
/// match.
#[tauri::command]
pub async fn sync_recovery_kit_saved<R: Runtime>(
    app: AppHandle<R>,
    last_group: String,
) -> Result<bool, String> {
    let _kit = KIT_LOCK.lock().await;
    let pool = writable_kit_vault(&app).await?;
    let mut conn = acquire_kit_connection(&pool).await?;
    recovery_kit::confirm_saved(
        &mut conn,
        &app.key_store()?,
        &app.active_vault_id()?,
        &last_group,
    )
    .await
}

/// The active vault opened for the recovery kit, when this device may write it.
async fn writable_kit_vault<R: Runtime>(app: &AppHandle<R>) -> Result<sqlx::SqlitePool, String> {
    crate::db::connect_active_vault_for_sync(app)
        .await?
        .ok_or_else(|| "this device cannot write the active vault".to_string())
}

async fn acquire_kit_connection(
    pool: &sqlx::SqlitePool,
) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, String> {
    pool.acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))
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

/// An authorization entry the frontend names, by hexadecimal hash and revision.
#[cfg(desktop)]
#[derive(Debug, Deserialize)]
pub struct EntryTarget {
    hash: String,
    revision: u64,
}

/// Forces the anchor to this linked desktop after the anchor is lost: an admin desktop signs
/// the handover with its admin key, any other enrolled desktop with the recovery admin key
/// `kit_code` derives, which then also promotes it to admin. A kit handover continues from
/// `point`, a kit point the contest view offers, or by default from the point the engine
/// chooses, and then removes the old anchor and every device in `remove_devices`. With
/// `quarantine_anchor`, it then quarantines the lost anchor and proposes its removal, which it
/// sequences itself. Returns the authorization revision of the last entry it sealed.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_force_anchor<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, SyncRuntime>,
    kit_code: Option<String>,
    point: Option<EntryTarget>,
    remove_devices: Vec<String>,
    quarantine_anchor: bool,
) -> Result<u64, String> {
    let request = force_request(kit_code, point, remove_devices)?;
    let (revision, old_anchor) =
        match run_authority(&runtime, AuthorityAction::ForceAnchor(request)).await? {
            AuthorityAnswer::ForcedAnchor {
                revision,
                old_anchor,
            } => (revision, old_anchor),
            _ => return Err("sync answered forcing the anchor with something else".to_string()),
        };
    if quarantine_anchor && old_anchor != app.device_id()? {
        let path = quarantine_path(&app)?;
        {
            let _guard = QUARANTINE_LOCK.lock().await;
            let target = old_anchor.clone();
            blocking(move || Quarantine::load(&path)?.add(&target)).await?;
        }
        runtime.request_authority_detached(AuthorityAction::ProposeRemoval(old_anchor))?;
    }
    runtime.sync_now();
    Ok(revision)
}

/// The anchor handover contests of the active vault's sync space, with the actions this
/// device may take in each, the removals the recovery kit's device review lists, and the record
/// storage backstop and growth alerts. `None` before the vault joined a sync space.
#[tauri::command]
pub async fn sync_contests<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, SyncRuntime>,
) -> Result<Option<ContestsDto>, String> {
    contests::read(&app, runtime.records()).await
}

/// As a contestant, or as the anchor withdrawing its objection, yields to the candidate
/// handover `handover` names by hexadecimal hash. Returns the authorization revision of the
/// yield.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_contest_yield(
    runtime: State<'_, SyncRuntime>,
    handover: String,
) -> Result<u64, String> {
    let handover = parse_digest(&handover, "handover")?;
    contest_step(&runtime, AuthorityAction::Yield(handover)).await
}

/// As the anchor of the contested instance, blocks the candidate handover `handover` names by
/// removing its target, which keeps the vault frozen until the recovery kit is used. Returns
/// the authorization revision of the objection.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_contest_object(
    runtime: State<'_, SyncRuntime>,
    handover: String,
) -> Result<u64, String> {
    let handover = parse_digest(&handover, "handover")?;
    contest_step(&runtime, AuthorityAction::Object(handover)).await
}

/// Resumes full record exchange after the storage backstop stopped it, once the person freed
/// space or chose to continue.
#[tauri::command]
pub fn sync_records_resume(runtime: State<'_, SyncRuntime>) {
    runtime.records().resume();
    runtime.sync_now();
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

/// The first rows removed devices changed after their removal cutoff that no admin decided yet,
/// with their total.
#[tauri::command]
pub async fn sync_late_changes<R: Runtime>(app: AppHandle<R>) -> Result<LateChangesView, String> {
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
async fn contest_step(runtime: &SyncRuntime, action: AuthorityAction) -> Result<u64, String> {
    let revision = match run_authority(runtime, action).await? {
        AuthorityAnswer::Contested(revision) => revision,
        _ => return Err("sync answered a contest action with something else".to_string()),
    };
    runtime.sync_now();
    Ok(revision)
}

/// The forced handover the frontend asks for, with a bounded device review.
#[cfg(desktop)]
fn force_request(
    kit: Option<String>,
    point: Option<EntryTarget>,
    remove_devices: Vec<String>,
) -> Result<ForceRequest, String> {
    if kit.is_none() && (point.is_some() || !remove_devices.is_empty()) {
        return Err("only a recovery kit chooses a kit point or removes devices".to_string());
    }
    if remove_devices.len() > MAX_REVIEW_REMOVALS {
        return Err("the device review marks too many devices".to_string());
    }
    if remove_devices
        .iter()
        .any(|device_id| device_id.is_empty() || device_id.len() > MAX_DEVICE_ID_BYTES)
    {
        return Err("the device review names an invalid device".to_string());
    }
    let point = point
        .map(|point| {
            Ok::<_, String>(ContestEntry {
                hash: parse_digest(&point.hash, "kit point")?,
                revision: point.revision,
            })
        })
        .transpose()?;
    Ok(ForceRequest {
        kit,
        point,
        removals: remove_devices,
    })
}

/// A 32-byte hash in lowercase hexadecimal.
#[cfg(desktop)]
fn parse_digest(text: &str, what: &str) -> Result<Digest32, String> {
    parse_hex::<32>(text).ok_or_else(|| format!("the {what} hash is not valid"))
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
