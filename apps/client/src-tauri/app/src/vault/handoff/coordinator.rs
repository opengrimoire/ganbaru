//! Desktop coordinator dispatch and shared single-transfer state.

mod incoming;
mod outgoing;

use super::pairing::PairingManager;
use super::protocol::{
    BundleMetadata, BundlePurpose, ContactRequestOutcome, DistractionsSampleMessage,
    HandoffCompatibility,
};
use crate::vault::ownership::VaultOwnershipManager;
use crate::vault::quiescence::SourceQuiescence;
use std::collections::BTreeSet;
use std::fs;
use std::time::{Duration, Instant};
use tauri::{Manager, Runtime};

#[derive(Debug)]
pub(crate) enum CoordinatorOperation {
    Prepare {
        compatibility: HandoffCompatibility,
        vault_id: String,
        device_id: String,
        generation: u64,
        purpose: BundlePurpose,
    },
    CommitOwnership {
        metadata: BundleMetadata,
    },
    Activated {
        vault_id: String,
        device_id: String,
        transfer_id: String,
        generation: u64,
        purpose: BundlePurpose,
    },
    PollUpload {
        compatibility: HandoffCompatibility,
        vault_id: String,
        device_id: String,
        generation: u64,
    },
    RequestUpload {
        purpose: BundlePurpose,
    },
    DistractionsExchange {
        vault_id: String,
        device_id: String,
        samples: Vec<DistractionsSampleMessage>,
        acknowledged_peer_sample_ids: Vec<String>,
        owner_snapshot: Vec<DistractionsSampleMessage>,
    },
    Uploaded {
        metadata: BundleMetadata,
        source_device_id: String,
        purpose: BundlePurpose,
    },
    AuthorizeUpload {
        metadata: BundleMetadata,
        source_device_id: String,
        purpose: BundlePurpose,
    },
    CommitUploadedOwnership {
        metadata: BundleMetadata,
        source_device_id: String,
    },
    Cancel {
        transfer_id: String,
    },
    /// Unauthenticated contact request addressed to the person owning this vault.
    ContactRequest {
        recipient_card_nonce: String,
        requester_card: String,
        request_id: String,
        signature: String,
    },
    /// Unauthenticated status poll for a contact request sent earlier.
    ContactRequestStatus {
        request_id: String,
        requester_public_key: String,
        issued_at_ms: i64,
        signature: String,
    },
    /// Authenticated request from a linked device for the person's signing key.
    ReleasePersonKey,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CoordinatorResponse {
    Prepared {
        metadata: BundleMetadata,
        purpose: BundlePurpose,
    },
    BundlePending {
        owner_device_id: String,
        generation: u64,
    },
    OwnershipGrant {
        vault_id: String,
        transfer_id: String,
        owner_device_id: String,
        generation: u64,
    },
    ActivationAcknowledged {
        transfer_id: String,
    },
    Cancelled {
        transfer_id: String,
    },
    UploadStatus {
        generation: u64,
        requested_upload: Option<BundlePurpose>,
    },
    UploadRequested {
        purpose: BundlePurpose,
    },
    IncomingStaged {
        transfer_id: String,
    },
    UploadAuthorized {
        transfer_id: String,
        already_received: bool,
    },
    DistractionsAcknowledged {
        acknowledged_sample_ids: Vec<String>,
        peer_samples: Vec<DistractionsSampleMessage>,
        combined_samples: Vec<DistractionsSampleMessage>,
    },
    ContactRequestReceived {
        request_id: String,
    },
    ContactRequestState {
        state: ContactRequestOutcome,
        recipient_card: Option<String>,
    },
    PersonKeyReleased {
        public_key: String,
        private_key_pkcs8: String,
    },
    /// A domain rejection that reaches the peer as a protocol error with a stable code.
    Rejected {
        code: String,
        message: String,
        retryable: bool,
    },
}

pub(crate) struct CoordinatorRequest {
    pub operation: CoordinatorOperation,
    pub response: tokio::sync::oneshot::Sender<Result<CoordinatorResponse, String>>,
}

pub(crate) type CoordinatorSender = tokio::sync::mpsc::Sender<CoordinatorRequest>;

struct PreparedTransfer {
    metadata: BundleMetadata,
    purpose: BundlePurpose,
    archive_path: std::path::PathBuf,
    quiescence: Option<SourceQuiescence>,
    committed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompletedActivation {
    vault_id: String,
    device_id: String,
    transfer_id: String,
    generation: u64,
    purpose: BundlePurpose,
}

struct CoordinatorState<R: Runtime> {
    app: tauri::AppHandle<R>,
    pairing: PairingManager,
    prepared: Option<PreparedTransfer>,
    completed: Option<CompletedActivation>,
    last_owner_poll: Option<(String, Instant)>,
    waiting_refresh_targets: BTreeSet<String>,
    relay_refresh_generation: Option<u64>,
}

const PEER_RECONNECT_GAP: Duration = Duration::from_secs(75);
const OWNER_REACHABILITY_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn run<R: Runtime>(
    app: tauri::AppHandle<R>,
    pairing: PairingManager,
    mut receiver: tokio::sync::mpsc::Receiver<CoordinatorRequest>,
) {
    if pairing.requested_upload().ok().flatten().is_none() {
        if let Ok(status) = crate::vault::ownership::active_status(&app) {
            if !status.can_write
                && pairing
                    .linked_peer(&status.owner_device_id)
                    .ok()
                    .flatten()
                    .is_some()
            {
                let _ = pairing.request_upload(BundlePurpose::Refresh);
            }
        }
    }
    let prepared = restore_prepared(&pairing).ok().flatten();
    let completed = pairing
        .completed_activation()
        .ok()
        .flatten()
        .map(|activation| CompletedActivation {
            vault_id: activation.vault_id,
            device_id: activation.device_id,
            transfer_id: activation.transfer_id,
            generation: activation.generation,
            purpose: activation.purpose,
        });
    let mut state = CoordinatorState {
        app,
        pairing,
        prepared,
        completed,
        last_owner_poll: None,
        waiting_refresh_targets: BTreeSet::new(),
        relay_refresh_generation: None,
    };
    while let Some(request) = receiver.recv().await {
        if matches!(request.operation, CoordinatorOperation::Shutdown) {
            state.shutdown();
            let _ = request.response.send(Ok(CoordinatorResponse::Cancelled {
                transfer_id: "shutdown".to_string(),
            }));
            break;
        }
        let response = state.handle(request.operation).await;
        let _ = request.response.send(response);
    }
    state.shutdown();
}

impl<R: Runtime> CoordinatorState<R> {
    async fn handle(
        &mut self,
        operation: CoordinatorOperation,
    ) -> Result<CoordinatorResponse, String> {
        match operation {
            CoordinatorOperation::Prepare {
                compatibility,
                vault_id,
                device_id,
                generation,
                purpose,
            } => {
                super::protocol::ensure_compatible(
                    &super::current_compatibility(&self.app),
                    &compatibility,
                )?;
                self.prepare(vault_id, device_id, generation, purpose).await
            }
            CoordinatorOperation::CommitOwnership { metadata } => self.commit_ownership(metadata),
            CoordinatorOperation::Activated {
                vault_id,
                device_id,
                transfer_id,
                generation,
                purpose,
            } => self.activated(vault_id, device_id, transfer_id, generation, purpose),
            CoordinatorOperation::PollUpload {
                compatibility,
                vault_id,
                device_id,
                generation,
            } => {
                super::protocol::ensure_compatible(
                    &super::current_compatibility(&self.app),
                    &compatibility,
                )?;
                self.poll_upload(vault_id, device_id, generation)
            }
            CoordinatorOperation::RequestUpload { purpose } => self.request_upload(purpose),
            CoordinatorOperation::DistractionsExchange {
                vault_id,
                device_id,
                samples,
                acknowledged_peer_sample_ids,
                owner_snapshot,
            } => {
                self.distractions_exchange(
                    vault_id,
                    device_id,
                    samples,
                    acknowledged_peer_sample_ids,
                    owner_snapshot,
                )
                .await
            }
            CoordinatorOperation::Uploaded {
                metadata,
                source_device_id,
                purpose,
            } => {
                let generation = metadata.generation;
                let response = self.uploaded(metadata, source_device_id, purpose).await;
                if response.is_ok() && purpose == BundlePurpose::Refresh {
                    self.relay_refresh_generation = Some(generation);
                }
                response
            }
            CoordinatorOperation::AuthorizeUpload {
                metadata,
                source_device_id,
                purpose,
            } => self.authorize_upload(&metadata, &source_device_id, purpose),
            CoordinatorOperation::CommitUploadedOwnership {
                metadata,
                source_device_id,
            } => {
                self.commit_uploaded_ownership(metadata, source_device_id)
                    .await
            }
            CoordinatorOperation::Cancel { transfer_id } => self.cancel(transfer_id).await,
            CoordinatorOperation::ContactRequest {
                recipient_card_nonce,
                requester_card,
                request_id,
                signature,
            } => {
                crate::contacts::requests::receive_request_for_coordinator(
                    &self.app,
                    recipient_card_nonce,
                    requester_card,
                    request_id,
                    signature,
                )
                .await
            }
            CoordinatorOperation::ContactRequestStatus {
                request_id,
                requester_public_key,
                issued_at_ms,
                signature,
            } => {
                crate::contacts::requests::request_state_for_coordinator(
                    &self.app,
                    request_id,
                    requester_public_key,
                    issued_at_ms,
                    signature,
                )
                .await
            }
            CoordinatorOperation::ReleasePersonKey => {
                crate::contacts::identity::release_key_for_coordinator(&self.app).await
            }
            CoordinatorOperation::Shutdown => unreachable!("shutdown is handled by the run loop"),
        }
    }

    fn poll_upload(
        &mut self,
        vault_id: String,
        device_id: String,
        generation: u64,
    ) -> Result<CoordinatorResponse, String> {
        if crate::vault::active_vault_id(&self.app)? != vault_id {
            return Err("requested vault is not active on the coordinator".to_string());
        }
        let status = self
            .app
            .state::<VaultOwnershipManager>()
            .status(&vault_id)?;
        if self.pairing.linked_peer(&device_id)?.is_none() {
            return Err("polling device is not linked to this coordinator".to_string());
        }
        if status.owner_device_id != device_id {
            if generation > status.generation {
                return Err("polling replica is ahead of the coordinator".to_string());
            }
            return Ok(CoordinatorResponse::UploadStatus {
                generation: status.generation,
                requested_upload: None,
            });
        }
        if status.generation != generation {
            return Err("polling owner generation does not match the coordinator".to_string());
        }
        let reconnected = self
            .last_owner_poll
            .as_ref()
            .filter(|(owner_device_id, _)| owner_device_id == &device_id)
            .is_none_or(|(_, activity)| activity.elapsed() >= PEER_RECONNECT_GAP);
        self.last_owner_poll = Some((device_id, Instant::now()));
        if reconnected
            && self.pairing.requested_upload()?.is_none()
            && self.pairing.incoming_transfer()?.is_none()
        {
            self.pairing.request_upload(BundlePurpose::Refresh)?;
        }
        Ok(CoordinatorResponse::UploadStatus {
            generation,
            requested_upload: self.pairing.requested_upload()?,
        })
    }

    fn request_upload(&self, purpose: BundlePurpose) -> Result<CoordinatorResponse, String> {
        let status = crate::vault::ownership::active_status(&self.app)?;
        if status.can_write || self.pairing.linked_peer(&status.owner_device_id)?.is_none() {
            return Err("a linked device is not the current vault owner".to_string());
        }
        self.ensure_owner_is_reachable(&status.owner_device_id, purpose)?;
        if let Some(incoming) = self.pairing.incoming_transfer()? {
            if incoming.purpose != purpose {
                return Err("another uploaded vault transfer requires recovery".to_string());
            }
        }
        self.pairing.request_upload(purpose)?;
        Ok(CoordinatorResponse::UploadRequested { purpose })
    }

    fn ensure_owner_is_reachable(
        &self,
        owner_device_id: &str,
        purpose: BundlePurpose,
    ) -> Result<(), String> {
        if owner_poll_is_fresh(self.last_owner_poll.as_ref(), owner_device_id) {
            Ok(())
        } else {
            self.pairing.cancel_requested_upload(purpose)?;
            Err("the current vault owner is unreachable".to_string())
        }
    }

    async fn distractions_exchange(
        &mut self,
        vault_id: String,
        device_id: String,
        samples: Vec<DistractionsSampleMessage>,
        acknowledged_peer_sample_ids: Vec<String>,
        owner_snapshot: Vec<DistractionsSampleMessage>,
    ) -> Result<CoordinatorResponse, String> {
        let status = self
            .app
            .state::<VaultOwnershipManager>()
            .status(&vault_id)?;
        if status.can_write {
            if !owner_snapshot.is_empty() || !acknowledged_peer_sample_ids.is_empty() {
                return Err(
                    "the non-owner cannot publish authoritative Distractions state".to_string(),
                );
            }
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
            let acknowledged_sample_ids =
                crate::distractions::linked::import_linked_samples(&pool, &samples).await?;
            let combined_samples =
                crate::distractions::linked::aggregate_owner_samples(&pool).await?;
            return Ok(CoordinatorResponse::DistractionsAcknowledged {
                acknowledged_sample_ids,
                peer_samples: Vec::new(),
                combined_samples,
            });
        }
        if status.owner_device_id != device_id || !samples.is_empty() {
            return Err("Distractions exchange does not match the current vault owner".to_string());
        }
        if !owner_snapshot.is_empty() {
            crate::distractions::linked::apply_owner_snapshot(
                &self.app,
                &vault_id,
                &status.device_id,
                &acknowledged_peer_sample_ids,
                &owner_snapshot,
            )
            .await?;
        } else {
            crate::distractions::linked::acknowledge(
                &self.app,
                &vault_id,
                &status.device_id,
                &acknowledged_peer_sample_ids,
            )
            .await?;
        }
        let peer_samples =
            crate::distractions::linked::pending(&self.app, &vault_id, &status.device_id).await?;
        let combined_samples = if peer_samples.is_empty() {
            crate::distractions::linked::accepted(&self.app, &vault_id).await?
        } else {
            Vec::new()
        };
        Ok(CoordinatorResponse::DistractionsAcknowledged {
            acknowledged_sample_ids: Vec::new(),
            peer_samples,
            combined_samples,
        })
    }

    fn cleanup_prepared(&mut self) {
        if let Some(prepared) = self.prepared.take() {
            let _ = self
                .pairing
                .unregister_outgoing_bundle(&prepared.metadata.transfer_id);
            let _ = self
                .pairing
                .clear_outgoing_transfer(&prepared.metadata.transfer_id);
            let _ = fs::remove_file(prepared.archive_path);
        }
    }

    fn shutdown(&mut self) {
        if let Some(quiescence) = self
            .prepared
            .as_mut()
            .and_then(|prepared| prepared.quiescence.take())
        {
            quiescence.retain_outgoing();
        }
    }

    fn reload_shell(&self) -> Result<(), String> {
        let window = self
            .app
            .get_webview_window("main")
            .ok_or_else(|| "main application window is unavailable".to_string())?;
        window
            .eval("window.location.reload()")
            .map_err(|error| format!("reload application after vault activation: {error}"))
    }
}

fn restore_prepared(pairing: &PairingManager) -> Result<Option<PreparedTransfer>, String> {
    let Some(transfer) = pairing.outgoing_transfer()? else {
        return Ok(None);
    };
    let (_, archive_path) = pairing.outgoing_snapshot_paths(&transfer.metadata.transfer_id)?;
    super::protocol::validate_staging_file(&archive_path, &transfer.metadata)?;
    pairing.register_outgoing_bundle(transfer.metadata.clone(), archive_path.clone())?;
    Ok(Some(PreparedTransfer {
        metadata: transfer.metadata,
        purpose: transfer.purpose,
        archive_path,
        quiescence: None,
        committed: transfer.committed,
    }))
}

fn owner_poll_is_fresh(last_owner_poll: Option<&(String, Instant)>, owner_device_id: &str) -> bool {
    last_owner_poll.is_some_and(|(polled_device_id, activity)| {
        polled_device_id == owner_device_id && activity.elapsed() <= OWNER_REACHABILITY_TIMEOUT
    })
}

pub(crate) async fn request(
    sender: &CoordinatorSender,
    operation: CoordinatorOperation,
) -> Result<CoordinatorResponse, String> {
    let (response, receiver) = tokio::sync::oneshot::channel();
    sender
        .send(CoordinatorRequest {
            operation,
            response,
        })
        .await
        .map_err(|_| "vault handoff coordinator is stopping".to_string())?;
    receiver
        .await
        .map_err(|_| "vault handoff coordinator did not return a response".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::handoff::pairing::{StoredOutgoingTransfer, random_token};
    use crate::vault::handoff::protocol::PROTOCOL_VERSION;

    #[test]
    fn owner_reachability_requires_a_recent_poll_from_the_current_owner() {
        let now = Instant::now();
        let current = ("phone-owner".to_string(), now);
        let stale = (
            "phone-owner".to_string(),
            now - OWNER_REACHABILITY_TIMEOUT - Duration::from_millis(1),
        );
        let other = ("other-phone".to_string(), now);

        assert!(owner_poll_is_fresh(Some(&current), "phone-owner"));
        assert!(!owner_poll_is_fresh(Some(&stale), "phone-owner"));
        assert!(!owner_poll_is_fresh(Some(&other), "phone-owner"));
        assert!(!owner_poll_is_fresh(None, "phone-owner"));
    }

    #[test]
    fn prepared_download_is_restored_from_private_state() {
        let root = std::env::temp_dir().join(format!(
            "ganbaru-coordinator-restart-{}",
            random_token("test").unwrap()
        ));
        fs::create_dir_all(&root).unwrap();
        let manager = PairingManager::default();
        manager
            .initialize(root.clone(), "desktop".to_string())
            .unwrap();
        let transfer_id = "restart-transfer";
        let (_, archive_path) = manager.outgoing_snapshot_paths(transfer_id).unwrap();
        fs::write(&archive_path, b"durable prepared archive").unwrap();
        let metadata = BundleMetadata {
            protocol_version: PROTOCOL_VERSION,
            compatibility: crate::vault::handoff::protocol::test_compatibility(),
            vault_id: "vault".to_string(),
            device_id: "phone".to_string(),
            transfer_id: transfer_id.to_string(),
            generation: 1,
            archive_bytes: archive_path.metadata().unwrap().len(),
            archive_sha256: super::super::sha256_file(&archive_path).unwrap(),
        };
        manager
            .store_outgoing_transfer(StoredOutgoingTransfer {
                metadata: metadata.clone(),
                purpose: BundlePurpose::Ownership,
                committed: false,
            })
            .unwrap();
        drop(manager);

        let restarted = PairingManager::default();
        restarted
            .initialize(root.clone(), "desktop".to_string())
            .unwrap();
        let prepared = restore_prepared(&restarted).unwrap().unwrap();
        assert_eq!(prepared.metadata, metadata);
        assert_eq!(prepared.archive_path, archive_path);
        assert!(!prepared.committed);
        assert_eq!(
            restarted.outgoing_bundle(transfer_id).unwrap().metadata,
            prepared.metadata
        );
        fs::remove_dir_all(root).unwrap();
    }
}
