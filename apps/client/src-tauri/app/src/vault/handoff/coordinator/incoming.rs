//! Coordinator staging and activation of snapshots returned by the current owner.

use super::{CoordinatorResponse, CoordinatorState};
use crate::vault::handoff::pairing::{PendingAcknowledgement, StoredIncomingTransfer};
use crate::vault::handoff::protocol::{BundleMetadata, BundlePurpose};
use crate::vault::handoff::{current_compatibility, protocol};
use crate::vault::ownership::VaultOwnershipManager;
use tauri::{Manager, Runtime};

impl<R: Runtime> CoordinatorState<R> {
    pub(super) async fn uploaded(
        &mut self,
        metadata: BundleMetadata,
        source_device_id: String,
        purpose: BundlePurpose,
    ) -> Result<CoordinatorResponse, String> {
        self.authorize_upload(&metadata, &source_device_id, purpose)?;
        if let Some(completed) = self.pairing.completed_activation()? {
            if completed.transfer_id == metadata.transfer_id
                && completed.purpose == purpose
                && completed.generation == metadata.generation
            {
                return Ok(CoordinatorResponse::IncomingStaged {
                    transfer_id: metadata.transfer_id,
                });
            }
        }
        if let Some(incoming) = self.pairing.incoming_transfer()? {
            if incoming.metadata == metadata
                && incoming.source_device_id == source_device_id
                && incoming.purpose == purpose
            {
                if purpose == BundlePurpose::Refresh {
                    let staging = crate::vault::backup::active_handoff_staging_path(
                        &self.app,
                        &metadata.transfer_id,
                    )?;
                    crate::vault::backup::activate_active_handoff(
                        &self.app,
                        &staging,
                        &metadata.transfer_id,
                        &metadata.vault_id,
                        false,
                    )
                    .await?;
                    let (device_id, _) = self.pairing.identity()?;
                    self.pairing
                        .complete_incoming_activation(PendingAcknowledgement {
                            vault_id: metadata.vault_id.clone(),
                            device_id,
                            transfer_id: metadata.transfer_id.clone(),
                            generation: metadata.generation,
                            purpose,
                        })?;
                    self.reload_shell()?;
                    if let Err(error) = self.pairing.remove_staging(&metadata.transfer_id) {
                        eprintln!("failed to clean completed vault upload: {error}");
                    }
                }
                return Ok(CoordinatorResponse::IncomingStaged {
                    transfer_id: metadata.transfer_id,
                });
            }
            return Err("another uploaded vault transfer is pending".to_string());
        }
        let (local_device_id, _) = self.pairing.identity()?;
        let status = self
            .app
            .state::<VaultOwnershipManager>()
            .status(&metadata.vault_id)?;
        let expected_generation = match purpose {
            BundlePurpose::Ownership => status
                .generation
                .checked_add(1)
                .ok_or_else(|| "vault ownership generation is exhausted".to_string())?,
            BundlePurpose::Refresh => status.generation,
        };
        if metadata.device_id != local_device_id
            || status.can_write
            || status.owner_device_id != source_device_id
            || metadata.generation != expected_generation
            || self.pairing.requested_upload()? != Some(purpose)
        {
            return Err("uploaded bundle does not match the requested owner state".to_string());
        }
        let (_, _, archive) = self.pairing.staging_paths(&metadata.transfer_id)?;
        let staging =
            crate::vault::backup::active_handoff_staging_path(&self.app, &metadata.transfer_id)?;
        crate::vault::backup::stage_handoff_archive(&archive, &staging, &metadata.vault_id).await?;
        self.pairing
            .store_incoming_transfer(StoredIncomingTransfer {
                metadata: metadata.clone(),
                source_device_id,
                purpose,
            })?;
        if purpose == BundlePurpose::Refresh {
            crate::vault::backup::activate_active_handoff(
                &self.app,
                &staging,
                &metadata.transfer_id,
                &metadata.vault_id,
                false,
            )
            .await?;
            let (device_id, _) = self.pairing.identity()?;
            self.pairing
                .complete_incoming_activation(PendingAcknowledgement {
                    vault_id: metadata.vault_id.clone(),
                    device_id,
                    transfer_id: metadata.transfer_id.clone(),
                    generation: metadata.generation,
                    purpose,
                })?;
            self.reload_shell()?;
            if let Err(error) = self.pairing.remove_staging(&metadata.transfer_id) {
                eprintln!("failed to clean completed vault upload: {error}");
            }
        }
        Ok(CoordinatorResponse::IncomingStaged {
            transfer_id: metadata.transfer_id,
        })
    }

    pub(super) fn authorize_upload(
        &mut self,
        metadata: &BundleMetadata,
        source_device_id: &str,
        purpose: BundlePurpose,
    ) -> Result<CoordinatorResponse, String> {
        protocol::ensure_compatible(&current_compatibility(&self.app), &metadata.compatibility)?;
        if let Some(completed) = self.pairing.completed_activation()? {
            if completed.vault_id == metadata.vault_id
                && completed.device_id == metadata.device_id
                && completed.transfer_id == metadata.transfer_id
                && completed.purpose == purpose
                && completed.generation == metadata.generation
            {
                return Ok(CoordinatorResponse::UploadAuthorized {
                    transfer_id: metadata.transfer_id.clone(),
                    already_received: true,
                });
            }
        }
        if let Some(incoming) = self.pairing.incoming_transfer()? {
            if incoming.metadata == *metadata
                && incoming.source_device_id == source_device_id
                && incoming.purpose == purpose
            {
                return Ok(CoordinatorResponse::UploadAuthorized {
                    transfer_id: metadata.transfer_id.clone(),
                    already_received: true,
                });
            }
            return Err("another uploaded vault transfer is pending".to_string());
        }
        let (local_device_id, _) = self.pairing.identity()?;
        let status = self
            .app
            .state::<VaultOwnershipManager>()
            .status(&metadata.vault_id)?;
        let expected_generation = match purpose {
            BundlePurpose::Ownership => status
                .generation
                .checked_add(1)
                .ok_or_else(|| "vault ownership generation is exhausted".to_string())?,
            BundlePurpose::Refresh => status.generation,
        };
        if metadata.device_id != local_device_id
            || status.can_write
            || status.owner_device_id != source_device_id
            || metadata.generation != expected_generation
            || self.pairing.requested_upload()? != Some(purpose)
        {
            return Err("uploaded bundle does not match the requested owner state".to_string());
        }
        Ok(CoordinatorResponse::UploadAuthorized {
            transfer_id: metadata.transfer_id.clone(),
            already_received: false,
        })
    }

    pub(super) async fn commit_uploaded_ownership(
        &mut self,
        metadata: BundleMetadata,
        source_device_id: String,
    ) -> Result<CoordinatorResponse, String> {
        if let Some(completed) = self.pairing.completed_activation()? {
            if completed.vault_id == metadata.vault_id
                && completed.transfer_id == metadata.transfer_id
                && completed.generation == metadata.generation
                && completed.purpose == BundlePurpose::Ownership
            {
                return Ok(CoordinatorResponse::ActivationAcknowledged {
                    transfer_id: metadata.transfer_id,
                });
            }
        }
        let incoming = self
            .pairing
            .incoming_transfer()?
            .ok_or_else(|| "uploaded ownership transfer is not staged".to_string())?;
        if incoming.metadata != metadata
            || incoming.source_device_id != source_device_id
            || incoming.purpose != BundlePurpose::Ownership
        {
            return Err("uploaded ownership grant does not match staged data".to_string());
        }
        let ownership = self.app.state::<VaultOwnershipManager>();
        let status = ownership.status(&metadata.vault_id)?;
        let needs_finalize = match status.transfer_phase {
            crate::vault::ownership::TransferPhase::IncomingCommitted {
                ref transfer_id,
                ref source_device_id,
                committed_generation,
            } if transfer_id == &metadata.transfer_id
                && source_device_id == &incoming.source_device_id
                && committed_generation == metadata.generation =>
            {
                true
            }
            crate::vault::ownership::TransferPhase::Stable
                if status.can_write && status.generation == metadata.generation =>
            {
                false
            }
            crate::vault::ownership::TransferPhase::Stable => {
                ownership.accept_incoming_grant(
                    &metadata.vault_id,
                    metadata.transfer_id.clone(),
                    source_device_id,
                    metadata.generation,
                )?;
                true
            }
            _ => return Err("desktop ownership state conflicts with uploaded grant".to_string()),
        };
        let staging =
            crate::vault::backup::active_handoff_staging_path(&self.app, &metadata.transfer_id)?;
        crate::vault::backup::activate_uploaded_ownership(
            &self.app,
            &staging,
            &metadata.transfer_id,
            &metadata.vault_id,
        )
        .await?;
        if needs_finalize {
            ownership.finalize_incoming(
                &metadata.vault_id,
                &metadata.transfer_id,
                metadata.generation,
            )?;
        }
        let (device_id, _) = self.pairing.identity()?;
        self.pairing
            .complete_incoming_activation(PendingAcknowledgement {
                vault_id: metadata.vault_id,
                device_id,
                transfer_id: metadata.transfer_id.clone(),
                generation: metadata.generation,
                purpose: BundlePurpose::Ownership,
            })?;
        self.reload_shell()?;
        if let Err(error) = self.pairing.remove_staging(&metadata.transfer_id) {
            eprintln!("failed to clean completed vault upload: {error}");
        }
        Ok(CoordinatorResponse::ActivationAcknowledged {
            transfer_id: metadata.transfer_id,
        })
    }
}
