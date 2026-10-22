//! Pinned client transport and shared resumable archive streaming.

#[cfg(desktop)]
mod server;
#[cfg(desktop)]
pub(crate) use server::{
    CoordinatorListener, CoordinatorServices, LanAdmission, serve_with_coordinator,
};

use super::pairing::{
    CoordinatorPin, PairingManager, TlsIdentity, certificate_fingerprint, encode_certificate,
};
use super::protocol::{
    BundleMetadata, BundlePurpose, ControlMessage, PROTOCOL_VERSION, PairingInvitation,
    TRANSFER_CHUNK_BYTES, read_control, unix_time_ms, write_control,
};
use ganbaru_handoff::dial::{LoopbackPolicy, authorize_dial_target};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(15);
const CHUNK_TIMEOUT: Duration = Duration::from_secs(20);
const BUNDLE_COORDINATION_WAIT: Duration = Duration::from_secs(90);
/// How long the coordinator holds an enrollment for its user before refusing it. The enrolling
/// device waits longer, so it never gives up on a request the coordinator could still link.
pub(crate) const ENROLLMENT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
const BUNDLE_COORDINATION_POLL_INTERVAL: Duration = Duration::from_secs(1);
const MEMBERSHIP_REVOKED_CODE: &str = "membership_revoked";

#[derive(Clone, Default)]
pub(crate) struct TransferCancellation {
    cancelled: Arc<AtomicBool>,
}

impl TransferCancellation {
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
struct PinnedServerVerifier {
    fingerprint: String,
    algorithms: WebPkiSupportedAlgorithms,
}

impl PinnedServerVerifier {
    fn new(fingerprint: String) -> Self {
        let provider = rustls::crypto::ring::default_provider();
        Self {
            fingerprint,
            algorithms: provider.signature_verification_algorithms,
        }
    }
}

impl ServerCertVerifier for PinnedServerVerifier {
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
                "coordinator certificate does not match the pinned identity".to_string(),
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

async fn stream_file<W>(
    writer: &mut W,
    path: &Path,
    offset: u64,
    total_bytes: u64,
) -> Result<(), String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("open transfer bundle: {error}"))?;
    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|error| format!("seek transfer bundle: {error}"))?;
    let mut remaining = total_bytes.saturating_sub(offset);
    let mut buffer = vec![0_u8; TRANSFER_CHUNK_BYTES];
    while remaining > 0 {
        let chunk_bytes = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| "transfer chunk size overflow".to_string())?;
        tokio::time::timeout(CHUNK_TIMEOUT, file.read_exact(&mut buffer[..chunk_bytes]))
            .await
            .map_err(|_| "reading the transfer bundle timed out".to_string())?
            .map_err(|error| format!("read transfer bundle: {error}"))?;
        tokio::time::timeout(CHUNK_TIMEOUT, writer.write_all(&buffer[..chunk_bytes]))
            .await
            .map_err(|_| "sending the transfer bundle timed out".to_string())?
            .map_err(|error| format!("send transfer bundle: {error}"))?;
        remaining -= chunk_bytes as u64;
    }
    writer
        .flush()
        .await
        .map_err(|error| format!("flush transfer bundle: {error}"))
}

/// Loads the device ID and TLS identity on a blocking thread, because the first load may wait on
/// the operating-system keyring.
pub(crate) async fn load_tls_identity(
    manager: &PairingManager,
) -> Result<(String, TlsIdentity), String> {
    let manager = manager.clone();
    tokio::task::spawn_blocking(move || manager.identity())
        .await
        .map_err(|error| format!("load device linking key: {error}"))?
}

pub(crate) async fn enroll(
    manager: &PairingManager,
    invitation: &PairingInvitation,
    device_label: String,
    device_kind: super::protocol::DeviceKind,
) -> Result<(), String> {
    invitation.validate(unix_time_ms())?;
    let endpoint = invitation
        .endpoint
        .parse()
        .map_err(|_| "pairing endpoint is invalid".to_string())?;
    let (device_id, identity) = load_tls_identity(manager).await?;
    let mut stream = connect_pinned(endpoint, &invitation.coordinator_fingerprint, None).await?;
    timeout_control(write_control(
        &mut stream,
        &ControlMessage::Enroll {
            protocol_version: PROTOCOL_VERSION,
            invitation_id: invitation.invitation_id.clone(),
            secret: invitation.secret.clone(),
            vault_id: invitation.vault_id.clone(),
            device_id,
            device_label,
            device_kind,
            device_certificate: encode_certificate(&identity.certificate),
        },
    ))
    .await?;
    let response = tokio::time::timeout(
        ENROLLMENT_APPROVAL_TIMEOUT + CONTROL_TIMEOUT,
        read_control(&mut stream),
    )
    .await
    .map_err(|_| "the coordinator did not answer the enrollment".to_string())??;
    match response {
        ControlMessage::Enrolled {
            coordinator_device_id,
            coordinator_device_label,
            ..
        } if coordinator_device_id == invitation.coordinator_device_id => {
            let coordinator_certificate = stream
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|certificates| certificates.first())
                .ok_or_else(|| "coordinator did not provide a certificate".to_string())?;
            manager.record_coordinator(
                invitation,
                coordinator_certificate.as_ref(),
                coordinator_device_label,
            )
        }
        ControlMessage::Error { message, .. } => Err(message),
        _ => Err("coordinator returned an invalid enrollment response".to_string()),
    }
}

pub(crate) async fn request_bundle(
    manager: &PairingManager,
    compatibility: super::protocol::HandoffCompatibility,
    generation: u64,
    purpose: BundlePurpose,
    cancellation: &TransferCancellation,
) -> Result<BundleMetadata, String> {
    let coordinator = manager
        .coordinator_pin()?
        .ok_or_else(|| "this device is not linked to a coordinator".to_string())?;
    let device_id = manager.device_id()?;
    let started = tokio::time::Instant::now();
    loop {
        if cancellation.is_cancelled() {
            return Err("transfer was cancelled".to_string());
        }
        match authenticated_exchange(
            manager,
            ControlMessage::RequestBundle {
                protocol_version: PROTOCOL_VERSION,
                compatibility: compatibility.clone(),
                vault_id: coordinator.vault_id.clone(),
                device_id: device_id.clone(),
                generation,
                purpose,
            },
        )
        .await?
        {
            ControlMessage::BundlePrepared {
                metadata,
                purpose: offered_purpose,
            } if offered_purpose == purpose
                && metadata.vault_id == coordinator.vault_id
                && metadata.device_id == device_id
                && metadata.generation >= generation
                && (purpose != BundlePurpose::Ownership || metadata.generation > generation) =>
            {
                return Ok(metadata);
            }
            ControlMessage::BundlePending { .. }
                if started.elapsed() < BUNDLE_COORDINATION_WAIT =>
            {
                tokio::time::sleep(BUNDLE_COORDINATION_POLL_INTERVAL).await;
            }
            ControlMessage::BundlePending { .. } => {
                return Err("the current owner did not return the vault in time".to_string());
            }
            ControlMessage::Error { message, .. } => return Err(message),
            _ => return Err("coordinator returned an invalid prepared bundle".to_string()),
        }
    }
}

pub(crate) async fn commit_staged_ownership(
    manager: &PairingManager,
    metadata: BundleMetadata,
) -> Result<(String, u64), String> {
    match authenticated_exchange(
        manager,
        ControlMessage::CommitStagedOwnership {
            metadata: metadata.clone(),
        },
    )
    .await?
    {
        ControlMessage::OwnershipGrant {
            vault_id,
            transfer_id,
            owner_device_id,
            generation,
            ..
        } if vault_id == metadata.vault_id
            && transfer_id == metadata.transfer_id
            && owner_device_id == metadata.device_id
            && generation == metadata.generation =>
        {
            Ok((owner_device_id, generation))
        }
        ControlMessage::Error { message, .. } => Err(message),
        _ => Err("coordinator returned an invalid ownership grant".to_string()),
    }
}

pub(crate) async fn cancel_prepared_transfer(
    manager: &PairingManager,
    transfer_id: &str,
) -> Result<(), String> {
    match authenticated_exchange(
        manager,
        ControlMessage::CancelTransfer {
            transfer_id: transfer_id.to_string(),
        },
    )
    .await?
    {
        ControlMessage::TransferCancelled {
            transfer_id: cancelled,
        } if cancelled == transfer_id => Ok(()),
        ControlMessage::Error { message, .. } => Err(message),
        _ => Err("coordinator returned an invalid cancellation response".to_string()),
    }
}

pub(crate) async fn acknowledge_activation(
    manager: &PairingManager,
    pending: &super::pairing::PendingAcknowledgement,
) -> Result<(), String> {
    match authenticated_exchange(
        manager,
        ControlMessage::ActivationComplete {
            protocol_version: PROTOCOL_VERSION,
            vault_id: pending.vault_id.clone(),
            device_id: pending.device_id.clone(),
            transfer_id: pending.transfer_id.clone(),
            generation: pending.generation,
            purpose: pending.purpose,
        },
    )
    .await?
    {
        ControlMessage::ActivationAcknowledged { transfer_id }
            if transfer_id == pending.transfer_id =>
        {
            Ok(())
        }
        ControlMessage::Error { message, .. } => Err(message),
        _ => Err("coordinator returned an invalid activation acknowledgement".to_string()),
    }
}

pub(crate) async fn upload_bundle(
    manager: &PairingManager,
    metadata: BundleMetadata,
    source_device_id: String,
    purpose: BundlePurpose,
) -> Result<(), String> {
    let coordinator = manager
        .coordinator_pin()?
        .ok_or_else(|| "this device is not linked to a coordinator".to_string())?;
    if metadata.device_id != coordinator.device_id || metadata.vault_id != coordinator.vault_id {
        return Err("outgoing bundle does not target the linked coordinator".to_string());
    }
    let (_, archive_path) = manager.outgoing_snapshot_paths(&metadata.transfer_id)?;
    super::protocol::validate_staging_file(&archive_path, &metadata)?;
    let (_, identity) = load_tls_identity(manager).await?;
    let mut stream = dial_coordinator(manager, &coordinator, identity).await?;
    timeout_control(write_control(
        &mut stream,
        &ControlMessage::UploadBundle {
            metadata: metadata.clone(),
            source_device_id,
            purpose,
        },
    ))
    .await?;
    let offset = match read_authenticated_response(manager, &coordinator, &mut stream).await? {
        ControlMessage::ResumeAt { offset } if offset <= metadata.archive_bytes => offset,
        ControlMessage::Error { message, .. } => return Err(message),
        _ => return Err("coordinator returned an invalid upload offset".to_string()),
    };
    stream_file(&mut stream, &archive_path, offset, metadata.archive_bytes).await?;
    timeout_control(write_control(
        &mut stream,
        &ControlMessage::BundleComplete {
            transfer_id: metadata.transfer_id.clone(),
        },
    ))
    .await?;
    match read_authenticated_response(manager, &coordinator, &mut stream).await? {
        ControlMessage::BundleStaged { transfer_id } if transfer_id == metadata.transfer_id => {
            Ok(())
        }
        ControlMessage::Error { message, .. } => Err(message),
        _ => Err("coordinator did not accept the uploaded bundle".to_string()),
    }
}

pub(crate) async fn commit_uploaded_ownership(
    manager: &PairingManager,
    metadata: BundleMetadata,
    source_device_id: String,
) -> Result<(), String> {
    match authenticated_exchange(
        manager,
        ControlMessage::CommitUploadedOwnership {
            metadata: metadata.clone(),
            source_device_id,
        },
    )
    .await?
    {
        ControlMessage::ActivationAcknowledged { transfer_id }
            if transfer_id == metadata.transfer_id =>
        {
            Ok(())
        }
        ControlMessage::Error { message, .. } => Err(message),
        _ => Err("coordinator returned an invalid ownership acknowledgement".to_string()),
    }
}

pub(crate) async fn probe_coordinator(
    manager: &PairingManager,
    compatibility: super::protocol::HandoffCompatibility,
    generation: u64,
) -> Result<Option<BundlePurpose>, String> {
    let coordinator = manager
        .coordinator_pin()?
        .ok_or_else(|| "this device is not linked to a coordinator".to_string())?;
    let device_id = manager.device_id()?;
    match authenticated_exchange(
        manager,
        ControlMessage::RefreshRequest {
            protocol_version: PROTOCOL_VERSION,
            compatibility,
            vault_id: coordinator.vault_id,
            device_id,
            generation,
        },
    )
    .await?
    {
        ControlMessage::RefreshStatus {
            generation: returned,
            requested_upload,
            ..
        } if returned >= generation => Ok(requested_upload),
        ControlMessage::Error { message, .. } => Err(message),
        _ => Err("coordinator returned an invalid refresh status".to_string()),
    }
}

#[cfg(any(target_os = "android", all(test, not(target_os = "ios"))))]
pub(crate) async fn exchange_distractions(
    manager: &PairingManager,
    samples: Vec<super::protocol::DistractionsSampleMessage>,
    acknowledged_peer_sample_ids: Vec<String>,
    owner_snapshot: Vec<super::protocol::DistractionsSampleMessage>,
) -> Result<
    (
        Vec<String>,
        Vec<super::protocol::DistractionsSampleMessage>,
        Vec<super::protocol::DistractionsSampleMessage>,
    ),
    String,
> {
    let coordinator = manager
        .coordinator_pin()?
        .ok_or_else(|| "this device is not linked to a coordinator".to_string())?;
    let device_id = manager.device_id()?;
    match authenticated_exchange(
        manager,
        ControlMessage::DistractionsExchange {
            protocol_version: PROTOCOL_VERSION,
            vault_id: coordinator.vault_id,
            device_id,
            samples,
            acknowledged_peer_sample_ids,
            owner_snapshot,
        },
    )
    .await?
    {
        ControlMessage::DistractionsAcknowledged {
            acknowledged_sample_ids,
            peer_samples,
            combined_samples,
        } => Ok((acknowledged_sample_ids, peer_samples, combined_samples)),
        ControlMessage::Error { message, .. } => Err(message),
        _ => Err("coordinator returned an invalid Distractions response".to_string()),
    }
}

/// Sends one Contacts message to a coordinator reached through a contact card hint. The
/// connection pins the hinted certificate but presents no client identity.
pub(crate) async fn unauthenticated_exchange(
    endpoint: &str,
    certificate_fingerprint: &str,
    message: ControlMessage,
) -> Result<ControlMessage, String> {
    let endpoint = endpoint
        .parse()
        .map_err(|_| "contact endpoint is invalid".to_string())?;
    let mut stream = connect_pinned(endpoint, certificate_fingerprint, None).await?;
    timeout_control(write_control(&mut stream, &message)).await?;
    timeout_control(read_control(&mut stream)).await
}

/// Asks the coordinator for the person's signing key over the linked-device identity.
pub(crate) async fn request_person_key(manager: &PairingManager) -> Result<ControlMessage, String> {
    let coordinator = manager
        .coordinator_pin()?
        .ok_or_else(|| "this device is not linked to a coordinator".to_string())?;
    let device_id = manager.device_id()?;
    authenticated_exchange(
        manager,
        ControlMessage::PersonKeyRequest {
            protocol_version: PROTOCOL_VERSION,
            vault_id: coordinator.vault_id,
            device_id,
        },
    )
    .await
}

/// The vault and device ids a sync exchange presents, or `None` on a device that is not linked
/// to a coordinator.
pub(crate) fn sync_peer(manager: &PairingManager) -> Result<Option<(String, String)>, String> {
    let Some(coordinator) = manager.coordinator_pin()? else {
        return Ok(None);
    };
    let device_id = manager.device_id()?;
    Ok(Some((coordinator.vault_id, device_id)))
}

/// Sends one sync message to the coordinator over the linked-device identity. A sync state
/// response also refreshes the coordinator endpoints and discovery key this device remembers.
pub(crate) async fn sync_request(
    manager: &PairingManager,
    message: ControlMessage,
) -> Result<ControlMessage, String> {
    let response = authenticated_exchange(manager, message).await?;
    if let ControlMessage::SyncState {
        coordinator_endpoints,
        discovery_key,
        ..
    } = &response
    {
        record_reported_contact(manager, coordinator_endpoints, discovery_key);
    }
    Ok(response)
}

/// Remembers the contact a coordinator reported in a sync state. A failure only delays address
/// refresh until the next exchange, so it is logged instead of failing the sync.
fn record_reported_contact(manager: &PairingManager, endpoints: &[String], discovery_key: &str) {
    let recorded = manager.coordinator_pin().and_then(|coordinator| {
        let Some(coordinator) = coordinator else {
            return Ok(false);
        };
        manager.record_coordinator_contact(
            &coordinator.vault_id,
            &coordinator.certificate_fingerprint,
            super::pairing::CoordinatorContact {
                last_endpoint: None,
                reported_endpoints: Some(endpoints.to_vec()),
                discovery_key: Some(discovery_key.to_string()),
            },
        )
    });
    if let Err(error) = recorded {
        eprintln!("record the coordinator contact from a sync state: {error}");
    }
}

async fn authenticated_exchange(
    manager: &PairingManager,
    message: ControlMessage,
) -> Result<ControlMessage, String> {
    let coordinator = manager
        .coordinator_pin()?
        .ok_or_else(|| "this device is not linked to a coordinator".to_string())?;
    let (_, identity) = load_tls_identity(manager).await?;
    let mut stream = dial_coordinator(manager, &coordinator, identity).await?;
    timeout_control(write_control(&mut stream, &message)).await?;
    read_authenticated_response(manager, &coordinator, &mut stream).await
}

async fn read_authenticated_response<R>(
    manager: &PairingManager,
    coordinator: &CoordinatorPin,
    reader: &mut R,
) -> Result<ControlMessage, String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let response = timeout_control(read_control(reader)).await?;
    if matches!(
        &response,
        ControlMessage::Error { code, .. } if code == MEMBERSHIP_REVOKED_CODE
    ) {
        manager.accept_coordinator_revocation(coordinator)?;
    }
    Ok(response)
}

pub(crate) async fn download_bundle(
    manager: &PairingManager,
    metadata: BundleMetadata,
    cancellation: &TransferCancellation,
) -> Result<PathBuf, String> {
    download_bundle_inner(manager, metadata, cancellation, None).await
}

#[cfg(all(test, desktop))]
pub(crate) mod tests;

async fn download_bundle_inner(
    manager: &PairingManager,
    metadata: BundleMetadata,
    cancellation: &TransferCancellation,
    stop_after_bytes: Option<u64>,
) -> Result<PathBuf, String> {
    metadata.validate()?;
    let coordinator = manager
        .coordinator_pin()?
        .ok_or_else(|| "this device is not linked to a coordinator".to_string())?;
    if coordinator.vault_id != metadata.vault_id {
        return Err("transfer belongs to a different vault".to_string());
    }
    let (_, identity) = load_tls_identity(manager).await?;
    let mut stream = dial_coordinator(manager, &coordinator, identity).await?;
    timeout_control(write_control(
        &mut stream,
        &ControlMessage::DownloadBundle {
            metadata: metadata.clone(),
        },
    ))
    .await?;
    match read_authenticated_response(manager, &coordinator, &mut stream).await? {
        ControlMessage::BundleMetadata { metadata: offered } if offered == metadata => {}
        ControlMessage::Error { message, .. } => return Err(message),
        _ => return Err("coordinator returned inconsistent transfer metadata".to_string()),
    }

    let (partial_path, metadata_path, complete_path) =
        manager.staging_paths(&metadata.transfer_id)?;
    if complete_path.exists() {
        super::protocol::validate_staging_file(&complete_path, &metadata)?;
        return Ok(complete_path);
    }
    let resume_offset = prepare_partial(manager, &partial_path, &metadata_path, &metadata)?;
    timeout_control(write_control(
        &mut stream,
        &ControlMessage::ResumeAt {
            offset: resume_offset,
        },
    ))
    .await?;

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&partial_path)
        .await
        .map_err(|error| format!("open partial transfer: {error}"))?;
    let mut received = resume_offset;
    let mut remaining = metadata.archive_bytes.saturating_sub(resume_offset);
    let mut buffer = vec![0_u8; TRANSFER_CHUNK_BYTES];
    while remaining > 0 {
        if cancellation.is_cancelled() {
            return Err("transfer was cancelled".to_string());
        }
        if stop_after_bytes.is_some_and(|limit| received >= limit) {
            return Err("transfer was interrupted".to_string());
        }
        let chunk_bytes = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| "transfer chunk size overflow".to_string())?;
        let read_bytes =
            tokio::time::timeout(CHUNK_TIMEOUT, stream.read(&mut buffer[..chunk_bytes]))
                .await
                .map_err(|_| "receiving the transfer bundle timed out".to_string())?
                .map_err(|error| format!("receive transfer bundle: {error}"))?;
        if read_bytes == 0 {
            return Err("transfer was interrupted".to_string());
        }
        file.write_all(&buffer[..read_bytes])
            .await
            .map_err(|error| format!("write partial transfer: {error}"))?;
        received += read_bytes as u64;
        remaining -= read_bytes as u64;
    }
    file.sync_all()
        .await
        .map_err(|error| format!("sync partial transfer: {error}"))?;
    drop(file);

    if let Err(error) = super::protocol::validate_staging_file(&partial_path, &metadata) {
        let _ = manager.remove_staging(&metadata.transfer_id);
        return Err(error);
    }
    fs_rename(&partial_path, &complete_path)?;
    let _ = std::fs::remove_file(&metadata_path);
    timeout_control(write_control(
        &mut stream,
        &ControlMessage::BundleComplete {
            transfer_id: metadata.transfer_id.clone(),
        },
    ))
    .await?;
    Ok(complete_path)
}

fn prepare_partial(
    manager: &PairingManager,
    partial_path: &Path,
    metadata_path: &Path,
    metadata: &BundleMetadata,
) -> Result<u64, String> {
    let stored = manager.read_staging_metadata(metadata_path)?;
    let partial_bytes = partial_path
        .metadata()
        .map(|value| value.len())
        .unwrap_or(0);
    if stored.as_ref().is_some_and(|stored| stored == metadata)
        && partial_bytes <= metadata.archive_bytes
    {
        return Ok(partial_bytes);
    }
    manager.remove_staging(&metadata.transfer_id)?;
    manager.write_staging_metadata(metadata_path, metadata)?;
    Ok(0)
}

/// Loopback peers exist only when two development instances link on one machine, so release
/// builds neither dial nor admit them.
const LOOPBACK_PEERS: LoopbackPolicy = if cfg!(debug_assertions) {
    LoopbackPolicy::Allow
} else {
    LoopbackPolicy::Reject
};

/// Refuses dial targets outside the local network before any packet is sent. Invitations,
/// cards, and coordinator pins all reach the network through [`connect_pinned`].
fn authorize_endpoint(endpoint: SocketAddr) -> Result<(), String> {
    let on_link_source = match endpoint.ip() {
        IpAddr::V6(target) => ipv6_source_for(target),
        IpAddr::V4(_) => None,
    };
    authorize_dial_target(endpoint, LOOPBACK_PEERS, on_link_source)
        .map_err(|error| format!("refuse device connection: {error}"))
}

/// Returns the IPv6 source address the operating system selects to reach `target`. Connecting
/// a UDP socket only selects a route; it sends nothing.
fn ipv6_source_for(target: Ipv6Addr) -> Option<Ipv6Addr> {
    let socket = std::net::UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((target, 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V6(source) => Some(source),
        IpAddr::V4(_) => None,
    }
}

type PinnedStream = tokio_rustls::client::TlsStream<TcpStream>;

/// Connects to one endpoint whose certificate must match `fingerprint`, such as an invitation or
/// a contact card. Linked devices reach their coordinator through [`connect_coordinator`].
async fn connect_pinned(
    endpoint: SocketAddr,
    fingerprint: &str,
    identity: Option<TlsIdentity>,
) -> Result<PinnedStream, String> {
    authorize_endpoint(endpoint)?;
    let config = pinned_client_config(fingerprint, identity)?;
    pinned_handshake(endpoint, config, TLS_HANDSHAKE_TIMEOUT).await
}

/// TLS 1.3 client configuration that accepts only the certificate with `fingerprint` and
/// presents `identity` when one is given.
fn pinned_client_config(
    fingerprint: &str,
    identity: Option<TlsIdentity>,
) -> Result<Arc<rustls::ClientConfig>, String> {
    let verifier = Arc::new(PinnedServerVerifier::new(fingerprint.to_string()));
    let builder = rustls::ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
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

/// Opens a TCP connection to an authorized `endpoint` within `connect_timeout` and completes the
/// pinned TLS handshake.
async fn pinned_handshake(
    endpoint: SocketAddr,
    config: Arc<rustls::ClientConfig>,
    connect_timeout: Duration,
) -> Result<PinnedStream, String> {
    let socket = tokio::time::timeout(connect_timeout, TcpStream::connect(endpoint))
        .await
        .map_err(|_| "coordinator connection timed out".to_string())?
        .map_err(|error| format!("connect to coordinator: {error}"))?;
    let server_name = ServerName::try_from("ganbaru-coordinator.local")
        .map_err(|_| "coordinator TLS name is invalid".to_string())?;
    tokio::time::timeout(
        TLS_HANDSHAKE_TIMEOUT,
        TlsConnector::from(config).connect(server_name, socket),
    )
    .await
    .map_err(|_| "coordinator TLS handshake timed out".to_string())?
    .map_err(|error| format!("establish pinned TLS connection: {error}"))
}

/// How long one coordinator candidate gets to accept a TCP connection.
const COORDINATOR_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// How long the resolver waits on pending candidates before it also dials the next one.
const COORDINATOR_DIAL_STAGGER: Duration = Duration::from_millis(250);
/// Most coordinator candidates one dial considers.
const MAX_COORDINATOR_CANDIDATES: usize = 12;
/// Most coordinator candidates dialed at once, kept below the coordinator's pending connection
/// limit per source address so a race never refuses itself.
const MAX_CONCURRENT_COORDINATOR_DIALS: usize = 4;

/// Connects to the pinned coordinator over the first candidate endpoint that completes the
/// pinned handshake, and remembers that endpoint for the next dial.
///
/// Candidates are the last endpoint that worked, the endpoints the coordinator reported, the
/// `discovered` endpoints, and the invitation endpoint, in that order. A new candidate is dialed
/// whenever the pending ones have not finished within [`COORDINATOR_DIAL_STAGGER`] or one fails,
/// so a stale address costs a short delay instead of a full timeout.
///
/// When every candidate fails, it awaits `discover` once and dials the endpoints it returns that
/// were not tried yet. `discover` does nothing until then.
async fn connect_coordinator(
    manager: &PairingManager,
    coordinator: &CoordinatorPin,
    identity: TlsIdentity,
    discovered: &[SocketAddr],
    discover: impl std::future::Future<Output = Vec<SocketAddr>>,
) -> Result<PinnedStream, String> {
    let (candidates, mut failures) =
        coordinator_candidates(coordinator, discovered, authorize_endpoint);
    let config = pinned_client_config(&coordinator.certificate_fingerprint, Some(identity))?;
    let mut reached =
        race_coordinator_candidates(candidates.clone(), config.clone(), &mut failures).await;
    if reached.is_none() {
        let discovered = discover.await;
        let untried = untried_discovered_endpoints(
            &discovered,
            &candidates,
            authorize_endpoint,
            &mut failures,
        );
        if !untried.is_empty() {
            reached = race_coordinator_candidates(untried, config, &mut failures).await;
        }
    }
    finish_coordinator_dial(manager, coordinator, reached, failures)
}

/// Connects to the pinned coordinator through [`connect_coordinator`] with the endpoints a recent
/// discovery found, browsing the local network for it when no candidate answers.
async fn dial_coordinator(
    manager: &PairingManager,
    coordinator: &CoordinatorPin,
    identity: TlsIdentity,
) -> Result<PinnedStream, String> {
    let remembered = remembered_discovered_endpoints(coordinator).await;
    connect_coordinator(
        manager,
        coordinator,
        identity,
        &remembered,
        discover_coordinator_endpoints(coordinator),
    )
    .await
}

/// Endpoints a recent discovery found for `coordinator`.
#[cfg(desktop)]
async fn remembered_discovered_endpoints(coordinator: &CoordinatorPin) -> Vec<SocketAddr> {
    super::discovery::remembered_endpoints(coordinator).await
}

#[cfg(not(desktop))]
async fn remembered_discovered_endpoints(_coordinator: &CoordinatorPin) -> Vec<SocketAddr> {
    Vec::new()
}

/// Browses the local network for `coordinator` when its pin allows it.
#[cfg(desktop)]
async fn discover_coordinator_endpoints(coordinator: &CoordinatorPin) -> Vec<SocketAddr> {
    super::discovery::discover_coordinator(coordinator).await
}

#[cfg(not(desktop))]
async fn discover_coordinator_endpoints(_coordinator: &CoordinatorPin) -> Vec<SocketAddr> {
    Vec::new()
}

/// The `discovered` endpoints that are not among the `tried` ones, in order and without
/// repeats. Endpoints `authorize` refuses are recorded in `failures` and never dialed.
fn untried_discovered_endpoints(
    discovered: &[SocketAddr],
    tried: &[SocketAddr],
    authorize: impl Fn(SocketAddr) -> Result<(), String>,
    failures: &mut Vec<DialFailure>,
) -> Vec<SocketAddr> {
    let mut untried = Vec::new();
    for endpoint in discovered {
        if tried.contains(endpoint) || untried.contains(endpoint) {
            continue;
        }
        match authorize(*endpoint) {
            Ok(()) => untried.push(*endpoint),
            Err(error) => failures.push(DialFailure {
                endpoint: endpoint.to_string(),
                error,
            }),
        }
    }
    untried
}

/// Remembers the endpoint that answered for the next dial, or reports why none did.
fn finish_coordinator_dial(
    manager: &PairingManager,
    coordinator: &CoordinatorPin,
    reached: Option<(PinnedStream, SocketAddr)>,
    failures: Vec<DialFailure>,
) -> Result<PinnedStream, String> {
    let (stream, endpoint) = reached.ok_or_else(|| coordinator_dial_error(failures))?;
    let contact = super::pairing::CoordinatorContact {
        last_endpoint: Some(endpoint.to_string()),
        ..Default::default()
    };
    if let Err(error) = manager.record_coordinator_contact(
        &coordinator.vault_id,
        &coordinator.certificate_fingerprint,
        contact,
    ) {
        eprintln!("record the coordinator endpoint that answered: {error}");
    }
    Ok(stream)
}

/// Why one coordinator endpoint was not used.
#[derive(Debug)]
struct DialFailure {
    endpoint: String,
    error: String,
}

/// The distinct endpoints worth dialing for `coordinator`, best first, and the reasons the
/// others were left out. Endpoints `authorize` refuses are never dialed.
fn coordinator_candidates(
    coordinator: &CoordinatorPin,
    discovered: &[SocketAddr],
    authorize: impl Fn(SocketAddr) -> Result<(), String>,
) -> (Vec<SocketAddr>, Vec<DialFailure>) {
    let stored = coordinator
        .last_endpoint
        .iter()
        .chain(&coordinator.reported_endpoints)
        .map(|endpoint| (endpoint.clone(), endpoint.parse::<SocketAddr>()));
    let discovered = discovered
        .iter()
        .map(|endpoint| (endpoint.to_string(), Ok(*endpoint)));
    let invitation = std::iter::once((
        coordinator.endpoint.clone(),
        coordinator.endpoint.parse::<SocketAddr>(),
    ));
    let mut candidates = Vec::new();
    let mut failures = Vec::new();
    for (text, parsed) in stored.chain(discovered).chain(invitation) {
        let Ok(endpoint) = parsed else {
            failures.push(DialFailure {
                endpoint: text,
                error: "coordinator endpoint is invalid".to_string(),
            });
            continue;
        };
        if candidates.contains(&endpoint) {
            continue;
        }
        if candidates.len() == MAX_COORDINATOR_CANDIDATES {
            break;
        }
        match authorize(endpoint) {
            Ok(()) => candidates.push(endpoint),
            Err(error) => failures.push(DialFailure {
                endpoint: text,
                error,
            }),
        }
    }
    (candidates, failures)
}

/// Dials `candidates` with staggered starts and returns the first pinned stream, or `None` once
/// every candidate failed, with each failure appended to `failures`. Dropping the pending dials
/// closes their connections.
async fn race_coordinator_candidates(
    candidates: Vec<SocketAddr>,
    config: Arc<rustls::ClientConfig>,
    failures: &mut Vec<DialFailure>,
) -> Option<(PinnedStream, SocketAddr)> {
    let mut queued = candidates.into_iter();
    let mut dials = tokio::task::JoinSet::new();
    loop {
        if dials.len() < MAX_CONCURRENT_COORDINATOR_DIALS
            && let Some(endpoint) = queued.next()
        {
            let config = config.clone();
            dials.spawn(async move {
                let result = pinned_handshake(endpoint, config, COORDINATOR_CONNECT_TIMEOUT).await;
                (endpoint, result)
            });
        }
        let can_dial_more =
            !queued.as_slice().is_empty() && dials.len() < MAX_CONCURRENT_COORDINATOR_DIALS;
        tokio::select! {
            finished = dials.join_next() => match finished {
                None => return None,
                Some(Ok((endpoint, Ok(stream)))) => return Some((stream, endpoint)),
                Some(Ok((endpoint, Err(error)))) => failures.push(DialFailure {
                    endpoint: endpoint.to_string(),
                    error,
                }),
                Some(Err(error)) => failures.push(DialFailure {
                    endpoint: "coordinator dial".to_string(),
                    error: format!("dial task stopped: {error}"),
                }),
            },
            () = tokio::time::sleep(COORDINATOR_DIAL_STAGGER), if can_dial_more => {}
        }
    }
}

/// The error for a coordinator no candidate reached. A single failure keeps its own message so
/// callers can still recognize it; several are joined behind a summary.
fn coordinator_dial_error(mut failures: Vec<DialFailure>) -> String {
    match failures.len() {
        0 => "the coordinator has no endpoint to connect to".to_string(),
        1 => failures.remove(0).error,
        _ => {
            let details = failures
                .iter()
                .map(|failure| format!("{}: {}", failure.endpoint, failure.error))
                .collect::<Vec<_>>()
                .join("; ");
            format!("could not connect to the coordinator at any known endpoint: {details}")
        }
    }
}

async fn timeout_control<T>(
    future: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::time::timeout(CONTROL_TIMEOUT, future)
        .await
        .map_err(|_| "handoff control message timed out".to_string())?
}

fn fs_rename(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::rename(source, target).map_err(|error| format!("finalize staged transfer: {error}"))
}
