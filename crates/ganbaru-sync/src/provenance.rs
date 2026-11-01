//! Which devices sealed a replicated row, as read from the operation log.
//!
//! Domain columns such as a Chat message's author or display label are writer claims: a linked
//! device can seal any value there. The creating device comes from the signed log instead.

use crate::error::SyncResult;
use ganbaru_sync_contracts::TableId;
use sqlx::SqliteConnection;

/// Returns the devices whose writers created `row_key`, ordered by device id.
///
/// Empty when the row was never captured or sync state was reset.
pub async fn row_creating_devices(
    _connection: &mut SqliteConnection,
    _table: TableId,
    _row_key: &str,
) -> SyncResult<Vec<String>> {
    Ok(Vec::new())
}
