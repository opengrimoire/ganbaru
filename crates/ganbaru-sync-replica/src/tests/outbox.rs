//! The proposal outbox and the anchor's sequencing of it.

use super::{
    DEVICE_ID, NOW_MS, TestDirectory, VAULT_ID, acquire, anchor_keys, block_on, check, context,
    create_vault, device,
};
use crate::authority::{self, Enrollment, enroll_entry, propose, remove_entry};
use crate::outbox::{MAX_OUTBOX_PROPOSALS, Outbox, is_final};
use crate::writer::{
    MemoryKeyStore, SelectedHead, SequencedCheck, SequencedEntry, SuccessorPlan, WriterCheck,
    create_writer, read_record, sequenced_check,
};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::Engine;
use ganbaru_sync_contracts::{
    DeviceKeyPair, DeviceKind, Digest32, ProposalRejection, SignedProposal, SpaceId,
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

/// Charter hash of proposals in tests that never sequence them.
const ANY_CHARTER: Digest32 = [0; 32];

fn enroll_proposal(device_id: &str, seed: u8, now_ms: u64) -> SignedProposal {
    enroll_proposal_under(ANY_CHARTER, device_id, seed, now_ms)
}

fn enroll_proposal_under(
    charter: Digest32,
    device_id: &str,
    seed: u8,
    now_ms: u64,
) -> SignedProposal {
    propose(
        SpaceId::personal(VAULT_ID),
        charter,
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
    assert!(!outbox.has_room());
    assert!(outbox.add(extra.clone()).is_err());

    // Full of waiting proposals, reconciliation leaves its changes for a later pass.
    let admin = anchor_keys().admin;
    let space = SpaceId::personal(VAULT_ID);
    let deferred = authority::queue_reconciliation(
        &mut outbox,
        space,
        ANY_CHARTER,
        &admin,
        authority::Reconciliation {
            enroll: vec![enrollment("device-laptop", 41)],
            remove: vec!["device-tablet".to_string()],
        },
        NOW_MS + 20_000,
    )
    .expect("a full outbox does not fail the pass");
    assert_eq!(deferred, 0);

    // Final rejections make room, oldest first, and waiting proposals stay.
    let oldest = outbox.entries()[0].proposal.hash();
    let later = outbox.entries()[1].proposal.hash();
    outbox
        .settle(&later, Err(ProposalRejection::DeviceRemoved))
        .expect("settle later");
    outbox
        .settle(&oldest, Err(ProposalRejection::DeviceRemoved))
        .expect("settle oldest");
    assert!(outbox.has_room());
    assert!(outbox.add(extra.clone()).expect("add over a rejection"));
    assert!(
        outbox
            .entries()
            .iter()
            .all(|entry| entry.proposal.hash() != oldest)
    );
    assert!(
        outbox
            .entries()
            .iter()
            .any(|entry| entry.proposal.hash() == later)
    );
    assert_eq!(outbox.entries().len(), MAX_OUTBOX_PROPOSALS);
    let reloaded = Outbox::load(&directory.files().outbox_path()).expect("reload");
    assert_eq!(reloaded.entries().len(), MAX_OUTBOX_PROPOSALS);
    assert!(
        reloaded
            .entries()
            .iter()
            .any(|entry| entry.proposal.hash() == extra.hash())
    );
}

#[test]
fn only_a_moved_chain_ends_a_waiting_proposal() {
    let waiting = [
        ProposalRejection::NotAnchor,
        ProposalRejection::EpochFrozen,
        ProposalRejection::HandoverConflict,
        ProposalRejection::CatchingUp,
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
        let charter = authority::charter_hash(&engine, &mut conn, &ctx)
            .await
            .expect("charter");
        let enroll = enroll_proposal_under(charter, "device-phone", 30, NOW_MS);
        let remove_anchor = propose(
            ctx.space,
            charter,
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
        assert_eq!(outbox.entries().len(), 2);
        assert_eq!(
            (
                outbox.entries()[0].proposal.clone(),
                outbox.entries()[0].held
            ),
            (enroll.clone(), Some(0)),
            "the held proposal stays until a later epoch keeps it"
        );
        assert_eq!(outbox.entries()[1].proposal, remove_anchor);
        assert_eq!(
            outbox.entries()[1].rejection,
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
        assert_eq!(sequenced_entry.charter(), Some(charter));
        assert_eq!(sequenced_entry.epoch, 0);
        assert_eq!(
            sequenced_check(
                Some(&sequenced_entry),
                SelectedHead {
                    charter,
                    epoch: 0,
                    revision
                }
            ),
            SequencedCheck::Current
        );

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
fn a_record_one_entry_ahead_is_a_rolled_back_seal_and_further_ahead_is_a_restore() {
    let at = |epoch: u32, revision: u64| SequencedEntry {
        charter: URL_SAFE_NO_PAD.encode([1; 32]),
        epoch,
        revision,
        entry_hash: URL_SAFE_NO_PAD.encode([2; 32]),
        restored: false,
    };
    let head = |epoch: u32, revision: u64| SelectedHead {
        charter: [1; 32],
        epoch,
        revision,
    };
    assert_eq!(sequenced_check(None, head(0, 7)), SequencedCheck::Current);
    assert_eq!(
        sequenced_check(Some(&at(0, 7)), head(0, 7)),
        SequencedCheck::Current
    );
    assert_eq!(
        sequenced_check(Some(&at(0, 5)), head(1, 7)),
        SequencedCheck::Current,
        "a log another anchor extended is not a restore"
    );
    assert_eq!(
        sequenced_check(Some(&at(0, 9)), head(1, 7)),
        SequencedCheck::Current,
        "entries a handover superseded past its starting revision were not lost"
    );
    assert_eq!(
        sequenced_check(
            Some(&SequencedEntry {
                restored: true,
                ..at(0, 8)
            }),
            head(1, 7)
        ),
        SequencedCheck::Current
    );
    assert_eq!(
        sequenced_check(Some(&at(0, 8)), head(0, 7)),
        SequencedCheck::RolledBack
    );
    let restored = SequencedEntry {
        restored: true,
        ..at(0, 8)
    };
    assert_eq!(
        sequenced_check(Some(&restored), head(0, 7)),
        SequencedCheck::Restored { recorded: 8 },
        "one entry ahead after the writer lost part of its chain is a restore"
    );
    assert_eq!(
        sequenced_check(Some(&at(0, 9)), head(0, 7)),
        SequencedCheck::Restored { recorded: 9 }
    );
    assert_eq!(
        sequenced_check(Some(&at(2, 6)), head(1, 7)),
        SequencedCheck::Restored { recorded: 6 },
        "a database that lost an epoch this installation sealed was restored"
    );
    assert_eq!(
        sequenced_check(
            Some(&at(0, 9)),
            SelectedHead {
                charter: [3; 32],
                epoch: 0,
                revision: 1
            }
        ),
        SequencedCheck::Current,
        "a record of a charter a Reset cleared is not a restore"
    );
}

#[test]
fn a_restored_anchor_sees_its_lost_entries_and_hands_over_to_itself() {
    block_on(async {
        let directory = TestDirectory::new("outbox-restore");
        let registry = DatabasePoolRegistry::default();
        let engine = Engine::vault();
        let files = directory.files();
        let keys = directory.keys();
        let admin = anchor_keys().admin;
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
        for (index, (device_id, seed)) in [
            ("device-phone", 30),
            ("device-tablet", 31),
            ("device-watch", 32),
        ]
        .into_iter()
        .enumerate()
        {
            if index == 1 {
                let backup = directory.path().join("backup.sqlite");
                sqlx::query("VACUUM INTO ?")
                    .bind(backup.to_string_lossy().into_owned())
                    .execute(&mut *conn)
                    .await
                    .expect("back up the vault");
            }
            authority::enroll(
                &engine,
                &mut conn,
                &ctx,
                &admin,
                &mut writer.local(),
                enrollment(device_id, seed),
                NOW_MS,
            )
            .await
            .expect("enroll");
        }
        assert_eq!(
            authority::restore_check(&engine, &mut conn, &ctx, writer.record())
                .await
                .expect("check"),
            SequencedCheck::Current
        );
        drop(writer);
        drop(conn);

        // The backup holds revision 1; the record names revision 3.
        let restored = registry
            .connect_path(&directory.path().join("backup.sqlite"))
            .await
            .expect("open backup");
        let mut conn = acquire(&restored).await;
        // No commit was recorded after sequencing, so the backup's shorter chain still matches
        // the record; the high-water still retires the writer, because members may hold the
        // lost entries at its later sequences.
        let WriterCheck::Successor(plan) = check(&engine, &mut conn, &ctx, &directory).await else {
            panic!("a database behind the high-water keeps no writer");
        };
        assert!(plan.sequenced().is_some_and(|entry| entry.restored));
        let old_writer = read_record(&files.record_path())
            .expect("read record")
            .expect("record")
            .writer_id;
        let mut writer = create_writer(&files, &keys, plan, &device(), DEVICE_ID, NOW_MS)
            .expect("create successor");
        assert_ne!(writer.id().to_hex(), old_writer);
        assert_eq!(
            authority::restore_check(&engine, &mut conn, &ctx, writer.record())
                .await
                .expect("check"),
            SequencedCheck::Restored { recorded: 3 }
        );

        let revision = authority::seal_handover_here(
            &engine,
            &mut conn,
            &ctx,
            &admin,
            &mut writer.local(),
            None,
            NOW_MS,
        )
        .await
        .expect("seal handover")
        .expect("the anchor hands over to itself");
        assert_eq!(revision, 2);
        let view = engine
            .authorization(&mut conn, &ctx)
            .await
            .expect("read authorization")
            .expect("charter");
        assert_eq!((view.epoch, view.anchor_device_id.as_str()), (1, DEVICE_ID));
        assert_eq!(
            authority::restore_check(&engine, &mut conn, &ctx, writer.record())
                .await
                .expect("check"),
            SequencedCheck::Current,
            "the handover opened an epoch the lost entries never reached"
        );

        // The new epoch outranks the lost one, and sequencing continues in it.
        let enrolled = authority::enroll(
            &engine,
            &mut conn,
            &ctx,
            &admin,
            &mut writer.local(),
            enrollment("device-tablet", 31),
            NOW_MS,
        )
        .await
        .expect("enroll again");
        assert_eq!(enrolled, 3);
        drop(conn);
        registry.close_all().await.expect("close pools");
    });
}

#[test]
fn a_restore_that_loses_one_recorded_entry_is_not_a_rolled_back_seal() {
    block_on(async {
        let directory = TestDirectory::new("outbox-restore-one");
        let registry = DatabasePoolRegistry::default();
        let engine = Engine::vault();
        let files = directory.files();
        let keys = directory.keys();
        let admin = anchor_keys().admin;
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
        for (index, (device_id, seed)) in [("device-phone", 40), ("device-tablet", 41)]
            .into_iter()
            .enumerate()
        {
            if index == 1 {
                sqlx::query("VACUUM INTO ?")
                    .bind(
                        directory
                            .path()
                            .join("backup.sqlite")
                            .to_string_lossy()
                            .into_owned(),
                    )
                    .execute(&mut *conn)
                    .await
                    .expect("back up the vault");
            }
            authority::enroll(
                &engine,
                &mut conn,
                &ctx,
                &admin,
                &mut writer.local(),
                enrollment(device_id, seed),
                NOW_MS,
            )
            .await
            .expect("enroll");
            // The service records the commit after every sequencing step.
            let seq = engine
                .stored_vector(&mut conn, &ctx)
                .await
                .expect("stored vector")
                .get(&writer.id());
            writer.committed(seq).expect("record commit");
        }
        drop(writer);
        drop(conn);

        // The backup lacks only revision 2, which a rolled back seal would look like, but the
        // writer's recorded chain is gone too.
        let restored = registry
            .connect_path(&directory.path().join("backup.sqlite"))
            .await
            .expect("open backup");
        let mut conn = acquire(&restored).await;
        let WriterCheck::Successor(plan) = check(&engine, &mut conn, &ctx, &directory).await else {
            panic!("the restored database lost the writer's committed chain");
        };
        let writer = create_writer(&files, &keys, plan, &device(), DEVICE_ID, NOW_MS)
            .expect("create successor");
        assert_eq!(
            authority::restore_check(&engine, &mut conn, &ctx, writer.record())
                .await
                .expect("check"),
            SequencedCheck::Restored { recorded: 2 }
        );
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

    let added =
        authority::queue_reconciliation(&mut outbox, space, ANY_CHARTER, &admin, changes(), NOW_MS)
            .expect("queue changes");
    assert_eq!(added, 2);
    // A later pass signs at a new time, which is a new proposal hash, yet proposes nothing.
    let again = authority::queue_reconciliation(
        &mut outbox,
        space,
        ANY_CHARTER,
        &admin,
        changes(),
        NOW_MS + 1,
    )
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
        ANY_CHARTER,
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
