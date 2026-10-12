use super::*;
use std::path::Path;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-handoff-{label}-{}",
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

fn enrollment<'a>(invitation: &'a PairingInvitation, phone: &'a StoredIdentity) -> Enrollment<'a> {
    Enrollment {
        invitation_id: &invitation.invitation_id,
        secret: &invitation.secret,
        vault_id: &invitation.vault_id,
        device_id: "device-phone",
        device_label: "Phone",
        device_kind: DeviceKind::Phone,
        certificate_b64: &phone.certificate,
    }
}

fn enroll_device(manager: &PairingManager, device_id: &str, device_label: &str, now_ms: i64) {
    let identity = create_identity(device_id.to_string()).expect("peer identity");
    enroll_identity(manager, "vault-1", &identity, device_label, now_ms).expect("enroll device");
}

fn enroll_identity(
    manager: &PairingManager,
    vault_id: &str,
    identity: &StoredIdentity,
    device_label: &str,
    now_ms: i64,
) -> Result<LinkedPeer, String> {
    let invitation = manager
        .create_invitation(
            "127.0.0.1:41000".parse().expect("endpoint"),
            vault_id.to_string(),
            0,
            crate::protocol::test_compatibility(),
            now_ms,
        )
        .expect("invitation");
    manager.enroll_peer(
        Enrollment {
            invitation_id: &invitation.invitation_id,
            secret: &invitation.secret,
            vault_id: &invitation.vault_id,
            device_id: &identity.device_id,
            device_label,
            device_kind: if device_label == "Phone" {
                DeviceKind::Phone
            } else {
                DeviceKind::Computer
            },
            certificate_b64: &identity.certificate,
        },
        now_ms + 1,
    )
}

#[test]
fn invitation_is_single_use_and_not_restored_after_restart() {
    let temporary = TestDirectory::new("single-use");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let invitation = manager
        .create_invitation(
            "127.0.0.1:41000".parse().expect("endpoint"),
            "vault-1".to_string(),
            0,
            crate::protocol::test_compatibility(),
            100,
        )
        .expect("invitation");
    assert_eq!(invitation.expires_at_ms, 180_100);
    let phone = create_identity("device-phone".to_string()).expect("phone identity");
    manager
        .enroll_peer(enrollment(&invitation, &phone), 101)
        .expect("enroll");
    assert!(
        manager
            .enroll_peer(enrollment(&invitation, &phone), 102)
            .unwrap_err()
            .contains("already used")
    );

    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("restart");
    assert!(
        restarted
            .enroll_peer(enrollment(&invitation, &phone), 103)
            .unwrap_err()
            .contains("unknown")
    );
}

#[test]
fn repeated_enrollment_updates_one_existing_membership() {
    let temporary = TestDirectory::new("repeat-enrollment");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let phone = create_identity("device-phone".to_string()).expect("phone identity");

    for now_ms in [100, 200] {
        let invitation = manager
            .create_invitation(
                "127.0.0.1:41000".parse().expect("endpoint"),
                "vault-1".to_string(),
                0,
                crate::protocol::test_compatibility(),
                now_ms,
            )
            .expect("invitation");
        manager
            .enroll_peer(enrollment(&invitation, &phone), now_ms + 1)
            .expect("enroll same phone");
    }

    let peers = manager.linked_peers().expect("linked peers");
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].device_id, "device-phone");
}

#[test]
fn revoked_peer_is_remembered_across_restart_and_removed_by_reenrollment() {
    let temporary = TestDirectory::new("revoked-peer");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let phone = create_identity("device-phone".to_string()).expect("phone identity");
    let invitation = manager
        .create_invitation(
            "127.0.0.1:41000".parse().expect("endpoint"),
            "vault-1".to_string(),
            0,
            crate::protocol::test_compatibility(),
            100,
        )
        .expect("invitation");
    manager
        .enroll_peer(enrollment(&invitation, &phone), 101)
        .expect("enroll phone");
    let phone_certificate = decode_certificate(&phone.certificate).expect("phone certificate");

    manager.unlink_device("device-phone").expect("unlink phone");
    assert!(manager.linked_peers().expect("linked peers").is_empty());
    assert!(
        manager
            .is_revoked_certificate(Some(&phone_certificate))
            .expect("revoked certificate")
    );

    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("restart");
    assert!(
        restarted
            .is_revoked_certificate(Some(&phone_certificate))
            .expect("persisted revoked certificate")
    );

    let invitation = restarted
        .create_invitation(
            "127.0.0.1:41000".parse().expect("endpoint"),
            "vault-1".to_string(),
            0,
            crate::protocol::test_compatibility(),
            200,
        )
        .expect("new invitation");
    restarted
        .enroll_peer(enrollment(&invitation, &phone), 201)
        .expect("reenroll phone");
    assert!(
        !restarted
            .is_revoked_certificate(Some(&phone_certificate))
            .expect("active certificate")
    );
    assert_eq!(restarted.linked_peers().expect("linked peers").len(), 1);
}

#[test]
fn linked_client_accepts_only_its_existing_coordinator() {
    let phone_root = TestDirectory::new("coordinator-target-phone");
    let phone = PairingManager::default();
    phone
        .initialize(phone_root.path().to_path_buf(), "device-phone".to_string())
        .expect("initialize phone");

    let first_root = TestDirectory::new("coordinator-target-first");
    let first = PairingManager::default();
    first
        .initialize(first_root.path().to_path_buf(), "device-first".to_string())
        .expect("initialize first coordinator");
    let first_invitation = first
        .create_invitation(
            "127.0.0.1:41000".parse().expect("endpoint"),
            "vault-1".to_string(),
            3,
            crate::protocol::test_compatibility(),
            100,
        )
        .expect("first invitation");
    let (_, first_identity) = first.identity().expect("first identity");
    phone
        .record_coordinator(
            &first_invitation,
            first_identity.certificate.as_ref(),
            Some("Desktop".to_string()),
        )
        .expect("record first coordinator");

    let mut refreshed = first_invitation.clone();
    refreshed.endpoint = "127.0.0.1:42000".to_string();
    refreshed.generation = 4;
    phone
        .ensure_enrollment_target(&refreshed)
        .expect("same coordinator remains valid");
    phone
        .record_coordinator(
            &refreshed,
            first_identity.certificate.as_ref(),
            Some("Desktop".to_string()),
        )
        .expect("refresh coordinator endpoint");
    assert_eq!(
        phone
            .coordinator_pin()
            .expect("coordinator pin")
            .expect("coordinator")
            .endpoint,
        "127.0.0.1:42000"
    );

    let second_root = TestDirectory::new("coordinator-target-second");
    let second = PairingManager::default();
    second
        .initialize(
            second_root.path().to_path_buf(),
            "device-second".to_string(),
        )
        .expect("initialize second coordinator");
    let second_invitation = second
        .create_invitation(
            "127.0.0.1:43000".parse().expect("endpoint"),
            "vault-1".to_string(),
            4,
            crate::protocol::test_compatibility(),
            200,
        )
        .expect("second invitation");
    let (_, second_identity) = second.identity().expect("second identity");

    assert!(
        phone
            .ensure_enrollment_target(&second_invitation)
            .unwrap_err()
            .contains("already linked to another coordinator")
    );
    assert!(
        phone
            .record_coordinator(
                &second_invitation,
                second_identity.certificate.as_ref(),
                Some("Other desktop".to_string()),
            )
            .unwrap_err()
            .contains("already linked to another coordinator")
    );
    let mut different_vault = refreshed.clone();
    different_vault.vault_id = "vault-2".to_string();
    assert!(
        phone
            .ensure_enrollment_target(&different_vault)
            .unwrap_err()
            .contains("already linked to another coordinator")
    );
    let mut different_certificate = refreshed.clone();
    different_certificate.coordinator_fingerprint =
        second_invitation.coordinator_fingerprint.clone();
    assert!(
        phone
            .ensure_enrollment_target(&different_certificate)
            .unwrap_err()
            .contains("already linked to another coordinator")
    );
    assert_eq!(
        phone
            .coordinator_pin()
            .expect("coordinator pin")
            .expect("coordinator")
            .device_id,
        "device-first"
    );
}

#[test]
fn several_peers_survive_restart_and_can_be_unlinked_individually() {
    let temporary = TestDirectory::new("several-peers");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    enroll_device(&manager, "device-phone", "Phone", 100);
    enroll_device(&manager, "device-laptop", "Laptop", 200);

    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("restart");
    let peers = restarted.linked_peers().expect("linked peers");
    assert_eq!(peers.len(), 2);
    assert_eq!(peers[0].device_id, "device-laptop");
    assert_eq!(peers[1].device_id, "device-phone");

    restarted
        .unlink_device("device-phone")
        .expect("unlink one peer");
    assert!(
        restarted
            .linked_peer("device-phone")
            .expect("removed peer")
            .is_none()
    );
    assert!(
        restarted
            .linked_peer("device-laptop")
            .expect("remaining peer")
            .is_some()
    );
}

#[test]
fn memberships_belong_to_their_vault_and_a_moved_device_replaces_its_old_one() {
    let temporary = TestDirectory::new("vault-scoped-peers");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let phone = create_identity("device-phone".to_string()).expect("phone identity");
    let laptop = create_identity("device-laptop".to_string()).expect("laptop identity");
    enroll_identity(&manager, "vault-old", &phone, "Phone", 100).expect("enroll phone");
    enroll_identity(&manager, "vault-old", &laptop, "Laptop", 200).expect("enroll laptop");

    assert!(
        manager
            .linked_peers_for("vault-new")
            .expect("new vault peers")
            .is_empty()
    );
    assert!(!manager.is_linked_for("vault-new").expect("new vault link"));
    assert_eq!(
        manager
            .linked_peers_for("vault-old")
            .expect("old vault peers")
            .len(),
        2
    );

    enroll_identity(&manager, "vault-new", &phone, "Phone", 300).expect("move phone");
    let new_vault_peers = manager
        .linked_peers_for("vault-new")
        .expect("new vault peers");
    assert_eq!(new_vault_peers.len(), 1);
    assert_eq!(new_vault_peers[0].device_id, "device-phone");
    let old_vault_peers = manager
        .linked_peers_for("vault-old")
        .expect("old vault peers");
    assert_eq!(old_vault_peers.len(), 1);
    assert_eq!(old_vault_peers[0].device_id, "device-laptop");
    let phone_certificate = decode_certificate(&phone.certificate).expect("phone certificate");
    assert!(
        !manager
            .is_revoked_certificate(Some(&phone_certificate))
            .expect("moved certificate")
    );
}

#[test]
fn stored_limit_revokes_the_oldest_membership_of_another_vault_first() {
    let temporary = TestDirectory::new("membership-limit");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let identity =
        |index: usize| create_identity(format!("device-{index}")).expect("peer identity");
    let old_vault = (0..MAX_LINKED_PEERS).map(identity).collect::<Vec<_>>();
    for (index, peer) in old_vault.iter().enumerate() {
        enroll_identity(
            &manager,
            "vault-old",
            peer,
            "Phone",
            100 + index as i64 * 10,
        )
        .expect("fill old vault");
    }

    let first_new = identity(MAX_LINKED_PEERS);
    enroll_identity(&manager, "vault-new", &first_new, "Phone", 1_000).expect("enroll new");
    assert!(
        manager
            .linked_peer("device-0")
            .expect("evicted peer")
            .is_none()
    );
    assert!(
        manager
            .linked_peer("device-1")
            .expect("next oldest peer")
            .is_some()
    );
    let evicted_certificate =
        decode_certificate(&old_vault[0].certificate).expect("evicted certificate");
    assert!(
        manager
            .is_revoked_certificate(Some(&evicted_certificate))
            .expect("revoked certificate")
    );
    assert!(
        manager
            .revoked_device_ids("vault-old")
            .expect("revoked devices")
            .contains("device-0")
    );

    for index in MAX_LINKED_PEERS + 1..MAX_LINKED_PEERS * 2 {
        enroll_identity(
            &manager,
            "vault-new",
            &identity(index),
            "Phone",
            1_000 + index as i64,
        )
        .expect("replace old vault memberships");
    }
    assert!(
        manager
            .linked_peers_for("vault-old")
            .expect("old vault peers")
            .is_empty()
    );
    let error = enroll_identity(
        &manager,
        "vault-new",
        &identity(MAX_LINKED_PEERS * 2),
        "Phone",
        5_000,
    )
    .expect_err("full vault rejects another device");
    assert!(error.contains("one vault"), "{error}");
    assert_eq!(
        manager
            .linked_peers_for("vault-new")
            .expect("new vault peers")
            .len(),
        MAX_LINKED_PEERS
    );

    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("restart at the stored limit");
    assert_eq!(
        restarted.linked_peers().expect("linked peers").len(),
        MAX_LINKED_PEERS
    );
}

#[test]
fn obsolete_pairing_schema_is_rejected_without_rewriting_state() {
    let temporary = TestDirectory::new("schema-one-migration");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    enroll_device(&manager, "device-phone", "Phone", 100);

    let state_path = temporary.path().join(PAIRING_STATE_FILE);
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).expect("read current pairing state"))
            .expect("decode current pairing state");
    let object = value.as_object_mut().expect("pairing state object");
    object.insert("schemaVersion".to_string(), serde_json::json!(1));
    let peers = object
        .remove("linkedPeers")
        .expect("current linked peers")
        .as_object()
        .expect("linked peer map")
        .clone();
    object.insert(
        "linkedPeer".to_string(),
        peers.get("device-phone").expect("phone peer").clone(),
    );
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&value).expect("encode legacy state"),
    )
    .expect("write legacy state");

    let original = fs::read(&state_path).expect("read unsupported state");
    let manager = PairingManager::default();
    let error = manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect_err("reject obsolete pairing schema");
    assert_eq!(error, "pairing state schema version is unsupported");
    assert_eq!(
        fs::read(state_path).expect("read unchanged state"),
        original
    );
}

#[test]
fn ownership_upload_request_cannot_be_downgraded_to_refresh() {
    let temporary = TestDirectory::new("upload-priority");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");

    manager
        .request_upload(BundlePurpose::Ownership)
        .expect("request ownership upload");
    manager
        .request_upload(BundlePurpose::Refresh)
        .expect("request refresh upload");

    assert_eq!(
        manager.requested_upload().expect("requested upload"),
        Some(BundlePurpose::Ownership)
    );
    manager
        .cancel_requested_upload(BundlePurpose::Refresh)
        .expect("ignore lower-priority cancellation");
    assert_eq!(
        manager.requested_upload().expect("requested upload"),
        Some(BundlePurpose::Ownership)
    );
    manager
        .cancel_requested_upload(BundlePurpose::Ownership)
        .expect("cancel ownership upload");
    assert_eq!(manager.requested_upload().expect("requested upload"), None);
}

#[test]
fn expired_invitation_is_rejected() {
    let temporary = TestDirectory::new("expired");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let invitation = manager
        .create_invitation(
            "127.0.0.1:41000".parse().expect("endpoint"),
            "vault-1".to_string(),
            0,
            crate::protocol::test_compatibility(),
            100,
        )
        .expect("invitation");
    let phone = create_identity("device-phone".to_string()).expect("phone identity");
    assert!(
        manager
            .enroll_peer(enrollment(&invitation, &phone), invitation.expires_at_ms,)
            .unwrap_err()
            .contains("expired")
    );
}

#[test]
fn unlink_clears_the_peer_but_refuses_active_transfer_state() {
    let temporary = TestDirectory::new("unlink");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let invitation = manager
        .create_invitation(
            "127.0.0.1:41000".parse().expect("endpoint"),
            "vault-1".to_string(),
            0,
            crate::protocol::test_compatibility(),
            100,
        )
        .expect("invitation");
    let phone = create_identity("device-phone".to_string()).expect("phone identity");
    manager
        .enroll_peer(enrollment(&invitation, &phone), 101)
        .expect("enroll");
    manager
        .request_upload(BundlePurpose::Refresh)
        .expect("request upload");

    assert!(manager.unlink().is_err());
    {
        let mut inner = manager.lock().expect("pairing lock");
        initialized_state_mut(&mut inner)
            .expect("pairing state")
            .requested_upload = None;
        persist_initialized_state(&inner).expect("clear request");
    }
    manager.unlink().expect("unlink stable pairing");
    assert!(manager.linked_peers().expect("peer status").is_empty());
}

#[test]
fn activation_acknowledgement_survives_restart() {
    let temporary = TestDirectory::new("pending-ack");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-phone".to_string())
        .expect("initialize");
    let pending = PendingAcknowledgement {
        vault_id: "vault-1".to_string(),
        device_id: "device-phone".to_string(),
        transfer_id: "transfer-1".to_string(),
        generation: 1,
        purpose: BundlePurpose::Ownership,
    };
    manager
        .record_activation(pending.clone())
        .expect("record activation");
    drop(manager);

    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-phone".to_string())
        .expect("restart");
    assert!(restarted.replica_ready().expect("replica state"));
    assert_eq!(
        restarted
            .pending_acknowledgement()
            .expect("pending acknowledgement"),
        Some(pending)
    );
    restarted
        .clear_pending_acknowledgement("transfer-1")
        .expect("clear acknowledgement");
    assert_eq!(
        restarted
            .pending_acknowledgement()
            .expect("cleared acknowledgement"),
        None
    );
}

#[test]
fn bidirectional_transfer_state_survives_restart() {
    let temporary = TestDirectory::new("bidirectional-restart");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-phone".to_string())
        .expect("initialize");
    let metadata = BundleMetadata {
        protocol_version: PROTOCOL_VERSION,
        compatibility: crate::protocol::test_compatibility(),
        vault_id: "vault-1".to_string(),
        device_id: "device-desktop".to_string(),
        transfer_id: "transfer-return".to_string(),
        generation: 2,
        archive_bytes: 42,
        archive_sha256: "a".repeat(64),
    };
    let outgoing = StoredOutgoingTransfer {
        metadata: metadata.clone(),
        purpose: BundlePurpose::Ownership,
        committed: false,
    };
    manager
        .store_outgoing_transfer(outgoing.clone())
        .expect("store outgoing transfer");
    drop(manager);

    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-phone".to_string())
        .expect("restart");
    assert_eq!(
        restarted.outgoing_transfer().expect("outgoing transfer"),
        Some(outgoing)
    );
    restarted
        .mark_outgoing_committed("transfer-return")
        .expect("commit outgoing transfer");
    drop(restarted);

    let committed = PairingManager::default();
    committed
        .initialize(temporary.path().to_path_buf(), "device-phone".to_string())
        .expect("restart committed state");
    assert!(
        committed
            .outgoing_transfer()
            .expect("committed outgoing transfer")
            .expect("stored transfer")
            .committed
    );
    let completed = PendingAcknowledgement {
        vault_id: metadata.vault_id,
        device_id: metadata.device_id,
        transfer_id: metadata.transfer_id,
        generation: metadata.generation,
        purpose: BundlePurpose::Ownership,
    };
    committed
        .complete_outgoing_activation(completed.clone())
        .expect("complete outgoing activation");
    drop(committed);

    let acknowledged = PairingManager::default();
    acknowledged
        .initialize(temporary.path().to_path_buf(), "device-phone".to_string())
        .expect("restart completed transfer");
    assert_eq!(
        acknowledged.outgoing_transfer().expect("cleared outgoing"),
        None
    );
    assert_eq!(
        acknowledged
            .completed_activation()
            .expect("completed outgoing activation"),
        Some(completed)
    );
}

#[test]
fn incoming_transfer_and_completion_survive_restart() {
    let temporary = TestDirectory::new("incoming-restart");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let metadata = BundleMetadata {
        protocol_version: PROTOCOL_VERSION,
        compatibility: crate::protocol::test_compatibility(),
        vault_id: "vault-1".to_string(),
        device_id: "device-desktop".to_string(),
        transfer_id: "incoming-return".to_string(),
        generation: 3,
        archive_bytes: 84,
        archive_sha256: "b".repeat(64),
    };
    let incoming = StoredIncomingTransfer {
        metadata: metadata.clone(),
        source_device_id: "device-phone".to_string(),
        purpose: BundlePurpose::Ownership,
    };
    manager
        .store_incoming_transfer(incoming.clone())
        .expect("store incoming transfer");
    manager
        .request_upload(BundlePurpose::Ownership)
        .expect("store upload request");
    drop(manager);

    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("restart incoming state");
    assert_eq!(
        restarted.incoming_transfer().expect("incoming transfer"),
        Some(incoming)
    );
    let completed = PendingAcknowledgement {
        vault_id: metadata.vault_id,
        device_id: metadata.device_id,
        transfer_id: metadata.transfer_id,
        generation: metadata.generation,
        purpose: BundlePurpose::Ownership,
    };
    restarted
        .complete_incoming_activation(completed.clone())
        .expect("complete incoming activation");
    drop(restarted);

    let acknowledged = PairingManager::default();
    acknowledged
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("restart completed state");
    assert_eq!(
        acknowledged
            .completed_activation()
            .expect("completed activation"),
        Some(completed)
    );
    assert_eq!(
        acknowledged.incoming_transfer().expect("cleared incoming"),
        None
    );
    assert_eq!(
        acknowledged.requested_upload().expect("cleared request"),
        None
    );
}
