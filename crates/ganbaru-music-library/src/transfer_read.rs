//! Shared admission for complete Music transfer snapshots before payload loading.

use sqlx::{FromRow, Sqlite, Transaction, sqlite::SqliteRow};

use super::{MusicLibraryError, MusicLibraryResult, transfer_codec::validation};

pub(super) const MAX_READ_RECORDS: usize = 100_000;
pub(super) const MAX_READ_BYTES: usize = 32 * 1024 * 1024;

pub(super) const ITEM_TEXT_COLUMNS: &[&str] = &[
    "id",
    "identity_key",
    "source_kind",
    "media_kind",
    "youtube_video_id",
    "original_title",
    "original_artist",
    "original_album",
    "original_artwork_identity",
    "youtube_resolution_state",
    "title_override",
    "artist_override",
    "album_override",
    "artwork_override",
    "availability",
    "review_state",
];

/// One allowance covers all selected roots, rows and related record families.
pub(super) struct TransferReadBudget {
    records: usize,
    bytes: usize,
}

impl Default for TransferReadBudget {
    fn default() -> Self {
        Self {
            records: MAX_READ_RECORDS,
            bytes: MAX_READ_BYTES,
        }
    }
}

impl TransferReadBudget {
    /// Probe a repository-owned query in the same snapshot before allocating rows.
    pub(super) async fn read<T>(
        &mut self,
        transaction: &mut Transaction<'_, Sqlite>,
        source_sql: &str,
        text_columns: &[&str],
        bindings: &[&str],
        family_limit: usize,
    ) -> MusicLibraryResult<Vec<T>>
    where
        for<'row> T: FromRow<'row, SqliteRow> + Send + Unpin,
    {
        let allowance = self.records.min(family_limit);
        let limit =
            i64::try_from(allowance + 1).map_err(|_| validation("Invalid Music read allowance"))?;
        let sql = format!("{source_sql} LIMIT ?");
        let text_bytes = text_columns
            .iter()
            .map(|column| format!("COALESCE(length(CAST({column} AS BLOB)), 0)"))
            .collect::<Vec<_>>()
            .join(" + ");
        // Reserve row containers and escaped/decoded text without materializing JSON.
        let probe =
            format!("SELECT COUNT(*), COALESCE(SUM(256 + 4 * ({text_bytes})), 0) FROM ({sql})");
        let mut query = sqlx::query_as::<_, (i64, i64)>(&probe);
        for binding in bindings {
            query = query.bind(*binding);
        }
        let (records, bytes) = query
            .bind(limit)
            .fetch_one(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("admit Music transfer rows", error))?;
        let records =
            usize::try_from(records).map_err(|_| validation("Invalid Music read count"))?;
        let bytes =
            usize::try_from(bytes).map_err(|_| validation("Invalid Music read byte count"))?;
        if records > allowance || bytes > self.bytes {
            return Err(validation(
                "Music transfer source exceeds its shared record or byte allowance",
            ));
        }
        self.records -= records;
        self.bytes -= bytes;
        let mut query = sqlx::query_as::<_, T>(&sql);
        for binding in bindings {
            query = query.bind(*binding);
        }
        query
            .bind(limit)
            .fetch_all(&mut **transaction)
            .await
            .map_err(|error| {
                MusicLibraryError::database("read admitted Music transfer rows", error)
            })
    }
}
