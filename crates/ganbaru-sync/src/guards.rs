//! Read-only guards for replicas that write only the tables their domains allow.
//!
//! A replica opens the vault writable so it can capture, seal, and apply replicated rows, while
//! every other table keeps the owner-only rules. Guards are TEMP triggers on each guarded table
//! that abort inserts, updates, and deletes outside engine apply transactions, so applying
//! remote changes still materializes guarded replicated tables and cascades into guarded
//! children. They are connection-scoped, so they never enter the vault file that is snapshotted
//! to other devices, and they must be installed on every new connection of a guarded pool.
//!
//! Guarded tables are the plain tables of `main` that the manifest does not classify as
//! replicated, owned children, derived, `Local`, or engine state, plus the replicated and owned
//! child tables of domains that are not replica-writable. Tables the manifest does not know are
//! guarded, so a newer schema never opens a write path by accident.

use sqlx::{Executor, Row, SqliteConnection};

use crate::SyncResult;
use crate::manifest::{Classification, Manifest};

/// Message of the guard abort. Callers map it to their read-only error.
pub const READ_ONLY_MESSAGE: &str = "This vault is read-only on this device";

/// Guard condition: true unless the engine is applying, and true when the apply state row is
/// missing, so a damaged database keeps its guards.
const OUTSIDE_APPLY: &str = "(SELECT applying FROM sync_apply_state WHERE singleton = 1) IS NOT 1";

const GUARDED_ACTIONS: [&str; 3] = ["INSERT", "UPDATE", "DELETE"];

/// Whether guards protect a table of `main` on a replica.
fn is_guarded(manifest: &Manifest, table: &str) -> bool {
    match manifest.classify(table) {
        Classification::Unconverted => true,
        Classification::Replicated | Classification::OwnedChildren => manifest
            .domain_of(table)
            .is_none_or(|domain| !domain.replica_writable),
        Classification::Derived | Classification::Engine | Classification::Local => false,
    }
}

/// Tables of `main` that guards protect on a replica, in name order.
pub async fn guarded_tables(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
) -> SyncResult<Vec<String>> {
    let rows = Executor::fetch_all(
        &mut *conn,
        "SELECT name FROM pragma_table_list
         WHERE schema = 'main' AND type = 'table'
            AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' AND name <> '_sqlx_migrations'
         ORDER BY name",
    )
    .await?;
    let mut tables = Vec::with_capacity(rows.len());
    for row in rows {
        let name: String = row.try_get(0)?;
        if is_guarded(manifest, &name) {
            tables.push(name);
        }
    }
    Ok(tables)
}

/// Statements creating the guards of one table.
fn render_guards(table: &str) -> Vec<String> {
    let quoted_table = quote_identifier(table);
    GUARDED_ACTIONS
        .iter()
        .map(|action| {
            let trigger = quote_identifier(&format!(
                "sync_guard_{table}_{}",
                action.to_ascii_lowercase()
            ));
            format!(
                "CREATE TEMP TRIGGER IF NOT EXISTS {trigger} BEFORE {action} ON main.{quoted_table}
                 WHEN {OUTSIDE_APPLY}
                 BEGIN SELECT RAISE(ABORT, '{READ_ONLY_MESSAGE}'); END;"
            )
        })
        .collect()
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Installs the guards on a connection and returns how many tables they protect.
///
/// The queries go through the [`Executor`] trait methods, whose boxed futures stay `Send` for
/// every connection lifetime, so this runs inside a `ganbaru_db` connection hook.
pub async fn install_guards(conn: &mut SqliteConnection, manifest: &Manifest) -> SyncResult<usize> {
    let tables = guarded_tables(conn, manifest).await?;
    for table in &tables {
        for statement in render_guards(table) {
            Executor::execute(&mut *conn, sqlx::AssertSqlSafe(statement)).await?;
        }
    }
    Ok(tables.len())
}

/// Whether a database error message is a guard abort.
pub fn is_guard_abort(message: &str) -> bool {
    message.contains(READ_ONLY_MESSAGE)
}
