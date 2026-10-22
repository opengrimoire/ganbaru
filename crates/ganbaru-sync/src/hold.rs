//! Deletion holds: a short lease that keeps a local delete unsealed so domain Undo can restore
//! the row without a tombstone.
//!
//! A domain deletes rows of a table that holds deletes and calls [`hold_deletes`] in the same
//! transaction. Sealing skips a held delete until its lease expires, unless a remote operation
//! touches the row first, which seals it early. Undo inside the lease calls [`release_hold`]
//! and inserts the row again; the insert capture turns the held delete into an ordinary write.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::TableId;
use sqlx::SqliteConnection;

use crate::error::{SyncError, SyncResult};
use crate::log;
use crate::manifest::Manifest;

/// Longest lease a hold keeps a delete unsealed, in milliseconds.
pub const MAX_HOLD_MS: u64 = 10_000;

/// Holds the captured deletes of `keys` in `table` until `until_ms`, clamped to
/// [`MAX_HOLD_MS`] after `now_ms`. Call it in the transaction that deletes the rows. A key
/// without a captured delete, such as a row that was never published, is left alone.
///
/// Refuses a table the manifest does not mark as holding deletes.
pub async fn hold_deletes(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
    table: TableId,
    keys: &[&str],
    now_ms: u64,
    until_ms: u64,
) -> SyncResult<()> {
    let spec = manifest
        .table(table)
        .ok_or(SyncError::InvalidRequest("the table is not replicated"))?;
    if !spec.holds_deletes {
        return Err(SyncError::InvalidRequest("the table does not hold deletes"));
    }
    let until = until_ms.min(now_ms.saturating_add(MAX_HOLD_MS));
    for key in keys {
        sqlx::query(
            "UPDATE sync_capture SET held_until_ms = ?
             WHERE table_id = ? AND row_key = ? AND deleted = 1",
        )
        .bind(log::clamp_ms(until))
        .bind(i64::from(table.0))
        .bind(*key)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Whether every key of `keys` in `table` still has a held delete whose lease has not expired
/// at `now_ms`. Domain Undo calls it before inserting the rows again and refuses when it
/// returns false; the insert itself clears the hold.
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

/// The earliest lease expiry among held deletes, so the service can schedule a seal for it.
pub async fn next_hold_expiry(conn: &mut SqliteConnection) -> SyncResult<Option<u64>> {
    let next: Option<i64> = sqlx::query_scalar(
        "SELECT MIN(held_until_ms) FROM sync_capture WHERE held_until_ms IS NOT NULL",
    )
    .fetch_one(&mut *conn)
    .await?;
    next.map(|ms| log::stored_u64(ms, "hold expiry"))
        .transpose()
}

/// Ends every hold, so the next seal publishes held deletes. Quiescing before a handoff or a
/// backup calls it so no delete stays local.
pub async fn expire_holds(conn: &mut SqliteConnection) -> SyncResult<()> {
    sqlx::query("UPDATE sync_capture SET held_until_ms = NULL WHERE held_until_ms IS NOT NULL")
        .execute(&mut *conn)
        .await?;
    Ok(())
}
