//! Collection of managed asset files that nothing references any more.
//!
//! One sweep covers every asset class the vault manifest registers. A file stays while any live
//! row or retained version references its digest, while a row of a `Local` table names it
//! through a declared asset column, while a transfer touched it within the grace,
//! or while it is younger than the grace, which covers a file written just before the row that
//! references it commits. References are read before and after the directories are listed,
//! and a file goes only when neither read keeps it, so a row that commits during the listing
//! keeps its file. The sweep holds the [`ArchiveGate`], so no archive reads the vault while
//! files go.

use crate::backup::ArchiveGate;
use ganbaru_atomic_files::is_temporary_file_name;
use ganbaru_sync::Engine;
use ganbaru_sync::assets::{AssetClass, AssetRef, is_sha256_hex};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// How long an unreferenced asset file stays before collection, so a file written just before
/// the row that references it, or received before the operation that references it, survives.
pub const UNREFERENCED_ASSET_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

/// A class and digest, which name an asset file regardless of its extension.
type DigestKey = (&'static str, String);

/// Assets that transfers of this process touched recently: received, kept as already held, or
/// confirmed as held to a peer. The sweep spares them within its grace, since the operation
/// that references such an asset may not have arrived yet while its file is old.
#[derive(Debug, Default)]
pub struct InFlightAssets {
    touched: Mutex<HashMap<DigestKey, Instant>>,
}

impl InFlightAssets {
    /// The registry every asset store and sweep of this process shares.
    pub fn process() -> Arc<Self> {
        static PROCESS: LazyLock<Arc<InFlightAssets>> = LazyLock::new(Arc::default);
        Arc::clone(&PROCESS)
    }

    /// Records that a transfer touched `asset` now.
    pub fn touch(&self, asset: &AssetRef) {
        if let Ok(mut touched) = self.touched.lock() {
            touched.insert(
                (asset.class().name, asset.sha256().to_string()),
                Instant::now(),
            );
        }
    }
}

/// What one sweep works on.
#[derive(Clone, Copy)]
pub struct AssetSweep<'a> {
    /// The engine whose manifest registers the classes and whose adapters report references.
    pub engine: &'a Engine,
    /// The vault database.
    pub pool: &'a SqlitePool,
    /// The vault root the asset directories live under.
    pub vault_root: &'a Path,
    /// Assets transfers touched recently.
    pub in_flight: &'a Arc<InFlightAssets>,
    /// How long an unreferenced file stays.
    pub grace: Duration,
    /// The time file ages are measured against.
    pub now: SystemTime,
}

/// Removes the asset files of the registered classes that nothing references and that are
/// older than the grace, and returns how many it removed.
///
/// The sweep waits for `gate`, then asks `admit` for each class that some adapter reports
/// references for. A class `admit` refuses is skipped, and the permit it returns is held until
/// the sweep ends. Only regular files named `{sha256}.{ext}` are candidates, so staged
/// temporary files and anything else in an asset directory stay.
///
/// # Errors
///
/// Returns an error when the references cannot be read or a directory cannot be listed or
/// cleaned; files removed before the error stay removed, since none of them was referenced.
pub async fn collect_unreferenced_assets<P>(
    gate: &ArchiveGate,
    sweep: AssetSweep<'_>,
    mut admit: impl FnMut(&'static AssetClass) -> Option<P>,
) -> Result<usize, String> {
    let _gate = gate.lock().await;
    let mut permits = Vec::new();
    let mut classes = Vec::new();
    for class in sweep.engine.manifest().asset_classes() {
        // A class no adapter or `Local` column reports references for would look wholly
        // unreferenced.
        if sweep.engine.asset_tables(class).is_empty()
            && sweep.engine.local_asset_tables(class).is_empty()
        {
            continue;
        }
        if let Some(permit) = admit(class) {
            permits.push(permit);
            classes.push(class);
        }
    }
    if classes.is_empty() {
        return Ok(0);
    }
    let before = kept_digests(sweep).await?;
    let root = sweep.vault_root.to_path_buf();
    let (grace, now) = (sweep.grace, sweep.now);
    let candidates = tokio::task::spawn_blocking(move || {
        unreferenced_candidates(&root, &classes, &before, grace, now)
    })
    .await
    .map_err(|error| format!("asset sweep worker: {error}"))??;
    if candidates.is_empty() {
        return Ok(0);
    }
    // A row that commits while the directories are listed names a file the first read missed,
    // so only files that neither read keeps go.
    let after = kept_digests(sweep).await?;
    let in_flight = Arc::clone(sweep.in_flight);
    let removed = tokio::task::spawn_blocking(move || {
        remove_candidates(candidates, &after, &in_flight, grace)
    })
    .await
    .map_err(|error| format!("asset sweep worker: {error}"))?;
    drop(permits);
    removed
}

/// A file the sweep may remove: its class and digest, and its path.
type Candidate = (DigestKey, PathBuf);

/// The class and digest of every file the sweep keeps, read in one transaction.
async fn kept_digests(sweep: AssetSweep<'_>) -> Result<HashSet<DigestKey>, String> {
    let mut conn = sweep
        .pool
        .acquire()
        .await
        .map_err(|error| format!("acquire vault connection: {error}"))?;
    Ok(sweep
        .engine
        .kept_asset_digests(&mut conn)
        .await
        .map_err(|error| format!("read referenced assets: {error}"))?
        .into_iter()
        .collect())
}

/// Files of `classes` named `{sha256}.{ext}` that `live` does not keep and that are regular
/// files older than the grace.
fn unreferenced_candidates(
    root: &Path,
    classes: &[&'static AssetClass],
    live: &HashSet<DigestKey>,
    grace: Duration,
    now: SystemTime,
) -> Result<Vec<Candidate>, String> {
    let mut candidates = Vec::new();
    for class in classes {
        let directory = class.directory(root);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("list {} assets: {error}", class.name)),
        };
        for entry in entries {
            let entry = entry.map_err(|error| format!("list {} assets: {error}", class.name))?;
            let Some(digest) = asset_digest(&entry.file_name()) else {
                continue;
            };
            let key = (class.name, digest);
            if live.contains(&key) {
                continue;
            }
            let path = entry.path();
            if aged_regular_file(&path, grace, now)? {
                candidates.push((key, path));
            }
        }
    }
    Ok(candidates)
}

/// Removes each candidate that `live` does not keep and no transfer touched within `grace`,
/// and returns how many it removed.
fn remove_candidates(
    candidates: Vec<Candidate>,
    live: &HashSet<DigestKey>,
    in_flight: &InFlightAssets,
    grace: Duration,
) -> Result<usize, String> {
    let mut removed = 0;
    for (key, path) in candidates {
        if !live.contains(&key) && remove_unless_touched(in_flight, &key, &path, grace)? {
            removed += 1;
        }
    }
    Ok(removed)
}

/// The digest an asset file name carries, when the name has the `{sha256}.{ext}` shape.
fn asset_digest(name: &std::ffi::OsStr) -> Option<String> {
    if is_temporary_file_name(name) {
        return None;
    }
    let (stem, extension) = name.to_str()?.split_once('.')?;
    let extension_valid =
        !extension.is_empty() && extension.bytes().all(|byte| byte.is_ascii_alphanumeric());
    (is_sha256_hex(stem) && extension_valid).then(|| stem.to_string())
}

/// Whether `path` is a regular file unmodified for at least `grace` before `now`.
fn aged_regular_file(path: &Path, grace: Duration, now: SystemTime) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("inspect asset: {error}")),
    };
    if !metadata.is_file() {
        return Ok(false);
    }
    let modified = metadata
        .modified()
        .map_err(|error| format!("inspect asset: {error}"))?;
    Ok(now.duration_since(modified).unwrap_or(Duration::ZERO) >= grace)
}

/// Removes `path` unless a transfer touched its asset within `grace`. The registry stays locked
/// across the check and the removal, so a transfer that touches the asset either sees the file
/// gone or keeps it.
fn remove_unless_touched(
    in_flight: &InFlightAssets,
    key: &DigestKey,
    path: &Path,
    grace: Duration,
) -> Result<bool, String> {
    let mut touched = in_flight
        .touched
        .lock()
        .map_err(|_| "in-flight asset registry is unavailable".to_string())?;
    touched.retain(|_, at| at.elapsed() < grace);
    if touched.contains_key(key) {
        return Ok(false);
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("remove unreferenced asset: {error}")),
    }
}

#[cfg(test)]
mod tests;
