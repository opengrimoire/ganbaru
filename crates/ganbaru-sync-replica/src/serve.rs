//! Listening sockets, connection admission, and the listeners built on them: the desktop
//! coordinator listener and the member sync listener that desktops and foreground phones run.
//!
//! The listeners own TLS, connection limits, admission, frame IO, and sync dispatch. Requests
//! outside sync, such as enrollment, contacts, and vault copies, go to the caller's [`Routes`].

use ganbaru_handoff::dial::{LoopbackPolicy, admit_inbound_peer};
use ganbaru_handoff::pairing::{PairingManager, TlsIdentity};
use ganbaru_handoff::protocol::{ControlMessage, write_control};
use rustls::pki_types::CertificateDer;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

pub mod admission;
#[cfg(desktop)]
pub mod bundles;
#[cfg(desktop)]
pub mod coordinator;
pub mod limits;
pub mod listener;
#[cfg(desktop)]
pub mod live;
pub mod member;
pub mod server;

pub use admission::LanAdmission;
pub use listener::CoordinatorListener;
pub use member::{MemberAdmissionSource, MemberServices, serve_member};
pub use server::{PreAuthGate, PreAuthPolicy, ServerServices, serve_server};

use crate::peers::admission::MemberAdmission;
use limits::ConnectionLimits;

const ACCEPT_BACKOFF_MIN: Duration = Duration::from_millis(10);
const ACCEPT_BACKOFF_MAX: Duration = Duration::from_secs(1);
/// Error code for a linked device turned away because the authenticated budget is full.
pub const COORDINATOR_BUSY_CODE: &str = "coordinator_busy";
/// How long a TLS handshake may take before the connection is dropped.
pub const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long one control frame may take to read or write.
pub const CONTROL_TIMEOUT: Duration = Duration::from_secs(15);
/// Error code for a device the coordinator unlinked.
pub const MEMBERSHIP_REVOKED_CODE: &str = "membership_revoked";

/// The server side of an accepted TLS connection.
pub type ServerStream = tokio_rustls::server::TlsStream<TcpStream>;

/// Who sent a request a [`Routes`] implementation serves, and the listener state it may use.
pub struct Peer<'a> {
    /// The local device's pairing state.
    pub manager: &'a PairingManager,
    /// The address the connection came from.
    pub source: IpAddr,
    /// The client certificate the peer presented, if any.
    pub certificate: Option<&'a CertificateDer<'static>>,
    /// The listener's connection limits, including the contact message budget.
    pub limits: &'a ConnectionLimits,
    /// On a member listener, the admission the request was authenticated against.
    pub admission: Option<&'a MemberAdmission>,
    /// Where the listener reads admission from, so a long request such as a copy can check
    /// again while it runs.
    pub admission_source: Option<&'a MemberAdmissionSource>,
}

/// Requests a listener hands to its caller instead of answering itself.
pub trait Routes: Clone + Send + Sync + 'static {
    /// Whether [`Routes::serve`] answers `request`.
    fn handles(&self, request: &ControlMessage) -> bool;

    /// Answers one `request` from `peer` on `stream`. Called only when [`Routes::handles`]
    /// returned true.
    fn serve(
        &self,
        stream: &mut ServerStream,
        peer: Peer<'_>,
        request: ControlMessage,
    ) -> impl Future<Output = Result<(), String>> + Send;

    /// Answers a setup `request` from a server listener peer without a client certificate.
    /// `gate` carries the peer's pre-authentication deadline and budget slot; a route that
    /// verifies a setup proof calls [`PreAuthGate::setup_proven`] once it holds. The default
    /// ignores the gate and calls [`Routes::serve`], so the deadline covers the whole request.
    fn serve_unauthenticated(
        &self,
        stream: &mut ServerStream,
        peer: Peer<'_>,
        request: ControlMessage,
        gate: &PreAuthGate,
    ) -> impl Future<Output = Result<(), String>> + Send {
        let _ = gate;
        self.serve(stream, peer, request)
    }
}

/// Routes that answer nothing, for listeners that serve sync alone.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoRoutes;

impl Routes for NoRoutes {
    fn handles(&self, _request: &ControlMessage) -> bool {
        false
    }

    fn serve(
        &self,
        _stream: &mut ServerStream,
        _peer: Peer<'_>,
        _request: ControlMessage,
    ) -> impl Future<Output = Result<(), String>> + Send {
        std::future::ready(Err("no route serves this request".to_string()))
    }
}

impl<R: Routes> Routes for Option<R> {
    fn handles(&self, request: &ControlMessage) -> bool {
        self.as_ref().is_some_and(|routes| routes.handles(request))
    }

    async fn serve(
        &self,
        stream: &mut ServerStream,
        peer: Peer<'_>,
        request: ControlMessage,
    ) -> Result<(), String> {
        match self {
            Some(routes) => routes.serve(stream, peer, request).await,
            None => Err("no route serves this request".to_string()),
        }
    }

    async fn serve_unauthenticated(
        &self,
        stream: &mut ServerStream,
        peer: Peer<'_>,
        request: ControlMessage,
        gate: &PreAuthGate,
    ) -> Result<(), String> {
        match self {
            Some(routes) => {
                routes
                    .serve_unauthenticated(stream, peer, request, gate)
                    .await
            }
            None => Err("no route serves this request".to_string()),
        }
    }
}

/// Two route sets on one listener: a request goes to the first that handles it.
impl<A: Routes, B: Routes> Routes for (A, B) {
    fn handles(&self, request: &ControlMessage) -> bool {
        self.0.handles(request) || self.1.handles(request)
    }

    async fn serve(
        &self,
        stream: &mut ServerStream,
        peer: Peer<'_>,
        request: ControlMessage,
    ) -> Result<(), String> {
        if self.0.handles(&request) {
            self.0.serve(stream, peer, request).await
        } else {
            self.1.serve(stream, peer, request).await
        }
    }

    async fn serve_unauthenticated(
        &self,
        stream: &mut ServerStream,
        peer: Peer<'_>,
        request: ControlMessage,
        gate: &PreAuthGate,
    ) -> Result<(), String> {
        if self.0.handles(&request) {
            self.0
                .serve_unauthenticated(stream, peer, request, gate)
                .await
        } else {
            self.1
                .serve_unauthenticated(stream, peer, request, gate)
                .await
        }
    }
}
/// Runs one control frame read or write within [`CONTROL_TIMEOUT`].
pub async fn timeout_control<T>(
    future: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::time::timeout(CONTROL_TIMEOUT, future)
        .await
        .map_err(|_| "handoff control message timed out".to_string())?
}

/// Completes the server side of a TLS handshake on `socket` within `timeout`.
///
/// Gives `None` when the peer closes the connection before the handshake finishes. A device
/// dials several addresses of this one at once and closes every connection but the first to
/// answer, so such a close is part of an ordinary dial rather than a failure to report.
pub async fn accept_tls(
    acceptor: TlsAcceptor,
    socket: TcpStream,
    timeout: Duration,
) -> Result<Option<TlsStream<TcpStream>>, String> {
    match tokio::time::timeout(timeout, acceptor.accept(socket))
        .await
        .map_err(|_| "TLS handshake timed out".to_string())?
    {
        Ok(stream) => Ok(Some(stream)),
        Err(error) if peer_left_handshake(&error) => Ok(None),
        Err(error) => Err(format!("accept TLS connection: {error}")),
    }
}

/// Whether `error` means the peer closed the connection, rather than sent something invalid.
fn peer_left_handshake(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
    )
}

/// Loads the device ID and TLS identity on a blocking thread, because the first load may wait on
/// the operating-system keyring.
pub async fn load_tls_identity(manager: &PairingManager) -> Result<(String, TlsIdentity), String> {
    let manager = manager.clone();
    tokio::task::spawn_blocking(move || manager.identity())
        .await
        .map_err(|error| format!("load device linking key: {error}"))?
}

/// The accepted socket's own address when the shared address policy admits a peer at `source`
/// on it. A socket whose local address cannot be read is refused, so this gate never fails
/// open.
fn admitted_local_address(
    socket: &TcpStream,
    source: IpAddr,
    loopback: LoopbackPolicy,
) -> Option<SocketAddr> {
    socket
        .local_addr()
        .ok()
        .filter(|local| admit_inbound_peer(source, local.ip(), loopback).is_ok())
}

/// Accepts until shutdown. Accept errors such as descriptor or buffer exhaustion and aborted
/// handshakes are transient, so the loop logs them and backs off instead of stopping linking.
pub async fn accept_connections<S, A, F>(
    mut accept: A,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
    mut on_accepted: impl FnMut(S, SocketAddr),
) where
    A: FnMut() -> F,
    F: Future<Output = std::io::Result<(S, SocketAddr)>>,
{
    let mut backoff = ACCEPT_BACKOFF_MIN;
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = accept() => match accepted {
                Ok((socket, source)) => {
                    backoff = ACCEPT_BACKOFF_MIN;
                    on_accepted(socket, source);
                }
                Err(error) => {
                    eprintln!("vault handoff coordinator accept failed, retrying: {error}");
                    tokio::select! {
                        _ = &mut shutdown => break,
                        () = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(ACCEPT_BACKOFF_MAX);
                }
            },
        }
    }
}

/// Answers with a typed error frame.
pub async fn send_error<W>(
    writer: &mut W,
    code: &str,
    message: &str,
    retryable: bool,
) -> Result<(), String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    timeout_control(write_control(
        writer,
        &ControlMessage::Error {
            code: code.to_string(),
            message: message.to_string(),
            retryable,
        },
    ))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::server::{ClientHello, ResolvesServerCert};
    use rustls::sign::CertifiedKey;
    use std::sync::Arc;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    /// Holds no certificate; these handshakes end before the server would present one.
    #[derive(Debug)]
    struct NoCertificate;

    impl ResolvesServerCert for NoCertificate {
        fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
            None
        }
    }

    /// Accepts one connection whose client sends `bytes` and then closes.
    async fn accept_after(bytes: &[u8]) -> Result<Option<TlsStream<TcpStream>>, String> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let config =
            rustls::ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
                .with_no_client_auth()
                .with_cert_resolver(Arc::new(NoCertificate));
        let bytes = bytes.to_vec();
        let client = tokio::spawn(async move {
            let mut socket = TcpStream::connect(address).await.expect("connect");
            socket.write_all(&bytes).await.expect("write");
            socket.shutdown().await.expect("shutdown");
            socket
        });
        let (socket, _) = listener.accept().await.expect("accept");
        let accepted = accept_tls(
            TlsAcceptor::from(Arc::new(config)),
            socket,
            Duration::from_secs(5),
        )
        .await;
        drop(client.await.expect("client"));
        accepted
    }

    #[tokio::test]
    async fn a_peer_that_closes_before_the_handshake_is_not_a_failure() {
        assert!(matches!(accept_after(b"").await, Ok(None)));
    }

    #[tokio::test]
    async fn a_peer_that_sends_something_other_than_tls_is_a_failure() {
        let Err(error) = accept_after(b"GET / HTTP/1.1\r\n\r\n").await else {
            panic!("a request that is not TLS is refused");
        };
        assert!(error.starts_with("accept TLS connection:"), "{error}");
    }
}
