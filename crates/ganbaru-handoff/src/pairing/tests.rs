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
fn stored_coordinator_pins_must_use_the_lowercase_fingerprint_format() {
    let phone_root = TestDirectory::new("coordinator-pin-format-phone");
    let phone = PairingManager::default();
    phone
        .initialize(phone_root.path().to_path_buf(), "device-phone".to_string())
        .expect("initialize phone");
    let desktop_root = TestDirectory::new("coordinator-pin-format-desktop");
    let desktop = PairingManager::default();
    desktop
        .initialize(
            desktop_root.path().to_path_buf(),
            "device-desktop".to_string(),
        )
        .expect("initialize desktop");
    let invitation = desktop
        .create_invitation(
            "127.0.0.1:41000".parse().expect("endpoint"),
            "vault-1".to_string(),
            1,
            crate::protocol::test_compatibility(),
            100,
        )
        .expect("invitation");
    let (_, desktop_identity) = desktop.identity().expect("desktop identity");
    phone
        .record_coordinator(&invitation, desktop_identity.certificate.as_ref(), None)
        .expect("record coordinator");
    drop(phone);

    let state_path = phone_root.path().join(PAIRING_STATE_FILE);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).expect("read state")).expect("decode state");
    stored["coordinator"]["certificateFingerprint"] =
        serde_json::Value::from(invitation.coordinator_fingerprint.to_ascii_uppercase());
    fs::write(
        &state_path,
        serde_json::to_vec(&stored).expect("encode state"),
    )
    .expect("write state");

    let error = PairingManager::default()
        .initialize(phone_root.path().to_path_buf(), "device-phone".to_string())
        .expect_err("reject an uppercase coordinator pin");
    assert!(
        error.contains("coordinator certificate fingerprint is invalid"),
        "{error}"
    );
}

#[test]
fn stored_peer_fingerprints_must_equal_the_recomputed_lowercase_digest() {
    fn uppercase_stored_fingerprint(root: &Path, table: &str) {
        let state_path = root.join(PAIRING_STATE_FILE);
        let mut stored: serde_json::Value =
            serde_json::from_slice(&fs::read(&state_path).expect("read state"))
                .expect("decode state");
        let peer = &mut stored[table]["device-phone"]["certificateFingerprint"];
        let uppercase = peer
            .as_str()
            .expect("stored fingerprint")
            .to_ascii_uppercase();
        *peer = serde_json::Value::from(uppercase);
        fs::write(
            &state_path,
            serde_json::to_vec(&stored).expect("encode state"),
        )
        .expect("write state");
    }
    fn restart_error(root: &Path) -> String {
        PairingManager::default()
            .initialize(root.to_path_buf(), "device-desktop".to_string())
            .expect_err("reject a fingerprint that differs from the recomputed digest")
    }

    for table in ["linkedPeers", "revokedPeers"] {
        let temporary = TestDirectory::new("peer-fingerprint-format");
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
        if table == "revokedPeers" {
            manager.unlink_device("device-phone").expect("unlink phone");
        }
        drop(manager);

        uppercase_stored_fingerprint(temporary.path(), table);
        let error = restart_error(temporary.path());
        let expected = if table == "linkedPeers" {
            "linked peer certificate fingerprint is inconsistent"
        } else {
            "revoked peer certificate fingerprint is inconsistent"
        };
        assert!(error.contains(expected), "{table}: {error}");
    }
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
    let mut uppercase_certificate = refreshed.clone();
    uppercase_certificate.coordinator_fingerprint =
        refreshed.coordinator_fingerprint.to_ascii_uppercase();
    assert!(
        phone
            .ensure_enrollment_target(&uppercase_certificate)
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

/// A phone linked to a fresh desktop coordinator, with the coordinator's invitation and
/// certificate.
fn linked_phone(label: &str) -> (TestDirectory, PairingManager, PairingInvitation, Vec<u8>) {
    let phone_root = TestDirectory::new(&format!("{label}-phone"));
    let phone = PairingManager::default();
    phone
        .initialize(phone_root.path().to_path_buf(), "device-phone".to_string())
        .expect("initialize phone");
    let desktop_root = TestDirectory::new(&format!("{label}-desktop"));
    let desktop = PairingManager::default();
    desktop
        .initialize(
            desktop_root.path().to_path_buf(),
            "device-desktop".to_string(),
        )
        .expect("initialize desktop");
    let invitation = desktop
        .create_invitation(
            "192.168.1.20:43821".parse().expect("endpoint"),
            "vault-1".to_string(),
            1,
            crate::protocol::test_compatibility(),
            100,
        )
        .expect("invitation");
    let (_, desktop_identity) = desktop.identity().expect("desktop identity");
    let certificate = desktop_identity.certificate.as_ref().to_vec();
    phone
        .record_coordinator(&invitation, &certificate, None)
        .expect("record coordinator");
    (phone_root, phone, invitation, certificate)
}

fn reported_contact(endpoints: &[&str], discovery_key: &str) -> CoordinatorContact {
    CoordinatorContact {
        last_endpoint: None,
        reported_endpoints: Some(endpoints.iter().map(ToString::to_string).collect()),
        discovery_key: Some(discovery_key.to_string()),
    }
}

fn pin_of(manager: &PairingManager) -> CoordinatorPin {
    manager
        .coordinator_pin()
        .expect("coordinator pin")
        .expect("coordinator")
}

#[test]
fn a_pin_without_contact_fields_loads_as_a_pin_before_its_first_exchange() {
    let (phone_root, phone, invitation, _) = linked_phone("pin-without-contact");
    drop(phone);
    let state_path = phone_root.path().join(PAIRING_STATE_FILE);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).expect("read state")).expect("decode state");
    let coordinator = stored["coordinator"]
        .as_object_mut()
        .expect("stored coordinator");
    for field in ["lastEndpoint", "reportedEndpoints", "discoveryKey"] {
        assert!(coordinator.remove(field).is_some(), "{field}");
    }
    stored
        .as_object_mut()
        .expect("stored state")
        .remove("discoveryKeys");
    fs::write(
        &state_path,
        serde_json::to_vec(&stored).expect("encode state"),
    )
    .expect("write state");

    let restarted = PairingManager::default();
    restarted
        .initialize(phone_root.path().to_path_buf(), "device-phone".to_string())
        .expect("load a pin without contact fields");
    let pin = pin_of(&restarted);
    assert_eq!(pin.last_endpoint, None);
    assert!(pin.reported_endpoints.is_empty());
    assert_eq!(pin.discovery_key, None);
    assert_eq!(pin.current_endpoint(), invitation.endpoint);
}

#[test]
fn coordinator_contact_writes_only_on_change_and_only_for_the_current_pin() {
    let (phone_root, phone, invitation, _) = linked_phone("coordinator-contact");
    let state_path = phone_root.path().join(PAIRING_STATE_FILE);
    let fingerprint = invitation.coordinator_fingerprint.clone();
    assert_eq!(
        pin_of(&phone).last_endpoint.as_deref(),
        Some(invitation.endpoint.as_str()),
        "enrollment reached the invitation endpoint"
    );
    let key = random_discovery_key().expect("discovery key");
    let reported = ["192.168.1.30:43821", "[fd00::30]:43821"];

    assert_eq!(
        phone.record_coordinator_contact(
            "vault-1",
            &fingerprint,
            reported_contact(&reported, &key)
        ),
        Ok(true)
    );
    fs::remove_file(&state_path).expect("remove state to detect writes");
    assert_eq!(
        phone.record_coordinator_contact(
            "vault-1",
            &fingerprint,
            reported_contact(&reported, &key)
        ),
        Ok(false)
    );
    assert_eq!(
        phone.record_coordinator_contact(
            "vault-1",
            &fingerprint,
            CoordinatorContact {
                last_endpoint: Some(invitation.endpoint.clone()),
                ..CoordinatorContact::default()
            }
        ),
        Ok(false)
    );
    for (vault_id, other_fingerprint) in [
        ("vault-2", fingerprint.clone()),
        ("vault-1", "c".repeat(64)),
    ] {
        assert_eq!(
            phone.record_coordinator_contact(
                vault_id,
                &other_fingerprint,
                reported_contact(&["10.0.0.9:43821"], &key)
            ),
            Ok(false)
        );
    }
    assert!(!state_path.exists(), "an unchanged contact must not write");

    assert!(
        phone
            .record_coordinator_contact(
                "vault-1",
                &fingerprint,
                reported_contact(&["203.0.113.5:43821"], &key)
            )
            .is_err(),
        "a public reported endpoint is refused"
    );
    assert!(
        phone
            .record_coordinator_contact(
                "vault-1",
                &fingerprint,
                CoordinatorContact {
                    last_endpoint: Some("8.8.8.8:43821".to_string()),
                    ..CoordinatorContact::default()
                }
            )
            .is_err(),
        "a public last endpoint is refused"
    );
    assert_eq!(
        phone.record_coordinator_contact(
            "vault-1",
            &fingerprint,
            CoordinatorContact {
                last_endpoint: Some("192.168.1.30:43821".to_string()),
                ..CoordinatorContact::default()
            }
        ),
        Ok(true)
    );

    let restarted = PairingManager::default();
    restarted
        .initialize(phone_root.path().to_path_buf(), "device-phone".to_string())
        .expect("restart");
    let pin = pin_of(&restarted);
    assert_eq!(pin.current_endpoint(), "192.168.1.30:43821");
    assert_eq!(pin.reported_endpoints, reported);
    assert_eq!(pin.discovery_key.as_deref(), Some(key.as_str()));
    assert_eq!(pin.endpoint, invitation.endpoint);

    restarted.unlink().expect("unlink");
    assert_eq!(
        restarted.record_coordinator_contact(
            "vault-1",
            &fingerprint,
            reported_contact(&reported, &key)
        ),
        Ok(false),
        "an exchange finishing after unlink never resurrects the pin"
    );
    assert!(restarted.coordinator_pin().expect("pin").is_none());
}

#[test]
fn relinking_to_the_same_coordinator_keeps_its_reported_contact() {
    let (_phone_root, phone, invitation, certificate) = linked_phone("relink-contact");
    let key = random_discovery_key().expect("discovery key");
    phone
        .record_coordinator_contact(
            "vault-1",
            &invitation.coordinator_fingerprint,
            reported_contact(&["192.168.1.30:43821"], &key),
        )
        .expect("record contact");
    let mut refreshed = invitation.clone();
    refreshed.endpoint = "192.168.1.40:43821".to_string();
    refreshed.generation += 1;
    phone
        .record_coordinator(&refreshed, &certificate, None)
        .expect("relink");
    let pin = pin_of(&phone);
    assert_eq!(pin.endpoint, refreshed.endpoint);
    assert_eq!(pin.current_endpoint(), refreshed.endpoint);
    assert_eq!(pin.generation, refreshed.generation);
    assert_eq!(pin.reported_endpoints, ["192.168.1.30:43821"]);
    assert_eq!(pin.discovery_key.as_deref(), Some(key.as_str()));
}

#[test]
fn coordinator_discovery_keys_are_created_once_per_vault_and_dropped_with_the_last_peer() {
    let temporary = TestDirectory::new("discovery-keys");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    assert!(
        manager.discovery_key("vault-1").is_err(),
        "a vault without linked devices has no key"
    );
    let phone = create_identity("device-phone".to_string()).expect("phone identity");
    let laptop = create_identity("device-laptop".to_string()).expect("laptop identity");
    let tablet = create_identity("device-tablet".to_string()).expect("tablet identity");
    enroll_identity(&manager, "vault-1", &phone, "Phone", 100).expect("enroll phone");
    enroll_identity(&manager, "vault-1", &laptop, "Laptop", 200).expect("enroll laptop");
    enroll_identity(&manager, "vault-2", &tablet, "Tablet", 300).expect("enroll tablet");

    let first = manager.discovery_key("vault-1").expect("first vault key");
    crate::protocol::validate_discovery_key(&first).expect("key format");
    assert_eq!(manager.discovery_key("vault-1"), Ok(first.clone()));
    let second = manager.discovery_key("vault-2").expect("second vault key");
    assert_ne!(first, second);

    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("restart");
    assert_eq!(restarted.discovery_key("vault-1"), Ok(first.clone()));

    restarted
        .unlink_device("device-phone")
        .expect("unlink phone");
    assert_eq!(restarted.discovery_key("vault-1"), Ok(first.clone()));
    enroll_identity(&restarted, "vault-2", &laptop, "Laptop", 400).expect("move laptop");
    assert!(
        !state_text(temporary.path()).contains(&first),
        "moving the last device away drops the vault key"
    );
    assert!(restarted.discovery_key("vault-1").is_err());
    assert_eq!(restarted.discovery_key("vault-2"), Ok(second.clone()));

    restarted
        .unlink_device("device-tablet")
        .expect("unlink tablet");
    assert_eq!(restarted.discovery_key("vault-2"), Ok(second.clone()));
    restarted.unlink().expect("unlink all");
    assert!(!state_text(temporary.path()).contains(&second));
    assert!(restarted.discovery_key("vault-2").is_err());
}

#[test]
fn stored_contact_fields_are_validated_on_load() {
    let (phone_root, phone, invitation, _) = linked_phone("stored-contact");
    let key = random_discovery_key().expect("discovery key");
    phone
        .record_coordinator_contact(
            "vault-1",
            &invitation.coordinator_fingerprint,
            reported_contact(&["192.168.1.30:43821"], &key),
        )
        .expect("record contact");
    drop(phone);
    let state_path = phone_root.path().join(PAIRING_STATE_FILE);
    let original = fs::read(&state_path).expect("read state");
    for (field, value, expected) in [
        (
            "reportedEndpoints",
            serde_json::json!(["203.0.113.5:43821"]),
            "coordinator endpoint is refused",
        ),
        (
            "reportedEndpoints",
            serde_json::json!(vec![
                "192.168.1.30:43821";
                crate::protocol::MAX_COORDINATOR_ENDPOINTS + 1
            ]),
            "coordinator endpoint list is too large",
        ),
        (
            "lastEndpoint",
            serde_json::json!("8.8.8.8:43821"),
            "last coordinator endpoint is refused",
        ),
        (
            "discoveryKey",
            serde_json::json!(&key[1..]),
            "discovery key is invalid",
        ),
    ] {
        let mut stored: serde_json::Value =
            serde_json::from_slice(&original).expect("decode state");
        stored["coordinator"][field] = value;
        fs::write(
            &state_path,
            serde_json::to_vec(&stored).expect("encode state"),
        )
        .expect("write state");
        let error = PairingManager::default()
            .initialize(phone_root.path().to_path_buf(), "device-phone".to_string())
            .expect_err("refuse invalid stored contact");
        assert!(error.contains(expected), "{field}: {error}");
    }
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
fn memberships_in_two_vaults_survive_restart() {
    let temporary = TestDirectory::new("two-vault-peers");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let phone = create_identity("device-phone".to_string()).expect("phone identity");
    let laptop = create_identity("device-laptop".to_string()).expect("laptop identity");
    enroll_identity(&manager, "vault-old", &phone, "Phone", 100).expect("enroll phone");
    enroll_identity(&manager, "vault-new", &laptop, "Laptop", 200).expect("enroll laptop");

    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("restart with memberships in two vaults");
    for (vault_id, device_id) in [
        ("vault-old", "device-phone"),
        ("vault-new", "device-laptop"),
    ] {
        let peers = restarted.linked_peers_for(vault_id).expect("vault peers");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].device_id, device_id);
    }
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

#[test]
fn enrollment_rejects_control_and_bidi_labels_without_spending_the_invitation() {
    let temporary = TestDirectory::new("label-rejection");
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
    for label in [
        "phone\u{0007}",
        "a\u{202E}b",
        "a\u{2066}b\u{2069}",
        "\u{200B}",
    ] {
        let error = manager
            .enroll_peer(
                Enrollment {
                    device_label: label,
                    ..enrollment(&invitation, &phone)
                },
                101,
            )
            .expect_err("label must be rejected");
        assert!(error.contains("device label"), "{label:?}: {error}");
    }
    let peer = manager
        .enroll_peer(
            Enrollment {
                device_label: "\u{1F469}\u{200D}\u{1F4BB} Phone",
                ..enrollment(&invitation, &phone)
            },
            102,
        )
        .expect("joiners stay valid and the invitation is still usable");
    assert_eq!(peer.device_label, "\u{1F469}\u{200D}\u{1F4BB} Phone");
}

#[test]
fn stored_invalid_labels_are_quarantined_on_load_and_keep_the_link() {
    let temporary = TestDirectory::new("label-quarantine");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    enroll_device(&manager, "device-phone", "Phone", 100);
    enroll_device(&manager, "device-laptop", "Laptop", 200);
    drop(manager);

    let state_path = temporary.path().join(PAIRING_STATE_FILE);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).expect("read state")).expect("decode state");
    stored["linkedPeers"]["device-phone"]["deviceLabel"] =
        serde_json::Value::from("Ph\u{202E}one\u{0007}");
    stored["linkedPeers"]["device-laptop"]["deviceLabel"] =
        serde_json::Value::from("\u{202E}\u{0007}");
    fs::write(
        &state_path,
        serde_json::to_vec(&stored).expect("encode state"),
    )
    .expect("write state");

    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("an invalid stored label must not block startup");
    let labels: BTreeMap<String, String> = restarted
        .linked_peers_for("vault-1")
        .expect("linked peers")
        .into_iter()
        .map(|peer| (peer.device_id, peer.device_label))
        .collect();
    assert_eq!(
        labels.get("device-phone").map(String::as_str),
        Some("Phone")
    );
    assert_eq!(
        labels.get("device-laptop").map(String::as_str),
        Some("device-laptop")
    );
    let persisted = read_state(&state_path).expect("quarantined state is persisted");
    assert_eq!(persisted.linked_peers["device-phone"].device_label, "Phone");
}

#[test]
fn disabled_linking_reports_why_instead_of_running() {
    let temporary = TestDirectory::new("linking-disabled");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    manager
        .disable("decode pairing state: broken".to_string())
        .expect("disable");
    for error in [
        manager.identity().map(|_| ()).expect_err("identity"),
        manager
            .coordinator_pin()
            .map(|_| ())
            .expect_err("coordinator pin"),
        manager
            .linked_peers_for("vault-1")
            .map(|_| ())
            .expect_err("linked peers"),
    ] {
        assert_eq!(
            error,
            format!("{LINKING_DISABLED_ERROR}: decode pairing state: broken")
        );
    }
    assert!(!manager.is_linked_for("vault-1").unwrap_or(false));
}

#[test]
fn enrollment_verification_persists_nothing_and_returns_the_code_the_device_shows() {
    let temporary = TestDirectory::new("verify-enrollment");
    let manager = PairingManager::default();
    manager
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let create = |now_ms| {
        manager
            .create_invitation(
                "127.0.0.1:41000".parse().expect("endpoint"),
                "vault-1".to_string(),
                0,
                crate::protocol::test_compatibility(),
                now_ms,
            )
            .expect("invitation")
    };
    let invitation = create(100);
    let phone = create_identity("device-phone".to_string()).expect("phone identity");
    let intruder = create_identity("device-intruder".to_string()).expect("intruder identity");
    let phone_certificate = decode_certificate(&phone.certificate).expect("phone certificate");
    let intruder_certificate =
        decode_certificate(&intruder.certificate).expect("intruder certificate");

    let code = manager
        .verify_enrollment(&enrollment(&invitation, &phone), 101)
        .expect("verify phone");
    assert_eq!(
        code,
        enrollment_code(&invitation, phone_certificate.as_ref())
    );
    assert_ne!(
        code,
        enrollment_code(&invitation, intruder_certificate.as_ref())
    );
    assert!(manager.linked_peers().expect("peers").is_empty());
    let restarted = PairingManager::default();
    restarted
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("restart");
    assert!(
        restarted
            .linked_peers()
            .expect("restarted peers")
            .is_empty()
    );

    let mut wrong_secret = enrollment(&invitation, &phone);
    wrong_secret.secret = "wrong";
    assert!(
        manager
            .verify_enrollment(&wrong_secret, 101)
            .unwrap_err()
            .contains("credentials")
    );

    let replacement = create(200);
    assert_ne!(
        enrollment_code(&replacement, phone_certificate.as_ref()),
        code
    );
    assert!(
        manager
            .verify_enrollment(&enrollment(&invitation, &phone), 201)
            .unwrap_err()
            .contains("unknown")
    );
    manager
        .enroll_peer(enrollment(&replacement, &phone), 202)
        .expect("approved enrollment links");
    assert_eq!(manager.linked_peers().expect("peers").len(), 1);
}

#[test]
fn a_discarded_invitation_cannot_be_verified_or_enrolled() {
    let temporary = TestDirectory::new("discard-invitation");
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
        .discard_invitation(&invitation.invitation_id)
        .expect("discard");
    assert!(
        manager
            .verify_enrollment(&enrollment(&invitation, &phone), 101)
            .is_err()
    );
    assert!(
        manager
            .enroll_peer(enrollment(&invitation, &phone), 101)
            .is_err()
    );
    assert!(manager.linked_peers().expect("peers").is_empty());
}

/// In-memory stand-in for the operating-system keyring.
#[derive(Default)]
struct MemoryKeyStore {
    keys: Mutex<BTreeMap<String, Vec<u8>>>,
    refuse_writes: bool,
    fail_reads: bool,
    reads: std::sync::atomic::AtomicUsize,
}

impl MemoryKeyStore {
    fn key(&self, device_id: &str) -> Option<Vec<u8>> {
        self.keys.lock().expect("keys").get(device_id).cloned()
    }

    fn forget(&self, device_id: &str) {
        self.keys.lock().expect("keys").remove(device_id);
    }
}

impl IdentityKeyStore for MemoryKeyStore {
    fn read(&self, device_id: &str) -> Result<Option<Vec<u8>>, String> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail_reads {
            return Err("keyring is locked".to_string());
        }
        Ok(self.key(device_id))
    }

    fn write(&self, device_id: &str, pkcs8: &[u8]) -> Result<(), String> {
        if self.refuse_writes {
            return Err("no keyring service".to_string());
        }
        self.keys
            .lock()
            .expect("keys")
            .insert(device_id.to_string(), pkcs8.to_vec());
        Ok(())
    }

    fn remove(&self, device_id: &str) -> Result<(), String> {
        self.forget(device_id);
        Ok(())
    }
}

fn manager_with_store(root: &Path, store: &Arc<MemoryKeyStore>) -> PairingManager {
    let manager = PairingManager::default();
    manager
        .set_identity_key_store(store.clone())
        .expect("key store");
    manager
        .initialize(root.to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    manager
}

fn state_text(root: &Path) -> String {
    fs::read_to_string(root.join(PAIRING_STATE_FILE)).expect("pairing state")
}

fn private_key_bytes(identity: &TlsIdentity) -> Vec<u8> {
    identity.private_key.secret_der().to_vec()
}

#[test]
fn tls_key_moves_into_the_key_store_and_leaves_the_state_file() {
    let temporary = TestDirectory::new("key-store-move");
    let store = Arc::new(MemoryKeyStore::default());
    let manager = manager_with_store(temporary.path(), &store);

    assert_eq!(manager.device_id().expect("device id"), "device-desktop");
    assert_eq!(
        store.reads.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "reading the device ID never reaches the key store"
    );

    let (_, identity) = manager.identity().expect("identity");
    let key = private_key_bytes(&identity);
    KeyPair::try_from(key.as_slice()).expect("stored key is a usable PKCS#8 key");
    assert_eq!(store.key("device-desktop").as_deref(), Some(key.as_slice()));
    assert!(!state_text(temporary.path()).contains("privateKey"));

    let restarted = manager_with_store(temporary.path(), &store);
    let (_, reloaded) = restarted.identity().expect("reloaded identity");
    assert_eq!(private_key_bytes(&reloaded), key);
    assert_eq!(reloaded.certificate, identity.certificate);

    let reads = store.reads.load(std::sync::atomic::Ordering::SeqCst);
    restarted.identity().expect("cached identity");
    assert_eq!(
        store.reads.load(std::sync::atomic::Ordering::SeqCst),
        reads,
        "a loaded key is reused without another key store read"
    );
}

#[test]
fn tls_key_stays_in_the_state_file_when_the_key_store_refuses_it() {
    let temporary = TestDirectory::new("key-store-refused");
    let store = Arc::new(MemoryKeyStore {
        refuse_writes: true,
        ..MemoryKeyStore::default()
    });
    let manager = manager_with_store(temporary.path(), &store);

    let (_, identity) = manager.identity().expect("identity from the state file");
    assert!(store.key("device-desktop").is_none());
    assert!(state_text(temporary.path()).contains("privateKey"));

    let without_store = PairingManager::default();
    without_store
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize without a key store");
    let (_, reloaded) = without_store.identity().expect("identity");
    assert_eq!(private_key_bytes(&reloaded), private_key_bytes(&identity));
}

#[test]
fn a_lost_tls_key_blocks_a_linked_device_until_it_is_unlinked() {
    let temporary = TestDirectory::new("key-store-lost");
    let store = Arc::new(MemoryKeyStore::default());
    let manager = manager_with_store(temporary.path(), &store);
    enroll_device(&manager, "device-phone", "Phone", 100);
    let (_, original) = manager.identity().expect("identity");
    store.forget("device-desktop");

    let restarted = manager_with_store(temporary.path(), &store);
    assert_eq!(
        restarted.identity().map(|_| ()).unwrap_err(),
        DEVICE_KEY_MISSING_ERROR
    );
    assert_eq!(restarted.linked_peers().expect("linked peers").len(), 1);
    assert!(!state_text(temporary.path()).contains("privateKey"));

    restarted.unlink().expect("unlink");
    let (_, replaced) = restarted.identity().expect("fresh identity");
    assert_ne!(replaced.certificate, original.certificate);
    assert_eq!(
        store.key("device-desktop").as_deref(),
        Some(private_key_bytes(&replaced).as_slice())
    );
}

#[test]
fn an_unreadable_key_store_reports_the_failure_without_replacing_the_identity() {
    let temporary = TestDirectory::new("key-store-unreadable");
    let store = Arc::new(MemoryKeyStore::default());
    let manager = manager_with_store(temporary.path(), &store);
    let (_, original) = manager.identity().expect("identity");
    let before = state_text(temporary.path());

    let locked = Arc::new(MemoryKeyStore {
        fail_reads: true,
        ..MemoryKeyStore::default()
    });
    let restarted = manager_with_store(temporary.path(), &locked);
    let error = restarted.identity().map(|_| ()).unwrap_err();
    assert!(error.starts_with(DEVICE_KEY_UNAVAILABLE_ERROR), "{error}");
    assert_eq!(state_text(temporary.path()), before);

    let unlocked = manager_with_store(temporary.path(), &store);
    let (_, reloaded) = unlocked.identity().expect("identity after unlock");
    assert_eq!(reloaded.certificate, original.certificate);
}

#[test]
fn stored_identity_keeps_the_private_key_out_of_debug_output() {
    let identity = StoredIdentity {
        device_id: "device-desktop".to_string(),
        certificate: "certificate-b64".to_string(),
        private_key: Some("pkcs8-private-key-b64".to_string()),
    };
    let output = format!("{identity:?}");
    assert!(!output.contains("pkcs8-private-key-b64"), "{output}");
    assert!(
        output.contains("certificate-b64") && output.contains("[REDACTED]"),
        "{output}"
    );
    let serialized = serde_json::to_value(&identity).expect("serialize identity");
    assert_eq!(serialized["privateKey"], "pkcs8-private-key-b64");
}

#[cfg(unix)]
#[test]
fn handoff_staging_is_private_even_when_it_already_exists() {
    use std::os::unix::fs::PermissionsExt;
    let temporary = TestDirectory::new("private-staging");
    let staging = temporary.path().join(STAGING_DIRECTORY);
    std::fs::create_dir_all(&staging).expect("existing staging");
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))
        .expect("widen staging");
    PairingManager::default()
        .initialize(temporary.path().to_path_buf(), "device-desktop".to_string())
        .expect("initialize");
    let mode = std::fs::metadata(&staging)
        .expect("staging metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn device_certificate_subject_names_the_device() {
    let root = TestDirectory::new("certificate-subject");
    let manager = PairingManager::default();
    manager
        .initialize(
            root.path().to_path_buf(),
            "device-subject-check".to_string(),
        )
        .expect("initialize device");
    let (device_id, identity) = manager.identity().expect("device identity");
    let certificate = identity.certificate.as_ref();

    assert_eq!(device_id, "device-subject-check");
    assert!(
        certificate
            .windows(device_id.len())
            .any(|window| window == device_id.as_bytes()),
        "certificate subject should carry the device ID"
    );
    assert!(
        !certificate
            .windows(b"rcgen self signed cert".len())
            .any(|window| window == b"rcgen self signed cert"),
        "certificate should not keep the generator's default subject"
    );
}
