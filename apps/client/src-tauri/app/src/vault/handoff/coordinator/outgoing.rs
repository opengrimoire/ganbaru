//! Coordinator snapshot preparation and receiver acknowledgement.

use super::{CoordinatorResponse, CoordinatorState};
use crate::vault::handoff::pairing::{PendingAcknowledgement, random_token};
use crate::vault::handoff::protocol::unix_time_ms;
use crate::vault::handoff::snapshot::{archive_active_vault, bundle_metadata};
use crate::vault::handoff::transport::BUNDLE_BUSY_CODE;
use crate::vault::quiescence::begin_snapshot_quiescence;
use std::fs;
use std::time::Instant;
use tauri::Runtime;

impl<R: Runtime> CoordinatorState<R> {
    /// Prepares a vault copy for one linked device, or answers with the copy already offered to
    /// it. While a copy for another device is offered, the request is refused as retryable; the
    /// pairing manager releases an offered copy once its lifetime passes.
    pub(super) async fn prepare(
        &mut self,
        vault_id: String,
        device_id: String,
    ) -> Result<CoordinatorResponse, String> {
        if let Some(offered) = self.pairing.active_outgoing_transfer(unix_time_ms())? {
            let metadata = offered.metadata;
            if metadata.vault_id == vault_id && metadata.device_id == device_id {
                return Ok(CoordinatorResponse::Prepared { metadata });
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
        let offered = self
            .pairing
            .begin_outgoing_transfer(metadata, unix_time_ms())?;
        Ok(CoordinatorResponse::Prepared {
            metadata: offered.metadata,
        })
    }

    /// Records the activation of the copy offered to `device_id`, which releases it. A repeated
    /// acknowledgement, or one for a copy already released, is answered so the receiver stops
    /// retrying; `device_id` is the authenticated device linked for `vault_id`.
    pub(super) fn activated(
        &mut self,
        vault_id: String,
        device_id: String,
        transfer_id: String,
    ) -> Result<CoordinatorResponse, String> {
        let completed = PendingAcknowledgement {
            vault_id,
            device_id,
            transfer_id: transfer_id.clone(),
        };
        if self.pairing.completed_activation()?.as_ref() == Some(&completed) {
            return Ok(CoordinatorResponse::ActivationAcknowledged { transfer_id });
        }
        match self.pairing.active_outgoing_transfer(unix_time_ms())? {
            Some(offered)
                if offered.metadata.vault_id == completed.vault_id
                    && offered.metadata.device_id == completed.device_id
                    && offered.metadata.transfer_id == completed.transfer_id =>
            {
                self.pairing.complete_outgoing_activation(completed)?;
            }
            Some(offered) if offered.metadata.device_id == completed.device_id => {
                return Err("activation does not match the prepared transfer".to_string());
            }
            _ => {}
        }
        Ok(CoordinatorResponse::ActivationAcknowledged { transfer_id })
    }

    /// Cancels the offered copy when it is the one `device_id` asked for; another linked device
    /// cannot cancel it.
    pub(super) fn cancel(
        &mut self,
        transfer_id: String,
        device_id: String,
    ) -> Result<CoordinatorResponse, String> {
        let offered = self
            .pairing
            .active_outgoing_transfer(unix_time_ms())?
            .ok_or_else(|| "transfer is no longer active".to_string())?;
        if offered.metadata.transfer_id != transfer_id || offered.metadata.device_id != device_id {
            return Err("cancelled transfer does not match the active transfer".to_string());
        }
        self.pairing.release_outgoing_transfer(&transfer_id)?;
        Ok(CoordinatorResponse::Cancelled { transfer_id })
    }
}
