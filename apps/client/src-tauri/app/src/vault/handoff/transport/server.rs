//! Desktop coordinator listener, authenticated dispatch, and archive serving.

use super::{
    CHUNK_TIMEOUT, MEMBERSHIP_REVOKED_CODE, TLS_HANDSHAKE_TIMEOUT, prepare_partial, stream_file,
    timeout_control,
};
use crate::sync::hub::SyncHub;
use crate::vault::handoff::pairing::{Enrollment, PairingManager};
use crate::vault::handoff::protocol::{
    BundleMetadata, BundlePurpose, ControlMessage, PROTOCOL_VERSION, TRANSFER_CHUNK_BYTES,
    read_control, unix_time_ms, write_control,
};
use crate::vault::handoff::{coordinator, protocol, suggested_device_label};
use rustls::RootCertStore;
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

const UPLOAD_REQUEST_WAIT: Duration = Duration::from_secs(10);
const UPLOAD_REQUEST_POLL_INTERVAL: Duration = Duration::from_millis(200);

pub(crate) async fn serve_with_coordinator(
    listener: TcpListener,
    manager: PairingManager,
    coordinator: Option<coordinator::CoordinatorSender>,
    hub: Option<SyncHub>,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                match accepted {
                    Ok((socket, _)) => {
                        let manager = manager.clone();
                        let coordinator = coordinator.clone();
                        let hub = hub.clone();
                        tokio::spawn(async move {
                            if let Err(error) = handle_connection(socket, manager, coordinator, hub).await {
                                eprintln!("vault handoff connection failed: {error}");
                            }
                        });
                    }
                    Err(error) => {
                        eprintln!("vault handoff coordinator accept failed: {error}");
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_connection(
    socket: TcpStream,
    manager: PairingManager,
    coordinator: Option<coordinator::CoordinatorSender>,
    hub: Option<SyncHub>,
) -> Result<(), String> {
    let config = server_config(&manager)?;
    let mut stream = tokio::time::timeout(
        TLS_HANDSHAKE_TIMEOUT,
        TlsAcceptor::from(Arc::new(config)).accept(socket),
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
    let request = match timeout_control(read_control(&mut stream)).await {
        Ok(request) => request,
        Err(error) => {
            return send_error(&mut stream, "malformed_control", &error, false).await;
        }
    };

    if manager.is_revoked_certificate(peer_certificate.as_ref())? {
        return send_error(
            &mut stream,
            MEMBERSHIP_REVOKED_CODE,
            "this device was unlinked by the coordinator",
            false,
        )
        .await;
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

    match request {
        ControlMessage::Enroll {
            invitation_id,
            secret,
            vault_id,
            device_id,
            device_label,
            device_kind,
            device_certificate,
            ..
        } => {
            if peer_certificate.is_some() {
                return send_error(
                    &mut stream,
                    "enrollment_identity",
                    "enrollment must use the invitation identity",
                    false,
                )
                .await;
            }
            if let Err(error) = manager.enroll_peer(
                Enrollment {
                    invitation_id: &invitation_id,
                    secret: &secret,
                    vault_id: &vault_id,
                    device_id: &device_id,
                    device_label: &device_label,
                    device_kind,
                    certificate_b64: &device_certificate,
                },
                unix_time_ms(),
            ) {
                return send_error(&mut stream, "enrollment_rejected", &error, false).await;
            }
            let (coordinator_device_id, _) = manager.identity()?;
            let coordinator_device_label = suggested_device_label();
            timeout_control(write_control(
                &mut stream,
                &ControlMessage::Enrolled {
                    protocol_version: PROTOCOL_VERSION,
                    coordinator_device_id,
                    coordinator_device_label: Some(coordinator_device_label),
                },
            ))
            .await
        }
        ControlMessage::ContactRequest {
            recipient_card_nonce,
            requester_card,
            request_id,
            signature,
            ..
        } => {
            send_coordinator_response(
                &mut stream,
                coordinator.as_ref(),
                coordinator::CoordinatorOperation::ContactRequest {
                    recipient_card_nonce,
                    requester_card,
                    request_id,
                    signature,
                },
            )
            .await
        }
        ControlMessage::ContactRequestStatus {
            request_id,
            requester_public_key,
            issued_at_ms,
            signature,
            ..
        } => {
            send_coordinator_response(
                &mut stream,
                coordinator.as_ref(),
                coordinator::CoordinatorOperation::ContactRequestStatus {
                    request_id,
                    requester_public_key,
                    issued_at_ms,
                    signature,
                },
            )
            .await
        }
        ControlMessage::PersonKeyRequest {
            vault_id,
            device_id,
            ..
        } => {
            manager.verify_authenticated_peer(&device_id, peer_certificate.as_ref(), &vault_id)?;
            send_coordinator_response(
                &mut stream,
                coordinator.as_ref(),
                coordinator::CoordinatorOperation::ReleasePersonKey,
            )
            .await
        }
        ControlMessage::DownloadBundle { metadata } => {
            manager.verify_authenticated_peer(
                &metadata.device_id,
                peer_certificate.as_ref(),
                &metadata.vault_id,
            )?;
            serve_download(&mut stream, &manager, metadata).await
        }
        ControlMessage::UploadBundle {
            metadata,
            source_device_id,
            purpose,
        } => {
            manager.verify_authenticated_peer(
                &source_device_id,
                peer_certificate.as_ref(),
                &metadata.vault_id,
            )?;
            serve_upload(
                &mut stream,
                &manager,
                coordinator.as_ref(),
                metadata,
                source_device_id,
                purpose,
            )
            .await
        }
        ControlMessage::RequestBundle {
            compatibility,
            vault_id,
            device_id,
            generation,
            purpose,
            ..
        } => {
            manager.verify_authenticated_peer(&device_id, peer_certificate.as_ref(), &vault_id)?;
            let operation = coordinator::CoordinatorOperation::Prepare {
                compatibility,
                vault_id,
                device_id,
                generation,
                purpose,
            };
            send_coordinator_response(&mut stream, coordinator.as_ref(), operation).await
        }
        ControlMessage::CommitStagedOwnership { metadata } => {
            manager.verify_authenticated_peer(
                &metadata.device_id,
                peer_certificate.as_ref(),
                &metadata.vault_id,
            )?;
            send_coordinator_response(
                &mut stream,
                coordinator.as_ref(),
                coordinator::CoordinatorOperation::CommitOwnership { metadata },
            )
            .await
        }
        ControlMessage::CommitUploadedOwnership {
            metadata,
            source_device_id,
        } => {
            manager.verify_authenticated_peer(
                &source_device_id,
                peer_certificate.as_ref(),
                &metadata.vault_id,
            )?;
            send_coordinator_response(
                &mut stream,
                coordinator.as_ref(),
                coordinator::CoordinatorOperation::CommitUploadedOwnership {
                    metadata,
                    source_device_id,
                },
            )
            .await
        }
        ControlMessage::ActivationComplete {
            vault_id,
            device_id,
            transfer_id,
            generation,
            purpose,
            ..
        } => {
            manager.verify_authenticated_peer(&device_id, peer_certificate.as_ref(), &vault_id)?;
            send_coordinator_response(
                &mut stream,
                coordinator.as_ref(),
                coordinator::CoordinatorOperation::Activated {
                    vault_id,
                    device_id,
                    transfer_id,
                    generation,
                    purpose,
                },
            )
            .await
        }
        ControlMessage::CancelTransfer { transfer_id } => {
            manager.authenticated_peer(peer_certificate.as_ref())?;
            send_coordinator_response(
                &mut stream,
                coordinator.as_ref(),
                coordinator::CoordinatorOperation::Cancel { transfer_id },
            )
            .await
        }
        ControlMessage::RefreshRequest {
            compatibility,
            vault_id,
            device_id,
            generation,
            ..
        } => {
            manager.verify_authenticated_peer(&device_id, peer_certificate.as_ref(), &vault_id)?;
            send_upload_status_response(
                &mut stream,
                coordinator.as_ref(),
                compatibility,
                vault_id,
                device_id,
                generation,
            )
            .await
        }
        ControlMessage::DistractionsExchange {
            vault_id,
            device_id,
            samples,
            acknowledged_peer_sample_ids,
            owner_snapshot,
            ..
        } => {
            manager.verify_authenticated_peer(&device_id, peer_certificate.as_ref(), &vault_id)?;
            send_coordinator_response(
                &mut stream,
                coordinator.as_ref(),
                coordinator::CoordinatorOperation::DistractionsExchange {
                    vault_id,
                    device_id,
                    samples,
                    acknowledged_peer_sample_ids,
                    owner_snapshot,
                },
            )
            .await
        }
        _ => {
            send_error(
                &mut stream,
                "unsupported_message",
                "the coordinator does not accept this message in the current state",
                false,
            )
            .await
        }
    }
}

async fn send_coordinator_response(
    stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    coordinator: Option<&coordinator::CoordinatorSender>,
    operation: coordinator::CoordinatorOperation,
) -> Result<(), String> {
    let Some(coordinator) = coordinator else {
        return send_error(
            stream,
            "coordinator_unavailable",
            "vault handoff coordinator operations are unavailable",
            true,
        )
        .await;
    };
    let response = match coordinator::request(coordinator, operation).await {
        Ok(response) => response,
        Err(error) => return send_error(stream, "handoff_rejected", &error, true).await,
    };
    send_coordinator_response_value(stream, response).await
}

async fn send_upload_status_response(
    stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    coordinator: Option<&coordinator::CoordinatorSender>,
    compatibility: protocol::HandoffCompatibility,
    vault_id: String,
    device_id: String,
    generation: u64,
) -> Result<(), String> {
    let Some(coordinator) = coordinator else {
        return send_error(
            stream,
            "coordinator_unavailable",
            "vault handoff coordinator operations are unavailable",
            true,
        )
        .await;
    };
    let started = tokio::time::Instant::now();
    loop {
        let response = coordinator::request(
            coordinator,
            coordinator::CoordinatorOperation::PollUpload {
                compatibility: compatibility.clone(),
                vault_id: vault_id.clone(),
                device_id: device_id.clone(),
                generation,
            },
        )
        .await;
        match response {
            Ok(
                response @ coordinator::CoordinatorResponse::UploadStatus {
                    requested_upload: Some(_),
                    ..
                },
            ) => return send_coordinator_response_value(stream, response).await,
            Ok(response) if started.elapsed() >= UPLOAD_REQUEST_WAIT => {
                return send_coordinator_response_value(stream, response).await;
            }
            Ok(_) => tokio::time::sleep(UPLOAD_REQUEST_POLL_INTERVAL).await,
            Err(error) => return send_error(stream, "handoff_rejected", &error, true).await,
        }
    }
}

async fn send_coordinator_response_value(
    stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    response: coordinator::CoordinatorResponse,
) -> Result<(), String> {
    let message = match response {
        coordinator::CoordinatorResponse::Prepared { metadata, purpose } => {
            ControlMessage::BundlePrepared { metadata, purpose }
        }
        coordinator::CoordinatorResponse::BundlePending {
            owner_device_id,
            generation,
        } => ControlMessage::BundlePending {
            protocol_version: PROTOCOL_VERSION,
            owner_device_id,
            generation,
        },
        coordinator::CoordinatorResponse::OwnershipGrant {
            vault_id,
            transfer_id,
            owner_device_id,
            generation,
        } => ControlMessage::OwnershipGrant {
            protocol_version: PROTOCOL_VERSION,
            vault_id,
            transfer_id,
            owner_device_id,
            generation,
        },
        coordinator::CoordinatorResponse::ActivationAcknowledged { transfer_id } => {
            ControlMessage::ActivationAcknowledged { transfer_id }
        }
        coordinator::CoordinatorResponse::Cancelled { transfer_id } => {
            ControlMessage::TransferCancelled { transfer_id }
        }
        coordinator::CoordinatorResponse::UploadStatus {
            generation,
            requested_upload,
        } => ControlMessage::RefreshStatus {
            available: requested_upload.is_some(),
            generation,
            transfer_id: None,
            requested_upload,
        },
        coordinator::CoordinatorResponse::IncomingStaged { transfer_id } => {
            ControlMessage::BundleStaged { transfer_id }
        }
        coordinator::CoordinatorResponse::UploadRequested { .. } => {
            return Err("local upload request cannot be sent over the transport".to_string());
        }
        coordinator::CoordinatorResponse::UploadAuthorized { .. } => {
            return Err("upload authorization cannot be sent as a control response".to_string());
        }
        coordinator::CoordinatorResponse::DistractionsAcknowledged {
            acknowledged_sample_ids,
            peer_samples,
            combined_samples,
        } => ControlMessage::DistractionsAcknowledged {
            acknowledged_sample_ids,
            peer_samples,
            combined_samples,
        },
        coordinator::CoordinatorResponse::ContactRequestReceived { request_id } => {
            ControlMessage::ContactRequestReceived { request_id }
        }
        coordinator::CoordinatorResponse::ContactRequestState {
            state,
            recipient_card,
        } => ControlMessage::ContactRequestState {
            state,
            recipient_card,
        },
        coordinator::CoordinatorResponse::PersonKeyReleased {
            public_key,
            private_key_pkcs8,
        } => ControlMessage::PersonKeyRelease {
            public_key,
            private_key_pkcs8,
        },
        coordinator::CoordinatorResponse::Rejected {
            code,
            message,
            retryable,
        } => return send_error(stream, &code, &message, retryable).await,
    };
    timeout_control(write_control(stream, &message)).await
}

async fn serve_download(
    stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    manager: &PairingManager,
    requested: BundleMetadata,
) -> Result<(), String> {
    let registered = manager.outgoing_bundle(&requested.transfer_id)?;
    if registered.metadata != requested {
        return send_error(
            stream,
            "transfer_metadata",
            "requested transfer metadata does not match the registered bundle",
            false,
        )
        .await;
    }
    timeout_control(write_control(
        stream,
        &ControlMessage::BundleMetadata {
            metadata: registered.metadata.clone(),
        },
    ))
    .await?;
    let response = timeout_control(read_control(stream)).await?;
    let offset = match response {
        ControlMessage::ResumeAt { offset } if offset <= registered.metadata.archive_bytes => {
            offset
        }
        _ => {
            return send_error(
                stream,
                "resume_offset",
                "transfer resume offset is invalid",
                false,
            )
            .await;
        }
    };
    stream_file(
        stream,
        &registered.path,
        offset,
        registered.metadata.archive_bytes,
    )
    .await?;
    match timeout_control(read_control(stream)).await? {
        ControlMessage::BundleComplete { transfer_id }
            if transfer_id == registered.metadata.transfer_id =>
        {
            Ok(())
        }
        _ => Err("receiver did not acknowledge the completed transfer".to_string()),
    }
}

async fn serve_upload(
    stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    manager: &PairingManager,
    coordinator: Option<&coordinator::CoordinatorSender>,
    metadata: BundleMetadata,
    source_device_id: String,
    purpose: BundlePurpose,
) -> Result<(), String> {
    metadata.validate()?;
    let coordinator = coordinator
        .ok_or_else(|| "vault handoff coordinator operations are unavailable".to_string())?;
    let authorization = coordinator::request(
        coordinator,
        coordinator::CoordinatorOperation::AuthorizeUpload {
            metadata: metadata.clone(),
            source_device_id: source_device_id.clone(),
            purpose,
        },
    )
    .await;
    match authorization {
        Err(error) => return send_error(stream, "upload_rejected", &error, false).await,
        Ok(coordinator::CoordinatorResponse::UploadAuthorized {
            transfer_id,
            already_received,
        }) if transfer_id == metadata.transfer_id => {
            if already_received {
                timeout_control(write_control(
                    stream,
                    &ControlMessage::ResumeAt {
                        offset: metadata.archive_bytes,
                    },
                ))
                .await?;
                match timeout_control(read_control(stream)).await? {
                    ControlMessage::BundleComplete { transfer_id }
                        if transfer_id == metadata.transfer_id => {}
                    _ => return Err("source did not complete the uploaded transfer".to_string()),
                }
                return send_coordinator_response(
                    stream,
                    Some(coordinator),
                    coordinator::CoordinatorOperation::Uploaded {
                        metadata,
                        source_device_id,
                        purpose,
                    },
                )
                .await;
            }
        }
        _ => return Err("coordinator returned invalid upload authorization".to_string()),
    }
    let (partial_path, metadata_path, complete_path) =
        manager.staging_paths(&metadata.transfer_id)?;
    let resume_offset = if complete_path.exists() {
        protocol::validate_staging_file(&complete_path, &metadata)?;
        metadata.archive_bytes
    } else {
        prepare_partial(manager, &partial_path, &metadata_path, &metadata)?
    };
    timeout_control(write_control(
        stream,
        &ControlMessage::ResumeAt {
            offset: resume_offset,
        },
    ))
    .await?;

    if resume_offset < metadata.archive_bytes {
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&partial_path)
            .await
            .map_err(|error| format!("open incoming transfer: {error}"))?;
        let mut remaining = metadata.archive_bytes - resume_offset;
        let mut buffer = vec![0_u8; TRANSFER_CHUNK_BYTES];
        while remaining > 0 {
            let count = usize::try_from(remaining.min(buffer.len() as u64))
                .map_err(|_| "incoming transfer chunk size overflow".to_string())?;
            tokio::time::timeout(CHUNK_TIMEOUT, stream.read_exact(&mut buffer[..count]))
                .await
                .map_err(|_| "receiving the transfer bundle timed out".to_string())?
                .map_err(|error| format!("receive transfer bundle: {error}"))?;
            file.write_all(&buffer[..count])
                .await
                .map_err(|error| format!("write incoming transfer: {error}"))?;
            remaining -= count as u64;
        }
        file.sync_all()
            .await
            .map_err(|error| format!("sync incoming transfer: {error}"))?;
        drop(file);
    }
    match timeout_control(read_control(stream)).await? {
        ControlMessage::BundleComplete { transfer_id } if transfer_id == metadata.transfer_id => {}
        _ => return Err("source did not complete the uploaded transfer".to_string()),
    }
    if !complete_path.exists() {
        if let Err(error) = protocol::validate_staging_file(&partial_path, &metadata) {
            let _ = manager.remove_staging(&metadata.transfer_id);
            return Err(error);
        }
        fs::rename(&partial_path, &complete_path)
            .map_err(|error| format!("complete incoming transfer: {error}"))?;
        match fs::remove_file(&metadata_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("remove incoming transfer metadata: {error}")),
        }
    }
    send_coordinator_response(
        stream,
        Some(coordinator),
        coordinator::CoordinatorOperation::Uploaded {
            metadata,
            source_device_id,
            purpose,
        },
    )
    .await
}

fn server_config(manager: &PairingManager) -> Result<rustls::ServerConfig, String> {
    let (_, identity) = manager.identity()?;
    let builder = rustls::ServerConfig::builder();
    let client_certificates = manager.client_auth_certificates()?;
    let config = if !client_certificates.is_empty() {
        let mut roots = RootCertStore::empty();
        for certificate in client_certificates {
            roots
                .add(certificate)
                .map_err(|error| format!("trust linked device certificate: {error}"))?;
        }
        let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
            .allow_unauthenticated()
            .build()
            .map_err(|error| format!("configure linked device verifier: {error}"))?;
        builder.with_client_cert_verifier(verifier)
    } else {
        builder.with_no_client_auth()
    }
    .with_single_cert(vec![identity.certificate], identity.private_key.clone_key())
    .map_err(|error| format!("configure coordinator TLS identity: {error}"))?;
    Ok(config)
}

async fn send_error<W>(
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
