//! Deletion holds for domain Undo and sealing split into batches.

use sqlx::Connection;

use super::block_on;
use super::fixture::{ITEMS_TABLE, MEMOS_TABLE, edge_key};
use super::replica::{Replica, converge, rank};
use crate::error::failpoints;
use crate::hold::{self, MAX_HOLD_MS};
use crate::{MAX_OPS_PER_SEAL, SyncError};

/// Rows in the first seal of a large domain, enough to exceed one seal transaction.
const LARGE_SEAL_ROWS: u32 = 10_000;

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 5).await);
    }
    replicas
}

/// Two replicas sharing the sealed memo `memo-1`.
async fn shared_memo() -> Vec<Replica> {
    let mut replicas = replicas(2).await;
    replicas[0]
        .exec("INSERT INTO fixture_memos (id, label) VALUES ('memo-1', 'One')")
        .await;
    replicas[0].seal().await.unwrap();
    converge(&mut replicas).await;
    replicas
}

/// Deletes `memo-1` and holds the delete until `until_ms`, in one transaction like domain code.
async fn delete_held(replica: &mut Replica, until_ms: u64) {
    let manifest = replica.engine.manifest();
    let now = replica.now_ms;
    let mut tx = replica.conn.begin().await.unwrap();
    sqlx::query("DELETE FROM fixture_memos WHERE id = 'memo-1'")
        .execute(&mut *tx)
        .await
        .unwrap();
    hold::hold_deletes(&mut tx, manifest, MEMOS_TABLE, &["memo-1"], now, until_ms)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn release(replica: &mut Replica) -> bool {
    let now = replica.now_ms;
    hold::release_hold(&mut replica.conn, MEMOS_TABLE, &["memo-1"], now)
        .await
        .unwrap()
}

async fn memo_captures(replica: &mut Replica) -> i64 {
    replica
        .count("SELECT count(*) FROM sync_capture WHERE row_key = 'memo-1'")
        .await
}

#[test]
fn undo_inside_the_lease_restores_a_held_delete_without_a_tombstone() {
    block_on(async {
        let mut replicas = shared_memo().await;
        let author = &mut replicas[0];
        let start = author.now_ms;
        delete_held(author, start + 6 * MAX_HOLD_MS).await;
        assert_eq!(
            hold::next_hold_expiry(&mut author.conn).await.unwrap(),
            Some(start + MAX_HOLD_MS),
            "the lease is clamped"
        );
        assert_eq!(author.seal().await.unwrap().changes, 0);
        assert_eq!(memo_captures(author).await, 1, "a held delete stays local");

        author.now_ms = start + MAX_HOLD_MS - 1;
        assert!(release(author).await);
        author
            .exec("INSERT INTO fixture_memos (id, label) VALUES ('memo-1', 'One')")
            .await;
        assert_eq!(
            hold::next_hold_expiry(&mut author.conn).await.unwrap(),
            None,
            "the insert clears the hold"
        );
        assert_eq!(
            author
                .count("SELECT created FROM sync_capture WHERE row_key = 'memo-1'")
                .await,
            0,
            "the restored row is an ordinary write"
        );
        author.seal().await.unwrap();
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .scalar_text("SELECT label FROM fixture_memos WHERE id = 'memo-1'")
                    .await
                    .as_deref(),
                Some("One")
            );
            assert_eq!(
                replica.count("SELECT count(*) FROM sync_tombstones").await,
                0
            );
        }
    });
}

#[test]
fn an_expired_hold_seals_the_delete_and_refuses_undo() {
    block_on(async {
        let mut replicas = shared_memo().await;
        let author = &mut replicas[0];
        let start = author.now_ms;
        delete_held(author, start + 1_000).await;
        author.now_ms = start + 1_000;
        assert!(!release(author).await, "Undo refuses an expired hold");
        assert_eq!(author.seal().await.unwrap().changes, 1);
        assert!(
            !author
                .try_exec("INSERT INTO fixture_memos (id, label) VALUES ('memo-1', 'One')")
                .await,
            "a sealed delete cannot be undone by inserting the key again"
        );
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(replica.count("SELECT count(*) FROM fixture_memos").await, 0);
        }
    });
}

#[test]
fn holding_refuses_a_table_that_does_not_hold_deletes() {
    block_on(async {
        let mut replica = Replica::new(0, 5).await;
        let manifest = replica.engine.manifest();
        let now = replica.now_ms;
        let refused = hold::hold_deletes(
            &mut replica.conn,
            manifest,
            ITEMS_TABLE,
            &["item-1"],
            now,
            now,
        )
        .await;
        assert!(
            matches!(refused, Err(SyncError::InvalidRequest(_))),
            "{refused:?}"
        );
    });
}

#[test]
fn a_remote_change_seals_a_held_delete_early() {
    block_on(async {
        let mut replicas = shared_memo().await;
        let start = replicas[0].now_ms;
        delete_held(&mut replicas[0], start + MAX_HOLD_MS).await;
        replicas[1]
            .exec("UPDATE fixture_memos SET label = 'Two' WHERE id = 'memo-1'")
            .await;
        replicas[1].seal().await.unwrap();

        let known = replicas[0].stored_vector().await;
        let ops = replicas[1].ops_since(&known).await;
        let author = &mut replicas[0];
        author.receive(&ops).await;
        let before = author.snapshot().await;
        failpoints::arm("hold.before_seal");
        let crashed = author.apply_once().await;
        failpoints::clear();
        assert!(
            matches!(crashed, Err(SyncError::Failpoint("hold.before_seal"))),
            "{crashed:?}"
        );
        assert!(author.snapshot().await == before, "the crash left changes");

        let report = author.apply_once().await.unwrap();
        assert!(
            report.seal.is_some_and(|seal| seal.changes == 1),
            "the held delete seals before the remote change applies"
        );
        assert_eq!(memo_captures(author).await, 0);
        assert!(!release(author).await, "Undo refuses a sealed hold");
        converge(&mut replicas).await;
        let first = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, first);
    });
}

#[test]
fn a_large_first_seal_splits_into_batches_and_a_remote_batch_seals_its_rows_first() {
    block_on(async {
        let mut replicas = replicas(2).await;
        // The shared memo sorts after the generated ones, so the first batch leaves it.
        replicas[0]
            .exec("INSERT INTO fixture_memos (id, label) VALUES ('zz-shared', 'One')")
            .await;
        replicas[0].seal().await.unwrap();
        converge(&mut replicas).await;

        // Keys and labels long enough that the creates exceed one seal's byte budget.
        replicas[0]
            .exec(&format!(
                "UPDATE fixture_memos SET label = 'Local' WHERE id = 'zz-shared';
                 WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {LARGE_SEAL_ROWS})
                 INSERT INTO fixture_memos (id, label)
                 SELECT printf('memo-%031d', i), printf('%040d', i) FROM n;"
            ))
            .await;
        replicas[1]
            .exec("UPDATE fixture_memos SET label = 'Remote' WHERE id = 'zz-shared'")
            .await;
        replicas[1].seal().await.unwrap();

        let shared = "SELECT count(*) FROM sync_capture WHERE row_key = 'zz-shared'";
        let first = replicas[0].seal().await.unwrap();
        assert!(first.more, "one transaction cannot take every capture");
        assert!(first.sealed_ops <= MAX_OPS_PER_SEAL);
        assert_eq!(replicas[0].count(shared).await, 1);

        let known = replicas[0].stored_vector().await;
        let ops = replicas[1].ops_since(&known).await;
        let author = &mut replicas[0];
        author.receive(&ops).await;
        author.apply_once().await.unwrap();
        assert_eq!(
            author.count(shared).await,
            0,
            "the row the remote batch touches seals first"
        );
        assert!(
            author.count("SELECT count(*) FROM sync_capture").await > 0,
            "the other captures wait for their own batches"
        );

        let mut batches = 1;
        loop {
            let report = author.seal().await.unwrap();
            assert!(report.sealed_ops <= MAX_OPS_PER_SEAL);
            batches += 1;
            if !report.more {
                break;
            }
        }
        assert!(batches >= 2);
        assert_eq!(author.count("SELECT count(*) FROM sync_capture").await, 0);
        converge(&mut replicas).await;
        let expected = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, expected);
        assert_eq!(
            replicas[1]
                .count("SELECT count(*) FROM fixture_memos")
                .await,
            i64::from(LARGE_SEAL_ROWS) + 1
        );
    });
}

/// A local edit and a remote edit that together break a constraint over several rows: shared
/// setup, the unsealed local edit, and the remote edit.
struct SetCase {
    name: &'static str,
    setup: String,
    local: String,
    remote: String,
}

fn set_cases() -> Vec<SetCase> {
    let edge = |from: &str, to: &str| {
        format!(
            "INSERT INTO fixture_edges (from_id, to_id, sync_key) VALUES ('{from}', '{to}', '{}')",
            edge_key(from, to)
        )
    };
    let node = |id: &str, position: u32| {
        format!(
            "INSERT INTO fixture_nodes (id, parent_id, position, label)
             VALUES ('{id}', NULL, '{}', '{id}');",
            rank(position)
        )
    };
    vec![
        SetCase {
            name: "acyclic edges",
            setup: "INSERT INTO fixture_items (id, created_at, label, label_edits) VALUES
                 ('x', '2027-01-01T00:00:00.000Z', 'x', 0),
                 ('y', '2027-01-01T00:00:00.000Z', 'y', 0)"
                .to_string(),
            local: edge("x", "y"),
            remote: edge("y", "x"),
        },
        SetCase {
            name: "tree",
            setup: format!("{}{}", node("a", 0), node("b", 1)),
            local: "UPDATE fixture_nodes SET parent_id = 'b' WHERE id = 'a'".to_string(),
            remote: "UPDATE fixture_nodes SET parent_id = 'a' WHERE id = 'b'".to_string(),
        },
        SetCase {
            name: "unique label",
            setup: String::new(),
            local: "INSERT INTO fixture_cycle_a (id, label) VALUES ('a-2', 'Same')".to_string(),
            remote: "INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'Same')".to_string(),
        },
    ]
}

#[test]
fn a_remote_change_first_seals_unsealed_rows_that_a_constraint_over_its_table_reads() {
    block_on(async {
        for case in set_cases() {
            let mut replicas = replicas(2).await;
            if !case.setup.is_empty() {
                replicas[0].exec(&case.setup).await;
                replicas[0].seal().await.unwrap();
                converge(&mut replicas).await;
            }
            replicas[0].exec(&case.local).await;
            replicas[1].exec(&case.remote).await;
            replicas[1].seal().await.unwrap();

            let known = replicas[0].stored_vector().await;
            let ops = replicas[1].ops_since(&known).await;
            let author = &mut replicas[0];
            author.receive(&ops).await;
            author
                .apply_once()
                .await
                .unwrap_or_else(|error| panic!("{}: {error:?}", case.name));
            author.check_invariants().await;
            assert_eq!(
                author.count("SELECT count(*) FROM sync_capture").await,
                0,
                "{}: the local edit seals before the merge",
                case.name
            );
            converge(&mut replicas).await;
            let expected = replicas[0].dump().await;
            assert_eq!(replicas[1].dump().await, expected, "{}", case.name);
            replicas[1].check_invariants().await;
        }
    });
}
