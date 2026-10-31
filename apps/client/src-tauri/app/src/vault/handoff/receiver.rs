//! Snapshot bootstrap: receiving a vault copy, activating it, acknowledging it, and reporting
//! the link status of this device.

use super::pairing::{PairingManager, PendingAcknowledgement};
use super::transport::{BundleSource, FetchedBundle, TransferCancellation};
use crate::vault::backup::ReplacedVault;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager;

/// How often the link loop retries a pending acknowledgement and the person key release.
const LINK_RETRY_DELAY: Duration = Duration::from_secs(30);

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
fn destination(desktop: bool, replica_ready: bool, active_holds_linked_vault: bool) -> Destination {
    if desktop && !replica_ready && !active_holds_linked_vault {
        Destination::NewFolder
    } else {
        Destination::ActiveVault
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReceiveOutcome {
    transfer_id: Option<String>,
    activated: bool,
    in_progress: bool,
}

/// Snapshot bootstrap state of this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum BootstrapState {
    /// Nothing to copy: this device is not linked to a coordinator, or holds its copy already.
    None,
    /// Linked to a coordinator, but the linked vault has not been copied here yet.
    Required,
    /// A vault copy is being received.
    Running,
}

/// Link status of this device, derived from pairing and receiver state only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultLinkStatus {
    /// Whether this device is linked: pinned to a coordinator, or coordinating linked devices.
    linked: bool,
    /// The coordinator of the link: the pinned coordinator, or this desktop when it coordinates.
    coordinator_device_id: Option<String>,
    bootstrap: BootstrapState,
}

fn bootstrap_state(pinned: bool, replica_ready: bool, running: bool) -> BootstrapState {
    if running {
        BootstrapState::Running
    } else if pinned && !replica_ready {
        BootstrapState::Required
    } else {
        BootstrapState::None
    }
}

/// The coordinator of this device's current link: the pinned coordinator, or this device when
/// it coordinates linked peers. `None` when it is not linked now, whatever devices a sync log
/// once enrolled, and when pairing state is not loaded.
fn link_coordinator(pairing: &PairingManager) -> Result<Option<String>, String> {
    if !pairing.is_loaded()? {
        return Ok(None);
    }
    Ok(match pairing.coordinator_pin()? {
        Some(pin) => Some(pin.device_id),
        None if !pairing.linked_peers()?.is_empty() => Some(pairing.device_id()?),
        None => None,
    })
}

/// Whether this device is linked now, as [`vault_link_status`] reports it: it pins a
/// coordinator or coordinates linked peers.
pub(crate) fn is_linked(pairing: &PairingManager) -> Result<bool, String> {
    Ok(link_coordinator(pairing)?.is_some())
}

/// Reports whether this device is linked, which device coordinates the link, and whether a
/// snapshot bootstrap is required or running.
#[tauri::command]
pub(crate) fn vault_link_status(app: tauri::AppHandle) -> Result<VaultLinkStatus, String> {
    let pairing = app.state::<PairingManager>();
    let running = app.state::<ReceiverLifecycle>().is_running();
    if !pairing.is_loaded()? {
        return Ok(VaultLinkStatus {
            linked: false,
            coordinator_device_id: None,
            bootstrap: bootstrap_state(false, false, running),
        });
    }
    let coordinator_device_id = link_coordinator(&pairing)?;
    let pinned = pairing.coordinator_pin()?.is_some();
    Ok(VaultLinkStatus {
        linked: coordinator_device_id.is_some(),
        coordinator_device_id,
        bootstrap: bootstrap_state(pinned, pairing.replica_ready()?, running),
    })
}

#[derive(Default)]
pub(crate) struct ReceiverLifecycle {
    operation: Arc<tokio::sync::Mutex<()>>,
    cancellation: Mutex<Option<TransferCancellation>>,
    /// Folder the user chose for the first link on a computer. The webview never names it; it
    /// only asks for the native folder picker.
    #[cfg_attr(not(desktop), allow(dead_code))]
    link_folder: Mutex<Option<std::path::PathBuf>>,
}

impl ReceiverLifecycle {
    /// Whether a vault copy is being received.
    fn is_running(&self) -> bool {
        self.operation.try_lock().is_err()
    }

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
/// keeps the earlier choice. A folder that holds files, or that lies inside or holds another
/// vault, is refused.
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

/// Copies the linked vault to this device: the first copy after linking, or a fresh copy that
/// replaces the vault in use.
#[tauri::command]
pub(crate) async fn handoff_request_bootstrap(
    app: tauri::AppHandle,
) -> Result<ReceiveOutcome, String> {
    receive_if_idle(app).await
}

#[tauri::command]
pub(crate) fn handoff_cancel_receive(app: tauri::AppHandle) -> Result<(), String> {
    app.state::<ReceiverLifecycle>().cancel()
}

async fn receive_if_idle(app: tauri::AppHandle) -> Result<ReceiveOutcome, String> {
    let lifecycle = app.state::<ReceiverLifecycle>();
    let Ok(_operation) = lifecycle.operation.try_lock() else {
        return Ok(ReceiveOutcome {
            transfer_id: None,
            activated: false,
            in_progress: true,
        });
    };
    let cancellation = lifecycle.begin()?;
    let result = receive(&app, &cancellation).await;
    lifecycle.finish();
    result
}

async fn receive(
    app: &tauri::AppHandle,
    cancellation: &TransferCancellation,
) -> Result<ReceiveOutcome, String> {
    flush_pending_acknowledgement(app).await?;
    let pairing = app.state::<PairingManager>().inner().clone();
    let coordinator = pairing
        .coordinator_pin()?
        .ok_or_else(|| "this device is not linked to a coordinator".to_string())?;
    ensure_linked_vault_active_for(app, &pairing, &coordinator.vault_id)?;
    let replica_ready = pairing.replica_ready()?;
    let destination = destination(
        cfg!(desktop),
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

    let sources = bundle_sources(app, &coordinator.vault_id, &coordinator.device_id).await;
    let FetchedBundle {
        source,
        metadata,
        archive,
    } = super::transport::fetch_bundle(
        &pairing,
        &sources,
        super::current_compatibility(app),
        cancellation,
    )
    .await?;
    let from_coordinator = source == BundleSource::Coordinator;
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
    if metadata.derived_state_stale {
        crate::sync::rebuild_staged_vault(&staging).await?;
    }
    #[cfg(desktop)]
    let new_folder = match &link_folder {
        Some(folder) => Some(resolve_staged_link_folder(folder, &staging)?),
        None => None,
    };

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
    };
    pairing.record_activation(pending.clone())?;
    let acknowledgement = if from_coordinator {
        super::transport::acknowledge_activation(&pairing, &source, &pending).await
    } else {
        // A pending acknowledgement is only ever retried with the coordinator, so a member's
        // copy is settled first and acknowledged once; a member releases an unacknowledged
        // copy when its reservation expires.
        pairing.clear_pending_acknowledgement(&pending.transfer_id)?;
        if let Err(error) =
            super::transport::acknowledge_activation(&pairing, &source, &pending).await
        {
            eprintln!(
                "vault copy from {}: acknowledgement failed: {error}",
                source.describe()
            );
        }
        Ok(())
    };
    if acknowledgement.is_ok() {
        if from_coordinator {
            pairing.clear_pending_acknowledgement(&pending.transfer_id)?;
        }
        if let Err(error) = pairing.remove_staging(&pending.transfer_id) {
            eprintln!("failed to clean completed vault transfer: {error}");
        }
    }
    reload_application_shell(app)?;
    acknowledgement?;
    Ok(ReceiveOutcome {
        transfer_id: Some(metadata.transfer_id),
        activated: true,
        in_progress: false,
    })
}

/// Copies the vault a new desktop just recovered through its server into a new folder and opens
/// it. The recovery enrolled this desktop, so the server admits it without a coordinator pin. A
/// copy is settled once it opens, so an unanswered acknowledgement only leaves the server to
/// release its reservation when it expires.
#[cfg(desktop)]
pub(crate) async fn receive_recovered(
    app: tauri::AppHandle,
    target: ganbaru_sync_replica::anchor_link::ServerTarget,
    vault_id: String,
) -> Result<ReceiveOutcome, String> {
    let lifecycle = app.state::<ReceiverLifecycle>();
    let Ok(_operation) = lifecycle.operation.try_lock() else {
        return Ok(ReceiveOutcome {
            transfer_id: None,
            activated: false,
            in_progress: true,
        });
    };
    let cancellation = lifecycle.begin()?;
    let source = BundleSource::Recovery { target, vault_id };
    let result = receive_recovered_copy(&app, source, &cancellation).await;
    lifecycle.finish();
    result
}

#[cfg(desktop)]
async fn receive_recovered_copy(
    app: &tauri::AppHandle,
    source: BundleSource,
    cancellation: &TransferCancellation,
) -> Result<ReceiveOutcome, String> {
    let pairing = app.state::<PairingManager>().inner().clone();
    let folder = prepare_link_folder(app)?;
    let FetchedBundle {
        source,
        metadata,
        archive,
    } = super::transport::fetch_bundle(
        &pairing,
        &[source],
        super::current_compatibility(app),
        cancellation,
    )
    .await?;
    let staging =
        crate::vault::backup::handoff_staging_path_in(&folder.parent, &metadata.transfer_id);
    crate::vault::backup::stage_handoff_archive(&archive, &staging, &metadata.vault_id).await?;
    if metadata.derived_state_stale {
        crate::sync::rebuild_staged_vault(&staging).await?;
    }
    let target = resolve_staged_link_folder(&folder, &staging)?;
    crate::vault::backup::activate_handoff_in_new_folder(
        app,
        &staging,
        target,
        &metadata.transfer_id,
        &metadata.vault_id,
    )
    .await?;
    app.state::<ReceiverLifecycle>().set_link_folder(None)?;
    let pending = PendingAcknowledgement {
        vault_id: metadata.vault_id.clone(),
        device_id: pairing.device_id()?,
        transfer_id: metadata.transfer_id.clone(),
    };
    if let Err(error) = super::transport::acknowledge_activation(&pairing, &source, &pending).await
    {
        eprintln!(
            "vault copy from {}: acknowledgement failed: {error}",
            source.describe()
        );
    }
    if let Err(error) = pairing.remove_staging(&metadata.transfer_id) {
        eprintln!("failed to clean completed vault transfer: {error}");
    }
    reload_application_shell(app)?;
    Ok(ReceiveOutcome {
        transfer_id: Some(metadata.transfer_id),
        activated: true,
        in_progress: false,
    })
}

/// Where a vault copy is asked for, in order: the coordinator this device enrolled with, then
/// the member desktops the local selected log shows as live. A first copy comes from the
/// coordinator alone, since the log that names the other members arrives with it.
async fn bundle_sources(
    app: &tauri::AppHandle,
    vault_id: &str,
    coordinator_device_id: &str,
) -> Vec<BundleSource> {
    let mut sources = vec![BundleSource::Coordinator];
    match crate::sync::bootstrap_members(app, vault_id, coordinator_device_id).await {
        Ok(members) => sources.extend(members),
        Err(error) => eprintln!("vault copy: member sources are unavailable: {error}"),
    }
    sources
}

pub(super) fn reload_application_shell(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main application window is unavailable".to_string())?;
    window
        .eval("window.location.reload()")
        .map_err(|error| format!("reload application after vault activation: {error}"))
}

async fn flush_pending_acknowledgement(app: &tauri::AppHandle) -> Result<(), String> {
    let pairing = app.state::<PairingManager>().inner().clone();
    let Some(pending) = pairing.pending_acknowledgement()? else {
        return Ok(());
    };
    super::transport::acknowledge_activation(&pairing, &BundleSource::Coordinator, &pending)
        .await?;
    pairing.clear_pending_acknowledgement(&pending.transfer_id)?;
    if let Err(error) = pairing.remove_staging(&pending.transfer_id) {
        eprintln!("failed to clean acknowledged vault transfer: {error}");
    }
    Ok(())
}

/// One pass of the link loop: retries a pending activation acknowledgement and asks the
/// coordinator for the person key while the linked vault is active here.
async fn reconcile_link(app: &tauri::AppHandle) {
    let pairing = app.state::<PairingManager>().inner().clone();
    if !pairing.replica_ready().unwrap_or(false) {
        return;
    }
    let Some(coordinator) = pairing.coordinator_pin().ok().flatten() else {
        return;
    };
    if ensure_linked_vault_active_for(app, &pairing, &coordinator.vault_id).is_err() {
        return;
    }
    if app.state::<ReceiverLifecycle>().is_running() {
        return;
    }
    if let Err(error) = flush_pending_acknowledgement(app).await {
        eprintln!("vault copy acknowledgement deferred: {error}");
    }
    crate::contacts::identity::try_release(app).await;
}

#[cfg(target_os = "android")]
pub(crate) fn trigger_coordinator_reconciliation(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        reconcile_link(&app).await;
    });
}

/// Starts the link loop that keeps retrying a pending acknowledgement and the person key
/// release. The process runs one loop, so a later call does nothing.
pub(crate) fn start_link_reconciliation(app: tauri::AppHandle) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            reconcile_link(&app).await;
            tokio::time::sleep(LINK_RETRY_DELAY).await;
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
/// of it first, except over a starter that is still untouched when the swap begins, which holds
/// nothing of the user's.
#[cfg(target_os = "android")]
fn replaced_vault_policy(app: &tauri::AppHandle) -> Result<ReplacedVault, String> {
    crate::vault::replaced_vaults::replaced_vaults_root(app).map(ReplacedVault::BackUpUnlessStarter)
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

/// A phone pairing state under `root` that pins a coordinator desktop, as after linking.
#[cfg(all(test, desktop))]
pub(crate) fn test_linked_phone(root: &std::path::Path) -> PairingManager {
    let desktop = PairingManager::default();
    desktop
        .initialize(root.join("desktop"), "device-desktop".to_string())
        .unwrap();
    let invitation = desktop
        .create_invitation(
            "127.0.0.1:9".parse().unwrap(),
            "vault-1".to_string(),
            super::protocol::test_compatibility(),
            super::protocol::unix_time_ms(),
        )
        .unwrap();
    let phone = PairingManager::default();
    phone
        .initialize(root.join("phone"), "device-phone".to_string())
        .unwrap();
    let (_, identity) = desktop.identity().unwrap();
    phone
        .record_coordinator(&invitation, identity.certificate.as_ref(), None)
        .unwrap();
    phone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_first_link_on_a_computer_over_another_vault_uses_a_new_folder() {
        assert_eq!(destination(true, false, false), Destination::NewFolder);
        // A fresh copy, a computer that already holds the linked vault, and Android all
        // replace the vault in use.
        assert_eq!(destination(true, true, true), Destination::ActiveVault);
        assert_eq!(destination(true, true, false), Destination::ActiveVault);
        assert_eq!(destination(true, false, true), Destination::ActiveVault);
        assert_eq!(destination(false, false, false), Destination::ActiveVault);
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

    #[test]
    fn bootstrap_is_required_only_on_a_pinned_device_without_its_copy() {
        assert_eq!(
            bootstrap_state(true, false, false),
            BootstrapState::Required
        );
        assert_eq!(bootstrap_state(true, true, false), BootstrapState::None);
        // A coordinator and an unlinked device have nothing to copy.
        assert_eq!(bootstrap_state(false, false, false), BootstrapState::None);
        assert_eq!(bootstrap_state(false, true, false), BootstrapState::None);
    }

    #[test]
    fn a_running_receive_reports_running_whatever_the_pairing_state() {
        for pinned in [false, true] {
            for replica_ready in [false, true] {
                assert_eq!(
                    bootstrap_state(pinned, replica_ready, true),
                    BootstrapState::Running
                );
            }
        }
    }

    #[test]
    fn link_status_serializes_with_camel_case_fields_and_bootstrap_names() {
        let status = VaultLinkStatus {
            linked: true,
            coordinator_device_id: Some("device-coordinator".to_string()),
            bootstrap: BootstrapState::Required,
        };
        assert_eq!(
            serde_json::to_value(&status).expect("serialize link status"),
            serde_json::json!({
                "linked": true,
                "coordinatorDeviceId": "device-coordinator",
                "bootstrap": "required",
            })
        );
    }

    #[test]
    fn receiver_reports_running_only_while_the_operation_is_held() {
        let lifecycle = ReceiverLifecycle::default();
        assert!(!lifecycle.is_running());
        let held = lifecycle.operation.try_lock().expect("operation is free");
        assert!(lifecycle.is_running());
        drop(held);
        assert!(!lifecycle.is_running());
    }
}
