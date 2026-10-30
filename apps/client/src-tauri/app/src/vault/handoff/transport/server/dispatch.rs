//! The desktop coordinator listener: authenticated dispatch of pairing, handoff, contact, and
//! sync requests, and archive serving.

use super::super::{
    MEMBERSHIP_REVOKED_CODE, TLS_HANDSHAKE_TIMEOUT, stream_file,
    timeout_control,
};
use super::client_verifier::PinnedClientVerifier;
use super::limits::{ConnectionLimits, PendingPermit};
use super::{
    COORDINATOR_BUSY_CODE, CoordinatorListener, LanAdmission, MemberAdmissionSource,
    accept_connections, admitted_local_address, send_error,
};
use crate::contacts::requests::PeerReply;
use crate::sync::hub::SyncHub;
use crate::vault::handoff::contact_requests::{
    ContactJob, ContactOperation, ContactSender, MIN_CONTACT_REQUEST_RESPONSE,
};
use crate::vault::handoff::enrollment_approval::{
    ENROLLMENT_DECLINED_MESSAGE, ENROLLMENT_NO_CHARTER_MESSAGE, ENROLLMENT_UNANSWERED_MESSAGE,
    EnrollmentApprovals, EnrollmentDecision,
};
use crate::vault::handoff::pairing::{Enrollment, PairingManager, PinnedCharter, TlsIdentity};
use crate::vault::handoff::protocol::{
    BundleMetadata, ControlMessage, PROTOCOL_VERSION,
    read_control, read_control_limited, unix_time_ms, write_control,
};
use crate::vault::handoff::{coordinator, protocol, suggested_device_label};
use ganbaru_handoff::dial::LoopbackPolicy;
use ganbaru_sync_replica::peers::admission::MemberRefusal;
use rustls::pki_types::CertificateDer;
use std::collections::BTreeSet;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
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
        let (_, identity) = super::super::load_tls_identity(manager).await?;
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

/// Seals the sync enrollment of linked devices. The listener runs it after linking an approved
/// device and before replying, so the vault bundle the device downloads next already enrolls it.
/// It returns the space charter and its person key for the reply, so the device pins the anchor
/// it was told about. A failure leaves the enrollment to the next sync pass.
pub(crate) type EnrollmentSealer = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<Option<PinnedCharter>, String>> + Send>>
        + Send
        + Sync,
>;

/// The device ids this installation quarantined in the active vault. The listener turns their
/// connections away right after the TLS handshake.
pub(crate) type QuarantinedDevices = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<BTreeSet<String>, String>> + Send>>
        + Send
        + Sync,
>;

/// Optional coordinator features a listener dispatches to, and the approvals enrollment asks.
#[derive(Clone, Default)]
pub(crate) struct CoordinatorServices {
    pub coordinator: Option<coordinator::CoordinatorSender>,
    pub contacts: Option<ContactSender>,
    pub hub: Option<SyncHub>,
    /// Records the enrollment of a device that was just linked, before it is told so.
    pub enrollment_sealer: Option<EnrollmentSealer>,
    pub approvals: EnrollmentApprovals,
    /// Whether the network behind each local address may reach the listener at all.
    pub admission: LanAdmission,
    /// Devices whose connections are refused while they stay quarantined.
    pub quarantined: Option<QuarantinedDevices>,
    /// The selected log's admission: a linked device the log removed reaches no service, and
    /// sync requests need a device the log enrolled.
    pub log_admission: Option<MemberAdmissionSource>,
}

pub(crate) async fn serve_with_coordinator(
    listener: CoordinatorListener,
    manager: PairingManager,
    services: CoordinatorServices,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    serve_with_limits(
        listener,
        ConnectionLimits::default(),
        super::super::LOOPBACK_PEERS,
        manager,
        services,
        shutdown,
    )
    .await;
}

/// Serves `listener` until shutdown. `loopback` decides whether loopback peers are admitted.
pub(in super::super) async fn serve_with_limits(
    listener: CoordinatorListener,
    limits: ConnectionLimits,
    loopback: LoopbackPolicy,
    manager: PairingManager,
    services: CoordinatorServices,
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
struct Services {
    manager: PairingManager,
    features: CoordinatorServices,
    limits: ConnectionLimits,
}

async fn handle_connection(
    socket: TcpStream,
    source: IpAddr,
    permit: PendingPermit,
    configs: ServerConfigCache,
    services: Services,
) -> Result<(), String> {
    let Services {
        manager,
        features:
            CoordinatorServices {
                coordinator,
                contacts,
                hub,
                enrollment_sealer,
                approvals,
                admission: _,
                quarantined,
                log_admission,
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

    match request {
        ControlMessage::Enroll {
            invitation_id,
            secret,
            vault_id,
            device_id,
            device_label,
            device_kind,
            device_certificate,
            sync_device_key,
            sync_admin_key,
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
            let enrollment = Enrollment {
                invitation_id: &invitation_id,
                secret: &secret,
                vault_id: &vault_id,
                device_id: &device_id,
                device_label: &device_label,
                device_kind,
                certificate_b64: &device_certificate,
                sync_device_key: &sync_device_key,
                sync_admin_key: sync_admin_key.as_deref(),
            };
            // The invitation is checked when the request arrives, so time spent waiting for the
            // user does not expire it.
            let requested_at_ms = unix_time_ms();
            let code = match manager.verify_enrollment(&enrollment, requested_at_ms) {
                Ok(code) => code,
                Err(error) => {
                    return send_error(&mut stream, "enrollment_rejected", &error, false).await;
                }
            };
            let decision = await_enrollment_approval(
                &mut stream,
                &approvals,
                &device_label,
                device_kind,
                code,
            )
            .await;
            match decision {
                Ok(EnrollmentDecision::Approved) => {}
                Ok(EnrollmentDecision::Declined) => {
                    // A declined device presented a leaked invitation, so nobody may use it again.
                    manager.discard_invitation(&invitation_id)?;
                    return send_error(
                        &mut stream,
                        "enrollment_rejected",
                        ENROLLMENT_DECLINED_MESSAGE,
                        false,
                    )
                    .await;
                }
                Err(error) => {
                    return send_error(&mut stream, "enrollment_rejected", &error, false).await;
                }
            }
            if let Err(error) = manager.enroll_peer(enrollment, requested_at_ms) {
                return send_error(&mut stream, "enrollment_rejected", &error, false).await;
            }
            let charter = match enrollment_sealer.as_ref() {
                Some(seal) => seal().await.unwrap_or_else(|error| {
                    eprintln!("sync: enroll the linked device: {error}");
                    None
                }),
                None => None,
            };
            // A device told nothing about the anchor could not pin it, so it is refused. The link
            // stays: unlinking would make the anchor remove a device it may already have
            // enrolled, and a removed device id never enrolls again. Pairing the same device
            // again reuses the link.
            let Some(PinnedCharter {
                charter,
                person_key,
            }) = charter
            else {
                return send_error(
                    &mut stream,
                    "enrollment_rejected",
                    ENROLLMENT_NO_CHARTER_MESSAGE,
                    true,
                )
                .await;
            };
            let coordinator_device_id = manager.device_id()?;
            let coordinator_device_label = suggested_device_label();
            timeout_control(write_control(
                &mut stream,
                &ControlMessage::Enrolled {
                    protocol_version: PROTOCOL_VERSION,
                    coordinator_device_id,
                    coordinator_device_label: Some(coordinator_device_label),
                    charter: Some(charter),
                    person_key: Some(person_key),
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
            let operation = ContactOperation::Request {
                recipient_card_nonce,
                requester_card,
                request_id,
                signature,
            };
            send_contact_response(&mut stream, contacts.as_ref(), &limits, source, operation).await
        }
        ControlMessage::ContactRequestStatus {
            request_id,
            requester_public_key,
            issued_at_ms,
            signature,
            ..
        } => {
            let operation = ContactOperation::Status {
                request_id,
                requester_public_key,
                issued_at_ms,
                signature,
            };
            send_contact_response(&mut stream, contacts.as_ref(), &limits, source, operation).await
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
                coordinator::CoordinatorOperation::ReleasePersonKey {
                    vault_id,
                    device_id,
                },
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
        ControlMessage::RequestBundle {
            compatibility,
            vault_id,
            device_id,
            ..
        } => {
            manager.verify_authenticated_peer(&device_id, peer_certificate.as_ref(), &vault_id)?;
            let operation = coordinator::CoordinatorOperation::Prepare {
                compatibility,
                vault_id,
                device_id,
            };
            send_coordinator_response(&mut stream, coordinator.as_ref(), operation).await
        }
        ControlMessage::ActivationComplete {
            vault_id,
            device_id,
            transfer_id,
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

/// Holds a verified enrollment until the coordinator user answers it, the wait expires, or the
/// enrolling device closes its connection. Nothing is linked or sent while it waits.
async fn await_enrollment_approval<S>(
    stream: &mut S,
    approvals: &EnrollmentApprovals,
    device_label: &str,
    device_kind: protocol::DeviceKind,
    code: String,
) -> Result<EnrollmentDecision, String>
where
    S: tokio::io::AsyncRead + Unpin,
{
    let mut wait = approvals.begin(device_label, device_kind, code)?;
    let mut probe = [0_u8; 1];
    let outcome = tokio::select! {
        decision = &mut wait.decision => decision
            .map_err(|_| "the pairing code on the coordinator was replaced".to_string()),
        () = tokio::time::sleep(approvals.timeout()) => {
            Err(ENROLLMENT_UNANSWERED_MESSAGE.to_string())
        }
        _ = stream.read(&mut probe) => Err("the enrolling device closed its connection".to_string()),
    };
    approvals.finish(&wait.request_id);
    outcome
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

/// Hands an unauthenticated contact message to the contact worker. A source past its contact
/// budget, or a full worker queue, gets a retryable busy answer instead of waiting. A handled
/// contact request is answered no sooner than [`MIN_CONTACT_REQUEST_RESPONSE`] after it arrived.
async fn send_contact_response(
    stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    contacts: Option<&ContactSender>,
    limits: &ConnectionLimits,
    source: IpAddr,
    operation: ContactOperation,
) -> Result<(), String> {
    let Some(contacts) = contacts else {
        return send_error(
            stream,
            "coordinator_unavailable",
            "contact requests are unavailable on this coordinator",
            true,
        )
        .await;
    };
    if !limits.take_contact_message(source, std::time::Instant::now()) {
        return send_peer_reply(stream, PeerReply::busy()).await;
    }
    let answer_floor = matches!(operation, ContactOperation::Request { .. })
        .then(|| tokio::time::Instant::now() + MIN_CONTACT_REQUEST_RESPONSE);
    let (response, receiver) = tokio::sync::oneshot::channel();
    if contacts
        .try_send(ContactJob {
            operation,
            response,
        })
        .is_err()
    {
        return send_peer_reply(stream, PeerReply::busy()).await;
    }
    let handled = receiver.await;
    if let Some(floor) = answer_floor {
        tokio::time::sleep_until(floor).await;
    }
    match handled {
        Ok(Ok(reply)) => send_peer_reply(stream, reply).await,
        Ok(Err(error)) => send_error(stream, "handoff_rejected", &error, true).await,
        Err(_) => {
            send_error(
                stream,
                "coordinator_unavailable",
                "the contact request was not handled",
                true,
            )
            .await
        }
    }
}

async fn send_peer_reply(
    stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    reply: PeerReply,
) -> Result<(), String> {
    let message = match reply {
        PeerReply::Received { request_id } => ControlMessage::ContactRequestReceived { request_id },
        PeerReply::State {
            state,
            recipient_card,
        } => ControlMessage::ContactRequestState {
            state,
            recipient_card,
        },
        PeerReply::Rejected {
            code,
            message,
            retryable,
        } => return send_error(stream, code, message, retryable).await,
    };
    timeout_control(write_control(stream, &message)).await
}

async fn send_coordinator_response_value(
    stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    response: coordinator::CoordinatorResponse,
) -> Result<(), String> {
    let message = match response {
        coordinator::CoordinatorResponse::Prepared { metadata } => {
            ControlMessage::BundlePrepared { metadata }
        }
        coordinator::CoordinatorResponse::ActivationAcknowledged { transfer_id } => {
            ControlMessage::ActivationAcknowledged { transfer_id }
        }
        coordinator::CoordinatorResponse::Cancelled { transfer_id } => {
            ControlMessage::TransferCancelled { transfer_id }
        }
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

pub(super) async fn serve_download(
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
pub(crate) fn log_refusal(
    admission: Result<Option<ganbaru_sync_replica::peers::admission::MemberAdmission>, String>,
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
    let fingerprint = crate::vault::handoff::pairing::certificate_fingerprint(certificate.as_ref());
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
pub(crate) fn refuses_quarantined(
    quarantined: Result<BTreeSet<String>, String>,
    device_id: &str,
) -> bool {
    match quarantined {
        Ok(devices) => devices.contains(device_id),
        Err(error) => {
            eprintln!("sync: read the quarantine list: {error}");
            false
        }
    }
}
