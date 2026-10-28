//! Fork evidence: verification, exclusion of both branches past the shared prefix on every
//! replica but the forked writer's device, honest dependents across a fork, re-sealing from
//! received evidence, and the freeze of an anchor epoch whose writer forked. Removals beyond the
//! bound of stored excluded operations are covered here too, since the same readiness rule and
//! branch drop serve both causes.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::{DeviceRole, Envelope, ProposalRejection};

use super::authorization::{admin_entry, forge_handover, remove_entry, superseded, view};
use super::block_on;
use super::exclusion::{deliver, displaced, edit, late_changes, note, title};
use super::replica::{
    ANCHOR_DEVICE_ID, Replica, anchor_admin, converge, device_id, enroll_entry, pair, person,
};
use crate::{
    EvidenceOutcome, EvidenceRefusal, MAX_EXCLUDED_PER_WRITER, StoreOutcome, StoreRefusal,
};

const REVOKED: StoreOutcome = StoreOutcome::Refused(StoreRefusal::Revoked);

/// A writer's installation that sealed a shared prefix of two notes, and a clone of it on the
/// same device, as a restored copy of the vault folder would be.
async fn installation(salt: u64) -> (Replica, Replica) {
    let mut original = Replica::new(1, salt).await;
    edit(
        &mut original,
        &format!(
            "{}{}",
            note("note-1", "Draft", 0),
            note("note-2", "Other", 1)
        ),
    )
    .await;
    let clone = original.clone_device().await;
    (original, clone)
}

/// The original and the clone each seal their own title for `note-1` at the same sequence.
async fn diverge(original: &mut Replica, clone: &mut Replica) {
    edit(
        original,
        "UPDATE quick_notes SET title = 'Original' WHERE id = 'note-1'",
    )
    .await;
    edit(
        clone,
        "UPDATE quick_notes SET title = 'Clone' WHERE id = 'note-1'",
    )
    .await;
}

async fn offer(replica: &mut Replica, first: &[u8], second: &[u8]) -> EvidenceOutcome {
    replica
        .engine
        .store_fork_evidence(
            &mut replica.conn,
            &replica.ctx,
            first,
            second,
            replica.now_ms,
        )
        .await
        .unwrap()
}

/// Offers `to` every operation `from` sealed, including those at sequences `to` already stores
/// from another branch, then delivers the rest. Delivery by version vector alone never offers an
/// operation at a sequence the receiver already holds.
async fn cross(to: &mut Replica, from: &mut Replica) {
    let own = from.own_ops().await;
    to.receive(&own).await;
    deliver(to, from).await;
}

/// Stores `ops` and applies them.
async fn take(replica: &mut Replica, ops: &[Vec<u8>]) -> Vec<StoreOutcome> {
    let outcomes = replica.receive(ops).await;
    replica.apply_all().await.unwrap();
    outcomes
}

#[test]
fn fork_evidence_is_verified_before_it_is_stored() {
    block_on(async {
        let (mut original, mut clone) = installation(301).await;
        diverge(&mut original, &mut clone).await;
        let ours = original.own_ops().await;
        let theirs = clone.own_ops().await;
        assert_eq!(ours[..2], theirs[..2]);
        assert_ne!(ours[2], theirs[2]);
        let refused = |refusal| EvidenceOutcome::Refused(refusal);

        let mut member = Replica::new(2, 301).await;
        assert_eq!(
            offer(&mut member, &ours[2], &theirs[2]).await,
            refused(EvidenceRefusal::UnknownWriter),
            "the writer key is unknown before its genesis is stored"
        );
        take(&mut member, &ours[..2]).await;
        edit(&mut member, &note("note-member", "Member", 2)).await;
        let own = member.own_ops().await;
        assert_eq!(
            offer(&mut member, b"not an envelope", &theirs[2]).await,
            refused(EvidenceRefusal::Malformed)
        );
        assert_eq!(
            offer(&mut member, &ours[2], &ours[2]).await,
            refused(EvidenceRefusal::SameOperation)
        );
        assert_eq!(
            offer(&mut member, &ours[1], &theirs[2]).await,
            refused(EvidenceRefusal::Mismatched),
            "different sequences"
        );
        assert_eq!(
            offer(&mut member, &ours[1], &own[1]).await,
            refused(EvidenceRefusal::Mismatched),
            "different writers"
        );
        let mut forged = theirs[2].clone();
        *forged.last_mut().unwrap() ^= 1;
        assert_eq!(
            offer(&mut member, &ours[2], &forged).await,
            refused(EvidenceRefusal::BadSignature)
        );
        let other = Replica::with_space(3, 301, "vault-other", person()).await;
        let mut moved = Envelope::decode(&theirs[2]).unwrap().operation().unwrap();
        moved.space = other.ctx.space;
        let moved = moved.seal(&clone.identity.key).unwrap().into_bytes();
        assert_eq!(
            offer(&mut member, &ours[2], &moved).await,
            refused(EvidenceRefusal::WrongSpace)
        );
        assert!(member.evidence().await.is_empty());

        assert_eq!(
            offer(&mut member, &ours[2], &theirs[2]).await,
            EvidenceOutcome::Stored
        );
        assert_eq!(
            offer(&mut member, &theirs[2], &ours[2]).await,
            EvidenceOutcome::Known
        );
        let evidence = member.evidence().await;
        assert_eq!(evidence.len(), 1);
        let evidence = &evidence[0];
        assert_eq!(
            (evidence.writer, evidence.seq, evidence.keep_through()),
            (original.writer(), 3, 2)
        );
        let first = Envelope::decode(&evidence.first).unwrap().hash();
        let second = Envelope::decode(&evidence.second).unwrap().hash();
        assert!(first < second);
        assert_eq!(
            member
                .engine
                .forked_writers(&mut member.conn, &member.ctx)
                .await
                .unwrap(),
            vec![(original.writer(), 3)]
        );
        assert_eq!(member.receive(&ours[2..]).await, vec![REVOKED]);
        assert_eq!(member.receive(&theirs[2..]).await, vec![REVOKED]);
        member.check_invariants().await;
    });
}

#[test]
fn evidence_before_or_after_either_branch_leaves_what_never_storing_them_leaves() {
    block_on(async {
        let (mut original, mut clone) = installation(302).await;
        let prefix = original.own_ops().await;
        diverge(&mut original, &mut clone).await;
        edit(&mut clone, &note("note-clone", "Clone", 2)).await;
        let ours = original.own_ops().await;
        let theirs = clone.own_ops().await;
        let writer = original.writer();

        let mut replicas = Vec::new();
        for index in 2..7 {
            let mut replica = Replica::new(index, 302).await;
            take(&mut replica, &prefix).await;
            replicas.push(replica);
        }
        let expected = replicas[0].domain_dump().await;
        let [never, early, after_ours, after_theirs, detects] = &mut replicas[..] else {
            unreachable!()
        };

        assert_eq!(
            offer(early, &ours[2], &theirs[2]).await,
            EvidenceOutcome::Stored
        );
        assert_eq!(take(early, &ours[2..]).await, vec![REVOKED]);
        assert_eq!(take(early, &theirs[2..]).await, vec![REVOKED; 2]);

        take(after_ours, &ours[2..]).await;
        assert_eq!(
            title(after_ours, "note-1").await.as_deref(),
            Some("Original")
        );
        assert_eq!(
            offer(after_ours, &ours[2], &theirs[2]).await,
            EvidenceOutcome::Stored
        );

        take(after_theirs, &theirs[2..]).await;
        assert_eq!(
            title(after_theirs, "note-clone").await.as_deref(),
            Some("Clone")
        );
        assert_eq!(
            offer(after_theirs, &theirs[2], &ours[2]).await,
            EvidenceOutcome::Stored
        );

        take(detects, &ours[2..]).await;
        assert_eq!(
            take(detects, &theirs[2..]).await,
            vec![StoreOutcome::Refused(StoreRefusal::Fork { seq: 3 })],
            "storing the other branch records the evidence"
        );

        for replica in [never, early, after_ours, after_theirs, detects] {
            replica.apply_all().await.unwrap();
            assert_eq!(
                replica.domain_dump().await,
                expected,
                "replica {}",
                replica.index
            );
            assert_eq!(replica.stored_vector().await.get(&writer), 2);
            assert_eq!(replica.applied_vector().await.get(&writer), 2);
            assert_eq!(displaced(replica).await, 0);
            assert!(late_changes(replica).await.is_empty(), "a fork leaves none");
            replica.check_invariants().await;
        }
    });
}

#[test]
fn excluding_both_branches_restores_the_versions_they_displaced() {
    block_on(async {
        let (mut original, mut clone) = installation(303).await;
        let mut honest = Replica::new(2, 303).await;
        deliver(&mut honest, &mut original).await;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Honest' WHERE id = 'note-1'",
        )
        .await;
        deliver(&mut original, &mut honest).await;
        deliver(&mut clone, &mut honest).await;
        edit(
            &mut original,
            "UPDATE quick_notes SET title = 'Original' WHERE id = 'note-1';
             DELETE FROM quick_notes WHERE id = 'note-2';",
        )
        .await;
        edit(
            &mut clone,
            "UPDATE quick_notes SET title = 'Clone' WHERE id = 'note-2'",
        )
        .await;

        let mut observer = Replica::new(3, 303).await;
        deliver(&mut observer, &mut honest).await;
        let expected = observer.domain_dump().await;

        let mut member = Replica::new(4, 303).await;
        deliver(&mut member, &mut honest).await;
        deliver(&mut member, &mut original).await;
        assert_eq!(
            title(&mut member, "note-1").await.as_deref(),
            Some("Original")
        );
        assert_eq!(title(&mut member, "note-2").await, None);
        assert!(displaced(&mut member).await > 0);
        let theirs = clone.own_ops().await;
        assert_eq!(
            take(&mut member, &theirs[2..]).await,
            vec![StoreOutcome::Refused(StoreRefusal::Fork { seq: 3 })]
        );
        assert_eq!(
            member.domain_dump().await,
            expected,
            "excluding applied branches leaves what never applying them leaves"
        );
        assert_eq!(
            title(&mut member, "note-1").await.as_deref(),
            Some("Honest")
        );
        assert_eq!(displaced(&mut member).await, displaced(&mut observer).await);
        member.check_invariants().await;
    });
}

#[test]
fn honest_dependents_of_a_forked_operation_end_the_same_whenever_the_fork_was_learned() {
    block_on(async {
        let (mut original, mut clone) = installation(304).await;
        let prefix = original.own_ops().await;
        diverge(&mut original, &mut clone).await;
        let ours = original.own_ops().await;
        let theirs = clone.own_ops().await;
        let writer = original.writer();

        // The honest member applies the original's branch and edits on top of it.
        let mut honest = Replica::new(2, 304).await;
        take(&mut honest, &ours).await;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Honest' WHERE id = 'note-1';
             UPDATE quick_notes SET title = 'Dependent' WHERE id = 'note-2';",
        )
        .await;
        let dependent = honest.own_ops().await;

        // A stored and applied the forked operation, then learned of the fork; B learned of it
        // first and refused the operation; C never saw the operation.
        let mut a = Replica::new(3, 304).await;
        let mut b = Replica::new(4, 304).await;
        let mut c = Replica::new(5, 304).await;
        take(&mut a, &ours).await;
        assert_eq!(title(&mut a, "note-1").await.as_deref(), Some("Original"));
        for replica in [&mut b, &mut c] {
            take(replica, &prefix).await;
        }
        assert_eq!(
            offer(&mut b, &ours[2], &theirs[2]).await,
            EvidenceOutcome::Stored
        );
        assert_eq!(take(&mut b, &ours[2..]).await, vec![REVOKED]);
        assert_eq!(
            offer(&mut a, &theirs[2], &ours[2]).await,
            EvidenceOutcome::Stored
        );
        a.apply_all().await.unwrap();

        // The honest dependent arrives everywhere. C holds it until it learns of the fork.
        for replica in [&mut a, &mut b, &mut c] {
            take(replica, &dependent).await;
        }
        assert!(
            c.applied_vector().await.get(&honest.writer())
                < c.stored_vector().await.get(&honest.writer()),
            "the dependent waits for an operation C does not know is excluded"
        );
        assert_eq!(
            offer(&mut c, &ours[2], &theirs[2]).await,
            EvidenceOutcome::Stored
        );
        c.apply_all().await.unwrap();

        let expected = a.domain_dump().await;
        for replica in [&mut a, &mut b, &mut c] {
            assert_eq!(replica.domain_dump().await, expected);
            assert_eq!(
                replica.applied_vector().await.get(&honest.writer()),
                honest.stored_vector().await.get(&honest.writer())
            );
            assert_eq!(replica.stored_vector().await.get(&writer), 2);
            assert_eq!(title(replica, "note-1").await.as_deref(), Some("Honest"));
            assert_eq!(title(replica, "note-2").await.as_deref(), Some("Dependent"));
            replica.check_invariants().await;
        }
        assert_eq!(
            offer(&mut honest, &ours[2], &theirs[2]).await,
            EvidenceOutcome::Stored
        );
        honest.apply_all().await.unwrap();
        assert_eq!(honest.domain_dump().await, expected);

        // Both installations re-seal their branch, and every replica ends the same.
        let mut replicas = vec![original, clone, honest, a, b, c];
        converge(&mut replicas).await;
        let settled = replicas[0].domain_dump().await;
        for replica in &mut replicas {
            assert_eq!(replica.domain_dump().await, settled);
            assert_eq!(replica.stored_vector().await.get(&writer), 2);
            assert!(replica.evidence().await.len() == 1);
        }
    });
}

#[test]
fn a_fork_learned_after_the_anchor_confirmed_a_branch_converges_on_the_anchor_without_the_clone() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 305).await;
        let (mut original, mut clone) = installation(305).await;
        let mut honest = Replica::new(2, 305).await;
        deliver(&mut honest, &mut original).await;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Honest', color = 3 WHERE id = 'note-1'",
        )
        .await;
        deliver(&mut original, &mut honest).await;
        deliver(&mut clone, &mut honest).await;
        let mut never = Replica::new(3, 305).await;
        deliver(&mut never, &mut honest).await;
        let before = never.domain_dump().await;
        diverge(&mut original, &mut clone).await;

        // The anchor and a member apply the original's branch, and the member learns that the
        // anchor stored it, which releases what it displaced.
        let mut member = Replica::new(4, 305).await;
        for replica in [&mut anchor, &mut member] {
            deliver(replica, &mut honest).await;
            deliver(replica, &mut original).await;
            assert_eq!(title(replica, "note-1").await.as_deref(), Some("Original"));
        }
        let confirmed = anchor.stored_vector().await;
        assert!(
            member
                .engine
                .record_anchor_marks(&mut member.conn, &member.ctx, ANCHOR_DEVICE_ID, &confirmed)
                .await
                .unwrap()
        );
        assert_eq!(displaced(&mut member).await, 0);

        // One member receives only the clone's branch and another receives both, which stores
        // the evidence on it.
        let mut cloned = Replica::new(5, 305).await;
        deliver(&mut cloned, &mut honest).await;
        deliver(&mut cloned, &mut clone).await;
        assert_eq!(title(&mut cloned, "note-1").await.as_deref(), Some("Clone"));
        let mut both = Replica::new(6, 305).await;
        deliver(&mut both, &mut honest).await;
        cross(&mut both, &mut original).await;
        cross(&mut both, &mut clone).await;
        assert_eq!(both.evidence().await.len(), 1);

        // Evidence reaches the anchor and the member. Nothing they displaced is left to
        // restore, so the title group keeps no version and the branch value, while the color
        // stays. The anchor captured the group to publish it again.
        cross(&mut anchor, &mut clone).await;
        deliver(&mut member, &mut anchor).await;
        for replica in [&mut anchor, &mut member] {
            assert_eq!(replica.evidence().await.len(), 1);
            replica.check_invariants().await;
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM quick_notes WHERE id = 'note-1' AND color = 3")
                    .await,
                1
            );
            assert_eq!(title(replica, "note-1").await.as_deref(), Some("Original"));
        }
        assert_eq!(never.domain_dump().await, before);
        assert_eq!(
            anchor
                .count("SELECT count(*) FROM sync_capture WHERE row_key = 'note-1'")
                .await,
            1
        );
        assert_eq!(member.count("SELECT count(*) FROM sync_capture").await, 0);

        // The clone is destroyed before it re-seals. The anchor's own operation settles every
        // other replica on its state, original included once it re-seals its branch.
        drop(clone);
        assert!(anchor.seal().await.unwrap().sealed_ops > 0);
        let mut replicas = vec![anchor, honest, never, member, cloned, both];
        converge(&mut replicas).await;
        let settled = replicas[0].dump().await;
        for replica in &mut replicas {
            assert_eq!(replica.dump().await, settled, "replica {}", replica.index);
            assert_eq!(title(replica, "note-1").await.as_deref(), Some("Original"));
            assert!(late_changes(replica).await.is_empty());
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM sync_ops WHERE state != 'applied'")
                    .await,
                0
            );
        }
        replicas.push(original);
        converge(&mut replicas).await;
        let settled = replicas[0].dump().await;
        for replica in &mut replicas {
            assert_eq!(replica.dump().await, settled, "replica {}", replica.index);
            assert!(late_changes(replica).await.is_empty());
        }
    });
}

#[test]
fn a_branch_the_anchor_stored_without_applying_is_published_again_from_the_anchor() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 307).await;
        let (mut original, mut clone) = installation(307).await;
        let mut honest = Replica::new(2, 307).await;
        deliver(&mut honest, &mut original).await;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Honest' WHERE id = 'note-1'",
        )
        .await;
        deliver(&mut original, &mut honest).await;
        deliver(&mut clone, &mut honest).await;
        diverge(&mut original, &mut clone).await;

        // The anchor stores the original's branch without applying it, and its stored vector
        // releases what the branch displaced on a member that applied it.
        deliver(&mut anchor, &mut honest).await;
        let branch = original.ops_since(&anchor.stored_vector().await).await;
        anchor.receive(&branch).await;
        let mut member = Replica::new(4, 307).await;
        deliver(&mut member, &mut honest).await;
        deliver(&mut member, &mut original).await;
        let confirmed = anchor.stored_vector().await;
        assert!(
            member
                .engine
                .record_anchor_marks(&mut member.conn, &member.ctx, ANCHOR_DEVICE_ID, &confirmed)
                .await
                .unwrap()
        );
        assert_eq!(displaced(&mut member).await, 0);

        // Evidence reaches both before the anchor applies. The anchor keeps the honest title,
        // the member keeps the branch value, and the anchor's operation settles the member.
        cross(&mut anchor, &mut clone).await;
        deliver(&mut member, &mut anchor).await;
        assert_eq!(
            title(&mut anchor, "note-1").await.as_deref(),
            Some("Honest")
        );
        assert_eq!(
            title(&mut member, "note-1").await.as_deref(),
            Some("Original")
        );
        drop(clone);
        drop(original);
        let mut replicas = vec![anchor, honest, member];
        converge(&mut replicas).await;
        let settled = replicas[0].dump().await;
        for replica in &mut replicas {
            assert_eq!(replica.dump().await, settled, "replica {}", replica.index);
            assert_eq!(title(replica, "note-1").await.as_deref(), Some("Honest"));
        }
    });
}

/// The anchor applies the original's branch, which creates `note-3`, and changes that note with
/// `statement`. A member learns of the fork before it stores the branch and applies the anchor's
/// change to a hidden row waiting for a creation; the anchor hides the row the same way once
/// evidence reaches it. The original re-seals its branch, which creates the note again: an
/// observer that applies the re-sealed creation before the anchor's change ends on the same
/// state, and every replica shows `expected`.
async fn change_branch_row_on_the_anchor(space: u64, statement: &str, expected: Option<&str>) {
    let mut anchor = Replica::anchor(0, space).await;
    let (mut original, mut clone) = installation(space).await;
    let mut member = Replica::new(2, space).await;
    let mut observer = Replica::new(3, space).await;
    for replica in [&mut anchor, &mut member, &mut observer] {
        deliver(replica, &mut original).await;
    }
    edit(&mut original, &note("note-3", "Branch", 2)).await;
    edit(
        &mut clone,
        "UPDATE quick_notes SET title = 'Clone' WHERE id = 'note-1'",
    )
    .await;
    let ours = original.own_ops().await;
    let theirs = clone.own_ops().await;

    deliver(&mut anchor, &mut original).await;
    edit(&mut anchor, statement).await;
    assert_eq!(
        offer(&mut member, &ours[2], &theirs[2]).await,
        EvidenceOutcome::Stored
    );
    deliver(&mut member, &mut anchor).await;
    assert_eq!(
        member.applied_vector().await.get(&anchor.writer()),
        anchor.stored_vector().await.get(&anchor.writer()),
        "the anchor's change applies to a row waiting for a creation"
    );
    assert_eq!(
        member
            .count("SELECT count(*) FROM sync_ops WHERE state != 'applied'")
            .await,
        0
    );
    assert_eq!(title(&mut member, "note-3").await, None);

    // Evidence reaches the anchor, which hides the note only the branch created.
    cross(&mut anchor, &mut clone).await;
    anchor.seal().await.unwrap();
    assert_eq!(title(&mut anchor, "note-3").await, None);

    // The original re-seals its branch, and the observer applies it before the anchor's change.
    deliver(&mut original, &mut anchor).await;
    assert_eq!(
        offer(&mut observer, &ours[2], &theirs[2]).await,
        EvidenceOutcome::Stored
    );
    deliver(&mut observer, &mut original).await;
    drop(clone);
    let mut replicas = vec![anchor, member, original, observer];
    converge(&mut replicas).await;
    let settled = replicas[0].dump().await;
    for replica in &mut replicas {
        assert_eq!(replica.dump().await, settled, "replica {}", replica.index);
        assert_eq!(
            replica
                .count("SELECT count(*) FROM sync_ops WHERE state != 'applied'")
                .await,
            0
        );
        assert_eq!(
            replica
                .count("SELECT count(*) FROM sync_rows WHERE creation_pending = 1")
                .await,
            0
        );
        assert_eq!(title(replica, "note-3").await.as_deref(), expected);
        replica.check_invariants().await;
    }
}

#[test]
fn a_change_to_a_row_only_a_forked_branch_created_waits_for_its_re_sealed_creation() {
    block_on(change_branch_row_on_the_anchor(
        308,
        "UPDATE quick_notes SET title = 'Anchor' WHERE id = 'note-3'",
        Some("Anchor"),
    ));
}

#[test]
fn a_delete_of_a_row_only_a_forked_branch_created_survives_its_re_sealed_creation() {
    block_on(change_branch_row_on_the_anchor(
        309,
        "DELETE FROM quick_notes WHERE id = 'note-3'",
        None,
    ));
}

#[test]
fn both_copies_reseal_from_received_evidence_and_a_successor_can_fork_again() {
    block_on(async {
        let (mut original, mut clone) = installation(306).await;
        let mut hub = Replica::new(2, 306).await;
        let observer = Replica::new(3, 306).await;
        let forked = original.writer();
        diverge(&mut original, &mut clone).await;
        deliver(&mut hub, &mut original).await;
        cross(&mut hub, &mut clone).await;
        assert_eq!(hub.evidence().await.len(), 1);

        // Each installation learns of the fork from the hub and re-seals its own copy.
        deliver(&mut original, &mut hub).await;
        deliver(&mut clone, &mut hub).await;
        assert_ne!(original.writer(), forked);
        assert_ne!(clone.writer(), forked);
        assert_ne!(original.writer(), clone.writer());
        for installation in [&mut original, &mut clone] {
            assert!(
                installation
                    .engine
                    .own_forks(&mut installation.conn, &installation.ctx)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(installation.stored_vector().await.get(&forked), 2);
        }
        let successors = [original.writer(), clone.writer()];
        let mut replicas = vec![hub, observer, original, clone];
        converge(&mut replicas).await;
        let settled = replicas[0].domain_dump().await;
        for replica in &mut replicas {
            assert_eq!(replica.domain_dump().await, settled);
            let stored = replica.stored_vector().await;
            assert_eq!(stored.get(&forked), 2);
            for successor in successors {
                assert!(stored.get(&successor) > 1, "both successors are admitted");
            }
        }
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM quick_notes WHERE id = 'note-1'")
                .await,
            1
        );

        // A copy of the original made after its re-seal forks the successor the same way.
        let mut second = replicas[2].clone_device().await;
        let successor = replicas[2].writer();
        edit(
            &mut replicas[2],
            "UPDATE quick_notes SET title = 'Again' WHERE id = 'note-2'",
        )
        .await;
        edit(
            &mut second,
            "UPDATE quick_notes SET title = 'Copy' WHERE id = 'note-2'",
        )
        .await;
        cross(&mut replicas[0], &mut second).await;
        let (hub, first) = pair(&mut replicas, 0, 2);
        cross(hub, first).await;
        assert_eq!(replicas[0].evidence().await.len(), 2);
        replicas.push(second);
        converge(&mut replicas).await;
        let settled = replicas[0].domain_dump().await;
        let mut writers = Vec::new();
        for replica in &mut replicas {
            assert_eq!(replica.domain_dump().await, settled);
            assert_eq!(replica.evidence().await.len(), 2);
            writers.push(replica.writer());
            replica.check_invariants().await;
        }
        writers.sort();
        writers.dedup();
        assert_eq!(writers.len(), replicas.len());
        assert!(!writers.contains(&successor));
    });
}

#[test]
fn a_forked_anchor_freezes_its_epoch_until_a_handover() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 307).await;
        anchor.sequence(admin_entry(20)).await.unwrap().unwrap();
        let mut successor = Replica::new(20, 307).await;
        let mut member = Replica::new(2, 307).await;
        deliver(&mut successor, &mut anchor).await;
        deliver(&mut member, &mut anchor).await;
        let base = view(&mut member).await.revision;
        assert_eq!(view(&mut member).await.frozen_at, None);

        // Both copies of the anchor sequence an entry at the same revision.
        let mut copy = anchor.clone_device().await;
        anchor
            .sequence(enroll_entry(&person(), 21, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        copy.sequence(enroll_entry(&person(), 22, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        deliver(&mut member, &mut anchor).await;
        assert_eq!(view(&mut member).await.revision, base + 1);
        cross(&mut member, &mut copy).await;
        let frozen = view(&mut member).await;
        assert_eq!(
            frozen.revision, base,
            "entries past the fork are superseded"
        );
        assert_eq!(frozen.frozen_at, Some(base));
        assert!(
            !frozen.devices.iter().any(
                |device| device.device_id.ends_with("-21") || device.device_id.ends_with("-22")
            )
        );

        // Each anchor copy learns of the fork, re-seals its domain operations only, and refuses
        // to sequence in the frozen epoch.
        for replica in [&mut anchor, &mut copy] {
            deliver(replica, &mut member).await;
            assert_eq!(view(replica).await, frozen);
            assert_eq!(
                replica
                    .sequence(enroll_entry(&person(), 23, DeviceRole::Member))
                    .await
                    .unwrap(),
                Err(ProposalRejection::EpochFrozen)
            );
            replica.check_invariants().await;
        }

        // A handover continues the log in a new epoch.
        deliver(&mut successor, &mut member).await;
        edit(&mut successor, &note("note-successor", "Successor", 0)).await;
        let (_, handover) = forge_handover(&mut successor, 20).await;
        assert_eq!(
            successor.receive(&[handover]).await,
            vec![StoreOutcome::Stored { held: None }]
        );
        successor.apply_all().await.unwrap();
        deliver(&mut member, &mut successor).await;
        let continued = view(&mut member).await;
        assert_eq!(continued.epoch, frozen.epoch + 1);
        assert_eq!(continued.revision, base + 1);
        assert_eq!(continued.frozen_at, None);
        deliver(&mut anchor, &mut member).await;
        assert_eq!(view(&mut anchor).await, continued);
        assert_eq!(
            anchor
                .sequence(enroll_entry(&person(), 23, DeviceRole::Member))
                .await
                .unwrap(),
            Err(ProposalRejection::NotAnchor)
        );
    });
}

/// Seals, past `cutoff`, the creation of `note-bound`, [`MAX_EXCLUDED_PER_WRITER`] minus one
/// edits of it, and then the creation of `note-late` in the first operation past the bound.
/// Returns the operations past `cutoff`.
async fn seal_past_bound(removed: &mut Replica, cutoff: u64) -> Vec<Vec<u8>> {
    edit(removed, &note("note-bound", "Bound", 5)).await;
    for index in 1..MAX_EXCLUDED_PER_WRITER {
        edit(
            removed,
            &format!("UPDATE quick_notes SET title = 'Bound {index}' WHERE id = 'note-bound'"),
        )
        .await;
    }
    edit(removed, &note("note-late", "Late", 6)).await;
    let ops = removed.own_ops().await;
    let past = ops[usize::try_from(cutoff).unwrap()..].to_vec();
    assert_eq!(past.len() as u64, MAX_EXCLUDED_PER_WRITER + 1);
    past
}

#[test]
fn a_removal_beyond_the_bound_ends_the_same_whether_the_operation_applied_was_refused_or_never_seen()
 {
    block_on(async {
        let mut anchor = Replica::anchor(0, 311).await;
        let mut removed = Replica::new(1, 311).await;
        let mut honest = Replica::new(2, 311).await;
        edit(&mut removed, &note("note-early", "Early", 0)).await;
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;
        let mut replicas = Vec::new();
        for index in 3..6 {
            let mut replica = Replica::new(index, 311).await;
            deliver(&mut replica, &mut anchor).await;
            replicas.push(replica);
        }
        let writer = removed.writer();
        let cutoff = anchor.stored_vector().await.get(&writer);
        anchor.remove(&removed.device_id).await.unwrap();
        let late = seal_past_bound(&mut removed, cutoff).await;

        // The honest member applies the creation past the bound and edits the row it created.
        take(&mut honest, &late).await;
        edit(
            &mut honest,
            "UPDATE quick_notes SET title = 'Honest' WHERE id = 'note-late'",
        )
        .await;
        let dependent = honest.own_ops().await;

        // A stored and applied the operation past the bound, then learned of the removal; B
        // learned first and refused it; C never saw any operation past the cutoff.
        let [a, b, c] = &mut replicas[..] else {
            unreachable!()
        };
        take(a, &late).await;
        assert_eq!(title(a, "note-late").await.as_deref(), Some("Late"));
        deliver(a, &mut anchor).await;
        deliver(b, &mut anchor).await;
        assert_eq!(take(b, &late).await.last(), Some(&REVOKED));
        deliver(c, &mut anchor).await;
        for replica in [&mut *a, &mut *b] {
            assert_eq!(
                replica.stored_vector().await.get(&writer),
                cutoff + MAX_EXCLUDED_PER_WRITER,
                "no replica stores an excluded operation past the bound"
            );
        }

        for replica in [&mut *a, &mut *b, &mut *c] {
            take(replica, &dependent).await;
            assert_eq!(
                replica.applied_vector().await.get(&honest.writer()),
                honest.stored_vector().await.get(&honest.writer()),
                "the honest dependent applies"
            );
        }
        deliver(&mut honest, &mut anchor).await;
        let expected = c.domain_dump().await;
        for replica in [a, b, c, &mut honest] {
            assert_eq!(replica.domain_dump().await, expected);
            assert_eq!(title(replica, "note-early").await.as_deref(), Some("Early"));
            assert_eq!(title(replica, "note-bound").await, None);
            replica.check_invariants().await;
        }
    });
}

#[test]
fn a_removed_device_stores_only_a_few_writers_its_removal_does_not_name() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 313).await;
        let mut removed = Replica::new(1, 313).await;
        let mut a = Replica::new(3, 313).await;
        let mut b = Replica::new(4, 313).await;
        // The anchor never stores the device's writers, so its removal names none of them.
        let mut writers = Vec::new();
        for (position, id) in ["note-first", "note-second", "note-third"]
            .into_iter()
            .enumerate()
        {
            if position > 0 {
                removed.rotate();
            }
            edit(&mut removed, &note(id, "Late", 0)).await;
            writers.push((removed.writer(), removed.own_ops().await));
        }
        let distinct: BTreeSet<_> = writers.iter().map(|(writer, _)| *writer).collect();
        assert_eq!(distinct.len(), writers.len());
        anchor.remove(&removed.device_id).await.unwrap();

        // A stored every writer before the removal; B learned of the removal first.
        for (_, ops) in &writers {
            take(&mut a, ops).await;
        }
        deliver(&mut a, &mut anchor).await;
        deliver(&mut b, &mut anchor).await;
        let (named, rest) = writers.split_at(2);
        for (writer, ops) in named {
            take(&mut b, ops).await;
            assert_eq!(
                b.stored_vector().await.get(writer),
                a.stored_vector().await.get(writer),
                "an unnamed writer within the bound keeps its excluded operations"
            );
        }
        let (writer, ops) = &rest[0];
        assert_eq!(take(&mut b, ops).await.first(), Some(&REVOKED));
        assert_eq!(b.stored_vector().await.get(writer), 0);

        let expected = anchor.domain_dump().await;
        for replica in [&mut a, &mut b] {
            assert_eq!(replica.domain_dump().await, expected);
            for id in ["note-first", "note-second", "note-third"] {
                assert_eq!(title(replica, id).await, None);
            }
            replica.check_invariants().await;
        }
    });
}

#[test]
fn a_creation_beyond_the_bound_is_offered_again_after_a_reselection_reverts_the_removal() {
    block_on(async {
        let mut space = superseded(312).await;
        let writer = space.member.writer();
        space.hub.pull(&mut space.member).await;
        let cutoff = space.hub.stored_vector().await.get(&writer);
        space
            .hub
            .sequence(remove_entry(&device_id(1)))
            .await
            .unwrap()
            .unwrap();
        let late = seal_past_bound(&mut space.member, cutoff).await;
        let chain = space.contender.own_ops().await;

        let mut reverted = Replica::new(2, 312).await;
        reverted.pull(&mut space.hub).await;
        assert_eq!(take(&mut reverted, &late).await.last(), Some(&REVOKED));
        assert_eq!(
            reverted.stored_vector().await.get(&writer),
            cutoff + MAX_EXCLUDED_PER_WRITER
        );
        take(&mut reverted, &chain).await;
        assert_eq!(view(&mut reverted).await.anchor_device_id, device_id(20));

        // A refused operation never counts as delivered, so the next exchange offers it again.
        let again = reverted.pull(&mut space.member).await;
        assert_eq!(again.len(), 1);
        assert!(matches!(again[0], StoreOutcome::Stored { .. }));
        assert_eq!(
            title(&mut reverted, "note-late").await.as_deref(),
            Some("Late")
        );

        let mut direct = Replica::new(3, 312).await;
        direct.pull(&mut space.hub).await;
        take(&mut direct, &chain).await;
        take(&mut direct, &late).await;
        assert_eq!(reverted.domain_dump().await, direct.domain_dump().await);
        assert!(late_changes(&mut reverted).await.is_empty());
        for replica in [&mut reverted, &mut direct] {
            replica.check_invariants().await;
        }
    });
}

#[test]
fn a_forked_anchor_continues_its_log_through_a_handover_to_itself() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 311).await;
        let mut member = Replica::new(2, 311).await;
        deliver(&mut member, &mut anchor).await;
        let base = view(&mut member).await.revision;
        let mut copy = anchor.clone_device().await;
        anchor
            .sequence(enroll_entry(&person(), 21, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        copy.sequence(enroll_entry(&person(), 22, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        deliver(&mut member, &mut anchor).await;
        cross(&mut member, &mut copy).await;
        deliver(&mut anchor, &mut member).await;
        let frozen = view(&mut anchor).await;
        assert_eq!(frozen.frozen_at, Some(base));

        // The anchor's own admin key hands the anchor to the same device in the next epoch,
        // which lifts the freeze there and on every member.
        let signed = anchor.handover_proposal(&anchor_admin(&person())).await;
        anchor.seal_handover(&signed, None).await.unwrap().unwrap();
        let continued = view(&mut anchor).await;
        assert_eq!(
            (continued.epoch, continued.frozen_at),
            (frozen.epoch + 1, None)
        );
        assert_eq!(continued.anchor_device_id, ANCHOR_DEVICE_ID);
        anchor
            .sequence(enroll_entry(&person(), 23, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        deliver(&mut member, &mut anchor).await;
        assert_eq!(view(&mut member).await, view(&mut anchor).await);
    });
}
