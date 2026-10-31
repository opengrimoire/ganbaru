//! The public listener of an always-on anchor server.
//!
//! A server is reached over the internet, so this listener has no LAN gate. It admits the
//! certificates of the devices the selected log enrolled, as a member listener does, and it also
//! completes a handshake without a client certificate, which is how an admin device proves a
//! setup secret. Such a connection stays on the unauthenticated budget and reaches only the
//! caller's setup route with a request [`is_setup_request`] accepts.

use super::limits::{ConnectionLimits, PendingPermit};
use super::member::{HandshakeAdmission, MemberAdmissionSource, MemberConfigCache, refuse};
use super::{
    COORDINATOR_BUSY_CODE, CoordinatorListener, Peer, Routes, TLS_HANDSHAKE_TIMEOUT,
    accept_connections, send_error, timeout_control,
};
use crate::hub::SyncHub;
use ganbaru_handoff::pairing::{PairingManager, certificate_fingerprint};
use ganbaru_handoff::protocol::{self, ControlMessage, read_control_limited, write_control};
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use tokio::net::TcpStream;
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

/// Serves `listener` as a server listener until shutdown.
pub async fn serve_server<R: Routes>(
    listener: CoordinatorListener,
    limits: ConnectionLimits,
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
    source: IpAddr,
}

async fn handle_connection<R: Routes>(
    socket: TcpStream,
    connection: &ServerConnection<'_, R>,
    permit: PendingPermit,
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
        TLS_HANDSHAKE_TIMEOUT,
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
        return serve_setup(&mut stream, connection).await;
    };
    let fingerprint = certificate_fingerprint(certificate.as_ref());
    // The handshake may have used an admission read a few seconds ago; an authenticated peer
    // is checked against the log as it is now.
    let Some(admission) = (connection.services.admission)().await? else {
        return Err("sync does not run on this server yet".to_string());
    };
    let Some(_authenticated) = permit.authenticate() else {
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
    };
    connection
        .services
        .routes
        .serve(stream, peer, request)
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
