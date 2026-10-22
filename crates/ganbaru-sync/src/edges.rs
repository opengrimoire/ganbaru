//! Cycle repair of acyclic edge sets.
//!
//! The live rows of an acyclic edge table are directed edges from one row of the endpoint
//! table to another. An edge counts while its existence register holds 1, it is not a hidden
//! duplicate, and neither endpoint is hidden. Counted edges are taken in ascending
//! `(clock, writer)` of their winning existence version, then by row key; an edge whose end
//! already reaches its start through the edges taken before it would close a cycle and is
//! severed instead. A severed edge is hidden and surfaces as a conflict of its existence group,
//! which the user resolves by removing it or another edge of the cycle. Every input is merge
//! state, so every replica severs the same edges without sealing a repair.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{Field, Hlc, TableId, WriterId};
use sqlx::{AssertSqlSafe, Row, SqliteConnection};

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::manifest::{AcyclicSpec, GroupSpec, TableSpec};
use crate::merge::{self, table_param};
use crate::sql::ident;
use crate::visibility::{RowRef, Visibility};

/// A counted edge of an acyclic edge set.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    rank: (Hlc, WriterId),
    key: String,
    from: String,
    to: String,
}

/// Where the endpoints of an edge live: the identity group, the endpoint table, and the field
/// indexes of the start and the end.
struct Endpoints {
    group: &'static GroupSpec,
    table: TableId,
    from: usize,
    to: usize,
}

impl Endpoints {
    fn of(table: &TableSpec, acyclic: AcyclicSpec) -> SyncResult<Self> {
        let find = |column: &str| {
            table
                .references()
                .find(|(_, _, reference)| reference.column == column)
                .ok_or_else(|| corrupt("acyclic edge endpoint is not a reference column"))
        };
        let (group, from, reference) = find(acyclic.from_column)?;
        let (_, to, _) = find(acyclic.to_column)?;
        Ok(Self {
            group,
            table: reference.table,
            from,
            to,
        })
    }
}

/// The counted edges of an acyclic edge table.
async fn edges(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &Visibility,
    probe: &mut Visibility,
    table: &TableSpec,
    acyclic: AcyclicSpec,
) -> SyncResult<Vec<Edge>> {
    let exists = table
        .exists_group()
        .ok_or_else(|| corrupt("acyclic edge table without an existence group"))?;
    let endpoints = Endpoints::of(table, acyclic)?;
    let rows = sqlx::query(
        "SELECT v.row_key, v.group_id, v.writer_id, v.seq, v.clock, v.value_hash, v.value
         FROM sync_register_versions v
         JOIN sync_rows r ON r.table_id = v.table_id AND r.row_key = v.row_key
         WHERE v.table_id = ? AND r.state = 'live'
         ORDER BY v.row_key",
    )
    .bind(table_param(table.id))
    .fetch_all(&mut *conn)
    .await?;
    let mut versions: BTreeMap<String, Vec<merge::Version>> = BTreeMap::new();
    for row in &rows {
        versions
            .entry(row.try_get(0)?)
            .or_default()
            .push(merge::version_from_row(row, 1)?);
    }
    let mut edges = Vec::new();
    for (key, versions) in versions {
        if visibility.is_duplicate(table.id, &key) {
            continue;
        }
        let winners = merge::winners(engine, table, &versions);
        let Some(existence) = winners.get(&exists.id) else {
            continue;
        };
        if merge::is_removal(Some(&existence.value)) {
            continue;
        }
        let Some(ends) = winners.get(&endpoints.group.id) else {
            continue;
        };
        let (Some(Field::Text(from)), Some(Field::Text(to))) = (
            ends.value.fields().get(endpoints.from),
            ends.value.fields().get(endpoints.to),
        ) else {
            continue;
        };
        let target = endpoints.table;
        if probe.hidden(engine, conn, target, from).await?
            || probe.hidden(engine, conn, target, to).await?
            || merge::row_state(conn, target, from).await?.is_none()
            || merge::row_state(conn, target, to).await?.is_none()
        {
            continue;
        }
        edges.push(Edge {
            rank: (existence.clock, existence.writer),
            from: from.clone(),
            to: to.clone(),
            key,
        });
    }
    Ok(edges)
}

/// Whether `start` reaches `goal` through the taken edges.
fn reaches(taken: &BTreeMap<String, Vec<String>>, start: &str, goal: &str) -> bool {
    let mut seen = BTreeSet::from([start]);
    let mut pending = vec![start];
    while let Some(row) = pending.pop() {
        if row == goal {
            return true;
        }
        for next in taken.get(row).into_iter().flatten() {
            if seen.insert(next.as_str()) {
                pending.push(next.as_str());
            }
        }
    }
    false
}

/// Keys of the edges severed to keep the set acyclic.
fn sever(mut edges: Vec<Edge>) -> BTreeSet<String> {
    edges.sort();
    let mut taken: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut severed = BTreeSet::new();
    for edge in edges {
        if reaches(&taken, &edge.to, &edge.from) {
            severed.insert(edge.key);
        } else {
            taken.entry(edge.from).or_default().push(edge.to);
        }
    }
    severed
}

/// Marks dirty the counted edges whose domain presence disagrees with their severing.
async fn mark_mismatches(
    conn: &mut SqliteConnection,
    table: &TableSpec,
    edges: &[Edge],
    severed: &BTreeSet<String>,
    dirty: &mut BTreeSet<RowRef>,
) -> SyncResult<()> {
    let sql = format!(
        "SELECT {} FROM {}",
        ident(table.key_column),
        ident(table.name)
    );
    let present: BTreeSet<String> = sqlx::query_scalar(AssertSqlSafe(sql.as_str()))
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .collect();
    for edge in edges {
        if severed.contains(&edge.key) == present.contains(&edge.key) {
            dirty.insert((table.id, edge.key.clone()));
        }
    }
    Ok(())
}

/// Severed edges of every acyclic edge table. Edges whose domain presence disagrees with their
/// severing are marked dirty so materialize and the conflict flags follow.
pub(crate) async fn severed(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &Visibility,
    dirty: &mut BTreeSet<RowRef>,
) -> SyncResult<BTreeSet<RowRef>> {
    let mut severed = BTreeSet::new();
    // Endpoint visibility before any edge is severed; its cascade cache is discarded.
    let mut probe = Visibility::with_duplicates(visibility.duplicates.clone());
    probe.detached = visibility.detached.clone();
    for table in engine.manifest().tables {
        let Some(acyclic) = table.acyclic else {
            continue;
        };
        let edges = edges(engine, conn, visibility, &mut probe, table, acyclic).await?;
        let keys = sever(edges.clone());
        mark_mismatches(conn, table, &edges, &keys, dirty).await?;
        severed.extend(keys.into_iter().map(|key| (table.id, key)));
    }
    Ok(severed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(key: &str, from: &str, to: &str, clock: u64, writer: u8) -> Edge {
        Edge {
            rank: (Hlc::from_u64(clock), WriterId::from_bytes([writer; 16])),
            key: key.to_string(),
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    #[test]
    fn the_newest_edge_of_a_cycle_is_severed_with_ties_going_to_the_greater_writer() {
        let edges = vec![
            edge("e-ab", "a", "b", 5, 1),
            edge("e-bc", "b", "c", 5, 3),
            edge("e-ca", "c", "a", 5, 2),
            edge("e-ad", "a", "d", 9, 1),
        ];
        assert_eq!(sever(edges), BTreeSet::from(["e-bc".to_string()]));
    }

    #[test]
    fn a_self_loop_is_severed_and_a_diamond_is_kept() {
        let edges = vec![
            edge("loop", "a", "a", 1, 1),
            edge("ab", "a", "b", 2, 1),
            edge("ac", "a", "c", 3, 1),
            edge("bd", "b", "d", 4, 1),
            edge("cd", "c", "d", 5, 1),
        ];
        assert_eq!(sever(edges), BTreeSet::from(["loop".to_string()]));
    }

    #[test]
    fn an_older_edge_that_arrives_late_severs_a_newer_one() {
        let edges = vec![edge("new", "b", "a", 9, 1), edge("old", "a", "b", 1, 1)];
        assert_eq!(sever(edges), BTreeSet::from(["new".to_string()]));
    }
}
