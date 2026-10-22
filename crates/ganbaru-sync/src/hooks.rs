//! Domain code the app registers at engine construction.
//!
//! A [`MaterializeHook`] runs inside the engine transaction after merged rows of its table are
//! written, so state a domain derives from several replicated rows, such as hook columns and
//! count tables, is recomputed with the rows it reads. Hooks receive only the connection and a
//! [`ChangeSink`]: they never see operations, versions, or engine state.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use ganbaru_sync_contracts::TableId;
use sqlx::SqliteConnection;

use crate::manifest::TableSpec;

/// Error a hook returns; it rolls the engine transaction back.
pub type HookError = Box<dyn Error + Send + Sync>;

/// Future a hook returns.
pub type HookFuture<'a> = Pin<Box<dyn Future<Output = Result<(), HookError>> + Send + 'a>>;

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
}

/// A [`ChangeSink`] that keeps each reported change once.
#[derive(Debug, Default)]
pub(crate) struct Notices(pub(crate) BTreeSet<ChangeNotice>);

impl ChangeSink for Notices {
    fn report(&mut self, topic: &'static str, key: String) {
        self.0.insert(ChangeNotice { topic, key });
    }
}
