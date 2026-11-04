//! Capture seeding: the first publication of rows that existed before their table replicated.
//!
//! Capture triggers only see changes made after they are installed. When a domain starts to
//! replicate, its existing rows are captured once as creates, and `sync_seeded_tables` records
//! the table so later vault opens leave it alone.

use ganbaru_sync_contracts::PENDING_KEY_PREFIX;
use sqlx::{AssertSqlSafe, Connection, SqliteConnection};

use crate::Engine;
use crate::error::{SyncError, SyncResult, corrupt};
use crate::log::clamp_ms;
use crate::manifest::{Manifest, TableSpec};
use crate::merge::{mask_param, table_param};
use crate::publication;
use crate::sql::ident;

/// Refuses when a row of `table` holds a `pending:` placeholder key.
pub(crate) async fn refuse_pending_key(
    conn: &mut SqliteConnection,
    table: &TableSpec,
) -> SyncResult<()> {
    // The prefix range compares bytes, so it uses the key index.
    let end = PENDING_KEY_PREFIX
        .strip_suffix(':')
        .map(|stem| format!("{stem};"))
        .ok_or_else(|| corrupt("pending key prefix ends with a colon"))?;
    let sql = format!(
        "SELECT 1 FROM {table} WHERE {key} >= ? AND {key} < ? LIMIT 1",
        table = ident(table.name),
        key = ident(table.key_column),
    );
    let found: Option<i64> = sqlx::query_scalar(AssertSqlSafe(sql.as_str()))
        .bind(PENDING_KEY_PREFIX)
        .bind(&end)
        .fetch_optional(&mut *conn)
        .await?;
    match found {
        Some(_) => Err(SyncError::PendingKey(table.name)),
        None => Ok(()),
    }
}

/// Captures every published row of each manifest table missing from `sync_seeded_tables` as a
/// create with every group, then records the table. Returns the tables it seeded.
///
/// It runs in the caller's transaction, so a domain conversion pass can rekey its rows and seed
/// them in one commit. The app runs it at vault open, after every conversion pass and before
/// the sync service starts. Refuses with [`SyncError::PendingKey`] while a table to seed still
/// holds a `pending:` placeholder key.
pub async fn seed_captures(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
    now_ms: u64,
) -> SyncResult<Vec<&'static str>> {
    let mut seeded = Vec::new();
    for table in manifest.tables {
        let done: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM sync_seeded_tables WHERE table_id = ?")
                .bind(table_param(table.id))
                .fetch_optional(&mut *conn)
                .await?;
        if done.is_some() {
            continue;
        }
        refuse_pending_key(conn, table).await?;
        let condition =
            publication::condition_in(manifest, table, "seeded").unwrap_or_else(|| "1".into());
        let sql = format!(
            "INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
             SELECT ?, seeded.{key}, ?, 1, 0, ? FROM {name} AS seeded WHERE {condition}
             ON CONFLICT (table_id, row_key) DO UPDATE SET
                mask = mask | excluded.mask, created = 1, deleted = 0, held_until_ms = NULL",
            key = ident(table.key_column),
            name = ident(table.name),
        );
        sqlx::query(AssertSqlSafe(sql.as_str()))
            .bind(table_param(table.id))
            .bind(mask_param(table.full_mask()))
            .bind(clamp_ms(now_ms))
            .execute(&mut *conn)
            .await?;
        sqlx::query("INSERT INTO sync_seeded_tables (table_id) VALUES (?)")
            .bind(table_param(table.id))
            .execute(&mut *conn)
            .await?;
        seeded.push(table.name);
    }
    Ok(seeded)
}

impl Engine {
    /// [`seed_captures`] for the engine's manifest in its own transaction.
    pub async fn seed_captures(
        &self,
        conn: &mut SqliteConnection,
        now_ms: u64,
    ) -> SyncResult<Vec<&'static str>> {
        let mut tx = conn.begin().await?;
        let seeded = seed_captures(&mut tx, self.manifest(), now_ms).await?;
        tx.commit().await?;
        Ok(seeded)
    }
}
