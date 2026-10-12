//! Whole-vault receive, activation, acknowledgement, and refresh lifecycle.

use super::pairing::{PairingManager, PendingAcknowledgement};
use super::protocol::BundlePurpose;
use super::transport::TransferCancellation;
use crate::vault::ownership::{TransferPhase, VaultOwnershipManager};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager;

const CONNECTED_REPOLL_DELAY: Duration = Duration::from_secs(1);
const DISCONNECTED_RETRY_DELAY: Duration = Duration::from_secs(30);

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
    if status.can_write || status.owner_device_id != coordinator.device_id {
        return Err("desktop is not the current owner of this vault".to_string());
    }
    if matches!(mode, ReceiveMode::Refresh) && !pairing.replica_ready()? {
        return Err("the linked vault must be activated before it can refresh".to_string());
    }
    if matches!(mode, ReceiveMode::Bootstrap) && pairing.replica_ready()? {
        return Err("the linked vault is already active on this device".to_string());
    }

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
    let staging = handoff_staging_path(app, &metadata.transfer_id)?;
    crate::vault::backup::stage_handoff_archive(&archive, &staging, &metadata.vault_id).await?;

    let preserve_local_copy = purpose == BundlePurpose::Ownership && !pairing.replica_ready()?;
    if preserve_local_copy && cfg!(target_os = "android") {
        #[cfg(target_os = "android")]
        crate::vault::backup::vault_backup_to_downloads(app.clone()).await?;
    }

    if purpose == BundlePurpose::Ownership {
        let (_, generation) =
            super::transport::commit_staged_ownership(&pairing, metadata.clone()).await?;
        ownership.accept_incoming_coordinator_grant(
            &metadata.vault_id,
            metadata.transfer_id.clone(),
            coordinator.device_id,
            generation,
        )?;
    } else {
        ownership.advance_remote_coordinator(
            &metadata.vault_id,
            &coordinator.device_id,
            metadata.generation,
        )?;
    }

    activate_handoff(
        app,
        &staging,
        &metadata.transfer_id,
        &metadata.vault_id,
        preserve_local_copy,
    )
    .await?;
    let (device_id, _) = pairing.identity()?;
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
        !pairing.replica_ready()?,
    )
    .await?;
    let (device_id, _) = pairing.identity()?;
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
            "if (window.dispatchEvent(new Event('ganbaru-ai:mobile-ownership-reload-requested', { cancelable: true }))) window.location.reload()",
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
    let was_connected = lifecycle.connected.swap(reachable, Ordering::AcqRel);
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
    preserve_previous: bool,
) -> Result<crate::vault::VaultInfo, String> {
    crate::vault::backup::activate_android_handoff(
        app,
        staging,
        transfer_id,
        vault_id,
        preserve_previous,
    )
    .await
}

#[cfg(desktop)]
async fn activate_handoff(
    app: &tauri::AppHandle,
    staging: &std::path::Path,
    transfer_id: &str,
    vault_id: &str,
    preserve_previous: bool,
) -> Result<crate::vault::VaultInfo, String> {
    crate::vault::backup::activate_active_handoff(
        app,
        staging,
        transfer_id,
        vault_id,
        preserve_previous,
    )
    .await
}

#[cfg(target_os = "ios")]
async fn activate_handoff(
    _app: &tauri::AppHandle,
    _staging: &std::path::Path,
    _transfer_id: &str,
    _vault_id: &str,
    _preserve_previous: bool,
) -> Result<crate::vault::VaultInfo, String> {
    Err("vault handoff is not available on iOS".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_modes_map_to_the_single_transport_purpose() {
        assert_eq!(ReceiveMode::Ownership.purpose(), BundlePurpose::Ownership);
        assert_eq!(ReceiveMode::Refresh.purpose(), BundlePurpose::Refresh);
        assert_eq!(ReceiveMode::Bootstrap.purpose(), BundlePurpose::Refresh);
        assert_eq!(super::super::protocol::PROTOCOL_VERSION, 5);
    }
}
