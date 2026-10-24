//! Deletion holds: a short lease that keeps a local delete unsealed so domain Undo can restore
//! the row without a tombstone.
//!
//! A domain deletes rows of a table that holds deletes and calls [`hold_deletes`] in the same
//! transaction. Sealing skips a held delete until its lease expires, unless a remote operation
//! touches the row first, which seals it early. Undo inside the lease calls [`release_hold`]
//! and inserts the row again; the insert capture turns the held delete into an ordinary write.
//! Rows the delete removed through `ON DELETE CASCADE` share the hold: Undo inserts them
//! again too, and expiry or an early seal publishes the whole set.
//!
//! A deletion that also inserts rows, such as the archive a Calendar deletion writes, holds
//! those inserts with [`hold_creates`] under the same lease. Undo inside the lease deletes them
//! again, so the hold ends with nothing to publish; expiry publishes them once with the delete.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::TableId;
use sqlx::{AssertSqlSafe, SqliteConnection};

use crate::Engine;
use crate::error::{SyncError, SyncResult, corrupt};
use crate::log;
use crate::manifest::OnDelete;
use crate::merge::{self, table_param};
use crate::sql::ident;
use crate::visibility::{RowRef, Visibility};

/// Longest lease a hold keeps a delete unsealed, in milliseconds.
pub const MAX_HOLD_MS: u64 = 10_000;

/// Holds the captured deletes of `keys` in `table` until `until_ms`, clamped to
/// [`MAX_HOLD_MS`] after `now_ms`. Call it in the transaction that deletes the rows. A key
/// without a captured delete, such as a row that was never published, is left alone.
///
/// Published rows that the delete removed through `ON DELETE CASCADE`, at any depth, inherit
/// the same lease, so Undo inserts them again with the held rows and expiry seals them
/// together.
///
/// Refuses a table the manifest does not mark as holding deletes.
pub async fn hold_deletes(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: TableId,
    keys: &[&str],
    now_ms: u64,
    until_ms: u64,
) -> SyncResult<()> {
    let spec = engine
        .manifest()
        .table(table)
        .ok_or(SyncError::InvalidRequest("the table is not replicated"))?;
    if !spec.holds_deletes {
        return Err(SyncError::InvalidRequest("the table does not hold deletes"));
    }
    let until = log::clamp_ms(until_ms.min(now_ms.saturating_add(MAX_HOLD_MS)));
    let mut held: BTreeSet<RowRef> = BTreeSet::new();
    for key in keys {
        if hold_row(conn, table, key, until).await? {
            held.insert((table, (*key).to_string()));
        }
    }
    for (child_table, child_key) in cascade_children(engine, conn, held).await? {
        hold_row(conn, child_table, &child_key, until).await?;
    }
    Ok(())
}

/// Holds the captured inserts of `keys` in `table` until `until_ms`, clamped like
/// [`hold_deletes`], so rows a held deletion wrote stay local with it. Call it in the
/// transaction that inserts the rows. Only inserts of rows sync has never published are held;
/// any other key is left alone.
///
/// Captured inserts of rows that reference a held row through `ON DELETE CASCADE`, at any
/// depth, inherit the lease: Undo removes them with the held row, so they never publish.
///
/// Refuses a table whose deletes cannot be held, see [`crate::manifest::Manifest::may_hold_deletes`].
pub async fn hold_creates(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: TableId,
    keys: &[&str],
    now_ms: u64,
    until_ms: u64,
) -> SyncResult<()> {
    if !engine.manifest().may_hold_deletes(table) {
        return Err(SyncError::InvalidRequest("the table does not hold deletes"));
    }
    let until = log::clamp_ms(until_ms.min(now_ms.saturating_add(MAX_HOLD_MS)));
    let mut pending: Vec<RowRef> = Vec::new();
    for key in keys {
        if hold_created_row(conn, table, key, until).await? {
            pending.push((table, (*key).to_string()));
        }
    }
    while let Some((parent, parent_key)) = pending.pop() {
        for child in engine.manifest().tables {
            for (_, _, reference) in child.references() {
                if reference.table != parent
                    || reference.on_delete != OnDelete::Cascade
                    || !engine.manifest().may_hold_deletes(child.id)
                {
                    continue;
                }
                let sql = format!(
                    "SELECT c.row_key FROM sync_capture c
                     JOIN {table} t ON t.{key} = c.row_key
                     WHERE c.table_id = ? AND c.created = 1 AND c.deleted = 0
                       AND c.held_until_ms IS NULL AND t.{column} = ?",
                    table = ident(child.name),
                    key = ident(child.key_column),
                    column = ident(reference.column),
                );
                let keys: Vec<String> = sqlx::query_scalar(AssertSqlSafe(sql))
                    .bind(table_param(child.id))
                    .bind(parent_key.as_str())
                    .fetch_all(&mut *conn)
                    .await?;
                for key in keys {
                    if hold_created_row(conn, child.id, &key, until).await? {
                        pending.push((child.id, key));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Holds the captured insert of one row sync has never published; whether it had one.
async fn hold_created_row(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    until: i64,
) -> SyncResult<bool> {
    let held = sqlx::query(
        "UPDATE sync_capture SET held_until_ms = ?
         WHERE table_id = ? AND row_key = ? AND created = 1 AND deleted = 0
           AND NOT EXISTS (SELECT 1 FROM sync_rows r
                           WHERE r.table_id = sync_capture.table_id
                             AND r.row_key = sync_capture.row_key)",
    )
    .bind(until)
    .bind(table_param(table))
    .bind(key)
    .execute(&mut *conn)
    .await?;
    Ok(held.rows_affected() > 0)
}

/// Holds the captured delete of one row; whether it had one.
async fn hold_row(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    until: i64,
) -> SyncResult<bool> {
    let held = sqlx::query(
        "UPDATE sync_capture SET held_until_ms = ?
         WHERE table_id = ? AND row_key = ? AND deleted = 1",
    )
    .bind(until)
    .bind(table_param(table))
    .bind(key)
    .execute(&mut *conn)
    .await?;
    Ok(held.rows_affected() > 0)
}

/// Captured deletes of published rows, in tables that may hold deletes, whose cascading
/// references reach `held` through merge state, at any depth. `held` itself is left out.
async fn cascade_children(
    engine: &Engine,
    conn: &mut SqliteConnection,
    mut held: BTreeSet<RowRef>,
) -> SyncResult<BTreeSet<RowRef>> {
    let manifest = engine.manifest();
    let deleted: Vec<(i64, String)> =
        sqlx::query_as("SELECT table_id, row_key FROM sync_capture WHERE deleted = 1")
            .fetch_all(&mut *conn)
            .await?;
    let visibility = Visibility::default();
    let mut parents: Vec<(RowRef, Vec<RowRef>)> = Vec::new();
    for (table, key) in deleted {
        let table = u16::try_from(table)
            .map(TableId)
            .map_err(|_| corrupt("table id out of range"))?;
        if held.contains(&(table, key.clone())) || !manifest.may_hold_deletes(table) {
            continue;
        }
        let Some(spec) = manifest.table(table) else {
            continue;
        };
        if merge::row_state(conn, table, &key).await?.is_none() {
            continue;
        }
        let targets = visibility
            .cascade_targets(engine, conn, spec, &key)
            .await?
            .into_iter()
            .map(|target| target.row)
            .collect();
        parents.push(((table, key), targets));
    }
    let mut children = BTreeSet::new();
    loop {
        let reached: Vec<RowRef> = parents
            .iter()
            .filter(|(row, targets)| {
                !held.contains(row) && targets.iter().any(|target| held.contains(target))
            })
            .map(|(row, _)| row.clone())
            .collect();
        if reached.is_empty() {
            return Ok(children);
        }
        for row in reached {
            held.insert(row.clone());
            children.insert(row);
        }
    }
}

/// Whether every key of `keys` in `table` still has a held delete whose lease has not expired
/// at `now_ms`. Domain Undo calls it before inserting the rows again and refuses when it
/// returns false; the insert itself clears the hold. Only the keys the domain held are
/// checked: their cascade children share the lease, and inserting each child again clears its
/// own hold.
pub async fn release_hold(
    conn: &mut SqliteConnection,
    table: TableId,
    keys: &[&str],
    now_ms: u64,
) -> SyncResult<bool> {
    let keys: BTreeSet<&str> = keys.iter().copied().collect();
    for key in keys {
        let held: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM sync_capture
             WHERE table_id = ? AND row_key = ? AND deleted = 1 AND held_until_ms > ?",
        )
        .bind(i64::from(table.0))
        .bind(key)
        .bind(log::clamp_ms(now_ms))
        .fetch_optional(&mut *conn)
        .await?;
        if held.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The earliest lease expiry among held captures, so the service can schedule a seal for it.
pub async fn next_hold_expiry(conn: &mut SqliteConnection) -> SyncResult<Option<u64>> {
    let next: Option<i64> = sqlx::query_scalar(
        "SELECT MIN(held_until_ms) FROM sync_capture WHERE held_until_ms IS NOT NULL",
    )
    .fetch_one(&mut *conn)
    .await?;
    next.map(|ms| log::stored_u64(ms, "hold expiry"))
        .transpose()
}

/// Ends every hold, so the next seal publishes held captures. Quiescing before a handoff or a
/// backup calls it so no held change stays local.
pub async fn expire_holds(conn: &mut SqliteConnection) -> SyncResult<()> {
    sqlx::query("UPDATE sync_capture SET held_until_ms = NULL WHERE held_until_ms IS NOT NULL")
        .execute(&mut *conn)
        .await?;
    Ok(())
}
