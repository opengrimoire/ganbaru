//! Desktop coordinator dispatch and shared single-transfer state.

mod outgoing;

use super::pairing::{LinkedPeer, PairingManager};
use super::protocol::{BundleMetadata, HandoffCompatibility};
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::time::{Duration, Instant};
use tauri::Runtime;

#[derive(Debug)]
pub(crate) enum CoordinatorOperation {
    Prepare {
        compatibility: HandoffCompatibility,
        vault_id: String,
        device_id: String,
    },
    Activated {
        vault_id: String,
        device_id: String,
        transfer_id: String,
    },
    Cancel {
        transfer_id: String,
    },
    /// Authenticated request from a linked device for the person's signing key of the vault
    /// it is linked for.
    ReleasePersonKey {
        vault_id: String,
        device_id: String,
    },
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CoordinatorResponse {
    Prepared {
        metadata: BundleMetadata,
    },
    ActivationAcknowledged {
        transfer_id: String,
    },
    Cancelled {
        transfer_id: String,
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
    archive_path: std::path::PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompletedActivation {
    vault_id: String,
    device_id: String,
    transfer_id: String,
}

/// Drops events that left the sliding window ending at `now`.
fn forget_expired(events: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while events
        .front()
        .is_some_and(|event| now.saturating_duration_since(*event) >= window)
    {
        events.pop_front();
    }
}

/// Span of the sliding window that bounds how often one linked device makes a vault snapshot.
const BUNDLE_PREPARATION_WINDOW: Duration = Duration::from_secs(60 * 60);
/// Most vault snapshots one linked device can start within [`BUNDLE_PREPARATION_WINDOW`]. Each
/// one pauses writers and builds a full archive. First copies and fresh copies stay far below it;
/// a device that keeps requesting and cancelling bundles does not.
const MAX_BUNDLE_PREPARATIONS_PER_PEER: usize = 30;
/// Error returned when one device requested too many snapshots; the frontend maps it to
/// localized guidance.
pub(crate) const BUNDLE_PREPARATION_LIMIT: &str =
    "this device requested the vault too often; wait before requesting another copy";

/// Recent vault snapshots started for each linked device, kept in memory for the per-device limit.
#[derive(Debug, Default)]
pub(crate) struct BundlePreparationWindows {
    peers: BTreeMap<String, VecDeque<Instant>>,
}

impl BundlePreparationWindows {
    /// Records a snapshot for the device, or refuses it while that device's window is full.
    pub(crate) fn admit(&mut self, device_id: &str, now: Instant) -> Result<(), String> {
        self.peers.retain(|_, events| {
            forget_expired(events, now, BUNDLE_PREPARATION_WINDOW);
            !events.is_empty()
        });
        let events = self.peers.entry(device_id.to_string()).or_default();
        if events.len() >= MAX_BUNDLE_PREPARATIONS_PER_PEER {
            return Err(BUNDLE_PREPARATION_LIMIT.to_string());
        }
        events.push_back(now);
        Ok(())
    }
}

struct CoordinatorState<R: Runtime> {
    app: tauri::AppHandle<R>,
    pairing: PairingManager,
    prepared: Option<PreparedTransfer>,
    completed: Option<CompletedActivation>,
    bundle_preparations: BundlePreparationWindows,
}

pub(crate) async fn run<R: Runtime>(
    app: tauri::AppHandle<R>,
    pairing: PairingManager,
    mut receiver: tokio::sync::mpsc::Receiver<CoordinatorRequest>,
) {
    let prepared = restore_prepared(&pairing).ok().flatten();
    let completed = pairing
        .completed_activation()
        .ok()
        .flatten()
        .map(|activation| CompletedActivation {
            vault_id: activation.vault_id,
            device_id: activation.device_id,
            transfer_id: activation.transfer_id,
        });
    let mut state = CoordinatorState {
        app,
        pairing,
        prepared,
        completed,
        bundle_preparations: BundlePreparationWindows::default(),
    };
    while let Some(request) = receiver.recv().await {
        if matches!(request.operation, CoordinatorOperation::Shutdown) {
            let _ = request.response.send(Ok(CoordinatorResponse::Cancelled {
                transfer_id: "shutdown".to_string(),
            }));
            break;
        }
        let response = state.handle(request.operation).await;
        let _ = request.response.send(response);
    }
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
            } => {
                super::protocol::ensure_compatible(
                    &super::current_compatibility(&self.app),
                    &compatibility,
                )?;
                self.prepare(vault_id, device_id).await
            }
            CoordinatorOperation::Activated {
                vault_id,
                device_id,
                transfer_id,
            } => self.activated(vault_id, device_id, transfer_id),
            CoordinatorOperation::Cancel { transfer_id } => self.cancel(transfer_id),
            CoordinatorOperation::ReleasePersonKey {
                vault_id,
                device_id,
            } => self.release_person_key(vault_id, device_id).await,
            CoordinatorOperation::Shutdown => unreachable!("shutdown is handled by the run loop"),
        }
    }

    /// Releases the person key only for the vault open here and only to a device linked for it.
    async fn release_person_key(
        &mut self,
        vault_id: String,
        device_id: String,
    ) -> Result<CoordinatorResponse, String> {
        require_active_vault(&crate::vault::active_vault_id(&self.app)?, &vault_id)?;
        require_linked_for(self.pairing.linked_peer(&device_id)?.as_ref(), &vault_id)?;
        crate::contacts::identity::release_key_for_coordinator(&self.app, &vault_id).await
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
        archive_path,
    }))
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

/// Refuses a request that names a vault other than the one open on the coordinator.
fn require_active_vault(active_vault_id: &str, requested_vault_id: &str) -> Result<(), String> {
    if active_vault_id == requested_vault_id {
        Ok(())
    } else {
        Err("requested vault is not active on the coordinator".to_string())
    }
}

/// Refuses a device that is not, or is no longer, linked for `vault_id`.
fn require_linked_for(peer: Option<&LinkedPeer>, vault_id: &str) -> Result<(), String> {
    match peer {
        Some(peer) if peer.vault_id == vault_id => Ok(()),
        Some(_) => Err("requesting device is linked for a different vault".to_string()),
        None => Err("requesting device is not linked to this coordinator".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::handoff::pairing::{StoredOutgoingTransfer, random_token};
    use crate::vault::handoff::protocol::PROTOCOL_VERSION;

    fn peer_linked_for(vault_id: &str) -> LinkedPeer {
        LinkedPeer {
            device_id: "phone".to_string(),
            device_label: "Phone".to_string(),
            device_kind: Default::default(),
            vault_id: vault_id.to_string(),
            certificate: String::new(),
            certificate_fingerprint: String::new(),
            sync_device_key: String::new(),
            sync_admin_key: None,
            linked_at_ms: 0,
        }
    }

    #[test]
    fn person_key_release_is_refused_for_a_vault_that_is_not_open_or_not_linked() {
        require_active_vault("vault-a", "vault-a").unwrap();
        require_linked_for(Some(&peer_linked_for("vault-a")), "vault-a").unwrap();

        // A peer linked for vault A asks for the key while vault B is open on the coordinator.
        let inactive = require_active_vault("vault-b", "vault-a").unwrap_err();
        assert!(inactive.contains("not active on the coordinator"));
        assert!(require_linked_for(Some(&peer_linked_for("vault-a")), "vault-b").is_err());
        assert!(require_linked_for(None, "vault-a").is_err());
    }

    #[test]
    fn bundle_preparations_are_limited_per_device_and_sliding_window() {
        let start = Instant::now();
        let mut windows = BundlePreparationWindows::default();
        for _ in 0..MAX_BUNDLE_PREPARATIONS_PER_PEER {
            windows.admit("phone-a", start).unwrap();
        }
        assert_eq!(
            windows.admit("phone-a", start).unwrap_err(),
            BUNDLE_PREPARATION_LIMIT
        );
        // Another linked device keeps its own budget.
        windows.admit("phone-b", start).unwrap();
        let almost = start + BUNDLE_PREPARATION_WINDOW - Duration::from_millis(1);
        assert!(windows.admit("phone-a", almost).is_err());
        windows
            .admit("phone-a", start + BUNDLE_PREPARATION_WINDOW)
            .unwrap();
        // Refused requests do not extend the window.
        assert_eq!(
            windows.peers["phone-a"].len(),
            1,
            "only the admitted snapshot after expiry is kept"
        );
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
            archive_bytes: archive_path.metadata().unwrap().len(),
            archive_sha256: super::super::sha256_file(&archive_path).unwrap(),
        };
        manager
            .store_outgoing_transfer(StoredOutgoingTransfer {
                metadata: metadata.clone(),
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
        assert_eq!(
            restarted.outgoing_bundle(transfer_id).unwrap().metadata,
            prepared.metadata
        );
        fs::remove_dir_all(root).unwrap();
    }
}
