//! Pinned client transport and shared resumable archive streaming.

#[cfg(desktop)]
mod server;
#[cfg(desktop)]
pub(crate) use server::serve_with_coordinator;

use super::pairing::{
    CoordinatorPin, PairingManager, TlsIdentity, certificate_fingerprint, encode_certificate,
};
use super::protocol::{
    BundleMetadata, BundlePurpose, ControlMessage, PROTOCOL_VERSION, PairingInvitation,
    TRANSFER_CHUNK_BYTES, read_control, unix_time_ms, write_control,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
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
    let (device_id, identity) = manager.identity()?;
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
    match timeout_control(read_control(&mut stream)).await? {
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
    let (device_id, _) = manager.identity()?;
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
    let endpoint = coordinator
        .endpoint
        .parse()
        .map_err(|_| "coordinator endpoint is invalid".to_string())?;
    let (_, identity) = manager.identity()?;
    let mut stream = connect_pinned(
        endpoint,
        &coordinator.certificate_fingerprint,
        Some(identity),
    )
    .await?;
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
    let (device_id, _) = manager.identity()?;
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
    let (device_id, _) = manager.identity()?;
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
    let (device_id, _) = manager.identity()?;
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
    let (device_id, _) = manager.identity()?;
    Ok(Some((coordinator.vault_id, device_id)))
}

/// Sends one sync message to the coordinator over the linked-device identity.
pub(crate) async fn sync_request(
    manager: &PairingManager,
    message: ControlMessage,
) -> Result<ControlMessage, String> {
    authenticated_exchange(manager, message).await
}

async fn authenticated_exchange(
    manager: &PairingManager,
    message: ControlMessage,
) -> Result<ControlMessage, String> {
    let coordinator = manager
        .coordinator_pin()?
        .ok_or_else(|| "this device is not linked to a coordinator".to_string())?;
    let endpoint = coordinator
        .endpoint
        .parse()
        .map_err(|_| "coordinator endpoint is invalid".to_string())?;
    let (_, identity) = manager.identity()?;
    let mut stream = connect_pinned(
        endpoint,
        &coordinator.certificate_fingerprint,
        Some(identity),
    )
    .await?;
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
mod tests;

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
    let endpoint = coordinator
        .endpoint
        .parse()
        .map_err(|_| "coordinator endpoint is invalid".to_string())?;
    let (_, identity) = manager.identity()?;
    let mut stream = connect_pinned(
        endpoint,
        &coordinator.certificate_fingerprint,
        Some(identity),
    )
    .await?;
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

async fn connect_pinned(
    endpoint: std::net::SocketAddr,
    fingerprint: &str,
    identity: Option<TlsIdentity>,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, String> {
    let verifier = Arc::new(PinnedServerVerifier::new(fingerprint.to_string()));
    let builder = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier);
    let config = match identity {
        Some(identity) => builder
            .with_client_auth_cert(vec![identity.certificate], identity.private_key.clone_key())
            .map_err(|error| format!("configure device TLS identity: {error}"))?,
        None => builder.with_no_client_auth(),
    };
    let socket = tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, TcpStream::connect(endpoint))
        .await
        .map_err(|_| "coordinator connection timed out".to_string())?
        .map_err(|error| format!("connect to coordinator: {error}"))?;
    let server_name = ServerName::try_from("ganbaru-coordinator.local")
        .map_err(|_| "coordinator TLS name is invalid".to_string())?;
    tokio::time::timeout(
        TLS_HANDSHAKE_TIMEOUT,
        TlsConnector::from(Arc::new(config)).connect(server_name, socket),
    )
    .await
    .map_err(|_| "coordinator TLS handshake timed out".to_string())?
    .map_err(|error| format!("establish pinned TLS connection: {error}"))
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
