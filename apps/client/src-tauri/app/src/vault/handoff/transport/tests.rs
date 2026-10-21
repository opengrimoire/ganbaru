use super::*;
use crate::contacts::requests::{PeerReply, RECIPIENT_BUSY_CODE};
use crate::vault::handoff::contact_requests::{ContactJob, ContactOperation};
use crate::vault::handoff::enrollment_approval::{
    ENROLLMENT_DECLINED_MESSAGE, ENROLLMENT_UNANSWERED_MESSAGE, EnrollmentApprovals,
    EnrollmentDecision, PendingEnrollmentView,
};
use crate::vault::handoff::pairing::random_token;
use crate::vault::handoff::protocol::{DeviceKind, MAX_ARCHIVE_BYTES, PROTOCOL_VERSION};
use crate::vault::handoff::sha256_file;
use base64::Engine;
use std::fs;
use tokio::net::TcpListener;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-transport-{label}-{}",
            random_token("test").expect("random test directory")
        ));
        fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct LocalPair {
    _desktop_root: TestDirectory,
    _phone_root: TestDirectory,
    desktop: PairingManager,
    phone: PairingManager,
    invitation: PairingInvitation,
    approvals: EnrollmentApprovals,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl LocalPair {
    async fn start() -> Self {
        Self::start_with_hub(None).await
    }

    async fn start_with_hub(hub: Option<crate::sync::hub::SyncHub>) -> Self {
        Self::start_with_limits(hub, server::limits::ConnectionLimits::default()).await
    }

    async fn start_with_limits(
        hub: Option<crate::sync::hub::SyncHub>,
        limits: server::limits::ConnectionLimits,
    ) -> Self {
        Self::start_with_options(
            hub,
            limits,
            EnrollmentApprovals::default(),
            LanAdmission::default(),
        )
        .await
    }

    async fn start_with_options(
        hub: Option<crate::sync::hub::SyncHub>,
        limits: server::limits::ConnectionLimits,
        approvals: EnrollmentApprovals,
        admission: LanAdmission,
    ) -> Self {
        let desktop_root = TestDirectory::new("desktop");
        let phone_root = TestDirectory::new("phone");
        let desktop = PairingManager::default();
        desktop
            .initialize(
                desktop_root.path().to_path_buf(),
                "device-desktop".to_string(),
            )
            .expect("initialize desktop");
        let phone = PairingManager::default();
        phone
            .initialize(phone_root.path().to_path_buf(), "device-phone".to_string())
            .expect("initialize phone");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind coordinator");
        let endpoint = listener.local_addr().expect("coordinator endpoint");
        let invitation = desktop
            .create_invitation(
                endpoint,
                "vault-1".to_string(),
                0,
                super::super::protocol::test_compatibility(),
                unix_time_ms(),
            )
            .expect("create invitation");
        let (shutdown, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(server::serve_with_limits(
            listener,
            limits,
            desktop.clone(),
            CoordinatorServices {
                hub,
                approvals: approvals.clone(),
                admission,
                ..CoordinatorServices::default()
            },
            receiver,
        ));
        Self {
            _desktop_root: desktop_root,
            _phone_root: phone_root,
            desktop,
            phone,
            invitation,
            approvals,
            shutdown: Some(shutdown),
        }
    }

    async fn enroll(&self) {
        approving(
            &self.approvals,
            super::enroll(
                &self.phone,
                &self.invitation,
                "Phone".to_string(),
                DeviceKind::Phone,
            ),
        )
        .await
        .expect("enroll phone");
    }

    async fn start_with_coordinator() -> (
        Self,
        tokio::sync::mpsc::Receiver<super::super::coordinator::CoordinatorRequest>,
    ) {
        let (pair, requests, _) = Self::start_with_services().await;
        (pair, requests)
    }

    async fn start_with_services() -> (
        Self,
        tokio::sync::mpsc::Receiver<super::super::coordinator::CoordinatorRequest>,
        tokio::sync::mpsc::Receiver<ContactJob>,
    ) {
        let desktop_root = TestDirectory::new("controlled-desktop");
        let phone_root = TestDirectory::new("controlled-phone");
        let desktop = PairingManager::default();
        desktop
            .initialize(
                desktop_root.path().to_path_buf(),
                "device-desktop".to_string(),
            )
            .expect("initialize desktop");
        let phone = PairingManager::default();
        phone
            .initialize(phone_root.path().to_path_buf(), "device-phone".to_string())
            .expect("initialize phone");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind coordinator");
        let endpoint = listener.local_addr().expect("coordinator endpoint");
        let invitation = desktop
            .create_invitation(
                endpoint,
                "vault-1".to_string(),
                0,
                super::super::protocol::test_compatibility(),
                unix_time_ms(),
            )
            .expect("create invitation");
        let (requests, request_receiver) = tokio::sync::mpsc::channel(8);
        let (contacts, contact_receiver) = super::super::contact_requests::channel();
        let (shutdown, receiver) = tokio::sync::oneshot::channel();
        let approvals = EnrollmentApprovals::default();
        tokio::spawn(serve_with_coordinator(
            listener,
            desktop.clone(),
            CoordinatorServices {
                coordinator: Some(requests),
                contacts: Some(contacts),
                hub: None,
                approvals: approvals.clone(),
                admission: LanAdmission::default(),
            },
            receiver,
        ));
        (
            Self {
                _desktop_root: desktop_root,
                _phone_root: phone_root,
                desktop,
                phone,
                invitation,
                approvals,
                shutdown: Some(shutdown),
            },
            request_receiver,
            contact_receiver,
        )
    }

    fn register_bundle(&self, bytes: &[u8], transfer_id: &str) -> BundleMetadata {
        let path = self._desktop_root.path().join(format!("{transfer_id}.zip"));
        fs::write(&path, bytes).expect("write bundle");
        let metadata = BundleMetadata {
            protocol_version: PROTOCOL_VERSION,
            compatibility: super::super::protocol::test_compatibility(),
            vault_id: "vault-1".to_string(),
            device_id: "device-phone".to_string(),
            transfer_id: transfer_id.to_string(),
            generation: 1,
            archive_bytes: bytes.len() as u64,
            archive_sha256: sha256_file(&path).expect("bundle digest"),
        };
        self.desktop
            .register_outgoing_bundle(metadata.clone(), path)
            .expect("register bundle");
        metadata
    }
}

impl Drop for LocalPair {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

const APPROVAL_POLL_INTERVAL: Duration = Duration::from_millis(5);
const APPROVAL_TEST_WAIT: Duration = Duration::from_secs(10);

/// Waits until a device asks the coordinator user for approval.
async fn wait_for_pending(approvals: &EnrollmentApprovals) -> PendingEnrollmentView {
    tokio::time::timeout(APPROVAL_TEST_WAIT, async {
        loop {
            if let Some(pending) = approvals.pending().expect("pending enrollment") {
                return pending;
            }
            tokio::time::sleep(APPROVAL_POLL_INTERVAL).await;
        }
    })
    .await
    .expect("a device waits for approval")
}

/// Runs an enrollment while the coordinator user approves every request that waits.
async fn approving<T>(approvals: &EnrollmentApprovals, enrollment: impl Future<Output = T>) -> T {
    let approver = approvals.clone();
    let task = tokio::spawn(async move {
        loop {
            let pending = wait_for_pending(&approver).await;
            let _ = approver.decide(&pending.request_id, EnrollmentDecision::Approved);
        }
    });
    let output = enrollment.await;
    task.abort();
    output
}

fn spawn_phone_enrollment(
    pair: &LocalPair,
    invitation: PairingInvitation,
) -> tokio::task::JoinHandle<Result<(), String>> {
    let phone = pair.phone.clone();
    tokio::spawn(async move {
        super::enroll(&phone, &invitation, "Phone".to_string(), DeviceKind::Phone).await
    })
}

#[tokio::test]
async fn enrollment_links_nothing_until_the_coordinator_approves_the_shown_code() {
    let pair = LocalPair::start().await;
    let enrollment = spawn_phone_enrollment(&pair, pair.invitation.clone());
    let pending = wait_for_pending(&pair.approvals).await;
    let (_, phone_identity) = pair.phone.identity().expect("phone identity");
    assert_eq!(
        pending.code,
        crate::vault::handoff::pairing::enrollment_code(
            &pair.invitation,
            phone_identity.certificate.as_ref()
        )
    );
    assert_eq!(pending.device_label, "Phone");
    assert_eq!(pending.device_kind, DeviceKind::Phone);
    assert!(pair.desktop.linked_peers().expect("peers").is_empty());
    assert!(pair.phone.coordinator_pin().expect("pin").is_none());

    pair.approvals
        .decide(&pending.request_id, EnrollmentDecision::Approved)
        .expect("approve");
    enrollment
        .await
        .expect("enrollment task")
        .expect("approved enrollment");
    assert_eq!(pair.desktop.linked_peers().expect("peers").len(), 1);
    assert!(pair.phone.coordinator_pin().expect("pin").is_some());
    assert!(pair.approvals.pending().expect("pending").is_none());
}

#[tokio::test]
async fn declined_enrollment_links_nothing_and_spends_the_invitation() {
    let pair = LocalPair::start().await;
    let enrollment = spawn_phone_enrollment(&pair, pair.invitation.clone());
    let pending = wait_for_pending(&pair.approvals).await;
    pair.approvals
        .decide(&pending.request_id, EnrollmentDecision::Declined)
        .expect("decline");
    let error = enrollment
        .await
        .expect("enrollment task")
        .expect_err("declined enrollment");
    assert_eq!(error, ENROLLMENT_DECLINED_MESSAGE);
    assert!(pair.desktop.linked_peers().expect("peers").is_empty());
    assert!(pair.phone.coordinator_pin().expect("pin").is_none());

    // A retry with the same invitation fails before anyone is asked again.
    let retry = tokio::time::timeout(
        APPROVAL_TEST_WAIT,
        spawn_phone_enrollment(&pair, pair.invitation.clone()),
    )
    .await
    .expect("retry answers without waiting for approval")
    .expect("retry task")
    .expect_err("spent invitation");
    assert!(retry.contains("unknown"), "unexpected retry error: {retry}");
    assert!(pair.desktop.linked_peers().expect("peers").is_empty());
}

#[tokio::test]
async fn unanswered_enrollment_times_out_and_keeps_the_invitation() {
    let pair = LocalPair::start_with_options(
        None,
        server::limits::ConnectionLimits::default(),
        EnrollmentApprovals::with_timeout(Duration::from_millis(100)),
        LanAdmission::default(),
    )
    .await;
    let error = spawn_phone_enrollment(&pair, pair.invitation.clone())
        .await
        .expect("enrollment task")
        .expect_err("unanswered enrollment");
    assert_eq!(error, ENROLLMENT_UNANSWERED_MESSAGE);
    assert!(pair.desktop.linked_peers().expect("peers").is_empty());
    assert!(pair.approvals.pending().expect("pending").is_none());

    pair.enroll().await;
    assert_eq!(pair.desktop.linked_peers().expect("peers").len(), 1);
}

#[tokio::test]
async fn an_unconfirmed_network_reaches_neither_enrollment_nor_the_handshake() {
    let pair = LocalPair::start_with_options(
        None,
        server::limits::ConnectionLimits::default(),
        EnrollmentApprovals::default(),
        LanAdmission::deciding(|| false),
    )
    .await;
    let error = tokio::time::timeout(
        APPROVAL_TEST_WAIT,
        spawn_phone_enrollment(&pair, pair.invitation.clone()),
    )
    .await
    .expect("refused without waiting for approval")
    .expect("enrollment task")
    .expect_err("enrollment on an unconfirmed network");
    assert!(!error.is_empty());
    assert!(pair.approvals.pending().expect("pending").is_none());
    assert!(pair.desktop.linked_peers().expect("peers").is_empty());
}

#[tokio::test]
async fn enrollment_with_a_wrong_secret_is_refused_without_a_prompt() {
    let pair = LocalPair::start().await;
    let mut forged = pair.invitation.clone();
    forged.secret = random_token("secret").expect("forged secret").into();
    let error = tokio::time::timeout(APPROVAL_TEST_WAIT, spawn_phone_enrollment(&pair, forged))
        .await
        .expect("refused without waiting for approval")
        .expect("enrollment task")
        .expect_err("forged enrollment");
    assert!(error.contains("credentials"), "unexpected error: {error}");
    assert!(pair.approvals.pending().expect("pending").is_none());
    assert!(pair.desktop.linked_peers().expect("peers").is_empty());
}

#[tokio::test]
async fn waiting_enrollment_ends_when_the_invitation_is_replaced_or_the_device_leaves() {
    let pair = LocalPair::start().await;
    let enrollment = spawn_phone_enrollment(&pair, pair.invitation.clone());
    wait_for_pending(&pair.approvals).await;
    pair.approvals.cancel();
    let error = enrollment
        .await
        .expect("enrollment task")
        .expect_err("cancelled enrollment");
    assert!(error.contains("replaced"), "unexpected error: {error}");

    let enrollment = spawn_phone_enrollment(&pair, pair.invitation.clone());
    wait_for_pending(&pair.approvals).await;
    enrollment.abort();
    tokio::time::timeout(APPROVAL_TEST_WAIT, async {
        while pair.approvals.pending().expect("pending").is_some() {
            tokio::time::sleep(APPROVAL_POLL_INTERVAL).await;
        }
    })
    .await
    .expect("a closed connection frees the approval slot");
    assert!(pair.desktop.linked_peers().expect("peers").is_empty());
}

#[tokio::test]
async fn fresh_phone_enrolls_and_stages_authenticated_bundle() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    assert!(
        pair.phone
            .coordinator_pin()
            .expect("coordinator pin")
            .expect("coordinator")
            .device_label
            .is_some()
    );
    let bytes = b"authenticated whole-vault test bundle";
    let metadata = pair.register_bundle(bytes, "transfer-fresh");

    let staged = download_bundle(&pair.phone, metadata, &TransferCancellation::default())
        .await
        .expect("download bundle");

    assert_eq!(fs::read(staged).expect("read staged bundle"), bytes);
}

#[tokio::test]
async fn revoked_phone_clears_its_stale_link_after_authenticated_reconnect() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    pair.desktop
        .unlink_device("device-phone")
        .expect("unlink offline phone");

    let error = probe_coordinator(&pair.phone, super::super::protocol::test_compatibility(), 0)
        .await
        .expect_err("revoked phone must be rejected");

    assert!(error.contains("unlinked by the coordinator"));
    assert!(
        pair.phone
            .coordinator_pin()
            .expect("coordinator pin")
            .is_none()
    );
    assert!(
        pair.phone
            .revoked_by_coordinator()
            .expect("revocation status")
    );
    assert!(
        pair.desktop
            .linked_peers()
            .expect("linked peers")
            .is_empty()
    );
}

#[tokio::test]
async fn authenticated_control_flow_prepares_commits_and_acknowledges() {
    use super::super::coordinator::{CoordinatorOperation, CoordinatorResponse};
    use super::super::pairing::PendingAcknowledgement;

    let (pair, mut requests) = LocalPair::start_with_coordinator().await;
    pair.enroll().await;
    let metadata = pair.register_bundle(b"controlled handoff bundle", "transfer-control");
    let expected = metadata.clone();
    let coordinator = tokio::spawn(async move {
        while let Some(request) = requests.recv().await {
            let response = match request.operation {
                CoordinatorOperation::Prepare {
                    compatibility,
                    vault_id,
                    device_id,
                    generation,
                    purpose,
                } => {
                    assert_eq!(compatibility, super::super::protocol::test_compatibility());
                    assert_eq!(vault_id, expected.vault_id);
                    assert_eq!(device_id, expected.device_id);
                    assert_eq!(generation, 0);
                    assert_eq!(purpose, BundlePurpose::Ownership);
                    CoordinatorResponse::Prepared {
                        metadata: expected.clone(),
                        purpose,
                    }
                }
                CoordinatorOperation::CommitOwnership { metadata } => {
                    assert_eq!(metadata, expected);
                    CoordinatorResponse::OwnershipGrant {
                        vault_id: expected.vault_id.clone(),
                        transfer_id: expected.transfer_id.clone(),
                        owner_device_id: expected.device_id.clone(),
                        generation: expected.generation,
                    }
                }
                CoordinatorOperation::Activated {
                    vault_id,
                    device_id,
                    transfer_id,
                    generation,
                    purpose,
                } => {
                    assert_eq!(vault_id, expected.vault_id);
                    assert_eq!(device_id, expected.device_id);
                    assert_eq!(transfer_id, expected.transfer_id);
                    assert_eq!(generation, expected.generation);
                    assert_eq!(purpose, BundlePurpose::Ownership);
                    let response = CoordinatorResponse::ActivationAcknowledged {
                        transfer_id: expected.transfer_id.clone(),
                    };
                    request.response.send(Ok(response)).expect("send response");
                    break;
                }
                operation => panic!("unexpected coordinator operation: {operation:?}"),
            };
            request.response.send(Ok(response)).expect("send response");
        }
    });

    let prepared = request_bundle(
        &pair.phone,
        super::super::protocol::test_compatibility(),
        0,
        BundlePurpose::Ownership,
        &TransferCancellation::default(),
    )
    .await
    .expect("prepare ownership bundle");
    assert_eq!(prepared, metadata);
    let staged = download_bundle(
        &pair.phone,
        prepared.clone(),
        &TransferCancellation::default(),
    )
    .await
    .expect("stage ownership bundle");
    assert_eq!(
        fs::read(staged).expect("read staged bundle"),
        b"controlled handoff bundle"
    );
    assert_eq!(
        commit_staged_ownership(&pair.phone, prepared.clone())
            .await
            .expect("commit ownership"),
        ("device-phone".to_string(), 1)
    );
    acknowledge_activation(
        &pair.phone,
        &PendingAcknowledgement {
            vault_id: prepared.vault_id,
            device_id: prepared.device_id,
            transfer_id: prepared.transfer_id,
            generation: prepared.generation,
            purpose: BundlePurpose::Ownership,
        },
    )
    .await
    .expect("acknowledge activation");
    coordinator.await.expect("coordinator task");
}

#[tokio::test]
async fn authenticated_distractions_exchange_returns_acknowledged_and_combined_samples() {
    use super::super::coordinator::{CoordinatorOperation, CoordinatorResponse};
    use super::super::protocol::DistractionsSampleMessage;

    let (pair, mut requests) = LocalPair::start_with_coordinator().await;
    pair.enroll().await;
    let sample = DistractionsSampleMessage {
        sample_id: "phone-sample".to_string(),
        device_id: "device-phone".to_string(),
        source_type: "mobile-app".to_string(),
        source_key: "com.example.video".to_string(),
        display_name: Some("Video".to_string()),
        started_at_ms: 1_700_000_000_000,
        elapsed_seconds: 30,
        local_date: "2026-09-13".to_string(),
        created_at_ms: 1_700_000_030_000,
    };
    let expected = sample.clone();
    let response_sample = sample.clone();
    let coordinator = tokio::spawn(async move {
        let request = requests.recv().await.expect("Distractions request");
        match request.operation {
            CoordinatorOperation::DistractionsExchange {
                vault_id,
                device_id,
                samples,
                acknowledged_peer_sample_ids,
                owner_snapshot,
            } => {
                assert_eq!(vault_id, "vault-1");
                assert_eq!(device_id, "device-phone");
                assert_eq!(samples, vec![expected]);
                assert!(acknowledged_peer_sample_ids.is_empty());
                assert!(owner_snapshot.is_empty());
            }
            operation => panic!("unexpected coordinator operation: {operation:?}"),
        }
        request
            .response
            .send(Ok(CoordinatorResponse::DistractionsAcknowledged {
                acknowledged_sample_ids: vec!["phone-sample".to_string()],
                peer_samples: Vec::new(),
                combined_samples: vec![response_sample],
            }))
            .expect("send Distractions response");
    });

    let (acknowledged, peer, combined) =
        exchange_distractions(&pair.phone, vec![sample.clone()], Vec::new(), Vec::new())
            .await
            .expect("exchange Distractions usage");
    assert_eq!(acknowledged, vec![sample.sample_id]);
    assert!(peer.is_empty());
    assert_eq!(combined.len(), 1);
    coordinator.await.expect("coordinator task");
}

#[tokio::test]
async fn android_upload_resumes_from_durable_desktop_staging() {
    use super::super::coordinator::{CoordinatorOperation, CoordinatorResponse};
    use super::super::pairing::StoredOutgoingTransfer;

    let (pair, mut requests) = LocalPair::start_with_coordinator().await;
    pair.enroll().await;
    let bytes = vec![73_u8; TRANSFER_CHUNK_BYTES * 2 + 17];
    let transfer_id = "transfer-upload";
    let (_, archive_path) = pair
        .phone
        .outgoing_snapshot_paths(transfer_id)
        .expect("phone outgoing paths");
    fs::write(&archive_path, &bytes).expect("write Android archive");
    let metadata = BundleMetadata {
        protocol_version: PROTOCOL_VERSION,
        compatibility: super::super::protocol::test_compatibility(),
        vault_id: "vault-1".to_string(),
        device_id: "device-desktop".to_string(),
        transfer_id: transfer_id.to_string(),
        generation: 1,
        archive_bytes: bytes.len() as u64,
        archive_sha256: sha256_file(&archive_path).expect("archive digest"),
    };
    pair.phone
        .store_outgoing_transfer(StoredOutgoingTransfer {
            metadata: metadata.clone(),
            purpose: BundlePurpose::Ownership,
            committed: false,
        })
        .expect("persist outgoing transfer");
    let (partial, metadata_path, complete) = pair
        .desktop
        .staging_paths(transfer_id)
        .expect("desktop staging paths");
    fs::write(&partial, &bytes[..TRANSFER_CHUNK_BYTES]).expect("write partial upload");
    pair.desktop
        .write_staging_metadata(&metadata_path, &metadata)
        .expect("persist upload metadata");

    let restarted = PairingManager::default();
    restarted
        .initialize(
            pair._desktop_root.path().to_path_buf(),
            "device-desktop".to_string(),
        )
        .expect("restart desktop pairing state");
    assert_eq!(
        restarted
            .read_staging_metadata(&metadata_path)
            .expect("read restarted staging metadata"),
        Some(metadata.clone())
    );

    let expected = metadata.clone();
    let coordinator = tokio::spawn(async move {
        while let Some(request) = requests.recv().await {
            let response = match request.operation {
                CoordinatorOperation::PollUpload {
                    compatibility,
                    vault_id,
                    device_id,
                    generation,
                } => {
                    assert_eq!(compatibility, super::super::protocol::test_compatibility());
                    assert_eq!(vault_id, expected.vault_id);
                    assert_eq!(device_id, "device-phone");
                    assert_eq!(generation, 0);
                    CoordinatorResponse::UploadStatus {
                        generation,
                        requested_upload: Some(BundlePurpose::Ownership),
                    }
                }
                CoordinatorOperation::AuthorizeUpload {
                    metadata,
                    source_device_id,
                    purpose,
                } => {
                    assert_eq!(metadata, expected);
                    assert_eq!(source_device_id, "device-phone");
                    assert_eq!(purpose, BundlePurpose::Ownership);
                    CoordinatorResponse::UploadAuthorized {
                        transfer_id: expected.transfer_id.clone(),
                        already_received: false,
                    }
                }
                CoordinatorOperation::Uploaded {
                    metadata,
                    source_device_id,
                    purpose,
                } => {
                    assert_eq!(metadata, expected);
                    assert_eq!(source_device_id, "device-phone");
                    assert_eq!(purpose, BundlePurpose::Ownership);
                    CoordinatorResponse::IncomingStaged {
                        transfer_id: expected.transfer_id.clone(),
                    }
                }
                CoordinatorOperation::CommitUploadedOwnership {
                    metadata,
                    source_device_id,
                } => {
                    assert_eq!(metadata, expected);
                    assert_eq!(source_device_id, "device-phone");
                    let response = CoordinatorResponse::ActivationAcknowledged {
                        transfer_id: expected.transfer_id.clone(),
                    };
                    request.response.send(Ok(response)).expect("send response");
                    break;
                }
                operation => panic!("unexpected coordinator operation: {operation:?}"),
            };
            request.response.send(Ok(response)).expect("send response");
        }
    });

    assert_eq!(
        probe_coordinator(&pair.phone, super::super::protocol::test_compatibility(), 0,)
            .await
            .expect("poll coordinator"),
        Some(BundlePurpose::Ownership)
    );
    upload_bundle(
        &pair.phone,
        metadata.clone(),
        "device-phone".to_string(),
        BundlePurpose::Ownership,
    )
    .await
    .expect("resume Android upload");
    assert_eq!(fs::read(complete).expect("read completed upload"), bytes);
    commit_uploaded_ownership(&pair.phone, metadata, "device-phone".to_string())
        .await
        .expect("acknowledge uploaded ownership");
    coordinator.await.expect("coordinator task");
}

#[tokio::test]
async fn foreground_poll_observes_a_new_upload_request_without_reconnecting() {
    use super::super::coordinator::{CoordinatorOperation, CoordinatorResponse};

    let (pair, mut requests) = LocalPair::start_with_coordinator().await;
    pair.enroll().await;

    let coordinator = tokio::spawn(async move {
        let mut poll_count = 0;
        while let Some(request) = requests.recv().await {
            let CoordinatorOperation::PollUpload { generation, .. } = request.operation else {
                panic!("unexpected coordinator operation");
            };
            poll_count += 1;
            request
                .response
                .send(Ok(CoordinatorResponse::UploadStatus {
                    generation,
                    requested_upload: (poll_count >= 3).then_some(BundlePurpose::Refresh),
                }))
                .expect("send upload status");
            if poll_count >= 3 {
                return poll_count;
            }
        }
        poll_count
    });

    let purpose = tokio::time::timeout(
        Duration::from_secs(2),
        probe_coordinator(&pair.phone, super::super::protocol::test_compatibility(), 0),
    )
    .await
    .expect("foreground poll should remain responsive")
    .expect("poll coordinator");
    assert_eq!(purpose, Some(BundlePurpose::Refresh));
    assert_eq!(coordinator.await.expect("coordinator task"), 3);
}

#[tokio::test]
async fn invalid_device_identity_cannot_download_bundle() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let metadata = pair.register_bundle(b"private vault", "transfer-identity");

    let attacker_root = TestDirectory::new("attacker");
    let attacker = PairingManager::default();
    attacker
        .initialize(
            attacker_root.path().to_path_buf(),
            "device-attacker".to_string(),
        )
        .expect("initialize attacker");
    let (_, desktop_identity) = pair.desktop.identity().expect("desktop identity");
    attacker
        .record_coordinator(
            &pair.invitation,
            desktop_identity.certificate.as_ref(),
            Some("Desktop".to_string()),
        )
        .expect("record coordinator pin");
    let mut forged = metadata;
    forged.device_id = "device-phone".to_string();

    let error = download_bundle(&attacker, forged, &TransferCancellation::default())
        .await
        .unwrap_err();
    assert!(
        error.contains("TLS")
            || error.contains("certificate")
            || error.contains("alert")
            || error.contains("closed connection"),
        "unexpected identity error: {error}"
    );
}

#[tokio::test]
async fn coordinator_refuses_tls_1_2_even_from_a_linked_device() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let (_, identity) = pair.phone.identity().expect("phone identity");
    let config = rustls::ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS12])
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedServerVerifier::new(
            pair.invitation.coordinator_fingerprint.clone(),
        )))
        .with_client_auth_cert(vec![identity.certificate], identity.private_key.clone_key())
        .expect("configure TLS 1.2 client");
    let socket = TcpStream::connect(&pair.invitation.endpoint)
        .await
        .expect("connect to coordinator");
    let server_name = ServerName::try_from("ganbaru-coordinator.local").expect("server name");

    let handshake = tokio::time::timeout(
        TLS_HANDSHAKE_TIMEOUT,
        TlsConnector::from(Arc::new(config)).connect(server_name, socket),
    )
    .await
    .expect("handshake finishes");

    assert!(handshake.is_err(), "TLS 1.2 handshake must be refused");
}

#[tokio::test]
async fn non_lan_dial_targets_are_refused_before_connecting() {
    for endpoint in [
        "203.0.113.5:443",
        "8.8.8.8:43821",
        "224.0.0.251:43821",
        "[2001:db8::5]:43821",
    ] {
        let started = std::time::Instant::now();
        let Err(error) =
            connect_pinned(endpoint.parse().expect("endpoint"), &"b".repeat(64), None).await
        else {
            panic!("{endpoint} must be refused");
        };
        assert!(
            error.starts_with("refuse device connection"),
            "{endpoint}: {error}"
        );
        let error = unauthenticated_exchange(
            endpoint,
            &"b".repeat(64),
            ControlMessage::Error {
                code: "test".to_string(),
                message: "test".to_string(),
                retryable: false,
            },
        )
        .await
        .unwrap_err();
        assert!(
            error.starts_with("refuse device connection"),
            "{endpoint}: {error}"
        );
        assert!(
            started.elapsed() < TLS_HANDSHAKE_TIMEOUT,
            "{endpoint} attempted a connection"
        );
    }
    assert_eq!(
        authorize_endpoint("100.64.3.4:43821".parse().expect("endpoint")),
        Ok(())
    );
    assert_eq!(
        authorize_endpoint("192.168.1.20:43821".parse().expect("endpoint")),
        Ok(())
    );
}

#[tokio::test]
async fn invitation_replay_is_rejected_by_coordinator() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let error = super::enroll(
        &pair.phone,
        &pair.invitation,
        "Phone".to_string(),
        DeviceKind::Phone,
    )
    .await
    .unwrap_err();
    assert!(
        error.contains("already used") || error.contains("unknown"),
        "unexpected replay error: {error}"
    );
}

#[tokio::test]
async fn several_enrolled_devices_authenticate_with_distinct_certificates() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let second_root = TestDirectory::new("second-client");
    let second = PairingManager::default();
    second
        .initialize(
            second_root.path().to_path_buf(),
            "device-laptop".to_string(),
        )
        .expect("initialize second client");
    let invitation = pair
        .desktop
        .create_invitation(
            pair.invitation.endpoint.parse().expect("endpoint"),
            "vault-1".to_string(),
            0,
            super::super::protocol::test_compatibility(),
            unix_time_ms(),
        )
        .expect("create second invitation");
    approving(
        &pair.approvals,
        super::enroll(
            &second,
            &invitation,
            "Laptop".to_string(),
            DeviceKind::Computer,
        ),
    )
    .await
    .expect("enroll second client");

    let peers = pair.desktop.linked_peers().expect("linked devices");
    assert_eq!(peers.len(), 2);
    assert_ne!(
        peers[0].certificate_fingerprint,
        peers[1].certificate_fingerprint
    );
    let error = probe_coordinator(&second, super::super::protocol::test_compatibility(), 0)
        .await
        .expect_err("test server has no coordinator state");
    assert!(
        error.contains("coordinator operations are unavailable"),
        "authenticated request failed before coordinator dispatch: {error}"
    );
}

#[tokio::test]
async fn malformed_and_oversized_metadata_is_rejected_by_endpoint() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let coordinator = pair
        .phone
        .coordinator_pin()
        .expect("coordinator state")
        .expect("coordinator pin");
    let (_, phone_identity) = pair.phone.identity().expect("phone identity");

    for (archive_bytes, digest) in [
        (10_u64, "broken".to_string()),
        (MAX_ARCHIVE_BYTES + 1, "a".repeat(64)),
    ] {
        let mut stream = connect_pinned(
            coordinator.endpoint.parse().expect("endpoint"),
            &coordinator.certificate_fingerprint,
            Some(phone_identity.clone()),
        )
        .await
        .expect("connect authenticated phone");
        let invalid = serde_json::json!({
            "kind": "downloadBundle",
            "metadata": {
                "protocolVersion": PROTOCOL_VERSION,
                "vaultId": "vault-1",
                "deviceId": "device-phone",
                "transferId": "transfer-invalid",
                "generation": 1,
                "archiveBytes": archive_bytes,
                "archiveSha256": digest,
            }
        });
        let encoded = serde_json::to_vec(&invalid).expect("encode invalid request");
        stream
            .write_u32(encoded.len() as u32)
            .await
            .expect("write invalid message length");
        stream
            .write_all(&encoded)
            .await
            .expect("write invalid message");
        match read_control(&mut stream)
            .await
            .expect("bounded error response")
        {
            ControlMessage::Error { code, .. } => assert_eq!(code, "malformed_control"),
            response => panic!("unexpected response: {response:?}"),
        }
    }
}

#[tokio::test]
async fn interrupted_download_retries_same_transfer_from_partial_file() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let bytes = vec![42_u8; TRANSFER_CHUNK_BYTES * 3];
    let metadata = pair.register_bundle(&bytes, "transfer-resume");
    let cancellation = TransferCancellation::default();

    let error = download_bundle_inner(
        &pair.phone,
        metadata.clone(),
        &cancellation,
        Some(TRANSFER_CHUNK_BYTES as u64),
    )
    .await
    .unwrap_err();
    assert!(error.contains("interrupted"));
    let (partial, _, _) = pair
        .phone
        .staging_paths(&metadata.transfer_id)
        .expect("staging paths");
    let partial_bytes = partial.metadata().expect("partial bundle").len();
    assert!(partial_bytes > 0 && partial_bytes < metadata.archive_bytes);

    let staged = download_bundle(&pair.phone, metadata, &cancellation)
        .await
        .expect("resume bundle");
    assert_eq!(fs::read(staged).expect("read resumed bundle"), bytes);
}

#[tokio::test]
async fn cancelled_download_keeps_a_retryable_transfer_identity() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let bytes = vec![21_u8; TRANSFER_CHUNK_BYTES + 1];
    let metadata = pair.register_bundle(&bytes, "transfer-cancel");
    let cancellation = TransferCancellation::default();
    cancellation.cancel();

    let error = download_bundle(&pair.phone, metadata.clone(), &cancellation)
        .await
        .unwrap_err();
    assert!(error.contains("cancelled"));

    let staged = download_bundle(&pair.phone, metadata, &TransferCancellation::default())
        .await
        .expect("retry cancelled transfer");
    assert_eq!(fs::read(staged).expect("read retried bundle"), bytes);
}

#[tokio::test]
async fn digest_mismatch_removes_corrupt_staging() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let original = vec![7_u8; TRANSFER_CHUNK_BYTES + 1];
    let metadata = pair.register_bundle(&original, "transfer-digest");
    let registered = pair
        .desktop
        .outgoing_bundle(&metadata.transfer_id)
        .expect("registered bundle");
    fs::write(&registered.path, vec![8_u8; original.len()]).expect("corrupt source");

    let error = download_bundle(
        &pair.phone,
        metadata.clone(),
        &TransferCancellation::default(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("digest"), "unexpected digest error: {error}");
    let (partial, metadata_path, complete) = pair
        .phone
        .staging_paths(&metadata.transfer_id)
        .expect("staging paths");
    assert!(!partial.exists());
    assert!(!metadata_path.exists());
    assert!(!complete.exists());
}

#[test]
fn oversized_bundle_metadata_is_rejected_before_network_work() {
    let metadata = BundleMetadata {
        protocol_version: PROTOCOL_VERSION,
        compatibility: super::super::protocol::test_compatibility(),
        vault_id: "vault-1".to_string(),
        device_id: "device-phone".to_string(),
        transfer_id: "transfer-oversize".to_string(),
        generation: 1,
        archive_bytes: MAX_ARCHIVE_BYTES + 1,
        archive_sha256: "a".repeat(64),
    };
    assert!(metadata.validate().unwrap_err().contains("bundle size"));
}

fn sample_contact_card() -> (ganbaru_contacts::PersonKeyPair, String) {
    let (_, key) = ganbaru_contacts::PersonKeyPair::generate().expect("person key");
    let card = ganbaru_contacts::ContactCard {
        public_key: key.public_key(),
        display_name: "Requester".to_string(),
        color: 3,
        nonce: [4u8; ganbaru_contacts::CARD_NONCE_BYTES],
        endpoint_hint: "127.0.0.1:43821".to_string(),
        coordinator_fingerprint: Some([5u8; 32]),
        issued_at_ms: unix_time_ms(),
    };
    let signed = ganbaru_contacts::sign_card(&card, &key).expect("sign card");
    (key, signed.text())
}

fn base64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[tokio::test]
async fn unauthenticated_contact_request_is_forwarded_to_the_contact_worker() {
    let (pair, _requests, mut contacts) = LocalPair::start_with_services().await;
    let (key, card) = sample_contact_card();
    let recipient_nonce = [7u8; ganbaru_contacts::CARD_NONCE_BYTES];
    let signature = key.sign(&ganbaru_contacts::request_signature_payload(
        &recipient_nonce,
        "contact-one",
    ));
    let expected_card = card.clone();
    let coordinator = tokio::spawn(async move {
        let job = contacts.recv().await.expect("contact request");
        match job.operation {
            ContactOperation::Request {
                recipient_card_nonce,
                requester_card,
                request_id,
                ..
            } => {
                assert_eq!(
                    recipient_card_nonce,
                    ganbaru_contacts::encode_nonce(&recipient_nonce)
                );
                assert_eq!(requester_card, expected_card);
                assert_eq!(request_id, "contact-one");
            }
            operation => panic!("unexpected contact operation: {operation:?}"),
        }
        job.response
            .send(Ok(PeerReply::Received {
                request_id: "contact-one".to_string(),
            }))
            .expect("send contact response");
    });

    let response = unauthenticated_exchange(
        &pair.invitation.endpoint,
        &pair.invitation.coordinator_fingerprint,
        ControlMessage::ContactRequest {
            protocol_version: PROTOCOL_VERSION,
            recipient_card_nonce: ganbaru_contacts::encode_nonce(&recipient_nonce),
            requester_card: card,
            request_id: "contact-one".to_string(),
            signature: base64url(&signature),
        },
    )
    .await
    .expect("contact request exchange");
    assert!(matches!(
        response,
        ControlMessage::ContactRequestReceived { request_id } if request_id == "contact-one"
    ));
    coordinator.await.expect("coordinator task");
}

#[tokio::test]
async fn contact_request_answers_wait_for_the_same_minimum_time_whatever_the_outcome() {
    let (pair, _requests, mut contacts) = LocalPair::start_with_services().await;
    let (key, card) = sample_contact_card();
    let recipient_nonce = [7u8; ganbaru_contacts::CARD_NONCE_BYTES];
    let replies = [
        PeerReply::Received {
            request_id: "contact-one".to_string(),
        },
        PeerReply::Rejected {
            code: "invalid_request",
            message: "the contact request is invalid",
            retryable: false,
        },
    ];
    let coordinator = tokio::spawn(async move {
        for reply in replies {
            let job = contacts.recv().await.expect("contact request");
            job.response.send(Ok(reply)).expect("send contact response");
        }
    });

    for _ in 0..2 {
        let signature = key.sign(&ganbaru_contacts::request_signature_payload(
            &recipient_nonce,
            "contact-one",
        ));
        let started = std::time::Instant::now();
        let _ = unauthenticated_exchange(
            &pair.invitation.endpoint,
            &pair.invitation.coordinator_fingerprint,
            ControlMessage::ContactRequest {
                protocol_version: PROTOCOL_VERSION,
                recipient_card_nonce: ganbaru_contacts::encode_nonce(&recipient_nonce),
                requester_card: card.clone(),
                request_id: "contact-one".to_string(),
                signature: base64url(&signature),
            },
        )
        .await;
        assert!(
            started.elapsed()
                >= crate::vault::handoff::contact_requests::MIN_CONTACT_REQUEST_RESPONSE
        );
    }
    coordinator.await.expect("coordinator task");
}

#[tokio::test]
async fn contact_rejection_reaches_the_requester_as_a_protocol_error() {
    let (pair, _requests, mut contacts) = LocalPair::start_with_services().await;
    let (key, _) = sample_contact_card();
    let issued_at_ms = unix_time_ms();
    let signature = key.sign(&ganbaru_contacts::status_signature_payload(
        "contact-one",
        issued_at_ms,
    ));
    let coordinator = tokio::spawn(async move {
        let job = contacts.recv().await.expect("status request");
        assert!(matches!(
            job.operation,
            ContactOperation::Status { ref request_id, .. } if request_id == "contact-one"
        ));
        job.response
            .send(Ok(PeerReply::Rejected {
                code: "card_revoked",
                message: "the recipient regenerated their contact card",
                retryable: false,
            }))
            .expect("send rejection");
    });

    let response = unauthenticated_exchange(
        &pair.invitation.endpoint,
        &pair.invitation.coordinator_fingerprint,
        ControlMessage::ContactRequestStatus {
            protocol_version: PROTOCOL_VERSION,
            request_id: "contact-one".to_string(),
            requester_public_key: key.public_key().to_text(),
            issued_at_ms,
            signature: base64url(&signature),
        },
    )
    .await
    .expect("status exchange");
    assert!(matches!(
        response,
        ControlMessage::Error { code, retryable: false, .. } if code == "card_revoked"
    ));
    coordinator.await.expect("coordinator task");
}

fn contact_request_message(request_id: &str) -> ControlMessage {
    let (key, card) = sample_contact_card();
    let recipient_nonce = [7u8; ganbaru_contacts::CARD_NONCE_BYTES];
    let signature = key.sign(&ganbaru_contacts::request_signature_payload(
        &recipient_nonce,
        request_id,
    ));
    ControlMessage::ContactRequest {
        protocol_version: PROTOCOL_VERSION,
        recipient_card_nonce: ganbaru_contacts::encode_nonce(&recipient_nonce),
        requester_card: card,
        request_id: request_id.to_string(),
        signature: base64url(&signature),
    }
}

fn is_busy(message: &ControlMessage) -> bool {
    matches!(
        message,
        ControlMessage::Error { code, retryable: true, .. } if code == RECIPIENT_BUSY_CODE
    )
}

#[tokio::test]
async fn contact_request_flood_does_not_delay_coordinator_polls() {
    use super::super::coordinator::{CoordinatorOperation, CoordinatorResponse};

    let (pair, mut requests, mut contacts) = LocalPair::start_with_services().await;
    pair.enroll().await;
    let slow_worker = tokio::spawn(async move {
        while let Some(job) = contacts.recv().await {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let request_id = match job.operation {
                ContactOperation::Request { request_id, .. } => request_id,
                operation => panic!("unexpected contact operation: {operation:?}"),
            };
            let _ = job.response.send(Ok(PeerReply::Received { request_id }));
        }
    });
    let coordinator = tokio::spawn(async move {
        while let Some(request) = requests.recv().await {
            let CoordinatorOperation::PollUpload { generation, .. } = request.operation else {
                panic!("unexpected coordinator operation");
            };
            let _ = request.response.send(Ok(CoordinatorResponse::UploadStatus {
                generation,
                requested_upload: Some(BundlePurpose::Refresh),
            }));
        }
    });

    // Stays under the per-source pending connection cap so the poll can still connect.
    let flood: Vec<_> = (0..6)
        .map(|index| {
            let endpoint = pair.invitation.endpoint.clone();
            let fingerprint = pair.invitation.coordinator_fingerprint.clone();
            tokio::spawn(async move {
                unauthenticated_exchange(
                    &endpoint,
                    &fingerprint,
                    contact_request_message(&format!("flood-{index}")),
                )
                .await
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let purpose = tokio::time::timeout(
        Duration::from_secs(2),
        probe_coordinator(&pair.phone, super::super::protocol::test_compatibility(), 0),
    )
    .await
    .expect("coordinator polls stay responsive during a contact flood")
    .expect("poll coordinator");
    assert_eq!(purpose, Some(BundlePurpose::Refresh));

    for exchange in flood {
        let response = exchange.await.expect("flood task").expect("flood exchange");
        assert!(
            matches!(response, ControlMessage::ContactRequestReceived { .. }) || is_busy(&response),
            "unexpected flood response: {response:?}"
        );
    }
    slow_worker.abort();
    coordinator.abort();
}

#[tokio::test]
async fn contact_requests_past_the_source_budget_get_a_busy_answer() {
    let (pair, _requests, mut contacts) = LocalPair::start_with_services().await;
    let worker = tokio::spawn(async move {
        while let Some(job) = contacts.recv().await {
            let request_id = match job.operation {
                ContactOperation::Request { request_id, .. } => request_id,
                operation => panic!("unexpected contact operation: {operation:?}"),
            };
            let _ = job.response.send(Ok(PeerReply::Received { request_id }));
        }
    });

    let mut received = 0;
    let mut busy = 0;
    for index in 0..24 {
        let response = unauthenticated_exchange(
            &pair.invitation.endpoint,
            &pair.invitation.coordinator_fingerprint,
            contact_request_message(&format!("burst-{index}")),
        )
        .await
        .expect("burst exchange");
        if is_busy(&response) {
            busy += 1;
        } else {
            assert!(
                matches!(response, ControlMessage::ContactRequestReceived { .. }),
                "unexpected burst response: {response:?}"
            );
            received += 1;
        }
    }
    assert!(
        received >= server::limits::CONTACT_MESSAGE_BURST as usize,
        "the burst allowance is honored"
    );
    assert!(busy > 0, "messages past the budget are refused as busy");
    worker.abort();
}

#[tokio::test]
async fn person_key_is_released_only_to_an_enrolled_device() {
    use super::super::coordinator::{CoordinatorOperation, CoordinatorResponse};

    let (pair, mut requests) = LocalPair::start_with_coordinator().await;
    let unauthenticated = unauthenticated_exchange(
        &pair.invitation.endpoint,
        &pair.invitation.coordinator_fingerprint,
        ControlMessage::PersonKeyRequest {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-1".to_string(),
            device_id: "device-phone".to_string(),
        },
    )
    .await;
    assert!(
        unauthenticated.is_err(),
        "a connection without a client certificate must not receive the key"
    );

    pair.enroll().await;
    let (pkcs8, key) = ganbaru_contacts::PersonKeyPair::generate().expect("person key");
    let public_key = key.public_key().to_text();
    let released_key = public_key.clone();
    let released_pkcs8 = base64url(&pkcs8);
    let expected_pkcs8 = released_pkcs8.clone();
    let coordinator = tokio::spawn(async move {
        let request = requests.recv().await.expect("key request");
        assert!(matches!(
            request.operation,
            CoordinatorOperation::ReleasePersonKey { ref vault_id, ref device_id }
                if vault_id == "vault-1" && device_id == "device-phone"
        ));
        request
            .response
            .send(Ok(CoordinatorResponse::PersonKeyReleased {
                public_key: released_key,
                private_key_pkcs8: released_pkcs8,
            }))
            .expect("send key");
    });

    let response = request_person_key(&pair.phone).await.expect("key exchange");
    match response {
        ControlMessage::PersonKeyRelease {
            public_key: released,
            private_key_pkcs8,
        } => {
            assert_eq!(released, public_key);
            assert_eq!(private_key_pkcs8, expected_pkcs8);
        }
        other => panic!("unexpected response: {other:?}"),
    }
    coordinator.await.expect("coordinator task");
}

#[tokio::test]
async fn sync_requests_reach_the_hub_only_from_enrolled_devices() {
    use crate::sync::hub::{OpenHubVault, SyncHub};

    let open: OpenHubVault = std::sync::Arc::new(|| Box::pin(async { Ok(None) }));
    let hub = SyncHub::new(open, tokio::sync::watch::Sender::new(0));
    let pair = LocalPair::start_with_hub(Some(hub)).await;
    let hello = |protocol_version| ControlMessage::SyncHello {
        protocol_version,
        vault_id: "vault-1".to_string(),
        device_id: "device-phone".to_string(),
        manifest_version: 1,
        stored: Vec::new(),
        probe: None,
    };
    let unauthenticated = unauthenticated_exchange(
        &pair.invitation.endpoint,
        &pair.invitation.coordinator_fingerprint,
        hello(PROTOCOL_VERSION),
    )
    .await;
    assert!(
        unauthenticated.is_err(),
        "a connection without a client certificate must not reach the hub"
    );

    pair.enroll().await;
    assert_eq!(
        sync_peer(&pair.phone).expect("sync peer"),
        Some(("vault-1".to_string(), "device-phone".to_string()))
    );
    let response = sync_request(&pair.phone, hello(PROTOCOL_VERSION))
        .await
        .expect("sync exchange");
    assert!(matches!(
        response,
        ControlMessage::Error { code, retryable: true, .. } if code == "sync_unavailable"
    ));
    assert!(
        hello(PROTOCOL_VERSION - 1).validate().is_err(),
        "sync messages of an older protocol are refused at the frame boundary"
    );
}

#[tokio::test]
async fn coordinator_without_a_hub_refuses_sync_requests() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let response = sync_request(
        &pair.phone,
        ControlMessage::SyncWait {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-1".to_string(),
            device_id: "device-phone".to_string(),
            known: Vec::new(),
        },
    )
    .await
    .expect("sync exchange");
    assert!(matches!(
        response,
        ControlMessage::Error { code, retryable: false, .. } if code == "sync_unsupported"
    ));
}

/// Opens raw TCP connections from `source` that never start TLS, as a flooding LAN host would.
#[cfg(target_os = "linux")]
async fn open_idle_connections(
    endpoint: SocketAddr,
    source: IpAddr,
    count: usize,
) -> Vec<TcpStream> {
    let mut streams = Vec::with_capacity(count);
    for _ in 0..count {
        let socket = tokio::net::TcpSocket::new_v4().expect("attacker socket");
        socket
            .bind(SocketAddr::new(source, 0))
            .expect("bind attacker source");
        streams.push(socket.connect(endpoint).await.expect("attacker connect"));
    }
    streams
}

/// Whether the coordinator closed the stream before `wait` elapsed.
#[cfg(target_os = "linux")]
async fn closed_by_coordinator(stream: &mut TcpStream, wait: Duration) -> bool {
    let mut byte = [0_u8; 1];
    matches!(
        tokio::time::timeout(wait, stream.read(&mut byte)).await,
        Ok(Ok(0) | Err(_))
    )
}

// Binding a second loopback source address works on Linux without configuration.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn connection_flood_is_closed_while_linked_devices_still_connect() {
    let limits = server::limits::ConnectionLimits::new(6, 3, 64);
    let pair = LocalPair::start_with_limits(None, limits).await;
    pair.enroll().await;
    let endpoint: SocketAddr = pair.invitation.endpoint.parse().expect("endpoint");

    let mut flood = open_idle_connections(endpoint, IpAddr::from([127, 0, 0, 2]), 10).await;
    let mut closed = 0;
    for stream in &mut flood {
        if closed_by_coordinator(stream, Duration::from_millis(500)).await {
            closed += 1;
        }
    }
    assert_eq!(closed, 7, "only the per-source pending budget stays open");

    let error = probe_coordinator(&pair.phone, super::super::protocol::test_compatibility(), 0)
        .await
        .expect_err("test server has no coordinator state");
    assert!(
        error.contains("coordinator operations are unavailable"),
        "a linked device must still reach dispatch during a flood: {error}"
    );
    drop(flood);
}

#[tokio::test]
async fn accept_errors_back_off_instead_of_stopping_the_listener() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let endpoint = listener.local_addr().expect("listener endpoint");
    let failures = std::sync::atomic::AtomicUsize::new(3);
    let (accepted, mut accepted_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let listener = &listener;
    let failures = &failures;
    let serve = server::accept_connections(
        move || async move {
            if failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_ok()
            {
                // EMFILE on Unix and WSAEMFILE on Windows: the process ran out of descriptors.
                return Err(std::io::Error::from_raw_os_error(if cfg!(windows) {
                    10024
                } else {
                    24
                }));
            }
            listener.accept().await
        },
        receiver,
        move |_socket: TcpStream, source| {
            let _ = accepted.send(source);
        },
    );
    let client = async {
        let _stream = TcpStream::connect(endpoint).await.expect("connect");
        let source = tokio::time::timeout(Duration::from_secs(10), accepted_receiver.recv())
            .await
            .expect("the listener accepted after transient errors");
        assert!(source.is_some());
        let _ = shutdown.send(());
    };
    tokio::join!(serve, client);
    assert_eq!(failures.load(Ordering::SeqCst), 0);
}
