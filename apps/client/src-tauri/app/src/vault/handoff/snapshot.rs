//! Vault snapshot archives a desktop serves to another device: the coordinator for a first or
//! fresh copy, and a member listener for a fresh copy. Both build the archive the same way, so the
//! copy leaves out Local tables and device-only files whichever device serves it.

use super::pairing::{PairingManager, random_token};
use super::protocol::{BundleMetadata, PROTOCOL_VERSION};
use super::transport::{PreparedSnapshot, SnapshotPreparer, SnapshotRequest};
use super::{current_compatibility, sha256_file};
use crate::vault::quiescence::begin_snapshot_quiescence;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Runtime};

/// Archives the active vault for `transfer_id` while the caller keeps vault writers paused, and
/// returns the archive path. The database copy the archive is built from is always removed, and
/// so is a partial archive when building fails.
pub(crate) async fn archive_active_vault<R: Runtime>(
    app: &AppHandle<R>,
    pairing: &PairingManager,
    transfer_id: &str,
) -> Result<PathBuf, String> {
    let (database_snapshot, archive_path) = pairing.outgoing_snapshot_paths(transfer_id)?;
    let vault_root = crate::vault::active_vault_path(app)?;
    let result = crate::vault::backup::create_handoff_archive(
        &vault_root,
        &database_snapshot,
        &archive_path,
    )
    .await;
    let _ = fs::remove_file(&database_snapshot);
    if let Err(error) = result {
        let _ = fs::remove_file(&archive_path);
        return Err(error);
    }
    Ok(archive_path)
}

/// The metadata a receiver checks the archive at `archive_path` against.
pub(crate) fn bundle_metadata<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: String,
    device_id: String,
    transfer_id: String,
    archive_path: &Path,
) -> Result<BundleMetadata, String> {
    Ok(BundleMetadata {
        protocol_version: PROTOCOL_VERSION,
        compatibility: current_compatibility(app),
        vault_id,
        device_id,
        transfer_id,
        archive_bytes: archive_path
            .metadata()
            .map_err(|error| format!("inspect handoff archive: {error}"))?
            .len(),
        archive_sha256: sha256_file(archive_path)?,
    })
}

/// Prepares the snapshot copies a member listener serves, from the active vault of this
/// desktop. A member that has not taken its own first copy of the vault serves none.
pub(crate) fn member_snapshot_preparer<R: Runtime>(
    app: AppHandle<R>,
    pairing: PairingManager,
) -> SnapshotPreparer {
    Arc::new(move |request: SnapshotRequest| {
        let app = app.clone();
        let pairing = pairing.clone();
        Box::pin(async move { prepare_member_snapshot(&app, &pairing, request).await })
    })
}

async fn prepare_member_snapshot<R: Runtime>(
    app: &AppHandle<R>,
    pairing: &PairingManager,
    request: SnapshotRequest,
) -> Result<PreparedSnapshot, String> {
    if crate::vault::active_vault_id(app)? != request.vault_id {
        return Err("this device does not have the requested vault open".to_string());
    }
    if !pairing.replica_ready()? {
        return Err("this device has not taken its own copy of the vault yet".to_string());
    }
    let transfer_id = random_token("transfer")?;
    let quiescence = begin_snapshot_quiescence(app).await?;
    let archived = archive_active_vault(app, pairing, &transfer_id).await;
    drop(quiescence);
    let archive_path = archived?;
    match bundle_metadata(
        app,
        request.vault_id,
        request.device_id,
        transfer_id,
        &archive_path,
    ) {
        Ok(metadata) => Ok(PreparedSnapshot {
            metadata,
            archive_path,
        }),
        Err(error) => {
            let _ = fs::remove_file(&archive_path);
            Err(error)
        }
    }
}
