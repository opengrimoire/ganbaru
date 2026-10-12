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
