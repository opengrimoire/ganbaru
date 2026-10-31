//! Domain code the app registers at engine construction.
//!
//! A [`MaterializeHook`] runs inside the engine transaction after merged rows of its table are
//! written, so state a domain derives from several replicated rows, such as hook columns and
//! count tables, is recomputed with the rows it reads. Hooks receive only the connection and a
//! [`ChangeSink`]: they never see operations or engine state.
//!
//! A hook may also settle the concurrent versions of groups it names in
//! [`MaterializeHook::settled_groups`]: after each materialization, for every row the merge
//! decided whose named group keeps several concurrent versions, the engine passes those versions
//! to [`MaterializeHook::settle`]. A value it returns is written to the domain row and captured
//! like a local edit of that group, so the next seal publishes it causally after every version
//! the hook saw.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use ganbaru_sync_contracts::{GroupId, Hlc, TableId, Value, WriterId};
use sqlx::{AssertSqlSafe, SqliteConnection};

use crate::manifest::{MergeKind, Resolution, TableSpec};
use crate::sql;

/// Error a hook returns; it rolls the engine transaction back.
pub type HookError = Box<dyn Error + Send + Sync>;

/// Future a hook returns.
pub type HookFuture<'a> = Pin<Box<dyn Future<Output = Result<(), HookError>> + Send + 'a>>;

/// Future [`MaterializeHook::settle`] returns: the value that settles the group, or `None` to
/// keep the merge's winner.
pub type SettleFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<Value>, HookError>> + Send + 'a>>;

/// One retained version of a settled group: the versions of a group are mutually concurrent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcurrentVersion {
    /// Writer that sealed it.
    pub writer: WriterId,
    /// Clock of its operation.
    pub clock: Hlc,
    /// Group value, read back when it was elided.
    pub value: Value,
    /// Whether the merge picked it, so the domain row holds it.
    pub won: bool,
}

/// A change a hook reports for the app to refresh, such as a recomputed aggregate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChangeNotice {
    /// What changed, named by the domain.
    pub topic: &'static str,
    /// Key of the changed item within the topic.
    pub key: String,
}

/// Collects the changes hooks report.
pub trait ChangeSink: Send {
    /// Reports that `key` of `topic` changed.
    fn report(&mut self, topic: &'static str, key: String);
}

/// Domain code that runs after the engine materializes rows of one table.
pub trait MaterializeHook: Send + Sync {
    /// Recomputes what the domain derives from the rows of `table` whose keys are in
    /// `changed_keys`: rows inserted, updated, or deleted by the materialization, including
    /// rows a deletion cascaded to. A deleted key no longer has a domain row.
    fn after_materialize<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a>;

    /// Recomputes everything the domain derives from all rows of `table`, for a vault whose
    /// derived state was never computed, such as a snapshot from a server that registers no
    /// hooks. It runs before the vault opens, under the same engine write as
    /// [`MaterializeHook::after_materialize`]. A hook whose work follows the rows of the table
    /// delegates to [`rebuild_every_key`]; one that only reports changes has nothing to do.
    fn rebuild_all<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a>;

    /// Groups of `table` whose concurrent versions [`MaterializeHook::settle`] decides. Each
    /// must be a `Register` or `Coupled` group of columns, without owned tables or
    /// duplicate-repaired columns; [`crate::Engine::with`] refuses others. Its reference columns
    /// may only take a row one of the versions names, see [`MaterializeHook::settle`]. None by
    /// default.
    fn settled_groups(&self, table: &'static TableSpec) -> &'static [GroupId] {
        let _ = table;
        &[]
    }

    /// Decides the concurrent `versions` of a settled `group` of the live row `key`, after the
    /// materialization wrote every row of the pass and before
    /// [`MaterializeHook::after_materialize`] runs. Called for each row the merge decided in
    /// the pass, whether or not its domain row changed, while the group keeps more than one
    /// version.
    ///
    /// A returned value that differs from the winner's is written to the domain row and
    /// captured like a local edit of the group, which the next seal publishes as a write that
    /// supersedes every version passed here; one equal to the winner's writes nothing. The
    /// engine settles only in a pass whose call could seal the capture and while no chain of
    /// apply groups is open, so the chain's last pass decides the state the whole chain
    /// leaves. It skips the row while it holds a local capture, or when this connection may
    /// not edit the table, and reports a value that breaks the group's rules, the adapter's
    /// derived columns, or a constraint of the domain table, or whose reference column names a
    /// row no version names, as an invalid row instead of writing it. A reference is written
    /// resolved, so a row hidden since resolves to null as a winner's would.
    ///
    /// Every device decides the same versions, so the result must depend only on `versions`
    /// and on replicated rows, never on device-local state; devices that settle concurrently
    /// then write equal values, which converge without a conflict. Return `None` whenever
    /// the winner already satisfies the rule, and settle only on a value the static priority
    /// ranks at least as high as every version passed here, so a group that only ever holds settled
    /// values needs no further settle and settles never go back and forth. The hook reads
    /// through `conn` and must not write.
    fn settle<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        key: &'a str,
        group: GroupId,
        versions: &'a [ConcurrentVersion],
    ) -> SettleFuture<'a> {
        let _ = (conn, table, key, group, versions);
        Box::pin(std::future::ready(Ok(None)))
    }
}

/// Keys one [`rebuild_every_key`] call passes to its hook at a time, so a keyed statement stays
/// within SQLite's bound variable limit.
const REBUILD_KEYS_PER_CALL: usize = 500;

/// Runs [`MaterializeHook::after_materialize`] over every row of `table`, for a
/// [`MaterializeHook::rebuild_all`] whose per-key work already recomputes all it derives.
pub fn rebuild_every_key<'a>(
    hook: &'a dyn MaterializeHook,
    conn: &'a mut SqliteConnection,
    table: &'static TableSpec,
    changes: &'a mut dyn ChangeSink,
) -> HookFuture<'a> {
    Box::pin(async move {
        let keys: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT {} FROM {}",
            sql::ident(table.key_column),
            sql::ident(table.name)
        )))
        .fetch_all(&mut *conn)
        .await?;
        for chunk in keys.chunks(REBUILD_KEYS_PER_CALL) {
            hook.after_materialize(&mut *conn, table, chunk, &mut *changes)
                .await?;
        }
        Ok(())
    })
}

/// Hooks the engine runs inside its transactions, at most one per table.
#[derive(Default)]
pub struct Hooks {
    hooks: BTreeMap<TableId, Box<dyn MaterializeHook>>,
}

impl fmt::Debug for Hooks {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_set().entries(self.hooks.keys()).finish()
    }
}

impl Hooks {
    /// No hooks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the hook of `table`, replacing an earlier one.
    pub fn with(mut self, table: TableId, hook: Box<dyn MaterializeHook>) -> Self {
        self.hooks.insert(table, hook);
        self
    }

    /// Tables with a hook.
    pub(crate) fn tables(&self) -> impl Iterator<Item = TableId> + '_ {
        self.hooks.keys().copied()
    }

    /// Hook of a table.
    pub(crate) fn get(&self, table: TableId) -> Option<&dyn MaterializeHook> {
        self.hooks.get(&table).map(|hook| hook.as_ref())
    }

    /// Groups of a table whose concurrent versions its hook settles.
    pub(crate) fn settled_groups(&self, table: &'static TableSpec) -> &'static [GroupId] {
        self.get(table.id)
            .map_or(&[], |hook| hook.settled_groups(table))
    }
}

/// Why a hook may not settle `group` of `table`, see [`MaterializeHook::settled_groups`]. The
/// engine writes a settled value into columns of the row only, so a group whose value fills
/// owned tables or takes part in a duplicate repair is refused. Reference columns are allowed:
/// the engine resolves them like a winner's and accepts only rows a version names.
pub(crate) fn settle_problem(table: &TableSpec, group: GroupId) -> Option<&'static str> {
    let Some(spec) = table.group(group) else {
        return Some("names an unknown group");
    };
    if !matches!(spec.kind, MergeKind::Register | MergeKind::Coupled) {
        return Some("names a group that is neither a register nor coupled");
    }
    if table.is_exists_group(group) || spec.columns().is_empty() {
        return Some("names a group without columns");
    }
    if !spec.owned_tables().is_empty() {
        return Some("names a group with owned tables");
    }
    let repaired = table.resolutions.iter().any(|resolution| match resolution {
        Resolution::DuplicateRepair { columns, .. } => spec
            .columns()
            .iter()
            .any(|column| columns.contains(&column.name)),
        Resolution::WithinGroup { .. } => false,
    });
    if repaired {
        return Some("names a group with duplicate-repaired columns");
    }
    None
}

/// A [`ChangeSink`] that keeps each reported change once.
#[derive(Debug, Default)]
pub(crate) struct Notices(pub(crate) BTreeSet<ChangeNotice>);

impl ChangeSink for Notices {
    fn report(&mut self, topic: &'static str, key: String) {
        self.0.insert(ChangeNotice { topic, key });
    }
}
