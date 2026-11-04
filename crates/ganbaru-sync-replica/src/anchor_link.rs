//! Connections to an always-on anchor server.
//!
//! The selected log's server configuration is the only source of these endpoints, which is why
//! they follow the configured server dial rule instead of the LAN-only rule of member dials. A
//! connection pins the configured certificate. An enrolled device presents its own certificate;
//! a device that proves a setup secret presents none.

use crate::client::SyncTransport;
use ganbaru_handoff::dial::{LoopbackPolicy, authorize_server_dial_target};
use ganbaru_handoff::pairing::{PairingManager, TlsIdentity};
use ganbaru_handoff::protocol::{
    self, ControlMessage, HandoffCompatibility, anchor, read_control, relay, write_control,
};
use ganbaru_handoff::tls::PinnedServerVerifier;
use ganbaru_sync::AuthorizationView;
use ganbaru_sync_contracts::ServerMode;
use rustls::pki_types::ServerName;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

/// How long one address gets to accept a connection, and then to finish the handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a request may take. A server answers proposals through its own pass, which is
/// bounded well below this.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
/// The name a TLS client sends, which pinning makes meaningless.
const SERVER_NAME: &str = "ganbaru-anchor.local";

/// A TLS stream to a server.
pub type ServerStream = TlsStream<TcpStream>;

/// Where an anchor server listens and which certificate it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerTarget {
    pub endpoints: Vec<String>,
    /// SHA-256 of the server's certificate.
    pub certificate_fingerprint: [u8; 32],
}

impl ServerTarget {
    /// The anchor server the selected server configuration names, `None` when the space has no
    /// server or its server runs as a relay.
    pub fn of(view: &AuthorizationView) -> Option<Self> {
        let server = view.server.as_ref()?;
        (server.mode == ServerMode::Anchor).then(|| Self {
            endpoints: server.endpoints.clone(),
            certificate_fingerprint: server.certificate_fingerprint,
        })
    }

    /// The certificate fingerprint as hex.
    fn fingerprint_hex(&self) -> String {
        self.certificate_fingerprint
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// The enrolled device behind this target in `view`, matched by certificate, so a server
    /// configuration cannot speak for another device.
    pub fn device_id<'v>(&self, view: &'v AuthorizationView) -> Option<&'v str> {
        view.devices
            .iter()
            .find(|device| {
                device.removed_revision.is_none()
                    && device.tls_fingerprint == self.certificate_fingerprint
            })
            .map(|device| device.device_id.as_str())
    }
}

/// The compatibility this build reports in a setup conversation.
pub fn local_compatibility(app_version: &str) -> HandoffCompatibility {
    let schema = Sha256::digest(ganbaru_db::migration_set_identity_material());
    HandoffCompatibility {
        app_version: app_version.to_string(),
        database_schema_sha256: format!("{schema:x}"),
    }
}

fn client_config(
    target: &ServerTarget,
    identity: Option<TlsIdentity>,
) -> Result<Arc<rustls::ClientConfig>, String> {
    let verifier = Arc::new(PinnedServerVerifier::new(target.fingerprint_hex()));
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|error| format!("server TLS configuration: {error}"))?
    .dangerous()
    .with_custom_certificate_verifier(verifier);
    let config = match identity {
        Some(identity) => builder
            .with_client_auth_cert(vec![identity.certificate], identity.private_key.clone_key())
            .map_err(|error| format!("configure device TLS identity: {error}"))?,
        None => builder.with_no_client_auth(),
    };
    Ok(Arc::new(config))
}

/// Why a dial to a server failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialError {
    /// No address accepted a TCP connection: the server is down, the address is wrong, or a
    /// firewall drops the port.
    Unreachable(String),
    /// An address accepted the connection, or the dial could not start, and it still failed.
    Failed(String),
}

impl DialError {
    /// Prefix of the unreachable text, which callers that only see text match on.
    pub const UNREACHABLE: &'static str = "could not reach the server";
}

impl std::fmt::Display for DialError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(detail) => write!(formatter, "{}: {detail}", Self::UNREACHABLE),
            Self::Failed(detail) => formatter.write_str(detail),
        }
    }
}

impl From<DialError> for String {
    fn from(error: DialError) -> Self {
        error.to_string()
    }
}

/// Why one address failed: before the TCP connection, or after it.
enum AttemptError {
    Connect(String),
    Handshake(String),
}

async fn handshake(
    config: &Arc<rustls::ClientConfig>,
    address: SocketAddr,
) -> Result<ServerStream, AttemptError> {
    let socket = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
        Ok(Ok(socket)) => socket,
        Ok(Err(error)) => {
            return Err(AttemptError::Connect(format!(
                "connect to {address}: {error}"
            )));
        }
        Err(_) => {
            return Err(AttemptError::Connect(format!(
                "connecting to {address} timed out"
            )));
        }
    };
    let name = ServerName::try_from(SERVER_NAME)
        .map_err(|_| AttemptError::Handshake("server TLS name is invalid".to_string()))?;
    let connecting = TlsConnector::from(config.clone()).connect(name, socket);
    match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(error)) => Err(AttemptError::Handshake(format!(
            "establish pinned server connection: {error}"
        ))),
        Err(_) => Err(AttemptError::Handshake(format!(
            "the server at {address} did not finish the handshake in time"
        ))),
    }
}

/// Dials `target`'s endpoints in order. Each resolved address passes the configured server dial
/// rule first, see [`authorize_server_dial_target`]. The error is [`DialError::Unreachable`]
/// only when no address accepted a connection.
pub async fn dial_server(
    target: &ServerTarget,
    identity: Option<TlsIdentity>,
    loopback: LoopbackPolicy,
) -> Result<ServerStream, DialError> {
    let config = client_config(target, identity).map_err(DialError::Failed)?;
    let mut last = DialError::Failed("the server has no reachable address".to_string());
    let mut reached = false;
    for endpoint in &target.endpoints {
        let addresses: Vec<SocketAddr> =
            match tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::lookup_host(endpoint)).await {
                Ok(Ok(addresses)) => addresses.collect(),
                Ok(Err(error)) => {
                    if !reached {
                        last = DialError::Unreachable(format!("resolve {endpoint}: {error}"));
                    }
                    continue;
                }
                Err(_) => {
                    if !reached {
                        last = DialError::Unreachable(format!("resolving {endpoint} timed out"));
                    }
                    continue;
                }
            };
        for address in addresses {
            if let Err(error) = authorize_server_dial_target(address, loopback) {
                if !reached {
                    last = DialError::Failed(format!(
                        "server address {address} is not allowed: {error:?}"
                    ));
                }
                continue;
            }
            match handshake(&config, address).await {
                Ok(stream) => return Ok(stream),
                Err(AttemptError::Connect(detail)) => {
                    if !reached {
                        last = DialError::Unreachable(detail);
                    }
                }
                Err(AttemptError::Handshake(detail)) => {
                    reached = true;
                    last = DialError::Failed(detail);
                }
            }
        }
    }
    Err(last)
}

/// Sends one control message on `stream` and reads its answer.
pub async fn request_on(
    stream: &mut ServerStream,
    message: &ControlMessage,
) -> Result<ControlMessage, String> {
    let exchange = async {
        write_control(stream, message).await?;
        read_control(stream).await
    };
    tokio::time::timeout(REQUEST_TIMEOUT, exchange)
        .await
        .map_err(|_| "the server did not answer in time".to_string())?
}

/// Sends sync requests to an anchor server as an enrolled device, one connection per request.
#[derive(Clone)]
pub struct ServerTransport {
    pub manager: PairingManager,
    pub target: ServerTarget,
    pub loopback: LoopbackPolicy,
}

impl SyncTransport for ServerTransport {
    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        let (_, identity) = crate::serve::load_tls_identity(&self.manager).await?;
        let mut stream = dial_server(&self.target, Some(identity), self.loopback).await?;
        request_on(&mut stream, &message).await
    }
}

/// Asks a server which protocol it speaks after an exchange failed. Returns the code of the side
/// that needs an update, `None` when the two agree or the server does not answer.
pub async fn protocol_gap(transport: &ServerTransport, app_version: &str) -> Option<&'static str> {
    let client_nonce = anchor::anchor_nonce().ok()?;
    let hello = ControlMessage::AnchorHello {
        protocol_version: protocol::PROTOCOL_VERSION,
        compatibility: local_compatibility(app_version),
        client_nonce: relay::encode_relay_bytes(&client_nonce),
    };
    match transport.request(hello).await.ok()? {
        ControlMessage::AnchorChallenge {
            protocol_version, ..
        } => anchor::version_gap(protocol::PROTOCOL_VERSION, protocol_version),
        ControlMessage::Error { code, .. } if code == anchor::SERVER_NEEDS_UPDATE_CODE => {
            Some(anchor::SERVER_NEEDS_UPDATE_CODE)
        }
        ControlMessage::Error { code, .. } if code == anchor::DEVICE_NEEDS_UPDATE_CODE => {
            Some(anchor::DEVICE_NEEDS_UPDATE_CODE)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(endpoint: String) -> ServerTarget {
        ServerTarget {
            endpoints: vec![endpoint],
            certificate_fingerprint: [7; 32],
        }
    }

    #[tokio::test]
    async fn a_closed_port_is_unreachable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let error = dial_server(&target(address.to_string()), None, LoopbackPolicy::Allow)
            .await
            .err()
            .unwrap();
        assert!(matches!(error, DialError::Unreachable(_)), "{error:?}");
        assert!(error.to_string().starts_with(DialError::UNREACHABLE));
    }

    #[tokio::test]
    async fn a_server_that_accepts_and_fails_the_handshake_is_not_unreachable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let closer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            drop(socket);
        });
        let error = dial_server(&target(address.to_string()), None, LoopbackPolicy::Allow)
            .await
            .err()
            .unwrap();
        closer.await.unwrap();
        assert!(matches!(error, DialError::Failed(_)), "{error:?}");
    }

    #[tokio::test]
    async fn an_answering_address_outranks_a_later_unreachable_one() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let answering = listener.local_addr().unwrap();
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed_address = closed.local_addr().unwrap();
        drop(closed);
        let closer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            drop(socket);
        });
        let target = ServerTarget {
            endpoints: vec![answering.to_string(), closed_address.to_string()],
            certificate_fingerprint: [7; 32],
        };
        let error = dial_server(&target, None, LoopbackPolicy::Allow)
            .await
            .err()
            .unwrap();
        closer.await.unwrap();
        assert!(matches!(error, DialError::Failed(_)), "{error:?}");
    }
}
