//! The member sync listener over real TLS: who completes a handshake and what it answers.

use super::*;
use crate::sync::hub::{OpenHubVault, SyncHub};
use crate::vault::handoff::pairing::{PendingAcknowledgement, certificate_fingerprint};
use crate::vault::handoff::protocol::decode_certificate_fingerprint;
use ganbaru_sync::{AuthorizationView, EnrolledDevice};
use ganbaru_sync_contracts::{AdminPublicKey, DeviceKind as SyncDeviceKind, DevicePublicKey};
use ganbaru_sync_replica::peers::admission::MemberAdmission;

const VAULT: &str = "vault-1";
const MEMBER: &str = "device-laptop";
const PHONE: &str = "device-phone";

/// Admission the test changes while the listener runs, as a log update or a quarantine does.
type SharedAdmission = std::sync::Arc<std::sync::Mutex<Option<MemberAdmission>>>;

/// A member desktop serving its sync listener on loopback, and a phone that may dial it.
struct MemberListener {
    _member_root: TestDirectory,
    _phone_root: TestDirectory,
    phone: PairingManager,
    endpoint: String,
    member_fingerprint: String,
    phone_fingerprint: String,
    admission: SharedAdmission,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl MemberListener {
    async fn start() -> Self {
        Self::start_serving(None, false).await
    }

    /// A member listener dialed by `phone`, or by a new unlinked phone, that serves vault
    /// copies when `serve_copies`.
    async fn start_serving(phone: Option<PairingManager>, serve_copies: bool) -> Self {
        let member_root = TestDirectory::new("member");
        let phone_root = TestDirectory::new("member-phone");
        let member = PairingManager::default();
        member
            .initialize(member_root.path().to_path_buf(), MEMBER.to_string())
            .expect("initialize member");
        let phone = phone.unwrap_or_else(|| {
            let phone = PairingManager::default();
            phone
                .initialize(phone_root.path().to_path_buf(), PHONE.to_string())
                .expect("initialize phone");
            phone
        });
        let fingerprint = |manager: &PairingManager| {
            certificate_fingerprint(manager.certificate().expect("certificate").as_ref())
        };
        let member_fingerprint = fingerprint(&member);
        let phone_fingerprint = fingerprint(&phone);
        let admission: SharedAdmission = std::sync::Arc::new(std::sync::Mutex::new(Some(
            admission_with(&phone_fingerprint, false),
        )));
        let source = admission.clone();
        let open: OpenHubVault = std::sync::Arc::new(|| Box::pin(async { Ok(None) }));
        let services = server::MemberServices {
            hub: SyncHub::new(open, tokio::sync::watch::Sender::new(0)),
            admission: std::sync::Arc::new(move || {
                let current = source.lock().expect("admission").clone();
                Box::pin(async move { Ok(current) })
            }),
            lan: LanAdmission::default(),
            bundles: serve_copies.then(|| snapshot_copies(member_root.path().to_path_buf())),
        };
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind member listener");
        let endpoint = listener.local_addr().expect("member endpoint").to_string();
        let (shutdown, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(server::serve_member(
            listener.into(),
            server::limits::ConnectionLimits::default(),
            LoopbackPolicy::Allow,
            member,
            services,
            receiver,
        ));
        Self {
            _member_root: member_root,
            _phone_root: phone_root,
            phone,
            endpoint,
            member_fingerprint,
            phone_fingerprint,
            admission,
            shutdown: Some(shutdown),
        }
    }

    /// Replaces the admission the listener reads for its next connection.
    fn admit(&self, admission: MemberAdmission) {
        *self.admission.lock().expect("admission") = Some(admission);
    }

    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        member_sync_request(
            &self.phone,
            std::slice::from_ref(&self.endpoint),
            &self.member_fingerprint,
            message,
        )
        .await
    }

    /// This listener as a vault copy source.
    fn source(&self) -> BundleSource {
        BundleSource::Member {
            device_id: MEMBER.to_string(),
            endpoints: vec![self.endpoint.clone()],
            fingerprint: self.member_fingerprint.clone(),
        }
    }
}

/// The archive bytes every test snapshot copy holds.
const MEMBER_COPY: &[u8] = b"member snapshot copy";

/// Snapshot copies written under `root`, built for the request as the app builds them.
fn snapshot_copies(root: PathBuf) -> server::MemberBundles {
    server::MemberBundles::new(std::sync::Arc::new(
        move |request: server::SnapshotRequest| {
            let root = root.clone();
            Box::pin(async move {
                let transfer_id = random_token("transfer")?;
                let archive_path = root.join(format!("{transfer_id}.zip"));
                fs::write(&archive_path, MEMBER_COPY).map_err(|error| error.to_string())?;
                let metadata = BundleMetadata {
                    protocol_version: PROTOCOL_VERSION,
                    compatibility: super::super::super::protocol::test_compatibility(),
                    vault_id: request.vault_id,
                    device_id: request.device_id,
                    transfer_id,
                    generation: request.generation,
                    archive_bytes: MEMBER_COPY.len() as u64,
                    archive_sha256: sha256_file(&archive_path)?,
                };
                Ok(server::PreparedSnapshot {
                    metadata,
                    archive_path,
                })
            })
        },
    ))
}

fn bundle_request(purpose: BundlePurpose) -> ControlMessage {
    ControlMessage::RequestBundle {
        protocol_version: PROTOCOL_VERSION,
        compatibility: super::super::super::protocol::test_compatibility(),
        vault_id: VAULT.to_string(),
        device_id: PHONE.to_string(),
        generation: 0,
        purpose,
    }
}

impl Drop for MemberListener {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

/// Admission of a log that enrolled the phone with `fingerprint`, removed when `removed`.
fn admission_with(fingerprint: &str, removed: bool) -> MemberAdmission {
    MemberAdmission::new(
        VAULT,
        &enrolling_view(fingerprint, removed),
        &std::collections::BTreeSet::new(),
    )
}

/// Admission of a log that enrolled the phone with `fingerprint`, which this installation
/// quarantined.
fn admission_quarantining(fingerprint: &str) -> MemberAdmission {
    MemberAdmission::new(
        VAULT,
        &enrolling_view(fingerprint, false),
        &std::collections::BTreeSet::from([PHONE.to_string()]),
    )
}

fn enrolling_view(fingerprint: &str, removed: bool) -> AuthorizationView {
    AuthorizationView {
        epoch: 1,
        revision: 3,
        entry_hash: [7; 32],
        anchor_device_id: "device-desktop".to_string(),
        frozen_at: None,
        catch_up: None,
        charter: [8; 32],
        recovery_admin_key: AdminPublicKey::from_bytes([9; 32]),
        kit_admin_key: AdminPublicKey::from_bytes([9; 32]),
        devices: vec![EnrolledDevice {
            device_id: PHONE.to_string(),
            device_key: DevicePublicKey::from_bytes([3; 32]),
            tls_fingerprint: decode_certificate_fingerprint(fingerprint).expect("fingerprint"),
            label: None,
            kind: SyncDeviceKind::Mobile,
            role: ganbaru_sync_contracts::DeviceRole::Member,
            enrolled_revision: 2,
            removed_revision: removed.then_some(3),
            encryption_key: None,
        }],
        discovery_key: [5; 32],
        previous_discovery_key: None,
        key_epoch: 0,
        recovery_encryption_key: None,
        pending_removal: None,
        server: None,
    }
}

fn hello(device_id: &str) -> ControlMessage {
    ControlMessage::SyncHello {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT.to_string(),
        device_id: device_id.to_string(),
        manifest_version: 1,
        stored: Vec::new(),
        probe: None,
        listening: Vec::new(),
    }
}

fn refused_with(response: &ControlMessage, expected: &str) -> bool {
    matches!(response, ControlMessage::Error { code, retryable: false, .. } if code == expected)
}

#[tokio::test]
async fn an_enrolled_member_reaches_the_hub_of_a_member_listener() {
    let listener = MemberListener::start().await;
    let response = listener.request(hello(PHONE)).await.expect("sync exchange");
    assert!(
        matches!(
            &response,
            ControlMessage::Error { code, retryable: true, .. } if code == "sync_unavailable"
        ),
        "{response:?}"
    );
}

#[tokio::test]
async fn a_member_listener_refuses_a_client_without_a_certificate() {
    let listener = MemberListener::start().await;
    let unauthenticated = unauthenticated_exchange(
        &listener.endpoint,
        &listener.member_fingerprint,
        hello(PHONE),
    )
    .await;
    assert!(
        unauthenticated.is_err(),
        "a connection without a client certificate must not reach the hub"
    );
}

#[tokio::test]
async fn a_member_listener_serves_a_device_removed_before_the_handshake_record_exchange_only() {
    let listener = MemberListener::start().await;
    listener.admit(admission_with(&listener.phone_fingerprint, true));
    // A removed device still completes the handshake so it can take the records that removed
    // it, and every other request is refused.
    let response = listener.request(hello(PHONE)).await;
    assert!(
        matches!(&response, Ok(message) if refused_with(message, "member_removed")),
        "{response:?}"
    );
    let records = ControlMessage::SyncRecordGet {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT.to_string(),
        device_id: PHONE.to_string(),
        hashes: Vec::new(),
        writers: Vec::new(),
    };
    let response = listener.request(records).await.expect("record exchange");
    assert!(
        !refused_with(&response, "member_removed"),
        "record exchange reaches the hub: {response:?}"
    );
}

#[tokio::test]
async fn a_member_listener_rechecks_the_log_after_the_handshake() {
    let listener = MemberListener::start().await;
    listener
        .request(hello(PHONE))
        .await
        .expect("admitted before the removal");
    // The handshake reuses the admission it read moments ago, so the fresh read refuses.
    listener.admit(admission_with(&listener.phone_fingerprint, true));
    let response = listener.request(hello(PHONE)).await;
    assert!(
        matches!(&response, Ok(message) if refused_with(message, "member_removed")),
        "{response:?}"
    );
    *listener.admission.lock().expect("admission") = None;
    assert!(
        listener.request(hello(PHONE)).await.is_err(),
        "a listener whose vault does not run sync answers nothing"
    );
}

#[tokio::test]
async fn a_member_listener_refuses_pairing_handoff_contact_and_foreign_device_frames() {
    let listener = MemberListener::start().await;
    let refresh = ControlMessage::RefreshRequest {
        protocol_version: PROTOCOL_VERSION,
        compatibility: super::super::super::protocol::test_compatibility(),
        vault_id: VAULT.to_string(),
        device_id: PHONE.to_string(),
        generation: 0,
    };
    for (message, code) in [
        (refresh, "sync_only"),
        (super::contact_request_message("request-1"), "sync_only"),
        (hello("device-other"), "member_device_mismatch"),
    ] {
        let response = listener.request(message).await.expect("answered refusal");
        assert!(refused_with(&response, code), "{code}: {response:?}");
    }
}

#[tokio::test]
async fn a_member_client_refuses_a_listener_with_another_certificate() {
    let listener = MemberListener::start().await;
    let wrong = "0".repeat(64);
    let reached = member_sync_request(
        &listener.phone,
        std::slice::from_ref(&listener.endpoint),
        &wrong,
        hello(PHONE),
    )
    .await;
    assert!(
        reached.is_err(),
        "the client must refuse a listener whose certificate the log did not enroll"
    );
}

#[tokio::test]
async fn the_coordinator_serves_nothing_to_a_device_the_log_removed() {
    let open: OpenHubVault = std::sync::Arc::new(|| Box::pin(async { Ok(None) }));
    let hub = SyncHub::new(open, tokio::sync::watch::Sender::new(0));
    let admission: SharedAdmission = std::sync::Arc::new(std::sync::Mutex::new(None));
    let source = admission.clone();
    let pair = LocalPair::start_with_sealer(
        Some(hub),
        server::limits::ConnectionLimits::default(),
        EnrollmentApprovals::default(),
        LanAdmission::default(),
        test_enrollment_sealer(),
        None,
        Some(std::sync::Arc::new(move || {
            let current = source.lock().expect("admission").clone();
            Box::pin(async move { Ok(current) })
        })),
    )
    .await;
    pair.enroll().await;
    let phone_fingerprint =
        certificate_fingerprint(pair.phone.certificate().expect("certificate").as_ref());
    *admission.lock().expect("admission") = Some(admission_with(&phone_fingerprint, false));
    let pull = || {
        sync_request(
            &pair.phone,
            ControlMessage::SyncWait {
                protocol_version: PROTOCOL_VERSION,
                vault_id: VAULT.to_string(),
                device_id: PHONE.to_string(),
                known: Vec::new(),
            },
        )
    };
    let served = pull().await.expect("sync exchange");
    assert!(
        !refused_with(&served, "member_removed"),
        "an enrolled device reaches the hub: {served:?}"
    );

    // Another admin removed the phone; the coordinator's pairing still lists it as linked.
    *admission.lock().expect("admission") = Some(admission_with(&phone_fingerprint, true));
    let key = request_person_key(&pair.phone)
        .await
        .expect("person key exchange");
    assert!(refused_with(&key, "member_removed"), "{key:?}");
    let pulled = pull().await.expect("sync exchange");
    assert!(refused_with(&pulled, "member_removed"), "{pulled:?}");
    let metadata = pair.register_bundle(b"whole-vault bundle", "transfer-removed");
    let download = download_bundle(
        &pair.phone,
        &BundleSource::Coordinator,
        metadata,
        &TransferCancellation::default(),
    )
    .await;
    assert!(
        download.is_err_and(|error| error.contains("removed from the vault")),
        "a removed device downloads no bundle"
    );
}

#[tokio::test]
async fn the_coordinator_serves_nothing_while_its_log_admission_cannot_be_read() {
    let open: OpenHubVault = std::sync::Arc::new(|| Box::pin(async { Ok(None) }));
    let hub = SyncHub::new(open, tokio::sync::watch::Sender::new(0));
    let unreadable = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let source = unreadable.clone();
    let pair = LocalPair::start_with_sealer(
        Some(hub),
        server::limits::ConnectionLimits::default(),
        EnrollmentApprovals::default(),
        LanAdmission::default(),
        test_enrollment_sealer(),
        None,
        Some(std::sync::Arc::new(move || {
            let failing = source.load(std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move {
                if failing {
                    Err("the selected log is damaged".to_string())
                } else {
                    Ok(None)
                }
            })
        })),
    )
    .await;
    pair.enroll().await;
    unreadable.store(true, std::sync::atomic::Ordering::SeqCst);
    let unavailable = |response: &ControlMessage| {
        matches!(
            response,
            ControlMessage::Error { code, retryable: true, .. } if code == "admission_unavailable"
        )
    };
    let pulled = sync_request(
        &pair.phone,
        ControlMessage::SyncWait {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT.to_string(),
            device_id: PHONE.to_string(),
            known: Vec::new(),
        },
    )
    .await
    .expect("sync exchange");
    assert!(unavailable(&pulled), "{pulled:?}");
    let key = request_person_key(&pair.phone)
        .await
        .expect("person key exchange");
    assert!(unavailable(&key), "{key:?}");
}

/// A phone linked to a running coordinator, and a member desktop serving copies to it.
async fn linked_phone_and_member() -> (LocalPair, MemberListener) {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let member = MemberListener::start_serving(Some(pair.phone.clone()), true).await;
    member.admit(admission_with(&member.phone_fingerprint, false));
    (pair, member)
}

fn acknowledgement_of(metadata: &BundleMetadata) -> PendingAcknowledgement {
    PendingAcknowledgement {
        vault_id: metadata.vault_id.clone(),
        device_id: metadata.device_id.clone(),
        transfer_id: metadata.transfer_id.clone(),
        generation: metadata.generation,
        purpose: BundlePurpose::Refresh,
    }
}

#[tokio::test]
async fn a_member_listener_serves_a_vault_copy_to_an_enrolled_device() {
    let (pair, member) = linked_phone_and_member().await;
    let source = member.source();
    let fetched = fetch_bundle(
        &pair.phone,
        std::slice::from_ref(&source),
        super::super::super::protocol::test_compatibility(),
        4,
        BundlePurpose::Refresh,
        &TransferCancellation::default(),
    )
    .await
    .expect("copy from the member");
    assert_eq!(fetched.source, source);
    assert_eq!(fetched.metadata.device_id, PHONE);
    assert_eq!(
        fetched.metadata.generation, 4,
        "a member's copy keeps the ownership generation the device holds"
    );
    assert_eq!(fs::read(&fetched.archive).expect("read copy"), MEMBER_COPY);

    let pending = acknowledgement_of(&fetched.metadata);
    acknowledge_activation(&pair.phone, &source, &pending)
        .await
        .expect("acknowledge the copy");
    acknowledge_activation(&pair.phone, &source, &pending)
        .await
        .expect("a repeated acknowledgement is answered");
    let fresh = fetch_bundle(
        &pair.phone,
        std::slice::from_ref(&source),
        super::super::super::protocol::test_compatibility(),
        4,
        BundlePurpose::Refresh,
        &TransferCancellation::default(),
    )
    .await
    .expect("a later copy after the first was released");
    assert_ne!(fresh.metadata.transfer_id, fetched.metadata.transfer_id);
}

#[tokio::test]
async fn a_member_listener_serves_no_ownership_transfer() {
    let listener = MemberListener::start_serving(None, true).await;
    let response = listener
        .request(bundle_request(BundlePurpose::Ownership))
        .await
        .expect("answered refusal");
    assert!(refused_with(&response, "bootstrap_only"), "{response:?}");
}

#[tokio::test]
async fn a_member_listener_gives_no_copy_to_a_removed_quarantined_or_unknown_device() {
    let listener = MemberListener::start_serving(None, true).await;
    listener.admit(admission_with(&listener.phone_fingerprint, true));
    let removed = listener
        .request(bundle_request(BundlePurpose::Refresh))
        .await
        .expect("answered refusal");
    assert!(refused_with(&removed, "member_removed"), "{removed:?}");

    // A handshake may reuse the admission read moments ago; the fresh read after it refuses.
    listener.admit(admission_quarantining(&listener.phone_fingerprint));
    let quarantined = listener
        .request(bundle_request(BundlePurpose::Refresh))
        .await;
    assert!(
        quarantined
            .as_ref()
            .is_ok_and(|response| refused_with(response, "member_quarantined")),
        "{quarantined:?}"
    );

    listener.admit(admission_with(&"0".repeat(64), false));
    let unknown = listener
        .request(bundle_request(BundlePurpose::Refresh))
        .await;
    assert!(
        unknown
            .as_ref()
            .is_ok_and(|response| refused_with(response, "member_unknown")),
        "{unknown:?}"
    );

    // Once the handshake admission is read again, neither completes a handshake.
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(
        listener
            .request(bundle_request(BundlePurpose::Refresh))
            .await
            .is_err(),
        "a device the log never enrolled completes no handshake"
    );
    listener.admit(admission_quarantining(&listener.phone_fingerprint));
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(
        listener
            .request(bundle_request(BundlePurpose::Refresh))
            .await
            .is_err(),
        "a quarantined device completes no handshake"
    );
}

#[tokio::test]
async fn a_device_falls_back_from_an_unreachable_coordinator_to_a_member() {
    let (mut pair, member) = linked_phone_and_member().await;
    if let Some(shutdown) = pair.shutdown.take() {
        let _ = shutdown.send(());
    }
    // Lets the coordinator's accept loop observe the shutdown and close its socket.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let fetched = fetch_bundle(
        &pair.phone,
        &[BundleSource::Coordinator, member.source()],
        super::super::super::protocol::test_compatibility(),
        0,
        BundlePurpose::Refresh,
        &TransferCancellation::default(),
    )
    .await
    .expect("copy from the member after the coordinator failed");
    assert_eq!(fetched.source, member.source());
    assert_eq!(fs::read(&fetched.archive).expect("read copy"), MEMBER_COPY);
}

#[tokio::test]
async fn a_failed_copy_reports_the_coordinator_error_first() {
    let (mut pair, member) = linked_phone_and_member().await;
    if let Some(shutdown) = pair.shutdown.take() {
        let _ = shutdown.send(());
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    member.admit(admission_with(&member.phone_fingerprint, true));
    let error = fetch_bundle(
        &pair.phone,
        &[BundleSource::Coordinator, member.source()],
        super::super::super::protocol::test_compatibility(),
        0,
        BundlePurpose::Refresh,
        &TransferCancellation::default(),
    )
    .await
    .expect_err("no source serves a copy");
    assert!(
        !error.contains("removed"),
        "the device it enrolled with explains the failure: {error}"
    );
}
