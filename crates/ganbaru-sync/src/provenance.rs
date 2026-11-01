//! Which devices sealed a replicated row, as read from the operation log.
//!
//! Domain columns such as a Chat message's author or display label are writer claims: a linked
//! device can seal any value there. The creating device comes from the signed log instead.

use crate::error::SyncResult;
use crate::merge::table_param;
use ganbaru_sync_contracts::TableId;
use sqlx::{Row, SqliteConnection};
use std::collections::HashMap;

/// Returns the devices whose writers created `row_key`, ordered by device id.
///
/// Empty when the row was never captured or sync state was reset.
pub async fn row_creating_devices(
    connection: &mut SqliteConnection,
    table: TableId,
    row_key: &str,
) -> SyncResult<Vec<String>> {
    let devices = sqlx::query_scalar(
        "SELECT DISTINCT w.device_id
         FROM sync_row_creations c JOIN sync_writers w ON w.writer_id = c.writer_id
         WHERE c.table_id = ? AND c.row_key = ?
         ORDER BY w.device_id",
    )
    .bind(table_param(table))
    .bind(row_key)
    .fetch_all(&mut *connection)
    .await?;
    Ok(devices)
}

/// Returns the label of every device the authorization log enrolled, keyed by device id.
///
/// A removed device keeps its label, so a row it created can still name it.
pub async fn device_labels(
    connection: &mut SqliteConnection,
) -> SyncResult<HashMap<String, String>> {
    let rows = sqlx::query("SELECT device_id, label FROM sync_devices WHERE label IS NOT NULL")
        .fetch_all(&mut *connection)
        .await?;
    rows.iter()
        .map(|row| Ok((row.try_get("device_id")?, row.try_get("label")?)))
        .collect()
}
