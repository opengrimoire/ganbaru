//! Publication predicates, rows published with their parent, and capture seeding.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use ganbaru_sync_contracts::{ChangeAction, TableId};
use sqlx::Connection;

use super::block_on;
use super::fixture::{
    CYCLE_A_TABLE, CYCLE_B_TABLE, DRAFT_NOTES_TABLE, DRAFT_PARTS_TABLE, DRAFTS_TABLE,
    FIXTURE_TABLE_IDS, ITEMS_TABLE, MEMOS_TABLE,
};
use super::replica::{Replica, converge};
use crate::visibility::{
    CascadeWalk, DeriveScope, Root, RootKind, Roots, RowRef, Visibility, WalkValue,
};
use crate::{SyncError, repair, seed};

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
            let value = value.as_ref().clone();
            walk.supply(row, value, Vec::new());
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
