use super::*;
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
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl LocalPair {
    async fn start() -> Self {
        Self::start_with_hub(None).await
    }

    async fn start_with_hub(hub: Option<crate::sync::hub::SyncHub>) -> Self {
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
        tokio::spawn(serve_with_coordinator(
            listener,
            desktop.clone(),
            None,
            hub,
            receiver,
        ));
        Self {
            _desktop_root: desktop_root,
            _phone_root: phone_root,
            desktop,
            phone,
            invitation,
            shutdown: Some(shutdown),
        }
    }

    async fn enroll(&self) {
        super::enroll(
            &self.phone,
            &self.invitation,
            "Phone".to_string(),
            DeviceKind::Phone,
        )
        .await
        .expect("enroll phone");
    }

    async fn start_with_coordinator() -> (
        Self,
        tokio::sync::mpsc::Receiver<super::super::coordinator::CoordinatorRequest>,
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
        let (shutdown, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(serve_with_coordinator(
            listener,
            desktop.clone(),
            Some(requests),
            None,
            receiver,
        ));
        (
            Self {
                _desktop_root: desktop_root,
                _phone_root: phone_root,
                desktop,
                phone,
                invitation,
                shutdown: Some(shutdown),
            },
            request_receiver,
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
    super::enroll(
        &second,
        &invitation,
        "Laptop".to_string(),
        DeviceKind::Computer,
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
async fn unauthenticated_contact_request_is_forwarded_to_the_coordinator() {
    use super::super::coordinator::{CoordinatorOperation, CoordinatorResponse};

    let (pair, mut requests) = LocalPair::start_with_coordinator().await;
    let (key, card) = sample_contact_card();
    let recipient_nonce = [7u8; ganbaru_contacts::CARD_NONCE_BYTES];
    let signature = key.sign(&ganbaru_contacts::request_signature_payload(
        &recipient_nonce,
        "contact-one",
    ));
    let expected_card = card.clone();
    let coordinator = tokio::spawn(async move {
        let request = requests.recv().await.expect("contact request");
        match request.operation {
            CoordinatorOperation::ContactRequest {
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
            operation => panic!("unexpected coordinator operation: {operation:?}"),
        }
        request
            .response
            .send(Ok(CoordinatorResponse::ContactRequestReceived {
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
async fn coordinator_rejection_reaches_the_requester_as_a_protocol_error() {
    use super::super::coordinator::{CoordinatorOperation, CoordinatorResponse};

    let (pair, mut requests) = LocalPair::start_with_coordinator().await;
    let (key, _) = sample_contact_card();
    let issued_at_ms = unix_time_ms();
    let signature = key.sign(&ganbaru_contacts::status_signature_payload(
        "contact-one",
        issued_at_ms,
    ));
    let coordinator = tokio::spawn(async move {
        let request = requests.recv().await.expect("status request");
        assert!(matches!(
            request.operation,
            CoordinatorOperation::ContactRequestStatus { ref request_id, .. }
                if request_id == "contact-one"
        ));
        request
            .response
            .send(Ok(CoordinatorResponse::Rejected {
                code: "card_revoked".to_string(),
                message: "the recipient regenerated their contact card".to_string(),
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
            CoordinatorOperation::ReleasePersonKey
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
