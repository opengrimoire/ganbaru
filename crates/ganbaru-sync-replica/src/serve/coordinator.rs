//! The desktop coordinator listener: TLS pinned to linked devices, quarantine and log admission,
//! and sync dispatch. Pairing, handoff, and contact requests go to the caller's routes.

use super::limits::{ConnectionLimits, PendingPermit};
use super::{
    COORDINATOR_BUSY_CODE, CoordinatorListener, LanAdmission, MEMBERSHIP_REVOKED_CODE,
    MemberAdmissionSource, Peer, Routes, TLS_HANDSHAKE_TIMEOUT, accept_connections,
    admitted_local_address, load_tls_identity, send_error, timeout_control,
};
use crate::hub::SyncHub;
use crate::peers::admission::{MemberAdmission, MemberRefusal};
use ganbaru_handoff::dial::LoopbackPolicy;
use ganbaru_handoff::pairing::{PairingManager, TlsIdentity, certificate_fingerprint};
use ganbaru_handoff::protocol::{self, ControlMessage, read_control_limited, write_control};
use ganbaru_handoff::tls::PinnedClientVerifier;
use rustls::pki_types::CertificateDer;
use std::collections::BTreeSet;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;

/// TLS server configuration built once per set of trusted device certificates, so connections
/// reuse it until enrollment or revocation changes the set.
#[derive(Clone, Default)]
struct ServerConfigCache(Arc<Mutex<Option<CachedServerConfig>>>);

struct CachedServerConfig {
    client_certificates: Vec<CertificateDer<'static>>,
    config: Arc<rustls::ServerConfig>,
}

impl ServerConfigCache {
    async fn current(&self, manager: &PairingManager) -> Result<Arc<rustls::ServerConfig>, String> {
        let client_certificates = manager.client_auth_certificates()?;
        if let Some(cached) = self.lock()?.as_ref()
            && cached.client_certificates == client_certificates
        {
            return Ok(cached.config.clone());
        }
        let (_, identity) = load_tls_identity(manager).await?;
        let config = Arc::new(server_config(identity, client_certificates.clone())?);
        *self.lock()? = Some(CachedServerConfig {
            client_certificates,
            config: config.clone(),
        });
        Ok(config)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Option<CachedServerConfig>>, String> {
        self.0
            .lock()
            .map_err(|_| "coordinator TLS configuration lock is unavailable".to_string())
    }
}

/// The device ids this installation quarantined in the active vault. The listener turns their
/// connections away right after the TLS handshake.
pub type QuarantinedDevices = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<BTreeSet<String>, String>> + Send>>
        + Send
        + Sync,
>;

/// What a coordinator listener serves itself, whom it admits, and the routes it hands the
/// remaining requests to.
#[derive(Clone)]
pub struct CoordinatorServices<R> {
    /// The hub that answers sync requests, if this coordinator serves sync.
    pub hub: Option<SyncHub>,
    /// Whether the network behind each local address may reach the listener at all.
    pub admission: LanAdmission,
    /// Devices whose connections are refused while they stay quarantined.
    pub quarantined: Option<QuarantinedDevices>,
    /// The selected log's admission: a linked device the log removed reaches no service, and
    /// sync requests need a device the log enrolled.
    pub log_admission: Option<MemberAdmissionSource>,
    /// Pairing, handoff, and contact requests.
    pub routes: R,
}

/// Serves `listener` until shutdown. `loopback` decides whether loopback peers are admitted.
pub async fn serve_coordinator<R: Routes>(
    listener: CoordinatorListener,
    limits: ConnectionLimits,
    loopback: LoopbackPolicy,
    manager: PairingManager,
    services: CoordinatorServices<R>,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    let configs = ServerConfigCache::default();
    accept_connections(
        || listener.accept(),
        shutdown,
        |socket, source: SocketAddr| {
            let Some(local) = admitted_local_address(&socket, source.ip(), loopback) else {
                // Refused before the pending budget so a host outside the local networks never
                // costs a slot, a task, or a read.
                drop(socket);
                return;
            };
            let Some(permit) = limits.admit(source.ip()) else {
                // Closing at once keeps a flood from holding descriptors or handshakes.
                drop(socket);
                return;
            };
            let configs = configs.clone();
            let connection = Services {
                manager: manager.clone(),
                features: services.clone(),
                limits: limits.clone(),
            };
            tokio::spawn(async move {
                if !connection.features.admission.admits(local).await {
                    return;
                }
                if let Err(error) =
                    handle_connection(socket, source.ip(), permit, configs, connection).await
                {
                    eprintln!("vault handoff connection failed: {error}");
                }
            });
        },
    )
    .await;
}

/// What one connection may reach on the coordinator.
struct Services<R> {
    manager: PairingManager,
    features: CoordinatorServices<R>,
    limits: ConnectionLimits,
}

async fn handle_connection<R: Routes>(
    socket: TcpStream,
    source: IpAddr,
    permit: PendingPermit,
    configs: ServerConfigCache,
    services: Services<R>,
) -> Result<(), String> {
    let Services {
        manager,
        features:
            CoordinatorServices {
                hub,
                admission: _,
                quarantined,
                log_admission,
                routes,
            },
        limits,
    } = services;
    let config = configs.current(&manager).await?;
    let mut stream = tokio::time::timeout(
        TLS_HANDSHAKE_TIMEOUT,
        TlsAcceptor::from(config).accept(socket),
    )
    .await
    .map_err(|_| "TLS handshake timed out".to_string())?
    .map_err(|error| format!("accept TLS connection: {error}"))?;
    let peer_certificate = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .cloned();
    if let Some(quarantined) = quarantined.as_ref()
        && peer_certificate.is_some()
        && let Ok(peer) = manager.authenticated_peer(peer_certificate.as_ref())
        && refuses_quarantined(quarantined().await, &peer.device_id)
    {
        return Err(format!("refused quarantined device {}", peer.device_id));
    }
    let revoked = manager.is_revoked_certificate(peer_certificate.as_ref())?;
    // A linked certificate moves the connection to the authenticated budget; anything else keeps
    // its pending slot and the small frame bound until it closes.
    let (_pending, authenticated) = if peer_certificate.is_some() && !revoked {
        let Some(authenticated) = permit.authenticate() else {
            return send_error(
                &mut stream,
                COORDINATOR_BUSY_CODE,
                "the coordinator is handling too many connections; retry shortly",
                true,
            )
            .await;
        };
        (None, Some(authenticated))
    } else {
        (Some(permit), None)
    };
    let control_limit = if authenticated.is_some() {
        protocol::MAX_CONTROL_BYTES
    } else {
        protocol::MAX_UNAUTHENTICATED_CONTROL_BYTES
    };
    let request = match timeout_control(read_control_limited(&mut stream, control_limit)).await {
        Ok(request) => request,
        Err(error) => {
            return send_error(&mut stream, "malformed_control", &error, false).await;
        }
    };

    if revoked {
        return send_error(
            &mut stream,
            MEMBERSHIP_REVOKED_CODE,
            "this device was unlinked by the coordinator",
            false,
        )
        .await;
    }

    if let (Some(source), Some(certificate)) = (log_admission.as_ref(), peer_certificate.as_ref())
        && let Some(refusal) = log_refusal(source().await, certificate, &request)
    {
        let retryable = refusal == MemberRefusal::Unavailable;
        return send_error(&mut stream, refusal.code(), refusal.describe(), retryable).await;
    }

    if let Some((vault_id, device_id)) = request.sync_peer() {
        manager.verify_authenticated_peer(device_id, peer_certificate.as_ref(), vault_id)?;
        let Some(hub) = hub.as_ref() else {
            return send_error(
                &mut stream,
                "sync_unsupported",
                "this coordinator does not serve sync",
                false,
            )
            .await;
        };
        let response = hub.respond(request).await;
        return timeout_control(write_control(&mut stream, &response)).await;
    }

    if routes.handles(&request) {
        let peer = Peer {
            manager: &manager,
            source,
            certificate: peer_certificate.as_ref(),
            limits: &limits,
            admission: None,
        };
        return routes.serve(&mut stream, peer, request).await;
    }
    send_error(
        &mut stream,
        "unsupported_message",
        "the coordinator does not accept this message in the current state",
        false,
    )
    .await
}

/// Coordinator TLS: version 1.3 only, and client certificates checked against the exact pinned
/// certificates of linked and revoked devices, so a revoked device still reaches its stale-link
/// cleanup while an unknown certificate never completes the handshake.
fn server_config(
    identity: TlsIdentity,
    client_certificates: Vec<CertificateDer<'static>>,
) -> Result<rustls::ServerConfig, String> {
    let builder = rustls::ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13]);
    let config = if !client_certificates.is_empty() {
        builder.with_client_cert_verifier(Arc::new(PinnedClientVerifier::new(&client_certificates)))
    } else {
        builder.with_no_client_auth()
    }
    .with_single_cert(vec![identity.certificate], identity.private_key.clone_key())
    .map_err(|error| format!("configure coordinator TLS identity: {error}"))?;
    Ok(config)
}

/// Why the selected log turns away `request` from the device whose certificate is
/// `certificate`: a device the log removed reaches no coordinator service but record exchange,
/// pairing still linking it or not, and any other sync request needs a device the log enrolled.
/// Record exchange is served to every paired device, so an authorization branch the local log
/// does not select still reaches it. A request from a certificate
/// the log does not know, such as a device linked before its enrollment was sequenced, keeps
/// every other service. Without a log pairing alone decides, and a log that cannot be read
/// refuses every request until it can be, since it may hold the removal.
pub fn log_refusal(
    admission: Result<Option<MemberAdmission>, String>,
    certificate: &CertificateDer<'_>,
    request: &ControlMessage,
) -> Option<MemberRefusal> {
    let admission = match admission {
        Ok(Some(admission)) => admission,
        Ok(None) => return None,
        Err(error) => {
            eprintln!("sync: read the log admission: {error}");
            return Some(MemberRefusal::Unavailable);
        }
    };
    let fingerprint = certificate_fingerprint(certificate.as_ref());
    match admission.certificate(&fingerprint) {
        Ok(_) | Err(MemberRefusal::Quarantined) => None,
        Err(MemberRefusal::Removed | MemberRefusal::Unknown) if request.is_record_exchange() => {
            None
        }
        Err(MemberRefusal::Removed) => Some(MemberRefusal::Removed),
        Err(refusal) if request.sync_peer().is_some() => Some(refusal),
        Err(_) => None,
    }
}

/// Whether the listener turns a linked device away as quarantined. A quarantine list that
/// cannot be read is reported and refuses no one, so a damaged file never locks every linked
/// device out, handoff recovery included.
pub fn refuses_quarantined(quarantined: Result<BTreeSet<String>, String>, device_id: &str) -> bool {
    match quarantined {
        Ok(devices) => devices.contains(device_id),
        Err(error) => {
            eprintln!("sync: read the quarantine list: {error}");
            false
        }
    }
}
