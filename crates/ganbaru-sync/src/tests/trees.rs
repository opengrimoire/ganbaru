//! Tree moves and acyclic edges under concurrent writes: cycle breaking, depth limits, and
//! severed edges.

use ganbaru_sync_contracts::GroupMask;

use super::block_on;
use super::engine::{add_edge, add_node, assert_converged, node_parents, replicas, texts};
use super::fixture::{EDGES_TABLE, NODES_TABLE, edge_group, edge_key};
use super::replica::{Replica, converge};
use crate::ConflictRow;

#[test]
fn a_cycle_of_three_concurrent_moves_breaks_at_the_move_of_the_greatest_writer() {
    block_on(async {
        let mut replicas = replicas(3).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}",
                add_node("a", None, 0, "A"),
                add_node("b", None, 1, "B"),
                add_node("c", None, 2, "C"),
            ))
            .await;
        converge(&mut replicas).await;
        let moves = [("a", "b"), ("b", "c"), ("c", "a")];
        let mut ranks = Vec::new();
        for (replica, (row, parent)) in replicas.iter_mut().zip(moves) {
            replica
                .exec(&format!(
                    "UPDATE fixture_nodes SET parent_id = '{parent}' WHERE id = '{row}'"
                ))
                .await;
            replica.seal().await.unwrap();
            let clock = replica
                .count(&format!(
                    "SELECT max(clock) FROM sync_register_versions
                     WHERE table_id = {} AND row_key = '{row}' AND group_id = 0",
                    NODES_TABLE.0
                ))
                .await;
            ranks.push((clock, replica.writer(), row));
        }
        // Every move carries the same clock, so the writer breaks the tie.
        assert!(ranks.iter().all(|(clock, _, _)| *clock == ranks[0].0));
        let broken = ranks.iter().max().map(|(_, _, row)| *row).unwrap();
        let expected: Vec<String> = moves
            .iter()
            .map(|(row, parent)| {
                let parent = if *row == broken { "-" } else { parent };
                format!("{row}>{parent}")
            })
            .collect();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(node_parents(replica).await, expected);
        }
    });
}

#[test]
fn concurrent_moves_that_together_exceed_the_maximum_depth_move_the_deepest_row_to_the_root() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}{}",
                add_node("p", None, 0, "P"),
                add_node("q", None, 1, "Q"),
                add_node("q-c", Some("q"), 0, "Q child"),
                add_node("s", None, 2, "S"),
            ))
            .await;
        converge(&mut replicas).await;
        // Each move alone keeps every node within the maximum depth of 2.
        replicas[0]
            .exec("UPDATE fixture_nodes SET parent_id = 'p' WHERE id = 'q'")
            .await;
        replicas[1]
            .exec("UPDATE fixture_nodes SET parent_id = 'q-c' WHERE id = 's'")
            .await;
        replicas[0].seal().await.unwrap();
        replicas[1].seal().await.unwrap();
        assert_eq!(
            node_parents(&mut replicas[1]).await,
            ["p>-", "q>-", "q-c>q", "s>q-c"]
        );
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(node_parents(replica).await, ["p>-", "q>p", "q-c>q", "s>-"]);
            // The repair is sealed: the newest parent version of the moved row holds the same
            // null parent as the root `p`.
            let newest = texts(
                replica,
                &format!(
                    "SELECT quote(v.value) FROM sync_register_versions v
                     WHERE v.table_id = {} AND v.group_id = 0 AND v.row_key IN ('p', 's')
                     AND v.clock = (SELECT max(w.clock) FROM sync_register_versions w
                         WHERE w.table_id = v.table_id AND w.group_id = 0
                         AND w.row_key = v.row_key)
                     ORDER BY v.row_key",
                    NODES_TABLE.0
                ),
            )
            .await;
            assert_eq!(newest.len(), 2);
            assert_eq!(newest[0], newest[1]);
        }
    });
}

/// Every materialized edge as `from>to`.
async fn edge_rows(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT from_id || '>' || to_id FROM fixture_edges ORDER BY from_id, to_id",
    )
    .await
}

#[test]
fn concurrent_edges_that_close_a_cycle_sever_the_newest_until_another_edge_is_removed() {
    block_on(async {
        let mut replicas = replicas(3).await;
        let items: String = ["x", "y", "z"]
            .iter()
            .map(|id| {
                format!(
                    "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('{id}', '2027-01-01T00:00:00.000Z', '{id}', 0);"
                )
            })
            .collect();
        replicas[0].exec(&items).await;
        converge(&mut replicas).await;
        let added = [("x", "y"), ("y", "z"), ("z", "x")];
        let mut ranks = Vec::new();
        for (replica, (from, to)) in replicas.iter_mut().zip(added) {
            replica.exec(&add_edge(from, to)).await;
            replica.seal().await.unwrap();
            let clock = replica
                .count(&format!(
                    "SELECT max(clock) FROM sync_register_versions
                     WHERE table_id = {} AND row_key = '{}' AND group_id = {}",
                    EDGES_TABLE.0,
                    edge_key(from, to),
                    edge_group::EXISTS.get()
                ))
                .await;
            ranks.push((clock, replica.writer(), (from, to)));
        }
        let (severed_from, severed_to) = ranks.iter().max().map(|(_, _, edge)| *edge).unwrap();
        let severed = format!("{severed_from}>{severed_to}");
        let kept: Vec<String> = added
            .iter()
            .map(|(from, to)| format!("{from}>{to}"))
            .filter(|edge| *edge != severed)
            .collect();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            let mut edges = edge_rows(replica).await;
            edges.sort();
            assert_eq!(edges, kept);
            assert_eq!(
                replica
                    .engine
                    .conflicts(&mut replica.conn, EDGES_TABLE)
                    .await
                    .unwrap(),
                vec![ConflictRow {
                    table: EDGES_TABLE,
                    row_key: edge_key(severed_from, severed_to),
                    groups: GroupMask::of(edge_group::EXISTS),
                }]
            );
        }
        // Removing another edge of the cycle shows the severed edge again.
        let (from, to) = kept[0].split_once('>').unwrap();
        replicas[0]
            .exec(&format!(
                "DELETE FROM fixture_edges WHERE from_id = '{from}' AND to_id = '{to}'"
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let mut expected = vec![kept[1].clone(), severed.clone()];
        expected.sort();
        for replica in &mut replicas {
            let mut edges = edge_rows(replica).await;
            edges.sort();
            assert_eq!(edges, expected);
            assert!(
                replica
                    .engine
                    .conflicts(&mut replica.conn, EDGES_TABLE)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    });
}
