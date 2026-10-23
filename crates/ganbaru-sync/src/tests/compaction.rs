//! Compaction: which bodies it drops, what it stores back first, and what readers do after.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ganbaru_sync_contracts::{RevokeReason, VersionVector, WriterId};

use super::block_on;
use super::replica::{Replica, converge};
use crate::error::failpoints;
use crate::manifest::vault::quick_notes::{NOTES_TABLE, note_group};
use crate::{COMPACTION_BATCH_OPS, SyncError, compaction_limit, merge};

/// Kinds of operations as stored.
const CHANGES: i64 = 0;
const GENESIS: i64 = 1;
const REVOKE: i64 = 2;
const ACKNOWLEDGEMENT: i64 = 3;

/// The stored vector of `replica`, as a limit that covers every operation it holds, so only
/// applied state and heads bound compaction.
async fn all_of(replica: &mut Replica) -> VersionVector {
    replica.stored_vector().await
}

async fn note(replica: &mut Replica, id: &str, title: &str) {
    replica
        .exec(&format!(
            "INSERT INTO quick_notes (id, title, order_key, body_plain_text)
             VALUES ('{id}', '{title}', 'a0', '')"
        ))
        .await;
    replica.seal().await.unwrap();
}

async fn retitle(replica: &mut Replica, id: &str, title: &str) {
    replica
        .exec(&format!(
            "UPDATE quick_notes SET title = '{title}' WHERE id = '{id}'"
        ))
        .await;
    replica.seal().await.unwrap();
}

/// Kind and body length of every stored operation of `writer`, by sequence.
async fn chain(replica: &mut Replica, writer: WriterId) -> Vec<(i64, i64)> {
    sqlx::query_as("SELECT kind, length(envelope) FROM sync_ops WHERE writer_id = ? ORDER BY seq")
        .bind(writer.as_bytes().as_slice())
        .fetch_all(&mut replica.conn)
        .await
        .unwrap()
}

async fn compacted_seq(replica: &mut Replica, writer: WriterId) -> u64 {
    replica
        .engine
        .compacted_vector(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
        .get(&writer)
}

/// Whether the stored value of the title of `note-1` written by `writer` is null.
async fn title_elided(replica: &mut Replica, writer: WriterId) -> bool {
    replica
        .count(&format!(
            "SELECT count(*) FROM sync_register_versions
             WHERE table_id = {} AND row_key = 'note-1' AND group_id = {} AND value IS NULL
                AND writer_id = x'{}'",
            NOTES_TABLE.0,
            note_group::TITLE.get(),
            writer.to_hex()
        ))
        .await
        > 0
}

/// Titles of `note-1` the engine retains.
async fn retained_titles(replica: &mut Replica) -> BTreeSet<String> {
    merge::versions(&replica.engine, &mut replica.conn, NOTES_TABLE, "note-1")
        .await
        .unwrap()
        .into_iter()
        .filter(|version| version.group == note_group::TITLE)
        .map(|version| format!("{:?}", version.value.fields()))
        .collect()
}

/// Two replicas that both applied `note-1` from replica 0, retitled twice so its writer has
/// changes below its head.
async fn shared_note() -> Vec<Replica> {
    let mut replicas = vec![Replica::new(0, 21).await, Replica::new(1, 21).await];
    note(&mut replicas[0], "note-1", "One").await;
    note(&mut replicas[0], "note-2", "Two").await;
    retitle(&mut replicas[0], "note-2", "Other").await;
    converge(&mut replicas).await;
    replicas
}

#[test]
fn compaction_drops_applied_changes_and_acknowledgements_below_the_head() {
    block_on(async {
        let mut a = Replica::new(0, 21).await;
        let mut b = Replica::new(1, 21).await;
        note(&mut a, "note-1", "One").await;
        retitle(&mut a, "note-1", "Two").await;
        b.pull(&mut a).await;
        b.engine
            .acknowledge(&mut b.conn, &b.ctx, &mut b.identity.local(), b.now_ms)
            .await
            .unwrap();
        note(&mut b, "note-2", "Three").await;
        a.pull(&mut b).await;
        // A kind this format reserves keeps its body wherever it sits.
        a.exec(&format!(
            "UPDATE sync_ops SET kind = 4 WHERE writer_id = x'{}' AND seq = 2",
            a.writer().to_hex()
        ))
        .await;

        let limit = all_of(&mut a).await;
        let report = a.compact(&limit, COMPACTION_BATCH_OPS).await.unwrap();
        assert_eq!((report.compacted, report.batches), (1, 1));
        assert!(!report.more);
        let own = a.writer();
        let other = b.writer();
        let own_chain = chain(&mut a, own).await;
        assert_eq!(
            own_chain.iter().map(|op| op.0).collect::<Vec<_>>(),
            [GENESIS, 4, CHANGES]
        );
        assert!(
            own_chain.iter().all(|op| op.1 > 0),
            "the head keeps its body"
        );
        assert_eq!(compacted_seq(&mut a, own).await, 2);
        let other_chain = chain(&mut a, other).await;
        assert_eq!(
            other_chain.iter().map(|op| op.0).collect::<Vec<_>>(),
            [GENESIS, ACKNOWLEDGEMENT, CHANGES]
        );
        assert!(other_chain[0].1 > 0, "a genesis keeps its body");
        assert_eq!(other_chain[1].1, 0, "an acknowledgement is compacted");
        assert!(other_chain[2].1 > 0);
        assert_eq!(compacted_seq(&mut a, other).await, 2);

        // A second run finds nothing new.
        let again = a.compact(&limit, COMPACTION_BATCH_OPS).await.unwrap();
        assert_eq!((again.compacted, again.batches), (0, 0));
    });
}

#[test]
fn compaction_keeps_revocations_and_stops_at_the_limit_and_what_was_applied() {
    block_on(async {
        let mut hub = Replica::new(0, 22).await;
        let mut member = Replica::new(1, 22).await;
        note(&mut member, "note-1", "One").await;
        retitle(&mut member, "note-1", "Two").await;
        hub.pull(&mut member).await;
        let revoked = member.writer();
        let cutoff = hub.stored_vector().await.get(&revoked);
        hub.engine
            .seal_revoke(
                &mut hub.conn,
                &hub.ctx,
                revoked,
                cutoff,
                RevokeReason::Revoked,
                &mut hub.identity.local(),
                hub.now_ms,
            )
            .await
            .unwrap();
        note(&mut hub, "note-2", "Hub").await;
        retitle(&mut hub, "note-2", "Hub again").await;
        let own = hub.writer();
        assert_eq!(
            chain(&mut hub, own)
                .await
                .iter()
                .map(|op| op.0)
                .collect::<Vec<_>>(),
            [GENESIS, REVOKE, CHANGES, CHANGES]
        );

        // The limit stops at the revocation, so no change of the hub is compacted yet.
        let mut limit = VersionVector::new();
        limit.set(own, 2);
        let report = hub.compact(&limit, COMPACTION_BATCH_OPS).await.unwrap();
        assert_eq!(report.compacted, 0);
        assert_eq!(compacted_seq(&mut hub, own).await, 2);
        let limit = all_of(&mut hub).await;
        hub.compact(&limit, COMPACTION_BATCH_OPS).await.unwrap();
        let kept = chain(&mut hub, own).await;
        assert!(kept[1].1 > 0, "a revocation keeps its body");
        assert_eq!(kept[2].1, 0);
        assert_eq!(compacted_seq(&mut hub, own).await, 3);

        // Operations a replica stores but has not applied are never compacted.
        let mut late = Replica::new(2, 22).await;
        let ops = member.ops_since(&VersionVector::new()).await;
        late.receive(&ops).await;
        let limit = all_of(&mut late).await;
        let report = late.compact(&limit, COMPACTION_BATCH_OPS).await.unwrap();
        assert_eq!((report.compacted, report.batches), (0, 0));
    });
}

#[test]
fn a_peer_behind_a_compacted_sequence_is_told_to_bootstrap() {
    block_on(async {
        let mut replicas = shared_note().await;
        let source = replicas[0].writer();
        let limit = all_of(&mut replicas[1]).await;
        replicas[1]
            .compact(&limit, COMPACTION_BATCH_OPS)
            .await
            .unwrap();
        let compacted = compacted_seq(&mut replicas[1], source).await;
        assert!(compacted > 1);

        let server = &mut replicas[1];
        let behind = server
            .engine
            .ops_after(
                &mut server.conn,
                &server.ctx,
                &VersionVector::new(),
                1 << 20,
            )
            .await;
        match behind {
            Err(SyncError::Compacted { writer, seq }) => {
                assert_eq!((writer, seq), (source, compacted));
            }
            other => panic!("expected a compacted refusal, got {other:?}"),
        }

        // A peer holding everything through the compacted sequence is served the rest.
        let mut known = VersionVector::new();
        known.set(source, compacted);
        let page = server
            .engine
            .ops_after(&mut server.conn, &server.ctx, &known, 1 << 20)
            .await
            .unwrap();
        assert!(!page.ops.is_empty());
        let current = server.stored_vector().await;
        let page = server
            .engine
            .ops_after(&mut server.conn, &server.ctx, &current, 1 << 20)
            .await
            .unwrap();
        assert!(page.ops.is_empty());
    });
}

#[test]
fn an_empty_body_above_the_compacted_sequence_is_corrupt() {
    block_on(async {
        let mut replicas = shared_note().await;
        let source = replicas[0].writer();
        let server = &mut replicas[1];
        server
            .exec(&format!(
                "UPDATE sync_ops SET envelope = x'' WHERE writer_id = x'{}' AND seq = 2",
                source.to_hex()
            ))
            .await;
        let read = server
            .engine
            .ops_after(
                &mut server.conn,
                &server.ctx,
                &VersionVector::new(),
                1 << 20,
            )
            .await;
        assert!(matches!(read, Err(SyncError::Corrupt(_))), "{read:?}");
        let limit = all_of(server).await;
        let compacted = server
            .engine
            .compact(&mut server.conn, &server.ctx, &limit)
            .await;
        assert!(
            matches!(compacted, Err(SyncError::Corrupt(_))),
            "{compacted:?}"
        );

        // A waiting operation without a body cannot be applied.
        let mut late = Replica::new(2, 21).await;
        let ops = replicas[0].ops_since(&VersionVector::new()).await;
        late.receive(&ops).await;
        late.exec(&format!(
            "UPDATE sync_ops SET envelope = x'' WHERE writer_id = x'{}' AND seq = 2",
            source.to_hex()
        ))
        .await;
        assert!(matches!(
            late.apply_once().await,
            Err(SyncError::Corrupt(_))
        ));
    });
}

#[test]
fn an_elided_winner_whose_operation_lost_its_body_is_corrupt() {
    block_on(async {
        let mut replicas = shared_note().await;
        let source = replicas[0].writer();
        let server = &mut replicas[1];
        assert!(title_elided(server, source).await);
        // The row no longer matches, so reading the winner needs the operation body.
        server
            .exec(
                "UPDATE quick_notes SET title = 'Drifted' WHERE id = 'note-1';
                 DELETE FROM sync_capture;",
            )
            .await;
        server
            .exec(&format!(
                "UPDATE sync_ops SET envelope = x'' WHERE writer_id = x'{}' AND seq = 2",
                source.to_hex()
            ))
            .await;
        let read = merge::versions(&server.engine, &mut server.conn, NOTES_TABLE, "note-1").await;
        assert!(matches!(read, Err(SyncError::Corrupt(_))), "{read:?}");
    });
}

#[test]
fn compaction_stores_back_a_winner_whose_row_no_longer_matches() {
    block_on(async {
        let mut replicas = shared_note().await;
        let source = replicas[0].writer();
        let server = &mut replicas[1];
        let before = retained_titles(server).await;
        server
            .exec(
                "UPDATE quick_notes SET title = 'Drifted' WHERE id = 'note-1';
                 DELETE FROM sync_capture;",
            )
            .await;
        let limit = all_of(server).await;
        let report = server.compact(&limit, COMPACTION_BATCH_OPS).await.unwrap();
        assert!(report.hydrated > 0);
        assert!(!title_elided(server, source).await);
        assert_eq!(retained_titles(server).await, before);
    });
}

#[test]
fn compaction_stores_back_a_winner_with_a_pending_capture_and_the_edit_still_seals() {
    block_on(async {
        let mut replicas = shared_note().await;
        let source = replicas[0].writer();
        let editor = &mut replicas[1];
        editor
            .exec("UPDATE quick_notes SET title = 'Unsealed' WHERE id = 'note-1'")
            .await;
        assert_eq!(editor.count("SELECT count(*) FROM sync_capture").await, 1);
        let before = retained_titles(editor).await;
        let limit = all_of(editor).await;
        editor.compact(&limit, COMPACTION_BATCH_OPS).await.unwrap();
        assert!(!title_elided(editor, source).await);
        assert_eq!(retained_titles(editor).await, before);
        assert_eq!(editor.seal().await.unwrap().changes, 1);
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .scalar_text("SELECT title FROM quick_notes WHERE id = 'note-1'")
                    .await
                    .as_deref(),
                Some("Unsealed")
            );
            replica.check_compacted().await;
        }
    });
}

#[test]
fn compaction_stores_back_a_winner_whose_row_was_deleted_and_recovery_reads_it() {
    block_on(async {
        let mut replicas = shared_note().await;
        let source = replicas[0].writer();
        // A concurrent edit of another field outlives the deletion, so recovery presents the
        // title whose operation body the deleting replica dropped.
        replicas[0]
            .exec("UPDATE quick_notes SET color = 3 WHERE id = 'note-1'")
            .await;
        replicas[0].seal().await.unwrap();
        let editor = &mut replicas[1];
        editor
            .exec("DELETE FROM quick_notes WHERE id = 'note-1'")
            .await;
        let limit = all_of(editor).await;
        editor.compact(&limit, COMPACTION_BATCH_OPS).await.unwrap();
        assert!(!title_elided(editor, source).await);
        editor.seal().await.unwrap();
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM quick_notes WHERE id = 'note-1'")
                    .await,
                0
            );
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].presentation.title, "One");
            replica.check_compacted().await;
        }
    });
}

#[test]
fn a_matching_winner_is_stored_back_and_never_elided_again_after_compaction() {
    block_on(async {
        let mut replicas = shared_note().await;
        let source = replicas[0].writer();
        let server = &mut replicas[1];
        assert!(title_elided(server, source).await);
        let limit = all_of(server).await;
        server.compact(&limit, COMPACTION_BATCH_OPS).await.unwrap();
        assert!(!title_elided(server, source).await);

        // Concurrent edits after compaction merge over the stored-back winner on both sides.
        replicas[0]
            .exec("UPDATE quick_notes SET title = 'From A' WHERE id = 'note-1'")
            .await;
        replicas[1]
            .exec("UPDATE quick_notes SET title = 'From B' WHERE id = 'note-1'")
            .await;
        converge(&mut replicas).await;
        let mut titles = Vec::new();
        for replica in &mut replicas {
            replica.check_compacted().await;
            assert_eq!(retained_titles(replica).await.len(), 2);
            titles.push(
                replica
                    .scalar_text("SELECT title FROM quick_notes WHERE id = 'note-1'")
                    .await,
            );
        }
        assert_eq!(titles[0], titles[1]);
        let domain = replicas[0].domain_dump().await;
        assert_eq!(replicas[1].domain_dump().await, domain);
    });
}

#[test]
fn a_crash_between_batches_leaves_a_compacted_prefix() {
    block_on(async {
        let mut replicas = vec![Replica::new(0, 23).await, Replica::new(1, 23).await];
        for index in 0..12 {
            note(&mut replicas[0], &format!("note-{index}"), "Draft").await;
        }
        converge(&mut replicas).await;
        let source = replicas[0].writer();
        let server = &mut replicas[1];
        let limit = all_of(server).await;

        // A zero budget stops after the first transaction.
        let first = server
            .engine
            .compact_with(&mut server.conn, &server.ctx, &limit, 4, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(first.batches, 1);
        assert!(first.more);
        server.check_compacted().await;
        let prefix = compacted_seq(server, source).await;
        assert!(prefix > 0 && prefix < 12);

        let before = server.snapshot().await;
        failpoints::arm("compact_batch");
        let crashed = server
            .engine
            .compact_with(&mut server.conn, &server.ctx, &limit, 4, Duration::MAX)
            .await;
        failpoints::clear();
        assert!(matches!(
            crashed,
            Err(SyncError::Failpoint("compact_batch"))
        ));
        assert_eq!(server.snapshot().await, before, "a failed batch rolls back");
        assert_eq!(compacted_seq(server, source).await, prefix);

        let rest = server.compact(&limit, 4).await.unwrap();
        assert!(!rest.more);
        assert_eq!(compacted_seq(server, source).await, 12);
        let empty: Vec<i64> = chain(server, source)
            .await
            .iter()
            .enumerate()
            .filter(|(_, op)| op.1 == 0)
            .map(|(index, _)| index as i64 + 1)
            .collect();
        assert_eq!(empty, (2..=12).collect::<Vec<_>>());
    });
}

#[test]
fn a_client_caps_its_own_writer_at_the_confirmed_head_so_a_fork_reseals() {
    block_on(async {
        let mut original = Replica::new(1, 24).await;
        let mut hub = Replica::new(0, 24).await;
        note(&mut original, "note-1", "One").await;
        retitle(&mut original, "note-1", "Two").await;
        retitle(&mut original, "note-1", "Three").await;
        hub.pull(&mut original).await;
        let mut clone = original.clone_installation(1).await;
        retitle(&mut original, "note-1", "Original").await;
        hub.pull(&mut original).await;
        retitle(&mut clone, "note-1", "Forked").await;
        retitle(&mut clone, "note-1", "Forked again").await;
        let old = clone.writer();

        // The coordinator holds sequence 5 of the shared writer, with another hash.
        let stable = hub.stored_vector().await;
        assert_eq!(stable.get(&old), 5);
        let own = BTreeSet::from([old]);
        let capped = compaction_limit(&stable, &own, Some((old, 4)));
        assert_eq!(capped.get(&old), 4);
        assert_eq!(compaction_limit(&stable, &own, None).get(&old), 0);
        assert_eq!(compaction_limit(&stable, &BTreeSet::new(), None), stable);

        let mut uncapped = clone.clone_installation(1).await;
        clone.compact(&capped, COMPACTION_BATCH_OPS).await.unwrap();
        assert_eq!(compacted_seq(&mut clone, old).await, 4);
        clone.rotate();
        let report = clone
            .engine
            .reseal_fork(
                &mut clone.conn,
                &clone.ctx,
                old,
                4,
                &mut clone.identity.local(),
                clone.now_ms,
            )
            .await
            .unwrap();
        assert_eq!(report.replaced_ops, 2);
        clone.pull(&mut hub).await;
        hub.pull(&mut clone).await;
        clone.check_compacted().await;

        // Compacting the forked sequence would leave nothing to re-seal it from.
        uncapped
            .compact(&stable, COMPACTION_BATCH_OPS)
            .await
            .unwrap();
        assert_eq!(compacted_seq(&mut uncapped, old).await, 5);
        uncapped.rotate();
        let refused = uncapped
            .engine
            .reseal_fork(
                &mut uncapped.conn,
                &uncapped.ctx,
                old,
                4,
                &mut uncapped.identity.local(),
                uncapped.now_ms,
            )
            .await;
        assert!(
            matches!(refused, Err(SyncError::InvalidReseal(_))),
            "{refused:?}"
        );
    });
}

#[test]
fn the_stable_vector_is_recorded_and_replaced() {
    block_on(async {
        let mut replicas = shared_note().await;
        let source = replicas[0].writer();
        let other = replicas[1].writer();
        let replica = &mut replicas[1];
        let mut stable = VersionVector::new();
        stable.set(source, 3);
        stable.set(other, 1);
        replica
            .engine
            .record_stable(&mut replica.conn, &replica.ctx, &stable)
            .await
            .unwrap();
        let read = replica
            .engine
            .stable_vector(&mut replica.conn, &replica.ctx)
            .await
            .unwrap();
        assert_eq!(read, stable);
        let mut smaller = VersionVector::new();
        smaller.set(source, 2);
        replica
            .engine
            .record_stable(&mut replica.conn, &replica.ctx, &smaller)
            .await
            .unwrap();
        let read = replica
            .engine
            .stable_vector(&mut replica.conn, &replica.ctx)
            .await
            .unwrap();
        assert_eq!(read, smaller);
    });
}

/// Measures one full batch, for the budgets the plan records: the time of a transaction of
/// [`COMPACTION_BATCH_OPS`] operations and the bytes each compacted operation frees.
#[test]
fn compaction_of_a_full_batch_reports_its_time_and_freed_bytes() {
    block_on(async {
        let mut writer = Replica::new(0, 25).await;
        note(&mut writer, "note-1", "Start").await;
        for index in 1..COMPACTION_BATCH_OPS {
            retitle(&mut writer, "note-1", &format!("Title {index}")).await;
        }
        let mut server = Replica::new(1, 25).await;
        server.pull(&mut writer).await;
        let bodies = "SELECT coalesce(sum(length(envelope)), 0) FROM sync_ops";
        let before = server.count(bodies).await;
        let limit = all_of(&mut server).await;
        let started = Instant::now();
        let report = server
            .engine
            .compact_with(
                &mut server.conn,
                &server.ctx,
                &limit,
                COMPACTION_BATCH_OPS,
                Duration::MAX,
            )
            .await
            .unwrap();
        let elapsed = started.elapsed();
        assert_eq!(report.compacted, COMPACTION_BATCH_OPS - 1);
        assert_eq!(report.batches, 1);
        let freed = before - server.count(bodies).await;
        eprintln!(
            "compacted {} operations in {elapsed:?}, freeing {} bytes each",
            report.compacted,
            freed / i64::try_from(report.compacted).unwrap()
        );
        server.check_compacted().await;
    });
}
