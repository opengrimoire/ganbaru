//! Recovery offers: deleted rows that kept edits their deletion did not see. Restoring and
//! discarding seal with the local writer, so the service runs them inside a pass.

use super::writer::ActiveWriter;
use ganbaru_sync::manifest::vault::quick_notes::{NOTES, NOTES_TABLE};
use ganbaru_sync::{Engine, SpaceContext};
use ganbaru_sync_contracts::TableId;
use serde::Deserialize;
use sqlx::pool::PoolConnection;
use sqlx::{Sqlite, SqlitePool};

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

/// Table id of a recoverable table name; only Quick notes offer recovery in this version.
pub fn recoverable_table(name: &str) -> Result<TableId, String> {
    if name == NOTES.name {
        Ok(NOTES_TABLE)
    } else {
        Err(format!("{name} rows cannot be recovered"))
    }
}

/// Restores or discards a recovery offer, then closes it with a tombstone that covers every
/// retained version. Returns the id of the restored row.
pub async fn close(
    engine: &Engine,
    pool: &SqlitePool,
    ctx: &SpaceContext,
    writer: &mut ActiveWriter,
    choice: &RecoveryChoice,
    now_ms: u64,
) -> Result<Option<String>, String> {
    let (table, row_key) = (choice.table, choice.row_key.as_str());
    let restored = match choice.action {
        RecoveryAction::Discard => None,
        RecoveryAction::Restore => {
            let values = {
                let mut conn = acquire(pool).await?;
                engine
                    .recovery_values(&mut conn, table, row_key)
                    .await
                    .map_err(|error| format!("read recovered values: {error}"))?
            }
            .ok_or_else(|| "the recovery offer is closed".to_string())?;
            Some(ganbaru_quick_notes::restore_recovered(pool, &values).await?)
        }
    };
    let report = {
        let mut conn = acquire(pool).await?;
        engine
            .close_recovery(&mut conn, ctx, table, row_key, &mut writer.local(), now_ms)
            .await
            .map_err(|error| format!("close recovery offer: {error}"))?
    };
    if let Some(seq) = report.last_seq {
        writer.committed(seq)?;
    }
    Ok(restored)
}

async fn acquire(pool: &SqlitePool) -> Result<PoolConnection<Sqlite>, String> {
    pool.acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))
}
