//! Deletion and insert holds for domain Undo and sealing split into batches.

use ganbaru_sync_contracts::{Change, ChangeAction, Content, Envelope, RowKey, TableId};
use sqlx::Connection;

use super::block_on;
use super::fixture::{
    CYCLE_A_TABLE, CYCLE_B_TABLE, ITEMS_TABLE, MEMO_LINES_TABLE, MEMO_TAGS_TABLE, MEMOS_TABLE,
    TEST_MANIFEST, edge_key, memo_tag_key,
};
use super::replica::{Replica, connection_from, converge, forge_change, quoted, rank};
use crate::domains::themes::PALETTE_SIZE;
use crate::error::failpoints;
use crate::hold::{self, MAX_HOLD_MS};
use crate::manifest::MergeKind;
use crate::manifest::vault::chat::{
    PARTICIPANTS_TABLE, POLICY_REVISIONS_TABLE, PROFILE_REVISIONS_TABLE, PROFILES_TABLE,
    TEAMMATES_TABLE,
};
use crate::manifest::vault::contacts::{CARD_KEY, CARD_TABLE, CONTACTS_TABLE};
use crate::manifest::vault::distractions::{SETTINGS_KEY, SETTINGS_TABLE};
use crate::manifest::vault::themes::THEMES_TABLE;
use crate::{HoldReason, MAX_OPS_PER_SEAL, StoreOutcome, SyncError};

/// Rows in the first seal of a large domain, enough to exceed one seal transaction with keys of
/// the longest allowed length.
const LARGE_SEAL_ROWS: u32 = 6_000;

/// A timestamp every timestamp column accepts.
const WRITTEN_AT: &str = "2027-01-01T00:00:00.000Z";

/// A person public key of the length `contacts` requires.
const PERSON_KEY: &str = "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE";

/// A theme id outside the built-in themes.
const THEME_KEY: &str = "theme-shared";

/// Honest rows of the tables whose key CHECK narrows a row key, with keys it refuses.
const NARROWED_KEYS: [(TableId, &str, &[&str]); 4] = [
    (CARD_TABLE, CARD_KEY, &["x"]),
    (SETTINGS_TABLE, SETTINGS_KEY, &["other"]),
    (THEMES_TABLE, THEME_KEY, &["light", "dark"]),
    (CONTACTS_TABLE, PERSON_KEY, &["short", "contact-short"]),
];

/// A row key of every table whose rows no replicated operation deletes.
const NEVER_DELETED: [(TableId, &str); 7] = [
    (CARD_TABLE, CARD_KEY),
    (SETTINGS_TABLE, SETTINGS_KEY),
    (PARTICIPANTS_TABLE, "participant:reviewer"),
    (TEAMMATES_TABLE, "participant:reviewer"),
    (POLICY_REVISIONS_TABLE, "policy-0"),
    (PROFILES_TABLE, "access-profile:read-only"),
    (
        PROFILE_REVISIONS_TABLE,
        "access-profile-revision:read-only:1",
    ),
];

/// A key sorting before every key of [`NEVER_DELETED`], so a redirect to it is well formed.
const LOWEST_KEY: &str = "0";

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
    let now = replica.now_ms;
    let mut tx = replica.conn.begin().await.unwrap();
    sqlx::query("DELETE FROM fixture_memos WHERE id = 'memo-1'")
        .execute(&mut *tx)
        .await
        .unwrap();
    hold::hold_deletes(
        &replica.engine,
        &mut tx,
        MEMOS_TABLE,
        &["memo-1"],
        now,
        until_ms,
    )
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
        let now = replica.now_ms;
        let refused = hold::hold_deletes(
            &replica.engine,
            &mut replica.conn,
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
fn cascade_children_of_a_holding_table_may_hold_deletes() {
    let manifest = &*TEST_MANIFEST;
    for table in [MEMOS_TABLE, MEMO_LINES_TABLE, MEMO_TAGS_TABLE] {
        assert!(manifest.may_hold_deletes(table), "{table:?}");
    }
    for table in [ITEMS_TABLE, CYCLE_A_TABLE, CYCLE_B_TABLE] {
        assert!(!manifest.may_hold_deletes(table), "{table:?}");
    }
}

/// SQL inserting `memo-1` with two lines and a derived-key tag.
fn memo_with_children() -> String {
    format!(
        "INSERT INTO fixture_memos (id, label) VALUES ('memo-1', 'One');
         INSERT INTO fixture_memo_lines (id, memo_id, body) VALUES
             ('line-1', 'memo-1', 'First'), ('line-2', 'memo-1', 'Second');
         INSERT INTO fixture_memo_tags (memo_id, tag, sync_key) VALUES ('memo-1', 'red', '{}');",
        memo_tag_key("memo-1", "red")
    )
}

/// Two replicas sharing the sealed `memo-1` with its lines and tag.
async fn shared_memo_with_children() -> Vec<Replica> {
    let mut replicas = replicas(2).await;
    replicas[0].exec(&memo_with_children()).await;
    replicas[0].seal().await.unwrap();
    converge(&mut replicas).await;
    replicas
}

/// Captured deletes held until `until_ms`.
async fn held_until(replica: &mut Replica, until_ms: u64) -> i64 {
    replica
        .count(&format!(
            "SELECT count(*) FROM sync_capture WHERE deleted = 1 AND held_until_ms = {until_ms}"
        ))
        .await
}

/// Rows of `memo-1`, its lines, and its tags on `replica`.
async fn memo_rows(replica: &mut Replica) -> [i64; 3] {
    [
        replica.count("SELECT count(*) FROM fixture_memos").await,
        replica
            .count("SELECT count(*) FROM fixture_memo_lines")
            .await,
        replica
            .count("SELECT count(*) FROM fixture_memo_tags")
            .await,
    ]
}

#[test]
fn undo_inside_the_lease_restores_cascade_children_without_tombstones_or_removals() {
    block_on(async {
        let mut replicas = shared_memo_with_children().await;
        let author = &mut replicas[0];
        let start = author.now_ms;
        delete_held(author, start + MAX_HOLD_MS).await;
        assert_eq!(memo_rows(author).await, [0, 0, 0], "the delete cascades");
        assert_eq!(
            held_until(author, start + MAX_HOLD_MS).await,
            4,
            "the lines and the tag share the memo's lease"
        );
        assert_eq!(author.seal().await.unwrap().changes, 0);

        author.now_ms = start + MAX_HOLD_MS - 1;
        assert!(release(author).await);
        author.exec(&memo_with_children()).await;
        assert_eq!(
            author
                .count(
                    "SELECT count(*) FROM sync_capture
                     WHERE deleted = 1 OR created = 1 OR held_until_ms IS NOT NULL"
                )
                .await,
            0,
            "the restored rows are ordinary writes"
        );
        author.seal().await.unwrap();
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(memo_rows(replica).await, [1, 2, 1]);
            assert_eq!(
                replica.count("SELECT count(*) FROM sync_tombstones").await,
                0
            );
        }
        let first = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, first);
    });
}

#[test]
fn an_expired_hold_seals_cascade_children_with_the_held_row() {
    block_on(async {
        let mut replicas = shared_memo_with_children().await;
        let author = &mut replicas[0];
        let start = author.now_ms;
        delete_held(author, start + 1_000).await;
        author.now_ms = start + 999;
        assert_eq!(author.seal().await.unwrap().changes, 0);

        author.now_ms = start + 1_000;
        assert!(!release(author).await, "Undo refuses an expired hold");
        author.seal().await.unwrap();
        assert_eq!(
            author.count("SELECT count(*) FROM sync_capture").await,
            0,
            "the whole set seals together"
        );
        assert!(
            !author
                .try_exec(
                    "INSERT INTO fixture_memo_lines (id, memo_id, body)
                     VALUES ('line-1', 'memo-1', 'First')"
                )
                .await,
            "a sealed child delete cannot be undone by inserting the key again"
        );
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(memo_rows(replica).await, [0, 0, 0]);
            assert_eq!(
                replica
                    .count(&format!(
                        "SELECT count(*) FROM sync_tombstones
                         WHERE table_id = {} AND cause_row = 'memo-1'",
                        MEMO_LINES_TABLE.0
                    ))
                    .await,
                2,
                "the line tombstones name the memo as their cause"
            );
        }
    });
}

#[test]
fn a_remote_change_to_a_cascade_child_seals_the_whole_held_set_early() {
    block_on(async {
        let mut replicas = shared_memo_with_children().await;
        let start = replicas[0].now_ms;
        delete_held(&mut replicas[0], start + MAX_HOLD_MS).await;
        replicas[1]
            .exec("UPDATE fixture_memo_lines SET body = 'Edited' WHERE id = 'line-1'")
            .await;
        replicas[1].seal().await.unwrap();

        let known = replicas[0].stored_vector().await;
        let ops = replicas[1].ops_since(&known).await;
        let author = &mut replicas[0];
        author.receive(&ops).await;
        author.apply_once().await.unwrap();
        assert_eq!(
            author.count("SELECT count(*) FROM sync_capture").await,
            0,
            "the held memo seals with the line the remote change touched"
        );
        assert!(!release(author).await, "Undo refuses a sealed hold");
        converge(&mut replicas).await;
        let first = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, first);
        assert_eq!(memo_rows(&mut replicas[0]).await[0], 0);
    });
}

#[test]
fn undo_of_the_held_row_alone_publishes_the_deletes_of_its_children() {
    block_on(async {
        let mut replicas = shared_memo_with_children().await;
        let author = &mut replicas[0];
        let start = author.now_ms;
        delete_held(author, start + MAX_HOLD_MS).await;
        assert!(release(author).await);
        author
            .exec("INSERT INTO fixture_memos (id, label) VALUES ('memo-1', 'Restored')")
            .await;
        // Materializing the changed memo would bring back children that merge state still
        // holds live, so their held deletes seal with it.
        author.seal().await.unwrap();
        assert_eq!(author.count("SELECT count(*) FROM sync_capture").await, 0);
        assert_eq!(memo_rows(author).await, [1, 0, 0]);
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(memo_rows(replica).await, [1, 0, 0]);
        }
        let first = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, first);
    });
}

#[test]
fn undo_of_the_held_row_alone_with_its_old_values_seals_nothing_and_keeps_its_children_held() {
    block_on(async {
        let mut replicas = shared_memo_with_children().await;
        let author = &mut replicas[0];
        let start = author.now_ms;
        delete_held(author, start + MAX_HOLD_MS).await;
        assert!(release(author).await);
        author
            .exec("INSERT INTO fixture_memos (id, label) VALUES ('memo-1', 'One')")
            .await;
        let held = held_until(author, start + MAX_HOLD_MS).await;
        assert!(held > 0, "the children's deletes stay held");
        // The restored memo equals its uncontested winner, so nothing is sealed and the
        // children's deletes wait for their lease.
        assert_eq!(author.seal().await.unwrap().sealed_ops, 0);
        assert_eq!(memo_captures(author).await, 0);
        assert_eq!(held_until(author, start + MAX_HOLD_MS).await, held);
        assert_eq!(memo_rows(author).await, [1, 0, 0]);
    });
}

/// Inserts `memo-1` with its lines and tag and holds the inserts until `until_ms`, in one
/// transaction like a domain deletion that writes rows Undo removes again.
async fn create_held(replica: &mut Replica, until_ms: u64) {
    let now = replica.now_ms;
    let mut tx = replica.conn.begin().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(memo_with_children()))
        .execute(&mut *tx)
        .await
        .unwrap();
    hold::hold_creates(
        &replica.engine,
        &mut tx,
        MEMOS_TABLE,
        &["memo-1"],
        now,
        until_ms,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// Captured inserts held until `until_ms`.
async fn held_creates(replica: &mut Replica, until_ms: u64) -> i64 {
    replica
        .count(&format!(
            "SELECT count(*) FROM sync_capture
             WHERE created = 1 AND deleted = 0 AND held_until_ms = {until_ms}"
        ))
        .await
}

#[test]
fn held_inserts_removed_inside_the_lease_publish_nothing() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let author = &mut replicas[0];
        let start = author.now_ms;
        create_held(author, start + MAX_HOLD_MS).await;
        assert_eq!(
            held_creates(author, start + MAX_HOLD_MS).await,
            4,
            "the lines and the tag share the memo's lease"
        );
        assert_eq!(author.seal().await.unwrap().changes, 0);

        author.now_ms = start + MAX_HOLD_MS / 2;
        author
            .exec("DELETE FROM fixture_memos WHERE id = 'memo-1'")
            .await;
        assert_eq!(author.seal().await.unwrap().changes, 0);
        author.now_ms = start + MAX_HOLD_MS;
        assert_eq!(author.seal().await.unwrap().changes, 0);
        assert_eq!(author.count("SELECT count(*) FROM sync_capture").await, 0);
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(memo_rows(replica).await, [0, 0, 0]);
            assert_eq!(replica.count("SELECT count(*) FROM sync_rows").await, 0);
            assert_eq!(
                replica.count("SELECT count(*) FROM sync_tombstones").await,
                0
            );
        }
    });
}

#[test]
fn held_inserts_publish_once_at_expiry_across_a_crash_and_a_restart() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let author = &mut replicas[0];
        let start = author.now_ms;
        create_held(author, start + 1_000).await;
        author.conn = connection_from(&author.snapshot().await).await;
        assert_eq!(held_creates(author, start + 1_000).await, 4);
        author.now_ms = start + 999;
        assert_eq!(author.seal().await.unwrap().changes, 0);

        author.now_ms = start + 1_000;
        let before = author.snapshot().await;
        failpoints::arm("hold.before_seal");
        let crashed = author.seal().await;
        failpoints::clear();
        assert!(
            matches!(crashed, Err(SyncError::Failpoint("hold.before_seal"))),
            "{crashed:?}"
        );
        assert!(author.snapshot().await == before, "the crash left changes");
        author.conn = connection_from(&before).await;
        assert_eq!(held_creates(author, start + 1_000).await, 4);

        assert_eq!(author.seal().await.unwrap().changes, 4);
        assert_eq!(author.count("SELECT count(*) FROM sync_capture").await, 0);
        assert_eq!(author.seal().await.unwrap().sealed_ops, 0);
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(memo_rows(replica).await, [1, 2, 1]);
        }
        let first = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, first);
    });
}

#[test]
fn holding_inserts_leaves_published_rows_and_refuses_tables_without_holds() {
    block_on(async {
        let mut replicas = shared_memo().await;
        let author = &mut replicas[0];
        let now = author.now_ms;
        author
            .exec("UPDATE fixture_memos SET label = 'Two' WHERE id = 'memo-1'")
            .await;
        hold::hold_creates(
            &author.engine,
            &mut author.conn,
            MEMOS_TABLE,
            &["memo-1"],
            now,
            now + MAX_HOLD_MS,
        )
        .await
        .unwrap();
        assert_eq!(
            author
                .count("SELECT count(*) FROM sync_capture WHERE held_until_ms IS NOT NULL")
                .await,
            0,
            "a published row's write is not held"
        );
        assert_eq!(author.seal().await.unwrap().changes, 1);
        let refused = hold::hold_creates(
            &author.engine,
            &mut author.conn,
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
                 SELECT printf('memo-%0123d', i), printf('%040d', i) FROM n;"
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

/// Inserts [`LARGE_SEAL_ROWS`] unique labels into `fixture_cycle_a`, more captures than one
/// seal transaction takes.
fn large_cycle_a() -> String {
    format!(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {LARGE_SEAL_ROWS})
         INSERT INTO fixture_cycle_a (id, label)
         SELECT printf('c-%0126d', i), printf('%040d', i) FROM n;"
    )
}

#[test]
fn a_remote_create_sees_an_unsealed_rename_that_frees_its_unique_label() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec("INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'A')")
            .await;
        replicas[0].seal().await.unwrap();
        replicas[1]
            .exec("INSERT INTO fixture_cycle_a (id, label) VALUES ('a-3', 'A')")
            .await;
        replicas[1].seal().await.unwrap();
        replicas[0]
            .exec("UPDATE fixture_cycle_a SET label = 'B' WHERE id = 'a-1'")
            .await;

        let known = replicas[0].stored_vector().await;
        let ops = replicas[1].ops_since(&known).await;
        let author = &mut replicas[0];
        author.receive(&ops).await;
        author.apply_once().await.unwrap();
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica.count("SELECT count(*) FROM sync_tombstones").await,
                0,
                "the rename seals before the duplicate repair"
            );
            assert_eq!(
                replica.count("SELECT count(*) FROM fixture_cycle_a").await,
                2
            );
        }
        let expected = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, expected);
    });
}

#[test]
fn a_seal_in_batches_takes_a_rename_before_the_create_that_reuses_its_label() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec("INSERT INTO fixture_cycle_a (id, label) VALUES ('a-0', 'Free')")
            .await;
        replicas[0].seal().await.unwrap();
        converge(&mut replicas).await;
        replicas[0]
            .exec(&format!(
                "UPDATE fixture_cycle_a SET label = 'Taken' WHERE id = 'a-0';
                 INSERT INTO fixture_cycle_a (id, label) VALUES ('b-reuse', 'Free');
                 {}",
                large_cycle_a()
            ))
            .await;
        let mut batches = 0;
        loop {
            let report = replicas[0].seal().await.unwrap();
            assert!(report.sealed_ops <= MAX_OPS_PER_SEAL);
            batches += 1;
            if !report.more {
                break;
            }
        }
        assert!(batches >= 2, "the captures exceed one seal transaction");
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica.count("SELECT count(*) FROM sync_tombstones").await,
                0
            );
            assert_eq!(
                replica
                    .count(
                        "SELECT count(*) FROM fixture_cycle_a
                         WHERE (id, label) IN (VALUES ('a-0', 'Taken'), ('b-reuse', 'Free'))"
                    )
                    .await,
                2
            );
        }
        let expected = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, expected);
    });
}

#[test]
fn apply_over_too_many_constrained_captures_waits_for_batched_seals_and_then_progresses() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&large_cycle_a()).await;
        let remote = |label: &str| {
            format!("INSERT INTO fixture_cycle_a (id, label) VALUES ('r-{label}', '{label}')")
        };
        replicas[1].exec(&remote("first")).await;
        replicas[1].seal().await.unwrap();
        let known = replicas[0].stored_vector().await;
        let ops = replicas[1].ops_since(&known).await;
        replicas[0].receive(&ops).await;

        let captures = "SELECT count(*) FROM sync_capture";
        let before = replicas[0].snapshot().await;
        let report = replicas[0].apply_once().await.unwrap();
        assert!(
            report.pending_captures,
            "one apply cannot seal every capture"
        );
        assert!(
            replicas[0].snapshot().await == before,
            "an apply over the bound rolls back"
        );

        // The caller seals one batch and applies again, and a remote operation arrives while
        // the captures drain. Every round seals captures, so the loop ends.
        let mut rounds = 0;
        loop {
            let report = replicas[0].apply_once().await.unwrap();
            if !report.pending_captures {
                break;
            }
            let left = replicas[0].count(captures).await;
            replicas[0].seal().await.unwrap();
            assert!(
                replicas[0].count(captures).await < left,
                "a seal drains captures"
            );
            if rounds == 0 {
                replicas[1].exec(&remote("second")).await;
                replicas[1].seal().await.unwrap();
                let known = replicas[0].stored_vector().await;
                let ops = replicas[1].ops_since(&known).await;
                replicas[0].receive(&ops).await;
            }
            rounds += 1;
            assert!(rounds < 100, "apply never progressed");
        }
        assert!(rounds >= 1);
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM fixture_cycle_a WHERE id IN ('r-first', 'r-second')")
                .await,
            2
        );
        converge(&mut replicas).await;
        let expected = replicas[0].dump().await;
        assert_eq!(replicas[1].dump().await, expected);
        assert_eq!(
            replicas[1]
                .count("SELECT count(*) FROM sync_tombstones")
                .await,
            0
        );
    });
}

/// Two replicas sharing one sealed row of every table in [`NARROWED_KEYS`].
async fn narrowed_rows() -> Vec<Replica> {
    let mut replicas = replicas(2).await;
    let writer = &mut replicas[1];
    let now = writer.now_ms;
    writer
        .engine
        .seed_captures(&mut writer.conn, now)
        .await
        .unwrap();
    writer
        .exec(&format!(
            "INSERT INTO contact_card (key, card_nonce, updated_at)
             VALUES ('{CARD_KEY}', 'AAAAAAAAAAAAAAAAAAAAAA', '{WRITTEN_AT}');
             INSERT INTO contacts (id, public_key, display_name, state, created_at, updated_at)
             VALUES ('contact-{PERSON_KEY}', '{PERSON_KEY}', 'Ana', 'active', '{WRITTEN_AT}',
                     '{WRITTEN_AT}');
             INSERT INTO themes
                 (id, display_name, blend_canvas, seed_blend_canvas, derivation_engine_version,
                  created_at_ms, updated_at_ms, icon_label, seed_icon_label)
             VALUES ('{THEME_KEY}', 'Ocean', '#101010', '#101010', 1, {now}, {now}, 'dark', 'dark');
             WITH RECURSIVE slot(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM slot WHERE n < {last})
             INSERT INTO theme_event_palette (theme_id, slot, value)
             SELECT '{THEME_KEY}', n, '#202020' FROM slot;
             WITH RECURSIVE slot(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM slot WHERE n < {last})
             INSERT INTO theme_seed_event_palette (theme_id, slot, value)
             SELECT '{THEME_KEY}', n, '#202020' FROM slot;",
            last = PALETTE_SIZE - 1,
        ))
        .await;
    converge(&mut replicas).await;
    replicas
}

/// The create `replica` sealed for `key` of `table`.
pub(super) async fn sealed_create(replica: &mut Replica, table: TableId, key: &str) -> Change {
    replica
        .own_ops()
        .await
        .iter()
        .filter_map(|bytes| {
            match Envelope::decode(bytes)
                .unwrap()
                .operation()
                .unwrap()
                .content
            {
                Content::Changes(changes) => Some(changes),
                _ => None,
            }
        })
        .flatten()
        .find(|change| {
            change.table == table
                && change.row.as_str() == key
                && change.action == ChangeAction::Create
        })
        .unwrap_or_else(|| panic!("the row {key} is sealed"))
}

/// `create` as a change of `action`, valid for its own row.
fn with_action(create: &Change, action: ChangeAction) -> Change {
    let table = TEST_MANIFEST.table(create.table).unwrap();
    let mut change = create.clone();
    change.action = action;
    match action {
        ChangeAction::Create => {}
        ChangeAction::Write => change
            .groups
            .retain(|value| table.group(value.group).unwrap().kind != MergeKind::Immutable),
        ChangeAction::Tombstone => change.groups.clear(),
    }
    change
}

/// Every action of a signed change names a key: a row the schema refuses could never apply, so
/// replicas hold the operation instead of passing it on, and a tombstone gets no further.
#[test]
fn key_rule_refused_for_each_action() {
    block_on(async {
        let mut replicas = narrowed_rows().await;
        for (table_id, honest, refused_keys) in NARROWED_KEYS {
            let table = TEST_MANIFEST.table(table_id).unwrap();
            let create = sealed_create(&mut replicas[1], table_id, honest).await;
            for action in [
                ChangeAction::Create,
                ChangeAction::Write,
                ChangeAction::Tombstone,
            ] {
                // A derived-key row has no tombstone, whatever its key.
                if action == ChangeAction::Tombstone && table.derived_key.is_some() {
                    continue;
                }
                let mut change = with_action(&create, action);
                assert_eq!(
                    crate::validate::change(&replicas[1].engine, &change),
                    Ok(()),
                    "{action:?} of {} under its own key",
                    table.name
                );
                for key in refused_keys {
                    change.row = RowKey::new(*key).unwrap();
                    let forged = forge_change(&mut replicas[1], change.clone()).await;
                    let mut receiver = replicas[0].clone_device().await;
                    assert_eq!(
                        receiver.receive(&[forged]).await,
                        vec![StoreOutcome::Stored {
                            held: Some(HoldReason::Invalid)
                        }],
                        "{action:?} of {} under key {key}",
                        table.name
                    );
                    receiver.apply_all().await.unwrap();
                    assert_eq!(
                        receiver
                            .count(&format!(
                                "SELECT count(*) FROM {} WHERE {} = {}",
                                table.name,
                                table.key_column,
                                quoted(key)
                            ))
                            .await,
                        0,
                        "{action:?} of {} under key {key} applied",
                        table.name
                    );
                }
            }
        }
    });
}

/// A forged tombstone of a table no operation deletes from is held by its bytes alone, with or
/// without a redirect, whether or not the row exists on the receiver.
#[test]
fn tombstone_of_a_never_deleted_table_is_invalid() {
    block_on(async {
        let mut replicas = narrowed_rows().await;
        for (table, key) in NEVER_DELETED {
            for replaced_by in [None, Some(LOWEST_KEY)] {
                let change = Change {
                    table,
                    row: RowKey::new(key).unwrap(),
                    action: ChangeAction::Tombstone,
                    groups: Vec::new(),
                    replaced_by: replaced_by.map(|target| RowKey::new(target).unwrap()),
                    cause: None,
                };
                let forged = forge_change(&mut replicas[1], change).await;
                let mut receiver = replicas[0].clone_device().await;
                assert_eq!(
                    receiver.receive(&[forged]).await,
                    vec![StoreOutcome::Stored {
                        held: Some(HoldReason::Invalid)
                    }],
                    "tombstone of {key} replaced by {replaced_by:?}"
                );
            }
        }
    });
}

/// The keys domains write pass the rules, so honest rows keep replicating.
#[test]
fn seeded_and_derived_keys_pass_their_rules() {
    block_on(async {
        let mut replicas = narrowed_rows().await;
        for (table_id, honest, _) in NARROWED_KEYS {
            let table = TEST_MANIFEST.table(table_id).unwrap();
            assert!(table.key_rule.admits(honest), "{}", table.name);
            let create = sealed_create(&mut replicas[1], table_id, honest).await;
            assert_eq!(
                crate::validate::change(&replicas[1].engine, &create),
                Ok(())
            );
            assert_eq!(
                replicas[0]
                    .count(&format!(
                        "SELECT count(*) FROM {} WHERE {} = {}",
                        table.name,
                        table.key_column,
                        quoted(honest)
                    ))
                    .await,
                1,
                "{} reached the other replica",
                table.name
            );
        }
    });
}
