//! Transfer progress and resumable staging on the shared pairing state lock.

#[cfg(desktop)]
use super::OUTGOING_TRANSFER_LIFETIME_MS;
#[cfg(desktop)]
use super::RegisteredBundle;
#[cfg(desktop)]
use super::storage::validate_stored_outgoing;
use super::storage::{validate_activation_record, write_private_file_atomically};
use super::{
    LinkEvent, PairingManager, PairingManagerInner, PendingAcknowledgement, StoredOutgoingTransfer,
    initialized_state, update_state,
};
#[cfg(desktop)]
use crate::protocol;
use crate::protocol::{BundleMetadata, validate_identifier};
use std::fs;
use std::path::{Path, PathBuf};

impl PairingManager {
    /// Whether a handoff is in flight: a copy this coordinator offers, or an activation this
    /// member has not had acknowledged. A copy past its lifetime is released first.
    pub fn has_pending_transfer(&self) -> Result<bool, String> {
        let mut inner = self.lock()?;
        release_expired_outgoing(&mut inner, crate::protocol::unix_time_ms())?;
        Ok(initialized_state(&inner)?.role.has_active_handoff())
    }

    pub fn replica_ready(&self) -> Result<bool, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?.role.copy_ready())
    }

    pub fn pending_acknowledgement(&self) -> Result<Option<PendingAcknowledgement>, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?
            .role
            .pending_acknowledgement()
            .cloned())
    }

    pub fn record_activation(&self, acknowledgement: PendingAcknowledgement) -> Result<(), String> {
        validate_identifier("vault id", &acknowledgement.vault_id)?;
        validate_identifier("device id", &acknowledgement.device_id)?;
        validate_identifier("transfer id", &acknowledgement.transfer_id)?;
        let mut inner = self.lock()?;
        update_state(&mut inner, |state| {
            state.apply(LinkEvent::CopyActivated { acknowledgement })
        })
    }

    pub fn clear_pending_acknowledgement(&self, transfer_id: &str) -> Result<(), String> {
        validate_identifier("transfer id", transfer_id)?;
        let mut inner = self.lock()?;
        if initialized_state(&inner)?
            .role
            .pending_acknowledgement()
            .is_none()
        {
            return Ok(());
        }
        update_state(&mut inner, |state| {
            state.apply(LinkEvent::CopyAcknowledged {
                transfer_id: transfer_id.to_string(),
            })
        })
    }

    /// Offers the archive staged for `metadata` to the device it names until the transfer
    /// lifetime passes from `now_ms`. The stored record and the served bundle change together,
    /// and the staged files are removed when the copy cannot be offered.
    #[cfg(desktop)]
    pub fn begin_outgoing_transfer(
        &self,
        metadata: BundleMetadata,
        now_ms: i64,
    ) -> Result<StoredOutgoingTransfer, String> {
        let transfer_id = metadata.transfer_id.clone();
        let result = self.offer_outgoing_transfer(metadata, now_ms);
        if result.is_err() {
            let mut inner = self.lock()?;
            discard_outgoing_copy(&mut inner, &transfer_id);
        }
        result
    }

    #[cfg(desktop)]
    fn offer_outgoing_transfer(
        &self,
        metadata: BundleMetadata,
        now_ms: i64,
    ) -> Result<StoredOutgoingTransfer, String> {
        let (_, archive_path) = self.outgoing_snapshot_paths(&metadata.transfer_id)?;
        protocol::validate_staging_file(&archive_path, &metadata)?;
        let transfer = StoredOutgoingTransfer {
            metadata,
            expires_at_ms: now_ms.saturating_add(OUTGOING_TRANSFER_LIFETIME_MS),
        };
        validate_stored_outgoing(&transfer)?;
        let mut inner = self.lock()?;
        release_expired_outgoing(&mut inner, now_ms)?;
        let linked_for_vault = initialized_state(&inner)?
            .linked_peers
            .get(&transfer.metadata.device_id)
            .is_some_and(|peer| peer.vault_id == transfer.metadata.vault_id);
        if !linked_for_vault {
            return Err("the vault copy is not for a device linked for this vault".to_string());
        }
        update_state(&mut inner, |state| {
            state.apply(LinkEvent::OutgoingPrepared {
                transfer: transfer.clone(),
            })
        })?;
        inner.outgoing_bundles.insert(
            transfer.metadata.transfer_id.clone(),
            RegisteredBundle {
                metadata: transfer.metadata.clone(),
                path: archive_path,
            },
        );
        Ok(transfer)
    }

    /// The copy offered at `now_ms`, after releasing one past its lifetime.
    pub fn active_outgoing_transfer(
        &self,
        now_ms: i64,
    ) -> Result<Option<StoredOutgoingTransfer>, String> {
        let mut inner = self.lock()?;
        release_expired_outgoing(&mut inner, now_ms)?;
        Ok(initialized_state(&inner)?.role.outgoing().cloned())
    }

    /// Stops offering the copy `transfer_id` and removes its staged files.
    pub fn release_outgoing_transfer(&self, transfer_id: &str) -> Result<(), String> {
        validate_identifier("transfer id", transfer_id)?;
        let mut inner = self.lock()?;
        let offered = initialized_state(&inner)?
            .role
            .outgoing()
            .is_some_and(|transfer| transfer.metadata.transfer_id == transfer_id);
        if !offered {
            return Err("transfer is no longer active".to_string());
        }
        release_outgoing(&mut inner, transfer_id)
    }

    /// Brings the served bundle back in line with the stored copy after the state loads. A copy
    /// past its lifetime, or whose archive did not survive intact, is released; on a device that
    /// does not coordinate, a stored copy is always released because nothing serves it.
    pub(super) fn restore_outgoing_transfer(&self, now_ms: i64) -> Result<(), String> {
        let Some(transfer) = self.active_outgoing_transfer(now_ms)? else {
            return Ok(());
        };
        let transfer_id = transfer.metadata.transfer_id.clone();
        let (_, archive_path) = self.outgoing_snapshot_paths(&transfer_id)?;
        let usable = crate::protocol::validate_staging_file(&archive_path, &transfer.metadata);
        let mut inner = self.lock()?;
        if initialized_state(&inner)?.role.outgoing() != Some(&transfer) {
            return Ok(());
        }
        #[cfg(desktop)]
        if usable.is_ok() {
            inner.outgoing_bundles.insert(
                transfer_id,
                RegisteredBundle {
                    metadata: transfer.metadata,
                    path: archive_path,
                },
            );
            return Ok(());
        }
        if let Err(error) = usable {
            eprintln!("release a vault copy that did not survive a restart: {error}");
        }
        release_outgoing(&mut inner, &transfer_id)
    }

    /// Records that the receiver activated the copy, which also releases it.
    pub fn complete_outgoing_activation(
        &self,
        completed: PendingAcknowledgement,
    ) -> Result<(), String> {
        validate_activation_record(&completed)?;
        let transfer_id = completed.transfer_id.clone();
        let mut inner = self.lock()?;
        update_state(&mut inner, |state| {
            state.apply(LinkEvent::OutgoingActivated { completed })
        })?;
        discard_outgoing_copy(&mut inner, &transfer_id);
        Ok(())
    }

    #[cfg(desktop)]
    pub fn completed_activation(&self) -> Result<Option<PendingAcknowledgement>, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?.role.completed().cloned())
    }

    #[cfg(desktop)]
    pub fn register_outgoing_bundle(
        &self,
        metadata: BundleMetadata,
        path: PathBuf,
    ) -> Result<(), String> {
        metadata.validate()?;
        protocol::validate_staging_file(&path, &metadata)?;
        let mut inner = self.lock()?;
        initialized_state(&inner)?;
        inner.outgoing_bundles.insert(
            metadata.transfer_id.clone(),
            RegisteredBundle { metadata, path },
        );
        Ok(())
    }

    #[cfg(desktop)]
    pub fn outgoing_bundle(&self, transfer_id: &str) -> Result<RegisteredBundle, String> {
        validate_identifier("transfer id", transfer_id)?;
        let inner = self.lock()?;
        inner
            .outgoing_bundles
            .get(transfer_id)
            .cloned()
            .ok_or_else(|| "transfer is not available".to_string())
    }

    #[cfg(desktop)]
    pub fn unregister_outgoing_bundle(&self, transfer_id: &str) -> Result<(), String> {
        validate_identifier("transfer id", transfer_id)?;
        let mut inner = self.lock()?;
        inner.outgoing_bundles.remove(transfer_id);
        Ok(())
    }

    pub fn outgoing_snapshot_paths(&self, transfer_id: &str) -> Result<(PathBuf, PathBuf), String> {
        validate_identifier("transfer id", transfer_id)?;
        let inner = self.lock()?;
        let root = inner
            .staging_root
            .as_ref()
            .ok_or_else(|| "pairing state is not initialized".to_string())?;
        Ok(outgoing_paths(root, transfer_id))
    }

    pub fn staging_paths(&self, transfer_id: &str) -> Result<(PathBuf, PathBuf, PathBuf), String> {
        validate_identifier("transfer id", transfer_id)?;
        let inner = self.lock()?;
        let root = inner
            .staging_root
            .as_ref()
            .ok_or_else(|| "pairing state is not initialized".to_string())?;
        Ok((
            root.join(format!("{transfer_id}.part")),
            root.join(format!("{transfer_id}.json")),
            root.join(format!("{transfer_id}.zip")),
        ))
    }

    pub fn remove_staging(&self, transfer_id: &str) -> Result<(), String> {
        let (partial, metadata, complete) = self.staging_paths(transfer_id)?;
        for path in [partial, metadata, complete] {
            let result = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            match result {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("remove staged transfer: {error}")),
            }
        }
        Ok(())
    }

    pub fn write_staging_metadata(
        &self,
        path: &Path,
        metadata: &BundleMetadata,
    ) -> Result<(), String> {
        metadata.validate()?;
        let json = serde_json::to_vec(metadata)
            .map_err(|error| format!("encode transfer metadata: {error}"))?;
        write_private_file_atomically(path, &json)
    }

    pub fn read_staging_metadata(&self, path: &Path) -> Result<Option<BundleMetadata>, String> {
        match fs::read(path) {
            Ok(json) => {
                let metadata: BundleMetadata = serde_json::from_slice(&json)
                    .map_err(|error| format!("decode transfer metadata: {error}"))?;
                metadata.validate()?;
                Ok(Some(metadata))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("read transfer metadata: {error}")),
        }
    }
}

/// Releases the stored copy when it is past its lifetime at `now_ms`.
pub(super) fn release_expired_outgoing(
    inner: &mut PairingManagerInner,
    now_ms: i64,
) -> Result<(), String> {
    let Some(transfer_id) = initialized_state(inner)?
        .role
        .outgoing()
        .filter(|transfer| transfer.is_expired(now_ms))
        .map(|transfer| transfer.metadata.transfer_id.clone())
    else {
        return Ok(());
    };
    release_outgoing(inner, &transfer_id)
}

fn release_outgoing(inner: &mut PairingManagerInner, transfer_id: &str) -> Result<(), String> {
    update_state(inner, |state| {
        state.apply(LinkEvent::OutgoingReleased {
            transfer_id: transfer_id.to_string(),
        })
    })?;
    discard_outgoing_copy(inner, transfer_id);
    Ok(())
}

/// Stops serving `transfer_id` and removes its staged database copy and archive. A file that
/// cannot be removed is reported and left in staging.
fn discard_outgoing_copy(inner: &mut PairingManagerInner, transfer_id: &str) {
    #[cfg(desktop)]
    inner.outgoing_bundles.remove(transfer_id);
    let Some(root) = inner.staging_root.as_ref() else {
        return;
    };
    let (database, archive) = outgoing_paths(root, transfer_id);
    for path in [database, archive] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!("remove a released vault copy: {error}"),
        }
    }
}

fn outgoing_paths(root: &Path, transfer_id: &str) -> (PathBuf, PathBuf) {
    (
        root.join(format!("{transfer_id}.source.sqlite")),
        root.join(format!("{transfer_id}.source.zip")),
    )
}
