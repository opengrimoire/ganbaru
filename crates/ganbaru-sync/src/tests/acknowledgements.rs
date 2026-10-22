//! Acknowledgements: when a writer seals one, and that acknowledgement traffic settles.

use std::fmt::Write as _;

use ganbaru_sync_contracts::bounds::MAX_DEPENDENCIES;
use ganbaru_sync_contracts::{Content, Envelope, VersionVector, WriterId};

use super::block_on;
use super::replica::{Identity, Replica, device_id};
use crate::acknowledge::{ACKNOWLEDGEMENT_INTERVAL_MS, KEEPALIVE_INTERVAL_MS};
use crate::{SealReport, StoreOutcome, StoreRefusal};

const MINUTE_MS: u64 = 60 * 1000;

async fn acknowledge(replica: &mut Replica) -> SealReport {
    replica
        .engine
        .acknowledge(
            &mut replica.conn,
            &replica.ctx,
            &mut replica.identity.local(),
            replica.now_ms,
        )
        .await
        .unwrap()
}

fn hex(writer: WriterId) -> String {
    writer
        .as_bytes()
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// Stored operations of `writer` of `kind`.
async fn count_kind(replica: &mut Replica, writer: WriterId, kind: u8) -> i64 {
    replica
        .count(&format!(
            "SELECT count(*) FROM sync_ops WHERE writer_id = x'{}' AND kind = {kind}",
            hex(writer)
        ))
        .await
}

/// Stored operations of the local writer of `kind`.
async fn own_kind(replica: &mut Replica, kind: u8) -> i64 {
    let writer = replica.writer();
    count_kind(replica, writer, kind).await
}

async fn total_ops(replica: &mut Replica) -> i64 {
    replica.count("SELECT count(*) FROM sync_ops").await
}

async fn edit(replica: &mut Replica, sql: &str) {
    replica.exec(sql).await;
    replica.seal().await.unwrap();
}

/// Stores and applies everything `from` has that `to` lacks.
async fn deliver(to: &mut Replica, from: &mut Replica) {
    to.pull(from).await;
    to.apply_all().await.unwrap();
}

async fn tagged_pair() -> (Replica, Replica) {
    let mut a = Replica::new(0, 1).await;
    let b = Replica::new(1, 1).await;
    edit(
        &mut a,
        "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0')",
    )
    .await;
    (a, b)
}

#[test]
fn acknowledging_seals_the_genesis_and_acknowledges_changes_of_other_writers() {
    block_on(async {
        let (mut a, mut b) = tagged_pair().await;
        deliver(&mut b, &mut a).await;
        let report = acknowledge(&mut b).await;
        assert_eq!((report.sealed_ops, report.last_seq), (2, Some(2)));
        assert_eq!(own_kind(&mut b, 1).await, 1);
        assert_eq!(own_kind(&mut b, 3).await, 1);
        let head = b.applied_vector().await;
        assert_eq!(head.get(&a.writer()), a.applied_vector().await.get(&a.writer()));

        // The genesis of B is not an acknowledgement, so A acknowledges it once.
        deliver(&mut a, &mut b).await;
        assert_eq!(acknowledge(&mut a).await.sealed_ops, 1);
        assert_eq!(own_kind(&mut a, 3).await, 1);

        // Only the acknowledgement of A is new to B, which never answers it.
        b.now_ms += ACKNOWLEDGEMENT_INTERVAL_MS;
        deliver(&mut b, &mut a).await;
        assert_eq!(acknowledge(&mut b).await, SealReport::default());
        assert_eq!(own_kind(&mut b, 3).await, 1);
    });
}

#[test]
fn acknowledgement_traffic_settles_across_many_passes() {
    block_on(async {
        let (mut a, mut b) = tagged_pair().await;
        for _ in 0..3 {
            deliver(&mut b, &mut a).await;
            acknowledge(&mut b).await;
            deliver(&mut a, &mut b).await;
            acknowledge(&mut a).await;
        }
        let settled = total_ops(&mut a).await;
        let passes: u64 = 1000;
        let step = (KEEPALIVE_INTERVAL_MS - MINUTE_MS) / passes;
        for _ in 0..passes {
            a.now_ms += step;
            b.now_ms += step;
            deliver(&mut b, &mut a).await;
            assert_eq!(acknowledge(&mut b).await.sealed_ops, 0);
            deliver(&mut a, &mut b).await;
            assert_eq!(acknowledge(&mut a).await.sealed_ops, 0);
        }
        assert_eq!(total_ops(&mut a).await, settled);
        assert_eq!(total_ops(&mut b).await, settled);
    });
}

#[test]
fn an_acknowledgement_waits_out_the_interval_since_the_previous_one() {
    block_on(async {
        let (mut a, mut b) = tagged_pair().await;
        deliver(&mut b, &mut a).await;
        acknowledge(&mut b).await;
        let first = b.now_ms;

        edit(&mut a, "UPDATE quick_note_tags SET name = 'Home' WHERE id = 'tag-1'").await;
        b.now_ms = first + ACKNOWLEDGEMENT_INTERVAL_MS - 1;
        deliver(&mut b, &mut a).await;
        let before = total_ops(&mut b).await;
        assert_eq!(acknowledge(&mut b).await, SealReport::default());
        assert_eq!(total_ops(&mut b).await, before);

        b.now_ms = first + ACKNOWLEDGEMENT_INTERVAL_MS;
        assert_eq!(acknowledge(&mut b).await.sealed_ops, 1);
        assert_eq!(own_kind(&mut b, 3).await, 2);
    });
}

#[test]
fn a_keepalive_acknowledgement_follows_a_quiet_week() {
    block_on(async {
        let mut a = Replica::new(0, 1).await;
        let report = acknowledge(&mut a).await;
        assert_eq!((report.sealed_ops, report.last_seq), (1, Some(1)));
        let genesis_at = a.now_ms;

        a.now_ms = genesis_at + KEEPALIVE_INTERVAL_MS - 1;
        assert_eq!(acknowledge(&mut a).await, SealReport::default());
        a.now_ms = genesis_at + KEEPALIVE_INTERVAL_MS;
        assert_eq!(acknowledge(&mut a).await.last_seq, Some(2));
        assert_eq!(own_kind(&mut a, 3).await, 1);
        assert_eq!(acknowledge(&mut a).await, SealReport::default());
    });
}

#[test]
fn acknowledging_more_writers_than_a_header_lists_seals_consecutive_acknowledgements() {
    block_on(async {
        let minted = MAX_DEPENDENCIES + MAX_DEPENDENCIES / 2;
        let mut minter = Replica::new(9, 1).await;
        for writer in 0..minted {
            let mut seed = [0u8; 32];
            seed[..8].copy_from_slice(&(writer as u64).to_le_bytes());
            seed[31] = 2;
            minter.identity =
                Identity::new(seed, &minter.person, &device_id(100 + writer), None);
            minter.seal().await.unwrap();
        }
        let geneses = minter.ops_since(&VersionVector::new()).await;
        let mut a = Replica::new(0, 1).await;
        a.receive(&geneses).await;
        a.apply_all().await.unwrap();

        let report = acknowledge(&mut a).await;
        assert_eq!((report.sealed_ops, report.last_seq), (3, Some(3)));
        let mut dependencies = Vec::new();
        for bytes in a.ops_since(&VersionVector::new()).await {
            let operation = Envelope::decode(&bytes).unwrap().operation().unwrap();
            if operation.writer == a.writer() && operation.seq > 1 {
                assert_eq!(operation.content, Content::Acknowledgement);
                dependencies.push(operation.header.dependencies.len());
            }
        }
        assert_eq!(dependencies, vec![MAX_DEPENDENCIES, minted - MAX_DEPENDENCIES]);
        assert_eq!(acknowledge(&mut a).await, SealReport::default());
    });
}

#[test]
fn a_fork_reseals_acknowledgements_as_acknowledgements() {
    block_on(async {
        let (mut a, mut other) = tagged_pair().await;
        let mut hub = Replica::new(2, 1).await;
        let mut clone = a.clone_installation(3).await;
        edit(&mut a, "UPDATE quick_note_tags SET name = 'Original' WHERE id = 'tag-1'").await;
        edit(
            &mut other,
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-2', 'Home', 'a1')",
        )
        .await;
        deliver(&mut clone, &mut other).await;
        assert_eq!(acknowledge(&mut clone).await.last_seq, Some(3));
        let old = clone.writer();

        hub.pull(&mut a).await;
        let forked = clone.ops_since(&VersionVector::new()).await;
        let outcomes = hub.receive(&forked).await;
        assert!(
            outcomes.contains(&StoreOutcome::Refused(StoreRefusal::Fork { seq: 3 })),
            "{outcomes:?}"
        );
        clone.rotate();
        let report = clone
            .engine
            .reseal_fork(
                &mut clone.conn,
                &clone.ctx,
                old,
                2,
                &mut clone.identity.local(),
                clone.now_ms,
            )
            .await
            .unwrap();
        assert_eq!(report.replaced_ops, 1);
        let new = clone.writer();
        assert_eq!(count_kind(&mut clone, new, 3).await, 1);
        assert_eq!(count_kind(&mut clone, new, 0).await, 0);
    });
}
