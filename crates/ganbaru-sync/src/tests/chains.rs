//! Chains of apply groups: a chain applies only once it is closed, repairs its groups need are
//! kept across a restart between them and sealed once the chain applied, colliding chains of
//! two writers end repaired on every replica, and a hold for a newer manifest keeps a chain
//! waiting instead of closing it.

use ganbaru_sync_contracts::bounds::MAX_OPERATIONS_PER_GROUP;

use super::block_on;
use super::groups::{
    applied_seq, assert_converged, insert_keyed_item, insert_note, new_ops, notes_of, replicas,
    reseal_chain, take,
};
use super::replica::{Replica, connection_from, converge, quoted};
use crate::{HoldReason, StoreOutcome};

/// Identity key the colliding items share.
const CLASH: &str = "local:clash";

/// Operations of a chain past the group bound that `replica` seals: one inserting the item
/// `item` under [`CLASH`], then notes, each operation but the last marked as continuing.
async fn clashing_chain(replica: &mut Replica, item: &str) -> Vec<Vec<u8>> {
    let previous = replica
        .own_ops()
        .await
        .pop()
        .expect("the replica has sealed");
    let known = replica.stored_vector().await;
    let mut edits = vec![insert_keyed_item(item, CLASH)];
    edits.extend((0..=MAX_OPERATIONS_PER_GROUP).map(|index| insert_note(replica.index, index)));
    for sql in &edits {
        replica.exec(sql).await;
        assert_eq!(replica.seal().await.unwrap().sealed_ops, 1);
    }
    let ops = new_ops(replica, &known).await;
    assert_eq!(ops.len(), edits.len());
    let mut flags = vec![true; ops.len()];
    flags[ops.len() - 1] = false;
    reseal_chain(replica, &previous, &ops, &flags, |_, _| {})
}

async fn clashing(replica: &mut Replica) -> i64 {
    replica
        .count(&format!(
            "SELECT count(*) FROM music_library_items WHERE identity_key = {}",
            quoted(CLASH)
        ))
        .await
}

async fn stale_rows(replica: &mut Replica) -> i64 {
    replica.count("SELECT count(*) FROM sync_stale_rows").await
}

#[test]
fn a_restart_between_groups_of_a_closed_chain_keeps_its_repair() {
    block_on(async {
        let mut replicas = replicas(2).await;
        for (index, replica) in replicas.iter_mut().enumerate() {
            replica.exec(&insert_note(index, 9_999)).await;
        }
        converge(&mut replicas).await;
        let chain = clashing_chain(&mut replicas[0], "music-a").await;
        replicas[1].exec(&insert_keyed_item("music-b", CLASH)).await;
        replicas[1].seal().await.unwrap();

        let own = replicas[1].own_ops().await.len();
        replicas[1].receive(&chain).await;
        let report = replicas[1].apply_once().await.unwrap();
        assert_eq!(report.applied, MAX_OPERATIONS_PER_GROUP);
        assert!(report.more);
        // The first group made the items collide; the receiver hides one and keeps the rows.
        assert_eq!(clashing(&mut replicas[1]).await, 1);
        assert_eq!(replicas[1].own_ops().await.len(), own);
        assert!(stale_rows(&mut replicas[1]).await > 0);

        let snapshot = replicas[1].snapshot().await;
        replicas[1].conn = connection_from(&snapshot).await;
        replicas[1].apply_all().await.unwrap();
        let writer = replicas[0].writer();
        assert_eq!(
            applied_seq(&mut replicas[1], &writer).await,
            replicas[0].stored_vector().await.get(&writer)
        );
        assert!(
            replicas[1].own_ops().await.len() > own,
            "the repair is sealed once the chain applied"
        );
        assert_eq!(stale_rows(&mut replicas[1]).await, 0);
        assert_eq!(clashing(&mut replicas[1]).await, 1);
        replicas[1].check_invariants().await;
    });
}

#[test]
fn colliding_chains_of_two_writers_end_repaired_on_every_replica() {
    block_on(async {
        let mut replicas = replicas(5).await;
        for (index, replica) in replicas.iter_mut().take(2).enumerate() {
            replica.exec(&insert_note(index, 9_999)).await;
        }
        converge(&mut replicas).await;
        // The authors keep their own originals of the re-sealed chains, so only the other
        // replicas take part from here on.
        let a = clashing_chain(&mut replicas[0], "music-a").await;
        let b = clashing_chain(&mut replicas[1], "music-b").await;
        let receivers = &mut replicas[2..];
        let orders = [[&a, &b], [&b, &a]];
        for (receiver, order) in receivers.iter_mut().zip(orders) {
            for chain in order {
                take(receiver, chain).await;
            }
        }
        receivers[2].receive(&a).await;
        receivers[2].receive(&b).await;
        receivers[2].apply_all().await.unwrap();
        for receiver in receivers.iter_mut() {
            assert_eq!(clashing(receiver).await, 1, "replica {}", receiver.index);
            assert_eq!(stale_rows(receiver).await, 0);
        }
        converge(receivers).await;
        assert_converged(receivers).await;

        // The repair is sealed, so deleting the kept item does not bring back the other one.
        receivers[0]
            .exec(&format!(
                "DELETE FROM music_library_items WHERE identity_key = {}",
                quoted(CLASH)
            ))
            .await;
        receivers[0].seal().await.unwrap();
        converge(receivers).await;
        for receiver in receivers.iter_mut() {
            assert_eq!(clashing(receiver).await, 0, "replica {}", receiver.index);
            receiver.check_invariants().await;
        }
        assert_converged(receivers).await;
    });
}

#[test]
fn a_hold_for_a_newer_manifest_keeps_its_chain_waiting() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&insert_note(0, 9_999)).await;
        converge(&mut replicas).await;
        let writer = replicas[0].writer();
        let boundary = applied_seq(&mut replicas[1], &writer).await;
        let previous = replicas[0].own_ops().await.pop().unwrap();
        let known = replicas[0].stored_vector().await;
        for index in 0..3 {
            replicas[0].exec(&insert_note(0, index)).await;
            replicas[0].seal().await.unwrap();
        }
        let ops = new_ops(&mut replicas[0], &known).await;
        let forged = reseal_chain(
            &replicas[0],
            &previous,
            &ops,
            &[true, true, false],
            |index, op| {
                if index == 1 {
                    op.header.manifest_version += 1;
                }
            },
        );

        let outcomes = replicas[1].receive(&forged).await;
        assert_eq!(
            outcomes[1],
            StoreOutcome::Stored {
                held: Some(HoldReason::NewerManifest)
            }
        );
        let reports = replicas[1].apply_all().await.unwrap();
        assert!(reports.iter().all(|report| report.applied == 0));
        assert_eq!(applied_seq(&mut replicas[1], &writer).await, boundary);
        assert_eq!(notes_of(&mut replicas[1], 0).await, 1, "no part is visible");
        replicas[1].check_invariants().await;
    });
}
