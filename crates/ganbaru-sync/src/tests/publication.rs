//! Publication predicates, rows published with their parent, and capture seeding.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use ganbaru_sync_contracts::{Cause, Change, ChangeAction, RowKey, TableId, VersionVector};
use sqlx::Connection;

use super::block_on;
use super::engine::pull_without_writer;
use super::fixture::{
    CYCLE_A_TABLE, CYCLE_B_TABLE, DRAFT_NOTES_TABLE, DRAFT_PARTS_TABLE, DRAFTS_TABLE,
    FIXTURE_TABLE_IDS, ITEMS_TABLE, MEMBERSHIPS_TABLE, MEMOS_TABLE, membership_key,
};
use super::replica::{Replica, converge, pair};
use crate::apply::{Constrained, finish_sealed};
use crate::merge::{self, Batch};
use crate::seal::{SealReport, SealSlot};
use crate::visibility::{
    CascadeWalk, DeriveScope, Root, RootKind, Roots, RowRef, Visibility, WalkValue,
};
use crate::{SyncError, log, recovery, repair, seed, sql};

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 3).await);
    }
    replicas
}

/// Captured fixture rows as `table key created mask`, in table and key order. Vault domain
/// rows every vault seeds, such as the Music soundscape state, are left out.
async fn captures(replica: &mut Replica) -> Vec<String> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT table_id || ' ' || row_key || ' ' || created || ' ' || mask FROM sync_capture
         WHERE table_id >= ? ORDER BY table_id, row_key",
    )
    .bind(i64::from(FIXTURE_TABLE_IDS))
    .fetch_all(&mut replica.conn)
    .await
    .unwrap();
    rows
}

async fn ids(replica: &mut Replica, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut replica.conn)
        .await
        .unwrap()
}

#[test]
fn a_draft_publishes_with_its_parts_and_never_becomes_a_draft_again() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let author = &mut replicas[0];
        author
            .exec(
                "INSERT INTO fixture_drafts (id, title, published) VALUES ('draft-1', 'One', 0);
                 INSERT INTO fixture_draft_parts (id, draft_id, body)
                     VALUES ('part-1', 'draft-1', 'body');
                 UPDATE fixture_drafts SET title = 'Edited' WHERE id = 'draft-1';
                 UPDATE fixture_draft_parts SET body = 'edited' WHERE id = 'part-1';
                 INSERT INTO fixture_drafts (id, title, published) VALUES ('draft-2', 'Two', 0);
                 INSERT INTO fixture_draft_parts (id, draft_id, body)
                     VALUES ('part-2', 'draft-2', 'body');
                 DELETE FROM fixture_drafts WHERE id = 'draft-2';",
            )
            .await;
        assert!(
            captures(author).await.is_empty(),
            "a draft and its parts stay local"
        );

        author
            .exec("UPDATE fixture_drafts SET published = 1 WHERE id = 'draft-1'")
            .await;
        let created = |table: TableId, key: &str| format!("{} {key} 1 3", table.0);
        assert_eq!(
            captures(author).await,
            vec![
                created(DRAFTS_TABLE, "draft-1"),
                created(DRAFT_PARTS_TABLE, "part-1"),
            ],
            "publishing captures the draft and its parts with every group"
        );
        assert!(
            !author
                .try_exec("UPDATE fixture_drafts SET published = 0 WHERE id = 'draft-1'")
                .await,
            "a published draft cannot become a draft again"
        );
        author
            .exec(
                "INSERT INTO fixture_draft_parts (id, draft_id, body)
                     VALUES ('part-3', 'draft-1', 'later');",
            )
            .await;
        assert!(
            captures(author)
                .await
                .contains(&created(DRAFT_PARTS_TABLE, "part-3")),
            "a part added to a published draft publishes at once"
        );

        author.seal().await.unwrap();
        converge(&mut replicas).await;
        let reader = &mut replicas[1];
        assert_eq!(
            ids(reader, "SELECT id || ' ' || title FROM fixture_drafts").await,
            vec!["draft-1 Edited"]
        );
        assert_eq!(
            ids(
                reader,
                "SELECT id || ' ' || body FROM fixture_draft_parts ORDER BY id"
            )
            .await,
            vec!["part-1 edited", "part-3 later"]
        );

        reader
            .exec("DELETE FROM fixture_drafts WHERE id = 'draft-1'")
            .await;
        reader.seal().await.unwrap();
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM fixture_draft_parts")
                    .await,
                0
            );
        }
    });
}

#[test]
fn seeding_captures_published_rows_once_per_table() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let seeder = &mut replicas[0];
        seeder
            .exec(
                "INSERT INTO fixture_drafts (id, title, published) VALUES
                     ('draft-1', 'Sent', 1), ('draft-2', 'Unsent', 0);
                 INSERT INTO fixture_draft_parts (id, draft_id, body) VALUES
                     ('part-1', 'draft-1', 'sent'), ('part-2', 'draft-2', 'unsent');
                 INSERT INTO fixture_memos (id, label) VALUES ('memo-1', 'Memo');
                 DELETE FROM sync_capture;",
            )
            .await;
        let now = seeder.now_ms;
        let seeded = seeder
            .engine
            .seed_captures(&mut seeder.conn, now)
            .await
            .unwrap();
        for table in ["fixture_items", "fixture_drafts", "fixture_draft_parts"] {
            assert!(seeded.contains(&table), "{table} should be seeded");
        }
        assert!(
            !seeded.contains(&"quick_notes"),
            "the baseline records tables replicated from the start"
        );
        assert_eq!(
            captures(seeder).await,
            vec![
                format!("{} draft-1 1 3", DRAFTS_TABLE.0),
                format!("{} part-1 1 3", DRAFT_PARTS_TABLE.0),
                format!("{} memo-1 1 1", MEMOS_TABLE.0),
            ],
            "only published rows are seeded"
        );

        seeder.exec("DELETE FROM sync_capture").await;
        let again = seeder
            .engine
            .seed_captures(&mut seeder.conn, now)
            .await
            .unwrap();
        assert!(again.is_empty());
        assert!(captures(seeder).await.is_empty());

        seeder
            .exec(&format!(
                "DELETE FROM sync_seeded_tables WHERE table_id = {}",
                DRAFTS_TABLE.0
            ))
            .await;
        let reseeded = seeder
            .engine
            .seed_captures(&mut seeder.conn, now)
            .await
            .unwrap();
        assert_eq!(reseeded, vec!["fixture_drafts"]);
        assert_eq!(
            captures(seeder).await,
            vec![format!("{} draft-1 1 3", DRAFTS_TABLE.0)]
        );

        seeder
            .exec(&format!(
                "DELETE FROM sync_seeded_tables WHERE table_id IN ({}, {})",
                DRAFT_PARTS_TABLE.0, MEMOS_TABLE.0
            ))
            .await;
        seeder
            .engine
            .seed_captures(&mut seeder.conn, now)
            .await
            .unwrap();
        assert_eq!(captures(seeder).await.len(), 3);
        seeder.seal().await.unwrap();
        converge(&mut replicas).await;
        let reader = &mut replicas[1];
        assert_eq!(
            ids(reader, "SELECT id FROM fixture_drafts").await,
            vec!["draft-1"]
        );
        assert_eq!(
            ids(reader, "SELECT id FROM fixture_draft_parts").await,
            vec!["part-1"]
        );
        assert_eq!(
            ids(reader, "SELECT id FROM fixture_memos").await,
            vec!["memo-1"]
        );
    });
}

#[test]
fn seeding_refuses_a_pending_key_and_records_nothing() {
    block_on(async {
        let mut replica = Replica::new(0, 3).await;
        replica
            .exec(&format!(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('pending:item', '2027-01-01T00:00:00.000Z', 'Pending', 0);
                 DELETE FROM sync_seeded_tables WHERE table_id >= {}",
                ITEMS_TABLE.0
            ))
            .await;
        let now = replica.now_ms;
        let refused = replica.engine.seed_captures(&mut replica.conn, now).await;
        assert!(
            matches!(refused, Err(SyncError::PendingKey("fixture_items"))),
            "{refused:?}"
        );
        assert_eq!(
            replica
                .count(&format!(
                    "SELECT count(*) FROM sync_seeded_tables WHERE table_id >= {}",
                    ITEMS_TABLE.0
                ))
                .await,
            0,
            "a refused seeding records no table"
        );

        // A conversion pass replaces the placeholder and seeds in its own transaction.
        let manifest = replica.engine.manifest();
        let mut tx = replica.conn.begin().await.unwrap();
        sqlx::raw_sql(
            "DELETE FROM fixture_items WHERE id = 'pending:item';
             INSERT INTO fixture_items (id, created_at, label, label_edits)
                 VALUES ('item-1', '2027-01-01T00:00:00.000Z', 'Pending', 0);",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        let seeded = seed::seed_captures(&mut tx, manifest, now).await.unwrap();
        tx.commit().await.unwrap();
        assert!(seeded.contains(&"fixture_items"));
        assert!(
            captures(&mut replica)
                .await
                .contains(&format!("{} item-1 1 3", ITEMS_TABLE.0))
        );
    });
}

#[test]
fn a_child_naming_no_parent_publishes_at_once_and_a_named_parent_holds_it() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let author = &mut replicas[0];
        author
            .exec(
                "INSERT INTO fixture_drafts (id, title, published) VALUES ('draft-1', 'One', 0);
                 INSERT INTO fixture_draft_notes (id, draft_id, body) VALUES
                     ('note-free', NULL, 'free'), ('note-held', 'draft-1', 'held');
                 UPDATE fixture_draft_notes SET body = 'edited' WHERE id = 'note-held';",
            )
            .await;
        let created = |table: TableId, key: &str| format!("{} {key} 1 3", table.0);
        assert_eq!(
            captures(author).await,
            vec![created(DRAFT_NOTES_TABLE, "note-free")],
            "a note naming no draft publishes at once and a note naming a draft waits"
        );
        author.seal().await.unwrap();
        converge(&mut replicas).await;
        assert_eq!(
            ids(&mut replicas[1], "SELECT id FROM fixture_draft_notes").await,
            vec!["note-free"]
        );

        let author = &mut replicas[0];
        author
            .exec("UPDATE fixture_drafts SET published = 1 WHERE id = 'draft-1'")
            .await;
        assert_eq!(
            captures(author).await,
            vec![
                created(DRAFTS_TABLE, "draft-1"),
                created(DRAFT_NOTES_TABLE, "note-held"),
            ],
            "publishing the draft captures the note naming it"
        );
        author.seal().await.unwrap();
        converge(&mut replicas).await;
        assert_eq!(
            ids(
                &mut replicas[1],
                "SELECT id || ' ' || body FROM fixture_draft_notes ORDER BY id"
            )
            .await,
            vec!["note-free free", "note-held edited"]
        );

        replicas[1]
            .exec("DELETE FROM fixture_drafts WHERE id = 'draft-1'")
            .await;
        replicas[1].seal().await.unwrap();
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                ids(replica, "SELECT id FROM fixture_draft_notes").await,
                vec!["note-free"],
                "deleting the draft deletes its notes and keeps the free one"
            );
        }
    });
}

#[test]
fn seeding_captures_a_child_naming_no_parent() {
    block_on(async {
        let mut replica = Replica::new(0, 3).await;
        let now = replica.now_ms;
        replica
            .engine
            .seed_captures(&mut replica.conn, now)
            .await
            .unwrap();
        replica
            .exec(&format!(
                "INSERT INTO fixture_drafts (id, title, published) VALUES
                     ('draft-1', 'Sent', 1), ('draft-2', 'Unsent', 0);
                 INSERT INTO fixture_draft_notes (id, draft_id, body) VALUES
                     ('note-free', NULL, 'free'), ('note-sent', 'draft-1', 'sent'),
                     ('note-unsent', 'draft-2', 'unsent');
                 DELETE FROM sync_capture;
                 DELETE FROM sync_seeded_tables WHERE table_id = {};",
                DRAFT_NOTES_TABLE.0
            ))
            .await;
        let seeded = replica
            .engine
            .seed_captures(&mut replica.conn, now)
            .await
            .unwrap();
        assert_eq!(seeded, vec!["fixture_draft_notes"]);
        assert_eq!(
            captures(&mut replica).await,
            vec![
                format!("{} note-free 1 3", DRAFT_NOTES_TABLE.0),
                format!("{} note-sent 1 3", DRAFT_NOTES_TABLE.0),
            ]
        );
    });
}

/// Rows of the hidden chain below its root.
const CHAIN_ROWS: usize = 256;

/// Rows of the in-memory chain, more than recursion could walk on a test thread's stack.
const LONG_CHAIN_ROWS: usize = 262_144;

/// A row of a chain that alternates between the cycle tables. Row 0 is the root.
fn chain_row(index: usize) -> RowRef {
    let table = if index.is_multiple_of(2) {
        CYCLE_A_TABLE
    } else {
        CYCLE_B_TABLE
    };
    (table, format!("chain-{index:03}"))
}

#[test]
fn cascade_roots_walks_each_row_once() {
    block_on(async {
        let mut replica = Replica::new(0, 3).await;
        let mut rows = String::new();
        for index in 0..=CHAIN_ROWS {
            let table = if chain_row(index).0 == CYCLE_A_TABLE {
                "fixture_cycle_a"
            } else {
                "fixture_cycle_b"
            };
            let peer = index.checked_sub(1).map_or("NULL".to_string(), |below| {
                format!("'{}'", chain_row(below).1)
            });
            rows.push_str(&format!(
                "INSERT INTO {table} (id, label, peer_id) VALUES ('{}', 'label-{index}', {peer});\n",
                chain_row(index).1
            ));
        }
        replica.exec(&rows).await;
        replica.seal().await.unwrap();
        // The tombstone of a group whose cascaded rows have not been deleted yet.
        let (root_table, root_key) = chain_row(0);
        replica
            .exec(&format!(
                "UPDATE sync_rows SET state = 'tombstoned'
                 WHERE table_id = {} AND row_key = '{root_key}'",
                root_table.0
            ))
            .await;

        let mut dirty: BTreeSet<RowRef> = (1..=CHAIN_ROWS).map(chain_row).collect();
        let mut visibility = Visibility::load(
            &replica.engine,
            &mut replica.conn,
            &mut dirty,
            DeriveScope::All,
        )
        .await
        .unwrap();
        // The row furthest from the root first, so no walk meets a cached row above it.
        for index in (1..=CHAIN_ROWS).rev() {
            let (table, key) = chain_row(index);
            let roots = visibility
                .cascade_roots(&replica.engine, &mut replica.conn, table, &key)
                .await
                .unwrap();
            let cause = roots.cause().expect("the chain reaches the deleted root");
            assert_eq!((cause.table, &cause.key), (root_table, &root_key));
        }

        let repairs =
            repair::repair_changes(&replica.engine, &mut replica.conn, &mut visibility, &dirty)
                .await
                .unwrap();
        let tombstones: BTreeSet<(TableId, String, Option<RowRef>)> = repairs
            .iter()
            .map(|change| {
                assert_eq!(change.action, ChangeAction::Tombstone);
                let cause = change.cause.as_ref();
                (
                    change.table,
                    change.row.as_str().to_string(),
                    cause.map(|cause| (cause.table, cause.row.as_str().to_string())),
                )
            })
            .collect();
        let expected: BTreeSet<_> = (1..=CHAIN_ROWS)
            .map(|index| {
                let (table, key) = chain_row(index);
                (table, key, Some(chain_row(0)))
            })
            .collect();
        assert_eq!(tombstones, expected, "cascade tombstones name their root");
        assert_eq!(
            visibility.cascade_walks, CHAIN_ROWS,
            "each chain row is walked once"
        );
    });
}

/// A row of the in-memory graph.
fn node(key: &str) -> RowRef {
    (CYCLE_A_TABLE, key.to_string())
}

/// The value of a row that reaches one root by itself.
fn reaching(key: &str, kind: RootKind) -> Roots {
    Roots::of(Root {
        table: CYCLE_A_TABLE,
        key: key.to_string(),
        kind,
    })
}

/// Drives a walk over a graph of rows, with `known` rows standing for finished results. Returns
/// every finished row and how many rows the graph had to answer for.
fn walk_graph<V: WalkValue>(
    start: &str,
    graph: &HashMap<RowRef, (V, Vec<RowRef>)>,
    known: &HashMap<RowRef, Arc<V>>,
) -> (HashMap<RowRef, Arc<V>>, usize) {
    let mut walk = CascadeWalk::new(node(start));
    let mut read = 0;
    while let Some(row) = walk.next_row() {
        if let Some(value) = known.get(&row) {
            walk.supply_finished(row, Arc::clone(value));
            continue;
        }
        read += 1;
        let (own, successors) = graph[&row].clone();
        walk.supply(row, own, successors);
    }
    (walk.finish().into_iter().collect(), read)
}

/// A graph from `(row, own value, successors)` lines.
fn graph(rows: Vec<(&str, Roots, &[&str])>) -> HashMap<RowRef, (Roots, Vec<RowRef>)> {
    rows.into_iter()
        .map(|(key, own, successors)| {
            let successors = successors.iter().map(|next| node(next)).collect();
            (node(key), (own, successors))
        })
        .collect()
}

#[test]
fn cascade_walk_handles_a_cycle_and_a_long_chain() {
    let shown = Roots::default;
    let known = HashMap::new();

    // A cycle hides nothing on its own, from whichever member the walk starts.
    let cycle = graph(vec![
        ("x", shown(), &["y"]),
        ("y", shown(), &["z"]),
        ("z", shown(), &["x"]),
    ]);
    for start in ["x", "y", "z"] {
        let (finished, read) = walk_graph(start, &cycle, &known);
        assert_eq!((finished.len(), read), (3, 3), "from {start}");
        assert!(finished.values().all(|roots| !roots.hides()));
    }

    // Members of a cycle share one complete result, whichever member reaches the way out.
    let exits = graph(vec![
        ("tail", shown(), &["x"]),
        ("x", shown(), &["y"]),
        ("y", shown(), &["z", "out"]),
        ("z", reaching("waiting", RootKind::Missing), &["x", "free"]),
        ("out", reaching("gone", RootKind::Tombstoned), &[]),
        ("free", shown(), &[]),
        ("held", reaching("waiting", RootKind::Missing), &["free"]),
    ]);
    for start in ["tail", "x", "y", "z"] {
        let (finished, read) = walk_graph(start, &exits, &known);
        let expected = if start == "tail" { 6 } else { 5 };
        assert_eq!((finished.len(), read), (expected, expected), "from {start}");
        for member in ["x", "y", "z", "out"] {
            let cause = finished[&node(member)]
                .cause()
                .map(|root| root.key.as_str());
            assert_eq!(cause, Some("gone"), "{member} from {start}");
        }
        assert!(Arc::ptr_eq(&finished[&node("x")], &finished[&node("z")]));
        assert!(!finished[&node("free")].hides(), "a successor stays shown");
    }
    let (finished, _) = walk_graph("held", &exits, &known);
    let held = &finished[&node("held")];
    assert!(
        held.hides() && held.cause().is_none(),
        "a missing root deletes nothing"
    );

    // A finished row ends the walk, with its result standing for every row behind it.
    let cached = graph(vec![
        ("above", shown(), &["below"]),
        ("below", shown(), &["behind"]),
        ("behind", reaching("gone", RootKind::Tombstoned), &[]),
    ]);
    let (first, read) = walk_graph("below", &cached, &known);
    assert_eq!(read, 2);
    let (second, read) = walk_graph("above", &cached, &first);
    assert_eq!(read, 1, "only the row outside the cache is read");
    let cause = second[&node("above")].cause().map(|root| root.key.as_str());
    assert_eq!(cause, Some("gone"));

    // Every row of a long chain reaches a root of its own, so a result that grew with the chain
    // would cost memory quadratic in its length.
    let key = |index: usize| format!("row-{index:06}");
    let long: HashMap<RowRef, (Roots, Vec<RowRef>)> = (0..LONG_CHAIN_ROWS)
        .map(|index| {
            let own = reaching(&format!("root-{index:06}"), RootKind::Tombstoned);
            let next = (index + 1 < LONG_CHAIN_ROWS).then(|| node(&key(index + 1)));
            (node(&key(index)), (own, next.into_iter().collect()))
        })
        .collect();
    let (finished, read) = walk_graph(&key(0), &long, &known);
    assert_eq!((finished.len(), read), (LONG_CHAIN_ROWS, LONG_CHAIN_ROWS));
    for index in [0, LONG_CHAIN_ROWS / 2, LONG_CHAIN_ROWS - 1] {
        let cause = finished[&node(&key(index))]
            .cause()
            .map(|root| root.key.clone());
        assert_eq!(cause, Some(format!("root-{index:06}")));
    }
}

/// A row that adds nothing to what it reaches keeps the same allocation, whether that result was
/// joined in this walk or finished in an earlier one, and a row that adds something leaves the
/// result it reaches as it was.
#[test]
fn cascade_walk_shares_a_result_a_row_adds_nothing_to() {
    let shown = Roots::default;
    let known = HashMap::new();
    let gone = || reaching("b-gone", RootKind::Tombstoned);
    let lower = || reaching("a-gone", RootKind::Tombstoned);
    let waiting = || reaching("waiting", RootKind::Missing);
    let rows = graph(vec![
        ("empty", shown(), &["copy"]),
        ("copy", gone(), &["two"]),
        ("two", shown(), &["base", "other"]),
        ("base", gone(), &[]),
        ("other", waiting(), &[]),
        ("lowers", lower(), &["base"]),
        ("hides", waiting(), &["hidden"]),
        ("hidden", shown(), &[]),
        ("x", shown(), &["y"]),
        ("y", shown(), &["x", "base"]),
    ]);
    let (first, _) = walk_graph("empty", &rows, &known);
    let shared = &first[&node("base")];
    for row in ["empty", "copy", "two"] {
        assert!(Arc::ptr_eq(&first[&node(row)], shared), "{row} shares");
    }

    let (second, read) = walk_graph("lowers", &rows, &first);
    assert_eq!(read, 1, "the finished row is not read again");
    assert!(Arc::ptr_eq(&second[&node("base")], shared));
    assert_eq!(second[&node("base")].cause(), gone().cause());
    assert!(!Arc::ptr_eq(&second[&node("lowers")], shared));
    assert_eq!(second[&node("lowers")].cause(), lower().cause());

    let (third, read) = walk_graph("x", &rows, &first);
    assert_eq!(read, 2);
    for member in ["x", "y"] {
        assert!(
            Arc::ptr_eq(&third[&node(member)], shared),
            "{member} shares"
        );
    }

    let (fourth, _) = walk_graph("hides", &rows, &known);
    assert!(fourth[&node("hides")].hides());
    assert!(!fourth[&node("hidden")].hides(), "the row reached is shown");
}

/// Rows of the tombstone cause chain, each naming the row before it as its cause.
const CAUSE_ROWS: usize = 256;

fn cause_row(index: usize) -> String {
    format!("cause-{index:03}")
}

/// Inserts `keys` into `fixture_cycle_a` and seals them.
async fn seal_cycle_rows(replica: &mut Replica, keys: &[String]) {
    let values = keys
        .iter()
        .map(|key| format!("('{key}', '{key}')"))
        .collect::<Vec<_>>()
        .join(", ");
    replica
        .exec(&format!(
            "INSERT INTO fixture_cycle_a (id, label) VALUES {values}"
        ))
        .await;
    replica.seal().await.unwrap();
}

/// Seals a tombstone of a `fixture_cycle_a` row in an operation of its own.
async fn seal_tombstone(replica: &mut Replica, key: &str, cause: Option<&str>) {
    let now_ms = replica.now_ms;
    let Replica {
        engine,
        conn,
        ctx,
        identity,
        ..
    } = replica;
    let mut local = identity.local();
    let mut tx = conn.begin().await.unwrap();
    log::ensure_space(&mut tx, ctx.space).await.unwrap();
    sql::begin_engine_write(&mut tx).await.unwrap();
    let mut batch = Batch::load(&mut tx, ctx.space).await.unwrap();
    let mut slot = SealSlot::new(Some(&mut local));
    let mut report = SealReport::default();
    let (sealer, writer) = slot
        .open(engine, &mut tx, ctx, &mut batch, now_ms)
        .await
        .unwrap()
        .expect("the slot has a writer");
    let change = Change {
        table: CYCLE_A_TABLE,
        row: RowKey::new(key).unwrap(),
        action: ChangeAction::Tombstone,
        groups: Vec::new(),
        replaced_by: None,
        cause: cause.map(|row| Cause {
            table: CYCLE_A_TABLE,
            row: RowKey::new(row).unwrap(),
        }),
    };
    sealer
        .seal_changes(
            engine,
            &mut tx,
            ctx,
            &mut batch,
            writer,
            vec![change],
            None,
            now_ms,
            &mut report,
        )
        .await
        .unwrap();
    finish_sealed(
        engine,
        &mut tx,
        ctx,
        &mut batch,
        &mut slot,
        Constrained::Seal,
        now_ms,
        &mut report,
    )
    .await
    .unwrap();
    sql::set_applying(&mut tx, false).await.unwrap();
    tx.commit().await.unwrap();
}

/// The context of the newest tombstone of a `fixture_cycle_a` row.
async fn tombstone_context(replica: &mut Replica, key: &str) -> VersionVector {
    let (writer, seq): (Vec<u8>, i64) = sqlx::query_as(
        "SELECT writer_id, seq FROM sync_tombstones
         WHERE table_id = ? AND row_key = ? ORDER BY seq DESC LIMIT 1",
    )
    .bind(i64::from(CYCLE_A_TABLE.0))
    .bind(key)
    .fetch_one(&mut replica.conn)
    .await
    .unwrap();
    let writer = log::writer_id(&writer).unwrap();
    let mut batch = Batch::load(&mut replica.conn, replica.ctx.space)
        .await
        .unwrap();
    recovery::operation_context(
        &mut replica.conn,
        &mut batch,
        writer,
        u64::try_from(seq).unwrap(),
    )
    .await
    .unwrap()
}

async fn loaded_visibility(replica: &mut Replica) -> Visibility {
    Visibility::load(
        &replica.engine,
        &mut replica.conn,
        &mut BTreeSet::new(),
        DeriveScope::All,
    )
    .await
    .unwrap()
}

/// The coverage of each of `keys`, read in order on one visibility, with the rows that read.
async fn coverages(replica: &mut Replica, keys: &[String]) -> (Vec<VersionVector>, usize) {
    let mut visibility = loaded_visibility(replica).await;
    let mut batch = Batch::load(&mut replica.conn, replica.ctx.space)
        .await
        .unwrap();
    let mut covering = Vec::with_capacity(keys.len());
    for key in keys {
        let coverage = recovery::coverage(
            &replica.engine,
            &mut replica.conn,
            &mut batch,
            &mut visibility,
            CYCLE_A_TABLE,
            key,
        )
        .await
        .unwrap();
        covering.push(coverage.as_ref().clone());
    }
    (covering, visibility.coverage_rows)
}

/// Every row of a cause chain covers what the chain's root covers, and each row is read once no
/// matter how many rows above it reach it. A row with a tombstone of its own adds that tombstone
/// to itself and the rows above it, and a cycle of causes shares one result.
#[test]
fn tombstone_cause_chain_reads_each_row_once() {
    block_on(async {
        let mut replica = Replica::new(0, 3).await;
        let rows: Vec<String> = (0..CAUSE_ROWS).map(cause_row).collect();
        seal_cycle_rows(&mut replica, &rows).await;
        for (index, row) in rows.iter().enumerate() {
            let cause = index.checked_sub(1).map(cause_row);
            seal_tombstone(&mut replica, row, cause.as_deref()).await;
        }
        let root = tombstone_context(&mut replica, &rows[0]).await;

        let mut descending = rows.clone();
        descending.reverse();
        for order in [&rows, &descending] {
            let (covering, read) = coverages(&mut replica, order).await;
            assert_eq!(read, CAUSE_ROWS, "rows read for {} first", order[0]);
            assert!(covering.iter().all(|coverage| *coverage == root));
        }

        let middle = CAUSE_ROWS / 2;
        seal_tombstone(&mut replica, &rows[middle], None).await;
        let own = tombstone_context(&mut replica, &rows[middle]).await;
        let (covering, _) = coverages(&mut replica, &rows).await;
        let mut both = root.clone();
        both.merge(&own);
        for (index, coverage) in covering.iter().enumerate() {
            let expected = if index < middle { &root } else { &both };
            assert_eq!(coverage, expected, "coverage of row {index}");
        }
        assert!(!root.dominates(&own), "the own tombstone adds to the chain");

        let cycle = ["loop-x", "loop-y", "loop-above"].map(String::from);
        seal_cycle_rows(&mut replica, &cycle).await;
        seal_tombstone(&mut replica, "loop-x", Some("loop-y")).await;
        seal_tombstone(&mut replica, "loop-y", Some("loop-x")).await;
        seal_tombstone(&mut replica, "loop-above", Some("loop-x")).await;
        let (covering, read) = coverages(&mut replica, &cycle).await;
        assert_eq!(read, cycle.len());
        assert_eq!(covering, vec![VersionVector::new(); cycle.len()]);

        seal_tombstone(&mut replica, "loop-y", None).await;
        let own = tombstone_context(&mut replica, "loop-y").await;
        let (covering, read) = coverages(&mut replica, &cycle).await;
        assert_eq!(read, cycle.len());
        assert_eq!(covering, vec![own; cycle.len()]);
    });
}

/// Joining the contexts of a row into one vector decides every retained version the way the
/// contexts do one by one, for each way a row reaches its coverage: its own tombstone, a deleted
/// cause, a cause that is live, a cycle of causes, a cascade, and a removal.
#[test]
fn coverage_join_keeps_the_same_flagged_rows() {
    block_on(async {
        let mut replicas = replicas(3).await;
        let member = membership_key("item-1", "a-member");
        replicas[0]
            .exec(&format!(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('item-1', '2027-01-01T00:00:00.000Z', 'One', 0);
                 INSERT INTO fixture_cycle_b (id, label) VALUES ('b-1', 'B');
                 INSERT INTO fixture_cycle_a (id, label) VALUES
                     ('a-plain', 'plain'), ('a-root', 'root'), ('a-caused', 'caused'),
                     ('a-live', 'live'), ('a-freed', 'freed'), ('a-x', 'x'), ('a-y', 'y'),
                     ('a-hidden', 'hidden'), ('a-member', 'member');
                 INSERT INTO fixture_memberships (item_id, end_id, sync_key, created_at, role)
                     VALUES ('item-1', 'a-member', '{member}', '2027-01-01T00:00:00.000Z', 'r');"
            ))
            .await;
        converge(&mut replicas).await;

        replicas[1]
            .exec(
                "UPDATE fixture_cycle_a SET label = label || '-edited';
                 UPDATE fixture_cycle_a SET peer_id = 'b-1' WHERE id = 'a-hidden';
                 UPDATE fixture_memberships SET role = 'edited';",
            )
            .await;
        replicas[1].seal().await.unwrap();

        let deleter = &mut replicas[0];
        seal_tombstone(deleter, "a-plain", None).await;
        seal_tombstone(deleter, "a-root", None).await;
        seal_tombstone(deleter, "a-caused", Some("a-root")).await;
        seal_tombstone(deleter, "a-freed", Some("a-live")).await;
        seal_tombstone(deleter, "a-x", Some("a-y")).await;
        seal_tombstone(deleter, "a-y", Some("a-x")).await;
        seal_tombstone(deleter, "a-x", None).await;
        deleter
            .exec(
                "DELETE FROM fixture_cycle_b WHERE id = 'b-1';
                 DELETE FROM fixture_memberships WHERE sync_key IS NOT NULL;",
            )
            .await;
        deleter.seal().await.unwrap();

        for source in [0, 1] {
            let (observer, from) = pair(&mut replicas, 2, source);
            pull_without_writer(observer, from).await;
        }
        let observer = &mut replicas[2];
        let mut rows: Vec<RowRef> = [
            "a-plain", "a-root", "a-caused", "a-live", "a-freed", "a-x", "a-y", "a-hidden",
            "a-member",
        ]
        .map(|key| (CYCLE_A_TABLE, key.to_string()))
        .into();
        rows.push((CYCLE_B_TABLE, "b-1".to_string()));
        rows.push((MEMBERSHIPS_TABLE, member));
        let edited: BTreeSet<String> = rows
            .iter()
            .filter(|(table, _)| *table != CYCLE_B_TABLE)
            .map(|(_, key)| key.clone())
            .collect();

        // The memo fills in reading order, so each order enters the cycle from a different row.
        let mut descending = rows.clone();
        descending.reverse();
        for order in [&rows, &descending] {
            let (seen, unseen) = flagged_rows(observer, order).await;
            assert!(seen > 0, "some version is covered");
            assert_eq!(unseen, edited);
        }
    });
}

/// Reads `rows` in order on one visibility, asserting that the joined coverage decides every
/// retained version the way the row's contexts do one by one. Returns how many versions are
/// covered and the keys of rows with a version that is not.
async fn flagged_rows(observer: &mut Replica, rows: &[RowRef]) -> (usize, BTreeSet<String>) {
    let mut visibility = loaded_visibility(observer).await;
    let mut batch = Batch::load(&mut observer.conn, observer.ctx.space)
        .await
        .unwrap();
    let mut seen = 0;
    let mut unseen = BTreeSet::new();
    for (table, key) in rows {
        let versions = merge::versions(&observer.engine, &mut observer.conn, *table, key)
            .await
            .unwrap();
        assert!(!versions.is_empty(), "{key} retains versions");
        let contexts = recovery::coverage_contexts(
            &observer.engine,
            &mut observer.conn,
            &mut batch,
            &mut visibility,
            *table,
            key,
        )
        .await
        .unwrap();
        let joined = recovery::coverage(
            &observer.engine,
            &mut observer.conn,
            &mut batch,
            &mut visibility,
            *table,
            key,
        )
        .await
        .unwrap();
        for version in &versions {
            let listed = !contexts
                .iter()
                .any(|context| context.covers(&version.writer, version.seq));
            assert_eq!(
                recovery::uncovered(version, &joined),
                listed,
                "{key} version {}",
                version.seq
            );
            if listed {
                unseen.insert(key.clone());
            } else {
                seen += 1;
            }
        }
    }
    (seen, unseen)
}

/// How many vectors the coverage memo holds for `keys` once they are read in order on one
/// visibility, counted by allocation.
async fn memo_vectors(replica: &mut Replica, keys: &[&String]) -> usize {
    let mut visibility = loaded_visibility(replica).await;
    let mut batch = Batch::load(&mut replica.conn, replica.ctx.space)
        .await
        .unwrap();
    for key in keys {
        recovery::coverage(
            &replica.engine,
            &mut replica.conn,
            &mut batch,
            &mut visibility,
            CYCLE_A_TABLE,
            key,
        )
        .await
        .unwrap();
    }
    let memo: BTreeSet<*const VersionVector> = keys
        .iter()
        .map(|key| {
            let row = (CYCLE_A_TABLE, (*key).clone());
            Arc::as_ptr(&visibility.known_coverage(&row).expect("the row was read"))
        })
        .collect();
    memo.len()
}

/// The rows of a cause chain that add no context hold one vector however a read enters the chain,
/// so its memory follows the contexts and not its length. A row that adds a context starts one
/// vector for itself and the rows above it, and the rows below keep theirs. Flagged rows stay
/// those of the contexts listed one by one.
#[test]
fn coverage_chain_shares_one_vector() {
    block_on(async {
        let mut replicas = replicas(3).await;
        let rows: Vec<String> = (0..CAUSE_ROWS).map(cause_row).collect();
        seal_cycle_rows(&mut replicas[0], &rows).await;
        converge(&mut replicas).await;

        replicas[1]
            .exec("UPDATE fixture_cycle_a SET label = label || '-edited'")
            .await;
        replicas[1].seal().await.unwrap();

        for (index, row) in rows.iter().enumerate() {
            let cause = index.checked_sub(1).map(cause_row);
            seal_tombstone(&mut replicas[0], row, cause.as_deref()).await;
        }
        for source in [0, 1] {
            let (observer, from) = pair(&mut replicas, 2, source);
            pull_without_writer(observer, from).await;
        }

        let chain: BTreeSet<String> = rows.iter().cloned().collect();
        let references: Vec<RowRef> = rows
            .iter()
            .map(|key| (CYCLE_A_TABLE, key.clone()))
            .collect();
        let middle = CAUSE_ROWS / 2;
        let ascending: Vec<&String> = rows.iter().collect();
        let descending: Vec<&String> = rows.iter().rev().collect();
        let from_the_middle: Vec<&String> = rows[middle..].iter().chain(&rows[..middle]).collect();
        let orders = [&ascending, &descending, &from_the_middle];

        for order in orders {
            let vectors = memo_vectors(&mut replicas[2], order).await;
            assert_eq!(vectors, 1, "vectors for {} first", order[0]);
        }
        let (seen, unseen) = flagged_rows(&mut replicas[2], &references).await;
        assert!(seen > 0, "some version is covered");
        assert_eq!(unseen, chain);

        seal_tombstone(&mut replicas[0], &rows[middle], None).await;
        // A second cause at the top of the chain leads to what the first one covers.
        seal_tombstone(&mut replicas[0], &rows[CAUSE_ROWS - 1], Some(&rows[0])).await;
        let (observer, from) = pair(&mut replicas, 2, 0);
        pull_without_writer(observer, from).await;

        for order in orders {
            let vectors = memo_vectors(&mut replicas[2], order).await;
            assert_eq!(vectors, 2, "vectors for {} first", order[0]);
        }
        let (seen, unseen) = flagged_rows(&mut replicas[2], &references).await;
        assert!(seen > 0, "some version is covered");
        assert_eq!(unseen, chain);
    });
}
