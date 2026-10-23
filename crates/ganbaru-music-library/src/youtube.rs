use crate::identity::{collection_id, stable_id, youtube_item_id, youtube_playlist_identity_key};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::{HashMap, HashSet};

use super::*;

const MAX_YOUTUBE_PLAYLIST_ITEMS: usize = 5_000;
const MAX_YOUTUBE_TITLE_CHARS: usize = 500;
const MAX_YOUTUBE_CHANNEL_CHARS: usize = 300;
const MAX_YOUTUBE_ERROR_CHARS: usize = 200;

pub async fn duplicate_video_count(
    pool: &SqlitePool,
    video_ids: Vec<String>,
) -> MusicLibraryResult<i64> {
    if video_ids.len() > MAX_YOUTUBE_PLAYLIST_ITEMS {
        return Err(MusicLibraryError::validation(
            "videoIds",
            format!("cannot contain more than {MAX_YOUTUBE_PLAYLIST_ITEMS} items"),
        ));
    }
    let unique = video_ids.into_iter().collect::<HashSet<_>>();
    for (index, video_id) in unique.iter().enumerate() {
        validate_video_id(video_id, &format!("videoIds[{index}]"))?;
    }
    let encoded = serde_json::to_string(&unique)
        .map_err(|error| MusicLibraryError::runtime("encode YouTube ids", error))?;
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM music_library_items
         WHERE youtube_video_id IN (SELECT value FROM json_each(?))",
    )
    .bind(encoded)
    .fetch_one(pool)
    .await
    .map_err(|error| MusicLibraryError::database("count known YouTube videos", error))
}

pub async fn upsert_video(
    pool: &SqlitePool,
    request: MusicYouTubeVideoWrite,
) -> MusicLibraryResult<MusicWriteReceipt> {
    validate_video_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin YouTube video save", error))?;
    let item_id = canonical_youtube_item_id(&mut transaction, &request.video_id).await?;
    upsert_video_row(
        &mut transaction,
        &item_id,
        &request,
        VideoWriteOrigin::Video,
    )
    .await?;
    super::search::refresh_item(&mut transaction, &item_id).await?;
    let version: i64 = sqlx::query_scalar("SELECT version FROM music_library_items WHERE id = ?")
        .bind(&item_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("load saved YouTube video", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit YouTube video save", error))?;
    Ok(MusicWriteReceipt {
        id: item_id,
        version,
    })
}

pub async fn apply_playlist_snapshot(
    pool: &SqlitePool,
    request: MusicYouTubePlaylistSnapshotWrite,
) -> MusicLibraryResult<MusicYouTubeSnapshotResult> {
    validate_playlist_snapshot(&request)?;
    let mut seen = HashSet::new();
    let unique_video_ids = request
        .video_ids
        .iter()
        .filter(|video_id| seen.insert(video_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let repeated_video_count = request.video_ids.len() - unique_video_ids.len();
    let metadata = request
        .videos
        .iter()
        .map(|video| (video.video_id.as_str(), video))
        .collect::<HashMap<_, _>>();
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin YouTube playlist snapshot", error))?;
    let (collection_id, generation) = upsert_collection(&mut transaction, &request).await?;
    let mut newly_discovered_count = 0_i64;
    for (position, video_id) in unique_video_ids.iter().enumerate() {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM music_library_items WHERE youtube_video_id = ?)",
        )
        .bind(video_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("match YouTube playlist video", error))?;
        if !exists {
            newly_discovered_count += 1;
        }
        let item_id = canonical_youtube_item_id(&mut transaction, video_id).await?;
        let video_metadata = metadata.get(video_id.as_str());
        upsert_video_row(
            &mut transaction,
            &item_id,
            &MusicYouTubeVideoWrite {
                video_id: video_id.clone(),
                title: video_metadata.map_or_else(String::new, |video| video.title.clone()),
                channel: video_metadata.map_or_else(String::new, |video| video.channel.clone()),
                duration_ms: None,
                resolution_state: if video_metadata.is_some() {
                    MusicYouTubeResolutionState::Ready
                } else {
                    MusicYouTubeResolutionState::Resolving
                },
                resolved_at_ms: request.resolved_at_ms,
            },
            VideoWriteOrigin::PlaylistSnapshot,
        )
        .await?;
        sqlx::query(
            "INSERT INTO music_source_collection_items
                (collection_id, item_id, source_position, first_discovered_at_ms,
                 last_seen_generation, missing_from_latest_snapshot, sync_key)
             VALUES (?, ?, ?, ?, ?, 0, ?)
             ON CONFLICT(collection_id, item_id) DO UPDATE SET
                source_position = excluded.source_position,
                last_seen_generation = excluded.last_seen_generation,
                missing_from_latest_snapshot = 0",
        )
        .bind(&collection_id)
        .bind(&item_id)
        .bind(position as i64)
        .bind(request.resolved_at_ms)
        .bind(generation)
        .bind(ganbaru_music::sync_keys::collection_item_key(
            &collection_id,
            &item_id,
        )?)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save YouTube playlist order", error))?;
        super::search::refresh_item(&mut transaction, &item_id).await?;
    }
    sqlx::query(
        "UPDATE music_source_collection_items
         SET missing_from_latest_snapshot = 1
         WHERE collection_id = ? AND last_seen_generation < ?",
    )
    .bind(&collection_id)
    .bind(generation)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("reconcile YouTube playlist snapshot", error))?;
    sqlx::query(
        "UPDATE music_source_collections
         SET refresh_state = 'idle', last_successful_refresh_at_ms = ?,
             previous_successful_refresh_at_ms = last_successful_refresh_at_ms,
             last_refresh_error_code = NULL, snapshot_generation = ?
         WHERE id = ?",
    )
    .bind(request.resolved_at_ms)
    .bind(generation)
    .bind(&collection_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("complete YouTube playlist refresh", error))?;
    resolve_collection_failures(&mut transaction, &collection_id, request.resolved_at_ms).await?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit YouTube playlist snapshot", error))?;
    Ok(MusicYouTubeSnapshotResult {
        collection_id,
        canonical_item_count: unique_video_ids.len() as i64,
        newly_discovered_count,
        repeated_video_count: repeated_video_count as i64,
        generation,
    })
}

pub async fn report_source_failure(
    pool: &SqlitePool,
    request: MusicYouTubeSourceFailureWrite,
) -> MusicLibraryResult<()> {
    validate_source_failure(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin YouTube source failure", error))?;
    let collection_id = ensure_collection(
        &mut transaction,
        &request.playlist_id,
        &request.name,
        request.occurred_at,
    )
    .await?;
    sqlx::query(
        "UPDATE music_source_collections
         SET refresh_state = 'failed', last_refresh_error_code = ? WHERE id = ?",
    )
    .bind(&request.error_code)
    .bind(&collection_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("mark YouTube source failure", error))?;
    let occurrence = request.occurred_at.to_string();
    let job_id = stable_id("youtube-failure", &[&collection_id, &occurrence]);
    let generation: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(generation), 0) + 1 FROM music_refresh_jobs
         WHERE source_collection_id = ?",
    )
    .bind(&collection_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("allocate YouTube failure generation", error))?;
    sqlx::query(
        "INSERT INTO music_refresh_jobs
            (id, source_collection_id, kind, state, generation, issue_count,
             status_message, requested_at_ms, started_at_ms, finished_at_ms, updated_at_ms)
         VALUES (?, ?, 'youtube-playlist', 'failed', ?, 1, ?, ?, ?, ?, ?)",
    )
    .bind(&job_id)
    .bind(&collection_id)
    .bind(generation)
    .bind(failure_message(request.resolution_state))
    .bind(request.occurred_at)
    .bind(request.occurred_at)
    .bind(request.occurred_at)
    .bind(request.occurred_at)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save YouTube failure job", error))?;
    sqlx::query(
        "INSERT INTO music_refresh_job_issues
            (id, job_id, issue_code, message, created_at_ms) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(stable_id("youtube-issue", &[&job_id, &request.error_code]))
    .bind(&job_id)
    .bind(&request.error_code)
    .bind(failure_message(request.resolution_state))
    .bind(request.occurred_at)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save YouTube source issue", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit YouTube source failure", error))
}

/// What asks for a YouTube video row to be written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VideoWriteOrigin {
    /// A save for one video: playback starting or reporting its metadata or failure, or the user
    /// adding it. It records the resolution it carries, except that `resolving` never replaces
    /// `ready`.
    Video,
    /// A playlist refresh naming the video. It never replaces a resolution already recorded;
    /// only playing the video resolves it again.
    PlaylistSnapshot,
}

/// The resolution state a write leaves, or `None` when the stored one stays.
fn next_resolution_state(
    origin: VideoWriteOrigin,
    stored: Option<&str>,
    requested: MusicYouTubeResolutionState,
) -> Option<MusicYouTubeResolutionState> {
    let stored_resolving =
        stored.is_none_or(|state| state == MusicYouTubeResolutionState::Resolving.as_ref());
    let keeps_stored = match origin {
        VideoWriteOrigin::Video => {
            requested == MusicYouTubeResolutionState::Resolving
                && stored == Some(MusicYouTubeResolutionState::Ready.as_ref())
        }
        VideoWriteOrigin::PlaylistSnapshot => !stored_resolving,
    };
    (!keeps_stored).then_some(requested)
}

async fn upsert_video_row(
    transaction: &mut Transaction<'_, Sqlite>,
    item_id: &str,
    request: &MusicYouTubeVideoWrite,
    origin: VideoWriteOrigin,
) -> MusicLibraryResult<()> {
    let supplied_title = request.title.trim();
    // Metadata is resolved once a ready video supplies its own title; a placeholder named by
    // its video id loses to it when devices write both concurrently.
    let supplies_metadata = request.resolution_state == MusicYouTubeResolutionState::Ready
        && !supplied_title.is_empty();
    let stored_title = if supplied_title.is_empty() {
        request.video_id.as_str()
    } else {
        supplied_title
    };
    let availability = match request.resolution_state {
        MusicYouTubeResolutionState::Ready => "available",
        MusicYouTubeResolutionState::Resolving => "unknown",
        MusicYouTubeResolutionState::Unavailable
        | MusicYouTubeResolutionState::EmbeddingBlocked
        | MusicYouTubeResolutionState::TimedOut => "unavailable",
    };
    let existing =
        sqlx::query_as::<_, (String, String, String, Option<i64>, Option<String>, bool)>(
            "SELECT id, original_title, original_artist, duration_ms, youtube_resolution_state,
                metadata_resolved
         FROM music_library_items WHERE youtube_video_id = ?",
        )
        .bind(&request.video_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("load canonical YouTube video", error))?;
    let Some((existing_id, title, artist, duration_ms, state, metadata_resolved)) = existing else {
        sqlx::query(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, media_kind, youtube_video_id,
                 original_title, original_artist, duration_ms, metadata_resolved, availability,
                 youtube_resolution_state, discovered_at_ms, updated_at_ms)
             VALUES (?, ?, 'youtube-video', 'video', ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(item_id)
        .bind(format!("youtube:video:{}", request.video_id))
        .bind(&request.video_id)
        .bind(stored_title)
        .bind(request.channel.trim())
        .bind(request.duration_ms)
        .bind(supplies_metadata)
        .bind(availability)
        .bind(request.resolution_state.as_ref())
        .bind(request.resolved_at_ms)
        .bind(request.resolved_at_ms)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save canonical YouTube video", error))?;
        return Ok(());
    };
    let resolution = next_resolution_state(origin, state.as_deref(), request.resolution_state);
    let next_metadata_resolved = metadata_resolved || supplies_metadata;
    let next_title = if supplied_title.is_empty() {
        title.as_str()
    } else {
        supplied_title
    };
    let supplied_artist = request.channel.trim();
    let next_artist = if supplied_artist.is_empty() {
        artist.as_str()
    } else {
        supplied_artist
    };
    let next_duration = request.duration_ms.or(duration_ms);
    let next_state = resolution.map_or(state, |resolution| Some(resolution.as_ref().to_string()));
    sqlx::query(
        "UPDATE music_library_items
         SET original_title = ?, original_artist = ?, duration_ms = ?, metadata_resolved = ?,
             youtube_resolution_state = ?, updated_at_ms = ?, version = version + 1
         WHERE id = ?
           AND (original_title, original_artist, duration_ms, metadata_resolved,
                youtube_resolution_state)
               IS NOT (?, ?, ?, ?, ?)",
    )
    .bind(next_title)
    .bind(next_artist)
    .bind(next_duration)
    .bind(next_metadata_resolved)
    .bind(next_state.as_deref())
    .bind(request.resolved_at_ms)
    .bind(&existing_id)
    .bind(next_title)
    .bind(next_artist)
    .bind(next_duration)
    .bind(next_metadata_resolved)
    .bind(next_state.as_deref())
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save canonical YouTube video", error))?;
    if resolution.is_some() {
        // Availability is observed by this device, so it changes without advancing the item.
        sqlx::query(
            "UPDATE music_library_items SET availability = ?
             WHERE id = ? AND availability IS NOT ?",
        )
        .bind(availability)
        .bind(&existing_id)
        .bind(availability)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save YouTube video availability", error))?;
    }
    Ok(())
}

/// Ensures the playlist's collection and returns its id and next snapshot generation.
async fn upsert_collection(
    transaction: &mut Transaction<'_, Sqlite>,
    request: &MusicYouTubePlaylistSnapshotWrite,
) -> MusicLibraryResult<(String, i64)> {
    let collection_id = ensure_collection(
        transaction,
        &request.playlist_id,
        &request.name,
        request.resolved_at_ms,
    )
    .await?;
    let generation = sqlx::query_scalar(
        "SELECT snapshot_generation + 1 FROM music_source_collections WHERE id = ?",
    )
    .bind(&collection_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("allocate YouTube snapshot generation", error))?;
    Ok((collection_id, generation))
}

/// Finds the collection that mirrors `playlist_id`, by playlist id and then identity key, or
/// creates it under the derived collection key, and marks it running on this device.
async fn ensure_collection(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_id: &str,
    name: &str,
    at_ms: i64,
) -> MusicLibraryResult<String> {
    let identity_key = youtube_playlist_identity_key(playlist_id);
    let existing: Option<(String, i64)> = sqlx::query_as(
        "SELECT id, discovery_enabled FROM music_source_collections
         WHERE kind = 'youtube-playlist' AND (youtube_playlist_id = ? OR identity_key = ?)
         ORDER BY youtube_playlist_id = ? DESC LIMIT 1",
    )
    .bind(playlist_id)
    .bind(&identity_key)
    .bind(playlist_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("match YouTube source collection", error))?;
    let collection_id = match existing {
        Some((_, 0)) => {
            return Err(MusicLibraryError::conflict(
                "discovery is disabled for this YouTube source",
            ));
        }
        Some((id, _)) => {
            sqlx::query(
                "UPDATE music_source_collections
                 SET name = ?, updated_at_ms = ?, version = version + 1
                 WHERE id = ? AND name IS NOT ?",
            )
            .bind(name.trim())
            .bind(at_ms)
            .bind(&id)
            .bind(name.trim())
            .execute(&mut **transaction)
            .await
            .map_err(|error| {
                MusicLibraryError::database("rename YouTube source collection", error)
            })?;
            id
        }
        None => {
            let id = crate::identity::creatable_key(
                transaction,
                crate::identity::RetirableTable::Collections,
                collection_id(&identity_key),
            )
            .await?;
            sqlx::query(
                "INSERT INTO music_source_collections
                    (id, kind, identity_key, name, youtube_playlist_id, created_at_ms, updated_at_ms)
                 VALUES (?, 'youtube-playlist', ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(&identity_key)
            .bind(name.trim())
            .bind(playlist_id)
            .bind(at_ms)
            .bind(at_ms)
            .execute(&mut **transaction)
            .await
            .map_err(|error| super::writes::map_database_error("create YouTube source collection", error))?;
            id
        }
    };
    sqlx::query(
        "UPDATE music_source_collections
         SET refresh_state = 'running', last_refresh_error_code = NULL WHERE id = ?",
    )
    .bind(&collection_id)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("start YouTube source refresh", error))?;
    Ok(collection_id)
}

async fn resolve_collection_failures(
    transaction: &mut Transaction<'_, Sqlite>,
    collection_id: &str,
    resolved_at_ms: i64,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "UPDATE music_refresh_jobs SET status_message = 'A later refresh succeeded.',
             updated_at_ms = ?
         WHERE source_collection_id = ? AND kind = 'youtube-playlist' AND state = 'failed'",
    )
    .bind(resolved_at_ms)
    .bind(collection_id)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("resolve YouTube source failures", error))?;
    Ok(())
}

fn validate_video_write(request: &MusicYouTubeVideoWrite) -> MusicLibraryResult<()> {
    validate_video_id(&request.video_id, "videoId")?;
    validate_text(&request.title, "title", MAX_YOUTUBE_TITLE_CHARS)?;
    validate_text(&request.channel, "channel", MAX_YOUTUBE_CHANNEL_CHARS)?;
    validate_time(request.resolved_at_ms, "resolvedAt")?;
    if request.duration_ms.is_some_and(|duration| duration < 0) {
        return Err(MusicLibraryError::validation(
            "durationMs",
            "must be zero or greater",
        ));
    }
    Ok(())
}

fn validate_playlist_snapshot(
    request: &MusicYouTubePlaylistSnapshotWrite,
) -> MusicLibraryResult<()> {
    validate_collection_fields(&request.playlist_id, &request.name, request.resolved_at_ms)?;
    if request.video_ids.len() > MAX_YOUTUBE_PLAYLIST_ITEMS {
        return Err(MusicLibraryError::validation(
            "videoIds",
            format!("cannot contain more than {MAX_YOUTUBE_PLAYLIST_ITEMS} items"),
        ));
    }
    for (index, video_id) in request.video_ids.iter().enumerate() {
        validate_video_id(video_id, &format!("videoIds[{index}]"))?;
    }
    if request.videos.len() > request.video_ids.len() {
        return Err(MusicLibraryError::validation(
            "videos",
            "cannot contain more entries than videoIds",
        ));
    }
    let video_ids = request.video_ids.iter().collect::<HashSet<_>>();
    let mut metadata_ids = HashSet::new();
    for (index, video) in request.videos.iter().enumerate() {
        validate_video_id(&video.video_id, &format!("videos[{index}].videoId"))?;
        validate_text(
            &video.title,
            &format!("videos[{index}].title"),
            MAX_YOUTUBE_TITLE_CHARS,
        )?;
        validate_text(
            &video.channel,
            &format!("videos[{index}].channel"),
            MAX_YOUTUBE_CHANNEL_CHARS,
        )?;
        if !video_ids.contains(&video.video_id) || !metadata_ids.insert(video.video_id.as_str()) {
            return Err(MusicLibraryError::validation(
                "videos",
                "must contain unique metadata for videos in videoIds",
            ));
        }
    }
    Ok(())
}

fn validate_source_failure(request: &MusicYouTubeSourceFailureWrite) -> MusicLibraryResult<()> {
    validate_collection_fields(&request.playlist_id, &request.name, request.occurred_at)?;
    if matches!(
        request.resolution_state,
        MusicYouTubeResolutionState::Ready | MusicYouTubeResolutionState::Resolving
    ) {
        return Err(MusicLibraryError::validation(
            "resolutionState",
            "must describe an unavailable, blocked, or timed-out source",
        ));
    }
    if request.error_code.trim().is_empty()
        || request.error_code.chars().count() > MAX_YOUTUBE_ERROR_CHARS
    {
        return Err(MusicLibraryError::validation(
            "errorCode",
            "is required and must be concise",
        ));
    }
    Ok(())
}

fn validate_collection_fields(
    playlist_id: &str,
    name: &str,
    timestamp: i64,
) -> MusicLibraryResult<()> {
    if playlist_id.len() < 6
        || playlist_id.len() > 100
        || !playlist_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(MusicLibraryError::validation(
            "playlistId",
            "is not a supported YouTube playlist id",
        ));
    }
    if name.trim().is_empty() || name.chars().count() > MAX_YOUTUBE_TITLE_CHARS {
        return Err(MusicLibraryError::validation(
            "name",
            "is required and exceeds the title limit",
        ));
    }
    validate_time(timestamp, "timestamp")
}

fn validate_video_id(video_id: &str, field: &str) -> MusicLibraryResult<()> {
    if video_id.len() != 11
        || !video_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(MusicLibraryError::validation(
            field,
            "is not a supported YouTube video id",
        ));
    }
    Ok(())
}

fn validate_text(value: &str, field: &str, max_chars: usize) -> MusicLibraryResult<()> {
    if value.chars().count() > max_chars {
        return Err(MusicLibraryError::validation(
            field,
            format!("exceeds the {max_chars} character limit"),
        ));
    }
    Ok(())
}

fn validate_time(value: i64, field: &str) -> MusicLibraryResult<()> {
    if value <= 0 {
        return Err(MusicLibraryError::validation(
            field,
            "must be a positive Unix epoch millisecond value",
        ));
    }
    Ok(())
}

async fn canonical_youtube_item_id(
    transaction: &mut Transaction<'_, Sqlite>,
    video_id: &str,
) -> MusicLibraryResult<String> {
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM music_library_items WHERE youtube_video_id = ?")
            .bind(video_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("match canonical YouTube video", error))?;
    match existing {
        Some(item_id) => Ok(item_id),
        None => {
            crate::identity::creatable_key(
                transaction,
                crate::identity::RetirableTable::Items,
                youtube_item_id(video_id),
            )
            .await
        }
    }
}

fn failure_message(state: MusicYouTubeResolutionState) -> &'static str {
    match state {
        MusicYouTubeResolutionState::Unavailable => {
            "The YouTube source is unavailable or private. The last successful snapshot was preserved."
        }
        MusicYouTubeResolutionState::EmbeddingBlocked => {
            "The owner does not allow this YouTube source in embedded players. The last successful snapshot was preserved."
        }
        MusicYouTubeResolutionState::TimedOut => {
            "YouTube did not finish resolving this source in time. The last successful snapshot was preserved."
        }
        MusicYouTubeResolutionState::Resolving | MusicYouTubeResolutionState::Ready => {
            "The YouTube source did not complete. The last successful snapshot was preserved."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        pool
    }

    fn snapshot(video_ids: Vec<&str>) -> MusicYouTubePlaylistSnapshotWrite {
        MusicYouTubePlaylistSnapshotWrite {
            playlist_id: "PLabcdef12345".to_string(),
            name: "Focus soundtrack".to_string(),
            video_ids: video_ids.into_iter().map(str::to_string).collect(),
            videos: Vec::new(),
            resolved_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn duplicate_video_count_deduplicates_preview_ids() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            upsert_video(
                &pool,
                MusicYouTubeVideoWrite {
                    video_id: "abcDEF_1234".to_string(),
                    title: "Known video".to_string(),
                    channel: String::new(),
                    duration_ms: None,
                    resolution_state: MusicYouTubeResolutionState::Ready,
                    resolved_at_ms: 1_700_000_000_000,
                },
            )
            .await
            .unwrap();
            assert_eq!(
                duplicate_video_count(
                    &pool,
                    vec![
                        "abcDEF_1234".to_string(),
                        "abcDEF_1234".to_string(),
                        "xyzABC_5678".to_string(),
                    ],
                )
                .await
                .unwrap(),
                1
            );
        });
    }

    #[test]
    fn direct_video_states_preserve_metadata_and_review_state() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let first = upsert_video(
                &pool,
                MusicYouTubeVideoWrite {
                    video_id: "abcDEF_1234".to_string(),
                    title: "Opening theme".to_string(),
                    channel: "Official channel".to_string(),
                    duration_ms: Some(95_000),
                    resolution_state: MusicYouTubeResolutionState::Ready,
                    resolved_at_ms: 1_700_000_000_000,
                },
            )
            .await
            .unwrap();
            sqlx::query("UPDATE music_library_items SET review_state = 'reviewed' WHERE id = ?")
                .bind(&first.id)
                .execute(&pool)
                .await
                .unwrap();
            upsert_video(
                &pool,
                MusicYouTubeVideoWrite {
                    video_id: "abcDEF_1234".to_string(),
                    title: String::new(),
                    channel: String::new(),
                    duration_ms: None,
                    resolution_state: MusicYouTubeResolutionState::EmbeddingBlocked,
                    resolved_at_ms: 1_700_000_100_000,
                },
            )
            .await
            .unwrap();
            let stored: (String, String, i64, String, String) = sqlx::query_as(
                "SELECT original_title, original_artist, duration_ms, review_state,
                        youtube_resolution_state FROM music_library_items WHERE id = ?",
            )
            .bind(first.id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(stored.0, "Opening theme");
            assert_eq!(stored.1, "Official channel");
            assert_eq!(stored.2, 95_000);
            assert_eq!(stored.3, "reviewed");
            assert_eq!(stored.4, "embedding-blocked");
        });
    }

    #[test]
    fn playlist_snapshots_deduplicate_reorder_and_preserve_last_success_on_failure() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let first = apply_playlist_snapshot(
                &pool,
                snapshot(vec!["abcDEF_1234", "xyzXYZ_5678", "abcDEF_1234"]),
            )
            .await
            .unwrap();
            assert_eq!(first.canonical_item_count, 2);
            assert_eq!(first.newly_discovered_count, 2);
            assert_eq!(first.repeated_video_count, 1);
            let mut reordered = snapshot(vec!["xyzXYZ_5678", "abcDEF_1234"]);
            reordered.resolved_at_ms += 1;
            let second = apply_playlist_snapshot(&pool, reordered).await.unwrap();
            assert_eq!(second.newly_discovered_count, 0);
            assert_eq!(second.collection_id, first.collection_id);
            assert_eq!(
                first.collection_id,
                collection_id(&youtube_playlist_identity_key("PLabcdef12345"))
            );
            let order: Vec<String> = sqlx::query_scalar(
                "SELECT item.youtube_video_id
                 FROM music_source_collection_items AS source_item
                 JOIN music_library_items AS item ON item.id = source_item.item_id
                 WHERE source_item.collection_id = ?
                   AND source_item.missing_from_latest_snapshot = 0
                 ORDER BY source_item.source_position",
            )
            .bind(&first.collection_id)
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(order, vec!["xyzXYZ_5678", "abcDEF_1234"]);

            let mut removed = snapshot(vec!["abcDEF_1234"]);
            removed.resolved_at_ms += 2;
            apply_playlist_snapshot(&pool, removed).await.unwrap();
            let removed_from_snapshot: i64 = sqlx::query_scalar(
                "SELECT missing_from_latest_snapshot
                 FROM music_source_collection_items AS source_item
                 JOIN music_library_items AS item ON item.id = source_item.item_id
                 WHERE source_item.collection_id = ?
                   AND item.youtube_video_id = 'xyzXYZ_5678'",
            )
            .bind(&first.collection_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(removed_from_snapshot, 1);

            report_source_failure(
                &pool,
                MusicYouTubeSourceFailureWrite {
                    playlist_id: "PLabcdef12345".to_string(),
                    name: "Focus soundtrack".to_string(),
                    resolution_state: MusicYouTubeResolutionState::TimedOut,
                    error_code: "playlist-timeout".to_string(),
                    occurred_at: 1_700_000_200_000,
                },
            )
            .await
            .unwrap();
            let preserved: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM music_source_collection_items
                 WHERE collection_id = ?
                   AND missing_from_latest_snapshot = 0",
            )
            .bind(&first.collection_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(preserved, 1);
        });
    }

    #[test]
    fn playlist_snapshot_saves_resolved_metadata_as_playable() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let mut request = snapshot(vec!["01L4CFQdrWA", "O4iot2Jy_D0"]);
            request.videos.push(MusicYouTubePlaylistVideoWrite {
                video_id: "01L4CFQdrWA".to_string(),
                title: "Endless Embrace".to_string(),
                channel: "MYTH & ROID - Topic".to_string(),
            });
            apply_playlist_snapshot(&pool, request).await.unwrap();

            let resolved: (String, String, String, String) = sqlx::query_as(
                "SELECT original_title, original_artist, availability, youtube_resolution_state
                 FROM music_library_items WHERE youtube_video_id = '01L4CFQdrWA'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(resolved.0, "Endless Embrace");
            assert_eq!(resolved.1, "MYTH & ROID - Topic");
            assert_eq!(resolved.2, "available");
            assert_eq!(resolved.3, "ready");

            let unresolved: (String, String) = sqlx::query_as(
                "SELECT availability, youtube_resolution_state
                 FROM music_library_items WHERE youtube_video_id = 'O4iot2Jy_D0'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(unresolved.0, "unknown");
            assert_eq!(unresolved.1, "resolving");
        });
    }

    #[test]
    fn playlist_refresh_keeps_terminal_states_and_resolved_metadata_flags() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let video = |video_id: &str, title: &str, state| MusicYouTubeVideoWrite {
                video_id: video_id.to_string(),
                title: title.to_string(),
                channel: String::new(),
                duration_ms: None,
                resolution_state: state,
                resolved_at_ms: 1_700_000_000_000,
            };
            upsert_video(
                &pool,
                video("abcDEF_1234", "", MusicYouTubeResolutionState::Unavailable),
            )
            .await
            .unwrap();
            upsert_video(
                &pool,
                video("xyzXYZ_5678", "Known", MusicYouTubeResolutionState::Ready),
            )
            .await
            .unwrap();
            apply_playlist_snapshot(&pool, snapshot(vec!["abcDEF_1234", "xyzXYZ_5678"]))
                .await
                .unwrap();
            let stored = |video_id: &'static str| {
                let pool = pool.clone();
                async move {
                    sqlx::query_as::<_, (String, bool)>(
                        "SELECT youtube_resolution_state, metadata_resolved
                         FROM music_library_items WHERE youtube_video_id = ?",
                    )
                    .bind(video_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap()
                }
            };
            assert_eq!(
                stored("abcDEF_1234").await,
                ("unavailable".to_string(), false)
            );
            assert_eq!(stored("xyzXYZ_5678").await, ("ready".to_string(), true));

            // Playback asks to resolve again; only an explicit request leaves a terminal state.
            upsert_video(
                &pool,
                video("abcDEF_1234", "", MusicYouTubeResolutionState::Resolving),
            )
            .await
            .unwrap();
            assert_eq!(
                stored("abcDEF_1234").await,
                ("resolving".to_string(), false)
            );
            upsert_video(
                &pool,
                video("xyzXYZ_5678", "", MusicYouTubeResolutionState::Resolving),
            )
            .await
            .unwrap();
            assert_eq!(stored("xyzXYZ_5678").await, ("ready".to_string(), true));
        });
    }

    #[test]
    fn youtube_ids_and_failure_states_are_strictly_validated() {
        assert!(validate_video_id("short", "videoId").is_err());
        assert!(validate_playlist_snapshot(&snapshot(vec![])).is_ok());
        let mut invalid = snapshot(vec!["not valid!!"]);
        assert!(validate_playlist_snapshot(&invalid).is_err());
        invalid.video_ids.clear();
        invalid.playlist_id = "bad list".to_string();
        assert!(validate_playlist_snapshot(&invalid).is_err());
    }

    #[test]
    fn confirmed_empty_playlist_snapshot_is_persisted_without_placeholder_items() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let result = apply_playlist_snapshot(&pool, snapshot(vec![]))
                .await
                .unwrap();
            assert_eq!(result.canonical_item_count, 0);
            let item_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM music_library_items")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(item_count, 0);
        });
    }

    #[test]
    fn a_playlist_and_video_added_again_after_published_deletions_take_successor_keys() {
        use ganbaru_sync::manifest::vault::music::{COLLECTIONS, ITEMS};
        crate::test_support::block_on(async {
            let pool = pool().await;
            let first = apply_playlist_snapshot(&pool, snapshot(vec!["abcDEF_1234"]))
                .await
                .unwrap();
            let item = youtube_item_id("abcDEF_1234");
            for (table, key) in [(&COLLECTIONS, &first.collection_id), (&ITEMS, &item)] {
                sqlx::query(
                    "INSERT INTO sync_rows (table_id, row_key, state) VALUES (?, ?, 'live')",
                )
                .bind(i64::from(table.id.0))
                .bind(key)
                .execute(&pool)
                .await
                .unwrap();
            }
            sqlx::raw_sql(
                "DELETE FROM music_source_collections;
                 DELETE FROM music_library_items;",
            )
            .execute(&pool)
            .await
            .unwrap();

            let mut again = snapshot(vec!["abcDEF_1234"]);
            again.resolved_at_ms += 1;
            let second = apply_playlist_snapshot(&pool, again).await.unwrap();
            assert_eq!(second.collection_id, format!("{}-2", first.collection_id));
            let items: Vec<String> = sqlx::query_scalar("SELECT id FROM music_library_items")
                .fetch_all(&pool)
                .await
                .unwrap();
            assert_eq!(items, [format!("{item}-2")]);
        });
    }
}
