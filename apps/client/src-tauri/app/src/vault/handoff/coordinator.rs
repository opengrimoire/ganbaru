//! Desktop coordinator dispatch. The pairing manager owns the one copy offered at a time.

mod outgoing;

use super::pairing::{LinkedPeer, PairingManager};
use super::protocol::{BundleMetadata, HandoffCompatibility};
use ganbaru_sync_replica::serve::bundles::BundlePreparationWindows;
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
    /// Cancels the transfer prepared for `device_id`, the authenticated device that asks.
    Cancel {
        transfer_id: String,
        device_id: String,
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

struct CoordinatorState<R: Runtime> {
    app: tauri::AppHandle<R>,
    pairing: PairingManager,
    bundle_preparations: BundlePreparationWindows,
}

pub(crate) async fn run<R: Runtime>(
    app: tauri::AppHandle<R>,
    pairing: PairingManager,
    mut receiver: tokio::sync::mpsc::Receiver<CoordinatorRequest>,
) {
    let mut state = CoordinatorState {
        app,
        pairing,
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
            CoordinatorOperation::Cancel {
                transfer_id,
                device_id,
            } => self.cancel(transfer_id, device_id),
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
}
