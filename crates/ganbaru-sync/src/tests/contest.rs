//! Yields and objections the local device seals into a contested epoch instance.

use ganbaru_sync_contracts::{DeviceKeyPair, DeviceRole, Digest32, WriterId};

use super::authorization::{admin_entry, genesis_of, head, tag, view};
use super::block_on;
use super::replica::{
    ANCHOR_DEVICE_ID, Identity, Replica, anchor_admin, device_admin, device_id, enroll_entry,
    person,
};
use crate::{ContestRejection, StoreOutcome, StoreRefusal, WriterRetirement};

/// A hub anchor with admins 20, 21, and 22, and a replica of each admin that stores the log.
async fn admins(seed: u64) -> (Replica, Replica, Replica, Replica) {
    let mut hub = Replica::anchor(0, seed).await;
    for index in [20, 21, 22] {
        hub.sequence(admin_entry(index)).await.unwrap().unwrap();
    }
    let mut first = Replica::new(20, seed).await;
    let mut second = Replica::new(21, seed).await;
    let mut third = Replica::new(22, seed).await;
    for replica in [&mut first, &mut second, &mut third] {
        replica.pull(&mut hub).await;
    }
    (hub, first, second, third)
}

/// `replica` takes the anchor with its own admin key and returns the handover's entry hash.
async fn take(replica: &mut Replica, index: usize) -> Digest32 {
    let signed = replica.handover_proposal(&device_admin(index)).await;
    replica.seal_handover(&signed, None).await.unwrap().unwrap();
    head(replica).await.2
}

#[test]
fn a_contestant_yield_resolves_two_takes_and_is_refused_where_it_changes_nothing() {
    block_on(async {
        let (mut hub, mut first, mut second, mut third) = admins(70).await;
        let (_, point, _) = head(&mut hub).await;
        let to_first = take(&mut first, 20).await;
        let to_second = take(&mut second, 21).await;
        hub.pull(&mut first).await;
        hub.pull(&mut second).await;
        assert_eq!(view(&mut hub).await.frozen_at, Some(point));
        second.pull(&mut hub).await;
        third.pull(&mut hub).await;

        assert_eq!(
            second
                .seal_yield(&device_admin(21), &[9; 32])
                .await
                .unwrap(),
            Err(ContestRejection::UnknownCandidate)
        );
        assert_eq!(
            third
                .seal_yield(&device_admin(22), &to_first)
                .await
                .unwrap(),
            Err(ContestRejection::NotContestant)
        );
        // A contestant yields from its own instance, past the selected head.
        second
            .seal_yield(&device_admin(21), &to_first)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            second
                .seal_yield(&device_admin(21), &to_first)
                .await
                .unwrap(),
            Err(ContestRejection::NotFrozen)
        );
        hub.pull(&mut second).await;
        let resolved = view(&mut hub).await;
        assert_eq!(
            (resolved.anchor_device_id.as_str(), resolved.frozen_at),
            (device_id(20).as_str(), None)
        );
        first.pull(&mut hub).await;
        assert_eq!(view(&mut first).await, resolved);
        assert_eq!(
            first
                .seal_yield(&device_admin(20), &to_second)
                .await
                .unwrap(),
            Err(ContestRejection::NotFrozen)
        );
    });
}

#[test]
fn a_yield_already_stored_is_refused_while_the_instance_stays_frozen() {
    block_on(async {
        let (mut hub, mut first, mut second, mut third) = admins(71).await;
        let to_first = take(&mut first, 20).await;
        take(&mut second, 21).await;
        let to_third = take(&mut third, 22).await;
        for replica in [&mut first, &mut second, &mut third] {
            hub.pull(replica).await;
        }
        second.pull(&mut hub).await;
        // One of two other contestants yielding leaves the instance frozen.
        second
            .seal_yield(&device_admin(21), &to_first)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            second
                .seal_yield(&device_admin(21), &to_first)
                .await
                .unwrap(),
            Err(ContestRejection::AlreadyYielded)
        );
        hub.pull(&mut second).await;
        assert!(view(&mut hub).await.frozen_at.is_some());
        third.pull(&mut hub).await;
        third
            .seal_yield(&device_admin(22), &to_first)
            .await
            .unwrap()
            .unwrap();
        hub.pull(&mut third).await;
        let resolved = view(&mut hub).await;
        assert_eq!(
            (resolved.anchor_device_id.as_str(), resolved.frozen_at),
            (device_id(20).as_str(), None)
        );
        assert_ne!(to_first, to_third);
    });
}

#[test]
fn the_anchor_objects_to_a_lone_take_and_its_yield_withdraws_the_objection() {
    block_on(async {
        let (mut hub, mut first, mut second, _) = admins(72).await;
        let (_, point, _) = head(&mut hub).await;
        let taken = take(&mut first, 20).await;
        hub.pull(&mut first).await;
        assert_eq!(
            view(&mut hub).await.anchor_device_id,
            device_id(20),
            "a lone take wins until the anchor objects"
        );
        second.pull(&mut hub).await;
        assert_eq!(
            second
                .seal_objection(&device_admin(21), &taken)
                .await
                .unwrap(),
            Err(ContestRejection::NotAnchor)
        );
        assert_eq!(
            hub.seal_objection(&anchor_admin(&person()), &[9; 32])
                .await
                .unwrap(),
            Err(ContestRejection::NotCandidate)
        );

        // The anchor removes the target past the take's parent, which contests the instance.
        hub.seal_objection(&anchor_admin(&person()), &taken)
            .await
            .unwrap()
            .unwrap();
        let objected = view(&mut hub).await;
        assert_eq!(
            (objected.anchor_device_id.as_str(), objected.frozen_at),
            (ANCHOR_DEVICE_ID, Some(point))
        );
        assert_eq!(
            hub.seal_objection(&anchor_admin(&person()), &taken)
                .await
                .unwrap(),
            Err(ContestRejection::AlreadyObjected)
        );
        first.pull(&mut hub).await;
        assert_eq!(view(&mut first).await, objected);

        // The anchor's own yield withdraws its objection, and the take wins after all.
        hub.seal_yield(&anchor_admin(&person()), &taken)
            .await
            .unwrap()
            .unwrap();
        let resolved = view(&mut hub).await;
        assert_eq!(
            (resolved.anchor_device_id.as_str(), resolved.frozen_at),
            (device_id(20).as_str(), None)
        );
        assert_eq!(
            hub.seal_objection(&anchor_admin(&person()), &taken)
                .await
                .unwrap(),
            Err(ContestRejection::Resolved)
        );
        first.pull(&mut hub).await;
        assert_eq!(view(&mut first).await, resolved);
    });
}

/// The retirement the engine reports for `writer` on `replica`.
async fn retirement(replica: &mut Replica, writer: WriterId) -> Option<WriterRetirement> {
    replica
        .engine
        .writer_retirement(&mut replica.conn, &replica.ctx, writer)
        .await
        .unwrap()
}

#[test]
fn a_restored_anchor_whose_writer_forked_objects_and_yields_through_a_fresh_writer_without_the_kit()
{
    block_on(async {
        let (mut hub, mut first, _, _) = admins(73).await;
        let base = view(&mut hub).await.revision;
        // A restored copy of the anchor seals with the same writer as the anchor.
        let mut copy = hub.clone_device().await;
        hub.sequence(enroll_entry(&person(), 23, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        copy.sequence(enroll_entry(&person(), 24, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retirement(&mut hub, copy.writer()).await, None);
        let forked = hub.writer();
        let device_key = hub.identity.certificate.certificate.device_key;
        let twins = copy.own_ops().await;
        hub.receive(&twins).await;
        assert_eq!(
            retirement(&mut hub, forked).await,
            Some(WriterRetirement::Forked)
        );

        // Applying retires the forked writer: the unchanged device key certifies a successor.
        hub.apply_all().await.unwrap();
        assert_eq!(view(&mut hub).await.frozen_at, Some(base));
        let fresh = &hub.identity.certificate.certificate;
        assert_ne!(fresh.writer_id(), forked);
        assert_eq!(fresh.device_key, device_key);
        assert!(fresh.predecessor.is_some());
        let current = hub.writer();
        assert_eq!(retirement(&mut hub, current).await, None);

        // An admin takes the frozen instance at its freeze node; the anchor objects to it and
        // then yields to it, both through the fresh writer.
        first.pull(&mut hub).await;
        let taken = take(&mut first, 20).await;
        hub.pull(&mut first).await;
        hub.seal_objection(&anchor_admin(&person()), &taken)
            .await
            .unwrap()
            .unwrap();
        let objected = view(&mut hub).await;
        assert_eq!(
            (objected.anchor_device_id.as_str(), objected.frozen_at),
            (ANCHOR_DEVICE_ID, Some(base))
        );
        hub.seal_yield(&anchor_admin(&person()), &taken)
            .await
            .unwrap()
            .unwrap();
        let resolved = view(&mut hub).await;
        assert_eq!(
            (resolved.anchor_device_id.as_str(), resolved.frozen_at),
            (device_id(20).as_str(), None)
        );
        assert_eq!(resolved.kit_admin_key, objected.kit_admin_key);
        first.pull(&mut hub).await;
        assert_eq!(view(&mut first).await, resolved);
    });
}

#[test]
fn a_leaked_writer_key_alone_cannot_certify_a_fresh_writer_for_its_device() {
    block_on(async {
        let (mut hub, mut first, _, _) = admins(74).await;
        let before = view(&mut first).await;
        let leaked = hub.identity.seed;
        let forked = hub.writer();
        let genesis = hub.own_ops().await.swap_remove(0);
        let thief_key = DeviceKeyPair::from_seed(&leaked).unwrap();

        // A successor the leaked key certifies for itself names a device key the chain never
        // enrolled the anchor with: its certificate is stored but binds no writer.
        let thief = Identity::new([77; 32], &thief_key, ANCHOR_DEVICE_ID, Some(forked));
        assert_eq!(
            first.receive(&[genesis_of(&genesis, &thief)]).await,
            vec![StoreOutcome::Refused(StoreRefusal::Untrusted)]
        );

        // One that names the anchor's real device key does not verify under it, so the record
        // store discards it.
        let mut forger = Identity::new([78; 32], &thief_key, ANCHOR_DEVICE_ID, Some(forked));
        forger.certificate.certificate.device_key = hub.identity.certificate.certificate.device_key;
        let refused = first.receive(&[genesis_of(&genesis, &forger)]).await;
        assert!(matches!(refused[..], [StoreOutcome::Refused(_)]));

        first.apply_all().await.unwrap();
        assert_eq!(view(&mut first).await, before);
        for (identity, certificates) in [(&thief, 1), (&forger, 0)] {
            let writer = identity.id().to_hex().to_uppercase();
            let stored = first
                .count(&format!(
                    "SELECT COUNT(*) FROM sync_auth_envelopes
                     WHERE role = 'certificate' AND hex(writer_id) = '{writer}'"
                ))
                .await;
            let writers = first
                .count(&format!(
                    "SELECT COUNT(*) FROM sync_writers WHERE hex(writer_id) = '{writer}'"
                ))
                .await;
            assert_eq!((stored, writers), (certificates, 0));
        }
    });
}

#[test]
fn the_writer_of_a_removed_device_is_retired() {
    block_on(async {
        let mut hub = Replica::anchor(0, 75).await;
        let mut member = Replica::new(1, 75).await;
        member.pull(&mut hub).await;
        tag(&mut member, "tag-member").await;
        let sealed = member.own_ops().await;
        assert!(!sealed.is_empty());
        hub.receive(&sealed).await;
        hub.apply_all().await.unwrap();
        let writer = member.writer();
        assert_eq!(retirement(&mut hub, writer).await, None);
        hub.remove(&device_id(1)).await.unwrap();
        hub.apply_all().await.unwrap();
        assert_eq!(
            retirement(&mut hub, writer).await,
            Some(WriterRetirement::Removed)
        );
        let anchor = hub.writer();
        assert_eq!(retirement(&mut hub, anchor).await, None);
    });
}
