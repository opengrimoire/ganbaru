//! Restore of device state into a replacement database.
//!
//! A replacement arrives as a snapshot copy, which carries no device's `Local` state. Before the
//! replacement takes the live database's place, the receiver's `Local` tables and `Local` columns
//! are copied from its live database into the staged one, and references the copy leaves
//! dangling are resolved by their declared delete action.

use std::collections::BTreeSet;
use std::path::Path;

use sqlx::{AssertSqlSafe, Connection, Row, SqliteConnection};

use crate::error::{SyncError, SyncResult, corrupt};
use crate::manifest::Manifest;
use crate::sql::{self, ident};

/// Schema name the live database is attached under during a restore.
const LIVE_SCHEMA: &str = "ganbaru_restore_live";
/// Most foreign key repair rounds before a restore gives up. Each round resolves every violation
/// it sees, and declared actions only remove references, so a clean schema settles in one or two.
const MAX_REPAIR_ROUNDS: usize = 16;
/// Upper-case hexadecimal digits for percent-encoding.
const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// What a restore copied into the replacement and what it repaired.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalRestoreReport {
    /// Rows copied into `Local` tables.
    pub local_rows: u64,
    /// Rows of replicated tables whose `Local` columns took the live values.
    pub local_columns: u64,
    /// Rows deleted or nulled because a reference had no parent in the replacement.
    pub repaired: u64,
}

/// Copies the device state of the live database at `live` into the staged replacement on
/// `staged`: every `Local` table's rows, with seeded rows the live database lacks at new-vault
/// values, and `Local` column values onto replicated rows present in both databases. A table's
/// revision column takes one more than the larger of both values, so a revision observed before
/// the replacement never matches a row whose content changed.
/// Rows that reference a parent the replacement lacks are deleted or nulled by their declared
/// delete action. Runs in one transaction with the engine marked as applying, so capture
/// triggers stay quiet and nothing in the copy is published.
pub async fn restore_local_state(
    staged: &mut SqliteConnection,
    manifest: &Manifest,
    live: &Path,
) -> SyncResult<LocalRestoreReport> {
    manifest.validate().map_err(SyncError::InvalidManifest)?;
    sqlx::query(AssertSqlSafe(format!(
        "ATTACH DATABASE ? AS {}",
        ident(LIVE_SCHEMA)
    )))
    .bind(read_only_uri(live)?)
    .execute(&mut *staged)
    .await?;
    let restored = restore_attached(staged, manifest).await;
    let detached = sqlx::query(AssertSqlSafe(format!(
        "DETACH DATABASE {}",
        ident(LIVE_SCHEMA)
    )))
    .execute(&mut *staged)
    .await;
    let report = restored?;
    detached?;
    Ok(report)
}

async fn restore_attached(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
) -> SyncResult<LocalRestoreReport> {
    let mut report = LocalRestoreReport::default();
    let mut tx = conn.begin().await?;
    sql::set_applying(&mut tx, true).await?;
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *tx)
        .await?;
    for table in manifest.local {
        sqlx::query(AssertSqlSafe(format!("DELETE FROM main.{}", ident(table))))
            .execute(&mut *tx)
            .await?;
        let live = column_names(&mut tx, LIVE_SCHEMA, table).await?;
        let columns = column_names(&mut tx, "main", table)
            .await?
            .into_iter()
            .filter(|column| live.contains(column))
            .map(|column| ident(&column))
            .collect::<Vec<_>>()
            .join(", ");
        if columns.is_empty() {
            continue;
        }
        report.local_rows += sqlx::query(AssertSqlSafe(format!(
            "INSERT INTO main.{table} ({columns}) SELECT {columns} FROM {schema}.{table}",
            table = ident(table),
            schema = ident(LIVE_SCHEMA),
        )))
        .execute(&mut *tx)
        .await?
        .rows_affected();
    }
    super::write_local_seeds(&mut tx, manifest).await?;
    for table in manifest.tables {
        if table.local_columns.is_empty() {
            continue;
        }
        let live = column_names(&mut tx, LIVE_SCHEMA, table.name).await?;
        let assignments = table
            .local_columns
            .iter()
            .filter(|column| live.iter().any(|name| name == column.name))
            .map(|column| {
                let name = ident(column.name);
                if table.revision_column == Some(column.name) {
                    format!("{name} = max(staged.{name}, restored.{name}) + 1")
                } else {
                    format!("{name} = restored.{name}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        if assignments.is_empty() {
            continue;
        }
        report.local_columns += sqlx::query(AssertSqlSafe(format!(
            "UPDATE main.{table} AS staged SET {assignments}
             FROM {schema}.{table} AS restored WHERE restored.{key} = staged.{key}",
            table = ident(table.name),
            schema = ident(LIVE_SCHEMA),
            key = ident(table.key_column),
        )))
        .execute(&mut *tx)
        .await?
        .rows_affected();
    }
    report.repaired = repair_foreign_keys(&mut tx).await?;
    sql::set_applying(&mut tx, false).await?;
    tx.commit().await?;
    Ok(report)
}

/// Resolves every foreign key violation in the main schema by the reference's declared delete
/// action: rows of a `CASCADE` reference are deleted, and a `SET NULL` reference is nulled.
/// Any other action is refused, since the restore cannot choose a parent for the row. Returns
/// how many rows were deleted or updated.
pub(crate) async fn repair_foreign_keys(conn: &mut SqliteConnection) -> SyncResult<u64> {
    let mut repaired = 0;
    for _ in 0..MAX_REPAIR_ROUNDS {
        let violations = sqlx::query("PRAGMA main.foreign_key_check")
            .fetch_all(&mut *conn)
            .await?
            .iter()
            .map(|row| Ok((row.try_get::<String, _>(0)?, row.try_get::<i64, _>(3)?)))
            .collect::<Result<BTreeSet<_>, sqlx::Error>>()?;
        if violations.is_empty() {
            return Ok(repaired);
        }
        for (table, id) in violations {
            let reference = reference(conn, &table, id).await?;
            repaired += resolve(conn, &table, &reference).await?;
        }
    }
    Err(corrupt(
        "foreign key repair after a local restore did not settle",
    ))
}

/// One declared foreign key: parent table, column pairs in order, and its delete action.
struct Reference {
    parent: String,
    columns: Vec<(String, String)>,
    on_delete: String,
}

async fn reference(conn: &mut SqliteConnection, table: &str, id: i64) -> SyncResult<Reference> {
    let rows = sqlx::query(
        "SELECT \"table\", \"from\", \"to\", on_delete FROM pragma_foreign_key_list(?, 'main')
         WHERE id = ? ORDER BY seq",
    )
    .bind(table)
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let first = rows
        .first()
        .ok_or_else(|| corrupt(format!("foreign key {id} of {table} is not declared")))?;
    let parent: String = first.try_get(0)?;
    let on_delete: String = first.try_get(3)?;
    let mut children = Vec::with_capacity(rows.len());
    let mut parents = Vec::with_capacity(rows.len());
    for row in &rows {
        children.push(row.try_get::<String, _>(1)?);
        parents.push(row.try_get::<Option<String>, _>(2)?);
    }
    let parents = if parents.iter().all(Option::is_some) {
        parents.into_iter().flatten().collect()
    } else {
        primary_key(conn, &parent).await?
    };
    if parents.len() != children.len() {
        return Err(corrupt(format!(
            "foreign key {id} of {table} does not match the primary key of {parent}"
        )));
    }
    Ok(Reference {
        parent,
        columns: children.into_iter().zip(parents).collect(),
        on_delete,
    })
}

async fn resolve(
    conn: &mut SqliteConnection,
    table: &str,
    reference: &Reference,
) -> SyncResult<u64> {
    let present = reference
        .columns
        .iter()
        .map(|(child, _)| format!("child.{} IS NOT NULL", ident(child)))
        .collect::<Vec<_>>()
        .join(" AND ");
    let matches = reference
        .columns
        .iter()
        .map(|(child, parent)| format!("parent.{} = child.{}", ident(parent), ident(child)))
        .collect::<Vec<_>>()
        .join(" AND ");
    let dangling = format!(
        "{present} AND NOT EXISTS (SELECT 1 FROM main.{parent} AS parent WHERE {matches})",
        parent = ident(&reference.parent),
    );
    let statement = match reference.on_delete.to_ascii_uppercase().as_str() {
        "CASCADE" => format!(
            "DELETE FROM main.{} AS child WHERE {dangling}",
            ident(table)
        ),
        "SET NULL" => {
            let nulls = reference
                .columns
                .iter()
                .map(|(child, _)| format!("{} = NULL", ident(child)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "UPDATE main.{} AS child SET {nulls} WHERE {dangling}",
                ident(table)
            )
        }
        action => {
            return Err(corrupt(format!(
                "{table} references {} with ON DELETE {action}, which a local restore cannot \
                 resolve",
                reference.parent
            )));
        }
    };
    Ok(sqlx::query(AssertSqlSafe(statement))
        .execute(&mut *conn)
        .await?
        .rows_affected())
}

async fn column_names(
    conn: &mut SqliteConnection,
    schema: &str,
    table: &str,
) -> SyncResult<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT name FROM pragma_table_info(?, ?) ORDER BY cid")
            .bind(table)
            .bind(schema)
            .fetch_all(&mut *conn)
            .await?,
    )
}

async fn primary_key(conn: &mut SqliteConnection, table: &str) -> SyncResult<Vec<String>> {
    Ok(
        sqlx::query_scalar(
            "SELECT name FROM pragma_table_info(?, 'main') WHERE pk > 0 ORDER BY pk",
        )
        .bind(table)
        .fetch_all(&mut *conn)
        .await?,
    )
}

/// A read-only SQLite URI for a database file. Bytes outside the unreserved set, `/`, and `:`
/// are percent-encoded, so `%`, `?`, and `#` in a path stay part of the file name.
pub(crate) fn read_only_uri(path: &Path) -> SyncResult<String> {
    let text = path.to_str().ok_or(SyncError::InvalidRequest(
        "database path is not valid UTF-8",
    ))?;
    let text = if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.to_owned()
    };
    let mut uri = String::from("file:");
    let drive = cfg!(windows)
        && text.as_bytes().get(1) == Some(&b':')
        && text.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
    if drive {
        uri.push('/');
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push('%');
            uri.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
            uri.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
        }
    }
    uri.push_str("?mode=ro");
    Ok(uri)
}
