use super::*;

fn test_device_key() -> ganbaru_sync_contracts::DevicePublicKey {
    ganbaru_sync_contracts::DevicePublicKey::from_bytes([3; 32])
}

fn qr_invitation() -> PairingInvitation {
    PairingInvitation {
        protocol_version: PROTOCOL_VERSION,
        compatibility: test_compatibility(),
        invitation_id: "invite-abcdefghijklmnopqrstuv".to_string(),
        secret: "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG"
            .to_string()
            .into(),
        endpoint: "192.168.1.20:43821".to_string(),
        coordinator_fingerprint: "b".repeat(64),
        coordinator_device_id: "device-abcdefghijklmnopqrstuv".to_string(),
        vault_id: "vault-abcdefghijklmnopqrstuv".to_string(),
        expires_at_ms: i64::MAX,
    }
}

#[test]
fn invitation_secrets_serialize_plainly_and_stay_out_of_debug_output() {
    let invitation = qr_invitation();
    let secret = invitation.secret.to_string();
    let enroll = ControlMessage::Enroll {
        protocol_version: PROTOCOL_VERSION,
        invitation_id: invitation.invitation_id.clone(),
        secret: invitation.secret.clone(),
        vault_id: invitation.vault_id.clone(),
        device_id: "device-phone".to_string(),
        device_label: "Phone".to_string(),
        device_kind: DeviceKind::Phone,
        device_certificate: "certificate".to_string(),
        sync_device_key: encode_sync_device_key(&test_device_key()),
        sync_admin_key: None,
    };
    for output in [format!("{invitation:?}"), format!("{enroll:?}")] {
        assert!(!output.contains(&secret), "{output}");
        assert!(output.contains("[REDACTED]"), "{output}");
    }
    let serialized = serde_json::to_value(&enroll).expect("serialize enrollment");
    assert_eq!(serialized["secret"], secret.as_str());
    let decoded: ControlMessage = serde_json::from_value(serialized).expect("decode enrollment");
    assert_eq!(decoded, enroll);
}

#[test]
fn usage_exchange_messages_are_not_part_of_the_protocol() {
    // Usage reaches other devices as replicated daily totals, so the control channel
    // carries no usage samples in either direction.
    for kind in ["distractionsExchange", "distractionsAcknowledged"] {
        let message = serde_json::json!({
            "kind": kind,
            "protocolVersion": PROTOCOL_VERSION,
            "vaultId": "vault-abcdefghijklmnopqrstuv",
            "deviceId": "device-phone",
            "samples": [],
            "acknowledgedPeerSampleIds": [],
            "ownerSnapshot": [],
            "acknowledgedSampleIds": [],
            "peerSamples": [],
            "combinedSamples": [],
        });
        assert!(
            serde_json::from_value::<ControlMessage>(message).is_err(),
            "{kind} must not decode"
        );
    }
}

#[test]
fn enrollment_messages_reject_control_and_bidi_labels() {
    let enroll = |label: &str| ControlMessage::Enroll {
        protocol_version: PROTOCOL_VERSION,
        invitation_id: "invite-abcdefghijklmnopqrstuv".to_string(),
        secret: "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG"
            .to_string()
            .into(),
        vault_id: "vault-abcdefghijklmnopqrstuv".to_string(),
        device_id: "device-phone".to_string(),
        device_label: label.to_string(),
        device_kind: DeviceKind::Phone,
        device_certificate: "certificate".to_string(),
        sync_device_key: encode_sync_device_key(&test_device_key()),
        sync_admin_key: None,
    };
    let enrolled = |label: &str| ControlMessage::Enrolled {
        protocol_version: PROTOCOL_VERSION,
        coordinator_device_id: "device-desktop".to_string(),
        coordinator_device_label: Some(label.to_string()),
        charter: None,
        person_key: None,
    };
    for label in ["phone\u{0007}", "a\u{202E}b", "a\u{200F}b", ""] {
        assert!(enroll(label).validate().is_err(), "{label:?}");
        assert!(enrolled(label).validate().is_err(), "{label:?}");
    }
    for label in [
        "Phone",
        "\u{1F469}\u{200D}\u{1F4BB}",
        "\u{0645}\u{200C}\u{0627}",
    ] {
        assert_eq!(enroll(label).validate(), Ok(()), "{label:?}");
        assert_eq!(enrolled(label).validate(), Ok(()), "{label:?}");
    }
    let with_key = |key: String| match enroll("Phone") {
        ControlMessage::Enroll {
            protocol_version,
            invitation_id,
            secret,
            vault_id,
            device_id,
            device_label,
            device_kind,
            device_certificate,
            ..
        } => ControlMessage::Enroll {
            protocol_version,
            invitation_id,
            secret,
            vault_id,
            device_id,
            device_label,
            device_kind,
            device_certificate,
            sync_device_key: key,
            sync_admin_key: None,
        },
        _ => unreachable!(),
    };
    let full = encode_sync_device_key(&test_device_key());
    for invalid in [
        String::new(),
        full[1..].to_string(),
        format!("{full}AA"),
        "+".repeat(43),
    ] {
        assert!(with_key(invalid.clone()).validate().is_err(), "{invalid:?}");
    }
    assert_eq!(
        decode_sync_device_key(&full),
        Ok(test_device_key()),
        "the device key round-trips"
    );

    let admin = ganbaru_sync_contracts::AdminKeyPair::from_seed(&[6; 32])
        .expect("admin")
        .public_key();
    let with_admin = |key: Option<String>| match enroll("Desktop") {
        ControlMessage::Enroll {
            protocol_version,
            invitation_id,
            secret,
            vault_id,
            device_id,
            device_label,
            device_kind: _,
            device_certificate,
            sync_device_key,
            ..
        } => ControlMessage::Enroll {
            protocol_version,
            invitation_id,
            secret,
            vault_id,
            device_id,
            device_label,
            device_kind: DeviceKind::Computer,
            device_certificate,
            sync_device_key,
            sync_admin_key: key,
        },
        _ => unreachable!(),
    };
    let admin_text = encode_sync_admin_key(&admin);
    let desktop = with_admin(Some(admin_text.clone()));
    assert_eq!(desktop.validate(), Ok(()));
    assert_eq!(decode_sync_admin_key(&admin_text), Ok(admin));
    let serialized = serde_json::to_value(&desktop).expect("serialize enrollment");
    assert_eq!(serialized["sync_admin_key"], admin_text.as_str());
    let phone = serde_json::to_value(with_admin(None)).expect("serialize enrollment");
    assert!(
        phone.get("sync_admin_key").is_none(),
        "an enrollment without an admin key leaves the field out"
    );
    assert_eq!(
        serde_json::from_value::<ControlMessage>(phone).expect("parse enrollment"),
        with_admin(None)
    );
    for invalid in [String::new(), admin_text[1..].to_string(), "+".repeat(43)] {
        assert!(
            with_admin(Some(invalid.clone())).validate().is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn invitation_qr_round_trips_through_production_decoder() {
    let invitation = qr_invitation();
    let matrix = invitation_qr_matrix(&invitation).expect("QR should encode");
    let scale = 8;
    let quiet = 4;
    let image_width = (matrix.width + quiet * 2) * scale;
    let mut image = vec![255_u8; image_width * image_width];
    for y in 0..matrix.width {
        for x in 0..matrix.width {
            if matrix.modules[y * matrix.width + x] {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = (x + quiet) * scale + dx;
                        let py = (y + quiet) * scale + dy;
                        image[py * image_width + px] = 0;
                    }
                }
            }
        }
    }
    assert_eq!(
        decode_pairing_qr_luma(image_width, image_width, &image, 1).expect("QR should decode"),
        invitation
    );
}

#[test]
fn certificate_fingerprints_require_exactly_64_lowercase_hex_characters() {
    let lowercase = "0123456789abcdef".repeat(4);
    assert!(validate_certificate_fingerprint("fingerprint", &lowercase).is_ok());
    let digest = decode_certificate_fingerprint(&lowercase).expect("lowercase digest");
    assert_eq!(encode_qr_digest(digest), lowercase);
    for invalid in [
        lowercase.to_ascii_uppercase(),
        format!("+{}", &lowercase[1..]),
        format!("{} ", &lowercase[1..]),
        lowercase[1..].to_string(),
        format!("{lowercase}0"),
        "g".repeat(64),
    ] {
        assert!(
            validate_certificate_fingerprint("fingerprint", &invalid).is_err(),
            "{invalid}"
        );
        assert!(
            decode_certificate_fingerprint(&invalid).is_none(),
            "{invalid}"
        );
        let invitation = PairingInvitation {
            coordinator_fingerprint: invalid.clone(),
            ..qr_invitation()
        };
        assert!(invitation.validate(0).is_err(), "{invalid}");
    }
}
#[test]
fn invitations_with_non_lan_endpoints_fail_validation() {
    for endpoint in [
        "203.0.113.5:443",
        "8.8.8.8:43821",
        "224.0.0.251:43821",
        "192.168.1.20:0",
    ] {
        let invitation = PairingInvitation {
            endpoint: endpoint.to_string(),
            ..qr_invitation()
        };
        assert!(invitation.validate(0).is_err(), "{endpoint}");
        assert!(encode_invitation(&invitation).is_err(), "{endpoint}");
        let crafted = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            serde_json::to_vec(&invitation).expect("invitation json"),
        );
        assert!(decode_invitation(&crafted, 0).is_err(), "{endpoint}");
    }
    for endpoint in [
        "100.64.3.4:43821",
        "[2a01:4f8:10:20::5]:43821",
        "[fe80::1]:43821",
    ] {
        let invitation = PairingInvitation {
            endpoint: endpoint.to_string(),
            ..qr_invitation()
        };
        assert_eq!(invitation.validate(0), Ok(()), "{endpoint}");
    }
}

#[test]
fn compact_pairing_qr_is_materially_smaller_than_the_manual_code() {
    let invitation = qr_invitation();
    let payload = encode_pairing_qr_payload(&invitation).expect("QR payload");
    let manual = encode_invitation(&invitation).expect("manual invitation");
    let matrix = invitation_qr_matrix(&invitation).expect("QR should encode");

    assert!(payload.len() * 4 < manual.len() * 3);
    assert!(matrix.width <= 65, "QR width was {} modules", matrix.width);
}

#[test]
fn compact_pairing_qr_rejects_truncation_and_trailing_data() {
    let invitation = qr_invitation();
    let payload = encode_pairing_qr_payload(&invitation).expect("QR payload");

    assert!(
        decode_pairing_qr_payload(&payload[..payload.len() - 1], 1)
            .unwrap_err()
            .contains("truncated")
    );
    let mut trailing = payload;
    trailing.push(0);
    assert!(
        decode_pairing_qr_payload(&trailing, 1)
            .unwrap_err()
            .contains("unexpected data")
    );
}

#[test]
fn pairing_qr_decoder_rejects_text_invitations() {
    let invitation = qr_invitation();
    let encoded = encode_invitation(&invitation).expect("manual invitation");

    assert!(decode_pairing_qr_payload(encoded.as_bytes(), 1).is_err());
    assert_eq!(decode_invitation(&encoded, 1).unwrap(), invitation);
}

#[test]
fn malformed_and_oversized_metadata_is_rejected() {
    let mut metadata = BundleMetadata {
        protocol_version: PROTOCOL_VERSION,
        compatibility: test_compatibility(),
        vault_id: "vault-1".to_string(),
        device_id: "device-1".to_string(),
        transfer_id: "transfer-1".to_string(),
        archive_bytes: 10,
        archive_sha256: "a".repeat(64),
        derived_state_stale: false,
    };
    metadata.archive_sha256 = "not-a-digest".to_string();
    assert!(metadata.validate().unwrap_err().contains("SHA-256"));
    metadata.archive_sha256 = "a".repeat(64);
    metadata.archive_bytes = MAX_ARCHIVE_BYTES + 1;
    assert!(metadata.validate().unwrap_err().contains("bundle size"));
}

fn asset(byte_size: u32) -> SyncAsset {
    SyncAsset {
        class: "quick-notes/images".to_string(),
        sha256: "a".repeat(64),
        content_type: "image/png".to_string(),
        byte_size,
    }
}

fn put(asset: SyncAsset, offset: u32, data: String) -> ControlMessage {
    ControlMessage::SyncAssetPut {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-1".to_string(),
        asset,
        offset,
        data,
    }
}

#[test]
fn asset_references_must_be_well_formed() {
    let query = |assets: Vec<SyncAsset>| ControlMessage::SyncAssetQuery {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-1".to_string(),
        assets,
    };
    query(vec![asset(10)]).validate().unwrap();
    let mut upper = asset(10);
    upper.sha256 = "A".repeat(64);
    let mut short = asset(10);
    short.sha256 = "a".repeat(63);
    for content_type in [
        "image",
        "image/",
        "/png",
        "image/png/x",
        "Image/PNG",
        "image/p ng",
    ] {
        let mut typed = asset(10);
        typed.content_type = content_type.to_string();
        assert!(query(vec![typed]).validate().is_err(), "{content_type}");
    }
    for invalid in [upper, short, asset(0), asset(MAX_SYNC_ASSET_BYTES + 1)] {
        assert!(query(vec![invalid]).validate().is_err());
    }
    assert!(
        query(vec![asset(10); MAX_SYNC_ASSETS + 1])
            .validate()
            .unwrap_err()
            .contains("too large")
    );
    let presence = |held: Vec<SyncAssetKey>| ControlMessage::SyncAssetPresence { held };
    presence(vec![asset(10).key()]).validate().unwrap();
    let mut escaping = asset(10).key();
    escaping.sha256 = "../a".to_string();
    assert!(presence(vec![escaping]).validate().is_err());
    assert!(
        presence(vec![asset(10).key(); MAX_SYNC_ASSETS + 1])
            .validate()
            .unwrap_err()
            .contains("too large")
    );
}

#[test]
fn sync_asset_classes_must_be_relative_lowercase_paths() {
    let query = |class: &str| {
        let mut classed = asset(10);
        classed.class = class.to_string();
        ControlMessage::SyncAssetQuery {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-1".to_string(),
            device_id: "device-1".to_string(),
            assets: vec![classed],
        }
    };
    for valid in ["quick-notes/images", "notes", "a/b/c-2"] {
        query(valid).validate().unwrap();
    }
    for invalid in [
        "",
        "/quick-notes",
        "quick-notes/",
        "quick-notes//images",
        "../images",
        "quick-notes/..",
        ".",
        "Quick-notes/images",
        "quick notes",
        "quick-notes\\images",
        &"a".repeat(65),
    ] {
        assert_eq!(
            query(invalid).validate(),
            Err("sync asset class is invalid".to_string()),
            "{invalid}"
        );
        let key = SyncAssetKey {
            class: invalid.to_string(),
            sha256: "a".repeat(64),
        };
        assert!(
            ControlMessage::SyncAssetPresence { held: vec![key] }
                .validate()
                .is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn sync_assets_up_to_64_mib_stream_through_the_exchange() {
    assert_eq!(MAX_SYNC_ASSET_BYTES, 64 * 1024 * 1024);
    let last = MAX_SYNC_ASSET_BYTES - 3;
    put(asset(MAX_SYNC_ASSET_BYTES), last, "AAAA".to_string())
        .validate()
        .unwrap();
}

#[test]
fn asset_chunks_must_lie_inside_their_asset() {
    put(asset(6), 0, "AAAAAAAA".to_string()).validate().unwrap();
    put(asset(6), 3, "AAAA".to_string()).validate().unwrap();
    assert!(
        put(asset(6), 3, "AAAAAAAA".to_string())
            .validate()
            .unwrap_err()
            .contains("past the asset")
    );
    assert!(put(asset(6), 6, "AAAA".to_string()).validate().is_err());
    assert!(put(asset(6), 0, String::new()).validate().is_err());
    assert!(put(asset(6), 0, "AA+/".to_string()).validate().is_err());
    let oversized = "A".repeat((SYNC_ASSET_CHUNK_BYTES * 4).div_ceil(3) + 4);
    assert!(
        put(asset(MAX_SYNC_ASSET_BYTES), 0, oversized)
            .validate()
            .is_err()
    );

    let get = |offset| ControlMessage::SyncAssetGet {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-1".to_string(),
        asset: asset(6),
        offset,
    };
    get(5).validate().unwrap();
    assert!(get(6).validate().is_err());
    let received = |received| ControlMessage::SyncAssetReceived { received };
    received(MAX_SYNC_ASSET_BYTES).validate().unwrap();
    assert!(received(MAX_SYNC_ASSET_BYTES + 1).validate().is_err());
}

#[test]
fn compatibility_requires_the_same_database_migration_set() {
    let local = test_compatibility();
    let mut remote = local.clone();
    ensure_compatible(&local, &remote).expect("matching schemas should interoperate");

    remote.app_version = "newer".to_string();
    remote.database_schema_sha256 = "b".repeat(64);
    let error = ensure_compatible(&local, &remote).unwrap_err();
    assert!(error.contains("compatibility mismatch"));
    assert!(error.contains("test"));
    assert!(error.contains("newer"));
}

#[test]
fn contact_status_replay_window_holds_at_every_timestamp_extreme() {
    let status = |issued_at_ms: i64| ControlMessage::ContactRequestStatus {
        protocol_version: PROTOCOL_VERSION,
        request_id: "request-abcdefghijklmnopqrstuv".to_string(),
        requester_public_key: "a".repeat(ganbaru_contacts::PUBLIC_KEY_TEXT_LENGTH),
        issued_at_ms,
        signature: "s".repeat(SIGNATURE_TEXT_LENGTH),
    };
    let now = unix_time_ms();

    status(now).validate().expect("current status poll");
    status(now - STATUS_REPLAY_WINDOW_MS / 2)
        .validate()
        .expect("status poll inside the window");
    for issued_at_ms in [
        i64::MIN,
        i64::MIN + 1,
        i64::MAX,
        0,
        now - 2 * STATUS_REPLAY_WINDOW_MS,
        now + 2 * STATUS_REPLAY_WINDOW_MS,
    ] {
        let error = status(issued_at_ms).validate().unwrap_err();
        assert!(error.contains("replay window"), "{issued_at_ms}: {error}");
    }
}

/// Member endpoints of distinct devices, one per endpoint.
fn members(endpoints: &[&str]) -> Vec<MemberEndpoint> {
    endpoints
        .iter()
        .zip(0u64..)
        .map(|(endpoint, index)| MemberEndpoint {
            device_id: format!("device-{index}"),
            endpoint: (*endpoint).to_string(),
            observed_ms: 1_700_000_000_000 + index,
        })
        .collect()
}

fn sync_state(endpoints: &[&str]) -> ControlMessage {
    sync_state_with(members(endpoints))
}

fn sync_state_with(members: Vec<MemberEndpoint>) -> ControlMessage {
    ControlMessage::SyncState {
        manifest_version: 1,
        stored: vec![SyncSeq {
            writer: "a".repeat(32),
            seq: 3,
        }],
        probe_hash: Some("c".repeat(64)),
        members,
        stable: vec![SyncSeq {
            writer: "a".repeat(32),
            seq: 2,
        }],
        bootstrap_required: false,
        forked: vec![SyncSeq {
            writer: "b".repeat(32),
            seq: 3,
        }],
    }
}

fn frame_round_trip(message: &ControlMessage) -> Result<ControlMessage, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let mut frame = Vec::new();
        write_control(&mut frame, message).await?;
        read_control(&mut frame.as_slice()).await
    })
}

#[test]
fn sync_state_round_trips_reported_endpoints() {
    let state = sync_state(&[
        "192.168.1.20:43821",
        "[fd00::5]:43821",
        "[2a01:4f8:10:20::5]:43821",
        "100.64.3.4:43821",
    ]);
    assert_eq!(frame_round_trip(&state), Ok(state));
    let empty = sync_state(&[]);
    assert_eq!(frame_round_trip(&empty), Ok(empty));
    let full = vec!["192.168.1.20:43821"; MAX_MEMBER_ENDPOINTS];
    assert_eq!(sync_state(&full).validate(), Ok(()));
}

#[test]
fn member_endpoints_keep_their_wire_shape() {
    let member = MemberEndpoint {
        device_id: "device-laptop".to_string(),
        endpoint: "192.168.1.20:43821".to_string(),
        observed_ms: 1_700_000_000_123,
    };
    assert_eq!(
        serde_json::to_value(&member).expect("serialize member endpoint"),
        serde_json::json!({
            "deviceId": "device-laptop",
            "endpoint": "192.168.1.20:43821",
            "observedMs": 1_700_000_000_123u64,
        })
    );
    let hello = ControlMessage::SyncHello {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-laptop".to_string(),
        manifest_version: 1,
        stored: Vec::new(),
        probe: None,
        listening: vec!["192.168.1.20:43821".to_string()],
    };
    let wire = serde_json::to_value(&hello).expect("serialize sync hello");
    assert_eq!(wire["listening"], serde_json::json!(["192.168.1.20:43821"]));
    assert_eq!(wire["protocol_version"], serde_json::json!(20));
    assert_eq!(frame_round_trip(&hello), Ok(hello));
}

#[test]
fn sync_hello_refuses_too_many_or_unreportable_listening_endpoints() {
    let hello = |listening: Vec<&str>| ControlMessage::SyncHello {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-laptop".to_string(),
        manifest_version: 1,
        stored: Vec::new(),
        probe: None,
        listening: listening.into_iter().map(ToString::to_string).collect(),
    };
    assert_eq!(hello(Vec::new()).validate(), Ok(()));
    assert_eq!(
        hello(vec!["192.168.1.20:43821"; MAX_COORDINATOR_ENDPOINTS]).validate(),
        Ok(())
    );
    assert!(
        hello(vec!["192.168.1.20:43821"; MAX_COORDINATOR_ENDPOINTS + 1])
            .validate()
            .is_err()
    );
    assert!(hello(vec!["203.0.113.5:43821"]).validate().is_err());
}

#[test]
fn sync_state_refuses_unreportable_endpoints() {
    let over = vec!["192.168.1.20:43821"; MAX_MEMBER_ENDPOINTS + 1];
    assert!(sync_state(&over).validate().is_err());
    let mut repeated = members(&["192.168.1.20:43821", "192.168.1.21:43821"]);
    repeated[1].device_id = repeated[0].device_id.clone();
    repeated[1].endpoint = repeated[0].endpoint.clone();
    assert!(sync_state_with(repeated).validate().is_err());
    let mut unnamed = members(&["192.168.1.20:43821"]);
    unnamed[0].device_id = "device one".to_string();
    assert!(sync_state_with(unnamed).validate().is_err());
    for endpoint in [
        "203.0.113.5:43821",
        "127.0.0.1:43821",
        "[::1]:43821",
        "[fe80::1]:43821",
        "192.168.1.20:0",
        "192.168.1.20",
        "",
    ] {
        let state = sync_state(&["10.0.0.4:43821", endpoint]);
        assert!(state.validate().is_err(), "{endpoint:?}");
        assert!(frame_round_trip(&state).is_err(), "{endpoint:?}");
    }
}

#[test]
fn sync_state_round_trips_stability_and_refuses_malformed_stable_vectors() {
    let mut state = sync_state(&[]);
    if let ControlMessage::SyncState {
        bootstrap_required, ..
    } = &mut state
    {
        *bootstrap_required = true;
    }
    assert_eq!(frame_round_trip(&state), Ok(state));
    for stable in [
        vec![SyncSeq {
            writer: "a".repeat(31),
            seq: 1,
        }],
        vec![SyncSeq {
            writer: "a".repeat(32),
            seq: 0,
        }],
    ] {
        let mut state = sync_state(&[]);
        if let ControlMessage::SyncState { stable: field, .. } = &mut state {
            *field = stable.clone();
        }
        assert!(state.validate().is_err(), "{stable:?}");
    }
    let refusal = SyncRefusal {
        code: SyncRefusalCode::BootstrapRequired,
        writer: None,
        seq: None,
    };
    let encoded = serde_json::to_string(&refusal).expect("refusal encodes");
    assert!(encoded.contains("\"bootstrap_required\""), "{encoded}");
}

#[test]
fn sync_state_bounds_its_forked_writers() {
    let with_forked = |forked: Vec<SyncSeq>| {
        let mut state = sync_state(&[]);
        if let ControlMessage::SyncState { forked: field, .. } = &mut state {
            *field = forked;
        }
        state
    };
    let writers = ganbaru_sync_contracts::bounds::MAX_WRITERS_PER_SPACE;
    let entry = |index: usize| SyncSeq {
        writer: format!("{index:032x}"),
        seq: 2,
    };
    let full = with_forked((0..writers).map(entry).collect());
    assert_eq!(frame_round_trip(&full), Ok(full));
    assert!(
        with_forked((0..=writers).map(entry).collect())
            .validate()
            .is_err()
    );
    for malformed in [
        SyncSeq {
            writer: "B".repeat(32),
            seq: 2,
        },
        SyncSeq {
            writer: "b".repeat(32),
            seq: 0,
        },
    ] {
        assert!(with_forked(vec![malformed.clone()]).validate().is_err());
    }
}

#[test]
fn fork_evidence_messages_round_trip_and_bound_their_operations() {
    let op = |byte: u8| {
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            vec![byte; 90],
        )
    };
    let request = |evidence: Vec<String>| ControlMessage::SyncForkEvidence {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-phone".to_string(),
        writer: "a".repeat(32),
        evidence,
    };
    for evidence in [Vec::new(), vec![op(1)], vec![op(1), op(2)]] {
        let message = request(evidence);
        assert_eq!(frame_round_trip(&message), Ok(message.clone()));
        assert_eq!(message.sync_peer(), Some(("vault-1", "device-phone")));
    }
    assert!(request(vec![op(1), op(2), op(3)]).validate().is_err());
    assert!(request(vec!["not base64!".to_string()]).validate().is_err());
    let mut foreign = request(Vec::new());
    if let ControlMessage::SyncForkEvidence { writer, .. } = &mut foreign {
        *writer = "a".repeat(31);
    }
    assert!(foreign.validate().is_err());
    let pair = |evidence| ControlMessage::SyncForkPair { evidence };
    for evidence in [Vec::new(), vec![op(1), op(2)]] {
        let message = pair(evidence);
        assert_eq!(frame_round_trip(&message), Ok(message));
    }
    assert!(pair(vec![op(1)]).validate().is_err());
    assert!(pair(vec![op(1), op(2), op(3)]).validate().is_err());
}

#[test]
fn writer_head_messages_round_trip_and_validate_their_parts() {
    let request = |writer: String| ControlMessage::SyncWriterHead {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-phone".to_string(),
        writer,
    };
    let message = request("a".repeat(32));
    assert_eq!(frame_round_trip(&message), Ok(message.clone()));
    assert_eq!(message.sync_peer(), Some(("vault-1", "device-phone")));
    assert!(!message.is_record_exchange());
    assert!(request("a".repeat(31)).validate().is_err());
    let mut stale = request("a".repeat(32));
    if let ControlMessage::SyncWriterHead {
        protocol_version, ..
    } = &mut stale
    {
        *protocol_version = PROTOCOL_VERSION - 1;
    }
    assert!(stale.validate().is_err());

    let op = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        vec![7u8; 90],
    );
    for op in [None, Some(op)] {
        let message = ControlMessage::SyncWriterHeadOp { op };
        assert_eq!(frame_round_trip(&message), Ok(message));
    }
    let malformed = ControlMessage::SyncWriterHeadOp {
        op: Some("not base64!".to_string()),
    };
    assert!(malformed.validate().is_err());
}

fn signed_charter(
    person: &ganbaru_contacts::PersonKeyPair,
    anchor_device_id: &str,
) -> ganbaru_sync_contracts::SignedCharter {
    let admin = ganbaru_sync_contracts::AdminKeyPair::from_seed(&[5; 32]).expect("admin");
    ganbaru_sync_contracts::sign_charter(
        &ganbaru_sync_contracts::Charter {
            space: ganbaru_sync_contracts::SpaceId::personal("vault-1"),
            anchor_device_id: anchor_device_id.to_string(),
            anchor_device_key: test_device_key(),
            anchor_tls_fingerprint: [6; 32],
            anchor_admin_key: admin.public_key(),
            recovery_admin_key: admin.public_key(),
            discovery_key: [7; 32],
            created_at_ms: 1,
        },
        person,
    )
    .expect("sign charter")
}

#[test]
fn pairing_response_carries_a_charter_its_person_key_verifies() {
    let person = ganbaru_contacts::PersonKeyPair::from_seed(&[8; 32]).expect("person");
    let other = ganbaru_contacts::PersonKeyPair::from_seed(&[9; 32]).expect("other");
    let enrolled = |charter: Option<String>, person_key: Option<String>| ControlMessage::Enrolled {
        protocol_version: PROTOCOL_VERSION,
        coordinator_device_id: "device-desktop".to_string(),
        coordinator_device_label: None,
        charter,
        person_key,
    };
    let charter = encode_sync_charter(&signed_charter(&person, "device-desktop"));
    let person_key = person.public_key().to_text();
    let valid = enrolled(Some(charter.clone()), Some(person_key.clone()));
    assert_eq!(frame_round_trip(&valid), Ok(valid.clone()));
    assert_eq!(enrolled(None, None).validate(), Ok(()));
    // The charter names the founding anchor, which need not be the coordinator presenting it.
    let founded_elsewhere = encode_sync_charter(&signed_charter(&person, "device-other"));
    assert_eq!(
        enrolled(Some(founded_elsewhere), Some(person_key.clone())).validate(),
        Ok(())
    );

    let mut tampered =
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &charter)
            .expect("charter bytes");
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    let tampered =
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, tampered);
    for invalid in [
        enrolled(Some(charter.clone()), None),
        enrolled(None, Some(person_key.clone())),
        enrolled(Some(charter.clone()), Some(other.public_key().to_text())),
        enrolled(Some(tampered), Some(person_key.clone())),
        enrolled(Some("A".repeat(4096)), Some(person_key.clone())),
    ] {
        assert!(invalid.validate().is_err(), "{invalid:?}");
    }
}

#[test]
fn proposals_round_trip_verified_and_answers_name_one_outcome() {
    let admin = ganbaru_sync_contracts::AdminKeyPair::from_seed(&[5; 32]).expect("admin");
    let proposal = ganbaru_sync_contracts::sign_proposal(
        &ganbaru_sync_contracts::Proposal {
            space: ganbaru_sync_contracts::SpaceId::personal("vault-1"),
            charter: [7; 32],
            author: admin.public_key(),
            created_at_ms: 1,
            entry: ganbaru_sync_contracts::Entry::Remove(ganbaru_sync_contracts::Remove {
                device_id: "device-phone".to_string(),
            }),
        },
        &admin,
    )
    .expect("sign proposal");
    let propose = |proposal: String| ControlMessage::SyncPropose {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-desktop".to_string(),
        proposal,
    };
    let text = encode_sync_proposal(&proposal);
    let message = propose(text.clone());
    assert_eq!(frame_round_trip(&message), Ok(message.clone()));
    assert_eq!(message.sync_peer(), Some(("vault-1", "device-desktop")));
    assert_eq!(decode_sync_proposal(&text), Ok(proposal));
    let mut forged =
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &text)
            .expect("proposal bytes");
    let last = forged.len() - 1;
    forged[last] ^= 1;
    let forged = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, forged);
    for invalid in [String::new(), forged, "not base64!".to_string()] {
        assert!(propose(invalid.clone()).validate().is_err(), "{invalid:?}");
    }

    let answer = |revision, rejection| ControlMessage::SyncProposed {
        revision,
        rejection,
    };
    let not_anchor = ganbaru_sync_contracts::ProposalRejection::NotAnchor.as_u8();
    for valid in [answer(Some(3), None), answer(None, Some(not_anchor))] {
        assert_eq!(frame_round_trip(&valid), Ok(valid.clone()));
    }
    for invalid in [
        answer(None, None),
        answer(Some(3), Some(not_anchor)),
        answer(Some(0), None),
        answer(Some(u64::MAX), None),
        answer(None, Some(u8::MAX)),
    ] {
        assert!(invalid.validate().is_err(), "{invalid:?}");
    }

    let forward = |settled: Vec<SyncSettledProposal>| ControlMessage::SyncForward {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-laptop".to_string(),
        settled,
    };
    let rejected = |proposal: String, rejection: u8| SyncSettledProposal {
        proposal,
        rejection,
    };
    let hash = "ab".repeat(32);
    let removed = ganbaru_sync_contracts::ProposalRejection::DeviceRemoved.as_u8();
    let request = forward(vec![rejected(hash.clone(), removed)]);
    assert_eq!(frame_round_trip(&request), Ok(request.clone()));
    assert_eq!(request.sync_peer(), Some(("vault-1", "device-laptop")));
    for invalid in [
        forward(vec![rejected("AB".repeat(32), removed)]),
        forward(vec![rejected(hash[1..].to_string(), removed)]),
        forward(vec![rejected(hash.clone(), u8::MAX)]),
        forward(vec![
            rejected(hash.clone(), removed);
            MAX_SYNC_FORWARDED_PROPOSALS + 1
        ]),
    ] {
        assert!(invalid.validate().is_err(), "{invalid:?}");
    }
    let forwarded = |proposals: Vec<String>| ControlMessage::SyncForwarded { proposals };
    let held = forwarded(vec![text.clone(); MAX_SYNC_FORWARDED_PROPOSALS]);
    assert_eq!(frame_round_trip(&held), Ok(held.clone()));
    for invalid in [
        forwarded(vec![text.clone(); MAX_SYNC_FORWARDED_PROPOSALS + 1]),
        forwarded(vec![String::new()]),
    ] {
        assert!(invalid.validate().is_err(), "{invalid:?}");
    }
}

#[test]
fn sync_requests_of_the_previous_protocol_ask_for_an_update() {
    let hello = |protocol_version| ControlMessage::SyncHello {
        protocol_version,
        vault_id: "vault-1".to_string(),
        device_id: "device-phone".to_string(),
        manifest_version: 1,
        stored: Vec::new(),
        probe: None,
        listening: Vec::new(),
    };
    assert_eq!(hello(PROTOCOL_VERSION).validate(), Ok(()));
    assert_eq!(
        hello(PROTOCOL_VERSION - 1).validate(),
        Err("handoff protocol version is unsupported".to_string())
    );
}

#[test]
fn record_exchange_messages_round_trip_within_their_bounds() {
    let peer = || {
        (
            "vault-abcdefghijklmnopqrstuv".to_string(),
            "device-phone".to_string(),
        )
    };
    let records = |count: usize| vec!["AAEC".to_string(); count];
    let (vault_id, device_id) = peer();
    let request = ControlMessage::SyncRecords {
        protocol_version: PROTOCOL_VERSION,
        vault_id,
        device_id,
        cursor: vec![0, 3, 0, 9, 12],
        max_bytes: 64 * 1024,
    };
    assert!(request.is_record_exchange());
    assert_eq!(frame_round_trip(&request), Ok(request));
    let (vault_id, device_id) = peer();
    let get = ControlMessage::SyncRecordGet {
        protocol_version: PROTOCOL_VERSION,
        vault_id,
        device_id,
        hashes: vec!["a".repeat(64); MAX_SYNC_RECORDS],
        writers: vec!["b".repeat(32); MAX_SYNC_RECORDS],
    };
    assert!(get.is_record_exchange());
    assert_eq!(frame_round_trip(&get), Ok(get));
    let (vault_id, device_id) = peer();
    let push = ControlMessage::SyncRecordPush {
        protocol_version: PROTOCOL_VERSION,
        vault_id,
        device_id,
        records: records(MAX_SYNC_RECORDS),
    };
    assert!(push.is_record_exchange());
    assert_eq!(frame_round_trip(&push), Ok(push));
    let page = ControlMessage::SyncRecordPage {
        records: records(MAX_SYNC_RECORDS),
        cursor: vec![1; SYNC_RECORD_CLASSES],
        more: true,
    };
    assert!(!page.is_record_exchange());
    assert_eq!(frame_round_trip(&page), Ok(page));
    let result = ControlMessage::SyncRecordPushResult {
        admitted: MAX_SYNC_RECORDS as u32,
        missing_hashes: vec!["c".repeat(64)],
        missing_writers: vec!["d".repeat(32)],
    };
    assert_eq!(frame_round_trip(&result), Ok(result));
}

#[test]
fn record_exchange_messages_refuse_oversized_or_malformed_fields() {
    let records = |count: usize| vec!["AAEC".to_string(); count];
    let malformed = [
        ControlMessage::SyncRecords {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-abcdefghijklmnopqrstuv".to_string(),
            device_id: "device-phone".to_string(),
            cursor: vec![0; SYNC_RECORD_CLASSES - 1],
            max_bytes: 64 * 1024,
        },
        ControlMessage::SyncRecordPage {
            records: records(MAX_SYNC_RECORDS + 1),
            cursor: vec![0; SYNC_RECORD_CLASSES],
            more: false,
        },
        ControlMessage::SyncRecordGet {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-abcdefghijklmnopqrstuv".to_string(),
            device_id: "device-phone".to_string(),
            hashes: vec!["a".repeat(64); MAX_SYNC_RECORDS + 1],
            writers: Vec::new(),
        },
        ControlMessage::SyncRecordGet {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-abcdefghijklmnopqrstuv".to_string(),
            device_id: "device-phone".to_string(),
            hashes: Vec::new(),
            writers: vec!["B".repeat(32)],
        },
        ControlMessage::SyncRecordFound {
            records: vec![String::new()],
        },
        ControlMessage::SyncRecordPush {
            protocol_version: PROTOCOL_VERSION - 1,
            vault_id: "vault-abcdefghijklmnopqrstuv".to_string(),
            device_id: "device-phone".to_string(),
            records: records(1),
        },
        ControlMessage::SyncRecordPushResult {
            admitted: MAX_SYNC_RECORDS as u32 + 1,
            missing_hashes: Vec::new(),
            missing_writers: Vec::new(),
        },
    ];
    for message in malformed {
        assert!(message.validate().is_err(), "{message:?}");
    }
}

#[test]
fn anchor_join_requires_valid_keys_and_a_canonical_fingerprint() {
    let admin = ganbaru_sync_contracts::AdminKeyPair::from_seed(&[4; 32])
        .unwrap()
        .public_key();
    let join =
        |device_id: &str, admin_key: String, tls_fingerprint: String| ControlMessage::AnchorJoin {
            device_id: device_id.to_string(),
            device_label: "Work desktop".to_string(),
            device_key: encode_sync_device_key(&test_device_key()),
            admin_key,
            tls_fingerprint,
        };
    let fingerprint = "ab".repeat(32);
    let valid = join(
        "desktop-1",
        encode_sync_admin_key(&admin),
        fingerprint.clone(),
    );
    assert!(valid.validate().is_ok());
    for invalid in [
        join("", encode_sync_admin_key(&admin), fingerprint.clone()),
        join("desktop-1", "not a key".to_string(), fingerprint.clone()),
        join(
            "desktop-1",
            encode_sync_admin_key(&admin),
            fingerprint.to_uppercase(),
        ),
        join("desktop-1", encode_sync_admin_key(&admin), "ab".repeat(31)),
    ] {
        assert!(invalid.validate().is_err(), "{invalid:?}");
    }
}

fn chat_live_read(assignment_id: &str, after_change: Option<u64>) -> ControlMessage {
    ControlMessage::ChatLiveRead {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-phone".to_string(),
        assignment_id: assignment_id.to_string(),
        after_change,
        wait: true,
    }
}

fn chat_live(payload: String) -> ControlMessage {
    ControlMessage::ChatLive {
        payload,
        change_cursor: 7,
        has_more: false,
        reset_required: false,
        settled: false,
    }
}

#[test]
fn chat_live_read_accepts_generated_chat_ids_and_round_trips() {
    let read = chat_live_read("work:0123456789abcdef0123456789abcdef", Some(42));
    assert_eq!(frame_round_trip(&read), Ok(read.clone()));
    assert!(read.is_vault_material());
    assert_eq!(read.sync_peer(), None);
    assert!(!read.is_record_exchange());
}

#[test]
fn chat_live_read_refuses_bad_ids_cursors_and_versions() {
    for assignment_id in ["", "work id", "work/../x", "work\u{0}", &"a".repeat(161)] {
        assert!(
            chat_live_read(assignment_id, None).validate().is_err(),
            "accepted {assignment_id:?}"
        );
    }
    assert!(
        chat_live_read("work:1", Some(i64::MAX as u64 + 1))
            .validate()
            .is_err()
    );
    let mut read = chat_live_read("work:1", None);
    if let ControlMessage::ChatLiveRead {
        protocol_version, ..
    } = &mut read
    {
        *protocol_version = PROTOCOL_VERSION - 1;
    }
    assert!(read.validate().is_err());
}

fn chat_approval_resolve(request_id: &str, provider_option_id: &str) -> ControlMessage {
    ControlMessage::ChatApprovalResolve {
        protocol_version: PROTOCOL_VERSION,
        vault_id: "vault-1".to_string(),
        device_id: "device-phone".to_string(),
        assignment_id: "work-assignment:0123456789abcdef".to_string(),
        request_id: request_id.to_string(),
        decision: ChatRemoteApprovalDecision::AllowOnce,
        provider_option_id: provider_option_id.to_string(),
        command_id: "remote-approval:0df9a7d7-004d-41f0".to_string(),
    }
}

#[test]
fn chat_approval_resolve_round_trips_and_counts_as_vault_material() {
    let resolve = chat_approval_resolve("0df9a7d7-004d-41f0-aeb3-bfcca692e3af", "allow_once");
    assert_eq!(frame_round_trip(&resolve), Ok(resolve.clone()));
    assert!(resolve.is_vault_material());
    assert_eq!(resolve.sync_peer(), None);
    let resolved = ControlMessage::ChatApprovalResolved;
    assert_eq!(frame_round_trip(&resolved), Ok(resolved));
}

#[test]
fn chat_approval_resolve_refuses_session_grants_and_bad_options_but_takes_provider_request_ids() {
    let mut message = serde_json::to_value(chat_approval_resolve("request-1", "allow_once"))
        .expect("serialize the approval");
    message["decision"] = serde_json::json!("allow_session");
    assert!(serde_json::from_value::<ControlMessage>(message).is_err());
    for request_id in ["", "request\n1", &"a".repeat(1025)] {
        assert!(
            chat_approval_resolve(request_id, "allow_once")
                .validate()
                .is_err(),
            "accepted {request_id:?}"
        );
    }
    for option in ["", "allow\nonce", &"a".repeat(257)] {
        assert!(
            chat_approval_resolve("request-1", option)
                .validate()
                .is_err(),
            "accepted {option:?}"
        );
    }
    assert_eq!(
        chat_approval_resolve("call_9f2 |fc_01/tool:edit", "Allow once (proceed)").validate(),
        Ok(())
    );
}

#[test]
fn chat_live_payload_bound_counts_json_escapes() {
    assert_eq!(chat_live_payload_bytes("ab"), 4);
    assert_eq!(chat_live_payload_bytes("\"\\\n"), 8);
    assert_eq!(chat_live_payload_bytes("\u{1}"), 8);
    assert_eq!(chat_live_payload_bytes("é"), 4);
    // Quotes and backslashes double on the wire, so the raw length alone would let a frame
    // pass the payload bound and fail the frame bound.
    let escaped = "\"\\".repeat(MAX_CHAT_LIVE_PAYLOAD_BYTES / 4);
    assert!(escaped.len() < MAX_CHAT_LIVE_PAYLOAD_BYTES);
    assert!(chat_live(escaped).validate().is_err());
    let largest = "\"\\".repeat(MAX_CHAT_LIVE_PAYLOAD_BYTES / 4 - 1);
    let answer = chat_live(largest);
    assert_eq!(answer.validate(), Ok(()));
    assert_eq!(frame_round_trip(&answer), Ok(answer));
    let controls = "\u{1}".repeat(MAX_CHAT_LIVE_PAYLOAD_BYTES / 6 - 1);
    let answer = chat_live(controls);
    assert_eq!(frame_round_trip(&answer), Ok(answer));
}

#[test]
fn chat_live_refuses_empty_payloads_and_out_of_range_cursors() {
    assert!(chat_live(String::new()).validate().is_err());
    let ControlMessage::ChatLive {
        payload,
        has_more,
        reset_required,
        settled,
        ..
    } = chat_live("{}".to_string())
    else {
        unreachable!("built a live answer");
    };
    let answer = ControlMessage::ChatLive {
        payload,
        change_cursor: u64::MAX,
        has_more,
        reset_required,
        settled,
    };
    assert!(answer.validate().is_err());
}
