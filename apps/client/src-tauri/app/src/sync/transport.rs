//! The sync transport of a linked device: requests travel to the paired coordinator over the
//! pinned handoff connection.

use crate::vault::handoff::pairing::PairingManager;
use crate::vault::handoff::protocol::ControlMessage;
use ganbaru_sync_replica::client::SyncTransport;

/// Sends requests to the coordinator this device is paired with.
#[derive(Clone)]
pub(crate) struct PairedTransport(pub PairingManager);

impl SyncTransport for PairedTransport {
    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        crate::vault::handoff::transport::sync_request(&self.0, message).await
    }
}

/// Sends requests to one member listener, pinned to the certificate fingerprint its enrollment
/// recorded in the selected log.
#[derive(Clone)]
pub(crate) struct MemberTransport {
    pub manager: PairingManager,
    pub endpoints: Vec<String>,
    pub fingerprint: String,
}

impl SyncTransport for MemberTransport {
    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        crate::vault::handoff::transport::member_sync_request(
            &self.manager,
            &self.endpoints,
            &self.fingerprint,
            message,
        )
        .await
    }
}
