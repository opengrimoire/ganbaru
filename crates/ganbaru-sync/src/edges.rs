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
//!
//! The search keeps a level per row that never decreases along a taken edge, the two-level
//! algorithm of Bender, Fineman, Gilbert, and Tarjan. A search walks back through at most the
//! square root of the edge count within one level, then lifts the end of the edge and what it
//! reaches, so an acyclic set of `m` edges costs `O(m^1.5)` however its edges are ordered.
//!
//! A severed edge does not pay for the search that found its cycle, and the same long path can
//! be closed by edge after edge. So a set has a budget of edges walked, [`WALK_BUDGET`] times
//! `m` times the square root of `m`, about twice what an acyclic set can spend. Once it is spent,
//! an edge is taken only when its levels already prove it closes no cycle, and severed otherwise,
//! which keeps the set acyclic and may sever an edge that closes none. Such an edge surfaces as
//! a conflict like any other, and hiding it takes nothing an edge of lower rank could not
//! already take. The budget, the walk order, and what counts as walked are part of the merge
//! rule: every replica severs the same edges only while they agree on them.

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
            || probe.state(engine, conn, target, from).await?.is_none()
            || probe.state(engine, conn, target, to).await?.is_none()
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

/// Edges one set may walk, as a multiple of its edge count times the search limit. It is checked
/// between edges, so a set walks at most one more edge search past it.
pub(crate) const WALK_BUDGET: u64 = 6;

/// How a backward search from the start of an edge ended.
enum Backward {
    /// It reached the end of the edge, so the edge closes a cycle.
    Reached,
    /// It walked every edge of the level.
    Finished,
    /// It stopped at the search limit.
    Limited,
}

/// The taken edges of an acyclic edge set, with a level per row that never decreases along an
/// edge. Rows are dense indexes.
struct Levels {
    level: Vec<u64>,
    /// The taken edges out of each row.
    out: Vec<Vec<usize>>,
    /// The taken edges into each row from a row of the same level.
    same_level: Vec<Vec<usize>>,
    /// The search each row was last marked by.
    mark: Vec<u64>,
    search: u64,
    /// Most edges one backward search walks.
    limit: u64,
    /// Edges walked so far.
    visits: u64,
    /// Edges walked past which no search starts.
    budget: u64,
}

impl Levels {
    fn new(rows: usize, edges: u64) -> Self {
        let limit = ceil_sqrt(edges).max(1);
        Self {
            level: vec![1; rows],
            out: vec![Vec::new(); rows],
            same_level: vec![Vec::new(); rows],
            mark: vec![0; rows],
            search: 0,
            limit,
            visits: 0,
            budget: WALK_BUDGET.saturating_mul(edges).saturating_mul(limit),
        }
    }

    /// Walks same-level edges backward from `from`, marking what it passes, until it meets
    /// `goal` or walks `limit` edges.
    fn backward(&mut self, from: usize, goal: usize) -> Backward {
        self.search += 1;
        self.mark[from] = self.search;
        let mut walked = 0;
        let mut pending = vec![from];
        while let Some(row) = pending.pop() {
            for &previous in &self.same_level[row] {
                self.visits += 1;
                walked += 1;
                if previous == goal {
                    return Backward::Reached;
                }
                if self.mark[previous] != self.search {
                    self.mark[previous] = self.search;
                    pending.push(previous);
                }
                if walked >= self.limit {
                    return Backward::Limited;
                }
            }
        }
        Backward::Finished
    }

    /// Lifts `start` to `level` and what it reaches below that level, and whether that passes a
    /// row marked by the last backward search. The walk finishes even after it passes one: a
    /// severed edge leaves the other edges in place, so their levels must stay in order.
    fn raise(&mut self, start: usize, level: u64) -> bool {
        self.level[start] = level;
        self.same_level[start].clear();
        let mut closes = false;
        let mut pending = vec![start];
        while let Some(row) = pending.pop() {
            for &next in &self.out[row] {
                self.visits += 1;
                closes |= self.mark[next] == self.search;
                if self.level[next] == level {
                    self.same_level[next].push(row);
                } else if self.level[next] < level {
                    self.level[next] = level;
                    self.same_level[next].clear();
                    self.same_level[next].push(row);
                    pending.push(next);
                }
            }
        }
        closes
    }

    /// Takes the edge unless it closes a cycle, and whether it did take it.
    fn insert(&mut self, from: usize, to: usize) -> bool {
        if from == to {
            return false;
        }
        if self.level[from] < self.level[to] {
            self.out[from].push(to);
            return true;
        }
        if self.visits >= self.budget {
            return false;
        }
        let level = match self.backward(from, to) {
            Backward::Reached => return false,
            Backward::Limited => self.level[from] + 1,
            Backward::Finished => self.level[from],
        };
        if self.level[to] < level && self.raise(to, level) {
            return false;
        }
        self.out[from].push(to);
        if self.level[from] == self.level[to] {
            self.same_level[to].push(from);
        }
        true
    }
}

/// The least integer whose square is at least `value`.
fn ceil_sqrt(value: u64) -> u64 {
    let root = value.isqrt();
    if root * root < value { root + 1 } else { root }
}

/// Keys of the edges severed to keep the set acyclic, and the edges walked to find them.
fn sever_counted(mut edges: Vec<Edge>) -> (BTreeSet<String>, u64) {
    edges.sort();
    let mut rows: BTreeMap<&str, usize> = BTreeMap::new();
    let mut ends = Vec::with_capacity(edges.len());
    for edge in &edges {
        let next = rows.len();
        let from = *rows.entry(edge.from.as_str()).or_insert(next);
        let next = rows.len();
        let to = *rows.entry(edge.to.as_str()).or_insert(next);
        ends.push((from, to));
    }
    let mut levels = Levels::new(rows.len(), edges.len() as u64);
    let mut severed = BTreeSet::new();
    for (edge, (from, to)) in edges.iter().zip(ends) {
        if !levels.insert(from, to) {
            severed.insert(edge.key.clone());
        }
    }
    (severed, levels.visits)
}

/// Keys of the edges severed to keep the set acyclic.
fn sever(edges: Vec<Edge>) -> BTreeSet<String> {
    sever_counted(edges).0
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
    probe.scope = visibility.scope.clone();
    for table in engine.manifest().tables {
        let Some(acyclic) = table.acyclic.filter(|_| visibility.in_scope(table.id)) else {
            continue;
        };
        #[cfg(test)]
        crate::visibility::record_derived(table.id);
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

    /// Edges at each hostile bound.
    const HOSTILE_EDGES: u64 = 65_536;

    /// A linear congruential generator, so a seed always replays the same graph.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }

        fn below(&mut self, bound: u64) -> u64 {
            self.next() % bound
        }
    }

    /// Edges in rank order between the numbered rows of `ends`.
    fn numbered(ends: impl IntoIterator<Item = (u64, u64)>) -> Vec<Edge> {
        ends.into_iter()
            .enumerate()
            .map(|(at, (from, to))| {
                let clock = at as u64 + 1;
                edge(
                    &format!("e{at:08}"),
                    &format!("n{from:08}"),
                    &format!("n{to:08}"),
                    clock,
                    1,
                )
            })
            .collect()
    }

    /// The most edges a graph of `m` edges may visit.
    fn bound(m: u64) -> u64 {
        8 * m * ceil_sqrt(m)
    }

    /// Whether the edges outside `severed` form no cycle, by removing rows without an incoming
    /// edge until none remain.
    fn acyclic(edges: &[Edge], severed: &BTreeSet<String>) -> bool {
        let kept = || edges.iter().filter(|edge| !severed.contains(&edge.key));
        let mut incoming: BTreeMap<&str, usize> = BTreeMap::new();
        let mut out: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for edge in kept() {
            incoming.entry(edge.from.as_str()).or_default();
            *incoming.entry(edge.to.as_str()).or_default() += 1;
            out.entry(edge.from.as_str())
                .or_default()
                .push(edge.to.as_str());
        }
        let total = incoming.len();
        let mut free: Vec<&str> = incoming
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(row, _)| *row)
            .collect();
        let mut removed = 0;
        while let Some(row) = free.pop() {
            removed += 1;
            for next in out.get(row).into_iter().flatten() {
                let count = incoming.get_mut(next).expect("every end is counted");
                *count -= 1;
                if *count == 0 {
                    free.push(next);
                }
            }
        }
        removed == total
    }

    /// Asserts the bound and an acyclic result, and returns the severed edges.
    fn assert_within_bound(name: &str, edges: Vec<Edge>) -> BTreeSet<String> {
        let limit = bound(edges.len() as u64);
        let (severed, visits) = sever_counted(edges.clone());
        assert!(visits <= limit, "{name} visited {visits} of {limit} edges");
        assert!(acyclic(&edges, &severed), "{name} kept a cycle");
        severed
    }

    #[test]
    fn sever_stays_within_its_bound_on_hostile_shapes() {
        let m = HOSTILE_EDGES;
        // Each edge starts where the walk from the edges taken so far is longest.
        let reverse = numbered((0..m).rev().map(|at| (at, at + 1)));
        assert!(assert_within_bound("reverse chain", reverse).is_empty());
        let half = m / 2;
        let chain = (0..half).map(|at| (at, at + 1));
        let star = (0..half).map(|at| (m + at, 0));
        assert!(assert_within_bound("chain and star", numbered(chain.chain(star))).is_empty());
        let mut rng = Lcg(1);
        let rows = m / 4;
        let random: Vec<(u64, u64)> = (0..m).map(|_| (rng.below(rows), rng.below(rows))).collect();
        assert_within_bound("random", numbered(random));
    }

    #[test]
    fn edges_closing_one_long_chain_again_and_again_stay_within_the_bound() {
        let m = HOSTILE_EDGES;
        let half = m / 2;
        let chain = (0..half).map(|at| (at, at + 1));
        let closing = (0..half).map(|_| (half, 0));
        let severed = assert_within_bound("closed chain", numbered(chain.chain(closing)));
        assert_eq!(severed.len() as u64, half);
    }

    #[test]
    fn a_spent_budget_severs_only_edges_its_levels_cannot_prove_safe() {
        let mut levels = Levels::new(3, 1);
        levels.level[2] = 2;
        levels.budget = 0;
        assert!(levels.insert(0, 2), "levels prove this edge safe");
        assert!(!levels.insert(1, 0), "levels prove nothing about this edge");
        assert_eq!(levels.visits, 0, "no search runs past the budget");
    }

    #[test]
    fn a_spent_budget_leaves_later_edges_outside_its_component() {
        let length = 400;
        let chain = (0..length).map(|at| (at, at + 1));
        let closing = (0..length).map(|_| (length, 0));
        let fresh = [(2 * length, 2 * length + 1), (2 * length + 1, 2 * length + 2)];
        let edges = numbered(chain.chain(closing).chain(fresh));
        let severed = assert_within_bound("spent chain", edges.clone());
        assert_eq!(severed, reference(edges), "edges of fresh rows stay taken");
    }

    #[test]
    fn sever_matches_the_reference_on_random_graphs() {
        let mut rng = Lcg(2);
        for round in 0..400 {
            let rows = 2 + rng.below(40);
            let m = 1 + rng.below(rows * 4);
            let edges: Vec<Edge> = (0..m)
                .map(|at| {
                    let (from, to) = (rng.below(rows), rng.below(rows));
                    edge(
                        &format!("e{at:04}"),
                        &format!("n{from}"),
                        &format!("n{to}"),
                        rng.below(m / 2 + 1),
                        (rng.below(3) + 1) as u8,
                    )
                })
                .collect();
            assert_eq!(sever(edges.clone()), reference(edges), "round {round}");
        }
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

    /// The severed edges found by walking the taken edges for every edge.
    fn reference(mut edges: Vec<Edge>) -> BTreeSet<String> {
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
}
