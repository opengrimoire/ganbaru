//! Which live rows materialize.
//!
//! A live row is not materialized while it is a hidden duplicate (it collides with a shown row
//! of a lower key on a unique column set), its existence register holds
//! 0, it is an edge severed to break a cycle, its merged winners fail its publication
//! predicate, or it is hidden by cascade: one of its cascading references, followed through
//! cascading references of live rows, reaches a root. A root is a tombstoned row without a
//! redirect, a removed derived-key row, a severed edge, an unpublished row, a late change, a row
//! waiting for a creation, or, through a
//! composite-key identity reference, a hidden duplicate or a row that was never published.
//! Cascading references other than identity references follow redirects and duplicate keepers
//! first, and a reference to a row that was never published is ignored. The walk keeps a
//! visited set, so a reference cycle hides nothing on its own. Every input is merge state, so
//! replicas that cannot seal repairs still agree on which rows are shown.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ganbaru_sync_contracts::{Field, TableId};
use sqlx::SqliteConnection;

use crate::error::{SyncResult, corrupt};
use crate::manifest::{MergeKind, OnDelete, TableSpec};
use crate::merge::{self, RowState, table_param};
use crate::repair::Contested;
use crate::{Engine, edges, exclusion, publication, repair, sql, tree};

/// Longest chain of redirects and duplicate keepers a reference follows.
pub(crate) const MAX_REDIRECT_DEPTH: usize = 64;

/// Most rounds of choosing duplicate keepers while the rows hidden for other reasons settle.
pub(crate) const MAX_DUPLICATE_ROUNDS: usize = 8;

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
    /// A live row whose merged winners fail its publication predicate.
    Unpublished,
    /// A live row hidden as a late change of a removed device.
    Late,
    /// A live row whose merge state waits for an operation that creates it.
    Uncreated,
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

/// The sync state of one row and whether it waits for a creation.
#[derive(Debug, Clone, Copy)]
struct RowFacts {
    state: Option<RowState>,
    creation_pending: bool,
}

/// Why a live row is hidden by its own merged values, apart from duplicates and edges.
#[derive(Debug, Clone, Copy)]
struct OwnHiding {
    removed: bool,
    unpublished: bool,
    late: bool,
}

impl OwnHiding {
    /// Whether any of them hides the row.
    const fn any(self) -> bool {
        self.removed || self.unpublished || self.late
    }
}

/// Rows hidden by merge state, and the cascade roots computed so far.
///
/// Merge state stays fixed while a visibility is in use, so the merge state it reads for a row
/// is read once and kept with the cascade roots.
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
    /// No late change is recorded, so no row is hidden as one. Unknown unless loaded.
    no_late: bool,
    /// Sync state per row read so far.
    facts: HashMap<RowRef, RowFacts>,
    /// Own hiding per live row read so far.
    own: HashMap<RowRef, OwnHiding>,
}

impl Visibility {
    /// Visibility with the given duplicates and no tree, edge, or cascade state.
    pub(crate) fn with_duplicates(duplicates: Duplicates) -> Self {
        Self {
            duplicates,
            ..Self::default()
        }
    }

    /// Visibility of the current merge state: hidden duplicates, detached tree rows, and severed
    /// edges. Rows whose materialization those may change are marked dirty.
    ///
    /// A colliding row that is hidden for another reason never keeps its class, so a duplicate
    /// is only hidden behind a shown row. Which rows are hidden for other reasons can depend on
    /// the duplicates, so the choice repeats until the excluded rows settle. When they do not
    /// settle within [`MAX_DUPLICATE_ROUNDS`], keepers are chosen without exclusions, which still
    /// shows one row per class and is the same on every replica.
    pub(crate) async fn load(
        engine: &Engine,
        conn: &mut SqliteConnection,
        dirty: &mut BTreeSet<RowRef>,
    ) -> SyncResult<Self> {
        let contested = repair::contested(engine, conn, dirty).await?;
        let mut excluded = BTreeSet::new();
        for _ in 0..MAX_DUPLICATE_ROUNDS {
            let duplicates = repair::keepers(&contested, &excluded);
            let mut visibility = Self::derive(engine, conn, duplicates, dirty).await?;
            let next = visibility
                .otherwise_hidden(engine, conn, &contested)
                .await?;
            if next == excluded {
                visibility
                    .mark_unsettled(engine, conn, &contested, &excluded, dirty)
                    .await?;
                return Ok(visibility);
            }
            excluded = next;
        }
        excluded.clear();
        let duplicates = repair::keepers(&contested, &excluded);
        let visibility = Self::derive(engine, conn, duplicates, dirty).await?;
        visibility
            .mark_unsettled(engine, conn, &contested, &excluded, dirty)
            .await?;
        Ok(visibility)
    }

    /// Visibility with the given duplicates and the tree and edge state that follows from them.
    async fn derive(
        engine: &Engine,
        conn: &mut SqliteConnection,
        duplicates: Duplicates,
        dirty: &mut BTreeSet<RowRef>,
    ) -> SyncResult<Self> {
        let mut visibility = Self::with_duplicates(duplicates);
        visibility.no_late = !exclusion::any_late(conn).await?;
        visibility.detached = tree::detached(engine, conn, &mut visibility, dirty).await?;
        visibility.severed = edges::severed(engine, conn, &visibility, dirty).await?;
        Ok(visibility)
    }

    /// Contested rows that are hidden whether or not they are duplicates: removed, severed,
    /// unpublished, or hidden by cascade.
    async fn otherwise_hidden(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        contested: &Contested,
    ) -> SyncResult<BTreeSet<RowRef>> {
        let mut hidden = BTreeSet::new();
        for (table, classes) in contested {
            let keys: BTreeSet<&String> = classes.iter().flatten().collect();
            for key in keys {
                let facts = self.facts(conn, *table, key).await?;
                if facts.state != Some(RowState::Live) {
                    continue;
                }
                if self.is_severed(*table, key)
                    || self.own_hiding(engine, conn, *table, key).await?.any()
                    || facts.creation_pending
                    || self.cascade_roots(engine, conn, *table, key).await?.hides()
                {
                    hidden.insert((*table, key.clone()));
                }
            }
        }
        Ok(hidden)
    }

    /// Marks dirty every contested row whose domain row is present while it should be hidden as
    /// a duplicate or for another reason, or absent while it should be shown. A keeper can
    /// change because a row elsewhere was deleted or published, without any member of its class
    /// changing.
    async fn mark_unsettled(
        &self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        contested: &Contested,
        excluded: &BTreeSet<RowRef>,
        dirty: &mut BTreeSet<RowRef>,
    ) -> SyncResult<()> {
        for (table, classes) in contested {
            let spec = engine
                .manifest()
                .table(*table)
                .ok_or_else(|| corrupt("duplicate class of an unknown table"))?;
            let keys: BTreeSet<&String> = classes.iter().flatten().collect();
            for key in keys {
                let row = (*table, key.clone());
                let shown = !self.is_duplicate(*table, key) && !excluded.contains(&row);
                let present = sql::read_columns(conn, spec.name, spec.key_column, key, &[])
                    .await?
                    .is_some();
                if shown != present {
                    dirty.insert(row);
                }
            }
        }
        Ok(())
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
        &mut self,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<Option<String>> {
        let mut current = key.to_string();
        for _ in 0..MAX_REDIRECT_DEPTH {
            match self.state(conn, table, &current).await? {
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
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        row: &RowRef,
        identity: bool,
    ) -> SyncResult<Option<RootKind>> {
        let (table, key) = row;
        let facts = self.facts(conn, *table, key).await?;
        Ok(match facts.state {
            None => Some(RootKind::Missing),
            Some(RowState::Tombstoned) => Some(RootKind::Tombstoned),
            Some(RowState::Live) => {
                let own = self.own_hiding(engine, conn, *table, key).await?;
                if own.removed {
                    Some(RootKind::Removed)
                } else if self.is_severed(*table, key) {
                    Some(RootKind::Severed)
                } else if own.unpublished {
                    Some(RootKind::Unpublished)
                } else if own.late {
                    Some(RootKind::Late)
                } else if facts.creation_pending {
                    Some(RootKind::Uncreated)
                } else if identity && self.is_duplicate(*table, key) {
                    Some(RootKind::Duplicate)
                } else {
                    None
                }
            }
        })
    }

    /// Rows the cascading references of a live row reach from its winning versions: identity
    /// references name their row as stored, and the others go through [`Self::follow`] and are
    /// dropped when it reaches nothing. The parent of a detached tree row is not followed.
    pub(crate) async fn cascade_targets(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: &TableSpec,
        key: &str,
    ) -> SyncResult<Vec<CascadeTarget>> {
        let cascades = table
            .references()
            .any(|(_, _, reference)| reference.on_delete == OnDelete::Cascade);
        if !cascades {
            return Ok(Vec::new());
        }
        let versions = merge::versions(engine, conn, table.id, key).await?;
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
                } else if let Some(known) = self.cascade.get(&next) {
                    // The roots a row reaches are cached whole, so they stand for its walk.
                    roots.extend(known.0.iter().cloned());
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
    /// severed, a hidden duplicate, waiting for a creation, failing its publication predicate,
    /// or a late change.
    pub(crate) async fn self_hidden(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<bool> {
        let facts = self.facts(conn, table, key).await?;
        Ok(match facts.state {
            None => false,
            Some(RowState::Tombstoned) => true,
            Some(RowState::Live) => {
                self.is_duplicate(table, key)
                    || facts.creation_pending
                    || self.is_severed(table, key)
                    || self.own_hiding(engine, conn, table, key).await?.any()
            }
        })
    }

    /// The sync state of a row, read once per visibility.
    pub(crate) async fn state(
        &mut self,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<Option<RowState>> {
        Ok(self.facts(conn, table, key).await?.state)
    }

    /// The sync state of a row and whether it waits for a creation, read once per visibility.
    async fn facts(
        &mut self,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<RowFacts> {
        let row = (table, key.to_string());
        if let Some(facts) = self.facts.get(&row) {
            return Ok(*facts);
        }
        let stored: Option<(String, i64)> = sqlx::query_as(
            "SELECT state, creation_pending FROM sync_rows WHERE table_id = ? AND row_key = ?",
        )
        .bind(table_param(table))
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?;
        let facts = match stored {
            None => RowFacts {
                state: None,
                creation_pending: false,
            },
            Some((state, pending)) => RowFacts {
                state: merge::parse_row_state(Some(&state))?,
                creation_pending: pending == 1,
            },
        };
        self.facts.insert(row, facts);
        Ok(facts)
    }

    /// Whether a live row is removed, fails its publication predicate, or is hidden as a late
    /// change of a removed device, read once per visibility.
    async fn own_hiding(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<OwnHiding> {
        let row = (table, key.to_string());
        if let Some(own) = self.own.get(&row) {
            return Ok(*own);
        }
        let Some(spec) = engine.manifest().table(table) else {
            return Err(corrupt("reference to an unknown table"));
        };
        let versions = if spec.exists_group().is_some() || spec.publish_when.is_some() {
            merge::versions(engine, conn, table, key).await?
        } else {
            Vec::new()
        };
        let own = OwnHiding {
            removed: removed_by(engine, spec, &versions),
            unpublished: publication::unpublished(engine, conn, spec, key, &versions).await?,
            late: !self.no_late && exclusion::hides(engine, conn, table, key).await?,
        };
        self.own.insert(row, own);
        Ok(own)
    }

    /// Whether a row without merge state was created by a removed device's excluded operation,
    /// so a domain row left from applying that operation before the removal goes away.
    pub(crate) async fn late_created(
        &self,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<bool> {
        if self.no_late {
            return Ok(false);
        }
        exclusion::created_late(conn, table, key).await
    }

    /// Whether a published row is not materialized.
    pub(crate) async fn hidden(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<bool> {
        if self.state(conn, table, key).await?.is_none() {
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
    if spec.exists_group().is_none() {
        return Ok(false);
    }
    let versions = merge::versions(engine, conn, table, key).await?;
    Ok(removed_by(engine, spec, &versions))
}

/// Whether the winning existence value among a row's retained `versions` records a removal.
fn removed_by(engine: &Engine, spec: &TableSpec, versions: &[merge::Version]) -> bool {
    let Some(exists) = spec.exists_group() else {
        return false;
    };
    let winner = merge::winner(
        engine,
        spec,
        exists,
        versions.iter().filter(|version| version.group == exists.id),
    );
    merge::is_removal(winner.map(|version| &version.value))
}
