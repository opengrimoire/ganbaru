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
//! first. A required reference that reaches nothing hides its row until the target arrives, and
//! a nullable reference to a row that was never published is ignored. Rows of a reference cycle
//! share one result, so a cycle hides nothing on its own, and a walk keeps the result of every row
//! it finishes, so each row is walked once per [`Visibility`]. Every input is merge state, so
//! replicas that cannot seal repairs still agree on which rows are shown.
//!
//! A tombstoned row that its table's [`crate::manifest::RowRetention`] keeps reads as live while
//! its parent is shown, through [`Visibility::state`] and everything built on it. The merge state
//! keeps the tombstone, and the row reads as tombstoned again once the parent goes.
//!
//! The one local input is a row a domain constraint refused, see [`crate::refused`]. It joins
//! through [`Visibility::admit_refusals`] after repairs and row flags are done, and only decides
//! what materializes on this replica.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ganbaru_sync_contracts::{Field, TableId, VersionVector};
use sqlx::SqliteConnection;

use crate::error::{SyncResult, corrupt};
use crate::manifest::{ColumnType, Deletion, MergeKind, OnDelete, RowRetention, TableSpec};
use crate::merge::{self, RowState, table_param};
use crate::repair::Contested;
use crate::{Engine, edges, exclusion, publication, refused, repair, sql, tree};

/// Longest chain of redirects and duplicate keepers a reference follows.
pub(crate) const MAX_REDIRECT_DEPTH: usize = 64;

/// Most rounds of choosing duplicate keepers while the rows hidden for other reasons settle.
pub(crate) const MAX_DUPLICATE_ROUNDS: usize = 8;

/// A row by table and key.
pub(crate) type RowRef = (TableId, String);

/// Hidden duplicates per table, each with the key of the row it collides with and is kept.
pub(crate) type Duplicates = BTreeMap<TableId, BTreeMap<String, String>>;

/// Which tree and acyclic tables a [`Visibility`] derives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeriveScope {
    /// Every table, for reads outside an apply.
    All,
    /// The components of the tables holding a dirty or contested row, and of `alongside`, the
    /// tables of rows an apply writes without marking them dirty.
    Dirty { alongside: BTreeSet<TableId> },
}

impl DeriveScope {
    /// The tables derived, or `None` for every table.
    fn tables(
        self,
        engine: &Engine,
        dirty: &BTreeSet<RowRef>,
        contested: &Contested,
    ) -> Option<BTreeSet<TableId>> {
        let Self::Dirty { mut alongside } = self else {
            return None;
        };
        alongside.extend(dirty.iter().map(|(table, _)| *table));
        alongside.extend(contested.keys().copied());
        Some(engine.components_of(&alongside))
    }
}

#[cfg(test)]
thread_local! {
    /// Tree and acyclic tables derived on this thread since the last [`take_derived`].
    static DERIVED: std::cell::RefCell<Vec<TableId>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Records a derivation of a tree or acyclic table.
#[cfg(test)]
pub(crate) fn record_derived(table: TableId) {
    DERIVED.with_borrow_mut(|derived| derived.push(table));
}

/// The tree and acyclic tables derived on this thread since the last call.
#[cfg(test)]
pub(crate) fn take_derived() -> Vec<TableId> {
    DERIVED.take()
}

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
    /// A live row a domain constraint refused on this replica.
    Refused,
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

/// What the roots a row reaches decide: whether any hides it, and the lowest deleted one. Its size
/// is constant, so a row's result does not grow with the rows below it, each of which may reach a
/// root of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Roots {
    hides: bool,
    cause: Option<Root>,
}

impl Roots {
    /// The result of reaching one root.
    pub(crate) fn of(root: Root) -> Self {
        Self {
            hides: true,
            cause: root.kind.deleted().then_some(root),
        }
    }

    /// Whether the row is hidden by cascade.
    pub(crate) fn hides(&self) -> bool {
        self.hides
    }

    /// The lowest deleted root, which a repair tombstone names as its cause.
    pub(crate) fn cause(&self) -> Option<&Root> {
        self.cause.as_ref()
    }
}

/// The result of a row in a [`CascadeWalk`], which joins the results of the rows it reaches.
pub(crate) trait WalkValue: Clone + Default {
    /// Adds what `other` holds. Joining is commutative and idempotent, so the order of a walk
    /// and a row reached twice change nothing.
    fn join(&mut self, other: &Self);

    /// Whether joining `other` would leave this value as it is.
    fn holds(&self, other: &Self) -> bool;
}

impl WalkValue for Roots {
    fn join(&mut self, other: &Self) {
        self.hides |= other.hides;
        if let Some(cause) = &other.cause
            && self.cause.as_ref().is_none_or(|lowest| cause < lowest)
        {
            self.cause = Some(cause.clone());
        }
    }

    fn holds(&self, other: &Self) -> bool {
        (self.hides || !other.hides)
            && other
                .cause
                .as_ref()
                .is_none_or(|cause| self.cause.as_ref().is_some_and(|lowest| lowest <= cause))
    }
}

impl WalkValue for VersionVector {
    fn join(&mut self, other: &Self) {
        self.merge(other);
    }

    fn holds(&self, other: &Self) -> bool {
        self.dominates(other)
    }
}

/// A row a [`CascadeWalk`] has reached.
struct WalkNode<V> {
    row: RowRef,
    /// The row's own value, from when it is supplied until its component closes.
    own: Option<V>,
    /// Successors not looked at yet.
    remaining: std::vec::IntoIter<RowRef>,
    /// Successors looked at, as node indexes.
    reached: Vec<usize>,
    /// Lowest node index known to be reachable while its component is open.
    low: usize,
    /// The complete result, once the row's component has closed.
    result: Option<Arc<V>>,
}

/// One walk over rows and the rows they reach, as an iterative strongly connected component pass,
/// so the depth of a chain never reaches the stack.
///
/// The driver reads rows, which may be asynchronous, and the walk only keeps count: `next_row`
/// names the row it needs and `supply` answers with the row's own value and successors, until
/// `next_row` returns `None`. A row the driver has a finished result for is answered through
/// `supply_finished`, which ends the walk there.
///
/// Components close successors first. A component's result joins the own values of its members
/// and the results of the components they reach, and every member shares it, so a row in a cycle
/// never holds a partial result. A component that adds nothing to a result it reaches shares that
/// allocation, so the distinct results of a chain follow the rows that add something, not its
/// length.
pub(crate) struct CascadeWalk<V> {
    nodes: Vec<WalkNode<V>>,
    ids: HashMap<RowRef, usize>,
    /// Rows being walked, each reached from the one before it.
    path: Vec<usize>,
    /// Rows of components that have not closed, in the order they were reached.
    open: Vec<usize>,
}

impl<V: WalkValue> CascadeWalk<V> {
    /// A walk from `start`.
    pub(crate) fn new(start: RowRef) -> Self {
        let mut walk = Self {
            nodes: Vec::new(),
            ids: HashMap::new(),
            path: Vec::new(),
            open: Vec::new(),
        };
        walk.enter(start);
        walk
    }

    /// The row the walk needs supplied next, or `None` once every row reached has finished.
    pub(crate) fn next_row(&mut self) -> Option<RowRef> {
        while let Some(&id) = self.path.last() {
            let node = &mut self.nodes[id];
            if node.own.is_none() {
                return Some(node.row.clone());
            }
            match node.remaining.next() {
                Some(next) => self.look_at(id, next),
                None => self.leave(id),
            }
        }
        None
    }

    /// Answers the row [`Self::next_row`] named.
    pub(crate) fn supply(&mut self, row: RowRef, own: V, successors: Vec<RowRef>) {
        let Some(&id) = self.path.last() else {
            debug_assert!(false, "supplied a row to a finished walk");
            return;
        };
        let node = &mut self.nodes[id];
        debug_assert_eq!(node.row, row, "supplied a row the walk did not name");
        node.own = Some(own);
        node.remaining = successors.into_iter();
    }

    /// Answers the row [`Self::next_row`] named with the result an earlier walk finished for it.
    pub(crate) fn supply_finished(&mut self, row: RowRef, result: Arc<V>) {
        let Some(id) = self.path.pop() else {
            debug_assert!(false, "supplied a row to a finished walk");
            return;
        };
        let node = &mut self.nodes[id];
        debug_assert_eq!(node.row, row, "supplied a row the walk did not name");
        node.result = Some(result);
        self.open.pop();
    }

    /// Every row the walk finished with its complete result, once [`Self::next_row`] has
    /// returned `None`.
    pub(crate) fn finish(self) -> Vec<(RowRef, Arc<V>)> {
        self.nodes
            .into_iter()
            .filter_map(|node| Some((node.row, node.result?)))
            .collect()
    }

    /// Starts walking a row reached for the first time.
    fn enter(&mut self, row: RowRef) -> usize {
        let id = self.nodes.len();
        self.ids.insert(row.clone(), id);
        self.nodes.push(WalkNode {
            row,
            own: None,
            remaining: Vec::new().into_iter(),
            reached: Vec::new(),
            low: id,
            result: None,
        });
        self.path.push(id);
        self.open.push(id);
        id
    }

    /// Follows the reference from node `id` to `next`.
    fn look_at(&mut self, id: usize, next: RowRef) {
        let target = match self.ids.get(&next) {
            Some(target) => *target,
            None => self.enter(next),
        };
        let open = self.nodes[target].result.is_none();
        let node = &mut self.nodes[id];
        node.reached.push(target);
        if open {
            node.low = node.low.min(target);
        }
    }

    /// Ends the walk of node `id`, whose successors have all been looked at.
    fn leave(&mut self, id: usize) {
        self.path.pop();
        let low = self.nodes[id].low;
        if let Some(&parent) = self.path.last() {
            let parent = &mut self.nodes[parent];
            parent.low = parent.low.min(low);
        }
        if low == id {
            self.close(id);
        }
    }

    /// Closes the component whose first reached row is node `first`.
    fn close(&mut self, first: usize) {
        let start = self
            .open
            .iter()
            .rposition(|id| *id == first)
            .unwrap_or_default();
        let members = self.open.split_off(start);
        let mut value = Arc::new(V::default());
        for &member in &members {
            let node = &mut self.nodes[member];
            let own = node.own.take();
            let reached = std::mem::take(&mut node.reached);
            if let Some(own) = own
                && !value.holds(&own)
            {
                Arc::make_mut(&mut value).join(&own);
            }
            for target in reached {
                let Some(result) = &self.nodes[target].result else {
                    continue;
                };
                if result.holds(&value) {
                    value = Arc::clone(result);
                } else if !value.holds(result) {
                    Arc::make_mut(&mut value).join(result);
                }
            }
        }
        for member in members {
            self.nodes[member].result = Some(Arc::clone(&value));
        }
    }
}

/// A cascading reference of a row: the row it reaches and whether it is a composite-key
/// identity reference, which is never redirected.
pub(crate) struct CascadeTarget {
    pub(crate) row: RowRef,
    pub(crate) identity: bool,
    /// A required reference that reaches nothing: [`Visibility::follow`] cannot resolve the row
    /// it names, so the referrer hides with [`RootKind::Missing`].
    pub(crate) unresolved: bool,
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
/// is read once and kept with the cascade roots. [`Visibility::state`] is the effective state,
/// which retention decides; recovery of a deletion reads the raw state instead.
#[derive(Debug, Default)]
pub(crate) struct Visibility {
    /// Hidden duplicates.
    pub(crate) duplicates: Duplicates,
    /// Tree rows whose parent materializes as null to break a cycle or limit depth.
    pub(crate) detached: BTreeSet<RowRef>,
    /// Edges of acyclic edge sets that are hidden to break a cycle.
    pub(crate) severed: BTreeSet<RowRef>,
    /// Tables whose tree and edge state was derived, or `None` for every table.
    pub(crate) scope: Option<BTreeSet<TableId>>,
    /// Complete roots per row a walk finished; a shown row maps to no roots.
    cascade: HashMap<RowRef, Arc<Roots>>,
    /// Complete recovery coverage per row a walk finished. Coverage follows cascade causes, so
    /// it is dropped with them.
    coverage: HashMap<RowRef, Arc<VersionVector>>,
    /// No late change is recorded, so no row is hidden as one. Unknown unless loaded.
    no_late: bool,
    /// Sync state per row read so far.
    facts: HashMap<RowRef, RowFacts>,
    /// Own hiding per live row read so far.
    own: HashMap<RowRef, OwnHiding>,
    /// Whether a row of a table with a retention rule is retained, per row read so far. It
    /// follows its parent's visibility, so it is dropped with the cascade roots.
    retained: HashMap<RowRef, bool>,
    /// Refused rows count as hidden: they were admitted and at least one is listed.
    refusals_on: bool,
    /// Listed rows that are not hidden, since this materialization writes them again.
    retrying: BTreeSet<RowRef>,
    /// Whether a row is listed as hidden, per row read so far.
    refused: HashMap<RowRef, bool>,
    /// Rows whose cascading references were read.
    #[cfg(test)]
    pub(crate) cascade_walks: usize,
    /// Rows whose coverage was read.
    #[cfg(test)]
    pub(crate) coverage_rows: usize,
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
    ///
    /// A scoped load leaves the trees and edge sets of other components underived. A mismatch
    /// there can only come from a row of that component, which puts it in scope at the finish
    /// that touches it. Contested rows count toward the scope because keepers read their
    /// hiding wherever they are.
    pub(crate) async fn load(
        engine: &Engine,
        conn: &mut SqliteConnection,
        dirty: &mut BTreeSet<RowRef>,
        scope: DeriveScope,
    ) -> SyncResult<Self> {
        let contested = repair::contested(engine, conn, dirty).await?;
        let scope = scope.tables(engine, dirty, &contested);
        let mut excluded = BTreeSet::new();
        for _ in 0..MAX_DUPLICATE_ROUNDS {
            let duplicates = repair::keepers(&contested, &excluded);
            let mut visibility = Self::derive(engine, conn, duplicates, &scope, dirty).await?;
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
        let mut visibility = Self::derive(engine, conn, duplicates, &scope, dirty).await?;
        visibility
            .mark_unsettled(engine, conn, &contested, &excluded, dirty)
            .await?;
        Ok(visibility)
    }

    /// Visibility with the given duplicates and the tree and edge state that follows from them
    /// for the tables of `scope`.
    async fn derive(
        engine: &Engine,
        conn: &mut SqliteConnection,
        duplicates: Duplicates,
        scope: &Option<BTreeSet<TableId>>,
        dirty: &mut BTreeSet<RowRef>,
    ) -> SyncResult<Self> {
        let mut visibility = Self::with_duplicates(duplicates);
        visibility.scope = scope.clone();
        visibility.no_late = !exclusion::any_late(conn).await?;
        visibility.detached = tree::detached(engine, conn, &mut visibility, dirty).await?;
        visibility.severed = edges::severed(engine, conn, &visibility, dirty).await?;
        #[cfg(test)]
        visibility
            .assert_scope_matches_all(engine, conn, dirty)
            .await?;
        Ok(visibility)
    }

    /// Asserts that a scoped derivation equals the full one restricted to its scope.
    #[cfg(test)]
    async fn assert_scope_matches_all(
        &self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        dirty: &BTreeSet<RowRef>,
    ) -> SyncResult<()> {
        let Some(scope) = &self.scope else {
            return Ok(());
        };
        let derived = take_derived();
        let mut full = Self::with_duplicates(self.duplicates.clone());
        full.no_late = self.no_late;
        let mut unused = dirty.clone();
        full.detached = tree::detached(engine, conn, &mut full, &mut unused).await?;
        full.severed = edges::severed(engine, conn, &full, &mut unused).await?;
        take_derived();
        DERIVED.set(derived);
        let within = |rows: &BTreeSet<RowRef>| -> BTreeSet<RowRef> {
            rows.iter()
                .filter(|(table, _)| scope.contains(table))
                .cloned()
                .collect()
        };
        assert_eq!(
            self.detached,
            within(&full.detached),
            "scoped detached rows"
        );
        assert_eq!(self.severed, within(&full.severed), "scoped severed edges");
        Ok(())
    }

    /// Whether the tree and edge state of a table was derived.
    pub(crate) fn in_scope(&self, table: TableId) -> bool {
        self.scope
            .as_ref()
            .is_none_or(|scope| scope.contains(&table))
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
                let facts = self.effective(engine, conn, *table, key).await?;
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
    /// changing. A hidden row that [`Self::keeps_hidden`] keeps present is settled, and so is a
    /// row whose listed refusal explains the difference, which its retry revisits.
    async fn mark_unsettled(
        &mut self,
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
                if shown == present
                    || (!shown && self.keeps_hidden(engine, conn, spec, key).await?)
                    || refused::explains(conn, &row, shown).await?
                {
                    continue;
                }
                dirty.insert(row);
            }
        }
        Ok(())
    }

    /// Lets rows a domain constraint refused hide, except `retrying`. Called only after repairs
    /// and row flags, because the list is local and everything before them must be the same on
    /// every replica.
    pub(crate) async fn admit_refusals(
        &mut self,
        conn: &mut SqliteConnection,
        retrying: BTreeSet<RowRef>,
    ) -> SyncResult<()> {
        self.refusals_on = refused::any(conn).await?;
        self.retrying = retrying;
        self.cascade.clear();
        self.coverage.clear();
        self.retained.clear();
        Ok(())
    }

    /// The recovery coverage a walk finished for a row.
    pub(crate) fn known_coverage(&self, row: &RowRef) -> Option<Arc<VersionVector>> {
        self.coverage.get(row).cloned()
    }

    /// Keeps the rows a coverage walk finished.
    pub(crate) fn keep_coverage(&mut self, rows: Vec<(RowRef, Arc<VersionVector>)>) {
        self.coverage.extend(rows);
    }

    /// Whether a row is hidden because a domain constraint refused it.
    pub(crate) async fn is_refused(
        &mut self,
        conn: &mut SqliteConnection,
        row: &RowRef,
    ) -> SyncResult<bool> {
        if !self.refusals_on || self.retrying.contains(row) {
            return Ok(false);
        }
        if let Some(refused) = self.refused.get(row) {
            return Ok(*refused);
        }
        let hidden = refused::is_hidden(conn, row).await?;
        self.refused.insert(row.clone(), hidden);
        Ok(hidden)
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
        #[cfg(test)]
        assert!(self.in_scope(table), "severed edges read outside the scope");
        self.severed.contains(&(table, key.to_string()))
    }

    /// Whether a tree row's parent materializes as null.
    pub(crate) fn is_detached(&self, table: TableId, key: &str) -> bool {
        #[cfg(test)]
        assert!(self.in_scope(table), "detached rows read outside the scope");
        self.detached.contains(&(table, key.to_string()))
    }

    /// The row a reference that may be redirected reaches: itself while live and not a hidden
    /// duplicate, the kept row of a hidden duplicate, the lowest redirect of a tombstone, or a
    /// tombstoned row without one. `None` when the chain reaches a row that was never published
    /// or exceeds [`MAX_REDIRECT_DEPTH`].
    pub(crate) async fn follow(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<Option<String>> {
        let mut current = key.to_string();
        for _ in 0..MAX_REDIRECT_DEPTH {
            match self.state(engine, conn, table, &current).await? {
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
        let facts = self.effective(engine, conn, *table, key).await?;
        Ok(match facts.state {
            None => Some(RootKind::Missing),
            Some(RowState::Tombstoned) => Some(RootKind::Tombstoned),
            Some(RowState::Live) => {
                let own = self.own_hiding(engine, conn, *table, key).await?;
                if self.is_refused(conn, row).await? {
                    Some(RootKind::Refused)
                } else if own.removed {
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
    /// dropped when it reaches nothing, unless the column is required. The parent of a detached
    /// tree row is not followed. A key parent is named by the row's own key, as an identity.
    pub(crate) async fn cascade_targets(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: &TableSpec,
        key: &str,
    ) -> SyncResult<Vec<CascadeTarget>> {
        #[cfg(test)]
        {
            self.cascade_walks += 1;
        }
        let mut targets: Vec<CascadeTarget> = table
            .key_parent
            .map(|parent| CascadeTarget {
                row: (parent, key.to_string()),
                identity: true,
                unresolved: false,
            })
            .into_iter()
            .collect();
        let cascades = table
            .references()
            .any(|(_, _, reference)| reference.on_delete == OnDelete::Cascade);
        if !cascades {
            return Ok(targets);
        }
        let versions = merge::versions(engine, conn, table.id, key).await?;
        let winners = merge::winners(engine, table, &versions);
        let detached_parent = self
            .is_detached(table.id, key)
            .then(|| table.tree.map(|tree| tree.parent_group))
            .flatten();
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
            let followed = if identity {
                Some(target.clone())
            } else {
                self.follow(engine, conn, reference.table, target).await?
            };
            match followed {
                Some(followed) => targets.push(CascadeTarget {
                    row: (reference.table, followed),
                    identity,
                    unresolved: false,
                }),
                None if group.columns()[field].ty == ColumnType::RowKey => {
                    targets.push(CascadeTarget {
                        row: (reference.table, target.clone()),
                        identity,
                        unresolved: true,
                    });
                }
                None => {}
            }
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
    ) -> SyncResult<Arc<Roots>> {
        let start = (table, key.to_string());
        if let Some(roots) = self.cascade.get(&start) {
            return Ok(Arc::clone(roots));
        }
        let mut walk = CascadeWalk::new(start.clone());
        while let Some(row) = walk.next_row() {
            if let Some(known) = self.cascade.get(&row) {
                walk.supply_finished(row, Arc::clone(known));
            } else {
                let (own, successors) = self.direct_roots(engine, conn, &row).await?;
                walk.supply(row, own, successors);
            }
        }
        self.cascade.extend(walk.finish());
        self.cascade
            .get(&start)
            .cloned()
            .ok_or_else(|| corrupt("cascade walk left its start row"))
    }

    /// The roots the cascading references of a row reach directly, and the rows they reach that
    /// a walk continues through.
    async fn direct_roots(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        (table, key): &RowRef,
    ) -> SyncResult<(Roots, Vec<RowRef>)> {
        let spec = engine
            .manifest()
            .table(*table)
            .ok_or_else(|| corrupt("cascade through an unknown table"))?;
        let mut roots = Roots::default();
        let mut successors = Vec::new();
        for target in self.cascade_targets(engine, conn, spec, key).await? {
            let kind = if target.unresolved {
                Some(RootKind::Missing)
            } else {
                self.root_kind(engine, conn, &target.row, target.identity)
                    .await?
            };
            let (table, key) = target.row;
            match kind {
                Some(kind) => roots.join(&Roots::of(Root { table, key, kind })),
                None => successors.push((table, key)),
            }
        }
        Ok((roots, successors))
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
        let facts = self.effective(engine, conn, table, key).await?;
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

    /// The effective sync state of a row: a retained tombstoned row reads as live.
    pub(crate) async fn state(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<Option<RowState>> {
        Ok(self.effective(engine, conn, table, key).await?.state)
    }

    /// [`Self::facts`] with a retained tombstoned row read as live.
    async fn effective(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<RowFacts> {
        let facts = self.facts(conn, table, key).await?;
        if facts.state == Some(RowState::Tombstoned)
            && self.retained(engine, conn, table, key).await?
        {
            return Ok(RowFacts {
                state: Some(RowState::Live),
                ..facts
            });
        }
        Ok(facts)
    }

    /// Whether a tombstone of this row is overridden: its table's retention rule selects the row
    /// from its winning versions, and the parent they name is live and shown. Reads merge state
    /// and never the domain row, so every replica answers the same. The future is boxed because
    /// reading the parent's visibility comes back through this function's callers.
    pub(crate) fn retained<'a>(
        &'a mut self,
        engine: &'a Engine,
        conn: &'a mut SqliteConnection,
        table: TableId,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = SyncResult<bool>> + Send + 'a>> {
        Box::pin(async move {
            let Some(spec) = engine.manifest().table(table) else {
                return Err(corrupt("retention read of an unknown table"));
            };
            let Some(rule) = spec.retained else {
                return Ok(false);
            };
            let row = (table, key.to_string());
            if let Some(retained) = self.retained.get(&row) {
                return Ok(*retained);
            }
            let versions = merge::versions(engine, conn, table, key).await?;
            let winners = merge::winners(engine, spec, &versions);
            let parent = winners
                .get(&rule.group)
                .and_then(|winner| retention_parent(spec, rule, winner.value.fields()));
            let retained = match parent {
                Some(parent) => {
                    self.state(engine, conn, rule.parent, &parent).await? == Some(RowState::Live)
                        && !self.hidden(engine, conn, rule.parent, &parent).await?
                }
                None => false,
            };
            self.retained.insert(row, retained);
            Ok(retained)
        })
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

    /// Whether a live row is hidden as a late change of a removed device.
    pub(crate) async fn is_late(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<bool> {
        Ok(self.own_hiding(engine, conn, table, key).await?.late)
    }

    /// Whether a hidden row stays in its domain table, as a hidden row of a never-deleted table
    /// does. Exclusion still undoes one that is hidden as a late change of a removed device and
    /// that no operation which still counts created.
    pub(crate) async fn keeps_hidden(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: &TableSpec,
        key: &str,
    ) -> SyncResult<bool> {
        if table.deletion != Deletion::Never {
            return Ok(false);
        }
        Ok(!self.is_late(engine, conn, table.id, key).await?
            || exclusion::has_creation(conn, table.id, key).await?)
    }

    /// Whether a published row is not materialized.
    pub(crate) async fn hidden(
        &mut self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        table: TableId,
        key: &str,
    ) -> SyncResult<bool> {
        if self.state(engine, conn, table, key).await?.is_none() {
            return Ok(false);
        }
        if self.is_refused(conn, &(table, key.to_string())).await? {
            return Ok(true);
        }
        Ok(self.self_hidden(engine, conn, table, key).await?
            || self.cascade_roots(engine, conn, table, key).await?.hides())
    }
}

/// The parent row `rule` retains a row under, from the winning `fields` of its group, when the
/// fields select the row.
fn retention_parent(spec: &TableSpec, rule: RowRetention, fields: &[Field]) -> Option<String> {
    let group = spec.group(rule.group)?;
    let selector = group
        .columns()
        .iter()
        .position(|column| column.name == rule.column)?;
    let selected = matches!(fields.get(selector), Some(Field::Text(value)) if value == rule.value);
    if !selected {
        return None;
    }
    let (field, _) = group
        .reference_columns()
        .find(|(_, reference)| reference.table == rule.parent)?;
    match fields.get(field) {
        Some(Field::Text(parent)) => Some(parent.clone()),
        _ => None,
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
