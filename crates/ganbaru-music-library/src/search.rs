use super::*;
use sqlx::{QueryBuilder, Sqlite, SqlitePool, Transaction};

pub(crate) const SEARCH_SCHEMA_VERSION: i64 = 1;
pub(crate) const SEARCH_FINGERPRINT: &str =
    "music-search-v1-title-artist-album-sources-paths-signals-playlists";

fn push_projection(builder: &mut QueryBuilder<'_, Sqlite>) {
    builder.push(
        "SELECT item.id,
            COALESCE(item.title_override, item.original_title),
            COALESCE(item.artist_override, item.original_artist),
            COALESCE(item.album_override, item.original_album),
            COALESCE((
                SELECT group_concat(name, ' ') FROM (
                    SELECT DISTINCT source.name AS name
                    FROM music_source_collection_items AS source_item
                    JOIN music_source_collections AS source ON source.id = source_item.collection_id
                    WHERE source_item.item_id = item.id
                    ORDER BY source.name COLLATE NOCASE, source.id
                )
            ), ''),
            COALESCE((
                SELECT group_concat(relative_path, ' ') FROM (
                    SELECT location.relative_path
                    FROM music_local_locations AS location
                    WHERE location.item_id = item.id
                    ORDER BY location.root_id, location.relative_path
                )
            ), ''),
            COALESCE((
                SELECT group_concat(signal, ' ') FROM (
                    SELECT signal.signal
                    FROM music_item_signals AS signal
                    WHERE signal.item_id = item.id
                    ORDER BY signal.signal
                )
            ), ''),
            COALESCE((
                SELECT group_concat(metadata, ' ') FROM (
                    SELECT playlist.name || ' ' ||
                           COALESCE((
                               SELECT group_concat(intended_use, ' ')
                               FROM music_playlist_intended_uses AS intended
                               WHERE intended.playlist_id = playlist.id
                           ), '') AS metadata
                    FROM music_playlist_memberships AS membership
                    JOIN music_playlists AS playlist ON playlist.id = membership.playlist_id
                    WHERE membership.item_id = item.id
                    ORDER BY playlist.name COLLATE NOCASE, playlist.id
                )
            ), '')
         FROM music_library_items AS item",
    );
}

async fn replace_index_state(
    transaction: &mut Transaction<'_, Sqlite>,
    rebuilt_at_ms: i64,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "INSERT INTO music_search_index_state (singleton, schema_version, fingerprint, rebuilt_at_ms)
         VALUES (1, ?, ?, ?)
         ON CONFLICT(singleton) DO UPDATE SET
            schema_version = excluded.schema_version,
            fingerprint = excluded.fingerprint,
            rebuilt_at_ms = excluded.rebuilt_at_ms",
    )
    .bind(SEARCH_SCHEMA_VERSION)
    .bind(SEARCH_FINGERPRINT)
    .bind(rebuilt_at_ms)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save music search index state", error))?;
    Ok(())
}

pub async fn rebuild(
    pool: &SqlitePool,
    rebuilt_at_ms: i64,
) -> MusicLibraryResult<MusicSearchRebuildResult> {
    if rebuilt_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "rebuiltAt",
            "must be a positive Unix epoch millisecond value",
        ));
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin music search rebuild", error))?;
    sqlx::query("DELETE FROM music_search_fts")
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("clear music search index", error))?;
    let mut insert = QueryBuilder::<Sqlite>::new(
        "INSERT INTO music_search_fts
            (item_id, title, artist, album, source_collections, relative_paths, signals, playlist_metadata) ",
    );
    push_projection(&mut insert);
    insert
        .build()
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("rebuild music search index", error))?;
    replace_index_state(&mut transaction, rebuilt_at_ms).await?;
    let indexed_item_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM music_search_fts")
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("count music search index", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit music search rebuild", error))?;
    Ok(MusicSearchRebuildResult {
        indexed_item_count,
        schema_version: SEARCH_SCHEMA_VERSION,
        fingerprint: SEARCH_FINGERPRINT.to_string(),
        rebuilt_at_ms,
    })
}

pub(crate) async fn ensure(
    pool: &SqlitePool,
    now_ms: i64,
) -> MusicLibraryResult<Option<MusicSearchRebuildResult>> {
    let state: Option<(i64, String)> = sqlx::query_as(
        "SELECT schema_version, fingerprint FROM music_search_index_state WHERE singleton = 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music search index state", error))?;
    if state.as_ref().is_some_and(|(version, fingerprint)| {
        *version == SEARCH_SCHEMA_VERSION && fingerprint == SEARCH_FINGERPRINT
    }) {
        Ok(None)
    } else {
        rebuild(pool, now_ms).await.map(Some)
    }
}

pub(crate) async fn refresh_item(
    transaction: &mut Transaction<'_, Sqlite>,
    item_id: &str,
) -> MusicLibraryResult<()> {
    refresh_items(transaction, &[item_id.to_string()]).await
}

pub(crate) async fn refresh_items(
    transaction: &mut Transaction<'_, Sqlite>,
    item_ids: &[String],
) -> MusicLibraryResult<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    let mut delete = QueryBuilder::<Sqlite>::new("DELETE FROM music_search_fts WHERE item_id IN (");
    let mut separated = delete.separated(", ");
    for item_id in item_ids {
        separated.push_bind(item_id);
    }
    separated.push_unseparated(")");
    delete
        .build()
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("remove stale music search rows", error))?;
    let mut insert = QueryBuilder::<Sqlite>::new(
        "INSERT INTO music_search_fts
            (item_id, title, artist, album, source_collections, relative_paths, signals, playlist_metadata) ",
    );
    push_projection(&mut insert);
    insert.push(" WHERE item.id IN (");
    let mut separated = insert.separated(", ");
    for item_id in item_ids {
        separated.push_bind(item_id);
    }
    separated.push_unseparated(")");
    insert
        .build()
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("refresh music search rows", error))?;
    Ok(())
}

pub(crate) async fn refresh_playlist(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_id: &str,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "DELETE FROM music_search_fts
         WHERE item_id IN (
            SELECT item_id FROM music_playlist_memberships WHERE playlist_id = ?
         )",
    )
    .bind(playlist_id)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("remove stale playlist search rows", error))?;
    let mut insert = QueryBuilder::<Sqlite>::new(
        "INSERT INTO music_search_fts
            (item_id, title, artist, album, source_collections, relative_paths, signals, playlist_metadata) ",
    );
    push_projection(&mut insert);
    insert.push(
        " WHERE item.id IN (
            SELECT item_id FROM music_playlist_memberships WHERE playlist_id = ",
    );
    insert.push_bind(playlist_id.to_string());
    insert.push(")");
    insert
        .build()
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("refresh playlist search rows", error))?;
    Ok(())
}

pub(crate) fn match_expression(value: &str) -> Option<String> {
    let tokens = value
        .split_whitespace()
        .map(|token| token.trim_matches(|character: char| !character.is_alphanumeric()))
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" AND "))
}
