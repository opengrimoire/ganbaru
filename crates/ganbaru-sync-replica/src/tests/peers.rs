//! Member listener admission, the peer book, and the pass planner.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_handoff::protocol::{ControlMessage, MemberEndpoint, PROTOCOL_VERSION};
use ganbaru_sync::{AuthorizationView, EnrolledDevice};
use ganbaru_sync_contracts::{AdminPublicKey, DeviceKind, DevicePublicKey, DeviceRole};

use super::TestDirectory;
use crate::peers::admission::{MemberAdmission, MemberRefusal, fingerprint_hex};
use crate::peers::book::{
    MAX_ENDPOINTS_PER_PEER, MAX_PEER_BOOK_BYTES, MAX_PEER_ENDPOINTS, PeerBook, PeerEndpoints,
};
use crate::peers::plan::{
    DESKTOP_MAX_EXCHANGES, MOBILE_MAX_EXCHANGES, PeerSource, PlanInput, plan_pass,
};

const VAULT: &str = "vault-peers";

fn device(index: u8, removed: bool) -> EnrolledDevice {
    EnrolledDevice {
        device_id: format!("device-{index}"),
        device_key: DevicePublicKey::from_bytes([index; 32]),
        tls_fingerprint: [index; 32],
        label: None,
        kind: DeviceKind::Desktop,
        role: DeviceRole::Member,
        enrolled_revision: u64::from(index),
        removed_revision: removed.then_some(u64::from(index) + 10),
    }
}

fn view(devices: Vec<EnrolledDevice>) -> AuthorizationView {
    AuthorizationView {
        epoch: 1,
        revision: 4,
        entry_hash: [7; 32],
        anchor_device_id: "device-1".to_string(),
        frozen_at: None,
        catch_up: None,
        charter: [8; 32],
        recovery_admin_key: AdminPublicKey::from_bytes([9; 32]),
        devices,
    }
}

fn sync_hello(vault_id: &str, device_id: &str) -> ControlMessage {
    ControlMessage::SyncHello {
        protocol_version: PROTOCOL_VERSION,
        vault_id: vault_id.to_string(),
        device_id: device_id.to_string(),
        manifest_version: 1,
        stored: Vec::new(),
        probe: None,
        listening: Vec::new(),
    }
}

#[test]
fn member_admission_accepts_only_enrolled_unremoved_unquarantined_certificates() {
    let quarantined = BTreeSet::from(["device-3".to_string()]);
    let admission = MemberAdmission::new(
        VAULT,
        &view(vec![
            device(1, false),
            device(2, true),
            device(3, false),
            device(4, false),
        ]),
        &quarantined,
    );

    assert_eq!(
        admission.certificate(&fingerprint_hex(&[1; 32])),
        Ok("device-1")
    );
    assert_eq!(
        admission.certificate(&fingerprint_hex(&[2; 32])),
        Err(MemberRefusal::Removed)
    );
    assert_eq!(
        admission.certificate(&fingerprint_hex(&[3; 32])),
        Err(MemberRefusal::Quarantined)
    );
    assert_eq!(
        admission.certificate(&fingerprint_hex(&[5; 32])),
        Err(MemberRefusal::Unknown)
    );
    assert_eq!(
        admission.admitted_fingerprints(),
        BTreeSet::from([fingerprint_hex(&[1; 32]), fingerprint_hex(&[4; 32])])
    );
}

#[test]
fn member_admission_refuses_requests_that_are_not_sync_or_name_another_device() {
    let admission = MemberAdmission::new(VAULT, &view(vec![device(1, false)]), &BTreeSet::new());
    let fingerprint = fingerprint_hex(&[1; 32]);

    assert_eq!(
        admission.request(&fingerprint, &sync_hello(VAULT, "device-1")),
        Ok(())
    );
    assert_eq!(
        admission.request(&fingerprint, &sync_hello(VAULT, "device-4")),
        Err(MemberRefusal::DeviceMismatch)
    );
    assert_eq!(
        admission.request(&fingerprint, &sync_hello("vault-other", "device-1")),
        Err(MemberRefusal::VaultMismatch)
    );
    assert_eq!(
        admission.request(&fingerprint_hex(&[6; 32]), &sync_hello(VAULT, "device-1")),
        Err(MemberRefusal::Unknown)
    );
    for request in [
        ControlMessage::PersonKeyRequest {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT.to_string(),
            device_id: "device-1".to_string(),
        },
        ControlMessage::ContactRequestStatus {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-1".to_string(),
            requester_public_key: "key".to_string(),
            issued_at_ms: 1,
            signature: "signature".to_string(),
        },
        ControlMessage::CancelTransfer {
            transfer_id: "transfer-1".to_string(),
        },
    ] {
        assert_eq!(
            admission.request(&fingerprint, &request),
            Err(MemberRefusal::NotSync)
        );
    }
    for request in [
        ControlMessage::SyncPropose {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT.to_string(),
            device_id: "device-1".to_string(),
            proposal: "proposal".to_string(),
        },
        ControlMessage::SyncForward {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT.to_string(),
            device_id: "device-1".to_string(),
            settled: Vec::new(),
        },
    ] {
        assert_eq!(
            admission.request(&fingerprint, &request),
            Err(MemberRefusal::Proposal)
        );
    }
}

fn endpoint(last: u8) -> String {
    format!("192.168.1.{last}:43821")
}

#[test]
fn peer_book_round_trips_and_keeps_the_newest_observation() {
    let directory = TestDirectory::new("peer-book");
    let path = directory.files().peers_path();
    let mut book = PeerBook::load(&path).expect("missing file is empty");
    assert!(book.endpoints().is_empty());

    let mut endpoints = book.endpoints().clone();
    assert!(endpoints.observe("device-2", &[endpoint(20), endpoint(21)], 200));
    assert!(!endpoints.observe("device-2", &[endpoint(20)], 100));
    assert!(endpoints.observe("device-2", &[endpoint(21)], 300));
    assert!(!endpoints.observe("device 2", &[endpoint(22)], 400));
    assert!(!endpoints.observe("device-3", &["8.8.8.8:43821".to_string()], 400));
    assert!(endpoints.observe_reported(&[MemberEndpoint {
        device_id: "device-3".to_string(),
        endpoint: endpoint(30),
        observed_ms: 250,
    }]));
    book.store(endpoints.clone()).expect("store");

    let reloaded = PeerBook::load(&path).expect("reload");
    assert_eq!(reloaded.endpoints(), &endpoints);
    assert_eq!(
        reloaded.endpoints().endpoints("device-2"),
        vec![endpoint(21), endpoint(20)]
    );
    let reported: Vec<(String, u64)> = reloaded
        .endpoints()
        .report()
        .into_iter()
        .map(|member| (member.endpoint, member.observed_ms))
        .collect();
    assert_eq!(
        reported,
        vec![
            (endpoint(21), 300),
            (endpoint(30), 250),
            (endpoint(20), 200)
        ]
    );

    let mut retained = reloaded.endpoints().clone();
    assert!(retained.retain_members(&BTreeSet::from(["device-3".to_string()])));
    assert_eq!(retained.devices(), BTreeSet::from(["device-3".to_string()]));
}

#[test]
fn peer_book_bounds_endpoints_per_member_and_in_total() {
    let mut endpoints = PeerEndpoints::default();
    let many: Vec<String> = (1..=20).map(endpoint).collect();
    endpoints.observe("device-2", &many, 10);
    assert_eq!(
        endpoints.endpoints("device-2").len(),
        MAX_ENDPOINTS_PER_PEER
    );
    for index in 0..10u8 {
        let addresses: Vec<String> = (0..8u8)
            .map(|last| format!("10.0.{index}.{}:43821", last + 1))
            .collect();
        endpoints.observe(
            &format!("member-{index}"),
            &addresses,
            100 + u64::from(index),
        );
    }
    let reported = endpoints.report();
    assert_eq!(reported.len(), MAX_PEER_ENDPOINTS);
    assert!(reported.iter().all(|member| member.observed_ms >= 106));
}

#[test]
fn peer_book_refuses_corrupt_oversized_or_unbounded_files() {
    let directory = TestDirectory::new("peer-book-corrupt");
    let path = directory.files().peers_path();
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");

    std::fs::write(&path, b"{not json").expect("write corrupt");
    assert!(PeerBook::load(&path).is_err());
    std::fs::write(&path, br#"{"schemaVersion":2,"endpoints":[]}"#).expect("write newer");
    assert!(PeerBook::load(&path).is_err());
    std::fs::write(&path, br#"{"schemaVersion":1,"endpoints":[],"extra":true}"#)
        .expect("write unknown field");
    assert!(PeerBook::load(&path).is_err());
    std::fs::write(
        &path,
        br#"{"schemaVersion":1,"endpoints":[{"deviceId":"device-2","endpoint":"8.8.8.8:43821","observedMs":1}]}"#,
    )
    .expect("write public endpoint");
    assert!(PeerBook::load(&path).is_err());
    let repeated = format!(
        r#"{{"schemaVersion":1,"endpoints":[{entry},{entry}]}}"#,
        entry = r#"{"deviceId":"device-2","endpoint":"192.168.1.20:43821","observedMs":1}"#
    );
    std::fs::write(&path, repeated).expect("write repeated");
    assert!(PeerBook::load(&path).is_err());
    let crowded: Vec<String> = (1..=9u8)
        .map(|last| {
            format!(
                r#"{{"deviceId":"device-2","endpoint":"192.168.1.{last}:43821","observedMs":1}}"#
            )
        })
        .collect();
    std::fs::write(
        &path,
        format!(
            r#"{{"schemaVersion":1,"endpoints":[{}]}}"#,
            crowded.join(",")
        ),
    )
    .expect("write crowded");
    assert!(PeerBook::load(&path).is_err());
    std::fs::write(&path, vec![b' '; MAX_PEER_BOOK_BYTES + 1]).expect("write oversized");
    assert!(PeerBook::load(&path).is_err());
}

fn members(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_string()).collect()
}

#[test]
fn pass_plan_prefers_the_anchor_then_discovered_then_known_members_within_the_limit() {
    let members = members(&["anchor", "self", "lan-1", "lan-2", "known-1", "known-2"]);
    let anchor_endpoints = vec![endpoint(1)];
    let discovered = BTreeMap::from([
        ("lan-1".to_string(), vec![endpoint(11)]),
        ("lan-2".to_string(), vec![endpoint(12)]),
        ("stranger".to_string(), vec![endpoint(13)]),
    ]);
    let mut known = PeerEndpoints::default();
    known.observe("known-1", &[endpoint(21)], 50);
    known.observe("known-2", &[endpoint(22)], 60);
    known.observe("lan-1", &[endpoint(31)], 70);
    let input = PlanInput {
        kind: DeviceKind::Desktop,
        own_device_id: "self",
        members: &members,
        anchor: Some(("anchor", &anchor_endpoints)),
        anchor_reachable: true,
        discovered: &discovered,
        known: &known,
    };

    let plan = plan_pass(&input);
    let chosen: Vec<(&str, PeerSource)> = plan
        .peers
        .iter()
        .map(|peer| (peer.device_id.as_str(), peer.source))
        .collect();
    assert_eq!(plan.peers.len(), DESKTOP_MAX_EXCHANGES);
    assert_eq!(
        chosen,
        vec![
            ("anchor", PeerSource::Anchor),
            ("lan-1", PeerSource::Discovered),
            ("lan-2", PeerSource::Discovered),
        ]
    );
    assert_eq!(plan.long_poll.as_deref(), Some("anchor"));

    let phone = plan_pass(&PlanInput {
        kind: DeviceKind::Mobile,
        discovered: &BTreeMap::new(),
        ..input.clone()
    });
    let chosen: Vec<&str> = phone
        .peers
        .iter()
        .map(|peer| peer.device_id.as_str())
        .collect();
    assert_eq!(phone.peers.len(), MOBILE_MAX_EXCHANGES);
    assert_eq!(chosen, vec!["anchor", "lan-1"]);
    assert_eq!(phone.peers[1].source, PeerSource::Known);
}

#[test]
fn pass_plan_keeps_a_slot_for_an_unreachable_anchor_and_long_polls_a_member() {
    let members = members(&["anchor", "self", "known-1", "known-2", "known-3"]);
    let anchor_endpoints = vec![endpoint(1)];
    let mut known = PeerEndpoints::default();
    known.observe("known-1", &[endpoint(21)], 50);
    known.observe("known-2", &[endpoint(22)], 60);
    known.observe("known-3", &[endpoint(23)], 70);
    known.observe("removed", &[endpoint(24)], 80);
    known.observe("self", &[endpoint(25)], 90);
    let plan = plan_pass(&PlanInput {
        kind: DeviceKind::Desktop,
        own_device_id: "self",
        members: &members,
        anchor: Some(("anchor", &anchor_endpoints)),
        anchor_reachable: false,
        discovered: &BTreeMap::new(),
        known: &known,
    });
    let chosen: Vec<&str> = plan
        .peers
        .iter()
        .map(|peer| peer.device_id.as_str())
        .collect();
    assert_eq!(chosen, vec!["known-3", "known-2", "anchor"]);
    assert_eq!(plan.long_poll.as_deref(), Some("known-3"));

    let alone = plan_pass(&PlanInput {
        kind: DeviceKind::Desktop,
        own_device_id: "self",
        members: &members,
        anchor: None,
        anchor_reachable: false,
        discovered: &BTreeMap::new(),
        known: &PeerEndpoints::default(),
    });
    assert!(alone.peers.is_empty());
    assert_eq!(alone.long_poll, None);
}
