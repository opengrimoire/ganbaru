//! A planned handover from the hub to a promoted desktop, and the proposals the hub holds for
//! an anchor it no longer is.

use super::{
    DirectTransport, HUB_DEVICE, PHONE_DEVICE, Replica, TABLET_DEVICE, block_on, contact_source,
    device_key, hub_keys, open_hub, phone_ids, test_contact,
};
use crate::authority::{self, Enrollment};
use crate::client::{self, PeerIds};
use crate::hub::{HubForwards, SyncHub};
use crate::outbox::{MAX_OUTBOX_PROPOSALS, Outbox};
use crate::tests::{NOW_MS, VAULT_ID, acquire};
use crate::writer::{
    SequencedCheck, SyncKey, SyncKeyStore, WriterCheck, check_writer, create_writer,
    read_stored_writer,
};
use ganbaru_sync::Engine;
use ganbaru_sync_contracts::RecoverySeed;
use ganbaru_sync_contracts::{
    AdminKeyPair, DeviceKind, Digest32, ProposalRejection, SignedProposal, SpaceId,
};
use std::sync::Arc;
use tokio::sync::watch;

const LAPTOP_DEVICE: &str = "device-laptop";

fn laptop_ids() -> PeerIds {
    PeerIds {
        vault_id: VAULT_ID.to_string(),
        device_id: LAPTOP_DEVICE.to_string(),
    }
}

/// A hub whose service answers forward requests from the outbox of `replica`, as the app does.
fn forwarding_hub(replica: &Replica) -> DirectTransport {
    let (pool, ctx) = (replica.pool.clone(), replica.ctx.clone());
    let path = replica.directory.files().outbox_path();
    let forwards: HubForwards = Arc::new(move |requester, settled| {
        let (pool, ctx, path) = (pool.clone(), ctx.clone(), path.clone());
        Box::pin(async move {
            let mut outbox = Outbox::load(&path)?;
            let mut conn = acquire(&pool).await;
            authority::collect_forwards(
                &Engine::vault(),
                &mut conn,
                &ctx,
                &mut outbox,
                &requester,
                &settled,
            )
            .await
        })
    });
    DirectTransport(
        SyncHub::new(open_hub(replica), watch::Sender::new(0))
            .with_contact(contact_source(test_contact()))
            .with_forwards(forwards),
    )
}

/// Sequences a proposal through the writer of `replica`, which must anchor, and records the
/// commit.
async fn sequence_here(replica: &mut Replica, proposal: &SignedProposal) -> u64 {
    let mut conn = acquire(&replica.pool).await;
    let revision = authority::sequence(
        &replica.engine,
        &mut conn,
        &replica.ctx,
        &mut replica.writer.local(),
        proposal,
        NOW_MS,
    )
    .await
    .expect("sequence")
    .expect("the anchor accepts the proposal");
    drop(conn);
    commit_sequenced(replica).await;
    revision
}

/// Records the stored head of the replica's writer after sequencing through it.
async fn commit_sequenced(replica: &mut Replica) {
    let seq = replica.stored().await.get(&replica.writer.id());
    replica.writer.committed(seq).expect("record commit");
}

fn enroll_mobile(space: SpaceId, charter: Digest32, device_id: &str, seed: u8) -> SignedProposal {
    authority::propose(
        space,
        charter,
        &hub_keys().admin,
        authority::enroll_entry(Enrollment {
            device_id: device_id.to_string(),
            device_key: device_key(device_id).public_key(),
            tls_fingerprint: [seed; 32],
            label: device_id.to_string(),
            device_kind: DeviceKind::Mobile,
        }),
        NOW_MS + 1,
    )
    .expect("sign enrollment")
}

async fn hold(replica: &Replica, proposal: &SignedProposal) -> Result<u64, ProposalRejection> {
    let mut outbox = Outbox::load(&replica.directory.files().outbox_path()).expect("outbox");
    let mut conn = acquire(&replica.pool).await;
    authority::hold_for_anchor(
        &replica.engine,
        &mut conn,
        &replica.ctx,
        &mut outbox,
        proposal.clone(),
    )
    .await
    .expect("hold")
}

#[test]
fn a_promoted_desktop_takes_the_anchor_and_collects_what_the_hub_holds() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-handover", HUB_DEVICE).await;
        let mut laptop = Replica::new("laptop-handover", LAPTOP_DEVICE).await;
        let laptop_admin = AdminKeyPair::from_seed(&[29; 32]).expect("admin seed");
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;

        // The anchor enrolls the laptop and promotes it with the admin key it sent at pairing.
        let enroll = authority::propose(
            space,
            charter,
            &hub_keys().admin,
            authority::enroll_entry(Enrollment {
                device_id: LAPTOP_DEVICE.to_string(),
                device_key: device_key(LAPTOP_DEVICE).public_key(),
                tls_fingerprint: [27; 32],
                label: "Laptop".to_string(),
                device_kind: DeviceKind::Desktop,
            }),
            NOW_MS,
        )
        .expect("sign enrollment");
        sequence_here(&mut hub_replica, &enroll).await;
        let promote = authority::propose(
            space,
            charter,
            &hub_keys().admin,
            authority::promote_entry(LAPTOP_DEVICE, laptop_admin.public_key()),
            NOW_MS,
        )
        .expect("sign promotion");
        sequence_here(&mut hub_replica, &promote).await;

        let transport = forwarding_hub(&hub_replica);
        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("pull the log");
        laptop.apply().await;
        let stored = laptop.stored().await;
        let mut conn = acquire(&laptop.pool).await;
        let view = laptop
            .engine
            .authorization(&mut conn, &laptop.ctx)
            .await
            .expect("read authorization")
            .expect("charter");
        assert_eq!(view.anchor_device_id, HUB_DEVICE);
        assert!(authority::is_admin(
            &view,
            LAPTOP_DEVICE,
            &laptop_admin.public_key()
        ));
        assert!(
            !authority::is_admin(&view, PHONE_DEVICE, &laptop_admin.public_key()),
            "a member holds no admin role"
        );
        let handover = authority::seal_handover_here(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &laptop_admin,
            &mut laptop.writer.local(),
            Some(stored),
            NOW_MS,
        )
        .await
        .expect("seal handover")
        .expect("the promoted desktop takes the anchor");
        drop(conn);
        commit_sequenced(&mut laptop).await;
        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("push the handover");
        hub_replica.apply().await;
        let hub_view = {
            let mut conn = acquire(&hub_replica.pool).await;
            hub_replica
                .engine
                .authorization(&mut conn, &hub_replica.ctx)
                .await
                .expect("read authorization")
                .expect("charter")
        };
        assert_eq!(
            (
                hub_view.anchor_device_id.as_str(),
                hub_view.epoch,
                hub_view.revision
            ),
            (LAPTOP_DEVICE, 1, handover)
        );
        // The admin hub still reconciles links, but never proposes removing itself or the
        // anchor, which it does not link to as a peer.
        assert_eq!(
            authority::unlinked_devices(&hub_view, HUB_DEVICE, |device| device == PHONE_DEVICE),
            vec![TABLET_DEVICE.to_string()]
        );

        // The hub no longer anchors: it holds what devices propose for the laptop to collect.
        let wearable = enroll_mobile(space, charter, "device-watch", 40);
        let duplicate = enroll_mobile(space, charter, PHONE_DEVICE, 41);
        for proposal in [&wearable, &duplicate] {
            assert_eq!(
                hold(&hub_replica, proposal).await,
                Err(ProposalRejection::NotAnchor)
            );
        }
        // Only the anchor collects them, and no one else can settle them on its behalf.
        assert!(
            client::forward(
                &transport,
                &phone_ids(),
                &[(wearable.hash(), ProposalRejection::UnknownDevice)]
            )
            .await
            .expect("forward")
            .is_empty()
        );
        let forwarded = client::forward(&transport, &laptop_ids(), &[])
            .await
            .expect("forward");
        assert_eq!(forwarded, vec![wearable.clone(), duplicate.clone()]);
        let mut conn = acquire(&laptop.pool).await;
        let rejected = authority::sequence_forwarded(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &mut laptop.writer.local(),
            &forwarded,
            NOW_MS,
        )
        .await
        .expect("sequence forwarded");
        drop(conn);
        commit_sequenced(&mut laptop).await;
        assert_eq!(
            rejected,
            vec![(duplicate.hash(), ProposalRejection::AlreadyEnrolled)]
        );

        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("push the entry");
        hub_replica.apply().await;
        assert!(
            client::forward(&transport, &laptop_ids(), &rejected)
                .await
                .expect("forward")
                .is_empty(),
            "the chain holds one proposal and the anchor rejected the other"
        );
        // The hub answers each sender from what it learned.
        assert!(
            hold(&hub_replica, &wearable)
                .await
                .is_ok_and(|revision| revision > handover)
        );
        assert_eq!(
            hold(&hub_replica, &duplicate).await,
            Err(ProposalRejection::AlreadyEnrolled)
        );
        hub_replica.close().await;
        laptop.close().await;
    });
}

#[test]
fn a_regenerated_recovery_kit_replaces_the_recovery_admin_key() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-recovery-kit", HUB_DEVICE).await;
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        let keys = hub_replica.directory.keys();
        let outbox_path = hub_replica.directory.files().outbox_path();
        let admin = hub_keys().admin;
        let regenerate = |outbox: &mut Outbox, view: &ganbaru_sync::AuthorizationView| {
            authority::regenerate_recovery_kit(&keys, VAULT_ID, outbox, view, space, &admin, NOW_MS)
        };

        // The founding kit waits unsaved: it may not be replaced, and saving it forgets it.
        keys.write(
            VAULT_ID,
            SyncKey::RecoverySeed,
            hub_keys().recovery.as_bytes(),
        )
        .expect("store the founding seed");
        let view = view_of(&hub_replica).await;
        let mut outbox = Outbox::load(&outbox_path).expect("outbox");
        assert!(regenerate(&mut outbox, &view).is_err());
        authority::forget_recovery_seed(&keys, VAULT_ID, &view).expect("save the founding kit");

        // A full outbox takes no proposal, and no seed replaces anything.
        for index in 0..MAX_OUTBOX_PROPOSALS {
            let device_id = format!("device-filler-{index}");
            outbox
                .add(enroll_mobile(space, charter, &device_id, 60))
                .expect("fill the outbox");
        }
        assert!(regenerate(&mut outbox, &view).is_err());
        assert_eq!(
            authority::pending_recovery_seed(&keys, VAULT_ID).expect("pending seed"),
            None
        );
        Outbox::remove(&outbox_path).expect("empty the outbox");
        let mut outbox = Outbox::load(&outbox_path).expect("outbox");

        let seed = regenerate(&mut outbox, &view).expect("regenerate");
        assert_eq!(
            authority::pending_recovery_seed(&keys, VAULT_ID).expect("pending seed"),
            Some(seed.clone()),
            "the key store keeps the kit the user was shown"
        );
        assert_eq!(
            RecoverySeed::parse_kit_code(&seed.kit_code()).expect("parse kit code"),
            seed
        );
        assert!(
            regenerate(&mut outbox, &view).is_err(),
            "a kit whose proposal still waits is not replaced"
        );
        assert!(
            authority::forget_recovery_seed(&keys, VAULT_ID, &view).is_err(),
            "a kit the log does not name yet is kept"
        );
        {
            let mut conn = acquire(&hub_replica.pool).await;
            authority::sequence_outbox(
                &hub_replica.engine,
                &mut conn,
                &hub_replica.ctx,
                &mut hub_replica.writer.local(),
                &mut outbox,
                NOW_MS,
            )
            .await
            .expect("sequence the rotation");
        }
        commit_sequenced(&mut hub_replica).await;

        // Only the regenerated kit signs for the space now.
        let enroll = |admin: &AdminKeyPair, device_id: &str| {
            authority::propose(
                space,
                charter,
                admin,
                authority::enroll_entry(Enrollment {
                    device_id: device_id.to_string(),
                    device_key: device_key(device_id).public_key(),
                    tls_fingerprint: [42; 32],
                    label: device_id.to_string(),
                    device_kind: DeviceKind::Mobile,
                }),
                NOW_MS + 1,
            )
            .expect("sign enrollment")
        };
        let old_kit = hub_keys()
            .recovery
            .recovery_admin_key()
            .expect("old recovery key");
        let new_kit = seed.recovery_admin_key().expect("new recovery key");
        let mut conn = acquire(&hub_replica.pool).await;
        assert_eq!(
            authority::sequence(
                &hub_replica.engine,
                &mut conn,
                &hub_replica.ctx,
                &mut hub_replica.writer.local(),
                &enroll(&old_kit, "device-old-kit"),
                NOW_MS,
            )
            .await
            .expect("sequence"),
            Err(ProposalRejection::NotAdmin)
        );
        drop(conn);
        sequence_here(&mut hub_replica, &enroll(&new_kit, "device-new-kit")).await;
        let view = view_of(&hub_replica).await;
        authority::forget_recovery_seed(&keys, VAULT_ID, &view).expect("save the active kit");
        assert_eq!(
            authority::pending_recovery_seed(&keys, VAULT_ID).expect("pending seed"),
            None
        );
        hub_replica.close().await;
    });
}

#[test]
fn the_key_store_forgets_a_saved_recovery_kit_until_the_next_charter() {
    block_on(async {
        let replica = Replica::new("recovery-kit-store", HUB_DEVICE).await;
        let keys = replica.directory.keys();
        keys.write(
            VAULT_ID,
            SyncKey::RecoverySeed,
            hub_keys().recovery.as_bytes(),
        )
        .expect("store the founding seed");
        let founding = authority::ensure_anchor_keys(&keys, VAULT_ID).expect("anchor keys");
        assert_eq!(
            authority::pending_recovery_seed(&keys, VAULT_ID).expect("pending seed"),
            Some(founding.recovery.clone()),
            "the founding kit waits until the user saves it"
        );
        authority::forget_recovery_seed(&keys, VAULT_ID, &view_of(&replica).await)
            .expect("forget seed");
        assert_eq!(
            authority::pending_recovery_seed(&keys, VAULT_ID).expect("pending seed"),
            None
        );
        authority::ensure_admin_key(&keys, VAULT_ID).expect("admin key");
        assert_eq!(
            authority::pending_recovery_seed(&keys, VAULT_ID).expect("pending seed"),
            None,
            "signing as an admin never recreates the seed"
        );
        let next = authority::ensure_anchor_keys(&keys, VAULT_ID).expect("anchor keys");
        assert_ne!(
            next.recovery, founding.recovery,
            "a later charter founds with a fresh kit"
        );
        assert_eq!(
            next.admin.public_key(),
            founding.admin.public_key(),
            "the admin key survives"
        );
        replica.close().await;
    });
}

#[test]
fn a_recovery_kit_promotes_a_member_desktop_that_then_takes_the_anchor() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-kit-use", HUB_DEVICE).await;
        let mut laptop = Replica::new("laptop-kit-use", LAPTOP_DEVICE).await;
        let laptop_admin = AdminKeyPair::from_seed(&[31; 32]).expect("admin seed");
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        let enroll = authority::propose(
            space,
            charter,
            &hub_keys().admin,
            authority::enroll_entry(Enrollment {
                device_id: LAPTOP_DEVICE.to_string(),
                device_key: device_key(LAPTOP_DEVICE).public_key(),
                tls_fingerprint: [33; 32],
                label: "Laptop".to_string(),
                device_kind: DeviceKind::Desktop,
            }),
            NOW_MS,
        )
        .expect("sign enrollment");
        sequence_here(&mut hub_replica, &enroll).await;
        let transport = forwarding_hub(&hub_replica);
        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("pull the log");
        laptop.apply().await;
        let view = {
            let mut conn = acquire(&laptop.pool).await;
            laptop
                .engine
                .authorization(&mut conn, &laptop.ctx)
                .await
                .expect("read authorization")
                .expect("charter")
        };
        assert!(!authority::is_admin(
            &view,
            LAPTOP_DEVICE,
            &laptop_admin.public_key()
        ));

        // A kit the log does not name, or a mistyped code, signs nothing.
        let foreign = RecoverySeed::from_bytes([77; 32]).kit_code();
        assert!(
            authority::recovery_promotion(
                &view,
                space,
                &foreign,
                LAPTOP_DEVICE,
                laptop_admin.public_key(),
                NOW_MS,
            )
            .is_err()
        );
        let code = hub_keys().recovery.kit_code();
        assert!(
            authority::recovery_promotion(
                &view,
                space,
                &format!("{code}0"),
                LAPTOP_DEVICE,
                laptop_admin.public_key(),
                NOW_MS,
            )
            .is_err()
        );

        // The member desktop proposes its own promotion with the kit and the anchor sequences it.
        let promotion = authority::recovery_promotion(
            &view,
            space,
            &code,
            LAPTOP_DEVICE,
            laptop_admin.public_key(),
            NOW_MS,
        )
        .expect("sign with the kit");
        let outcome = client::propose(&transport, &laptop_ids(), &promotion)
            .await
            .expect("propose through the hub");
        assert_eq!(
            outcome,
            Err(ProposalRejection::NotAnchor),
            "the test hub has no anchor service, so the anchor sequences it directly"
        );
        sequence_here(&mut hub_replica, &promotion).await;
        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("pull the promotion");
        laptop.apply().await;
        let stored = laptop.stored().await;
        let mut conn = acquire(&laptop.pool).await;
        let view = laptop
            .engine
            .authorization(&mut conn, &laptop.ctx)
            .await
            .expect("read authorization")
            .expect("charter");
        assert!(authority::is_admin(
            &view,
            LAPTOP_DEVICE,
            &laptop_admin.public_key()
        ));
        authority::seal_handover_here(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &laptop_admin,
            &mut laptop.writer.local(),
            Some(stored),
            NOW_MS,
        )
        .await
        .expect("seal handover")
        .expect("the desktop the kit promoted takes the anchor");
        drop(conn);
        hub_replica.close().await;
        laptop.close().await;
    });
}

/// A hub that sequenced two enrollments its database then lost to a backup restore, with a
/// phone that pulled them before the restore. The hub runs the writer check on the restored
/// database and continues with the successor it plans.
async fn restored_hub(label: &str) -> (Replica, Replica) {
    let mut hub_replica = Replica::new(&format!("hub-{label}"), HUB_DEVICE).await;
    let mut phone = Replica::new(&format!("phone-{label}"), PHONE_DEVICE).await;
    let charter = hub_replica.charter().await;
    let space = hub_replica.ctx.space;
    sequence_here(
        &mut hub_replica,
        &enroll_mobile(space, charter, "device-watch", 50),
    )
    .await;
    let backup = hub_replica.directory.path().join("backup.sqlite");
    sqlx::query("VACUUM INTO ?")
        .bind(backup.to_string_lossy().into_owned())
        .execute(&hub_replica.pool)
        .await
        .expect("back up the hub");
    sequence_here(
        &mut hub_replica,
        &enroll_mobile(space, charter, "device-ring", 51),
    )
    .await;
    sequence_here(
        &mut hub_replica,
        &enroll_mobile(space, charter, "device-band", 52),
    )
    .await;
    phone
        .exchange(&forwarding_hub(&hub_replica), &phone_ids())
        .await
        .expect("pull the entries");
    phone.apply().await;

    hub_replica.pool = hub_replica
        .registry
        .connect_path(&backup)
        .await
        .expect("open the restored database");
    let files = hub_replica.directory.files();
    let keys = hub_replica.directory.keys();
    let stored = read_stored_writer(&files, &keys).expect("read stored writer");
    let check = {
        let mut conn = acquire(&hub_replica.pool).await;
        check_writer(
            &hub_replica.engine,
            &mut conn,
            &hub_replica.ctx,
            &files,
            &device_key(HUB_DEVICE).public_key(),
            stored,
        )
        .await
        .expect("check writer")
    };
    let WriterCheck::Successor(plan) = check else {
        panic!("the restored hub keeps no writer that sealed the lost entries");
    };
    let old_writer = hub_replica.writer.id();
    hub_replica.writer = create_writer(
        &files,
        &keys,
        plan,
        &device_key(HUB_DEVICE),
        HUB_DEVICE,
        NOW_MS,
    )
    .expect("create successor");
    assert_ne!(hub_replica.writer.id(), old_writer);
    let mut conn = acquire(&hub_replica.pool).await;
    assert!(matches!(
        authority::restore_check(
            &hub_replica.engine,
            &mut conn,
            &hub_replica.ctx,
            hub_replica.writer.record()
        )
        .await
        .expect("restore check"),
        SequencedCheck::Restored { .. }
    ));
    drop(conn);
    (hub_replica, phone)
}

async fn view_of(replica: &Replica) -> ganbaru_sync::AuthorizationView {
    let mut conn = acquire(&replica.pool).await;
    replica
        .engine
        .authorization(&mut conn, &replica.ctx)
        .await
        .expect("read authorization")
        .expect("charter")
}

#[test]
fn a_restored_anchor_takes_back_the_entries_members_return_within_its_window() {
    block_on(async {
        let (mut hub_replica, phone) = restored_hub("restore-returned").await;
        let lost = view_of(&phone).await;
        phone
            .exchange(&forwarding_hub(&hub_replica), &phone_ids())
            .await
            .expect("return the entries");
        hub_replica.apply().await;
        let mut conn = acquire(&hub_replica.pool).await;
        assert_eq!(
            authority::restore_check(
                &hub_replica.engine,
                &mut conn,
                &hub_replica.ctx,
                hub_replica.writer.record()
            )
            .await
            .expect("restore check"),
            SequencedCheck::Current,
            "the returned entries cover the high-water, so no handover is needed"
        );
        drop(conn);
        assert_eq!(view_of(&hub_replica).await, lost);
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_restored_anchor_hands_over_under_a_successor_that_members_accept() {
    block_on(async {
        let (mut hub_replica, mut phone) = restored_hub("restore-handover").await;
        let stored_before = phone.stored().await;
        let mut conn = acquire(&hub_replica.pool).await;
        let revision = authority::seal_handover_here(
            &hub_replica.engine,
            &mut conn,
            &hub_replica.ctx,
            &hub_keys().admin,
            &mut hub_replica.writer.local(),
            None,
            NOW_MS,
        )
        .await
        .expect("seal handover")
        .expect("the restored anchor hands over to itself");
        drop(conn);
        commit_sequenced(&mut hub_replica).await;

        // The phone returns the lost entries late and takes the handover; the successor never
        // sealed at a sequence the lost entries used, so nothing forks.
        phone
            .exchange(&forwarding_hub(&hub_replica), &phone_ids())
            .await
            .expect("exchange");
        hub_replica.apply().await;
        phone.apply().await;
        let (hub_view, phone_view) = (view_of(&hub_replica).await, view_of(&phone).await);
        assert_eq!(hub_view, phone_view);
        assert_eq!(
            (
                phone_view.epoch,
                phone_view.revision,
                phone_view.anchor_device_id.as_str()
            ),
            (1, revision, HUB_DEVICE)
        );
        assert_eq!(phone.stored().await, hub_replica.stored().await);
        assert!(
            phone.stored().await.dominates(&stored_before),
            "the phone keeps every operation it held"
        );
        let mut conn = acquire(&phone.pool).await;
        assert!(
            phone
                .engine
                .own_forks(&mut conn, &phone.ctx)
                .await
                .expect("own forks")
                .is_empty()
        );
        drop(conn);
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_proposal_the_old_anchor_sequenced_after_the_handover_point_is_sequenced_again() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-handover-race", HUB_DEVICE).await;
        let mut laptop = Replica::new("laptop-handover-race", LAPTOP_DEVICE).await;
        let laptop_admin = AdminKeyPair::from_seed(&[35; 32]).expect("admin seed");
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        let enroll = authority::propose(
            space,
            charter,
            &hub_keys().admin,
            authority::enroll_entry(Enrollment {
                device_id: LAPTOP_DEVICE.to_string(),
                device_key: device_key(LAPTOP_DEVICE).public_key(),
                tls_fingerprint: [36; 32],
                label: "Laptop".to_string(),
                device_kind: DeviceKind::Desktop,
            }),
            NOW_MS,
        )
        .expect("sign enrollment");
        sequence_here(&mut hub_replica, &enroll).await;
        let promote = authority::propose(
            space,
            charter,
            &hub_keys().admin,
            authority::promote_entry(LAPTOP_DEVICE, laptop_admin.public_key()),
            NOW_MS,
        )
        .expect("sign promotion");
        sequence_here(&mut hub_replica, &promote).await;
        let transport = forwarding_hub(&hub_replica);
        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("pull the log");
        laptop.apply().await;
        let stored = laptop.stored().await;

        // The old anchor sequences one more proposal from its outbox after the laptop's exchange.
        let late = enroll_mobile(space, charter, "device-late", 37);
        let outbox_path = hub_replica.directory.files().outbox_path();
        let mut outbox = Outbox::load(&outbox_path).expect("outbox");
        outbox.add(late.clone()).expect("queue");
        {
            let mut conn = acquire(&hub_replica.pool).await;
            authority::sequence_outbox(
                &hub_replica.engine,
                &mut conn,
                &hub_replica.ctx,
                &mut hub_replica.writer.local(),
                &mut outbox,
                NOW_MS,
            )
            .await
            .expect("sequence outbox");
        }
        commit_sequenced(&mut hub_replica).await;
        let held = Outbox::load(&outbox_path).expect("outbox");
        assert_eq!(held.entries().len(), 1, "the held proposal stays");
        assert!(held.pending().is_empty());

        // The laptop takes the anchor from the head it knew, superseding the late entry.
        let mut conn = acquire(&laptop.pool).await;
        authority::seal_handover_here(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &laptop_admin,
            &mut laptop.writer.local(),
            Some(stored),
            NOW_MS,
        )
        .await
        .expect("seal handover")
        .expect("the promoted desktop takes the anchor");
        drop(conn);
        commit_sequenced(&mut laptop).await;
        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("push the handover");
        hub_replica.apply().await;
        laptop.apply().await;
        assert!(
            !view_of(&laptop)
                .await
                .devices
                .iter()
                .any(|device| device.device_id == "device-late"),
            "the handover superseded the late entry"
        );

        // The hub forwards the superseded proposal, and the new anchor sequences it.
        let forwarded = client::forward(&transport, &laptop_ids(), &[])
            .await
            .expect("forward");
        assert_eq!(forwarded, vec![late]);
        let mut conn = acquire(&laptop.pool).await;
        let rejected = authority::sequence_forwarded(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &mut laptop.writer.local(),
            &forwarded,
            NOW_MS,
        )
        .await
        .expect("sequence forwarded");
        drop(conn);
        assert!(rejected.is_empty());
        commit_sequenced(&mut laptop).await;
        assert!(
            view_of(&laptop)
                .await
                .devices
                .iter()
                .any(|device| device.device_id == "device-late")
        );
        hub_replica.close().await;
        laptop.close().await;
    });
}
