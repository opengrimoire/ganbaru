//! Coordinator snapshot preparation and receiver acknowledgement.

use super::{CompletedActivation, CoordinatorResponse, CoordinatorState, PreparedTransfer};
use crate::vault::handoff::pairing::{
    PendingAcknowledgement, StoredOutgoingTransfer, random_token,
};
use crate::vault::handoff::snapshot::{archive_active_vault, bundle_metadata};
use crate::vault::handoff::transport::BUNDLE_BUSY_CODE;
use crate::vault::quiescence::begin_snapshot_quiescence;
use std::fs;
use std::time::Instant;
use tauri::Runtime;

impl<R: Runtime> CoordinatorState<R> {
    /// Prepares a vault copy for one linked device, or answers with the copy already prepared for
    /// it. While a copy for another device is pending, the request is refused as retryable.
    pub(super) async fn prepare(
        &mut self,
        vault_id: String,
        device_id: String,
    ) -> Result<CoordinatorResponse, String> {
        if let Some(prepared) = self.prepared.as_ref() {
            if prepared.metadata.vault_id == vault_id && prepared.metadata.device_id == device_id {
                return Ok(CoordinatorResponse::Prepared {
                    metadata: prepared.metadata.clone(),
                });
            }
            return Ok(CoordinatorResponse::Rejected {
                code: BUNDLE_BUSY_CODE.to_string(),
                message: "the coordinator is serving a vault copy to another device".to_string(),
                retryable: true,
            });
        }
        super::require_active_vault(&crate::vault::active_vault_id(&self.app)?, &vault_id)?;
        self.bundle_preparations.admit(&device_id, Instant::now())?;
        crate::distractions::usage_totals::flush_active_vault(&self.app).await?;
        let transfer_id = random_token("transfer")?;
        let snapshot_quiescence = begin_snapshot_quiescence(&self.app).await?;
        let archived = archive_active_vault(&self.app, &self.pairing, &transfer_id).await;
        drop(snapshot_quiescence);
        let archive_path = archived?;
        let metadata =
            match bundle_metadata(&self.app, vault_id, device_id, transfer_id, &archive_path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    let _ = fs::remove_file(&archive_path);
                    return Err(error);
                }
            };
        if let Err(error) = self
            .pairing
            .register_outgoing_bundle(metadata.clone(), archive_path.clone())
        {
            let _ = fs::remove_file(&archive_path);
            return Err(error);
        }
        if let Err(error) = self
            .pairing
            .store_outgoing_transfer(StoredOutgoingTransfer {
                metadata: metadata.clone(),
            })
        {
            let _ = self
                .pairing
                .unregister_outgoing_bundle(&metadata.transfer_id);
            let _ = fs::remove_file(&archive_path);
            return Err(error);
        }
        self.prepared = Some(PreparedTransfer {
            metadata: metadata.clone(),
            archive_path,
        });
        Ok(CoordinatorResponse::Prepared { metadata })
    }

    pub(super) fn activated(
        &mut self,
        vault_id: String,
        device_id: String,
        transfer_id: String,
    ) -> Result<CoordinatorResponse, String> {
        let completed = CompletedActivation {
            vault_id,
            device_id,
            transfer_id: transfer_id.clone(),
        };
        if self.completed.as_ref() == Some(&completed) {
            return Ok(CoordinatorResponse::ActivationAcknowledged { transfer_id });
        }
        let prepared = self
            .prepared
            .as_ref()
            .ok_or_else(|| "activation does not match an active transfer".to_string())?;
        if prepared.metadata.vault_id != completed.vault_id
            || prepared.metadata.device_id != completed.device_id
            || prepared.metadata.transfer_id != completed.transfer_id
        {
            return Err("activation does not match the prepared transfer".to_string());
        }
        self.pairing
            .complete_outgoing_activation(PendingAcknowledgement {
                vault_id: completed.vault_id.clone(),
                device_id: completed.device_id.clone(),
                transfer_id: completed.transfer_id.clone(),
            })?;
        self.cleanup_prepared();
        self.completed = Some(completed);
        Ok(CoordinatorResponse::ActivationAcknowledged { transfer_id })
    }

    pub(super) fn cancel(&mut self, transfer_id: String) -> Result<CoordinatorResponse, String> {
        let prepared = self
            .prepared
            .as_ref()
            .ok_or_else(|| "transfer is no longer active".to_string())?;
        if prepared.metadata.transfer_id != transfer_id {
            return Err("cancelled transfer does not match the active transfer".to_string());
        }
        self.cleanup_prepared();
        Ok(CoordinatorResponse::Cancelled { transfer_id })
    }
}
