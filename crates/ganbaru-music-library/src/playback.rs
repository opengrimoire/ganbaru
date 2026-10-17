use super::*;
use sqlx::SqliteConnection;
#[cfg(test)]
use sqlx::SqlitePool;

const RECENT_SELECTIONS_PER_CONTEXT: i64 = 64;
const RECENT_SELECTIONS_GLOBAL: i64 = 512;

#[cfg(test)]
pub(crate) async fn record_listening(
    pool: &SqlitePool,
    request: MusicListeningUpdate,
) -> MusicLibraryResult<()> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin listening update", error))?;
    record_listening_in_transaction(&mut transaction, &request).await?;
    super::writes::commit(transaction, "commit listening update").await
}

/// Records a selection outcome in the same transaction as its session transition.
pub async fn record_listening_in_transaction(
    connection: &mut SqliteConnection,
    request: &MusicListeningUpdate,
) -> MusicLibraryResult<()> {
    validate_id(&request.item_id, "itemId")?;
    if let Some(playlist_id) = &request.playlist_id {
        validate_id(playlist_id, "playlistId")?;
    }
    if request.occurred_at <= 0 {
        return Err(MusicLibraryError::validation(
            "occurredAt",
            "must be a positive Unix epoch millisecond value",
        ));
    }
    let (play_increment, completion_increment, skip_increment) = match request.outcome {
        MusicListeningOutcome::Started => (1_i64, 0_i64, 0_i64),
        MusicListeningOutcome::Completed => (0, 1, 0),
        MusicListeningOutcome::Skipped => (0, 0, 1),
    };
    sqlx::query(
        "INSERT INTO music_listening_statistics
            (item_id, last_played_at_ms, play_count, completion_count, skip_count, updated_at_ms)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(item_id) DO UPDATE SET
            last_played_at_ms = CASE WHEN excluded.play_count > 0
                THEN excluded.last_played_at_ms ELSE music_listening_statistics.last_played_at_ms END,
            play_count = music_listening_statistics.play_count + excluded.play_count,
            completion_count = music_listening_statistics.completion_count + excluded.completion_count,
            skip_count = music_listening_statistics.skip_count + excluded.skip_count,
            updated_at_ms = excluded.updated_at_ms",
    )
    .bind(&request.item_id)
    .bind((play_increment > 0).then_some(request.occurred_at))
    .bind(play_increment)
    .bind(completion_increment)
    .bind(skip_increment)
    .bind(request.occurred_at)
    .execute(&mut *connection)
    .await
    .map_err(|error| MusicLibraryError::database("update listening statistics", error))?;

    if request.outcome == MusicListeningOutcome::Started {
        sqlx::query(
            "INSERT INTO music_recent_selections
                (playlist_id, item_id, selection_kind, selected_at_ms)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&request.playlist_id)
        .bind(&request.item_id)
        .bind(request.selection_kind.as_ref())
        .bind(request.occurred_at)
        .execute(&mut *connection)
        .await
        .map_err(|error| MusicLibraryError::database("record recent music selection", error))?;
        sqlx::query(
            "DELETE FROM music_recent_selections
             WHERE playlist_id IS ? AND id NOT IN (
                SELECT id FROM music_recent_selections
                WHERE playlist_id IS ?
                ORDER BY selected_at_ms DESC, id DESC LIMIT ?
             )",
        )
        .bind(&request.playlist_id)
        .bind(&request.playlist_id)
        .bind(RECENT_SELECTIONS_PER_CONTEXT)
        .execute(&mut *connection)
        .await
        .map_err(|error| MusicLibraryError::database("bound playlist recent selections", error))?;
        sqlx::query(
            "DELETE FROM music_recent_selections WHERE id NOT IN (
                SELECT id FROM music_recent_selections
                ORDER BY selected_at_ms DESC, id DESC LIMIT ?
             )",
        )
        .bind(RECENT_SELECTIONS_GLOBAL)
        .execute(&mut *connection)
        .await
        .map_err(|error| MusicLibraryError::database("bound global recent selections", error))?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) async fn recent_selections(
    pool: &SqlitePool,
    playlist_id: Option<String>,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicRecentSelection>> {
    if let Some(id) = &playlist_id {
        validate_id(id, "playlistId")?;
    }
    if !(1..=RECENT_SELECTIONS_PER_CONTEXT).contains(&limit) {
        return Err(MusicLibraryError::validation(
            "limit",
            format!("must be between 1 and {RECENT_SELECTIONS_PER_CONTEXT}"),
        ));
    }
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT item_id, selected_at_ms FROM music_recent_selections
         WHERE playlist_id IS ?
         ORDER BY selected_at_ms DESC, id DESC LIMIT ?",
    )
    .bind(playlist_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load recent music selections", error))?;
    Ok(rows
        .into_iter()
        .map(|(item_id, selected_at_ms)| MusicRecentSelection {
            item_id,
            selected_at_ms,
        })
        .collect())
}
