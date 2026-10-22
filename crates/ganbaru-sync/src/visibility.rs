//! Which live rows materialize.
//!
//! A live row is not materialized while it is a hidden duplicate, its existence register holds
//! 0, it is an edge severed to break a cycle, or it is hidden by cascade: one of its cascading
//! references, followed through cascading references of live rows, reaches a root. A root is a
//! tombstoned row without a redirect, a removed derived-key row, a severed edge, or, through a
//! composite-key identity reference, a hidden duplicate or a row that was never published.
//! Cascading references other than identity references follow redirects and duplicate keepers
//! first, and a reference to a row that was never published is ignored. The walk keeps a
//! visited set, so a reference cycle hides nothing on its own. Every input is merge state, so
//! replicas that cannot seal repairs still agree on which rows are shown.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ganbaru_sync_contracts::{Field, TableId};
use sqlx::SqliteConnection;

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::manifest::{MergeKind, OnDelete, TableSpec};
use crate::merge::{self, RowState, table_param};

/// Longest chain of redirects and duplicate keepers a reference follows.
pub(crate) const MAX_REDIRECT_DEPTH: usize = 64;

/// A row by table and key.
pub(crate) type RowRef = (TableId, String);

/// Hidden duplicates per table, each with the key of the row it collides with and is kept.
pub(crate) type Duplicates = BTreeMap<TableId, BTreeMap<String, String>>;

/// Why a root hides the rows whose cascading references reach it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RootKind {
    /// Tombstoned without a redirect, or tombstoned and named by an identity reference.
    Tombstoned,
    /// A derived-key row whose existence register holds 0.
    Removed,
    /// A hidden duplicate named by an identity reference.
    Duplicate,
    /// An edge severed to break a cycle.
    Severed,
    /// A row named by an identity reference that was never published.
    Missing,
}

impl RootKind {
    /// Whether the root was deleted, which makes it the cause of the rows it hides.
    pub(crate) const fn deleted(self) -> bool {
        matches!(self, Self::Tombstoned | Self::Removed)
    }
}

/// A row that hides the rows whose cascading references reach it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Root {
    pub(crate) table: TableId,
    pub(crate) key: String,
    pub(crate) kind: RootKind,
}

/// The roots a cascade-hidden row reaches, lowest first; empty while the row is shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Roots(BTreeSet<Root>);

impl Roots {
    /// Whether the row is hidden by cascade.
    pub(crate) fn hides(&self) -> bool {
        !self.0.is_empty()
    }

    /// The lowest deleted root, which a repair tombstone names as its cause.
    pub(crate) fn cause(&self) -> Option<&Root> {
        self.0.iter().find(|root| root.kind.deleted())
    }
}

/// A cascading reference of a row: the row it reaches and whether it is a composite-key
/// identity reference, which is never redirected.
pub(crate) struct CascadeTarget {
    pub(crate) row: RowRef,
    pub(crate) identity: bool,
}

/// Rows hidden by merge state, and the cascade roots computed so far.
#[derive(Debug, Default)]
pub(crate) struct Visibility {
    /// Hidden duplicates.
    pub(crate) duplicates: Duplicates,
    /// Tree rows whose parent materializes as null to break a cycle or limit depth.
    pub(crate) detached: BTreeSet<RowRef>,
    /// Edges of acyclic edge sets that are hidden to break a cycle.
    pub(crate) severed: BTreeSet<RowRef>,
    /// Roots per row; a shown row maps to no roots.
    cascade: HashMap<RowRef, Roots>,
}

impl Visibility {
    /// Visibility with the given duplicates and no tree, edge, or cascade state.
    pub(crate) fn with_duplicates(duplicates: Duplicates) -> Self {
        Self {
            duplicates,
            ..Self::default()
        }
    }

    /// The kept row of a hidden duplicate.
    pub(crate) fn keeper(&self, table: TableId, key: &str) -> Option<&String> {
        self.duplicates.get(&table).and_then(|rows| rows.get(key))
    }

    /// Whether a row is a hidden duplicate.
    pub(crate) fn is_duplicate(&self, table: TableId, key: &str) -> bool {
        self.keeper(table, key).is_some()
    }

    /// Whether a row is an edge severed to break a cycle.
    pub(crate) fn is_severed(&self, table: TableId, key: &str) -> bool {
        self.severed.contains(&(table, key.to_string()))
    }

    /// Whether a tree row's parent materializes as null.
    pub(crate) fn is_detached(&self, table: TableId, key: &str) -> bool {
        self.detached.contains(&(table, key.to_string()))
    }

    /// The row a reference that may be redirected reaches: itself while live and not a hidden
    /// duplicate, the kept row of a hidden duplicate, the lowest redirect of a tombstone, or a
    /// tombstoned row without one. `None` when the chain reaches a row that was never published
    /// or exceeds [`MAX_REDIRECT_DEPTH`].
    pub(crate) async fn follow(
        &self,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<Option<String>> {
        let mut current = key.to_string();
        for _ in 0..MAX_REDIRECT_DEPTH {
            match merge::row_state(conn, table, &current).await? {
                None => return Ok(None),
                Some(RowState::Live) => match self.keeper(table, &current) {
                    Some(keeper) => current = keeper.clone(),
                    None => return Ok(Some(current)),
                },
                Some(RowState::Tombstoned) => match lowest_redirect(conn, table, &current).await? {
                    Some(next) => current = next,
                    None => return Ok(Some(current)),
                },
            }
        }
        Ok(None)
    }

    /// Why a row reached by a cascading reference hides its referrer, if it does by itself.
    async fn root_kind(
        &self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        row: &RowRef,
        identity: bool,
    ) -> SyncResult<Option<RootKind>> {
        let (table, key) = row;
        Ok(match merge::row_state(conn, *table, key).await? {
            None => Some(RootKind::Missing),
            Some(RowState::Tombstoned) => Some(RootKind::Tombstoned),
            Some(RowState::Live) if removed(engine, conn, *table, key).await? => {
                Some(RootKind::Removed)
            }
            Some(RowState::Live) if self.is_severed(*table, key) => Some(RootKind::Severed),
            Some(RowState::Live) if identity && self.is_duplicate(*table, key) => {
                Some(RootKind::Duplicate)
            }
            Some(RowState::Live) => None,
        })
    }

    /// Rows the cascading references of a live row reach from its winning versions: identity
    /// references name their row as stored, and the others go through [`Self::follow`] and are
    /// dropped when it reaches nothing. The parent of a detached tree row is not followed.
    pub(crate) async fn cascade_targets(
        &self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: &TableSpec,
        key: &str,
    ) -> SyncResult<Vec<CascadeTarget>> {
        let versions = merge::versions(conn, table.id, key).await?;
        let winners = merge::winners(engine, table, &versions);
        let detached_parent = self
            .is_detached(table.id, key)
            .then(|| table.tree.map(|tree| tree.parent_group))
            .flatten();
        let mut targets = Vec::new();
        for (group, field, reference) in table.references() {
            if reference.on_delete != OnDelete::Cascade || detached_parent == Some(group.id) {
                continue;
            }
            let Some(version) = winners.get(&group.id) else {
                continue;
            };
            let Some(Field::Text(target)) = version.value.fields().get(field) else {
                continue;
            };
            let identity = table.has_composite_key() && group.kind == MergeKind::Immutable;
            let target = if identity {
                target.clone()
            } else {
                match self.follow(conn, reference.table, target).await? {
                    Some(target) => target,
                    None => continue,
                }
            };
            targets.push(CascadeTarget {
                row: (reference.table, target),
                identity,
            });
        }
        Ok(targets)
    }

    /// The roots a live row reaches through cascading references; empty while it is shown.
    pub(crate) async fn cascade_roots(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<Roots> {
        let start = (table, key.to_string());
        if let Some(roots) = self.cascade.get(&start) {
            return Ok(roots.clone());
        }
        let mut visited = BTreeSet::from([start.clone()]);
        let mut pending = vec![start.clone()];
        let mut roots = BTreeSet::new();
        while let Some((table, key)) = pending.pop() {
            let spec = engine
                .manifest()
                .table(table)
                .ok_or_else(|| corrupt("cascade through an unknown table"))?;
            for target in self.cascade_targets(engine, conn, spec, &key).await? {
                let next = target.row;
                if let Some(kind) = self.root_kind(engine, conn, &next, target.identity).await? {
                    roots.insert(Root {
                        table: next.0,
                        key: next.1,
                        kind,
                    });
                } else if visited.insert(next.clone()) {
                    pending.push(next);
                }
            }
        }
        let roots = Roots(roots);
        if roots.hides() {
            self.cascade.insert(start, roots.clone());
        } else {
            // No visited row reaches a root either, so all of them are shown.
            for row in visited {
                self.cascade.insert(row, Roots::default());
            }
        }
        Ok(roots)
    }

    /// Whether a row is hidden by its own merge state, before cascade: tombstoned, removed,
    /// severed, or a hidden duplicate.
    pub(crate) async fn self_hidden(
        &self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<bool> {
        Ok(match merge::row_state(conn, table, key).await? {
            None => false,
            Some(RowState::Tombstoned) => true,
            Some(RowState::Live) => {
                self.is_duplicate(table, key)
                    || self.is_severed(table, key)
                    || removed(engine, conn, table, key).await?
            }
        })
    }

    /// Whether a published row is not materialized.
    pub(crate) async fn hidden(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<bool> {
        if merge::row_state(conn, table, key).await?.is_none() {
            return Ok(false);
        }
        Ok(self.self_hidden(engine, conn, table, key).await?
            || self.cascade_roots(engine, conn, table, key).await?.hides())
    }
}

/// The lowest redirect of a tombstoned row.
pub(crate) async fn lowest_redirect(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT min(replaced_by) FROM sync_tombstones WHERE table_id = ? AND row_key = ?",
    )
    .bind(table_param(table))
    .bind(key)
    .fetch_one(&mut *conn)
    .await?)
}

/// Whether the existence register of a derived-key row holds 0. Such a row is deleted for
/// every reference to it, like a tombstone without a redirect.
pub(crate) async fn removed(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<bool> {
    let Some(spec) = engine.manifest().table(table) else {
        return Err(corrupt("reference to an unknown table"));
    };
    let Some(exists) = spec.exists_group() else {
        return Ok(false);
    };
    let versions = merge::versions(conn, table, key).await?;
    let winner = merge::winner(
        engine,
        spec,
        exists,
        versions.iter().filter(|version| version.group == exists.id),
    );
    Ok(merge::is_removal(winner.map(|version| &version.value)))
}
