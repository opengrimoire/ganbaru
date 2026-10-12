//! Coordinator snapshot preparation, ownership grant, and receiver acknowledgement.

use super::{CompletedActivation, CoordinatorResponse, CoordinatorState, PreparedTransfer};
use crate::vault::handoff::pairing::{
    PendingAcknowledgement, StoredOutgoingTransfer, random_token,
};
use crate::vault::handoff::protocol::{BundleMetadata, BundlePurpose, PROTOCOL_VERSION};
use crate::vault::handoff::{current_compatibility, sha256_file};
use crate::vault::ownership::VaultOwnershipManager;
use crate::vault::quiescence::{
    SnapshotQuiescence, SourceQuiescence, abort_source_preparation, begin_snapshot_quiescence,
    begin_source_quiescence,
};
use std::fs;
use std::time::Instant;
use tauri::{Manager, Runtime};

impl<R: Runtime> CoordinatorState<R> {
    pub(super) async fn prepare(
        &mut self,
        vault_id: String,
        device_id: String,
        generation: u64,
        purpose: BundlePurpose,
    ) -> Result<CoordinatorResponse, String> {
        if let Some(prepared) = self.prepared.as_ref() {
            if prepared.metadata.vault_id == vault_id
                && prepared.metadata.device_id == device_id
                && prepared.purpose == purpose
            {
                return Ok(CoordinatorResponse::Prepared {
                    metadata: prepared.metadata.clone(),
                    purpose,
                });
            }
            let status = self
                .app
                .state::<VaultOwnershipManager>()
                .status(&vault_id)?;
            return Ok(CoordinatorResponse::BundlePending {
                owner_device_id: status.owner_device_id,
                generation: status.generation,
            });
        }
        if crate::vault::active_vault_id(&self.app)? != vault_id {
            return Err("requested vault is not active on the coordinator".to_string());
        }
        let ownership = self.app.state::<VaultOwnershipManager>();
        let status = ownership.status(&vault_id)?;
        if generation > status.generation {
            return Err("requesting replica is ahead of the coordinator".to_string());
        }
        if !status.can_write {
            if purpose == BundlePurpose::Refresh
                && self.relay_refresh_generation == Some(status.generation)
                && self.waiting_refresh_targets.remove(&device_id)
            {
                // The coordinator has just activated a fresh owner snapshot and can
                // relay that immutable copy without becoming the writer.
            } else {
                if self.pairing.linked_peer(&status.owner_device_id)?.is_none() {
                    return Err(
                        "the current vault owner is not linked to this coordinator".to_string()
                    );
                }
                self.ensure_owner_is_reachable(&status.owner_device_id, purpose)?;
                if purpose == BundlePurpose::Refresh {
                    self.waiting_refresh_targets.insert(device_id.clone());
                }
                self.pairing.request_upload(purpose)?;
                return Ok(CoordinatorResponse::BundlePending {
                    owner_device_id: status.owner_device_id,
                    generation: status.generation,
                });
            }
        }
        if status.can_write {
            let pool = crate::db::connect_sqlite(
                self.app.clone(),
                format!("sqlite:{}", crate::vault::APP_SQLITE_FILE),
            )
            .await?;
            crate::distractions::linked::drain_local_spool(
                &self.app,
                &pool,
                &vault_id,
                &status.device_id,
            )
            .await?;
            drop(pool);
        }
        let transfer_id = random_token("transfer")?;
        let next_generation = match purpose {
            BundlePurpose::Ownership => status
                .generation
                .checked_add(1)
                .ok_or_else(|| "vault ownership generation is exhausted".to_string())?,
            BundlePurpose::Refresh => status.generation,
        };
        let (mut quiescence, snapshot_quiescence): (
            Option<SourceQuiescence>,
            Option<SnapshotQuiescence>,
        ) = match purpose {
            BundlePurpose::Ownership => (
                Some(
                    begin_source_quiescence(
                        &self.app,
                        status.generation,
                        transfer_id.clone(),
                        device_id.clone(),
                    )
                    .await?,
                ),
                None,
            ),
            BundlePurpose::Refresh => (None, Some(begin_snapshot_quiescence(&self.app).await?)),
        };
        let (database_snapshot, archive_path) =
            match self.pairing.outgoing_snapshot_paths(&transfer_id) {
                Ok(paths) => paths,
                Err(error) => {
                    return Err(abort_source_preparation(quiescence, error).await);
                }
            };
        let vault_root = match crate::vault::active_vault_path(&self.app) {
            Ok(path) => path,
            Err(error) => {
                return Err(abort_source_preparation(quiescence, error).await);
            }
        };
        let snapshot_result = crate::vault::backup::create_handoff_archive(
            &vault_root,
            &database_snapshot,
            &archive_path,
        )
        .await;
        drop(snapshot_quiescence);
        let _ = fs::remove_file(&database_snapshot);
        if let Err(error) = snapshot_result {
            let _ = fs::remove_file(&archive_path);
            return Err(abort_source_preparation(quiescence, error).await);
        }
        let metadata_result: Result<BundleMetadata, String> = (|| {
            Ok(BundleMetadata {
                protocol_version: PROTOCOL_VERSION,
                compatibility: current_compatibility(&self.app),
                vault_id,
                device_id,
                transfer_id,
                generation: next_generation,
                archive_bytes: archive_path
                    .metadata()
                    .map_err(|error| format!("inspect handoff archive: {error}"))?
                    .len(),
                archive_sha256: sha256_file(&archive_path)?,
            })
        })();
        let metadata = match metadata_result {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = fs::remove_file(&archive_path);
                return Err(abort_source_preparation(quiescence, error).await);
            }
        };
        if let Err(error) = self
            .pairing
            .register_outgoing_bundle(metadata.clone(), archive_path.clone())
        {
            let _ = fs::remove_file(&archive_path);
            return Err(abort_source_preparation(quiescence, error).await);
        }
        if let Err(error) = self
            .pairing
            .store_outgoing_transfer(StoredOutgoingTransfer {
                metadata: metadata.clone(),
                purpose,
                committed: false,
            })
        {
            let _ = self
                .pairing
                .unregister_outgoing_bundle(&metadata.transfer_id);
            let _ = fs::remove_file(&archive_path);
            return Err(abort_source_preparation(quiescence, error).await);
        }
        if let Some(quiescence) = quiescence.as_mut() {
            quiescence.retain_for_retry();
        }
        self.prepared = Some(PreparedTransfer {
            metadata: metadata.clone(),
            purpose,
            archive_path,
            quiescence,
            committed: false,
        });
        Ok(CoordinatorResponse::Prepared { metadata, purpose })
    }

    pub(super) fn commit_ownership(
        &mut self,
        metadata: BundleMetadata,
    ) -> Result<CoordinatorResponse, String> {
        let prepared = self
            .prepared
            .as_mut()
            .ok_or_else(|| "ownership transfer is no longer available".to_string())?;
        if prepared.purpose != BundlePurpose::Ownership || prepared.metadata != metadata {
            return Err("staged ownership transfer does not match the coordinator".to_string());
        }
        let generation = self
            .app
            .state::<VaultOwnershipManager>()
            .commit_outgoing(&metadata.vault_id, &metadata.transfer_id)?;
        if let Some(quiescence) = prepared.quiescence.take() {
            quiescence.retain_outgoing();
        }
        self.pairing
            .mark_outgoing_committed(&metadata.transfer_id)?;
        prepared.committed = true;
        Ok(CoordinatorResponse::OwnershipGrant {
            vault_id: metadata.vault_id,
            transfer_id: metadata.transfer_id,
            owner_device_id: metadata.device_id,
            generation,
        })
    }

    pub(super) fn activated(
        &mut self,
        vault_id: String,
        device_id: String,
        transfer_id: String,
        generation: u64,
        purpose: BundlePurpose,
    ) -> Result<CoordinatorResponse, String> {
        let completed = CompletedActivation {
            vault_id,
            device_id,
            transfer_id: transfer_id.clone(),
            generation,
            purpose,
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
            || prepared.metadata.generation != completed.generation
            || prepared.purpose != completed.purpose
            || (completed.purpose == BundlePurpose::Ownership && !prepared.committed)
        {
            return Err("activation does not match the prepared transfer".to_string());
        }
        if completed.purpose == BundlePurpose::Ownership {
            self.app
                .state::<VaultOwnershipManager>()
                .finish_outgoing_acknowledgement(
                    &completed.vault_id,
                    &completed.transfer_id,
                    completed.generation,
                )?;
            self.last_owner_poll = Some((completed.device_id.clone(), Instant::now()));
        }
        self.pairing
            .complete_outgoing_activation(PendingAcknowledgement {
                vault_id: completed.vault_id.clone(),
                device_id: completed.device_id.clone(),
                transfer_id: completed.transfer_id.clone(),
                generation: completed.generation,
                purpose: completed.purpose,
            })?;
        self.cleanup_prepared();
        self.completed = Some(completed);
        Ok(CoordinatorResponse::ActivationAcknowledged { transfer_id })
    }

    pub(super) async fn cancel(
        &mut self,
        transfer_id: String,
    ) -> Result<CoordinatorResponse, String> {
        let prepared = self
            .prepared
            .as_ref()
            .ok_or_else(|| "transfer is no longer active".to_string())?;
        if prepared.metadata.transfer_id != transfer_id {
            return Err("cancelled transfer does not match the active transfer".to_string());
        }
        if prepared.committed {
            return Err("committed ownership cannot be cancelled".to_string());
        }
        if let Some(quiescence) = self
            .prepared
            .as_mut()
            .and_then(|prepared| prepared.quiescence.take())
        {
            quiescence.abort().await?;
        }
        self.cleanup_prepared();
        Ok(CoordinatorResponse::Cancelled { transfer_id })
    }
}
