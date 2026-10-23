//! Shared persistence for scanned local media files, used by desktop and Android refresh.
//!
//! A refresh writes portable item and location values only when the location is new or this
//! device observed a different size or modification time, and each write skips values that are
//! already stored. Device observations (availability, size, modification time, generation, and
//! artwork identity) update without changing `updated_at_ms` or `version`.

use sqlx::{Sqlite, Transaction};

use crate::identity::{RetirableTable, creatable_key};
use crate::{MusicLibraryError, MusicLibraryResult};

/// Values one refresh read for a local media file.
#[derive(Clone, Copy, Debug)]
pub struct LocalScanValues<'a> {
    pub relative_path: &'a str,
    pub media_kind: &'a str,
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub track_number: Option<i64>,
    pub duration_ms: Option<i64>,
    pub artwork_identity: Option<&'a str>,
    pub file_size_bytes: Option<i64>,
    pub modified_at_ms: Option<i64>,
    pub lightweight_fingerprint: &'a str,
    pub strong_fingerprint: Option<&'a str>,
}

/// Size and modification time this device last stored for a location.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalLocationStat {
    pub file_size_bytes: Option<i64>,
    pub modified_at_ms: Option<i64>,
}

impl LocalLocationStat {
    /// Returns whether a fresh observation matches the stored one, so the file needs no
    /// inspection. Unknown sizes or modification times never match.
    pub fn matches(self, file_size_bytes: Option<i64>, modified_at_ms: Option<i64>) -> bool {
        file_size_bytes.is_some()
            && modified_at_ms.is_some()
            && self.file_size_bytes == file_size_bytes
            && self.modified_at_ms == modified_at_ms
    }
}

/// Loads the stored observation for the location at exactly `relative_path`.
pub async fn load_location_stat(
    transaction: &mut Transaction<'_, Sqlite>,
    root_id: &str,
    relative_path: &str,
) -> MusicLibraryResult<Option<LocalLocationStat>> {
    let row: Option<(Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT file_size_bytes, modified_at_ms FROM music_local_locations
         WHERE root_id = ? AND relative_path = ?",
    )
    .bind(root_id)
    .bind(relative_path)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load local music location stat", error))?;
    Ok(
        row.map(|(file_size_bytes, modified_at_ms)| LocalLocationStat {
            file_size_bytes,
            modified_at_ms,
        }),
    )
}

/// Records that an unchanged location was seen in `generation` and links its item to the
/// source collection. Returns false when the location no longer exists.
pub async fn record_unchanged_location(
    transaction: &mut Transaction<'_, Sqlite>,
    collection_id: &str,
    root_id: &str,
    relative_path: &str,
    generation: i64,
    source_position: Option<i64>,
    seen_at_ms: i64,
) -> MusicLibraryResult<bool> {
    let item_id: Option<String> = sqlx::query_scalar(
        "UPDATE music_local_locations
         SET availability = 'available', last_seen_generation = ?
         WHERE root_id = ? AND relative_path = ?
         RETURNING item_id",
    )
    .bind(generation)
    .bind(root_id)
    .bind(relative_path)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("record unchanged local music", error))?;
    let Some(item_id) = item_id else {
        return Ok(false);
    };
    sqlx::query(
        "UPDATE music_library_items SET availability = 'available'
         WHERE id = ? AND availability IS NOT 'available'",
    )
    .bind(&item_id)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("observe unchanged local music", error))?;
    link_collection_item(
        transaction,
        collection_id,
        &item_id,
        source_position,
        generation,
        seen_at_ms,
    )
    .await?;
    Ok(true)
}

/// Whether persisting a scanned file needed portable writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalScanOutcome {
    /// The stored size and modification time matched; only observations were recorded.
    Unchanged,
    /// The location was new or changed on this device, so its values were written.
    Written,
}

/// Persists one scanned file whose values were read before comparing them with the stored
/// observation, as Android refresh does. The item key derives from the root and path.
#[allow(clippy::too_many_arguments)]
pub async fn persist_scanned_file(
    transaction: &mut Transaction<'_, Sqlite>,
    collection_id: &str,
    root_id: &str,
    values: &LocalScanValues<'_>,
    source_position: Option<i64>,
    generation: i64,
    seen_at_ms: i64,
    updated_at_ms: i64,
) -> MusicLibraryResult<LocalScanOutcome> {
    let stored = load_location_stat(transaction, root_id, values.relative_path).await?;
    if stored.is_some_and(|stored| stored.matches(values.file_size_bytes, values.modified_at_ms))
        && record_unchanged_location(
            transaction,
            collection_id,
            root_id,
            values.relative_path,
            generation,
            source_position,
            seen_at_ms,
        )
        .await?
    {
        return Ok(LocalScanOutcome::Unchanged);
    }
    let item_id = creatable_key(
        transaction,
        RetirableTable::Items,
        crate::identity::local_item_id(root_id, values.relative_path),
    )
    .await?;
    upsert_scanned_item(
        transaction,
        &item_id,
        values,
        "available",
        seen_at_ms,
        updated_at_ms,
    )
    .await?;
    upsert_scanned_location(
        transaction,
        root_id,
        &item_id,
        values,
        "available",
        generation,
        seen_at_ms,
        updated_at_ms,
    )
    .await?;
    link_collection_item(
        transaction,
        collection_id,
        &item_id,
        source_position,
        generation,
        seen_at_ms,
    )
    .await?;
    Ok(LocalScanOutcome::Written)
}

/// Inserts a scanned item or updates only the values that differ.
pub async fn upsert_scanned_item(
    transaction: &mut Transaction<'_, Sqlite>,
    item_id: &str,
    values: &LocalScanValues<'_>,
    availability: &str,
    discovered_at_ms: i64,
    updated_at_ms: i64,
) -> MusicLibraryResult<()> {
    let inserted = sqlx::query(
        "INSERT INTO music_library_items
            (id, identity_key, source_kind, media_kind, original_title, original_artist,
             original_album, original_track_number, original_artwork_identity, duration_ms,
             availability, discovered_at_ms, updated_at_ms)
         VALUES (?, ?, 'local-file', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(item_id)
    .bind(format!("local:{item_id}"))
    .bind(values.media_kind)
    .bind(values.title)
    .bind(values.artist)
    .bind(values.album)
    .bind(values.track_number)
    .bind(values.artwork_identity)
    .bind(values.duration_ms)
    .bind(availability)
    .bind(discovered_at_ms)
    .bind(updated_at_ms)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_unique("save discovered music item", error))?;
    if inserted.rows_affected() > 0 {
        return Ok(());
    }
    sqlx::query(
        "UPDATE music_library_items
         SET media_kind = ?, original_title = ?, original_artist = ?, original_album = ?,
             original_track_number = ?, duration_ms = ?, updated_at_ms = ?,
             version = version + 1
         WHERE id = ? AND (media_kind, original_title, original_artist, original_album,
                           original_track_number, duration_ms)
               IS NOT (?, ?, ?, ?, ?, ?)",
    )
    .bind(values.media_kind)
    .bind(values.title)
    .bind(values.artist)
    .bind(values.album)
    .bind(values.track_number)
    .bind(values.duration_ms)
    .bind(updated_at_ms)
    .bind(item_id)
    .bind(values.media_kind)
    .bind(values.title)
    .bind(values.artist)
    .bind(values.album)
    .bind(values.track_number)
    .bind(values.duration_ms)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("update discovered music item", error))?;
    sqlx::query(
        "UPDATE music_library_items SET availability = ?, original_artwork_identity = ?
         WHERE id = ? AND (availability, original_artwork_identity) IS NOT (?, ?)",
    )
    .bind(availability)
    .bind(values.artwork_identity)
    .bind(item_id)
    .bind(availability)
    .bind(values.artwork_identity)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("observe discovered music item", error))?;
    Ok(())
}

/// Inserts a scanned location or updates only the values that differ.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_scanned_location(
    transaction: &mut Transaction<'_, Sqlite>,
    root_id: &str,
    item_id: &str,
    values: &LocalScanValues<'_>,
    availability: &str,
    generation: i64,
    first_seen_at_ms: i64,
    updated_at_ms: i64,
) -> MusicLibraryResult<()> {
    let location_id = creatable_key(
        transaction,
        RetirableTable::Locations,
        crate::identity::location_id(root_id, values.relative_path),
    )
    .await?;
    let inserted = sqlx::query(
        "INSERT INTO music_local_locations
            (id, item_id, root_id, relative_path, file_size_bytes, modified_at_ms,
             lightweight_fingerprint, strong_fingerprint, availability,
             last_seen_generation, first_seen_at_ms, updated_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(root_id, relative_path) DO NOTHING",
    )
    .bind(&location_id)
    .bind(item_id)
    .bind(root_id)
    .bind(values.relative_path)
    .bind(values.file_size_bytes)
    .bind(values.modified_at_ms)
    .bind(values.lightweight_fingerprint)
    .bind(values.strong_fingerprint)
    .bind(availability)
    .bind(generation)
    .bind(first_seen_at_ms)
    .bind(updated_at_ms)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_unique("save discovered music location", error))?;
    if inserted.rows_affected() > 0 {
        return Ok(());
    }
    sqlx::query(
        "UPDATE music_local_locations
         SET item_id = ?, lightweight_fingerprint = ?, strong_fingerprint = ?, updated_at_ms = ?
         WHERE root_id = ? AND relative_path = ?
           AND (item_id, lightweight_fingerprint, strong_fingerprint) IS NOT (?, ?, ?)",
    )
    .bind(item_id)
    .bind(values.lightweight_fingerprint)
    .bind(values.strong_fingerprint)
    .bind(updated_at_ms)
    .bind(root_id)
    .bind(values.relative_path)
    .bind(item_id)
    .bind(values.lightweight_fingerprint)
    .bind(values.strong_fingerprint)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_unique("update discovered music location", error))?;
    sqlx::query(
        "UPDATE music_local_locations
         SET file_size_bytes = ?, modified_at_ms = ?, availability = ?, last_seen_generation = ?
         WHERE root_id = ? AND relative_path = ?",
    )
    .bind(values.file_size_bytes)
    .bind(values.modified_at_ms)
    .bind(availability)
    .bind(generation)
    .bind(root_id)
    .bind(values.relative_path)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("observe discovered music location", error))?;
    Ok(())
}

/// Links a scanned item to its source collection for `generation`. A missing position keeps
/// the stored one.
pub async fn link_collection_item(
    transaction: &mut Transaction<'_, Sqlite>,
    collection_id: &str,
    item_id: &str,
    source_position: Option<i64>,
    generation: i64,
    discovered_at_ms: i64,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "INSERT INTO music_source_collection_items
            (collection_id, item_id, source_position, first_discovered_at_ms,
             last_seen_generation, missing_from_latest_snapshot, sync_key)
         VALUES (?, ?, ?, ?, ?, 0, ?)
         ON CONFLICT(collection_id, item_id) DO UPDATE SET
            source_position = COALESCE(excluded.source_position, source_position),
            last_seen_generation = excluded.last_seen_generation,
            missing_from_latest_snapshot = 0",
    )
    .bind(collection_id)
    .bind(item_id)
    .bind(source_position)
    .bind(discovered_at_ms)
    .bind(generation)
    .bind(ganbaru_music::sync_keys::collection_item_key(
        collection_id,
        item_id,
    )?)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save local source provenance", error))?;
    Ok(())
}

/// Marks locations of `root_id` and items of `collection_id` that a complete scan in
/// `generation` did not see.
pub async fn mark_unseen(
    transaction: &mut Transaction<'_, Sqlite>,
    collection_id: &str,
    root_id: &str,
    generation: i64,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "UPDATE music_local_locations SET availability = 'missing'
         WHERE root_id = ? AND COALESCE(last_seen_generation, 0) < ?
           AND availability IS NOT 'missing'",
    )
    .bind(root_id)
    .bind(generation)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("mark absent local music", error))?;
    sqlx::query(
        "UPDATE music_source_collection_items SET missing_from_latest_snapshot = 1
         WHERE collection_id = ? AND last_seen_generation < ?
           AND missing_from_latest_snapshot = 0",
    )
    .bind(collection_id)
    .bind(generation)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("mark absent source provenance", error))?;
    Ok(())
}

/// Derives the availability of local items with a location in `root_id` from their locations.
pub async fn reconcile_item_availability(
    transaction: &mut Transaction<'_, Sqlite>,
    root_id: &str,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "UPDATE music_library_items AS item
         SET availability = derived.availability
         FROM (
            SELECT candidate.id AS id, CASE
                WHEN EXISTS (
                    SELECT 1 FROM music_local_locations AS location
                    WHERE location.item_id = candidate.id AND location.availability = 'available'
                ) THEN 'available'
                WHEN EXISTS (
                    SELECT 1 FROM music_local_locations AS location
                    WHERE location.item_id = candidate.id AND location.availability = 'ambiguous'
                ) THEN 'ambiguous'
                ELSE 'missing'
            END AS availability
            FROM music_library_items AS candidate
            WHERE candidate.source_kind = 'local-file' AND EXISTS (
                SELECT 1 FROM music_local_locations AS location
                WHERE location.item_id = candidate.id AND location.root_id = ?
            )
         ) AS derived
         WHERE item.id = derived.id AND item.availability IS NOT derived.availability",
    )
    .bind(root_id)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("reconcile local item availability", error))?;
    Ok(())
}

fn map_unique(context: &str, error: sqlx::Error) -> MusicLibraryError {
    if error
        .as_database_error()
        .is_some_and(|database| database.is_unique_violation())
    {
        MusicLibraryError::conflict(format!("{context}: the canonical identity is already used"))
    } else {
        MusicLibraryError::database(context, error)
    }
}

#[cfg(test)]
mod tests;
