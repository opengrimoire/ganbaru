//! The pinned TLS connection to the relay: dialing the configured endpoints under the server
//! dial rule, certificate pinning, request framing with its bounds, and the one-shot claim,
//! probe, and remove-space requests that need no relay session.

use super::{RelayError, RelayProblem, RelayTarget, relay_ranges, unexpected};
use ganbaru_handoff::dial::{LoopbackPolicy, authorize_server_dial_target};
use ganbaru_handoff::pairing::certificate_fingerprint;
use ganbaru_handoff::protocol::relay::{
    RELAY_ADMIN_DOMAIN, RELAY_EXPORTER_BYTES, RELAY_EXPORTER_LABEL, RELAY_PROTOCOL_VERSION,
    RELAY_WAIT_TIMEOUT, RelayAdminAction, RelayAdminCommand, RelayBinding, RelayRefusalCode,
    RelayRequest, RelayResponse, RelaySetupToken, encode_relay_bytes,
};
use ganbaru_handoff::protocol::{MAX_CONTROL_BYTES, read_frame, write_frame};
use ganbaru_sync_contracts::{
    AdminKeyPair, AdminPublicKey, RelayAccessKeyPair, SpaceId, VersionVector,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use zeroize::Zeroizing;

/// Bound on connecting to one relay address, the TLS handshake included.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Bound on one request and its answer; a wait answers within [`RELAY_WAIT_TIMEOUT`].
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
/// Name presented in the TLS handshake. The pinned fingerprint, not the name, identifies the
/// relay.
const RELAY_SERVER_NAME: &str = "ganbaru-relay.local";

/// One TLS connection to the relay, pinned to the configured certificate.
pub struct Connection {
    stream: TlsStream<TcpStream>,
    exporter: Zeroizing<[u8; RELAY_EXPORTER_BYTES]>,
    space: SpaceId,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Connection")
            .field("space", &self.space)
            .finish_non_exhaustive()
    }
}

impl Connection {
    /// Dials the target's endpoints in order. Each resolved address passes the configured
    /// server dial rule first, see [`authorize_server_dial_target`].
    pub async fn open(
        target: &RelayTarget,
        space: SpaceId,
        loopback: LoopbackPolicy,
    ) -> Result<Self, RelayError> {
        let config = client_config(&target.certificate_fingerprint)?;
        let mut last = "the relay has no reachable address".to_string();
        for endpoint in &target.endpoints {
            let addresses: Vec<SocketAddr> = match tokio::time::timeout(
                CONNECT_TIMEOUT,
                tokio::net::lookup_host(endpoint),
            )
            .await
            {
                Ok(Ok(addresses)) => addresses.collect(),
                Ok(Err(error)) => {
                    last = format!("resolve relay address {endpoint}: {error}");
                    continue;
                }
                Err(_) => {
                    last = format!("resolving relay address {endpoint} timed out");
                    continue;
                }
            };
            for address in addresses {
                if let Err(error) = authorize_server_dial_target(address, loopback) {
                    last = format!("relay address {address} is not allowed: {error:?}");
                    continue;
                }
                match tokio::time::timeout(CONNECT_TIMEOUT, connect(&config, address)).await {
                    Ok(Ok((stream, exporter))) => {
                        return Ok(Self {
                            stream,
                            exporter,
                            space,
                        });
                    }
                    Ok(Err(error)) => last = error,
                    Err(_) => last = format!("connecting to relay address {address} timed out"),
                }
            }
        }
        Err(RelayError::new(RelayProblem::Unreachable, last))
    }

    /// The signature context of this connection.
    pub fn binding(&self) -> RelayBinding {
        RelayBinding {
            exporter: *self.exporter,
            space: self.space,
        }
    }

    /// Sends one request and reads its answer.
    pub async fn request(&mut self, request: &RelayRequest) -> Result<RelayResponse, RelayError> {
        let exchange = async {
            write_frame(&mut self.stream, request).await?;
            read_frame(&mut self.stream, MAX_CONTROL_BYTES).await
        };
        let timeout = REQUEST_TIMEOUT.max(RELAY_WAIT_TIMEOUT * 2);
        tokio::time::timeout(timeout, exchange)
            .await
            .map_err(|_| {
                RelayError::new(
                    RelayProblem::Unreachable,
                    "the relay did not answer in time",
                )
            })?
            .map_err(|error| RelayError::new(RelayProblem::Unreachable, error))
    }

    /// Opens the exchange for the connection's space.
    pub async fn hello(&mut self) -> Result<(), RelayError> {
        let request = RelayRequest::Hello {
            protocol_version: RELAY_PROTOCOL_VERSION,
            space: self.space.to_hex(),
        };
        match self.request(&request).await? {
            RelayResponse::Welcome { protocol_version }
                if protocol_version == RELAY_PROTOCOL_VERSION =>
            {
                Ok(())
            }
            other => Err(unexpected("Welcome", &other)),
        }
    }

    /// Runs an admin command with `counter`. A stale counter is retried once with the counter
    /// the relay names. Returns the counter the relay applied.
    pub async fn admin(
        &mut self,
        admin: &AdminKeyPair,
        counter: u64,
        command: &RelayAdminCommand<'_>,
    ) -> Result<u64, RelayError> {
        let mut counter = counter.max(1);
        for _ in 0..2 {
            let request = self.admin_request(admin, counter, command)?;
            match self.request(&request).await? {
                RelayResponse::AdminApplied { counter: applied } => return Ok(applied),
                RelayResponse::Refused {
                    code: RelayRefusalCode::AdminCounter,
                    next_counter: Some(next),
                    ..
                } if next > counter => counter = next,
                other => return Err(unexpected("AdminApplied", &other)),
            }
        }
        Err(RelayError::new(
            RelayProblem::AdminRefused,
            "the relay kept refusing the admin counter",
        ))
    }

    fn admin_request(
        &self,
        admin: &AdminKeyPair,
        counter: u64,
        command: &RelayAdminCommand<'_>,
    ) -> Result<RelayRequest, RelayError> {
        let action = match command {
            RelayAdminCommand::AddKey(key) => RelayAdminAction::AddKey {
                key: encode_relay_bytes(key.as_bytes()),
            },
            RelayAdminCommand::RemoveKey(key) => RelayAdminAction::RemoveKey {
                key: encode_relay_bytes(key.as_bytes()),
            },
            RelayAdminCommand::Wipe => RelayAdminAction::Wipe,
            RelayAdminCommand::RemoveSpace => RelayAdminAction::RemoveSpace,
            RelayAdminCommand::RelayEpochRepair { epoch, access_key } => {
                RelayAdminAction::RelayEpochRepair {
                    epoch: *epoch,
                    access_key: encode_relay_bytes(access_key.as_bytes()),
                }
            }
            RelayAdminCommand::Claim { .. } => {
                return Err("a claim is not an admin request".to_string().into());
            }
        };
        let payload = self.binding().admin_payload(counter, command);
        Ok(RelayRequest::Admin {
            action,
            counter,
            signer: encode_relay_bytes(admin.public_key().as_bytes()),
            signature: encode_relay_bytes(&admin.sign(RELAY_ADMIN_DOMAIN, &payload)),
        })
    }

    /// Claims the space with a one-time setup token: its first epoch takes the access key of
    /// `epoch` and the relay's admin set is `admins`, `admin` among them.
    pub async fn claim(
        &mut self,
        token: &RelaySetupToken,
        epoch: u32,
        access: &RelayAccessKeyPair,
        admin: &AdminKeyPair,
        admins: &[AdminPublicKey],
    ) -> Result<(), RelayError> {
        let access_key = access.public_key();
        let payload = self.binding().admin_payload(
            1,
            &RelayAdminCommand::Claim {
                epoch,
                access_key: &access_key,
                admins,
            },
        );
        let request = RelayRequest::Claim {
            token: token.clone(),
            epoch,
            access_key: encode_relay_bytes(access_key.as_bytes()),
            admins: admins
                .iter()
                .map(|key| encode_relay_bytes(key.as_bytes()))
                .collect(),
            counter: 1,
            signer: encode_relay_bytes(admin.public_key().as_bytes()),
            signature: encode_relay_bytes(&admin.sign(RELAY_ADMIN_DOMAIN, &payload)),
        };
        match self.request(&request).await? {
            RelayResponse::Claimed { epoch: claimed } if claimed == epoch => Ok(()),
            other => Err(unexpected("Claimed", &other)),
        }
    }

    /// The last sequence of each writer the relay holds.
    pub(super) async fn ranges(&mut self) -> Result<VersionVector, RelayError> {
        match self.request(&RelayRequest::State).await? {
            RelayResponse::State { writers, .. } => relay_ranges(&writers),
            other => Err(unexpected("State", &other)),
        }
    }
}

/// Claims `space` on the relay `target` names, see [`Connection::claim`].
pub async fn claim_space(
    target: &RelayTarget,
    space: SpaceId,
    loopback: LoopbackPolicy,
    token: &RelaySetupToken,
    first: &ganbaru_sync_contracts::SpaceKey,
    admin: &AdminKeyPair,
    admins: &[AdminPublicKey],
) -> Result<(), RelayError> {
    let access = first
        .relay_access_key(&space)
        .map_err(|error| format!("derive relay access key: {error}"))?;
    let mut connection = Connection::open(target, space, loopback).await?;
    connection.hello().await?;
    connection
        .claim(token, first.epoch(), &access, admin, admins)
        .await
}

/// Opens the exchange with the relay `target` names and returns the protocol version it
/// answers with. Needs no key, so it tells whether the relay answers at all.
pub async fn probe(
    target: &RelayTarget,
    space: SpaceId,
    loopback: LoopbackPolicy,
) -> Result<u16, RelayError> {
    let mut connection = Connection::open(target, space, loopback).await?;
    let request = RelayRequest::Hello {
        protocol_version: RELAY_PROTOCOL_VERSION,
        space: space.to_hex(),
    };
    match connection.request(&request).await? {
        RelayResponse::Welcome { protocol_version } => Ok(protocol_version),
        other => Err(unexpected("Welcome", &other)),
    }
}

/// Asks the relay `target` names to drop `space` and everything it stores for it, signed by
/// `admin`, which must be in the relay's admin set.
pub async fn remove_space(
    target: &RelayTarget,
    space: SpaceId,
    loopback: LoopbackPolicy,
    admin: &AdminKeyPair,
) -> Result<(), RelayError> {
    let mut connection = Connection::open(target, space, loopback).await?;
    connection.hello().await?;
    connection
        .admin(admin, 1, &RelayAdminCommand::RemoveSpace)
        .await?;
    Ok(())
}

async fn connect(
    config: &Arc<rustls::ClientConfig>,
    address: SocketAddr,
) -> Result<(TlsStream<TcpStream>, Zeroizing<[u8; RELAY_EXPORTER_BYTES]>), String> {
    let socket = TcpStream::connect(address)
        .await
        .map_err(|error| format!("connect to relay address {address}: {error}"))?;
    let name = ServerName::try_from(RELAY_SERVER_NAME)
        .map_err(|_| "relay server name is invalid".to_string())?;
    let stream = TlsConnector::from(Arc::clone(config))
        .connect(name, socket)
        .await
        .map_err(|error| format!("relay TLS handshake with {address}: {error}"))?;
    let exporter = stream
        .get_ref()
        .1
        .export_keying_material(
            Zeroizing::new([0u8; RELAY_EXPORTER_BYTES]),
            RELAY_EXPORTER_LABEL,
            None,
        )
        .map_err(|error| format!("relay TLS exporter: {error}"))?;
    Ok((stream, exporter))
}

fn client_config(fingerprint: &[u8; 32]) -> Result<Arc<rustls::ClientConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(PinnedVerifier {
        fingerprint: fingerprint
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        algorithms: provider.signature_verification_algorithms,
    });
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| format!("relay TLS configuration: {error}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Accepts exactly the certificate whose SHA-256 the server configuration pins, with no chain.
#[derive(Debug)]
struct PinnedVerifier {
    fingerprint: String,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if !intermediates.is_empty()
            || certificate_fingerprint(end_entity.as_ref()) != self.fingerprint
        {
            return Err(rustls::Error::General(
                "relay certificate is not the pinned one".to_string(),
            ));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, certificate, signature, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, certificate, signature, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}
