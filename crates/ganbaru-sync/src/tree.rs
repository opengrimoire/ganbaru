//! Cycle and depth repair of tree tables.
//!
//! A live row of a tree table has a parent when the winning version of its parent group names
//! a row that [`Visibility::follow`] resolves to a live row of the table. Each row has at most
//! one parent, so every component of the parent graph holds at most one cycle. A cycle is
//! broken at the row whose winning parent version has the greatest `(clock, writer)`, with the
//! greater row key breaking a tie within one operation. Then every row deeper than the table's
//! maximum depth is detached too: the deepest row has no children, so moving it to the root
//! never changes the depth of another row, and moving the deepest row until none exceeds the
//! maximum detaches exactly the rows that started deeper than it.
//!
//! A detached row materializes with a null parent and keeps its order key; replicas that can
//! seal repair it with a null write of its parent group. Every input is merge state, so
//! replicas that cannot seal detach the same rows.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{Field, Hlc, WriterId};
use sqlx::{AssertSqlSafe, Row, SqliteConnection};

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::manifest::{TableSpec, TreeSpec};
use crate::merge::{self, RowState, table_param};
use crate::sql::ident;
use crate::visibility::{RowRef, Visibility};

/// The parent a tree row resolves to, and the rank of its winning parent version.
struct Parent {
    key: String,
    rank: (Hlc, WriterId),
}

/// Parents of the live rows of a tree table that have one, by row key.
async fn parents(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &Visibility,
    table: &TableSpec,
    tree: TreeSpec,
) -> SyncResult<BTreeMap<String, Parent>> {
    let group = table
        .group(tree.parent_group)
        .ok_or_else(|| corrupt("tree parent group is missing"))?;
    let rows = sqlx::query(
        "SELECT v.row_key, v.group_id, v.writer_id, v.seq, v.clock, v.value_hash, v.value
         FROM sync_register_versions v
         JOIN sync_rows r ON r.table_id = v.table_id AND r.row_key = v.row_key
         WHERE v.table_id = ? AND v.group_id = ? AND r.state = 'live'
         ORDER BY v.row_key",
    )
    .bind(table_param(table.id))
    .bind(i64::from(group.id.get()))
    .fetch_all(&mut *conn)
    .await?;
    let mut versions: BTreeMap<String, Vec<merge::Version>> = BTreeMap::new();
    for row in &rows {
        versions
            .entry(row.try_get(0)?)
            .or_default()
            .push(merge::version_from_row(row, 1)?);
    }
    let mut parents = BTreeMap::new();
    for (key, versions) in versions {
        let Some(winner) = merge::winner(engine, table, group, versions.iter()) else {
            continue;
        };
        let Some(Field::Text(named)) = winner.value.fields().first() else {
            continue;
        };
        let Some(parent) = visibility.follow(conn, table.id, named).await? else {
            continue;
        };
        if merge::row_state(conn, table.id, &parent).await? == Some(RowState::Live) {
            parents.insert(
                key,
                Parent {
                    key: parent,
                    rank: (winner.clock, winner.writer),
                },
            );
        }
    }
    Ok(parents)
}

/// The row each cycle of the parent graph is broken at.
fn cycle_breaks(parents: &BTreeMap<String, Parent>) -> BTreeSet<String> {
    // The walk that first reached each row.
    let mut reached: BTreeMap<&str, usize> = BTreeMap::new();
    let mut breaks = BTreeSet::new();
    for (walk, start) in parents.keys().enumerate() {
        let mut path = Vec::new();
        let mut current = start.as_str();
        loop {
            if let Some(&earlier) = reached.get(current) {
                if earlier == walk
                    && let Some(at) = path.iter().position(|row| *row == current)
                {
                    let cycle: &[&str] = &path[at..];
                    if let Some(row) = cycle
                        .iter()
                        .max_by_key(|row| (parents.get(**row).map(|parent| parent.rank), **row))
                    {
                        breaks.insert((*row).to_string());
                    }
                }
                break;
            }
            reached.insert(current, walk);
            path.push(current);
            match parents.get(current) {
                Some(parent) => current = &parent.key,
                None => break,
            }
        }
    }
    breaks
}

/// Rows deeper than `max_depth` once the detached rows are roots.
fn too_deep(
    parents: &BTreeMap<String, Parent>,
    detached: &BTreeSet<String>,
    max_depth: usize,
) -> SyncResult<BTreeSet<String>> {
    let mut depths: BTreeMap<&str, usize> = BTreeMap::new();
    for start in parents.keys() {
        let mut path = Vec::new();
        let mut current = start.as_str();
        let base = loop {
            if let Some(&known) = depths.get(current) {
                break known;
            }
            match parents.get(current).filter(|_| !detached.contains(current)) {
                Some(parent) if path.len() <= parents.len() => {
                    path.push(current);
                    current = &parent.key;
                }
                Some(_) => return Err(corrupt("a tree cycle survived its repair")),
                None => {
                    depths.insert(current, 0);
                    break 0;
                }
            }
        };
        for (offset, row) in path.iter().rev().enumerate() {
            depths.insert(row, base + offset + 1);
        }
    }
    Ok(depths
        .into_iter()
        .filter(|(_, depth)| *depth > max_depth)
        .map(|(row, _)| row.to_string())
        .collect())
}

/// Marks dirty the shown rows whose domain parent disagrees with their detachment: a detached
/// row that still names a parent, or an attached row with a parent that names none.
async fn mark_mismatches(
    conn: &mut SqliteConnection,
    table: &TableSpec,
    tree: TreeSpec,
    parents: &BTreeMap<String, Parent>,
    detached: &BTreeSet<String>,
    dirty: &mut BTreeSet<RowRef>,
) -> SyncResult<()> {
    let column = table
        .group(tree.parent_group)
        .and_then(|group| group.columns().first())
        .ok_or_else(|| corrupt("tree parent group has no column"))?;
    let sql = format!(
        "SELECT {}, {} FROM {}",
        ident(table.key_column),
        ident(column.name),
        ident(table.name)
    );
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(AssertSqlSafe(sql.as_str()))
        .fetch_all(&mut *conn)
        .await?;
    for (key, parent) in rows {
        let mismatch = if detached.contains(&key) {
            parent.is_some()
        } else {
            parent.is_none() && parents.contains_key(&key)
        };
        if mismatch {
            dirty.insert((table.id, key));
        }
    }
    Ok(())
}

/// Detached rows of every tree table. Rows whose domain parent disagrees with their
/// detachment are marked dirty so materialize corrects them.
pub(crate) async fn detached(
    engine: &Engine,
    conn: &mut SqliteConnection,
    visibility: &Visibility,
    dirty: &mut BTreeSet<RowRef>,
) -> SyncResult<BTreeSet<RowRef>> {
    let mut detached = BTreeSet::new();
    for table in engine.manifest().tables {
        let Some(tree) = table.tree else {
            continue;
        };
        let parents = parents(engine, conn, visibility, table, tree).await?;
        let mut rows = cycle_breaks(&parents);
        if let Some(max_depth) = tree.max_depth {
            let deep = too_deep(&parents, &rows, usize::from(max_depth))?;
            rows.extend(deep);
        }
        mark_mismatches(conn, table, tree, &parents, &rows, dirty).await?;
        detached.extend(rows.into_iter().map(|key| (table.id, key)));
    }
    Ok(detached)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer(byte: u8) -> WriterId {
        WriterId::from_bytes([byte; 16])
    }

    fn graph(edges: &[(&str, &str, u64, u8)]) -> BTreeMap<String, Parent> {
        edges
            .iter()
            .map(|(row, parent, clock, writer_byte)| {
                (
                    (*row).to_string(),
                    Parent {
                        key: (*parent).to_string(),
                        rank: (Hlc::from_u64(*clock), writer(*writer_byte)),
                    },
                )
            })
            .collect()
    }

    fn keys(rows: &[&str]) -> BTreeSet<String> {
        rows.iter().map(|row| (*row).to_string()).collect()
    }

    #[test]
    fn a_cycle_breaks_at_its_newest_parent_with_ties_going_to_the_greater_writer() {
        let parents = graph(&[
            ("a", "b", 5, 1),
            ("b", "c", 5, 3),
            ("c", "a", 5, 2),
            ("d", "a", 9, 9),
        ]);
        assert_eq!(cycle_breaks(&parents), keys(&["b"]));
        let parents = graph(&[("a", "b", 7, 1), ("b", "a", 5, 3)]);
        assert_eq!(cycle_breaks(&parents), keys(&["a"]));
    }

    #[test]
    fn a_self_parent_and_separate_cycles_each_break_once() {
        let parents = graph(&[
            ("a", "a", 1, 1),
            ("b", "c", 1, 1),
            ("c", "b", 2, 1),
            ("e", "f", 1, 1),
        ]);
        assert_eq!(cycle_breaks(&parents), keys(&["a", "c"]));
    }

    #[test]
    fn every_row_beyond_the_maximum_depth_detaches_unless_an_ancestor_already_did() {
        let parents = graph(&[
            ("b", "a", 1, 1),
            ("c", "b", 1, 1),
            ("d", "c", 1, 1),
            ("e", "d", 1, 1),
            ("x", "a", 1, 1),
        ]);
        assert_eq!(
            too_deep(&parents, &BTreeSet::new(), 2).unwrap(),
            keys(&["d", "e"])
        );
        assert_eq!(
            too_deep(&parents, &keys(&["c"]), 2).unwrap(),
            BTreeSet::new()
        );
    }
}
