//! Chains of apply groups: a chain applies only once it is closed, repairs its groups need are
//! kept across a restart between them and sealed once the chain applied or a cutoff ended it,
//! chains open one at a time with repairs sealed between them, in the order they were stored
//! whole rather than by clock, one writer's back-to-back chains both apply, colliding chains of
//! two writers end repaired on every replica, a replica that cannot seal defers nothing, even
//! once its writer froze mid-chain, deferred rows come back a bounded number at a time through
//! apply and seal, and a hold for a newer manifest keeps a chain waiting instead of closing it.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::bounds::MAX_OPERATIONS_PER_GROUP;

use super::block_on;
use super::exclusion::{deliver, edit};
use super::groups::{
    applied_seq, assert_converged, flagged_ops, insert_keyed_item, insert_note, new_ops, notes_of,
    replicas, reseal_chain, take,
};
use super::replica::{Replica, connection_from, converge, quoted};
use crate::exclusion::take_stale;
use crate::{ApplyReport, HoldReason, StoreOutcome};

/// Identity key the colliding items share.
const CLASH: &str = "local:clash";

/// Operations of a chain one operation past the group bound that `replica` seals: one
/// inserting the item `item` under [`CLASH`], then notes, each operation but the last marked as
/// continuing.
async fn clashing_chain(replica: &mut Replica, item: &str) -> Vec<Vec<u8>> {
    clashing_chain_of(replica, item, MAX_OPERATIONS_PER_GROUP + 1).await
}

/// Operations of a chain like [`clashing_chain`] with `notes` operations of notes.
async fn clashing_chain_of(replica: &mut Replica, item: &str, notes: usize) -> Vec<Vec<u8>> {
    let previous = replica
        .own_ops()
        .await
        .pop()
        .expect("the replica has sealed");
    let known = replica.stored_vector().await;
    let mut edits = vec![insert_keyed_item(item, CLASH)];
    edits.extend((0..notes).map(|index| insert_note(replica.index, index)));
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

/// Applies with no local writer, as a replica that cannot seal does.
async fn apply_without_writer(replica: &mut Replica) -> ApplyReport {
    replica
        .engine
        .apply(&mut replica.conn, &replica.ctx, None, replica.now_ms)
        .await
        .unwrap()
}

#[test]
fn chains_of_two_writers_open_one_at_a_time_and_repair_between() {
    block_on(async {
        let mut replicas = replicas(3).await;
        for (index, replica) in replicas.iter_mut().take(2).enumerate() {
            replica.exec(&insert_note(index, 9_999)).await;
        }
        converge(&mut replicas).await;
        let a = clashing_chain(&mut replicas[0], "music-a").await;
        let b = clashing_chain(&mut replicas[1], "music-b").await;
        let writers = [replicas[0].writer(), replicas[1].writer()];
        let receiver = &mut replicas[2];
        edit(receiver, &insert_keyed_item("music-r", CLASH)).await;
        let starts = [
            applied_seq(receiver, &writers[0]).await,
            applied_seq(receiver, &writers[1]).await,
        ];
        let ends = [
            starts[0] + u64::try_from(a.len()).unwrap(),
            starts[1] + u64::try_from(b.len()).unwrap(),
        ];
        let own = receiver.own_ops().await.len();
        receiver.receive(&a).await;
        receiver.receive(&b).await;

        // Each call leaves at most one chain between groups, and the second chain opens only
        // after the first one's deferred rows were repaired.
        let mut finished_first = None;
        for _ in 0..100 {
            let report = receiver.apply_once().await.unwrap();
            let mut between = 0;
            let mut done = 0;
            for (index, writer) in writers.iter().enumerate() {
                let seq = applied_seq(receiver, writer).await;
                between += usize::from(seq > starts[index] && seq < ends[index]);
                done += usize::from(seq == ends[index]);
            }
            assert!(between <= 1, "two chains are between groups at once");
            if done == 1 && finished_first.is_none() && stale_rows(receiver).await == 0 {
                finished_first = Some(receiver.own_ops().await.len());
            }
            if !report.more {
                break;
            }
        }
        let repaired = finished_first.expect("the first chain finishes before the second");
        assert!(
            repaired > own,
            "the first chain's repair is sealed before the second opens"
        );
        for (index, writer) in writers.iter().enumerate() {
            assert_eq!(applied_seq(receiver, writer).await, ends[index]);
        }
        assert_eq!(stale_rows(receiver).await, 0);
        assert_eq!(clashing(receiver).await, 1);
        receiver.check_invariants().await;
    });
}

#[test]
fn a_cutoff_after_the_first_group_ends_the_chain_and_its_deferral() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 311).await;
        let mut removed = Replica::new(1, 311).await;
        let mut honest = Replica::new(2, 311).await;
        edit(&mut removed, &insert_note(1, 9_999)).await;
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;
        edit(&mut honest, &insert_keyed_item("music-h", CLASH)).await;
        deliver(&mut anchor, &mut honest).await;
        let chain = clashing_chain(&mut removed, "music-a").await;
        let writer = removed.writer();
        let start = applied_seq(&mut honest, &writer).await;

        // The honest member stores the whole chain and applies its first group, which makes the
        // items collide; its repair waits for the rest.
        let own = honest.own_ops().await.len();
        honest.receive(&chain).await;
        let report = honest.apply_once().await.unwrap();
        assert_eq!(report.applied, MAX_OPERATIONS_PER_GROUP);
        assert!(stale_rows(&mut honest).await > 0);
        assert_eq!(honest.own_ops().await.len(), own);

        // The anchor stored only the first group when it removed the device, so the cutoff
        // ends the chain there and the rest never applies.
        anchor.receive(&chain[..MAX_OPERATIONS_PER_GROUP]).await;
        let device = removed.device_id.clone();
        anchor.remove(&device).await.unwrap();
        anchor.apply_all().await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        let cut = start + u64::try_from(MAX_OPERATIONS_PER_GROUP).unwrap();
        assert_eq!(applied_seq(&mut honest, &writer).await, cut);
        assert_eq!(applied_seq(&mut anchor, &writer).await, cut);
        assert_eq!(stale_rows(&mut honest).await, 0);
        deliver(&mut anchor, &mut honest).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(clashing(replica).await, 1, "replica {}", replica.index);
            assert_eq!(stale_rows(replica).await, 0);
            replica.check_invariants().await;
        }
        assert_eq!(anchor.domain_dump().await, honest.domain_dump().await);

        // The repair is sealed, so deleting the kept item does not bring back the other one.
        edit(
            &mut honest,
            &format!(
                "DELETE FROM music_library_items WHERE identity_key = {}",
                quoted(CLASH)
            ),
        )
        .await;
        deliver(&mut anchor, &mut honest).await;
        for replica in [&mut anchor, &mut honest] {
            assert_eq!(clashing(replica).await, 0, "replica {}", replica.index);
        }
    });
}

#[test]
fn a_replica_without_a_writer_defers_no_rows_for_a_chain() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&insert_note(0, 9_999)).await;
        converge(&mut replicas).await;
        let chain = clashing_chain(&mut replicas[0], "music-a").await;
        let writer = replicas[0].writer();
        let notes = notes_of(&mut replicas[0], 0).await;
        let receiver = &mut replicas[1];
        let end = applied_seq(receiver, &writer).await + u64::try_from(chain.len()).unwrap();
        receiver.receive(&chain).await;
        let first = apply_without_writer(receiver).await;
        assert_eq!(first.applied, MAX_OPERATIONS_PER_GROUP);
        assert_eq!(
            stale_rows(receiver).await,
            0,
            "a call that cannot seal defers nothing"
        );
        while apply_without_writer(receiver).await.more {}
        assert_eq!(applied_seq(receiver, &writer).await, end);
        assert_eq!(stale_rows(receiver).await, 0);
        assert_eq!(notes_of(receiver, 0).await, notes);
        assert_eq!(clashing(receiver).await, 1);
        receiver.check_invariants().await;
    });
}

#[test]
fn stale_rows_are_taken_back_a_bounded_number_at_a_time_in_order() {
    block_on(async {
        let mut replica = Replica::new(0, 313).await;
        replica
            .exec(
                "INSERT INTO sync_stale_rows (table_id, row_key)
                 VALUES (2, 'row-b'), (1, 'row-c'), (1, 'row-a')",
            )
            .await;
        let mut dirty = BTreeSet::new();
        assert!(take_stale(&mut replica.conn, &mut dirty, 2).await.unwrap());
        let first: Vec<_> = dirty
            .iter()
            .map(|(table, key)| (table.0, key.as_str()))
            .collect();
        assert_eq!(first, [(1, "row-a"), (1, "row-c")]);
        assert_eq!(stale_rows(&mut replica).await, 1);
        dirty.clear();
        assert!(!take_stale(&mut replica.conn, &mut dirty, 2).await.unwrap());
        let rest: Vec<_> = dirty
            .iter()
            .map(|(table, key)| (table.0, key.as_str()))
            .collect();
        assert_eq!(rest, [(2, "row-b")]);
        assert_eq!(stale_rows(&mut replica).await, 0);
    });
}

/// Flags of `chains` back-to-back chains of one writer, each one group past the group bound.
fn chain_flags(chains: usize) -> Vec<bool> {
    let mut chain = vec![true; MAX_OPERATIONS_PER_GROUP];
    chain.push(false);
    chain.repeat(chains)
}

#[test]
fn a_writer_with_low_clocks_cannot_keep_another_chain_from_opening() {
    block_on(async {
        let mut replicas = replicas(3).await;
        for (index, replica) in replicas.iter_mut().take(2).enumerate() {
            replica.exec(&insert_note(index, 9_999)).await;
        }
        converge(&mut replicas).await;
        // The hostile writer keeps its clocks low and sends a new chain each time the last one
        // applied; the honest writer's clocks run ten seconds ahead of it.
        let length = MAX_OPERATIONS_PER_GROUP + 1;
        let hostile = flagged_ops(&mut replicas[0], &chain_flags(3)).await;
        replicas[1].now_ms += 10_000;
        let honest = flagged_ops(&mut replicas[1], &chain_flags(1)).await;
        let writers = [replicas[0].writer(), replicas[1].writer()];
        let receiver = &mut replicas[2];
        receiver.now_ms += 10_000;
        let starts = [
            applied_seq(receiver, &writers[0]).await,
            applied_seq(receiver, &writers[1]).await,
        ];
        let step = u64::try_from(length).unwrap();
        let honest_end = starts[1] + step;
        receiver.receive(&hostile[..length]).await;
        receiver.now_ms += 1;
        receiver.receive(&honest).await;

        let mut sent = 1;
        let mut honest_done_after = None;
        for _ in 0..100 {
            if sent < 3 && applied_seq(receiver, &writers[0]).await == starts[0] + step * sent {
                receiver.now_ms += 1;
                let next = usize::try_from(sent).unwrap() * length;
                receiver.receive(&hostile[next..next + length]).await;
                sent += 1;
            }
            let report = receiver.apply_once().await.unwrap();
            let hostile_seq = applied_seq(receiver, &writers[0]).await;
            let honest_seq = applied_seq(receiver, &writers[1]).await;
            let hostile_between = (hostile_seq - starts[0]) % step != 0;
            let honest_between = honest_seq > starts[1] && honest_seq < honest_end;
            assert!(
                !(hostile_between && honest_between),
                "two chains are between groups at once"
            );
            if honest_seq == honest_end && honest_done_after.is_none() {
                honest_done_after = Some(hostile_seq);
            }
            if !report.more && sent == 3 {
                break;
            }
        }
        assert_eq!(
            honest_done_after,
            Some(starts[0] + step),
            "the honest chain opens once the hostile writer's first chain applied"
        );
        assert_eq!(
            applied_seq(receiver, &writers[0]).await,
            starts[0] + step * 3
        );
        assert_eq!(
            notes_of(receiver, 0).await,
            1 + 3 * i64::try_from(length).unwrap()
        );
        assert_eq!(
            notes_of(receiver, 1).await,
            1 + i64::try_from(length).unwrap()
        );
        receiver.check_invariants().await;
    });
}

#[test]
fn back_to_back_chains_of_one_writer_both_apply() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&insert_note(0, 9_999)).await;
        converge(&mut replicas).await;
        let ops = flagged_ops(&mut replicas[0], &chain_flags(2)).await;
        let writer = replicas[0].writer();
        let receiver = &mut replicas[1];
        let end = applied_seq(receiver, &writer).await + u64::try_from(ops.len()).unwrap();
        receiver.receive(&ops).await;
        let reports = receiver.apply_all().await.unwrap();
        assert_eq!(
            reports.iter().map(|report| report.applied).sum::<usize>(),
            ops.len()
        );
        assert_eq!(applied_seq(receiver, &writer).await, end);
        assert_eq!(
            notes_of(receiver, 0).await,
            1 + i64::try_from(ops.len()).unwrap()
        );
        assert_eq!(stale_rows(receiver).await, 0);
        receiver.check_invariants().await;
    });
}

#[test]
fn stale_rows_past_one_write_are_taken_back_over_bounded_writes() {
    block_on(async {
        let mut replicas = replicas(2).await;
        for index in 0..10 {
            replicas[0].exec(&insert_note(0, index)).await;
        }
        replicas[0].seal().await.unwrap();
        converge(&mut replicas).await;
        let replica = &mut replicas[1];
        let table = replica
            .engine
            .manifest()
            .table_named("quick_notes")
            .expect("the manifest replicates Quick notes")
            .id
            .0;
        let bound = crate::apply::MAX_STALE_ROWS_PER_WRITE;
        let listed = 2 * bound + 10;
        // Real notes and rows that no longer exist are listed alike.
        let keys = (0..10)
            .map(|index| format!("note-0-{index:04}"))
            .chain((10..listed).map(|index| format!("gone-{index:05}")));
        let values: Vec<_> = keys
            .map(|key| format!("({table}, {})", quoted(&key)))
            .collect();
        replica
            .exec(&format!(
                "INSERT INTO sync_stale_rows (table_id, row_key) VALUES {}",
                values.join(", ")
            ))
            .await;
        let before = replica.domain_dump().await;
        let left = |taken: usize| i64::try_from(listed - taken).unwrap();

        let report = replica.apply_once().await.unwrap();
        assert!(report.more, "rows remain after the apply");
        assert_eq!(stale_rows(replica).await, left(bound));
        let sealed = replica.seal().await.unwrap();
        assert!(sealed.more, "a seal reports the rows it left");
        assert_eq!(stale_rows(replica).await, left(2 * bound));
        let report = replica.apply_once().await.unwrap();
        assert!(!report.more);
        assert_eq!(stale_rows(replica).await, 0);
        assert_eq!(replica.domain_dump().await, before);
        replica.check_invariants().await;
    });
}

#[test]
fn a_frozen_local_writer_mid_chain_does_not_keep_other_chains_waiting() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 317).await;
        let mut first = Replica::new(1, 317).await;
        let mut second = Replica::new(2, 317).await;
        let mut member = Replica::new(3, 317).await;
        for writer in [&mut first, &mut second] {
            edit(writer, &insert_note(writer.index, 9_999)).await;
            deliver(&mut anchor, writer).await;
        }
        deliver(&mut member, &mut anchor).await;
        edit(&mut member, &insert_keyed_item("music-m", CLASH)).await;
        deliver(&mut anchor, &mut member).await;
        // Three groups, so the chain is still between groups once the removal applied.
        let chain =
            clashing_chain_of(&mut first, "music-a", 2 * MAX_OPERATIONS_PER_GROUP + 1).await;
        let other = flagged_ops(&mut second, &chain_flags(1)).await;
        let writers = [first.writer(), second.writer()];
        let starts = [
            applied_seq(&mut member, &writers[0]).await,
            applied_seq(&mut member, &writers[1]).await,
        ];
        let ends = [
            starts[0] + u64::try_from(chain.len()).unwrap(),
            starts[1] + u64::try_from(other.len()).unwrap(),
        ];

        // The member applies the first group of a colliding chain and defers its repair.
        member.receive(&chain).await;
        let report = member.apply_once().await.unwrap();
        assert_eq!(report.applied, MAX_OPERATIONS_PER_GROUP);
        assert!(stale_rows(&mut member).await > 0);
        member.receive(&other).await;

        // The anchor removes the member's device, which freezes its writer mid-chain.
        let device = member.device_id.clone();
        anchor.remove(&device).await.unwrap();
        anchor.apply_all().await.unwrap();
        let known = member.stored_vector().await;
        let entries = anchor.ops_since(&known).await;
        member.receive(&entries).await;
        let mut opened_beside = false;
        for _ in 0..20 {
            let before = applied_seq(&mut member, &writers[0]).await;
            let other_before = applied_seq(&mut member, &writers[1]).await;
            let report = member.apply_once().await.unwrap();
            let other_after = applied_seq(&mut member, &writers[1]).await;
            if before > starts[0] && before < ends[0] && other_before < other_after {
                opened_beside = true;
            }
            if !report.more {
                break;
            }
        }
        assert!(member.device_removed().await);
        assert!(
            opened_beside,
            "the other chain opens while the first is between groups"
        );
        for (index, writer) in writers.iter().enumerate() {
            assert_eq!(applied_seq(&mut member, writer).await, ends[index]);
        }
        assert_eq!(stale_rows(&mut member).await, 0);
        member.check_invariants().await;
    });
}
