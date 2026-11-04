//! The sync transports of a linked device: requests travel to the paired coordinator or to one
//! member listener through the dialer the host provides.

use crate::client::SyncTransport;
use ganbaru_handoff::pairing::PairingManager;
use ganbaru_handoff::protocol::ControlMessage;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A boxed request future a dialer returns.
pub type DialFuture<'a> = Pin<Box<dyn Future<Output = Result<ControlMessage, String>> + Send + 'a>>;

/// How sync requests leave this device: over the pinned connection to the paired coordinator,
/// or to a member listener pinned to the certificate fingerprint its enrollment recorded.
pub trait SyncDialer: Send + Sync + 'static {
    /// Sends `message` to the coordinator `manager` is paired with.
    fn coordinator<'a>(
        &'a self,
        manager: &'a PairingManager,
        message: ControlMessage,
    ) -> DialFuture<'a>;

    /// Sends `message` to the member listener at `endpoints` whose certificate has
    /// `fingerprint`.
    fn member<'a>(
        &'a self,
        manager: &'a PairingManager,
        endpoints: &'a [String],
        fingerprint: &'a str,
        message: ControlMessage,
    ) -> DialFuture<'a>;
}

/// Sends requests to the coordinator this device is paired with.
#[derive(Clone)]
pub struct PairedTransport {
    pub manager: PairingManager,
    pub dialer: Arc<dyn SyncDialer>,
}

impl SyncTransport for PairedTransport {
    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        self.dialer.coordinator(&self.manager, message).await
    }
}

/// Sends requests to one member listener, pinned to the certificate fingerprint its enrollment
/// recorded in the selected log.
#[derive(Clone)]
pub struct MemberTransport {
    pub manager: PairingManager,
    /// The device the pinned fingerprint belongs to in the selected log.
    pub device_id: String,
    pub endpoints: Vec<String>,
    pub fingerprint: String,
    pub dialer: Arc<dyn SyncDialer>,
}

impl SyncTransport for MemberTransport {
    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        self.dialer
            .member(&self.manager, &self.endpoints, &self.fingerprint, message)
            .await
    }
}
