//! Recovery offers: deleted rows that kept edits their deletion did not see. Restoring and
//! discarding seal with the local writer, so the service runs them inside a pass.
//!
//! The owning domain restores a row through its own persistence. The app registers one
//! restorer per table name that can root an offer, and only registered tables accept a choice.

use std::collections::BTreeMap;
use std::pin::Pin;

use super::writer::ActiveWriter;
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::{Engine, RecoveryDescendant, SpaceContext};
use ganbaru_sync_contracts::{GroupId, TableId, Value};
use serde::Deserialize;
use sqlx::{SqliteConnection, SqlitePool};

/// What to do with a recovery offer.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    /// Create a new row from the retained values, then close the offer.
    Restore,
    /// Close the offer without restoring.
    Discard,
}

/// A recovery action on one offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryChoice {
    pub table: TableId,
    pub row_key: String,
    pub action: RecoveryAction,
}

/// Retained values of a row offered for recovery and of the deleted descendants that restoring
/// it brings back, nearest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveredRow {
    /// Table of the root row.
    pub table: TableId,
    /// Key of the root row, which descendants name in their references.
    pub row_key: String,
    /// Winning retained value per group of the root row.
    pub values: Vec<(GroupId, Value)>,
    /// Deleted descendants grouped under the root.
    pub descendants: Vec<RecoveryDescendant>,
}

/// Future of a restore: the id of the new root row, or why it could not be created.
pub type RestoreFuture<'c> = Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'c>>;

/// Creates new rows from a recovered root and its descendants through the owning domain's
/// persistence, on a connection that is already inside the closing transaction. The restore
/// commits only together with the deletions that close the offer.
pub type Restorer = for<'c> fn(&'c mut SqliteConnection, RecoveredRow) -> RestoreFuture<'c>;

/// Restorers keyed by table name. A table without a restorer accepts no recovery choice.
#[derive(Default)]
pub struct RecoveryRestorers {
    by_name: BTreeMap<&'static str, (TableId, Restorer)>,
}

impl RecoveryRestorers {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the restorer of a table.
    ///
    /// # Panics
    ///
    /// When the table already has one, which is a composition error.
    #[must_use]
    pub fn with(mut self, table: &'static TableSpec, restorer: Restorer) -> Self {
        let previous = self.by_name.insert(table.name, (table.id, restorer));
        assert!(
            previous.is_none(),
            "{} has two recovery restorers",
            table.name
        );
        self
    }

    /// Table names that have a restorer, in name order.
    pub fn tables(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.by_name.keys().copied()
    }

    /// Table id of a table name that accepts recovery choices.
    pub fn table(&self, name: &str) -> Result<TableId, String> {
        self.by_name
            .get(name)
            .map(|(table, _)| *table)
            .ok_or_else(|| format!("{name} rows cannot be recovered"))
    }

    fn restorer(&self, table: TableId) -> Result<Restorer, String> {
        self.by_name
            .values()
            .find(|(id, _)| *id == table)
            .map(|(_, restorer)| *restorer)
            .ok_or_else(|| format!("table {} rows cannot be recovered", table.0))
    }
}

/// Restores or discards a recovery offer, then closes it with deletions that cover every
/// retained version. Reading the offer, restoring through the table's registered restorer, and
/// closing share one transaction, so a failed restore or close leaves the offer open and no
/// restored row behind. Returns the id of the restored row.
pub async fn close(
    engine: &Engine,
    pool: &SqlitePool,
    ctx: &SpaceContext,
    writer: &mut ActiveWriter,
    restorers: &RecoveryRestorers,
    choice: &RecoveryChoice,
    now_ms: u64,
) -> Result<Option<String>, String> {
    let (table, row_key) = (choice.table, choice.row_key.as_str());
    let restorer = restorers.restorer(table)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin recovery close: {error}"))?;
    let restored = match choice.action {
        RecoveryAction::Discard => None,
        RecoveryAction::Restore => {
            let row = recovered(engine, &mut tx, table, row_key).await?;
            Some(restorer(&mut tx, row).await?)
        }
    };
    let report = engine
        .close_recovery(&mut tx, ctx, table, row_key, &mut writer.local(), now_ms)
        .await
        .map_err(|error| format!("close recovery offer: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit recovery close: {error}"))?;
    if let Some(seq) = report.last_seq {
        writer.committed(seq)?;
    }
    Ok(restored)
}

/// Retained values of an offered row and its descendants.
async fn recovered(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: TableId,
    row_key: &str,
) -> Result<RecoveredRow, String> {
    let closed = || "the recovery offer is closed".to_string();
    let values = engine
        .recovery_values(conn, table, row_key)
        .await
        .map_err(|error| format!("read recovered values: {error}"))?
        .ok_or_else(closed)?;
    let descendants = engine
        .recovery_descendants(conn, table, row_key)
        .await
        .map_err(|error| format!("read recovered descendants: {error}"))?
        .ok_or_else(closed)?;
    Ok(RecoveredRow {
        table,
        row_key: row_key.to_string(),
        values,
        descendants,
    })
}

#[cfg(test)]
mod tests {
    use ganbaru_sync::manifest::vault::quick_notes::{NOTES, NOTES_TABLE, TAGS};

    use super::*;

    fn refuse(_: &mut SqliteConnection, _: RecoveredRow) -> RestoreFuture<'_> {
        Box::pin(async { Err("refused".to_string()) })
    }

    #[test]
    fn recovery_choices_resolve_only_registered_tables() {
        let restorers = RecoveryRestorers::new().with(&NOTES, refuse);
        assert_eq!(restorers.table("quick_notes"), Ok(NOTES_TABLE));
        assert!(restorers.table("quick_note_tags").is_err());
        assert!(restorers.table("missing").is_err());
        assert!(restorers.restorer(NOTES_TABLE).is_ok());
        assert!(restorers.restorer(TAGS.id).is_err());
        assert_eq!(restorers.tables().collect::<Vec<_>>(), vec!["quick_notes"]);
    }

    #[test]
    #[should_panic(expected = "two recovery restorers")]
    fn recovery_restorers_refuse_a_second_restorer_for_a_table() {
        let _ = RecoveryRestorers::new()
            .with(&NOTES, refuse)
            .with(&NOTES, refuse);
    }
}
