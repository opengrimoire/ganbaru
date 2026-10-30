//! The sync listener of a member that does not coordinate the vault: a desktop, or a phone while
//! the app is in the foreground.
//!
//! It serves only sync requests from the devices the local selected authorization log enrolled
//! and did not remove, and that this installation did not quarantine, and on a desktop the
//! snapshot copies of the vault those devices bootstrap from, through the caller's routes.
//! Pairing, other handoff, contact, and authorization proposal requests are refused with a typed
//! code, so the anchor and the coordinator keep those roles.

use super::limits::ConnectionLimits;
use super::{
    COORDINATOR_BUSY_CODE, CoordinatorListener, LanAdmission, Peer, Routes, TLS_HANDSHAKE_TIMEOUT,
    accept_connections, admitted_local_address, load_tls_identity, send_error, timeout_control,
};
use crate::hub::SyncHub;
use crate::peers::admission::{MemberAdmission, MemberRefusal};
use ganbaru_handoff::dial::LoopbackPolicy;
use ganbaru_handoff::pairing::{PairingManager, certificate_fingerprint};
use ganbaru_handoff::protocol::{self, read_control_limited, write_control};
use ganbaru_handoff::tls::PinnedClientVerifier;
use std::collections::BTreeSet;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;

/// Reads the admission of the active vault from its selected log and quarantine, or `None`
/// while sync does not run there.
pub type MemberAdmissionSource = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<Option<MemberAdmission>, String>> + Send>>
        + Send
        + Sync,
>;

/// What a member listener serves and whom it admits.
#[derive(Clone)]
pub struct MemberServices<R> {
    /// The hub that answers admitted sync requests.
    pub hub: SyncHub,
    /// Admission from the selected log, read for every authenticated connection.
    pub admission: MemberAdmissionSource,
    /// Whether the network behind each local address may reach the listener at all.
    pub lan: LanAdmission,
    /// Requests answered before admission and the hub, such as the snapshot copies a member
    /// desktop serves to enrolled devices.
    pub routes: R,
}

/// A TLS configuration and the admitted certificate fingerprints it was built for.
type CachedConfig = Option<(BTreeSet<String>, Arc<rustls::ServerConfig>)>;

/// TLS configuration built once per admitted certificate set, so connections reuse it until
/// the log or the quarantine changes the set.
#[derive(Clone, Default)]
struct MemberConfigCache(Arc<Mutex<CachedConfig>>);

impl MemberConfigCache {
    async fn current(
        &self,
        manager: &PairingManager,
        fingerprints: BTreeSet<String>,
    ) -> Result<Arc<rustls::ServerConfig>, String> {
        if let Some((cached, config)) = self.lock()?.as_ref()
            && *cached == fingerprints
        {
            return Ok(config.clone());
        }
        let (_, identity) = load_tls_identity(manager).await?;
        let verifier = PinnedClientVerifier::pinned(fingerprints.iter().cloned(), true);
        let config = Arc::new(
            rustls::ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
                .with_client_cert_verifier(Arc::new(verifier))
                .with_single_cert(vec![identity.certificate], identity.private_key.clone_key())
                .map_err(|error| format!("configure member listener TLS identity: {error}"))?,
        );
        *self.lock()? = Some((fingerprints, config.clone()));
        Ok(config)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, CachedConfig>, String> {
        self.0
            .lock()
            .map_err(|_| "member listener TLS configuration lock is unavailable".to_string())
    }
}

/// How long the admission a handshake is configured from may be reused, so connections that
/// never authenticate cannot make the listener read the log for each of them. Every connection
/// that completes the handshake is checked again against a fresh read.
const HANDSHAKE_ADMISSION_TTL: Duration = Duration::from_secs(5);

/// The admission handshakes were last configured from, and when it was read.
type CachedAdmission = Option<(Instant, Option<MemberAdmission>)>;

/// The admission read for configuring handshakes, reused for [`HANDSHAKE_ADMISSION_TTL`].
#[derive(Clone, Default)]
struct HandshakeAdmission(Arc<tokio::sync::Mutex<CachedAdmission>>);

impl HandshakeAdmission {
    async fn current(
        &self,
        source: &MemberAdmissionSource,
    ) -> Result<Option<MemberAdmission>, String> {
        let mut cached = self.0.lock().await;
        if let Some((read_at, admission)) = cached.as_ref()
            && read_at.elapsed() < HANDSHAKE_ADMISSION_TTL
        {
            return Ok(admission.clone());
        }
        let admission = source().await?;
        *cached = Some((Instant::now(), admission.clone()));
        Ok(admission)
    }
}

/// Serves `listener` as a member sync listener until shutdown.
pub async fn serve_member<R: Routes>(
    listener: CoordinatorListener,
    limits: ConnectionLimits,
    loopback: LoopbackPolicy,
    manager: PairingManager,
    services: MemberServices<R>,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    let configs = MemberConfigCache::default();
    let handshakes = HandshakeAdmission::default();
    accept_connections(
        || listener.accept(),
        shutdown,
        |socket, source: SocketAddr| {
            let Some(local) = admitted_local_address(&socket, source.ip(), loopback) else {
                drop(socket);
                return;
            };
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
                if !services.lan.admits(local).await {
                    return;
                }
                let connection = MemberConnection {
                    manager: &manager,
                    services: &services,
                    configs: &configs,
                    handshakes: &handshakes,
                    limits: &limits,
                    source: source.ip(),
                };
                if let Err(error) = handle_member_connection(socket, &connection, permit).await {
                    eprintln!("sync member listener: connection from {source} failed: {error}");
                }
            });
        },
    )
    .await;
}

/// What one accepted connection is served with.
struct MemberConnection<'a, R> {
    manager: &'a PairingManager,
    services: &'a MemberServices<R>,
    configs: &'a MemberConfigCache,
    handshakes: &'a HandshakeAdmission,
    limits: &'a ConnectionLimits,
    source: IpAddr,
}

async fn handle_member_connection<R: Routes>(
    socket: TcpStream,
    connection: &MemberConnection<'_, R>,
    permit: super::limits::PendingPermit,
) -> Result<(), String> {
    let Some(admission) = connection
        .handshakes
        .current(&connection.services.admission)
        .await?
    else {
        return Err("sync does not run in the active vault".to_string());
    };
    let fingerprints = admission.admitted_fingerprints();
    if fingerprints.is_empty() {
        return Err("no member is admitted".to_string());
    }
    let config = connection
        .configs
        .current(connection.manager, fingerprints)
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
        .cloned()
        .ok_or_else(|| "the peer presented no certificate".to_string())?;
    let fingerprint = certificate_fingerprint(certificate.as_ref());
    // The handshake may have used an admission read a few seconds ago; an authenticated peer
    // is checked against the log as it is now. The request is read before a refusal is sent,
    // so closing does not reset the connection under unread data and the peer sees the code.
    let Some(admission) = (connection.services.admission)().await? else {
        return Err("sync does not run in the active vault".to_string());
    };
    let Some(_authenticated) = permit.authenticate() else {
        return send_error(
            &mut stream,
            COORDINATOR_BUSY_CODE,
            "this device is handling too many connections; retry shortly",
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

/// Answers a refused request with its typed code and returns the refusal as the connection's
/// error, so it is logged without keys or certificates.
async fn refuse<W>(stream: &mut W, refusal: MemberRefusal) -> Result<(), String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    send_error(stream, refusal.code(), refusal.describe(), false).await?;
    Err(format!("refused: {}", refusal.code()))
}
