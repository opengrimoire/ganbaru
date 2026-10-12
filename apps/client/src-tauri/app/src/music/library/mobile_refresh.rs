//! Android Storage Access Framework refresh into the shared canonical music library.

use ganbaru_mobile_media::{MobileMediaExt, MobileMediaTreeTrack};
use ganbaru_music_library::{
    MusicCollectionKind, MusicLibraryError, MusicLibraryResult, MusicLocalRefreshRequest,
    MusicRefreshJobProgress, MusicRefreshJobState,
};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};

const MAX_MEDIA_FILES: u32 = 5_000;
const MAX_TREE_DEPTH: u32 = 48;

#[derive(FromRow)]
struct RefreshJobRow {
    id: String,
    source_collection_id: String,
    local_root_id: Option<String>,
    kind: String,
    state: String,
    generation: i64,
    discovered_count: i64,
    processed_count: i64,
    skipped_count: i64,
    issue_count: i64,
    truncated_count: i64,
    absence_determined: i64,
    status_message: String,
    requested_at_ms: i64,
    started_at_ms: Option<i64>,
    finished_at_ms: Option<i64>,
    updated_at_ms: i64,
}

impl TryFrom<RefreshJobRow> for MusicRefreshJobProgress {
    type Error = MusicLibraryError;

    fn try_from(row: RefreshJobRow) -> MusicLibraryResult<Self> {
        Ok(Self {
            job_id: row.id,
            collection_id: row.source_collection_id,
            root_id: row.local_root_id,
            kind: MusicCollectionKind::try_from(row.kind.as_str())
                .map_err(|message| MusicLibraryError::validation("kind", message))?,
            state: MusicRefreshJobState::try_from(row.state.as_str())
                .map_err(|message| MusicLibraryError::validation("state", message))?,
            generation: row.generation,
            discovered_count: row.discovered_count,
            processed_count: row.processed_count,
            skipped_count: row.skipped_count,
            issue_count: row.issue_count,
            truncated_count: row.truncated_count,
            absence_determined: row.absence_determined == 1,
            status_message: row.status_message,
            requested_at_ms: row.requested_at_ms,
            started_at_ms: row.started_at_ms,
            finished_at_ms: row.finished_at_ms,
            updated_at_ms: row.updated_at_ms,
        })
    }
}

pub(super) async fn start(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    request: MusicLocalRefreshRequest,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    validate_request(&request)?;
    let tree = app
        .mobile_media()
        .scan_media_tree(&request.folder_path, MAX_MEDIA_FILES, MAX_TREE_DEPTH)
        .await
        .map_err(|error| MusicLibraryError::runtime("scan selected Android music folder", error))?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin Android music refresh", error))?;

    let collection_root: Option<(Option<String>, i64)> = sqlx::query_as(
        "SELECT local_root_id, discovery_enabled FROM music_source_collections
         WHERE id = ? AND kind = 'local-root' AND removed_at_ms IS NULL",
    )
    .bind(&request.collection_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load Android music source", error))?;
    match collection_root {
        Some((Some(root_id), 1)) if root_id == request.root_id => {}
        Some((_, 0)) => {
            return Err(MusicLibraryError::conflict(
                "discovery is disabled for this local music source",
            ));
        }
        _ => {
            return Err(MusicLibraryError::not_found(
                "local source collection",
                &request.collection_id,
            ));
        }
    }

    let generation = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(candidate), 0) FROM (
            SELECT snapshot_generation AS candidate FROM music_source_collections WHERE id = ?
            UNION ALL
            SELECT generation AS candidate FROM music_refresh_jobs WHERE source_collection_id = ?
         )",
    )
    .bind(&request.collection_id)
    .bind(&request.collection_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("allocate Android music generation", error))?
        + 1;

    sqlx::query(
        "UPDATE music_refresh_jobs
         SET state = 'cancelled', status_message = 'Superseded by a newer refresh.',
             finished_at_ms = ?, updated_at_ms = ?
         WHERE source_collection_id = ? AND state IN ('queued', 'running')",
    )
    .bind(request.requested_at_ms)
    .bind(request.requested_at_ms)
    .bind(&request.collection_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("cancel stale Android music refresh", error))?;

    let job_state = if tree.truncated {
        "partial"
    } else {
        "completed"
    };
    let source_state = if tree.truncated { "partial" } else { "idle" };
    let status_message = if tree.truncated {
        "The selected folder was partially indexed because it reached the scan limit."
    } else {
        "Music folder is up to date."
    };
    let track_count = i64::try_from(tree.tracks.len()).unwrap_or(i64::MAX);
    sqlx::query(
        "INSERT INTO music_refresh_jobs
            (id, source_collection_id, local_root_id, kind, state, generation,
             discovered_count, processed_count, truncated_count, absence_determined,
             status_message, requested_at_ms, started_at_ms, finished_at_ms, updated_at_ms)
         VALUES (?, ?, ?, 'local-root', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&request.job_id)
    .bind(&request.collection_id)
    .bind(&request.root_id)
    .bind(job_state)
    .bind(generation)
    .bind(track_count)
    .bind(track_count)
    .bind(if tree.truncated { 1_i64 } else { 0_i64 })
    .bind(if tree.truncated { 0_i64 } else { 1_i64 })
    .bind(status_message)
    .bind(request.requested_at_ms)
    .bind(request.requested_at_ms)
    .bind(request.requested_at_ms)
    .bind(request.requested_at_ms)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("record Android music refresh", error))?;

    if !tree.truncated {
        sqlx::query(
            "UPDATE music_local_locations SET availability = 'missing', updated_at_ms = ?
             WHERE root_id = ?",
        )
        .bind(request.requested_at_ms)
        .bind(&request.root_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            MusicLibraryError::database("mark stale Android music locations", error)
        })?;
        sqlx::query(
            "UPDATE music_source_collection_items SET missing_from_latest_snapshot = 1
             WHERE collection_id = ?",
        )
        .bind(&request.collection_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("mark stale Android music items", error))?;
    }

    for (position, track) in tree.tracks.iter().enumerate() {
        persist_track(
            &mut transaction,
            &request,
            generation,
            i64::try_from(position).unwrap_or(i64::MAX),
            track,
        )
        .await?;
    }

    sqlx::query(
        "UPDATE music_library_items
         SET availability = CASE
           WHEN EXISTS (
             SELECT 1 FROM music_local_locations AS location
             WHERE location.item_id = music_library_items.id AND location.availability = 'available'
           ) THEN 'available' ELSE 'missing' END,
           updated_at_ms = ?
         WHERE source_kind = 'local-file'",
    )
    .bind(request.requested_at_ms)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("reconcile Android music availability", error))?;

    sqlx::query(
        "UPDATE music_source_collections
         SET refresh_state = ?, previous_successful_refresh_at_ms = last_successful_refresh_at_ms,
             last_successful_refresh_at_ms = ?, last_refresh_error_code = NULL,
             snapshot_generation = ?, updated_at_ms = ?, version = version + 1
         WHERE id = ?",
    )
    .bind(source_state)
    .bind(request.requested_at_ms)
    .bind(generation)
    .bind(request.requested_at_ms)
    .bind(&request.collection_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("finish Android music source refresh", error))?;

    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit Android music refresh", error))?;
    ganbaru_music_library::search::rebuild(pool, request.requested_at_ms).await?;
    progress(pool, &request.job_id).await
}

async fn persist_track(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    request: &MusicLocalRefreshRequest,
    generation: i64,
    position: i64,
    track: &MobileMediaTreeTrack,
) -> MusicLibraryResult<()> {
    validate_relative_path(&track.relative_path)?;
    let item_id = stable_id("item", &[&request.root_id, &track.relative_path]);
    let location_id = stable_id("location", &[&request.root_id, &track.relative_path]);
    let media_kind = match track.media_kind.as_str() {
        "audio" => "audio",
        "video" => "video",
        _ => "unknown",
    };
    let artwork_identity = if track.embedded_artwork_candidate {
        Some(format!(
            "embedded:saf:{}:{}:{}",
            track.relative_path,
            track.file_size_bytes.unwrap_or_default(),
            track.modified_at_ms.unwrap_or_default()
        ))
    } else {
        track
            .artwork_uri
            .as_ref()
            .map(|_| format!("sidecar:{}", track.relative_path))
    };
    sqlx::query(
        "INSERT INTO music_library_items
            (id, identity_key, source_kind, media_kind, original_title, original_artist,
             original_album, original_track_number, original_artwork_identity, duration_ms,
             availability, discovered_at_ms, updated_at_ms)
         VALUES (?, ?, 'local-file', ?, ?, ?, ?, ?, ?, ?, 'available', ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            media_kind = excluded.media_kind,
            original_title = excluded.original_title,
            original_artist = excluded.original_artist,
            original_album = excluded.original_album,
            original_track_number = excluded.original_track_number,
            original_artwork_identity = excluded.original_artwork_identity,
            duration_ms = excluded.duration_ms,
            availability = 'available', updated_at_ms = excluded.updated_at_ms,
            version = music_library_items.version + 1",
    )
    .bind(&item_id)
    .bind(format!("local:{item_id}"))
    .bind(media_kind)
    .bind(track.title.trim())
    .bind(track.artist.trim())
    .bind(track.album.trim())
    .bind(track.track_number)
    .bind(artwork_identity)
    .bind(track.duration_ms)
    .bind(request.requested_at_ms)
    .bind(request.requested_at_ms)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save Android music item", error))?;

    let lightweight_fingerprint = format!(
        "saf:{}:{}",
        track.file_size_bytes.unwrap_or_default(),
        track.modified_at_ms.unwrap_or_default(),
    );
    sqlx::query(
        "INSERT INTO music_local_locations
            (id, item_id, root_id, relative_path, file_size_bytes, modified_at_ms,
             lightweight_fingerprint, availability, last_seen_generation, first_seen_at_ms, updated_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'available', ?, ?, ?)
         ON CONFLICT(root_id, relative_path) DO UPDATE SET
            item_id = excluded.item_id, file_size_bytes = excluded.file_size_bytes,
            modified_at_ms = excluded.modified_at_ms,
            lightweight_fingerprint = excluded.lightweight_fingerprint,
            availability = 'available', last_seen_generation = excluded.last_seen_generation,
            updated_at_ms = excluded.updated_at_ms",
    )
    .bind(location_id)
    .bind(&item_id)
    .bind(&request.root_id)
    .bind(&track.relative_path)
    .bind(track.file_size_bytes)
    .bind(track.modified_at_ms)
    .bind(lightweight_fingerprint)
    .bind(generation)
    .bind(request.requested_at_ms)
    .bind(request.requested_at_ms)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save Android music location", error))?;

    sqlx::query(
        "INSERT INTO music_source_collection_items
            (collection_id, item_id, source_position, first_discovered_at_ms,
             last_seen_generation, missing_from_latest_snapshot)
         VALUES (?, ?, ?, ?, ?, 0)
         ON CONFLICT(collection_id, item_id) DO UPDATE SET
            source_position = excluded.source_position,
            last_seen_generation = excluded.last_seen_generation,
            missing_from_latest_snapshot = 0",
    )
    .bind(&request.collection_id)
    .bind(&item_id)
    .bind(position)
    .bind(request.requested_at_ms)
    .bind(generation)
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("link Android music item", error))?;
    Ok(())
}

pub(super) async fn progress(
    pool: &SqlitePool,
    job_id: &str,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    let row = sqlx::query_as::<_, RefreshJobRow>(
        "SELECT id, source_collection_id, local_root_id, kind, state, generation,
                discovered_count, processed_count, skipped_count, issue_count,
                truncated_count, absence_determined, status_message, requested_at_ms,
                started_at_ms, finished_at_ms, updated_at_ms
         FROM music_refresh_jobs WHERE id = ?",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load Android music refresh", error))?
    .ok_or_else(|| MusicLibraryError::not_found("music refresh job", job_id))?;
    row.try_into()
}

pub(super) async fn cancel(
    pool: &SqlitePool,
    job_id: &str,
    _cancelled_at: i64,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    progress(pool, job_id).await
}

fn validate_request(request: &MusicLocalRefreshRequest) -> MusicLibraryResult<()> {
    for (field, value) in [
        ("jobId", request.job_id.as_str()),
        ("rootId", request.root_id.as_str()),
        ("collectionId", request.collection_id.as_str()),
        ("folderPath", request.folder_path.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(MusicLibraryError::validation(field, "is required"));
        }
    }
    if request.requested_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "requestedAt",
            "must be a positive Unix epoch millisecond value",
        ));
    }
    if !request.folder_path.starts_with("content://") {
        return Err(MusicLibraryError::validation(
            "folderPath",
            "must be an Android document-tree URI",
        ));
    }
    if !request.available_roots.iter().any(|binding| {
        binding.root_id == request.root_id && binding.folder_path == request.folder_path
    }) {
        return Err(MusicLibraryError::validation(
            "availableRoots",
            "must include the selected Android music folder",
        ));
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> MusicLibraryResult<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(MusicLibraryError::validation(
            "relativePath",
            "contains an unsafe Android document path",
        ));
    }
    Ok(())
}

fn stable_id(kind: &str, values: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    for value in values {
        hasher.update([0]);
        hasher.update(value.as_bytes());
    }
    format!("music-{kind}-{:x}", hasher.finalize())
}
