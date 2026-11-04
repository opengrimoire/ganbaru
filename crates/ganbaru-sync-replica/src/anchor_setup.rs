//! The device side of setting up an always-on anchor server.
//!
//! An admin device dials the server the setup code names, pinned to its certificate and without a
//! client certificate, proves the setup secret, and learns the keys the server enrolls with. Once
//! the selected log enrolls the server, a second setup connection uploads the snapshot it runs.
//! After every device is lost, a new device proves a fresh secret and sends a proposal the
//! recovery admin key signed over the same connection.

use crate::anchor_link::{
    ServerStream, ServerTarget, dial_server, local_compatibility, request_on,
};
use crate::service::ServerEnrollment;
use ganbaru_handoff::dial::LoopbackPolicy;
use ganbaru_handoff::pairing::PinnedCharter;
use ganbaru_handoff::protocol::anchor::{
    self, AnchorSetupCode, AnchorSpace, DEVICE_NEEDS_UPDATE_CODE, SERVER_NEEDS_UPDATE_CODE,
    SETUP_REFUSED_CODE,
};
use ganbaru_handoff::protocol::{
    self, BundleMetadata, ControlMessage, decode_sync_admin_key, decode_sync_device_key,
    read_control, relay, write_control,
};
use ganbaru_sync_contracts::{Server, ServerMode, SignedProposal};
use std::fmt;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How long the server may take to open an uploaded snapshot: it checks the whole database.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How long one chunk of the snapshot may take on either side.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(60);

/// Why a setup conversation stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnchorSetupError {
    /// The server speaks an older protocol than this device.
    ServerNeedsUpdate,
    /// The server speaks a newer protocol than this device.
    DeviceNeedsUpdate,
    /// The setup secret is wrong, used, or expired.
    Refused,
    /// Anything else, as text for the person.
    Other(String),
}

impl fmt::Display for AnchorSetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServerNeedsUpdate => formatter.write_str("the server needs an update"),
            Self::DeviceNeedsUpdate => formatter.write_str("this device needs an update"),
            Self::Refused => formatter.write_str("the server refused this setup code"),
            Self::Other(message) => formatter.write_str(message),
        }
    }
}

impl From<String> for AnchorSetupError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

fn refusal(code: &str, message: String) -> AnchorSetupError {
    match code {
        SERVER_NEEDS_UPDATE_CODE => AnchorSetupError::ServerNeedsUpdate,
        DEVICE_NEEDS_UPDATE_CODE => AnchorSetupError::DeviceNeedsUpdate,
        SETUP_REFUSED_CODE => AnchorSetupError::Refused,
        _ => AnchorSetupError::Other(message),
    }
}

fn unexpected(answer: ControlMessage, doing: &str) -> AnchorSetupError {
    match answer {
        ControlMessage::Error { code, message, .. } => refusal(&code, message),
        _ => AnchorSetupError::Other(format!("the server answered {doing} unexpectedly")),
    }
}

/// What a server told a device that proved its setup secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerIdentity {
    pub enrollment: ServerEnrollment,
    /// The space the server already runs, unverified until the caller checks its charter.
    pub space: Option<AnchorSpace>,
}

/// A setup connection after the device proved the secret.
pub struct SetupConnection {
    stream: ServerStream,
    identity: ServerIdentity,
}

/// The target a setup code names.
pub fn setup_target(code: &AnchorSetupCode) -> ServerTarget {
    ServerTarget {
        endpoints: code.0.endpoints.clone(),
        certificate_fingerprint: code.0.certificate_fingerprint,
    }
}

/// Dials the server in `code` and proves its secret. `vault_id` is the vault this device sets
/// up, or `None` when it recovers the vault the server runs.
pub async fn open_setup(
    code: &AnchorSetupCode,
    app_version: &str,
    vault_id: Option<&str>,
    loopback: LoopbackPolicy,
) -> Result<SetupConnection, AnchorSetupError> {
    let target = setup_target(code);
    let mut stream = dial_server(&target, None, loopback).await?;
    let client_nonce = anchor::anchor_nonce()?;
    let local = local_compatibility(app_version);
    let hello = ControlMessage::AnchorHello {
        protocol_version: protocol::PROTOCOL_VERSION,
        compatibility: local.clone(),
        client_nonce: relay::encode_relay_bytes(&client_nonce),
    };
    let (protocol_version, compatibility, server_nonce) =
        match request_on(&mut stream, &hello).await? {
            ControlMessage::AnchorChallenge {
                protocol_version,
                compatibility,
                server_nonce,
            } => (protocol_version, compatibility, server_nonce),
            other => return Err(unexpected(other, "the setup greeting")),
        };
    if let Some(code) = anchor::version_gap(protocol::PROTOCOL_VERSION, protocol_version) {
        return Err(refusal(code, String::new()));
    }
    protocol::ensure_compatible(&local, &compatibility)?;
    let server_nonce = anchor::decode_anchor_nonce(&server_nonce)?;
    let proof = anchor::anchor_setup_proof(
        &code.0.token,
        &server_nonce,
        &client_nonce,
        &code.0.certificate_fingerprint,
    );
    let answer = request_on(
        &mut stream,
        &ControlMessage::AnchorProof {
            proof: relay::encode_relay_bytes(&proof),
            vault_id: vault_id.map(str::to_string),
        },
    )
    .await?;
    let ControlMessage::AnchorIdentity {
        device_id,
        device_label,
        device_key,
        admin_key,
        space,
    } = answer
    else {
        return Err(unexpected(answer, "the setup proof"));
    };
    let enrollment = ServerEnrollment {
        device_id,
        device_key: decode_sync_device_key(&device_key)?,
        label: device_label,
        admin_key: decode_sync_admin_key(&admin_key)?,
        server: Server {
            mode: ServerMode::Anchor,
            endpoints: target.endpoints,
            certificate_fingerprint: target.certificate_fingerprint,
        },
    };
    Ok(SetupConnection {
        stream,
        identity: ServerIdentity { enrollment, space },
    })
}

impl SetupConnection {
    /// What the server reported about itself.
    pub fn identity(&self) -> &ServerIdentity {
        &self.identity
    }

    /// Uploads the snapshot at `archive_path` and waits until the server activated it.
    /// `take_anchor` asks the server to take the anchor once it can.
    pub async fn upload(
        mut self,
        metadata: BundleMetadata,
        archive_path: &Path,
        charter: PinnedCharter,
        take_anchor: bool,
    ) -> Result<(), AnchorSetupError> {
        let transfer_id = metadata.transfer_id.clone();
        let vault_id = metadata.vault_id.clone();
        let upload = ControlMessage::AnchorUpload {
            metadata,
            charter: charter.charter,
            person_key: charter.person_key,
            take_anchor,
        };
        match request_on(&mut self.stream, &upload).await? {
            ControlMessage::ResumeAt { offset: 0 } => {}
            other => return Err(unexpected(other, "the snapshot upload")),
        }
        send_archive(&mut self.stream, archive_path).await?;
        let finish = async {
            write_control(
                &mut self.stream,
                &ControlMessage::BundleComplete { transfer_id },
            )
            .await?;
            read_control(&mut self.stream).await
        };
        let answer = tokio::time::timeout(ACTIVATION_TIMEOUT, finish)
            .await
            .map_err(|_| "the server did not open the snapshot in time".to_string())??;
        match answer {
            ControlMessage::AnchorActivated { vault_id: active } if active == vault_id => Ok(()),
            other => Err(unexpected(other, "the finished upload")),
        }
    }

    /// Hands the server's sequencer a proposal the recovery admin key signed and returns the
    /// revision that holds it.
    pub async fn recover(&mut self, proposal: &SignedProposal) -> Result<u64, AnchorSetupError> {
        let answer = request_on(
            &mut self.stream,
            &ControlMessage::AnchorRecover {
                proposal: protocol::encode_sync_proposal(proposal),
            },
        )
        .await?;
        match answer {
            ControlMessage::SyncProposed {
                revision: Some(revision),
                ..
            } => Ok(revision),
            ControlMessage::SyncProposed {
                rejection: Some(rejection),
                ..
            } => Err(AnchorSetupError::Other(format!(
                "the server rejected the recovery proposal with code {rejection}"
            ))),
            other => Err(unexpected(other, "the recovery proposal")),
        }
    }
}

async fn send_archive(stream: &mut ServerStream, archive_path: &Path) -> Result<(), String> {
    let mut file = tokio::fs::File::open(archive_path)
        .await
        .map_err(|error| format!("open snapshot archive: {error}"))?;
    let mut buffer = vec![0_u8; protocol::TRANSFER_CHUNK_BYTES];
    loop {
        let read = tokio::time::timeout(CHUNK_TIMEOUT, file.read(&mut buffer))
            .await
            .map_err(|_| "reading the snapshot archive timed out".to_string())?
            .map_err(|error| format!("read snapshot archive: {error}"))?;
        if read == 0 {
            break;
        }
        tokio::time::timeout(CHUNK_TIMEOUT, stream.write_all(&buffer[..read]))
            .await
            .map_err(|_| "sending the snapshot timed out".to_string())?
            .map_err(|error| format!("send snapshot: {error}"))?;
    }
    stream
        .flush()
        .await
        .map_err(|error| format!("flush snapshot: {error}"))
}
