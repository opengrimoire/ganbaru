//! The proposal outbox and the anchor's sequencing of it.

use super::{
    DEVICE_ID, NOW_MS, TestDirectory, VAULT_ID, acquire, anchor_keys, block_on, context,
    create_vault, device,
};
use crate::authority::{self, Enrollment, enroll_entry, propose, remove_entry};
use crate::outbox::{MAX_OUTBOX_PROPOSALS, Outbox, is_final};
use crate::writer::{MemoryKeyStore, SuccessorPlan, create_writer, read_record};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::Engine;
use ganbaru_sync_contracts::{
    DeviceKeyPair, DeviceKind, ProposalRejection, SignedProposal, SpaceId,
};

fn enrollment(device_id: &str, seed: u8) -> Enrollment {
    Enrollment {
        device_id: device_id.to_string(),
        device_key: DeviceKeyPair::from_seed(&[seed; 32])
            .expect("device seed")
            .public_key(),
        tls_fingerprint: [seed; 32],
        label: device_id.to_string(),
        device_kind: DeviceKind::Mobile,
    }
}

fn enroll_proposal(device_id: &str, seed: u8, now_ms: u64) -> SignedProposal {
    propose(
        SpaceId::personal(VAULT_ID),
        &anchor_keys().admin,
        enroll_entry(enrollment(device_id, seed)),
        now_ms,
    )
    .expect("sign proposal")
}

#[test]
fn the_outbox_keeps_waiting_proposals_and_final_rejections_across_reloads() {
    let directory = TestDirectory::new("outbox-file");
    let path = directory.files().outbox_path();
    let mut outbox = Outbox::load(&path).expect("missing outbox is empty");
    assert!(outbox.entries().is_empty());

    let first = enroll_proposal("device-phone", 30, NOW_MS);
    let second = enroll_proposal("device-tablet", 31, NOW_MS);
    assert!(outbox.add(first.clone()).expect("add first"));
    assert!(!outbox.add(first.clone()).expect("same proposal again"));
    assert!(outbox.add(second.clone()).expect("add second"));

    // A revision or a rejection that is not final leaves a proposal waiting.
    outbox
        .settle(&first.hash(), Ok(4))
        .expect("settle revision");
    outbox
        .settle(&first.hash(), Err(ProposalRejection::NotAnchor))
        .expect("settle not anchor");
    outbox
        .settle(&second.hash(), Err(ProposalRejection::AlreadyEnrolled))
        .expect("settle final");
    assert_eq!(outbox.pending(), vec![first.clone()]);

    let reloaded = Outbox::load(&path).expect("reload");
    assert_eq!(reloaded.entries(), outbox.entries());
    assert_eq!(
        reloaded.entries()[1].rejection,
        Some(ProposalRejection::AlreadyEnrolled)
    );
    assert!(
        reloaded
            .find(|entry| *entry == second.proposal.entry)
            .is_some(),
        "a rejected proposal still counts as proposed"
    );
}

#[test]
fn the_outbox_refuses_files_it_did_not_write() {
    let directory = TestDirectory::new("outbox-refuse");
    let path = directory.files().outbox_path();
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    let proposal = URL_SAFE_NO_PAD.encode(enroll_proposal("device-phone", 30, NOW_MS).encode());
    let mut forged = enroll_proposal("device-phone", 30, NOW_MS).encode();
    let last = forged.len() - 1;
    forged[last] ^= 1;
    let cases = [
        r#"{"schemaVersion":2,"proposals":[]}"#.to_string(),
        r#"{"schemaVersion":1,"proposals":[],"extra":true}"#.to_string(),
        format!(r#"{{"schemaVersion":1,"proposals":[{{"proposal":"{proposal}","rejection":0}}]}}"#),
        format!(
            r#"{{"schemaVersion":1,"proposals":[{{"proposal":"{proposal}","rejection":99}}]}}"#
        ),
        format!(
            r#"{{"schemaVersion":1,"proposals":[{{"proposal":"{}"}}]}}"#,
            URL_SAFE_NO_PAD.encode(forged)
        ),
    ];
    for case in cases {
        std::fs::write(&path, &case).expect("write outbox");
        assert!(Outbox::load(&path).is_err(), "accepted {case}");
    }
}

#[test]
fn the_outbox_is_bounded() {
    let directory = TestDirectory::new("outbox-bound");
    let mut outbox = Outbox::load(&directory.files().outbox_path()).expect("load");
    for index in 0..MAX_OUTBOX_PROPOSALS {
        let now = NOW_MS + u64::try_from(index).expect("index");
        outbox
            .add(enroll_proposal("device-phone", 30, now))
            .expect("add within bound");
    }
    let extra = enroll_proposal("device-phone", 30, NOW_MS + 10_000);
    assert!(outbox.add(extra).is_err());
}

#[test]
fn only_a_moved_chain_ends_a_waiting_proposal() {
    let waiting = [
        ProposalRejection::NotAnchor,
        ProposalRejection::EpochFrozen,
        ProposalRejection::HandoverConflict,
    ];
    for rejection in ProposalRejection::ALL {
        assert_eq!(
            is_final(rejection),
            !waiting.contains(&rejection),
            "{rejection}"
        );
    }
}

#[test]
fn the_anchor_sequences_its_outbox_reserves_each_entry_and_keeps_final_rejections() {
    block_on(async {
        let directory = TestDirectory::new("outbox-anchor");
        let registry = DatabasePoolRegistry::default();
        let engine = Engine::vault();
        let files = directory.files();
        let keys = MemoryKeyStore::default();
        let pool = create_vault(&registry, &directory.path().join("vault.sqlite")).await;
        let mut conn = acquire(&pool).await;
        let ctx = context(&mut conn).await;
        let mut writer = create_writer(
            &files,
            &keys,
            SuccessorPlan::default(),
            &device(),
            DEVICE_ID,
            NOW_MS,
        )
        .expect("create writer");

        let mut outbox = Outbox::load(&files.outbox_path()).expect("load");
        let enroll = enroll_proposal("device-phone", 30, NOW_MS);
        let remove_anchor = propose(
            ctx.space,
            &anchor_keys().admin,
            remove_entry(DEVICE_ID),
            NOW_MS,
        )
        .expect("sign removal");
        outbox.add(enroll.clone()).expect("add enroll");
        outbox.add(remove_anchor.clone()).expect("add removal");

        let sequenced = authority::sequence_outbox(
            &engine,
            &mut conn,
            &ctx,
            &mut writer.local(),
            &mut outbox,
            NOW_MS,
        )
        .await
        .expect("sequence outbox");
        assert_eq!(sequenced, 1);
        let revision = engine
            .proposal_revision(&mut conn, &ctx, &enroll.hash())
            .await
            .expect("read revision")
            .expect("the chain holds the enrollment");
        assert_eq!(outbox.entries().len(), 1, "the held proposal is dropped");
        assert_eq!(outbox.entries()[0].proposal, remove_anchor);
        assert_eq!(
            outbox.entries()[0].rejection,
            Some(ProposalRejection::NamesAnchor)
        );
        assert!(outbox.pending().is_empty());

        // The writer record names the last sequenced entry before it was signed.
        let record = read_record(&files.record_path())
            .expect("read record")
            .expect("record");
        let sequenced_entry = record.sequenced.expect("sequenced entry");
        assert_eq!(sequenced_entry.revision, revision);
        assert!(sequenced_entry.hash().is_some());

        // Replaying a sequenced proposal returns its revision without sealing again.
        let replayed = authority::sequence(
            &engine,
            &mut conn,
            &ctx,
            &mut writer.local(),
            &enroll,
            NOW_MS,
        )
        .await
        .expect("replay");
        assert_eq!(replayed, Ok(revision));
        let again = read_record(&files.record_path())
            .expect("read record")
            .expect("record");
        assert_eq!(again.sequenced.map(|entry| entry.revision), Some(revision));
        drop(conn);
        registry.close_all().await.expect("close pools");
    });
}

#[test]
fn the_anchor_removes_an_enrolled_device_once() {
    block_on(async {
        let directory = TestDirectory::new("outbox-removal");
        let registry = DatabasePoolRegistry::default();
        let engine = Engine::vault();
        let files = directory.files();
        let keys = MemoryKeyStore::default();
        let pool = create_vault(&registry, &directory.path().join("vault.sqlite")).await;
        let mut conn = acquire(&pool).await;
        let ctx = context(&mut conn).await;
        let mut writer = create_writer(
            &files,
            &keys,
            SuccessorPlan::default(),
            &device(),
            DEVICE_ID,
            NOW_MS,
        )
        .expect("create writer");
        let admin = anchor_keys().admin;
        let enrolled = authority::enroll(
            &engine,
            &mut conn,
            &ctx,
            &admin,
            &mut writer.local(),
            enrollment("device-phone", 30),
            NOW_MS,
        )
        .await
        .expect("enroll");
        let removed = authority::remove(
            &engine,
            &mut conn,
            &ctx,
            &admin,
            &mut writer.local(),
            "device-phone",
            NOW_MS + 1,
        )
        .await
        .expect("remove");
        assert!(removed > enrolled);
        let view = engine
            .authorization(&mut conn, &ctx)
            .await
            .expect("read authorization")
            .expect("charter");
        let phone = view
            .devices
            .iter()
            .find(|device| device.device_id == "device-phone")
            .expect("phone");
        assert_eq!(phone.removed_revision, Some(removed));
        let error = authority::remove(
            &engine,
            &mut conn,
            &ctx,
            &admin,
            &mut writer.local(),
            "device-phone",
            NOW_MS + 2,
        )
        .await
        .expect_err("a removed device cannot be removed again");
        assert!(error.contains("device_removed"), "{error}");
        drop(conn);
        registry.close_all().await.expect("close pools");
    });
}

#[test]
fn reconciliation_proposes_each_membership_change_once() {
    let directory = TestDirectory::new("outbox-reconcile");
    let path = directory.files().outbox_path();
    let mut outbox = Outbox::load(&path).expect("empty outbox");
    let admin = anchor_keys().admin;
    let space = SpaceId::personal(VAULT_ID);
    let changes = || authority::Reconciliation {
        enroll: vec![enrollment("device-phone", 40)],
        remove: vec!["device-tablet".to_string()],
    };

    let added = authority::queue_reconciliation(&mut outbox, space, &admin, changes(), NOW_MS)
        .expect("queue changes");
    assert_eq!(added, 2);
    // A later pass signs at a new time, which is a new proposal hash, yet proposes nothing.
    let again = authority::queue_reconciliation(&mut outbox, space, &admin, changes(), NOW_MS + 1)
        .expect("queue again");
    assert_eq!(again, 0);

    // A finally rejected change is never proposed again, even with a new label.
    let enroll = outbox
        .find(|entry| *entry == enroll_entry(enrollment("device-phone", 40)))
        .expect("enrollment queued")
        .proposal
        .hash();
    outbox
        .settle(&enroll, Err(ProposalRejection::DeviceRemoved))
        .expect("settle final");
    let mut relabeled = enrollment("device-phone", 40);
    relabeled.label = "renamed phone".to_string();
    let relinked = authority::queue_reconciliation(
        &mut outbox,
        space,
        &admin,
        authority::Reconciliation {
            enroll: vec![relabeled, enrollment("device-laptop", 41)],
            remove: Vec::new(),
        },
        NOW_MS + 2,
    )
    .expect("queue relinked");
    assert_eq!(relinked, 1, "only the new device is proposed");

    let reloaded = Outbox::load(&path).expect("reload");
    assert_eq!(reloaded.entries().len(), 3);
    assert!(
        reloaded
            .find(|entry| *entry == remove_entry("device-tablet"))
            .is_some_and(|entry| entry.pending())
    );
}
