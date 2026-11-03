use std::collections::{BTreeSet, HashMap};

use ganbaru_sync::Presentation;
use ganbaru_sync::{AuthorizationView, DeviceNames, Engine, EnrolledDevice, LateChange, LateKind};
use ganbaru_sync_contracts::{
    AdminKeyPair, AdminPublicKey, DeviceKind, DevicePublicKey, DeviceRole, Entry, Hlc,
    LateResolution, LateRow, ProposalRejection, Remove, ResolveLate, RowKey, SetRole, SpaceId,
};
use ganbaru_sync_replica::authority::propose;
use ganbaru_sync_replica::outbox::OutboxEntry;
use ganbaru_sync_replica::service::peers::coordinator_allowed;

use super::super::host::{StagedTrust, staged_trust};
use super::*;
use crate::sync::pinned_trust;
use crate::vault::handoff::pairing::{CoordinatorPin, PinnedCharter};

const VAULT_ID: &str = "vault-authority";
const AT_MS: u64 = 1_800_000_000_000;

fn admin(seed: u8) -> AdminKeyPair {
    AdminKeyPair::from_seed(&[seed; 32]).expect("admin seed")
}

fn device(id: &str, kind: DeviceKind, role: DeviceRole, removed: Option<u64>) -> EnrolledDevice {
    EnrolledDevice {
        device_id: id.to_string(),
        device_key: DevicePublicKey::from_bytes([7; 32]),
        tls_fingerprint: [0; 32],
        label: Some(format!("{id} label")),
        kind,
        role,
        enrolled_revision: 1,
        removed_revision: removed,
        held: false,
        encryption_key: None,
    }
}

fn view(devices: Vec<EnrolledDevice>) -> AuthorizationView {
    AuthorizationView {
        epoch: 2,
        revision: 9,
        entry_hash: [1; 32],
        anchor_device_id: "device-a".to_string(),
        frozen_at: None,
        catch_up: None,
        charter: [2; 32],
        recovery_admin_key: AdminPublicKey::from_bytes([3; 32]),
        kit_admin_key: AdminPublicKey::from_bytes([3; 32]),
        devices,
        discovery_key: [4; 32],
        previous_discovery_key: None,
        key_epoch: 0,
        recovery_encryption_key: None,
        pending_removal: None,
        server: None,
    }
}

fn names() -> DeviceNames {
    DeviceNames::new(
        "device-b",
        HashMap::from([("device-a".to_string(), "Desk".to_string())]),
    )
}

fn entry(change: Entry, created_ms: u64) -> OutboxEntry {
    OutboxEntry {
        proposal: propose(
            SpaceId::personal(VAULT_ID),
            [2; 32],
            &admin(5),
            change,
            created_ms,
        )
        .expect("proposal"),
        rejection: None,
        held: None,
    }
}

#[test]
fn sync_authority_view_lists_roles_anchor_quarantine_and_labels() {
    let view = view(vec![
        device("device-c", DeviceKind::Mobile, DeviceRole::Member, Some(4)),
        device(
            "device-a",
            DeviceKind::Desktop,
            DeviceRole::Admin(admin(1).public_key()),
            None,
        ),
        device("device-b", DeviceKind::Desktop, DeviceRole::Member, None),
    ]);
    let quarantined = BTreeSet::from(["device-c".to_string()]);
    let authority = authority_view(&view, &names(), &quarantined, false);

    assert_eq!(authority.anchor_device_id, "device-a");
    assert_eq!((authority.epoch, authority.revision), (2, 9));
    assert!(!authority.frozen && !authority.own_is_admin);
    let ids: Vec<&str> = authority
        .devices
        .iter()
        .map(|device| device.device.device_id.as_str())
        .collect();
    assert_eq!(ids, ["device-a", "device-b", "device-c"]);
    let [anchor, own, removed] = authority.devices.as_slice() else {
        panic!("three devices");
    };
    assert!(anchor.anchor && anchor.role == DeviceRoleView::Admin);
    assert_eq!(
        anchor.device.device_label.as_deref(),
        Some("Desk"),
        "the pairing label wins"
    );
    assert!(own.device.own_device && !own.anchor);
    assert_eq!(
        own.device.device_label.as_deref(),
        Some("device-b label"),
        "the enrollment label fills a missing pairing label"
    );
    assert!(removed.quarantined && removed.kind == DeviceKindView::Mobile);
    assert_eq!(removed.removed_revision, Some(4));

    let wire = serde_json::to_value(&authority).expect("serialize authority");
    assert_eq!(wire["anchorDeviceId"], "device-a");
    assert_eq!(wire["ownIsAdmin"], false);
    assert_eq!(wire["devices"][0]["role"], "admin");
    assert_eq!(wire["devices"][2]["kind"], "mobile");
    assert_eq!(wire["devices"][2]["removedRevision"], 4);
}

#[test]
fn sync_authority_view_reports_a_frozen_epoch() {
    let mut frozen = view(Vec::new());
    frozen.frozen_at = Some(7);
    assert!(authority_view(&frozen, &names(), &BTreeSet::new(), true).frozen);
}

#[test]
fn sync_outbox_views_name_the_change_its_target_and_its_state_newest_first() {
    let mut rejected = entry(
        Entry::Remove(Remove {
            device_id: "device-a".to_string(),
        }),
        AT_MS,
    );
    rejected.rejection = Some(ProposalRejection::NamesAnchor);
    let mut held = entry(
        Entry::SetRole(SetRole::Device {
            device_id: "device-c".to_string(),
            role: DeviceRole::Admin(admin(6).public_key()),
        }),
        AT_MS + 1,
    );
    held.held = Some(2);
    let pending = entry(
        Entry::SetRole(SetRole::Recovery {
            recovery_admin_key: admin(8).public_key(),
        }),
        AT_MS + 2,
    );

    let views = outbox_views(&[rejected, held, pending], &names());
    let summary: Vec<_> = views
        .iter()
        .map(|view| {
            (
                view.change,
                view.target.as_ref().map(|target| target.device_id.as_str()),
                view.state,
                view.rejection,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                ProposalChangeView::ReplaceRecoveryKit,
                None,
                OutboxStateView::Pending,
                None
            ),
            (
                ProposalChangeView::MakeAdmin,
                Some("device-c"),
                OutboxStateView::Held,
                None
            ),
            (
                ProposalChangeView::Remove,
                Some("device-a"),
                OutboxStateView::Rejected,
                Some("names_anchor")
            ),
        ]
    );
    assert_eq!(views[2].created_at_ms, AT_MS);
    let wire = serde_json::to_value(&views[0]).expect("serialize outbox entry");
    assert_eq!(wire["change"], "replaceRecoveryKit");
    assert_eq!(wire["state"], "pending");
}

#[test]
fn sync_late_views_mark_rows_a_pending_proposal_already_decides() {
    let engine = Engine::vault();
    let table = engine
        .manifest()
        .table_named("quick_notes")
        .expect("quick notes table")
        .id;
    let late = |row_key: &str, kind, clock| LateChange {
        removal_revision: 5,
        device_id: "device-c".to_string(),
        table,
        row_key: row_key.to_string(),
        kind,
        presentation: Presentation {
            title: format!("{row_key} title"),
            preview: String::new(),
        },
        values: Vec::new(),
        clock,
        restorable: true,
    };
    let decided = entry(
        Entry::ResolveLate(ResolveLate {
            removal_revision: 5,
            rows: vec![LateRow {
                table,
                row: RowKey::new("note-decided").expect("row key"),
                resolution: LateResolution::Discard,
            }],
        }),
        AT_MS,
    );
    let mut sequenced = entry(
        Entry::ResolveLate(ResolveLate {
            removal_revision: 5,
            rows: vec![LateRow {
                table,
                row: RowKey::new("note-open").expect("row key"),
                resolution: LateResolution::Restore,
            }],
        }),
        AT_MS + 1,
    );
    sequenced.held = Some(2);

    let mut changes = vec![
        late("note-decided", LateKind::Tombstone, None),
        late(
            "note-open",
            LateKind::Values,
            Some(Hlc::new(AT_MS + 60_000, 3)),
        ),
    ];
    changes[1].restorable = false;
    let listed = late_views(
        &engine,
        changes.clone(),
        &[decided.clone(), sequenced.clone()],
        &names(),
        MAX_LISTED_LATE_CHANGES,
    );
    assert_eq!(listed.total, 2);
    let views = listed.changes;
    assert_eq!(views.len(), 2);
    assert!(views[0].decision_pending);
    assert_eq!(views[0].kind, LateKindView::Tombstone);
    assert_eq!(views[0].changed_at_ms, None);
    assert!(
        !views[1].decision_pending,
        "a held proposal no longer waits for the anchor"
    );
    assert_eq!(views[1].changed_at_ms, Some(AT_MS + 60_000));
    assert_eq!(views[1].table, "quick_notes");
    assert_eq!(views[1].title, "note-open title");
    let wire = serde_json::to_value(&views[1]).expect("serialize late change");
    assert_eq!(wire["kind"], "values");
    assert_eq!(wire["removalRevision"], 5);
    assert_eq!(wire["device"]["deviceId"], "device-c");
    assert!(views[0].restorable);
    assert_eq!(
        wire["restorable"], false,
        "the view keeps whether keeping can write back"
    );

    let bounded = late_views(&engine, changes, &[decided, sequenced], &names(), 1);
    assert_eq!(
        bounded.total, 2,
        "the total counts the changes past the bound"
    );
    assert_eq!(bounded.changes.len(), 1);
    assert_eq!(bounded.changes[0].row_key, "note-decided");
    let wire = serde_json::to_value(&bounded).expect("serialize late changes");
    assert_eq!(wire["total"], 2);
}

/// A charter of the test vault founded at `created_ms`, with the person key that signed it.
fn charter_trust_at(created_ms: i64) -> crate::sync::CharterTrust {
    let person = ganbaru_contacts::PersonKeyPair::from_seed(&[8; 32]).expect("person key");
    let charter = ganbaru_sync_contracts::sign_charter(
        &ganbaru_sync_contracts::Charter {
            space: SpaceId::personal(VAULT_ID),
            anchor_device_id: "device-a".to_string(),
            anchor_device_key: DevicePublicKey::from_bytes([7; 32]),
            anchor_tls_fingerprint: [0; 32],
            anchor_admin_key: admin(5).public_key(),
            recovery_admin_key: admin(6).public_key(),
            discovery_key: [4; 32],
            created_at_ms: created_ms,
        },
        &person,
    )
    .expect("sign charter");
    (charter, person.public_key())
}

/// A coordinator pin of `vault_id` holding `trust` as its charter.
fn pin(vault_id: &str, trust: Option<&crate::sync::CharterTrust>) -> CoordinatorPin {
    CoordinatorPin {
        device_id: "device-a".to_string(),
        device_label: None,
        endpoint: "127.0.0.1:1".to_string(),
        vault_id: vault_id.to_string(),
        certificate_fingerprint: String::new(),
        last_endpoint: None,
        reported_endpoints: Vec::new(),
        charter: trust.map(|(charter, person_key)| PinnedCharter {
            charter: crate::vault::handoff::protocol::encode_sync_charter(charter),
            person_key: person_key.to_text(),
        }),
    }
}

/// A view of the test vault whose selected log starts from `charter` and removed `device-a`.
fn view_removing_the_coordinator(charter: [u8; 32]) -> AuthorizationView {
    AuthorizationView {
        charter,
        ..view(vec![device(
            "device-a",
            DeviceKind::Desktop,
            DeviceRole::Member,
            Some(5),
        )])
    }
}

#[test]
fn first_copy_with_no_trusted_charter_is_left_alone() {
    assert_eq!(pinned_trust(None, VAULT_ID), Ok(None));
    assert_eq!(staged_trust(None, None), None);
}

#[test]
fn staged_trust_prefers_the_pin_and_merges_only_the_same_log() {
    let current = charter_trust_at(1);
    let refounded = charter_trust_at(2);
    let trusted = |(charter, person_key): crate::sync::CharterTrust, merge_live| StagedTrust {
        charter,
        person_key,
        merge_live,
    };

    assert_eq!(
        staged_trust(Some(current.clone()), Some(current.clone())),
        Some(trusted(current.clone(), true))
    );
    assert_eq!(
        staged_trust(Some(refounded.clone()), Some(current.clone())),
        Some(trusted(refounded.clone(), false))
    );
    assert_eq!(
        staged_trust(Some(current.clone()), None),
        Some(trusted(current.clone(), false))
    );
    assert_eq!(
        staged_trust(None, Some(current.clone())),
        Some(trusted(current.clone(), true))
    );
    assert_eq!(staged_trust(None, None), None);

    let pinned = pin(VAULT_ID, Some(&current));
    assert_eq!(
        pinned_trust(Some(&pinned), VAULT_ID),
        Ok(Some(current.clone()))
    );
    assert_eq!(pinned_trust(Some(&pinned), "vault-other"), Ok(None));
    assert_eq!(pinned_trust(Some(&pin(VAULT_ID, None)), VAULT_ID), Ok(None));
    assert!(
        pinned_trust(Some(&pin("vault-other", Some(&current))), "vault-other").is_err(),
        "a pinned charter of another space is refused"
    );
}

#[test]
fn bundle_sources_skip_a_removed_coordinator() {
    let (charter, _) = charter_trust_at(1);
    let removed = view_removing_the_coordinator(charter.hash());
    let present = AuthorizationView {
        charter: charter.hash(),
        ..view(vec![device(
            "device-a",
            DeviceKind::Desktop,
            DeviceRole::Member,
            None,
        )])
    };
    let none = BTreeSet::new();

    assert!(!coordinator_allowed(
        "device-a",
        &none,
        Some(&removed),
        Some(charter.hash())
    ));
    assert!(coordinator_allowed(
        "device-a",
        &none,
        Some(&present),
        Some(charter.hash())
    ));
    assert!(coordinator_allowed(
        "device-a",
        &none,
        None,
        Some(charter.hash())
    ));
}

#[test]
fn bundle_sources_keep_a_coordinator_after_a_new_charter() {
    let (old, _) = charter_trust_at(1);
    let (refounded, _) = charter_trust_at(2);
    let removed = view_removing_the_coordinator(old.hash());
    let none = BTreeSet::new();

    assert!(coordinator_allowed(
        "device-a",
        &none,
        Some(&removed),
        Some(refounded.hash())
    ));
    assert!(coordinator_allowed("device-a", &none, Some(&removed), None));
    let quarantined = BTreeSet::from(["device-a".to_string()]);
    assert!(!coordinator_allowed(
        "device-a",
        &quarantined,
        Some(&removed),
        Some(refounded.hash())
    ));
}
