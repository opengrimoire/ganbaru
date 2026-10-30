//! Opening an uploaded snapshot as the vault an anchor runs, and building the snapshot copies it
//! serves.
//!
//! Activation mirrors what a linking device does: the archive is extracted into staging, the
//! database is checked, the vault moves into place, and the charter from the setup conversation
//! is pinned. The activation record is written last, once the selected log enrolls this device
//! with this device's key as an admin.

use super::host::AnchorHost;
use super::state::{Activation, DataPaths};
use crate::server::unix_time_ms;
use ganbaru_handoff::pairing::{PairingManager, PinnedCharter, random_token};
use ganbaru_handoff::protocol::{self, BundleMetadata};
use ganbaru_sync::{AuthorizationView, Engine, local};
use ganbaru_sync_contracts::{DeviceKind, DeviceRole};
use ganbaru_sync_replica::anchor_link::local_compatibility;
use ganbaru_sync_replica::authority::read_admin_key;
use ganbaru_sync_replica::backup::{
    ArchiveGate, DATABASE_FILE, create_snapshot_archive, extract_backup_archive,
    validate_vault_database,
};
use ganbaru_sync_replica::serve::bundles::{PreparedSnapshot, SnapshotPreparer, SnapshotRequest};
use ganbaru_sync_replica::service::{SyncHost, blocking};
use ganbaru_sync_replica::writer::read_device_key;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::sync::Arc;

/// What a finished upload asks of [`activate`].
pub struct Upload<'a> {
    pub vault_id: &'a str,
    pub charter: &'a PinnedCharter,
    pub take_anchor: bool,
}

/// Runs the uploaded archive as this anchor's vault, or leaves the anchor as it was.
pub async fn activate(host: &AnchorHost, upload: Upload<'_>) -> Result<(), String> {
    let paths = host.paths().clone();
    let staging = paths.upload_staging();
    let expected = upload.vault_id.to_string();
    let source = paths.upload_archive();
    let destination = staging.clone();
    blocking(move || extract_backup_archive(&source, &destination, Some(&expected)).map(drop))
        .await?;
    validate_vault_database(&staging.join(DATABASE_FILE)).await?;
    remove_vault(&paths)?;
    std::fs::rename(&staging, paths.vault())
        .map_err(|error| format!("move the vault into place: {error}"))?;
    let checked = pin_and_check(host, &upload).await;
    if let Err(message) = checked {
        host.pools().close_path(paths.database()).await?;
        remove_vault(&paths)?;
        return Err(message);
    }
    Activation {
        vault_id: upload.vault_id.to_string(),
        take_anchor: upload.take_anchor,
    }
    .save(&paths)
}

fn remove_vault(paths: &DataPaths) -> Result<(), String> {
    match std::fs::remove_dir_all(paths.vault()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("clear the vault folder: {error}")),
    }
}

/// Installs the pinned charter, or checks it against the one the vault holds, and refuses a log
/// that does not enroll this device as an admin.
async fn pin_and_check(host: &AnchorHost, upload: &Upload<'_>) -> Result<(), String> {
    let charter = upload.charter.verify(upload.vault_id)?;
    let pool = host.connect_for_sync().await?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("open the vault: {error}"))?;
    let engine = Engine::vault();
    let now_ms = u64::try_from(unix_time_ms()).map_err(|_| "the clock is invalid".to_string())?;
    engine
        .init_space(&mut conn, upload.vault_id, now_ms)
        .await
        .map_err(|error| format!("open the sync space: {error}"))?;
    let ctx = local::space_context(&mut conn, upload.vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
        .ok_or_else(|| "the snapshot has no sync space".to_string())?;
    match engine
        .charter(&mut conn, &ctx)
        .await
        .map_err(|error| format!("read sync charter: {error}"))?
    {
        Some(installed) if installed == charter => {}
        Some(_) => return Err("the snapshot has a different charter".to_string()),
        None => engine
            .install_charter(&mut conn, &ctx, &charter)
            .await
            .map_err(|error| format!("install the charter: {error}"))?,
    }
    let view = selected_view(&engine, &mut conn, &ctx).await?;
    let keys = host.keys();
    let vault_id = upload.vault_id.to_string();
    let own_keys = blocking(move || {
        Ok((
            read_device_key(&keys, &vault_id)?,
            read_admin_key(&keys, &vault_id)?,
        ))
    })
    .await?;
    let certificate: [u8; 32] = Sha256::digest(host.pairing().certificate()?.as_ref()).into();
    let device = view
        .devices
        .iter()
        .find(|device| device.device_id == host.own_device_id())
        .filter(|device| device.removed_revision.is_none());
    let enrolled = match (device, own_keys) {
        (Some(device), (Some(device_key), Some(admin_key))) => {
            device.device_key == device_key.public_key()
                && device.tls_fingerprint == certificate
                && device.kind == DeviceKind::Server
                && device.role == DeviceRole::Admin(admin_key.public_key())
        }
        _ => false,
    };
    if enrolled {
        Ok(())
    } else {
        Err("the snapshot does not enroll this server as an admin".to_string())
    }
}

async fn selected_view(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &ganbaru_sync::SpaceContext,
) -> Result<AuthorizationView, String> {
    engine
        .authorization(conn, ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?
        .ok_or_else(|| "the snapshot has no authorization log".to_string())
}

/// Builds the snapshot copies this anchor serves. It seals no derived state, which the flag in
/// the metadata tells the receiver to rebuild. `VACUUM INTO` reads one database state, so serving
/// goes on while a copy is taken.
pub fn snapshot_preparer(
    host: AnchorHost,
    pairing: PairingManager,
    version: &'static str,
) -> SnapshotPreparer {
    Arc::new(move |request: SnapshotRequest| {
        let host = host.clone();
        let pairing = pairing.clone();
        Box::pin(async move { prepare_snapshot(&host, &pairing, version, request).await })
    })
}

async fn prepare_snapshot(
    host: &AnchorHost,
    pairing: &PairingManager,
    version: &'static str,
    request: SnapshotRequest,
) -> Result<PreparedSnapshot, String> {
    if host.activation()?.is_none_or(|active| active.vault_id != request.vault_id) {
        return Err("this server does not run the requested vault".to_string());
    }
    let transfer_id = random_token("transfer")?;
    let (database_snapshot, archive_path) = pairing.outgoing_snapshot_paths(&transfer_id)?;
    let archived = create_snapshot_archive(
        &ArchiveGate::process(),
        &host.paths().vault(),
        &database_snapshot,
        &archive_path,
    )
    .await;
    let _ = std::fs::remove_file(&database_snapshot);
    archived?;
    let measured = archive_path
        .metadata()
        .map_err(|error| format!("inspect the snapshot: {error}"))
        .map(|metadata| metadata.len())
        .and_then(|bytes| Ok((bytes, protocol::sha256_file(&archive_path)?)));
    let (archive_bytes, archive_sha256) = match measured {
        Ok(measured) => measured,
        Err(error) => {
            let _ = std::fs::remove_file(&archive_path);
            return Err(error);
        }
    };
    Ok(PreparedSnapshot {
        metadata: BundleMetadata {
            protocol_version: protocol::PROTOCOL_VERSION,
            compatibility: local_compatibility(version),
            vault_id: request.vault_id,
            device_id: request.device_id,
            transfer_id,
            archive_bytes,
            archive_sha256,
            derived_state_stale: true,
        },
        archive_path,
    })
}
