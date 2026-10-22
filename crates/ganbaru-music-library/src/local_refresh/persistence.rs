use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::path::Path;

use super::super::*;
use super::helpers::{map_conflict, new_item_id, now_ms, validate_request};
use super::traversal::LocalMediaEvidence;
use crate::identity::stable_id;
use crate::local_scan::{self, LocalScanValues};

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
             version = version + 1 WHERE id = ?",
    )
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

/// Records media whose stored size and modification time match this device's observation and
/// returns the paths that still need inspection.
pub(super) async fn record_unchanged_batch(
    pool: &SqlitePool,
    request: &MusicLocalRefreshRequest,
    generation: i64,
    observed: Vec<(String, Option<super::traversal::MediaStat>)>,
) -> MusicLibraryResult<Vec<String>> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin unchanged music batch", error))?;
    let mut changed = Vec::new();
    let mut unchanged_count = 0_i64;
    for (relative_path, stat) in observed {
        let stored =
            local_scan::load_location_stat(&mut transaction, &request.root_id, &relative_path)
                .await?;
        let unchanged = match (stored, stat) {
            (Some(stored), Some(stat)) => {
                stored.matches(Some(stat.file_size_bytes), stat.modified_at_ms)
            }
            _ => false,
        };
        if !unchanged
            || !local_scan::record_unchanged_location(
                &mut transaction,
                &request.collection_id,
                &request.root_id,
                &relative_path,
                generation,
                None,
                request.requested_at_ms,
            )
            .await?
        {
            changed.push(relative_path);
            continue;
        }
        sqlx::query(
            "UPDATE music_refresh_job_entries SET state = 'processed'
             WHERE job_id = ? AND relative_path = ? AND entry_kind = 'media'",
        )
        .bind(&request.job_id)
        .bind(&relative_path)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("finish unchanged media entry", error))?;
        unchanged_count += 1;
    }
    if unchanged_count > 0 {
        sqlx::query(
            "UPDATE music_refresh_jobs
             SET processed_count = processed_count + ?,
                 status_message = 'Cataloging discovered media.', updated_at_ms = ?
             WHERE id = ? AND state = 'running'",
        )
        .bind(unchanged_count)
        .bind(now_ms())
        .bind(&request.job_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("update music catalog progress", error))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit unchanged music batch", error))?;
    Ok(changed)
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
        let values = scan_values(media, strong_hash.as_deref());
        let availability = if ambiguous { "ambiguous" } else { "available" };
        let updated_at_ms = now_ms();
        local_scan::upsert_scanned_item(
            &mut transaction,
            &item_id,
            &values,
            availability,
            request.requested_at_ms,
            updated_at_ms,
        )
        .await?;
        local_scan::upsert_scanned_location(
            &mut transaction,
            &request.root_id,
            &item_id,
            &values,
            availability,
            generation,
            request.requested_at_ms,
            updated_at_ms,
        )
        .await?;
        local_scan::link_collection_item(
            &mut transaction,
            &request.collection_id,
            &item_id,
            None,
            generation,
            request.requested_at_ms,
        )
        .await?;
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
    local_scan::mark_unseen(
        &mut transaction,
        &request.collection_id,
        &request.root_id,
        generation,
    )
    .await?;
    local_scan::reconcile_item_availability(&mut transaction, &request.root_id).await?;
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
             version = version + 1 WHERE id = ?",
    )
    .bind(finished_at_ms)
    .bind(generation)
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
             version = version + 1
         WHERE id = ? AND NOT EXISTS (
            SELECT 1 FROM music_refresh_jobs AS newer
            WHERE newer.source_collection_id = ? AND newer.generation > ?
         )",
    )
    .bind(source_state)
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
                "DELETE FROM music_local_locations WHERE root_id = ? AND relative_path = ?",
            )
            .bind(&request.root_id)
            .bind(stored_path)
            .execute(&mut **transaction)
            .await
            .map_err(|error| {
                MusicLibraryError::database("replace unnormalized music location", error)
            })?;
        }
        if keeper.as_ref().is_some_and(|(root_id, relative_path)| {
            root_id == &request.root_id && relative_path.replace('\\', "/") == media.relative_path
        }) {
            return Ok((item_id, None, false));
        }
    }
    Ok((new_item_id(request, media), None, false))
}

fn scan_values<'a>(
    media: &'a LocalMediaEvidence,
    strong_fingerprint: Option<&'a str>,
) -> LocalScanValues<'a> {
    LocalScanValues {
        relative_path: &media.relative_path,
        media_kind: media.media_kind.as_ref(),
        title: &media.title,
        artist: &media.artist,
        album: &media.album,
        track_number: media.track_number,
        duration_ms: media.duration_ms,
        artwork_identity: media.original_artwork_identity.as_deref(),
        file_size_bytes: Some(media.file_size_bytes),
        modified_at_ms: media.modified_at_ms,
        lightweight_fingerprint: &media.lightweight_fingerprint,
        strong_fingerprint,
    }
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
