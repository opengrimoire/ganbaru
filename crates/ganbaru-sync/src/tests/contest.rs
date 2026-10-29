//! Yields and objections the local device seals into a contested epoch instance.

use ganbaru_sync_contracts::{AdminKeyPair, DeviceKeyPair, DeviceRole, Digest32, WriterId};

use super::authorization::{admin_entry, genesis_of, head, remove_entry, tag, view};
use super::block_on;
use super::replica::{
    ANCHOR_DEVICE_ID, Identity, Replica, anchor_admin, device_admin, device_id, enroll_entry,
    person, proposal,
};
use crate::{
    CandidateKind, ContestAction, ContestCause, ContestEntry, ContestRejection, ContestRole,
    ContestView, ContestsView, KitPointPlace, LosingRemoval, StoreOutcome, StoreRefusal,
    WriterRetirement,
};

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

/// The contests `replica` shows to its own device, holding `admin` when it has one.
async fn contests(replica: &mut Replica, admin: Option<&AdminKeyPair>) -> ContestsView {
    let device = replica.device_id.clone();
    let key = admin.map(AdminKeyPair::public_key);
    replica
        .engine
        .contests(&mut replica.conn, &replica.ctx, &device, key.as_ref())
        .await
        .unwrap()
        .expect("test spaces have a charter")
}

/// The yield and objection actions of `view`, without the kit action.
fn contest_actions(view: &ContestView) -> Vec<ContestAction> {
    view.actions
        .iter()
        .filter(|action| !matches!(action, ContestAction::UseKit { .. }))
        .cloned()
        .collect()
}

#[test]
fn two_takes_show_one_contest_with_a_yield_for_each_contestant_and_the_kit_for_everyone() {
    block_on(async {
        let (mut hub, mut first, mut second, mut third) = admins(76).await;
        let (_, point, point_hash) = head(&mut hub).await;
        assert_eq!(
            contests(&mut hub, Some(&anchor_admin(&person()))).await,
            ContestsView::default()
        );
        let to_first = take(&mut first, 20).await;
        let to_second = take(&mut second, 21).await;
        hub.pull(&mut first).await;
        hub.pull(&mut second).await;
        for replica in [&mut first, &mut second, &mut third] {
            replica.pull(&mut hub).await;
        }

        let shown = contests(&mut second, Some(&device_admin(21))).await;
        assert_eq!(shown.contests.len(), 1);
        assert!(shown.losing_removals.is_empty());
        let contest = &shown.contests[0];
        assert_eq!(contest.cause, ContestCause::Contested);
        assert_eq!(
            contest.frozen_at,
            Some(ContestEntry {
                hash: point_hash,
                revision: point
            })
        );
        assert_eq!(contest.instance.anchor_device_id, ANCHOR_DEVICE_ID);
        assert_eq!(contest.instance.epoch, 0);
        let candidates: Vec<(Digest32, &str, Option<&str>, bool)> = contest
            .candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.hash,
                    candidate.target_device_id.as_str(),
                    candidate.signer_device_id.as_deref(),
                    candidate.local,
                )
            })
            .collect();
        assert_eq!(candidates.len(), 2);
        assert!(candidates.contains(&(
            to_first,
            device_id(20).as_str(),
            Some(device_id(20).as_str()),
            false
        )));
        assert!(candidates.contains(&(
            to_second,
            device_id(21).as_str(),
            Some(device_id(21).as_str()),
            true
        )));
        for candidate in &contest.candidates {
            assert_eq!(candidate.kind, CandidateKind::Forced);
            assert_eq!(candidate.parent.hash, point_hash);
            assert!(
                candidate
                    .target_enrolled_revision
                    .is_some_and(|revision| revision <= point)
            );
            assert_eq!(
                candidate.target_enroller_device_id.as_deref(),
                Some(ANCHOR_DEVICE_ID)
            );
            assert_eq!(candidate.withdrawn_in_favor_of, None);
        }
        assert!(contest.objections.is_empty() && contest.yields.is_empty());
        assert!(contest.held_device_ids.is_empty());
        assert_eq!(
            contest.local_role,
            ContestRole::Contestant {
                candidate: to_second
            }
        );
        assert_eq!(
            contest_actions(contest),
            vec![ContestAction::YieldTo {
                candidate: to_first
            }]
        );
        let Some(ContestAction::UseKit { points }) = contest.actions.last() else {
            panic!("the kit action closes every contest's actions");
        };
        assert_eq!(
            points.first().map(|point| &point.place),
            Some(&KitPointPlace::Head)
        );

        // A device without a candidate only has the kit, and so does one without an admin key.
        let bystander = contests(&mut third, Some(&device_admin(22))).await;
        assert_eq!(bystander.contests[0].local_role, ContestRole::Bystander);
        assert!(contest_actions(&bystander.contests[0]).is_empty());
        let keyless = contests(&mut third, None).await;
        assert_eq!(keyless.contests[0].local_role, ContestRole::Bystander);
        assert!(matches!(
            keyless.contests[0].actions.as_slice(),
            [ContestAction::UseKit { .. }]
        ));

        // The offered yield is one the sealing path accepts, and it resolves the contest.
        second
            .seal_yield(&device_admin(21), &to_first)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            contests(&mut second, Some(&device_admin(21))).await,
            ContestsView::default()
        );
        // The old anchor may still object to the take that won by the yield.
        hub.pull(&mut second).await;
        let resolved = contests(&mut hub, Some(&anchor_admin(&person()))).await;
        assert_eq!(resolved.contests.len(), 1);
        assert_eq!(
            (resolved.contests[0].cause, resolved.contests[0].frozen_at),
            (ContestCause::Taken, None)
        );
        assert_eq!(
            contest_actions(&resolved.contests[0]),
            vec![ContestAction::Object {
                candidate: to_first
            }]
        );
    });
}

#[test]
fn a_third_contestant_sees_the_yield_of_another_withdraw_its_candidate() {
    block_on(async {
        let (mut hub, mut first, mut second, mut third) = admins(77).await;
        let to_first = take(&mut first, 20).await;
        let to_second = take(&mut second, 21).await;
        let to_third = take(&mut third, 22).await;
        for replica in [&mut first, &mut second, &mut third] {
            hub.pull(replica).await;
        }
        second.pull(&mut hub).await;
        second
            .seal_yield(&device_admin(21), &to_first)
            .await
            .unwrap()
            .unwrap();
        hub.pull(&mut second).await;
        third.pull(&mut hub).await;

        let shown = contests(&mut third, Some(&device_admin(22))).await;
        let contest = &shown.contests[0];
        assert_eq!(contest.yields.len(), 1);
        assert_eq!(contest.yields[0].to, to_first);
        assert_eq!(
            contest.yields[0].author_device_id.as_deref(),
            Some(device_id(21).as_str())
        );
        let withdrawn: Vec<(Digest32, Option<Digest32>)> = contest
            .candidates
            .iter()
            .map(|candidate| (candidate.hash, candidate.withdrawn_in_favor_of))
            .collect();
        assert!(withdrawn.contains(&(to_second, Some(to_first))));
        assert!(withdrawn.contains(&(to_first, None)));
        assert!(withdrawn.contains(&(to_third, None)));
        // A withdrawn candidate is no longer one to give way to.
        assert_eq!(
            contest_actions(contest),
            vec![ContestAction::YieldTo {
                candidate: to_first
            }]
        );

        // The contestant that already yielded is offered nothing more.
        let own = contests(&mut second, Some(&device_admin(21))).await;
        assert!(contest_actions(&own.contests[0]).is_empty());
    });
}

#[test]
fn the_old_anchor_is_offered_an_objection_to_a_lone_take_and_then_its_withdrawal() {
    block_on(async {
        let (mut hub, mut first, mut second, _) = admins(78).await;
        let taken = take(&mut first, 20).await;
        hub.pull(&mut first).await;
        second.pull(&mut hub).await;
        let anchor = anchor_admin(&person());

        let shown = contests(&mut hub, Some(&anchor)).await;
        assert_eq!(shown.contests.len(), 1);
        let contest = &shown.contests[0];
        assert_eq!(
            (contest.cause, contest.frozen_at),
            (ContestCause::Taken, None)
        );
        assert_eq!(contest.local_role, ContestRole::Anchor);
        assert_eq!(
            contest_actions(contest),
            vec![ContestAction::Object { candidate: taken }]
        );
        // Nobody else sees a contest in a take that won.
        assert_eq!(
            contests(&mut second, Some(&device_admin(21))).await,
            ContestsView::default()
        );
        assert_eq!(
            contests(&mut first, Some(&device_admin(20))).await,
            ContestsView::default()
        );

        hub.seal_objection(&anchor, &taken).await.unwrap().unwrap();
        let objected = contests(&mut hub, Some(&anchor)).await;
        let contest = &objected.contests[0];
        assert_eq!(contest.cause, ContestCause::Contested);
        assert!(contest.frozen_at.is_some());
        assert_eq!(contest.objections.len(), 1);
        assert_eq!(
            (
                contest.objections[0].against,
                contest.objections[0].withdrawn
            ),
            (taken, false)
        );
        assert_eq!(
            contest.objections[0].objector_device_id.as_deref(),
            Some(ANCHOR_DEVICE_ID)
        );
        assert_eq!(
            contest_actions(contest),
            vec![ContestAction::WithdrawObjection { candidate: taken }]
        );
        first.pull(&mut hub).await;
        let contestant = contests(&mut first, Some(&device_admin(20))).await;
        assert_eq!(
            contestant.contests[0].local_role,
            ContestRole::Contestant { candidate: taken }
        );
        assert!(contest_actions(&contestant.contests[0]).is_empty());
        // The objection removes the taker on the anchor's branch, which is not effective.
        assert_eq!(
            contestant.losing_removals,
            vec![LosingRemoval {
                device_id: device_id(20),
                removed_by_device_id: Some(ANCHOR_DEVICE_ID.to_owned()),
                revision: contestant.losing_removals[0].revision,
            }]
        );

        hub.seal_yield(&anchor, &taken).await.unwrap().unwrap();
        assert_eq!(
            contests(&mut hub, Some(&anchor)).await,
            ContestsView::default()
        );
    });
}

#[test]
fn a_removal_on_a_contested_branch_is_listed_with_its_remover_for_the_kit_review() {
    block_on(async {
        let (mut hub, mut first, mut second, mut third) = admins(79).await;
        take(&mut first, 20).await;
        take(&mut second, 21).await;
        let removal = proposal(
            first.ctx.space,
            &device_admin(20),
            first.now_ms,
            remove_entry(&device_id(22)),
        );
        first.sequence_signed(&removal).await.unwrap().unwrap();
        for from in [&mut first, &mut second, &mut hub] {
            third.pull(from).await;
        }

        let shown = contests(&mut third, Some(&device_admin(22))).await;
        assert_eq!(shown.contests.len(), 1);
        assert_eq!(shown.contests[0].held_device_ids, vec![device_id(22)]);
        assert_eq!(shown.losing_removals.len(), 1);
        let removed = &shown.losing_removals[0];
        assert_eq!(
            (
                removed.device_id.as_str(),
                removed.removed_by_device_id.as_deref()
            ),
            (device_id(22).as_str(), Some(device_id(20).as_str()))
        );
    });
}
