//! Whole-vault receive, activation, acknowledgement, and refresh lifecycle.

use super::pairing::{PairingManager, PendingAcknowledgement};
use super::protocol::BundlePurpose;
use super::transport::TransferCancellation;
use crate::vault::backup::ReplacedVault;
use crate::vault::ownership::{TransferPhase, VaultOwnershipManager};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager;

const CONNECTED_REPOLL_DELAY: Duration = Duration::from_secs(1);
const DISCONNECTED_RETRY_DELAY: Duration = Duration::from_secs(30);

/// Error returned while a vault other than the linked one is active. The webview maps it to a
/// localized status, so the text is stable.
pub(crate) const LINKED_VAULT_INACTIVE: &str = "the linked vault is not active on this device";

/// Refuses handoff work for the linked vault while another vault is active, so switching vaults
/// suspends handoff instead of retargeting it. Before the linked vault has been activated here,
/// the active vault is the one the first link replaces, and activation keeps it.
pub(crate) fn ensure_linked_vault_active(
    replica_ready: bool,
    active_vault_id: &str,
    linked_vault_id: &str,
) -> Result<(), String> {
    if replica_ready && active_vault_id != linked_vault_id {
        return Err(LINKED_VAULT_INACTIVE.to_string());
    }
    Ok(())
}

fn ensure_linked_vault_active_for(
    app: &tauri::AppHandle,
    pairing: &PairingManager,
    linked_vault_id: &str,
) -> Result<(), String> {
    ensure_linked_vault_active(
        pairing.replica_ready()?,
        &crate::vault::active_vault_id(app)?,
        linked_vault_id,
    )
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ReceiveMode {
    Ownership,
    Refresh,
    Bootstrap,
}

impl ReceiveMode {
    fn purpose(self) -> BundlePurpose {
        match self {
            Self::Ownership => BundlePurpose::Ownership,
            Self::Refresh | Self::Bootstrap => BundlePurpose::Refresh,
        }
    }
}

/// Error returned when a refresh or an ownership transfer is asked of a device the coordinator
/// does not own the vault for.
const NOT_COORDINATOR_OWNED: &str = "desktop is not the current owner of this vault";

/// Error returned when a refresh or an ownership transfer is asked before the first link copied
/// the vault to this device.
const FIRST_LINK_NEEDS_BOOTSTRAP: &str =
    "the linked vault must be copied to this device before it can refresh or take ownership";

/// Error returned when the owner of the vault asks for a fresh copy of it.
const OWNER_CANNOT_BOOTSTRAP: &str = "this device owns the linked vault, so it holds it already";

/// Decides whether a receive may start. A bootstrap copies the vault from the coordinator for
/// a first link or as a fresh copy, whichever device owns it, and is refused only on the owner.
/// A refresh or an ownership transfer needs a device that already holds the vault and a
/// coordinator that owns it.
fn admit_receive(
    mode: ReceiveMode,
    replica_ready: bool,
    can_write: bool,
    coordinator_owns: bool,
) -> Result<(), &'static str> {
    match mode {
        ReceiveMode::Bootstrap if can_write => Err(OWNER_CANNOT_BOOTSTRAP),
        ReceiveMode::Bootstrap => Ok(()),
        ReceiveMode::Ownership | ReceiveMode::Refresh if can_write || !coordinator_owns => {
            Err(NOT_COORDINATOR_OWNED)
        }
        ReceiveMode::Ownership | ReceiveMode::Refresh if !replica_ready => {
            Err(FIRST_LINK_NEEDS_BOOTSTRAP)
        }
        ReceiveMode::Ownership | ReceiveMode::Refresh => Ok(()),
    }
}

/// Where a received vault is activated.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Destination {
    /// Replaces the vault in use, carrying this device's changes forward when it holds the same
    /// vault.
    ActiveVault,
    /// A first link on a computer: a new folder, so the vault in use stays where it is.
    #[cfg_attr(not(desktop), allow(dead_code))]
    NewFolder,
}

/// A first link on a computer that holds another vault activates the linked vault in a new
/// folder. Everything else, including every Android receive, replaces the vault in use.
fn destination(
    desktop: bool,
    mode: ReceiveMode,
    replica_ready: bool,
    active_holds_linked_vault: bool,
) -> Destination {
    if desktop
        && matches!(mode, ReceiveMode::Bootstrap)
        && !replica_ready
        && !active_holds_linked_vault
    {
        Destination::NewFolder
    } else {
        Destination::ActiveVault
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReceiveOutcome {
    transfer_id: Option<String>,
    generation: Option<u64>,
    activated: bool,
    in_progress: bool,
}

#[derive(Default)]
pub(crate) struct ReceiverLifecycle {
    operation: Arc<tokio::sync::Mutex<()>>,
    cancellation: Mutex<Option<TransferCancellation>>,
    connected: AtomicBool,
    /// Folder the user chose for the first link on a computer. The webview never names it; it
    /// only asks for the native folder picker.
    #[cfg_attr(not(desktop), allow(dead_code))]
    link_folder: Mutex<Option<std::path::PathBuf>>,
}

impl ReceiverLifecycle {
    #[cfg_attr(not(desktop), allow(dead_code))]
    fn link_folder(&self) -> Result<Option<std::path::PathBuf>, String> {
        self.link_folder
            .lock()
            .map(|folder| folder.clone())
            .map_err(|_| "linked vault folder choice is unavailable".to_string())
    }

    #[cfg_attr(not(desktop), allow(dead_code))]
    fn set_link_folder(&self, folder: Option<std::path::PathBuf>) -> Result<(), String> {
        *self
            .link_folder
            .lock()
            .map_err(|_| "linked vault folder choice is unavailable".to_string())? = folder;
        Ok(())
    }
}

/// Opens the folder picker for the first link on a computer and keeps the chosen folder for
/// the next bootstrap. Returns the chosen folder, or `None` when the picker was closed, which
/// keeps the earlier choice. A folder that holds files is refused.
#[cfg(desktop)]
#[tauri::command]
pub(crate) async fn handoff_pick_link_folder(
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    let start = crate::vault::default_vault_path(&app)
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .filter(|path| path.is_dir());
    let Some(folder) = crate::vault::pick_folder(&app, "Choose a folder", start).await? else {
        return Ok(None);
    };
    crate::vault::backup::resolve_link_folder(Some(&folder), &folder, "")?;
    let shown = crate::vault::path_to_string(&folder, "linked vault folder")?;
    app.state::<ReceiverLifecycle>()
        .set_link_folder(Some(folder))?;
    Ok(Some(shown))
}

/// Forgets the chosen folder, so the next first link uses the default location.
#[cfg(desktop)]
#[tauri::command]
pub(crate) fn handoff_reset_link_folder(app: tauri::AppHandle) -> Result<(), String> {
    app.state::<ReceiverLifecycle>().set_link_folder(None)
}

impl ReceiverLifecycle {
    fn begin(&self) -> Result<TransferCancellation, String> {
        let cancellation = TransferCancellation::default();
        *self
            .cancellation
            .lock()
            .map_err(|_| "receiver cancellation lock is unavailable".to_string())? =
            Some(cancellation.clone());
        Ok(cancellation)
    }

    fn finish(&self) {
        if let Ok(mut current) = self.cancellation.lock() {
            current.take();
        }
    }

    fn cancel(&self) -> Result<(), String> {
        let current = self
            .cancellation
            .lock()
            .map_err(|_| "receiver cancellation lock is unavailable".to_string())?;
        if let Some(cancellation) = current.as_ref() {
            cancellation.cancel();
        }
        Ok(())
    }
}

#[tauri::command]
pub(crate) async fn handoff_receive_desktop_bundle(
    app: tauri::AppHandle,
    mode: ReceiveMode,
) -> Result<ReceiveOutcome, String> {
    receive_if_idle(app, mode).await
}

#[tauri::command]
pub(crate) fn handoff_cancel_receive(app: tauri::AppHandle) -> Result<(), String> {
    app.state::<ReceiverLifecycle>().cancel()
}

async fn receive_if_idle(
    app: tauri::AppHandle,
    mode: ReceiveMode,
) -> Result<ReceiveOutcome, String> {
    let lifecycle = app.state::<ReceiverLifecycle>();
    let Ok(_operation) = lifecycle.operation.try_lock() else {
        return Ok(ReceiveOutcome {
            transfer_id: None,
            generation: None,
            activated: false,
            in_progress: true,
        });
    };
    let cancellation = lifecycle.begin()?;
    let result = receive(&app, mode, &cancellation).await;
    lifecycle.connected.store(result.is_ok(), Ordering::Release);
    lifecycle.finish();
    result
}

async fn receive(
    app: &tauri::AppHandle,
    mode: ReceiveMode,
    cancellation: &TransferCancellation,
) -> Result<ReceiveOutcome, String> {
    flush_pending_acknowledgement(app).await?;
    let pairing = app.state::<PairingManager>().inner().clone();
    let coordinator = pairing
        .coordinator_pin()?
        .ok_or_else(|| "this device is not linked to a coordinator".to_string())?;
    ensure_linked_vault_active_for(app, &pairing, &coordinator.vault_id)?;
    let ownership = app.state::<VaultOwnershipManager>();
    let status = ownership.status(&coordinator.vault_id)?;
    if let TransferPhase::IncomingCommitted {
        transfer_id,
        source_device_id,
        committed_generation,
    } = status.transfer_phase
    {
        if source_device_id != coordinator.device_id {
            return Err("pending ownership transfer has an unexpected source".to_string());
        }
        return resume_committed_ownership(
            app,
            &pairing,
            &coordinator.vault_id,
            transfer_id,
            committed_generation,
        )
        .await;
    }
    let replica_ready = pairing.replica_ready()?;
    let coordinator_owns = status.owner_device_id == coordinator.device_id;
    admit_receive(mode, replica_ready, status.can_write, coordinator_owns)
        .map_err(str::to_string)?;
    let destination = destination(
        cfg!(desktop),
        mode,
        replica_ready,
        crate::vault::active_vault_id(app)? == coordinator.vault_id,
    );
    #[cfg(not(desktop))]
    debug_assert_eq!(destination, Destination::ActiveVault);
    let previous = replaced_vault_policy(app)?;
    #[cfg(desktop)]
    let link_folder = match destination {
        Destination::NewFolder => Some(prepare_link_folder(app)?),
        Destination::ActiveVault => None,
    };

    let purpose = mode.purpose();
    let metadata = super::transport::request_bundle(
        &pairing,
        super::current_compatibility(app),
        status.generation,
        purpose,
        cancellation,
    )
    .await?;
    if let Err(error) = super::protocol::ensure_compatible(
        &super::current_compatibility(app),
        &metadata.compatibility,
    ) {
        let _ = super::transport::cancel_prepared_transfer(&pairing, &metadata.transfer_id).await;
        return Err(error);
    }
    let archive =
        super::transport::download_bundle(&pairing, metadata.clone(), cancellation).await?;
    #[cfg(desktop)]
    let staging = match &link_folder {
        Some(folder) => {
            crate::vault::backup::handoff_staging_path_in(&folder.parent, &metadata.transfer_id)
        }
        None => handoff_staging_path(app, &metadata.transfer_id)?,
    };
    #[cfg(not(desktop))]
    let staging = handoff_staging_path(app, &metadata.transfer_id)?;
    crate::vault::backup::stage_handoff_archive(&archive, &staging, &metadata.vault_id).await?;
    #[cfg(desktop)]
    let new_folder = match &link_folder {
        Some(folder) => Some(resolve_staged_link_folder(folder, &staging)?),
        None => None,
    };

    if purpose == BundlePurpose::Ownership {
        let (_, generation) =
            super::transport::commit_staged_ownership(&pairing, metadata.clone()).await?;
        ownership.accept_incoming_coordinator_grant(
            &metadata.vault_id,
            metadata.transfer_id.clone(),
            coordinator.device_id,
            generation,
        )?;
    } else if coordinator_owns && metadata.generation >= status.generation {
        // A bootstrap served while another device owns the vault leaves the ownership record
        // as it is; the coordinator's copy carries no newer ownership than the record.
        ownership.advance_remote_coordinator(
            &metadata.vault_id,
            &coordinator.device_id,
            metadata.generation,
        )?;
    }

    #[cfg(desktop)]
    if let Some(target) = new_folder {
        crate::vault::backup::activate_handoff_in_new_folder(
            app,
            &staging,
            target,
            &metadata.transfer_id,
            &metadata.vault_id,
        )
        .await?;
        app.state::<ReceiverLifecycle>().set_link_folder(None)?;
    } else {
        activate_handoff(
            app,
            &staging,
            &metadata.transfer_id,
            &metadata.vault_id,
            previous,
        )
        .await?;
    }
    #[cfg(not(desktop))]
    activate_handoff(
        app,
        &staging,
        &metadata.transfer_id,
        &metadata.vault_id,
        previous,
    )
    .await?;
    let device_id = pairing.device_id()?;
    let pending = PendingAcknowledgement {
        vault_id: metadata.vault_id.clone(),
        device_id,
        transfer_id: metadata.transfer_id.clone(),
        generation: metadata.generation,
        purpose,
    };
    pairing.record_activation(pending.clone())?;
    if purpose == BundlePurpose::Ownership {
        ownership.finalize_incoming(
            &metadata.vault_id,
            &metadata.transfer_id,
            metadata.generation,
        )?;
    }
    let acknowledgement = super::transport::acknowledge_activation(&pairing, &pending).await;
    if acknowledgement.is_ok() {
        pairing.clear_pending_acknowledgement(&pending.transfer_id)?;
        if let Err(error) = pairing.remove_staging(&pending.transfer_id) {
            eprintln!("failed to clean completed vault transfer: {error}");
        }
    }
    if purpose == BundlePurpose::Ownership {
        reload_application_shell_after_ownership_change(app)?;
    } else {
        reload_application_shell(app)?;
    }
    acknowledgement?;
    Ok(ReceiveOutcome {
        transfer_id: Some(metadata.transfer_id),
        generation: Some(metadata.generation),
        activated: true,
        in_progress: false,
    })
}

async fn resume_committed_ownership(
    app: &tauri::AppHandle,
    pairing: &PairingManager,
    vault_id: &str,
    transfer_id: String,
    generation: u64,
) -> Result<ReceiveOutcome, String> {
    let staging = handoff_staging_path(app, &transfer_id)?;
    activate_handoff(
        app,
        &staging,
        &transfer_id,
        vault_id,
        replaced_vault_policy(app)?,
    )
    .await?;
    let device_id = pairing.device_id()?;
    let pending = PendingAcknowledgement {
        vault_id: vault_id.to_string(),
        device_id,
        transfer_id: transfer_id.clone(),
        generation,
        purpose: BundlePurpose::Ownership,
    };
    pairing.record_activation(pending.clone())?;
    app.state::<VaultOwnershipManager>()
        .finalize_incoming(vault_id, &transfer_id, generation)?;
    let acknowledgement = super::transport::acknowledge_activation(pairing, &pending).await;
    if acknowledgement.is_ok() {
        pairing.clear_pending_acknowledgement(&transfer_id)?;
        if let Err(error) = pairing.remove_staging(&transfer_id) {
            eprintln!("failed to clean completed vault transfer: {error}");
        }
    }
    reload_application_shell_after_ownership_change(app)?;
    acknowledgement?;
    Ok(ReceiveOutcome {
        transfer_id: Some(transfer_id),
        generation: Some(generation),
        activated: true,
        in_progress: false,
    })
}

pub(super) fn reload_application_shell(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main application window is unavailable".to_string())?;
    window
        .eval("window.location.reload()")
        .map_err(|error| format!("reload application after vault activation: {error}"))
}

pub(super) fn reload_application_shell_after_ownership_change(
    app: &tauri::AppHandle,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main application window is unavailable".to_string())?;
    window
        .eval(
            "if (window.dispatchEvent(new Event('ganbaru:mobile-ownership-reload-requested', { cancelable: true }))) window.location.reload()",
        )
        .map_err(|error| format!("reload application after ownership change: {error}"))
}

async fn flush_pending_acknowledgement(app: &tauri::AppHandle) -> Result<(), String> {
    let pairing = app.state::<PairingManager>().inner().clone();
    let Some(pending) = pairing.pending_acknowledgement()? else {
        return Ok(());
    };
    if pending.purpose == BundlePurpose::Ownership {
        let ownership = app.state::<VaultOwnershipManager>();
        let status = ownership.status(&pending.vault_id)?;
        match status.transfer_phase {
            TransferPhase::IncomingCommitted { .. } => ownership.finalize_incoming(
                &pending.vault_id,
                &pending.transfer_id,
                pending.generation,
            )?,
            TransferPhase::Stable
                if status.can_write && status.generation == pending.generation => {}
            _ => {
                return Err(
                    "pending activation acknowledgement conflicts with ownership state".to_string(),
                );
            }
        }
    }
    super::transport::acknowledge_activation(&pairing, &pending).await?;
    pairing.clear_pending_acknowledgement(&pending.transfer_id)?;
    if let Err(error) = pairing.remove_staging(&pending.transfer_id) {
        eprintln!("failed to clean acknowledged vault transfer: {error}");
    }
    Ok(())
}

pub(crate) fn trigger_automatic_refresh(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let pairing = app.state::<PairingManager>().inner().clone();
        if !pairing.replica_ready().unwrap_or(false) {
            return;
        }
        let should_refresh = pairing
            .coordinator_pin()
            .ok()
            .flatten()
            .and_then(|coordinator| {
                app.state::<VaultOwnershipManager>()
                    .status(&coordinator.vault_id)
                    .ok()
                    .map(|status| {
                        !status.can_write && status.owner_device_id == coordinator.device_id
                    })
            })
            .unwrap_or(false);
        if !should_refresh {
            return;
        }
        if let Err(error) = receive_if_idle(app, ReceiveMode::Refresh).await {
            eprintln!("automatic vault refresh deferred: {error}");
        }
    });
}

async fn refresh_after_reconnect(app: &tauri::AppHandle) -> bool {
    let lifecycle = app.state::<ReceiverLifecycle>();
    let pairing = app.state::<PairingManager>().inner().clone();
    if let Some(transfer) = pairing.outgoing_transfer().ok().flatten() {
        super::source::trigger_requested_upload(app.clone(), transfer.purpose);
        return true;
    }
    if !pairing.replica_ready().unwrap_or(false) {
        return false;
    }
    let Some(coordinator) = pairing.coordinator_pin().ok().flatten() else {
        return false;
    };
    let linked_vault_active =
        ensure_linked_vault_active_for(app, &pairing, &coordinator.vault_id).is_ok();
    let Some(status) = app
        .state::<VaultOwnershipManager>()
        .status(&coordinator.vault_id)
        .ok()
    else {
        return false;
    };
    let poll = super::transport::probe_coordinator(
        &pairing,
        super::current_compatibility(app),
        status.generation,
    )
    .await;
    let reachable = poll.is_ok();
    if reachable {
        crate::contacts::identity::try_release(app).await;
    }
    // While another vault is active nothing is exchanged, and the stored state stays
    // disconnected so switching back counts as a reconnect and refreshes.
    let was_connected = lifecycle
        .connected
        .swap(reachable && linked_vault_active, Ordering::AcqRel);
    if !linked_vault_active {
        return reachable;
    }
    if let Ok(Some(purpose)) = poll {
        if status.can_write {
            super::source::trigger_requested_upload(app.clone(), purpose);
        }
    } else if reachable
        && !was_connected
        && !status.can_write
        && status.owner_device_id == coordinator.device_id
    {
        trigger_automatic_refresh(app.clone());
    }
    reachable
}

#[cfg(target_os = "android")]
pub(crate) fn trigger_coordinator_reconciliation(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let _ = refresh_after_reconnect(&app).await;
    });
}

pub(crate) fn start_reconnect_refresh(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            let reachable = refresh_after_reconnect(&app).await;
            tokio::time::sleep(if reachable {
                CONNECTED_REPOLL_DELAY
            } else {
                DISCONNECTED_RETRY_DELAY
            })
            .await;
        }
    });
}

#[cfg(target_os = "android")]
fn handoff_staging_path(
    app: &tauri::AppHandle,
    transfer_id: &str,
) -> Result<std::path::PathBuf, String> {
    crate::vault::backup::android_handoff_staging_path(app, transfer_id)
}

#[cfg(desktop)]
fn handoff_staging_path(
    app: &tauri::AppHandle,
    transfer_id: &str,
) -> Result<std::path::PathBuf, String> {
    crate::vault::backup::active_handoff_staging_path(app, transfer_id)
}

#[cfg(target_os = "ios")]
fn handoff_staging_path(
    _app: &tauri::AppHandle,
    _transfer_id: &str,
) -> Result<std::path::PathBuf, String> {
    Err("vault handoff is not available on iOS".to_string())
}

#[cfg(target_os = "android")]
async fn activate_handoff(
    app: &tauri::AppHandle,
    staging: &std::path::Path,
    transfer_id: &str,
    vault_id: &str,
    previous: ReplacedVault,
) -> Result<crate::vault::VaultInfo, String> {
    crate::vault::backup::activate_android_handoff(app, staging, transfer_id, vault_id, previous)
        .await
}

#[cfg(desktop)]
async fn activate_handoff(
    app: &tauri::AppHandle,
    staging: &std::path::Path,
    transfer_id: &str,
    vault_id: &str,
    previous: ReplacedVault,
) -> Result<crate::vault::VaultInfo, String> {
    crate::vault::backup::activate_active_handoff(app, staging, transfer_id, vault_id, previous)
        .await
}

#[cfg(target_os = "ios")]
async fn activate_handoff(
    _app: &tauri::AppHandle,
    _staging: &std::path::Path,
    _transfer_id: &str,
    _vault_id: &str,
    _previous: ReplacedVault,
) -> Result<crate::vault::VaultInfo, String> {
    Err("vault handoff is not available on iOS".to_string())
}

/// Android keeps one vault, so a receive that replaces another vault writes a portable backup
/// of it first, except over the untouched starter, which holds nothing of the user's.
#[cfg(target_os = "android")]
fn replaced_vault_policy(app: &tauri::AppHandle) -> Result<ReplacedVault, String> {
    if crate::vault::untouched_starter_vault(app)? {
        Ok(ReplacedVault::DiscardStarter)
    } else {
        crate::vault::replaced_vaults::replaced_vaults_root(app).map(ReplacedVault::BackUpOther)
    }
}

/// A computer keeps a replaced vault folder whole beside the linked vault.
#[cfg(not(target_os = "android"))]
fn replaced_vault_policy(_app: &tauri::AppHandle) -> Result<ReplacedVault, String> {
    Ok(ReplacedVault::KeepOther)
}

/// Destination of a first link on a computer, decided before the download.
#[cfg(desktop)]
struct LinkFolder {
    /// Folder the user chose, if any.
    chosen: Option<std::path::PathBuf>,
    /// Default vault folder of this platform.
    default: std::path::PathBuf,
    /// Folder that holds the staged vault and the folder it moves into.
    parent: std::path::PathBuf,
}

/// Reads the folder choice for a first link and refuses a chosen folder that holds files
/// before anything is downloaded.
#[cfg(desktop)]
fn prepare_link_folder(app: &tauri::AppHandle) -> Result<LinkFolder, String> {
    let chosen = app.state::<ReceiverLifecycle>().link_folder()?;
    let default = crate::vault::default_vault_path(app)?;
    if let Some(chosen) = &chosen {
        crate::vault::backup::resolve_link_folder(Some(chosen), &default, "")?;
    }
    let parent = chosen
        .as_deref()
        .unwrap_or(&default)
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?
        .to_path_buf();
    std::fs::create_dir_all(&parent)
        .map_err(|error| format!("create the linked vault parent folder: {error}"))?;
    Ok(LinkFolder {
        chosen,
        default,
        parent,
    })
}

/// Picks the folder the staged vault moves into, named after the vault it holds. The staged
/// copy is removed when no folder can take it.
#[cfg(desktop)]
fn resolve_staged_link_folder(
    folder: &LinkFolder,
    staging: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let resolved = crate::vault::vault_info_from_path(staging).and_then(|info| {
        crate::vault::backup::resolve_link_folder(
            folder.chosen.as_deref(),
            &folder.default,
            &info.display_name,
        )
    });
    if resolved.is_err()
        && let Err(error) = std::fs::remove_dir_all(staging)
    {
        eprintln!("failed to clean the staged linked vault: {error}");
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_modes_map_to_the_single_transport_purpose() {
        assert_eq!(ReceiveMode::Ownership.purpose(), BundlePurpose::Ownership);
        assert_eq!(ReceiveMode::Refresh.purpose(), BundlePurpose::Refresh);
        assert_eq!(ReceiveMode::Bootstrap.purpose(), BundlePurpose::Refresh);
    }

    #[test]
    fn bootstrap_is_admitted_for_every_first_link_and_fresh_copy_except_on_the_owner() {
        for replica_ready in [false, true] {
            for coordinator_owns in [false, true] {
                assert_eq!(
                    admit_receive(
                        ReceiveMode::Bootstrap,
                        replica_ready,
                        false,
                        coordinator_owns
                    ),
                    Ok(())
                );
                assert_eq!(
                    admit_receive(
                        ReceiveMode::Bootstrap,
                        replica_ready,
                        true,
                        coordinator_owns
                    ),
                    Err(OWNER_CANNOT_BOOTSTRAP)
                );
            }
        }
    }

    #[test]
    fn refresh_and_ownership_need_a_linked_copy_and_a_coordinator_that_owns_the_vault() {
        for mode in [ReceiveMode::Refresh, ReceiveMode::Ownership] {
            assert_eq!(admit_receive(mode, true, false, true), Ok(()));
            assert_eq!(
                admit_receive(mode, false, false, true),
                Err(FIRST_LINK_NEEDS_BOOTSTRAP)
            );
            assert_eq!(
                admit_receive(mode, true, false, false),
                Err(NOT_COORDINATOR_OWNED)
            );
            assert_eq!(
                admit_receive(mode, true, true, true),
                Err(NOT_COORDINATOR_OWNED)
            );
        }
    }

    #[test]
    fn only_a_first_link_on_a_computer_over_another_vault_uses_a_new_folder() {
        assert_eq!(
            destination(true, ReceiveMode::Bootstrap, false, false),
            Destination::NewFolder
        );
        // A fresh copy, a computer that already holds the linked vault, and Android all
        // replace the vault in use.
        assert_eq!(
            destination(true, ReceiveMode::Bootstrap, true, true),
            Destination::ActiveVault
        );
        assert_eq!(
            destination(true, ReceiveMode::Bootstrap, false, true),
            Destination::ActiveVault
        );
        assert_eq!(
            destination(false, ReceiveMode::Bootstrap, false, false),
            Destination::ActiveVault
        );
        assert_eq!(
            destination(true, ReceiveMode::Refresh, true, false),
            Destination::ActiveVault
        );
    }

    #[test]
    fn handoff_is_refused_while_another_vault_is_active() {
        assert_eq!(
            ensure_linked_vault_active(true, "vault-other", "vault-linked"),
            Err(LINKED_VAULT_INACTIVE.to_string())
        );
        assert_eq!(
            ensure_linked_vault_active(true, "vault-linked", "vault-linked"),
            Ok(())
        );
        // The first link replaces the active vault and keeps it beside the linked one.
        assert_eq!(
            ensure_linked_vault_active(false, "vault-other", "vault-linked"),
            Ok(())
        );
    }
}
