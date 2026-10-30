//! Transfer progress and resumable staging on the shared pairing state lock.

#[cfg(desktop)]
use super::RegisteredBundle;
use super::storage::{
    validate_activation_record, validate_stored_outgoing, write_private_file_atomically,
};
use super::{
    PairingManager, PendingAcknowledgement, StoredOutgoingTransfer, initialized_state,
    initialized_state_mut, persist_initialized_state,
};
#[cfg(desktop)]
use crate::protocol;
use crate::protocol::{BundleMetadata, validate_identifier};
use std::fs;
use std::path::{Path, PathBuf};

impl PairingManager {
    pub fn has_pending_transfer(&self) -> Result<bool, String> {
        let inner = self.lock()?;
        let state = initialized_state(&inner)?;
        Ok(state.pending_acknowledgement.is_some() || state.outgoing_transfer.is_some())
    }

    pub fn replica_ready(&self) -> Result<bool, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?.replica_ready)
    }

    pub fn pending_acknowledgement(&self) -> Result<Option<PendingAcknowledgement>, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?.pending_acknowledgement.clone())
    }

    pub fn record_activation(&self, acknowledgement: PendingAcknowledgement) -> Result<(), String> {
        validate_identifier("vault id", &acknowledgement.vault_id)?;
        validate_identifier("device id", &acknowledgement.device_id)?;
        validate_identifier("transfer id", &acknowledgement.transfer_id)?;
        let mut inner = self.lock()?;
        let state = initialized_state_mut(&mut inner)?;
        state.replica_ready = true;
        state.pending_acknowledgement = Some(acknowledgement);
        persist_initialized_state(&inner)
    }

    pub fn clear_pending_acknowledgement(&self, transfer_id: &str) -> Result<(), String> {
        validate_identifier("transfer id", transfer_id)?;
        let mut inner = self.lock()?;
        let state = initialized_state_mut(&mut inner)?;
        match state.pending_acknowledgement.as_ref() {
            Some(pending) if pending.transfer_id == transfer_id => {
                state.pending_acknowledgement = None;
                persist_initialized_state(&inner)
            }
            None => Ok(()),
            Some(_) => Err("pending activation acknowledgement does not match".to_string()),
        }
    }

    pub fn store_outgoing_transfer(&self, transfer: StoredOutgoingTransfer) -> Result<(), String> {
        validate_stored_outgoing(&transfer)?;
        let mut inner = self.lock()?;
        let state = initialized_state_mut(&mut inner)?;
        state.outgoing_transfer = Some(transfer);
        persist_initialized_state(&inner)
    }

    pub fn outgoing_transfer(&self) -> Result<Option<StoredOutgoingTransfer>, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?.outgoing_transfer.clone())
    }

    pub fn clear_outgoing_transfer(&self, transfer_id: &str) -> Result<(), String> {
        validate_identifier("transfer id", transfer_id)?;
        let mut inner = self.lock()?;
        let state = initialized_state_mut(&mut inner)?;
        match state.outgoing_transfer.as_ref() {
            Some(transfer) if transfer.metadata.transfer_id == transfer_id => {
                state.outgoing_transfer = None;
                persist_initialized_state(&inner)
            }
            None => Ok(()),
            Some(_) => Err("outgoing transfer identity does not match".to_string()),
        }
    }

    pub fn complete_outgoing_activation(
        &self,
        completed: PendingAcknowledgement,
    ) -> Result<(), String> {
        validate_activation_record(&completed)?;
        let mut inner = self.lock()?;
        let state = initialized_state_mut(&mut inner)?;
        if state
            .outgoing_transfer
            .as_ref()
            .is_some_and(|transfer| transfer.metadata.transfer_id != completed.transfer_id)
        {
            return Err("completed activation does not match outgoing transfer".to_string());
        }
        state.outgoing_transfer = None;
        state.completed_activation = Some(completed);
        persist_initialized_state(&inner)
    }

    #[cfg(desktop)]
    pub fn completed_activation(&self) -> Result<Option<PendingAcknowledgement>, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?.completed_activation.clone())
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
        Ok((
            root.join(format!("{transfer_id}.source.sqlite")),
            root.join(format!("{transfer_id}.source.zip")),
        ))
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
