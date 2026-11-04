mod helpers;
mod metadata;
mod persistence;
mod traversal;

pub(crate) use traversal::strong_fingerprint as strong_fingerprint_for_repair;
pub(super) use traversal::{
    ArtworkCache as RelinkArtworkCache, LocalMediaEvidence as RelinkMediaEvidence,
    inspect_media as inspect_for_relink,
};

use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

use super::{
    MusicLibraryError, MusicLibraryResult, MusicLocalRefreshRequest, MusicRefreshJobProgress,
};

/// Files and folders one refresh may discover before it stops, so a root chosen at a whole disk by
/// mistake ends with an error instead of staging millions of rows in the vault database.
const MAX_REFRESH_SCAN_ENTRIES: usize = 2_000_000;

pub async fn prepare(
    pool: &SqlitePool,
    request: &MusicLocalRefreshRequest,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    persistence::prepare(pool, request).await
}

pub async fn progress(
    pool: &SqlitePool,
    job_id: &str,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    persistence::load_progress(pool, job_id).await
}

pub async fn cancel(
    pool: &SqlitePool,
    job_id: &str,
    cancelled_at: i64,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    persistence::cancel(pool, job_id, cancelled_at).await
}

pub async fn run_prepared(
    pool: &SqlitePool,
    request: MusicLocalRefreshRequest,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    run_prepared_with_scan_limit(pool, request, MAX_REFRESH_SCAN_ENTRIES).await
}

pub(crate) async fn run_prepared_with_scan_limit(
    pool: &SqlitePool,
    request: MusicLocalRefreshRequest,
    scan_limit: usize,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    persistence::mark_running(pool, &request.job_id).await?;
    let root = PathBuf::from(&request.folder_path);
    let progress = persistence::load_progress(pool, &request.job_id).await?;
    let result = run_current(pool, &request, &root, progress.generation, scan_limit).await;
    if let Err(error) = result {
        persistence::finish_incomplete(pool, &request, &error.message).await?;
    }
    persistence::load_progress(pool, &request.job_id).await
}

async fn run_current(
    pool: &SqlitePool,
    request: &MusicLocalRefreshRequest,
    root: &Path,
    generation: i64,
    scan_limit: usize,
) -> MusicLibraryResult<()> {
    discover(pool, request, root, scan_limit).await?;
    if !persistence::is_current(pool, &request.job_id).await? {
        return Ok(());
    }
    catalog(pool, request, root, generation).await?;
    if !persistence::is_current(pool, &request.job_id).await? {
        return Ok(());
    }
    persistence::finalize_success(pool, request, generation).await
}

async fn discover(
    pool: &SqlitePool,
    request: &MusicLocalRefreshRequest,
    root: &Path,
    scan_limit: usize,
) -> MusicLibraryResult<()> {
    let mut visited_entries = 0_usize;
    while let Some(directory) = persistence::next_pending_directory(pool, &request.job_id).await? {
        if !persistence::is_current(pool, &request.job_id).await? {
            return Ok(());
        }
        let mut reader = traversal::open_directory(root, &directory)
            .map_err(|message| MusicLibraryError::runtime("discover local music", message))?;
        loop {
            let entries = traversal::next_discovery_batch(root, &mut reader)
                .map_err(|message| MusicLibraryError::runtime("discover local music", message))?;
            visited_entries = visited_entries.saturating_add(entries.len());
            if visited_entries > scan_limit {
                return Err(MusicLibraryError::validation(
                    "folderPath",
                    format!(
                        "contains more than {scan_limit} files and folders; choose the folder that holds this library"
                    ),
                ));
            }
            let complete = entries.len() < traversal::DISCOVERY_BATCH_SIZE;
            persistence::save_discovery_batch(
                pool,
                &request.job_id,
                &directory,
                &entries,
                complete,
            )
            .await?;
            if complete {
                break;
            }
            if !persistence::is_current(pool, &request.job_id).await? {
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn catalog(
    pool: &SqlitePool,
    request: &MusicLocalRefreshRequest,
    root: &Path,
    generation: i64,
) -> MusicLibraryResult<()> {
    loop {
        if !persistence::is_current(pool, &request.job_id).await? {
            return Ok(());
        }
        let paths =
            persistence::next_media_batch(pool, &request.job_id, traversal::RECONCILE_BATCH_SIZE)
                .await?;
        if paths.is_empty() {
            return Ok(());
        }
        let observed = paths
            .into_iter()
            .map(|relative_path| {
                let stat = traversal::stat_media(root, &relative_path);
                (relative_path, stat)
            })
            .collect();
        let paths =
            persistence::record_unchanged_batch(pool, request, generation, observed).await?;
        let inspected = inspect_batch(root, &paths)?;
        let mut evidence = Vec::with_capacity(inspected.len());
        for (relative_path, result) in inspected {
            match result {
                Ok(media) => evidence.push(media),
                Err(message) => {
                    persistence::record_media_failure(pool, request, relative_path, &message)
                        .await?;
                }
            }
        }
        if !evidence.is_empty() {
            persistence::reconcile_batch(pool, request, generation, &evidence).await?;
        }
    }
}

fn inspection_worker_count(item_count: usize, available_parallelism: usize) -> usize {
    item_count.min(available_parallelism.clamp(1, 4)).max(1)
}

fn inspect_batch<'a>(
    root: &'a Path,
    paths: &'a [String],
) -> MusicLibraryResult<Vec<(&'a str, Result<traversal::LocalMediaEvidence, String>)>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let available = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let worker_count = inspection_worker_count(paths.len(), available);
    let chunk_size = paths.len().div_ceil(worker_count);
    std::thread::scope(|scope| {
        let handles = paths
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    let mut artwork_cache = traversal::ArtworkCache::new();
                    chunk
                        .iter()
                        .map(|relative_path| {
                            (
                                relative_path.as_str(),
                                traversal::inspect_media(root, relative_path, &mut artwork_cache),
                            )
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let mut inspected = Vec::with_capacity(paths.len());
        for handle in handles {
            inspected.extend(handle.join().map_err(|_| {
                MusicLibraryError::runtime(
                    "inspect local music",
                    "a media inspection worker stopped unexpectedly",
                )
            })?);
        }
        Ok(inspected)
    })
}

#[cfg(test)]
mod tests;
