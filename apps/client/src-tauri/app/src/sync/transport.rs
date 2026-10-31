//! The app's sync dialer: requests travel to the paired coordinator over the pinned handoff
//! connection, and to member listeners pinned to their enrolled certificates.

use crate::vault::handoff::pairing::PairingManager;
use crate::vault::handoff::protocol::ControlMessage;
use ganbaru_sync_replica::service::transport::{DialFuture, SyncDialer};

/// Dials through the handoff transport of the app.
pub(crate) struct AppDialer;

impl SyncDialer for AppDialer {
    fn coordinator<'a>(
        &'a self,
        manager: &'a PairingManager,
        message: ControlMessage,
    ) -> DialFuture<'a> {
        Box::pin(crate::vault::handoff::transport::sync_request(
            manager, message,
        ))
    }

    fn member<'a>(
        &'a self,
        manager: &'a PairingManager,
        endpoints: &'a [String],
        fingerprint: &'a str,
        message: ControlMessage,
    ) -> DialFuture<'a> {
        Box::pin(crate::vault::handoff::transport::member_sync_request(
            manager,
            endpoints,
            fingerprint,
            message,
        ))
    }
}
