//! The public listener of an always-on anchor server.
//!
//! A server is reached over the internet, so this listener has no LAN gate. It admits the
//! certificates of the devices the selected log enrolled, as a member listener does, and it also
//! completes a handshake without a client certificate, which is how an admin device proves a
//! setup secret. Such a connection stays on the unauthenticated budget and reaches only the
//! caller's setup route with a request [`is_setup_request`] accepts.
//!
//! Every connection runs under a [`PreAuthGate`]: it is closed when it neither authenticates
//! nor proves a setup secret within [`PreAuthPolicy::deadline`], and a proven setup moves to its
//! own budget so an upload does not hold a pending slot.

use super::limits::{AuthenticatedPermit, ConnectionLimits, PendingPermit, SetupPermit};
use super::member::{HandshakeAdmission, MemberAdmissionSource, MemberConfigCache, refuse};
use super::{
    COORDINATOR_BUSY_CODE, CoordinatorListener, Peer, Routes, accept_connections, send_error,
    timeout_control,
};
use crate::hub::SyncHub;
use ganbaru_handoff::pairing::{PairingManager, certificate_fingerprint};
use ganbaru_handoff::protocol::{self, ControlMessage, read_control_limited, write_control};
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::Instant;
use tokio_rustls::TlsAcceptor;

/// Error code for a request that needs an enrolled device's certificate.
pub const AUTHENTICATION_REQUIRED_CODE: &str = "authentication_required";

/// What a server listener serves and whom it admits.
#[derive(Clone)]
pub struct ServerServices<R> {
    /// The hub that answers admitted sync requests.
    pub hub: SyncHub,
    /// Admission from the selected log, `None` until the server activated a vault.
    pub admission: MemberAdmissionSource,
    /// Setup and snapshot copy requests, answered before the hub.
    pub routes: R,
}

/// Whether `request` opens the setup conversation, the only thing a connection without a client
/// certificate can do.
pub fn is_setup_request(request: &ControlMessage) -> bool {
    matches!(request, ControlMessage::AnchorHello { .. })
}

/// How long a connection may take before it authenticates.
#[derive(Clone, Copy, Debug)]
pub struct PreAuthPolicy {
    /// Longest the TLS handshake may take.
    pub tls_timeout: Duration,
    /// Longest from accept until the peer authenticates or proves a setup secret.
    pub deadline: Duration,
}

/// The budget slot a connection holds before it authenticates.
enum GateSlot {
    /// Counted against the pending budget.
    Pending(PendingPermit),
    /// Moved to a setup slot after a proven setup secret; the permit is held for its release.
    Setup { _permit: SetupPermit },
    /// Released, or handed to the authenticated budget.
    Released,
}

/// The pre-authentication deadline and budget slot of one server connection.
///
/// The deadline runs from accept through TLS, the first frame, and a setup proof. It stops once
/// the peer authenticates with an enrolled certificate or proves a setup secret, so neither sync
/// nor an upload after a proof is cut short.
pub struct PreAuthGate {
    slot: Mutex<GateSlot>,
    deadline: Instant,
    stopped: AtomicBool,
}

impl PreAuthGate {
    fn new(permit: PendingPermit, policy: PreAuthPolicy) -> Self {
        Self {
            slot: Mutex::new(GateSlot::Pending(permit)),
            deadline: Instant::now() + policy.deadline,
            stopped: AtomicBool::new(false),
        }
    }

    fn take_pending(&self) -> Option<PendingPermit> {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        match std::mem::replace(&mut *slot, GateSlot::Released) {
            GateSlot::Pending(permit) => Some(permit),
            other => {
                *slot = other;
                None
            }
        }
    }

    /// Moves an enrolled peer to the authenticated budget and stops the deadline. Returns
    /// `None` when the authenticated budget is full.
    fn authenticate(&self) -> Option<AuthenticatedPermit> {
        let authenticated = self.take_pending()?.authenticate()?;
        self.stopped.store(true, Ordering::Release);
        Some(authenticated)
    }

    /// Moves a peer that proved a setup secret to a setup slot, which frees its pending slot,
    /// and stops the deadline. Returns false when the setup budget is full; the connection then
    /// holds no slot and should be closed.
    pub fn setup_proven(&self) -> bool {
        let Some(setup) = self.take_pending().and_then(PendingPermit::setup) else {
            return false;
        };
        *self.slot.lock().unwrap_or_else(PoisonError::into_inner) =
            GateSlot::Setup { _permit: setup };
        self.stopped.store(true, Ordering::Release);
        true
    }

    /// Completes when the deadline passes before the peer authenticated or proved a setup
    /// secret, and never completes otherwise.
    async fn expired(&self) {
        tokio::time::sleep_until(self.deadline).await;
        if self.stopped.load(Ordering::Acquire) {
            std::future::pending::<()>().await;
        }
    }
}

/// Serves `listener` as a server listener until shutdown.
pub async fn serve_server<R: Routes>(
    listener: CoordinatorListener,
    limits: ConnectionLimits,
    policy: PreAuthPolicy,
    manager: PairingManager,
    services: ServerServices<R>,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    let configs = MemberConfigCache::default();
    let handshakes = HandshakeAdmission::default();
    accept_connections(
        || listener.accept(),
        shutdown,
        |socket, source: SocketAddr| {
            let Some(permit) = limits.admit(source.ip()) else {
                drop(socket);
                return;
            };
            let configs = configs.clone();
            let handshakes = handshakes.clone();
            let manager = manager.clone();
            let services = services.clone();
            let limits = limits.clone();
            tokio::spawn(async move {
                let connection = ServerConnection {
                    manager: &manager,
                    services: &services,
                    configs: &configs,
                    handshakes: &handshakes,
                    limits: &limits,
                    policy,
                    source: source.ip(),
                };
                if let Err(error) = handle_connection(socket, &connection, permit).await {
                    eprintln!("sync server listener: connection from {source} failed: {error}");
                }
            });
        },
    )
    .await;
}

/// What one accepted connection is served with.
struct ServerConnection<'a, R> {
    manager: &'a PairingManager,
    services: &'a ServerServices<R>,
    configs: &'a MemberConfigCache,
    handshakes: &'a HandshakeAdmission,
    limits: &'a ConnectionLimits,
    policy: PreAuthPolicy,
    source: IpAddr,
}

async fn handle_connection<R: Routes>(
    socket: TcpStream,
    connection: &ServerConnection<'_, R>,
    permit: PendingPermit,
) -> Result<(), String> {
    let gate = PreAuthGate::new(permit, connection.policy);
    tokio::select! {
        result = serve_connection(socket, connection, &gate) => result,
        () = gate.expired() => Err("the connection did not authenticate in time".to_string()),
    }
}

async fn serve_connection<R: Routes>(
    socket: TcpStream,
    connection: &ServerConnection<'_, R>,
    gate: &PreAuthGate,
) -> Result<(), String> {
    let fingerprints = connection
        .handshakes
        .current(&connection.services.admission)
        .await?
        .map(|admission| admission.admitted_fingerprints())
        .unwrap_or_else(BTreeSet::new);
    let config = connection
        .configs
        .current(connection.manager, fingerprints, false)
        .await?;
    let mut stream = tokio::time::timeout(
        connection.policy.tls_timeout,
        TlsAcceptor::from(config).accept(socket),
    )
    .await
    .map_err(|_| "TLS handshake timed out".to_string())?
    .map_err(|error| format!("accept TLS connection: {error}"))?;
    let certificate = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .cloned();
    let Some(certificate) = certificate else {
        return serve_setup(&mut stream, connection, gate).await;
    };
    let fingerprint = certificate_fingerprint(certificate.as_ref());
    // The handshake may have used an admission read a few seconds ago; an authenticated peer
    // is checked against the log as it is now.
    let Some(admission) = (connection.services.admission)().await? else {
        return Err("sync does not run on this server yet".to_string());
    };
    let Some(_authenticated) = gate.authenticate() else {
        return send_error(
            &mut stream,
            COORDINATOR_BUSY_CODE,
            "this server is handling too many connections; retry shortly",
            true,
        )
        .await;
    };
    let request = match timeout_control(read_control_limited(
        &mut stream,
        protocol::MAX_CONTROL_BYTES,
    ))
    .await
    {
        Ok(request) => request,
        Err(error) => {
            return send_error(&mut stream, "malformed_control", &error, false).await;
        }
    };
    if connection.services.routes.handles(&request) {
        let peer = Peer {
            manager: connection.manager,
            source: connection.source,
            certificate: Some(&certificate),
            limits: connection.limits,
            admission: Some(&admission),
            admission_source: Some(&connection.services.admission),
        };
        return connection
            .services
            .routes
            .serve(&mut stream, peer, request)
            .await;
    }
    if let Err(refusal) = admission.request(&fingerprint, &request) {
        return refuse(&mut stream, refusal).await;
    }
    let response = connection.services.hub.respond(request).await;
    timeout_control(write_control(&mut stream, &response)).await
}

/// Serves a connection without a client certificate. Its first frame stays within the
/// unauthenticated bound, and anything but a setup request is turned away.
async fn serve_setup<R: Routes>(
    stream: &mut super::ServerStream,
    connection: &ServerConnection<'_, R>,
    gate: &PreAuthGate,
) -> Result<(), String> {
    let request = match timeout_control(read_control_limited(
        stream,
        protocol::MAX_UNAUTHENTICATED_CONTROL_BYTES,
    ))
    .await
    {
        Ok(request) => request,
        Err(error) => return send_error(stream, "malformed_control", &error, false).await,
    };
    if !is_setup_request(&request) || !connection.services.routes.handles(&request) {
        return send_error(
            stream,
            AUTHENTICATION_REQUIRED_CODE,
            "this request needs an enrolled device",
            false,
        )
        .await;
    }
    let peer = Peer {
        manager: connection.manager,
        source: connection.source,
        certificate: None,
        limits: connection.limits,
        admission: None,
        admission_source: None,
    };
    connection
        .services
        .routes
        .serve_unauthenticated(stream, peer, request, gate)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_hello_opens_setup_without_a_certificate() {
        let hello = ControlMessage::AnchorHello {
            protocol_version: protocol::PROTOCOL_VERSION,
            compatibility: protocol::HandoffCompatibility {
                app_version: "1.0.0".to_string(),
                database_schema_sha256: "0".repeat(64),
            },
            client_nonce: String::new(),
        };
        assert!(is_setup_request(&hello));
        assert!(!is_setup_request(&ControlMessage::AnchorProof {
            proof: String::new(),
            vault_id: None,
        }));
        assert!(!is_setup_request(&ControlMessage::AnchorActivated {
            vault_id: "vault".to_string(),
        }));
    }
}
