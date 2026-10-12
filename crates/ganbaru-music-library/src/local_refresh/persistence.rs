use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::path::Path;

use super::super::*;
use super::helpers::{map_conflict, new_item_id, now_ms, stable_id, validate_request};
use super::traversal::LocalMediaEvidence;

#[derive(Debug, sqlx::FromRow)]
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
            absence_determined: match row.absence_determined {
                0 => false,
                1 => true,
                value => {
                    return Err(MusicLibraryError::validation(
                        "absenceDetermined",
                        format!("expected 0 or 1, received {value}"),
                    ));
                }
            },
            status_message: row.status_message,
            requested_at_ms: row.requested_at_ms,
            started_at_ms: row.started_at_ms,
            finished_at_ms: row.finished_at_ms,
            updated_at_ms: row.updated_at_ms,
        })
    }
}

pub(super) async fn prepare(
    pool: &SqlitePool,
    request: &MusicLocalRefreshRequest,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    validate_request(request)?;
    let folder = Path::new(&request.folder_path);
    let metadata = std::fs::metadata(folder)
        .map_err(|error| MusicLibraryError::runtime("inspect local music root", error))?;
    if !metadata.is_dir() {
        return Err(MusicLibraryError::validation(
            "folderPath",
            "must reference an existing directory",
        ));
    }

    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin local music refresh", error))?;
    let collection_root: Option<(Option<String>, i64)> = sqlx::query_as(
        "SELECT local_root_id, discovery_enabled FROM music_source_collections
         WHERE id = ? AND kind = 'local-root'",
    )
    .bind(&request.collection_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load local source collection", error))?;
    match collection_root {
        Some((Some(root_id), 1)) if root_id == request.root_id => {}
        Some((_, 0)) => {
            return Err(MusicLibraryError::conflict(
                "discovery is disabled for this local music source",
            ));
        }
        Some((Some(_), 1)) => {
            return Err(MusicLibraryError::conflict(
                "the source collection belongs to a different logical root",
            ));
        }
        Some(_) | None => {
            return Err(MusicLibraryError::not_found(
                "local source collection",
                &request.collection_id,
            ));
        }
    }
    let generation: i64 = sqlx::query_scalar(
        "SELECT MAX(candidate) FROM (
            SELECT snapshot_generation AS candidate FROM music_source_collections WHERE id = ?
            UNION ALL
            SELECT generation AS candidate FROM music_refresh_jobs WHERE source_collection_id = ?
         )",
    )
    .bind(&request.collection_id)
    .bind(&request.collection_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("allocate music refresh generation", error))?;
    let generation = generation + 1;

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
    .map_err(|error| MusicLibraryError::database("cancel stale music refresh", error))?;
    sqlx::query(
        "INSERT INTO music_refresh_jobs
            (id, source_collection_id, local_root_id, kind, state, generation,
             status_message, requested_at_ms, updated_at_ms)
         VALUES (?, ?, ?, 'local-root', 'queued', ?, 'Waiting to scan.', ?, ?)",
    )
    .bind(&request.job_id)
    .bind(&request.collection_id)
    .bind(&request.root_id)
    .bind(generation)
    .bind(request.requested_at_ms)
    .bind(request.requested_at_ms)
    .execute(&mut *transaction)
    .await
    .map_err(|error| map_conflict("create local music refresh", error))?;
    sqlx::query(
        "INSERT INTO music_refresh_job_entries (job_id, relative_path, entry_kind)
         VALUES (?, '', 'directory')",
    )
    .bind(&request.job_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("seed music refresh traversal", error))?;
    sqlx::query(
        "UPDATE music_source_collections
         SET refresh_state = 'queued', last_refresh_error_code = NULL,
             updated_at_ms = ?, version = version + 1 WHERE id = ?",
    )
    .bind(request.requested_at_ms)
    .bind(&request.collection_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("queue local source refresh", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit local refresh queue", error))?;
    load_progress(pool, &request.job_id).await
}

pub(super) async fn mark_running(pool: &SqlitePool, job_id: &str) -> MusicLibraryResult<()> {
    let now = now_ms();
    let result = sqlx::query(
        "UPDATE music_refresh_jobs
         SET state = 'running', started_at_ms = COALESCE(started_at_ms, ?),
             status_message = 'Discovering media files.', updated_at_ms = ?
         WHERE id = ? AND state = 'queued'",
    )
    .bind(now)
    .bind(now)
    .bind(job_id)
    .execute(pool)
    .await
    .map_err(|error| MusicLibraryError::database("start local music refresh", error))?;
    if result.rows_affected() == 0 {
        return Err(MusicLibraryError::conflict(
            "the local music refresh is no longer queued",
        ));
    }
    Ok(())
}

pub(super) async fn load_progress(
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
    .map_err(|error| MusicLibraryError::database("load music refresh progress", error))?
    .ok_or_else(|| MusicLibraryError::not_found("music refresh job", job_id))?;
    row.try_into()
}

pub(super) async fn cancel(
    pool: &SqlitePool,
    job_id: &str,
    cancelled_at: i64,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    if cancelled_at <= 0 {
        return Err(MusicLibraryError::validation(
            "cancelledAt",
            "must be a positive Unix epoch millisecond value",
        ));
    }
    let result = sqlx::query(
        "UPDATE music_refresh_jobs
         SET state = 'cancelled', status_message = 'Refresh cancelled. Existing availability was preserved.',
             finished_at_ms = ?, updated_at_ms = ?
         WHERE id = ? AND state IN ('queued', 'running')",
    )
    .bind(cancelled_at)
    .bind(cancelled_at)
    .bind(job_id)
    .execute(pool)
    .await
    .map_err(|error| MusicLibraryError::database("cancel music refresh", error))?;
    if result.rows_affected() == 0 {
        let progress = load_progress(pool, job_id).await?;
        if matches!(
            progress.state,
            MusicRefreshJobState::Completed
                | MusicRefreshJobState::Failed
                | MusicRefreshJobState::Partial
                | MusicRefreshJobState::Cancelled
        ) {
            return Ok(progress);
        }
        return Err(MusicLibraryError::conflict(
            "the music refresh could not be cancelled",
        ));
    }
    load_progress(pool, job_id).await
}

pub(super) async fn is_current(pool: &SqlitePool, job_id: &str) -> MusicLibraryResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM music_refresh_jobs AS job
            WHERE job.id = ? AND job.state = 'running'
              AND NOT EXISTS (
                SELECT 1 FROM music_refresh_jobs AS newer
                WHERE newer.source_collection_id = job.source_collection_id
                  AND newer.generation > job.generation
              )
         )",
    )
    .bind(job_id)
    .fetch_one(pool)
    .await
    .map_err(|error| MusicLibraryError::database("check music refresh generation", error))
}

pub(super) async fn next_pending_directory(
    pool: &SqlitePool,
    job_id: &str,
) -> MusicLibraryResult<Option<String>> {
    sqlx::query_scalar(
        "SELECT relative_path FROM music_refresh_job_entries
         WHERE job_id = ? AND entry_kind = 'directory' AND state = 'pending'
         ORDER BY relative_path LIMIT 1",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load pending music directory", error))
}

pub(super) async fn save_discovery_batch(
    pool: &SqlitePool,
    job_id: &str,
    directory: &str,
    entries: &[super::traversal::DiscoveredEntry],
    directory_complete: bool,
) -> MusicLibraryResult<()> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin music discovery batch", error))?;
    let mut discovered = 0_i64;
    let mut skipped = 0_i64;
    for entry in entries {
        match entry.kind {
            super::traversal::DiscoveredEntryKind::Directory => {
                sqlx::query(
                    "INSERT OR IGNORE INTO music_refresh_job_entries
                        (job_id, relative_path, entry_kind) VALUES (?, ?, 'directory')",
                )
                .bind(job_id)
                .bind(&entry.relative_path)
                .execute(&mut *transaction)
                .await
                .map_err(|error| MusicLibraryError::database("queue music directory", error))?;
            }
            super::traversal::DiscoveredEntryKind::Media => {
                let result = sqlx::query(
                    "INSERT OR IGNORE INTO music_refresh_job_entries
                        (job_id, relative_path, entry_kind) VALUES (?, ?, 'media')",
                )
                .bind(job_id)
                .bind(&entry.relative_path)
                .execute(&mut *transaction)
                .await
                .map_err(|error| MusicLibraryError::database("queue discovered media", error))?;
                discovered += i64::try_from(result.rows_affected()).unwrap_or(0);
            }
            super::traversal::DiscoveredEntryKind::Skipped => skipped += 1,
        }
    }
    if directory_complete {
        sqlx::query(
            "UPDATE music_refresh_job_entries SET state = 'processed'
             WHERE job_id = ? AND relative_path = ? AND entry_kind = 'directory'",
        )
        .bind(job_id)
        .bind(directory)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("finish discovered directory", error))?;
    }
    sqlx::query(
        "UPDATE music_refresh_jobs
         SET discovered_count = discovered_count + ?, skipped_count = skipped_count + ?,
             status_message = 'Discovering media files.', updated_at_ms = ?
         WHERE id = ? AND state = 'running'",
    )
    .bind(discovered)
    .bind(skipped)
    .bind(now_ms())
    .bind(job_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("update music discovery progress", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit music discovery batch", error))
}

pub(super) async fn next_media_batch(
    pool: &SqlitePool,
    job_id: &str,
    limit: i64,
) -> MusicLibraryResult<Vec<String>> {
    sqlx::query_scalar(
        "SELECT relative_path FROM music_refresh_job_entries
         WHERE job_id = ? AND entry_kind = 'media' AND state = 'pending'
         ORDER BY relative_path LIMIT ?",
    )
    .bind(job_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load pending media batch", error))
}

pub(super) async fn reconcile_batch(
    pool: &SqlitePool,
    request: &MusicLocalRefreshRequest,
    generation: i64,
    evidence: &[LocalMediaEvidence],
) -> MusicLibraryResult<()> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin music reconciliation batch", error))?;
    let mut issue_count = 0_i64;
    let mut refreshed_item_ids = Vec::with_capacity(evidence.len());
    for media in evidence {
        let (item_id, strong_hash, ambiguous) =
            resolve_item_identity(&mut transaction, request, media).await?;
        upsert_item(
            &mut transaction,
            &item_id,
            media,
            request.requested_at_ms,
            ambiguous,
        )
        .await?;
        upsert_location(
            &mut transaction,
            request,
            generation,
            &item_id,
            media,
            strong_hash.as_deref(),
            ambiguous,
        )
        .await?;
        sqlx::query(
            "INSERT INTO music_source_collection_items
                (collection_id, item_id, first_discovered_at_ms, last_seen_generation,
                 missing_from_latest_snapshot)
             VALUES (?, ?, ?, ?, 0)
             ON CONFLICT(collection_id, item_id) DO UPDATE SET
                last_seen_generation = excluded.last_seen_generation,
                missing_from_latest_snapshot = 0",
        )
        .bind(&request.collection_id)
        .bind(&item_id)
        .bind(request.requested_at_ms)
        .bind(generation)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save local source provenance", error))?;
        sqlx::query(
            "UPDATE music_refresh_job_entries SET state = 'processed'
             WHERE job_id = ? AND relative_path = ? AND entry_kind = 'media'",
        )
        .bind(&request.job_id)
        .bind(&media.relative_path)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("finish media refresh entry", error))?;
        if ambiguous {
            issue_count += 1;
            insert_issue(
                &mut transaction,
                request,
                "ambiguous-identity",
                Some(&media.relative_path),
                Some(&item_id),
                "Several catalog items share the same lightweight evidence. The new location was kept separate for review.",
            )
            .await?;
        }
        if let Some(message) = &media.metadata_issue {
            issue_count += 1;
            insert_issue(
                &mut transaction,
                request,
                "metadata-fallback",
                Some(&media.relative_path),
                Some(&item_id),
                message,
            )
            .await?;
        }
        refreshed_item_ids.push(item_id);
    }
    super::super::search::refresh_items(&mut transaction, &refreshed_item_ids).await?;
    sqlx::query(
        "UPDATE music_refresh_jobs
         SET processed_count = processed_count + ?, issue_count = issue_count + ?,
             status_message = 'Cataloging discovered media.', updated_at_ms = ?
         WHERE id = ? AND state = 'running'",
    )
    .bind(evidence.len() as i64)
    .bind(issue_count)
    .bind(now_ms())
    .bind(&request.job_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("update music catalog progress", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit music reconciliation batch", error))
}

pub(super) async fn record_media_failure(
    pool: &SqlitePool,
    request: &MusicLocalRefreshRequest,
    relative_path: &str,
    message: &str,
) -> MusicLibraryResult<()> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin music refresh issue", error))?;
    sqlx::query(
        "UPDATE music_refresh_job_entries SET state = 'skipped'
         WHERE job_id = ? AND relative_path = ? AND entry_kind = 'media'",
    )
    .bind(&request.job_id)
    .bind(relative_path)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("skip unavailable media entry", error))?;
    insert_issue(
        &mut transaction,
        request,
        "media-inspection-failed",
        Some(relative_path),
        None,
        message,
    )
    .await?;
    sqlx::query(
        "UPDATE music_refresh_jobs
         SET skipped_count = skipped_count + 1, issue_count = issue_count + 1,
             updated_at_ms = ? WHERE id = ? AND state = 'running'",
    )
    .bind(now_ms())
    .bind(&request.job_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("update failed media progress", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit media refresh issue", error))
}

pub(super) async fn finalize_success(
    pool: &SqlitePool,
    request: &MusicLocalRefreshRequest,
    generation: i64,
) -> MusicLibraryResult<()> {
    let finished_at_ms = now_ms();
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin complete music refresh", error))?;
    sqlx::query(
        "UPDATE music_local_locations
         SET availability = 'missing', updated_at_ms = ?
         WHERE root_id = ? AND COALESCE(last_seen_generation, 0) < ?",
    )
    .bind(finished_at_ms)
    .bind(&request.root_id)
    .bind(generation)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("mark absent local music", error))?;
    sqlx::query(
        "UPDATE music_source_collection_items
         SET missing_from_latest_snapshot = 1
         WHERE collection_id = ? AND last_seen_generation < ?",
    )
    .bind(&request.collection_id)
    .bind(generation)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("mark absent source provenance", error))?;
    sqlx::query(
        "UPDATE music_library_items AS item
         SET availability = CASE
             WHEN EXISTS (
                SELECT 1 FROM music_local_locations AS location
                WHERE location.item_id = item.id AND location.availability = 'available'
             ) THEN 'available'
             WHEN EXISTS (
                SELECT 1 FROM music_local_locations AS location
                WHERE location.item_id = item.id AND location.availability = 'ambiguous'
             ) THEN 'ambiguous'
             ELSE 'missing'
         END,
         updated_at_ms = ?, version = version + 1
         WHERE item.source_kind = 'local-file'
           AND EXISTS (
                SELECT 1 FROM music_local_locations AS location
                WHERE location.item_id = item.id AND location.root_id = ?
           )",
    )
    .bind(finished_at_ms)
    .bind(&request.root_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("reconcile local item availability", error))?;
    sqlx::query(
        "UPDATE music_refresh_jobs
         SET state = 'completed', absence_determined = 1,
             status_message = 'Refresh complete.', finished_at_ms = ?, updated_at_ms = ?
         WHERE id = ? AND state = 'running'",
    )
    .bind(finished_at_ms)
    .bind(finished_at_ms)
    .bind(&request.job_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("complete local music refresh", error))?;
    sqlx::query(
        "UPDATE music_source_collections
         SET refresh_state = 'idle', last_successful_refresh_at_ms = ?,
             previous_successful_refresh_at_ms = last_successful_refresh_at_ms,
             last_refresh_error_code = NULL, snapshot_generation = ?,
             updated_at_ms = ?, version = version + 1 WHERE id = ?",
    )
    .bind(finished_at_ms)
    .bind(generation)
    .bind(finished_at_ms)
    .bind(&request.collection_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("complete local source refresh", error))?;
    sqlx::query("DELETE FROM music_refresh_job_entries WHERE job_id = ?")
        .bind(&request.job_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("clear music refresh staging", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit completed music refresh", error))
}

pub(super) async fn finish_incomplete(
    pool: &SqlitePool,
    request: &MusicLocalRefreshRequest,
    message: &str,
) -> MusicLibraryResult<()> {
    let finished_at_ms = now_ms();
    let progress = load_progress(pool, &request.job_id).await?;
    if progress.state == MusicRefreshJobState::Cancelled {
        return Ok(());
    }
    let state = if progress.processed_count > 0 || progress.discovered_count > 0 {
        "partial"
    } else {
        "failed"
    };
    let source_state = if state == "partial" {
        "partial"
    } else {
        "failed"
    };
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin incomplete music refresh", error))?;
    insert_issue(
        &mut transaction,
        request,
        "refresh-incomplete",
        None,
        None,
        message,
    )
    .await?;
    sqlx::query(
        "UPDATE music_refresh_jobs
         SET state = ?, issue_count = issue_count + 1, absence_determined = 0,
             status_message = ?, finished_at_ms = ?, updated_at_ms = ?
         WHERE id = ? AND state = 'running'",
    )
    .bind(state)
    .bind(message)
    .bind(finished_at_ms)
    .bind(finished_at_ms)
    .bind(&request.job_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("finish incomplete music refresh", error))?;
    sqlx::query(
        "UPDATE music_source_collections
         SET refresh_state = ?, last_refresh_error_code = 'refresh-incomplete',
             updated_at_ms = ?, version = version + 1
         WHERE id = ? AND NOT EXISTS (
            SELECT 1 FROM music_refresh_jobs AS newer
            WHERE newer.source_collection_id = ? AND newer.generation > ?
         )",
    )
    .bind(source_state)
    .bind(finished_at_ms)
    .bind(&request.collection_id)
    .bind(&request.collection_id)
    .bind(progress.generation)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("mark local source incomplete", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit incomplete music refresh", error))
}

async fn resolve_item_identity(
    transaction: &mut Transaction<'_, Sqlite>,
    request: &MusicLocalRefreshRequest,
    media: &LocalMediaEvidence,
) -> MusicLibraryResult<(String, Option<String>, bool)> {
    if let Some(location) = sqlx::query(
        "SELECT item_id, relative_path FROM music_local_locations
         WHERE root_id = ? AND REPLACE(relative_path, char(92), '/') = ?
         ORDER BY CASE WHEN relative_path = ? THEN 0 ELSE 1 END
         LIMIT 1",
    )
    .bind(&request.root_id)
    .bind(&media.relative_path)
    .bind(&media.relative_path)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("match exact local music location", error))?
    {
        let item_id: String = location.get("item_id");
        let stored_path: String = location.get("relative_path");
        let keeper: Option<(String, String)> = sqlx::query_as(
            "SELECT root_id, relative_path FROM music_local_locations
             WHERE item_id = ? ORDER BY root_id, relative_path LIMIT 1",
        )
        .bind(&item_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| {
            MusicLibraryError::database("select local music identity keeper", error)
        })?;
        if stored_path != media.relative_path {
            sqlx::query(
                "UPDATE music_local_locations SET relative_path = ?
                 WHERE root_id = ? AND relative_path = ?",
            )
            .bind(&media.relative_path)
            .bind(&request.root_id)
            .bind(stored_path)
            .execute(&mut **transaction)
            .await
            .map_err(|error| map_conflict("normalize local music path", error))?;
        }
        if keeper.as_ref().is_some_and(|(root_id, relative_path)| {
            root_id == &request.root_id && relative_path.replace('\\', "/") == media.relative_path
        }) {
            return Ok((item_id, None, false));
        }
    }
    Ok((new_item_id(request, media), None, false))
}

async fn upsert_item(
    transaction: &mut Transaction<'_, Sqlite>,
    item_id: &str,
    media: &LocalMediaEvidence,
    discovered_at_ms: i64,
    ambiguous: bool,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "INSERT INTO music_library_items
            (id, identity_key, source_kind, media_kind, original_title, original_artist,
             original_album, original_track_number, original_artwork_identity, duration_ms,
             availability, discovered_at_ms, updated_at_ms)
         VALUES (?, ?, 'local-file', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            media_kind = excluded.media_kind,
            original_title = excluded.original_title,
            original_artist = excluded.original_artist,
            original_album = excluded.original_album,
            original_track_number = excluded.original_track_number,
            original_artwork_identity = excluded.original_artwork_identity,
            duration_ms = excluded.duration_ms,
            availability = excluded.availability,
            updated_at_ms = excluded.updated_at_ms,
            version = music_library_items.version + 1",
    )
    .bind(item_id)
    .bind(format!("local:{item_id}"))
    .bind(media.media_kind.as_ref())
    .bind(&media.title)
    .bind(&media.artist)
    .bind(&media.album)
    .bind(media.track_number)
    .bind(&media.original_artwork_identity)
    .bind(media.duration_ms)
    .bind(if ambiguous { "ambiguous" } else { "available" })
    .bind(discovered_at_ms)
    .bind(now_ms())
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_conflict("save discovered music item", error))?;
    Ok(())
}

async fn upsert_location(
    transaction: &mut Transaction<'_, Sqlite>,
    request: &MusicLocalRefreshRequest,
    generation: i64,
    item_id: &str,
    media: &LocalMediaEvidence,
    strong_hash: Option<&str>,
    ambiguous: bool,
) -> MusicLibraryResult<()> {
    let location_id = stable_id("location", &[&request.root_id, &media.relative_path]);
    sqlx::query(
        "INSERT INTO music_local_locations
            (id, item_id, root_id, relative_path, file_size_bytes, modified_at_ms,
             lightweight_fingerprint, strong_fingerprint, availability,
             last_seen_generation, first_seen_at_ms, updated_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(root_id, relative_path) DO UPDATE SET
            item_id = excluded.item_id,
            file_size_bytes = excluded.file_size_bytes,
            modified_at_ms = excluded.modified_at_ms,
            lightweight_fingerprint = excluded.lightweight_fingerprint,
            strong_fingerprint = excluded.strong_fingerprint,
            availability = excluded.availability,
            last_seen_generation = excluded.last_seen_generation,
            updated_at_ms = excluded.updated_at_ms",
    )
    .bind(location_id)
    .bind(item_id)
    .bind(&request.root_id)
    .bind(&media.relative_path)
    .bind(media.file_size_bytes)
    .bind(media.modified_at_ms)
    .bind(&media.lightweight_fingerprint)
    .bind(strong_hash)
    .bind(if ambiguous { "ambiguous" } else { "available" })
    .bind(generation)
    .bind(request.requested_at_ms)
    .bind(now_ms())
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_conflict("save discovered music location", error))?;
    Ok(())
}

async fn insert_issue(
    transaction: &mut Transaction<'_, Sqlite>,
    request: &MusicLocalRefreshRequest,
    code: &str,
    relative_path: Option<&str>,
    item_id: Option<&str>,
    message: &str,
) -> MusicLibraryResult<()> {
    let issue_id = stable_id(
        "refresh-issue",
        &[
            &request.job_id,
            code,
            relative_path.unwrap_or_default(),
            item_id.unwrap_or_default(),
        ],
    );
    sqlx::query(
        "INSERT OR REPLACE INTO music_refresh_job_issues
            (id, job_id, issue_code, relative_path, item_id, message, created_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(issue_id)
    .bind(&request.job_id)
    .bind(code)
    .bind(relative_path)
    .bind(item_id)
    .bind(message)
    .bind(now_ms())
    .execute(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save music refresh issue", error))?;
    Ok(())
}
