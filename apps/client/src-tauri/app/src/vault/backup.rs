//! Whole-vault snapshots and activation, with portable Android backup commands.

#[cfg(target_os = "android")]
use super::active_vault_path;
use super::select_quiesced_vault;
use super::{APP_SQLITE_FILE, CONFIG_FILE, CONFIG_LOCK, VAULT_MANIFEST_FILE, VaultInfo};
use super::{MAX_VAULT_MANIFEST_BYTES, database_path, parse_vault_manifest, vault_info_from_path};
#[cfg(target_os = "android")]
use super::{default_vault_path, ensure_vault_skeleton, path_to_string};
#[cfg(target_os = "android")]
use crate::db;
#[cfg(target_os = "android")]
use chrono::{SecondsFormat, Utc};
#[cfg(target_os = "android")]
use ganbaru_mobile_documents::MobileDocumentsExt;
use sqlx::Row;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
#[cfg(target_os = "android")]
use tauri::Manager;
use tauri::Runtime;
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::{CompressionMethod, ZipArchive};

#[cfg(target_os = "android")]
const BACKUP_EXTENSION: &str = "ganbaru-backup";
#[cfg(target_os = "android")]
const BACKUP_MIME_TYPE: &str = "application/zip";
const BACKUP_MAX_ENTRIES: usize = 100_000;
const BACKUP_MAX_BYTES: u64 = 100 * 1024 * 1024 * 1024;
const BACKUP_MAX_DEPTH: usize = 64;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
/// Top-level directories a vault bundle carries with everything below them.
/// Every vault skeleton directory lives below one of these.
pub(super) const BUNDLE_DIRECTORIES: &[&str] = &[
    "notes",
    "diary",
    "projects",
    "reports",
    "assets",
    "templates",
    ".yjs",
];
/// Directory whose direct children are project working folders.
const BUNDLE_PROJECTS_DIRECTORY: &str = "projects";
/// Most skipped bundle entries named in one log line.
const MAX_REPORTED_SKIPPED_ENTRIES: usize = 8;
const DIFFERENT_VAULT_ERROR: &str = "received bundle belongs to a different vault";

/// Whether `relative` belongs to the vault layout a bundle carries: the vault
/// marker, configuration, and database files, the skeleton directories with
/// their contents, and only folders directly below `projects`. Anything else,
/// such as a `.git`, `.claude`, `.codex`, or `.vscode` folder at the vault
/// root, stays out of bundles in both directions.
fn is_bundle_layout_entry(relative: &Path, directory: bool) -> bool {
    let mut components = relative.components().map(|component| match component {
        Component::Normal(name) => name.to_str(),
        _ => None,
    });
    let Some(Some(top)) = components.next() else {
        return false;
    };
    match components.next() {
        None if directory => BUNDLE_DIRECTORIES.contains(&top),
        None => [VAULT_MANIFEST_FILE, CONFIG_FILE, APP_SQLITE_FILE].contains(&top),
        Some(None) => false,
        Some(Some(_)) if top == BUNDLE_PROJECTS_DIRECTORY => {
            directory || components.next().is_some()
        }
        Some(Some(_)) => BUNDLE_DIRECTORIES.contains(&top),
    }
}

/// Whether `name` is a SQLite journal, WAL, or shared-memory file of the vault
/// database. These belong to one database file and never move with another.
fn is_database_sidecar(name: &Path) -> bool {
    name.file_name()
        .and_then(|value| value.to_str())
        .and_then(|value| value.strip_prefix(APP_SQLITE_FILE))
        .is_some_and(|suffix| suffix.starts_with('-'))
}

/// Entries left out of an extracted bundle or a vault replacement.
#[derive(Debug, Default)]
struct SkippedEntries {
    count: usize,
    named: Vec<PathBuf>,
}

impl SkippedEntries {
    fn push(&mut self, path: PathBuf) {
        self.count += 1;
        if self.named.len() < MAX_REPORTED_SKIPPED_ENTRIES {
            self.named.push(path);
        }
    }

    /// Logs the skipped entries, naming the first few, so nothing is left out
    /// silently.
    fn report(&self, operation: &str, reason: &str) {
        if self.count == 0 {
            return;
        }
        let named = self
            .named
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "{operation} skipped {} entries {reason}: {named}",
            self.count
        );
    }
}

#[cfg(any(test, mobile))]
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultBackupOutcome {
    /// Display name assigned by Android in shared Downloads.
    pub file_name: String,
    /// Stable frontend destination identifier.
    pub destination: &'static str,
}

#[cfg(target_os = "android")]
fn unique_transfer_directory<R: Runtime>(
    app: &tauri::AppHandle<R>,
    operation: &str,
) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("find app cache directory: {error}"))?;
    fs::create_dir_all(&root).map_err(|error| format!("create app cache directory: {error}"))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("create transfer identifier: {error}"))?
        .as_nanos();
    let directory = root.join(format!(
        "vault-{operation}-{}-{nonce:x}",
        std::process::id()
    ));
    fs::create_dir(&directory).map_err(|error| format!("create transfer directory: {error}"))?;
    Ok(directory)
}

#[cfg(target_os = "android")]
fn backup_file_name() -> String {
    let timestamp: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    let timestamp = timestamp
        .to_rfc3339_opts(SecondsFormat::Secs, true)
        .replace(':', "-");
    format!("ganbaru-backup-{timestamp}.{BACKUP_EXTENSION}")
}

#[cfg(target_os = "android")]
async fn create_database_snapshot<R: Runtime>(
    app: &tauri::AppHandle<R>,
    destination: &Path,
) -> Result<(), String> {
    let pool = crate::db::connect_vault(&app).await?;
    vacuum_database(&pool, destination).await
}

async fn vacuum_database(pool: &sqlx::SqlitePool, destination: &Path) -> Result<(), String> {
    let destination = destination
        .to_str()
        .ok_or_else(|| "database snapshot path contains non-UTF-8 characters".to_string())?;
    sqlx::query("VACUUM INTO ?")
        .bind(destination)
        .execute(pool)
        .await
        .map_err(|error| format!("create consistent database snapshot: {error}"))?;
    Ok(())
}

#[derive(Clone)]
struct ArchiveEntry {
    source: PathBuf,
    relative: PathBuf,
    directory: bool,
}

fn collect_archive_entries(
    root: &Path,
    current: &Path,
    entries: &mut Vec<ArchiveEntry>,
    total_bytes: &mut u64,
) -> Result<(), String> {
    let mut children = fs::read_dir(current)
        .map_err(|error| format!("read backup source '{}': {error}", current.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read backup source entry: {error}"))?;
    children.sort_by_key(fs::DirEntry::file_name);

    for child in children {
        if entries.len() >= BACKUP_MAX_ENTRIES {
            return Err("Ganbaru folder contains too many backup entries".to_string());
        }
        let source = child.path();
        let relative = source
            .strip_prefix(root)
            .map_err(|_| "backup entry escaped the Ganbaru folder".to_string())?
            .to_path_buf();
        let depth = relative.components().count();
        if depth > BACKUP_MAX_DEPTH {
            return Err("Ganbaru folder is nested too deeply to back up".to_string());
        }
        let metadata = fs::symlink_metadata(&source)
            .map_err(|error| format!("inspect backup entry '{}': {error}", source.display()))?;
        if !is_bundle_layout_entry(&relative, metadata.is_dir()) {
            continue;
        }
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "backup does not support symbolic link '{}'",
                relative.display()
            ));
        }
        if metadata.is_dir() {
            entries.push(ArchiveEntry {
                source: source.clone(),
                relative,
                directory: true,
            });
            collect_archive_entries(root, &source, entries, total_bytes)?;
        } else if metadata.is_file() {
            if is_database_sidecar(&relative) {
                continue;
            }
            *total_bytes = total_bytes.saturating_add(metadata.len());
            if *total_bytes > BACKUP_MAX_BYTES {
                return Err("Ganbaru folder exceeds the backup size limit".to_string());
            }
            entries.push(ArchiveEntry {
                source,
                relative,
                directory: false,
            });
        } else {
            return Err(format!(
                "backup contains unsupported entry '{}'",
                relative.display()
            ));
        }
    }
    Ok(())
}

fn archive_name(path: &Path, directory: bool) -> Result<String, String> {
    let mut name = path
        .to_str()
        .ok_or_else(|| "backup entry path contains non-UTF-8 characters".to_string())?
        .replace('\\', "/");
    if directory && !name.ends_with('/') {
        name.push('/');
    }
    Ok(name)
}

fn create_backup_archive(
    vault_root: &Path,
    database_snapshot: &Path,
    destination: &Path,
) -> Result<(), String> {
    let _config_guard = CONFIG_LOCK
        .lock()
        .map_err(|_| "vault config lock is unavailable".to_string())?;
    let mut entries = Vec::new();
    let mut total_bytes = 0;
    collect_archive_entries(vault_root, vault_root, &mut entries, &mut total_bytes)?;

    let file =
        fs::File::create(destination).map_err(|error| format!("create backup archive: {error}"))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600);
    let directory_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o700);
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];

    for entry in entries {
        let name = archive_name(&entry.relative, entry.directory)?;
        if entry.directory {
            writer
                .add_directory(name, directory_options)
                .map_err(|error| format!("write backup directory: {error}"))?;
            continue;
        }
        writer
            .start_file(name, options)
            .map_err(|error| format!("write backup entry: {error}"))?;
        let source = if entry.relative == Path::new(APP_SQLITE_FILE) {
            database_snapshot
        } else {
            &entry.source
        };
        let mut input = fs::File::open(source).map_err(|error| {
            format!("open backup entry '{}': {error}", entry.relative.display())
        })?;
        loop {
            let count = input.read(&mut buffer).map_err(|error| {
                format!("read backup entry '{}': {error}", entry.relative.display())
            })?;
            if count == 0 {
                break;
            }
            writer.write_all(&buffer[..count]).map_err(|error| {
                format!(
                    "compress backup entry '{}': {error}",
                    entry.relative.display()
                )
            })?;
        }
    }
    writer
        .finish()
        .map_err(|error| format!("finish backup archive: {error}"))?
        .sync_all()
        .map_err(|error| format!("sync backup archive: {error}"))?;
    Ok(())
}

/// Creates one consistent whole-vault archive after the caller excludes vault writers.
pub(crate) async fn create_handoff_archive(
    vault_root: &Path,
    database_snapshot: &Path,
    archive_path: &Path,
) -> Result<(), String> {
    for path in [database_snapshot, archive_path] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("remove stale handoff snapshot: {error}")),
        }
    }
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path(database_path(vault_root)).await?;
    let snapshot_result = vacuum_database(&pool, database_snapshot).await;
    registry.close_all().await?;
    snapshot_result?;
    prepare_snapshot_copy(database_snapshot).await?;
    create_backup_archive(vault_root, database_snapshot, archive_path)
}

fn safe_archive_path(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().count() > BACKUP_MAX_DEPTH
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("backup contains an unsafe entry path".to_string());
    }
    Ok(path.to_path_buf())
}

/// Checks the central directory before anything is written: the archive must hold a vault
/// marker and database, its in-layout entries must declare at most `max_bytes` in total, and
/// the marker must parse. Returns the archived vault identity.
fn admit_backup_archive<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    max_bytes: u64,
) -> Result<String, String> {
    if archive.len() > BACKUP_MAX_ENTRIES {
        return Err("backup contains too many entries".to_string());
    }
    let mut declared_bytes = 0_u64;
    let mut has_database = false;
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|error| format!("read backup entry {index}: {error}"))?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        if entry.is_dir() || !is_bundle_layout_entry(&relative, false) {
            continue;
        }
        has_database |= relative == Path::new(APP_SQLITE_FILE);
        declared_bytes = declared_bytes.saturating_add(entry.size());
        if declared_bytes > max_bytes {
            return Err("backup exceeds the restore size limit".to_string());
        }
    }
    if !has_database {
        return Err("backup is missing its database".to_string());
    }
    let manifest = archive
        .by_name(VAULT_MANIFEST_FILE)
        .map_err(|error| format!("backup is missing its Ganbaru folder marker: {error}"))?;
    if !manifest.is_file() || manifest.size() > MAX_VAULT_MANIFEST_BYTES {
        return Err("backup has an invalid Ganbaru folder marker".to_string());
    }
    let mut contents = Vec::new();
    manifest
        .take(MAX_VAULT_MANIFEST_BYTES + 1)
        .read_to_end(&mut contents)
        .map_err(|error| format!("read backup Ganbaru folder marker: {error}"))?;
    if contents.len() as u64 > MAX_VAULT_MANIFEST_BYTES {
        return Err("backup has an invalid Ganbaru folder marker".to_string());
    }
    Ok(parse_vault_manifest(&contents)?.vault_id)
}

/// Extracts a bundle into a new `destination`, skipping entries outside the
/// vault layout instead of failing the whole restore.
fn extract_backup_archive(
    archive_path: &Path,
    destination: &Path,
    expected_vault_id: Option<&str>,
) -> Result<SkippedEntries, String> {
    extract_backup_archive_within(
        archive_path,
        destination,
        expected_vault_id,
        BACKUP_MAX_BYTES,
    )
}

fn extract_backup_archive_within(
    archive_path: &Path,
    destination: &Path,
    expected_vault_id: Option<&str>,
    max_bytes: u64,
) -> Result<SkippedEntries, String> {
    let file = fs::File::open(archive_path).map_err(|error| format!("open backup: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("open backup archive: {error}"))?;
    let vault_id = admit_backup_archive(&mut archive, max_bytes)?;
    if expected_vault_id.is_some_and(|expected| expected != vault_id) {
        return Err(DIFFERENT_VAULT_ERROR.to_string());
    }
    ganbaru_atomic_files::create_private_directory(destination)
        .map_err(|error| format!("create restore staging folder: {error}"))?;
    let mut total_bytes = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut skipped = SkippedEntries::default();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("read backup entry {index}: {error}"))?;
        if entry.encrypted() {
            return Err("encrypted backup archives are not supported".to_string());
        }
        if let Some(mode) = entry.unix_mode() {
            let file_type = mode & 0o170000;
            if file_type != 0 && file_type != 0o040000 && file_type != 0o100000 {
                return Err("backup contains an unsupported special file".to_string());
            }
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| "backup contains an unsafe entry path".to_string())?;
        let relative = safe_archive_path(&enclosed)?;
        if !is_bundle_layout_entry(&relative, entry.is_dir()) {
            skipped.push(relative);
            continue;
        }
        let output = destination.join(&relative);
        if output.exists() {
            return Err(format!(
                "backup contains duplicate entry '{}'",
                relative.display()
            ));
        }
        if entry.is_dir() {
            fs::create_dir(&output).map_err(|error| {
                format!(
                    "create restored directory '{}': {error}",
                    relative.display()
                )
            })?;
            continue;
        }
        if !entry.is_file() {
            return Err(format!(
                "backup entry '{}' is unsupported",
                relative.display()
            ));
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("create restored parent '{}': {error}", relative.display())
            })?;
        }
        let mut target = fs::File::create(&output)
            .map_err(|error| format!("create restored file '{}': {error}", relative.display()))?;
        let declared_bytes = entry.size();
        let mut entry_bytes = 0_u64;
        loop {
            let count = entry
                .read(&mut buffer)
                .map_err(|error| format!("read backup file '{}': {error}", relative.display()))?;
            if count == 0 {
                break;
            }
            entry_bytes = entry_bytes.saturating_add(count as u64);
            total_bytes = total_bytes.saturating_add(count as u64);
            if entry_bytes > declared_bytes {
                return Err(format!(
                    "backup file '{}' exceeds its declared size",
                    relative.display()
                ));
            }
            if total_bytes > max_bytes {
                return Err("backup exceeds the restore size limit".to_string());
            }
            target.write_all(&buffer[..count]).map_err(|error| {
                format!("write restored file '{}': {error}", relative.display())
            })?;
        }
        target
            .sync_all()
            .map_err(|error| format!("sync restored file '{}': {error}", relative.display()))?;
    }
    Ok(skipped)
}

/// Extracts and validates a received whole-vault archive in separate staging.
pub(crate) async fn stage_handoff_archive(
    archive_path: &Path,
    destination: &Path,
    expected_vault_id: &str,
) -> Result<VaultInfo, String> {
    if destination.exists() {
        fs::remove_dir_all(destination)
            .map_err(|error| format!("remove stale handoff staging: {error}"))?;
    }
    match extract_backup_archive(archive_path, destination, Some(expected_vault_id)) {
        Ok(skipped) => skipped.report("Handoff staging", "outside the vault layout"),
        Err(error) => {
            let _ = fs::remove_dir_all(destination);
            return Err(error);
        }
    }
    let info = match validate_restored_vault(destination).await {
        Ok(info) => info,
        Err(error) => {
            let _ = fs::remove_dir_all(destination);
            return Err(error);
        }
    };
    if info.vault_id != expected_vault_id {
        let _ = fs::remove_dir_all(destination);
        return Err(DIFFERENT_VAULT_ERROR.to_string());
    }
    Ok(info)
}

pub(crate) async fn validate_restored_vault(path: &Path) -> Result<VaultInfo, String> {
    let info = vault_info_from_path(path)?;
    let restored_database = database_path(path);
    let metadata = fs::metadata(&restored_database)
        .map_err(|error| format!("backup is missing its database: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err("backup database is empty".to_string());
    }
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path_read_only(&restored_database).await?;
    let validation = async {
        // The catalog check runs first so that integrity checking never reaches virtual table
        // modules, such as FTS5, that the restored file defines or reshapes.
        ganbaru_db::validate_current_schema(&pool).await?;
        let row = sqlx::query("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .map_err(|error| format!("check restored database: {error}"))?;
        let result: String = row
            .try_get(0)
            .map_err(|error| format!("read restored database check: {error}"))?;
        if result != "ok" {
            return Err(format!(
                "restored database failed its integrity check: {result}"
            ));
        }
        Ok(())
    }
    .await;
    let close_result = registry.close_all().await;
    validation?;
    close_result?;
    Ok(info)
}

/// Smallest replaced vault, in bytes of data, whose replacement can be flagged as a drastic
/// shrink. Below it a near-empty vault would be flagged on ordinary edits.
const MIN_FLAGGED_SHRINK_BYTES: u64 = 1024 * 1024;
/// A replacement is a drastic shrink when it holds less than this fraction (1/N) of the data
/// it replaces.
const DRASTIC_SHRINK_DIVISOR: u64 = 2;

/// Whether replacing a vault holding `previous` bytes of data with one holding `replacement`
/// bytes removes most of it.
fn is_drastic_shrink(previous: u64, replacement: u64) -> bool {
    previous >= MIN_FLAGGED_SHRINK_BYTES
        && replacement.saturating_mul(DRASTIC_SHRINK_DIVISOR) < previous
}

/// Bytes the database at `path` uses, excluding free pages, so a replacement snapshot that was
/// vacuumed compares fairly with a live database.
async fn database_used_bytes(path: &Path) -> Result<u64, String> {
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path_read_only(path).await?;
    let measured = async {
        let mut values = [0_i64; 3];
        for (value, pragma) in values.iter_mut().zip([
            "PRAGMA page_size",
            "PRAGMA page_count",
            "PRAGMA freelist_count",
        ]) {
            *value = sqlx::query_scalar(pragma)
                .fetch_one(&pool)
                .await
                .map_err(|error| format!("measure vault database: {error}"))?;
        }
        let [page_size, page_count, freelist] = values.map(|value| value.max(0) as u64);
        Ok::<u64, String>(
            page_count
                .saturating_sub(freelist)
                .saturating_mul(page_size),
        )
    }
    .await;
    let close_result = registry.close_all().await;
    let bytes = measured?;
    close_result?;
    Ok(bytes)
}

/// Bytes of the regular files in the vault layout at `root`, excluding the database files and
/// symbolic links. Counting stops after [`BACKUP_MAX_ENTRIES`] entries, the most a bundle holds.
fn layout_file_bytes(root: &Path) -> u64 {
    let mut total = 0_u64;
    let mut visited = 0_usize;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(children) = fs::read_dir(&directory) else {
            continue;
        };
        for child in children.flatten() {
            visited += 1;
            if visited > BACKUP_MAX_ENTRIES {
                return total;
            }
            let path = child.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            if !is_bundle_layout_entry(relative, metadata.is_dir()) {
                continue;
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file()
                && relative != Path::new(APP_SQLITE_FILE)
                && !is_database_sidecar(relative)
            {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

/// Bytes of data the vault at `vault` holds: its used database pages and its layout files.
async fn vault_content_bytes(vault: &Path) -> Result<u64, String> {
    let database = database_used_bytes(&database_path(vault)).await?;
    let root = vault.to_path_buf();
    let files = tauri::async_runtime::spawn_blocking(move || layout_file_bytes(&root))
        .await
        .map_err(|error| format!("measure vault files: {error}"))?;
    Ok(database.saturating_add(files))
}

/// Whether activating `staging` over `target`, a copy of the same vault, removes most of its
/// data. A measurement failure is logged and not flagged; the replaced database is kept either
/// way.
async fn replacement_shrinks_drastically(target: &Path, staging: &Path) -> bool {
    match (
        vault_content_bytes(target).await,
        vault_content_bytes(staging).await,
    ) {
        (Ok(previous), Ok(replacement)) => is_drastic_shrink(previous, replacement),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("Vault replacement size check: {error}");
            false
        }
    }
}

#[cfg(any(test, target_os = "android"))]
fn replace_vault(staging: &Path, target: &Path, rollback: &Path) -> Result<(), String> {
    replace_vault_flagging(staging, target, rollback, false)
}

/// Replaces `target` with `staging`. When both hold the same vault, the replaced data is kept as
/// a rollback inside the new folder: the database, or the whole layout when `flagged` marks a
/// drastic shrink.
fn replace_vault_flagging(
    staging: &Path,
    target: &Path,
    rollback: &Path,
    flagged: bool,
) -> Result<(), String> {
    recover_interrupted_restore(target, rollback)?;
    let had_target = target.exists();
    if had_target {
        fs::rename(target, rollback)
            .map_err(|error| format!("prepare current data for restore: {error}"))?;
    }
    if let Err(error) = fs::rename(staging, target) {
        if had_target {
            let _ = fs::rename(rollback, target);
        }
        return Err(format!("activate restored data: {error}"));
    }
    if had_target && let Err(error) = finish_retained_replacement(target, rollback, flagged) {
        eprintln!("Vault replacement cleanup: {error}");
    }
    Ok(())
}

/// Keeps the replaced data of the same vault as a rollback, then finishes the replacement.
/// When the data cannot be kept, the whole replaced folder stays as a visible sibling instead
/// of being deleted.
fn finish_retained_replacement(
    target: &Path,
    rollback: &Path,
    flagged: bool,
) -> Result<(), String> {
    let now = super::now_utc();
    let retained = vault_info_from_path(target).and_then(|info| {
        super::database_rollback::retain_replaced_vault(rollback, &info.vault_id, flagged, now)
    });
    if let Err(error) = retained {
        let kept = kept_previous_path(target)?;
        fs::rename(rollback, &kept)
            .map_err(|rename| format!("{error}; keep the replaced folder: {rename}"))?;
        eprintln!(
            "Vault replacement kept the replaced folder in '{}': {error}",
            kept.display()
        );
        return Ok(());
    }
    finish_replacement(target, rollback)?;
    super::database_rollback::prune_after_replacement(target, now)
}

/// Moves entries outside the vault layout from the replaced folder `rollback`
/// into its replacement `target`, then removes `rollback`. Bundles never carry
/// such entries, so they are files this device kept in its vault folder. When
/// one cannot move, `rollback` is kept as a visible sibling instead.
fn finish_replacement(target: &Path, rollback: &Path) -> Result<(), String> {
    let mut stranded = SkippedEntries::default();
    match fs::read_dir(rollback) {
        Ok(children) => {
            for child in children {
                let Ok(child) = child else {
                    stranded.push(rollback.to_path_buf());
                    continue;
                };
                let name = PathBuf::from(child.file_name());
                let directory = child.file_type().is_ok_and(|kind| kind.is_dir());
                if is_bundle_layout_entry(&name, directory) || is_database_sidecar(&name) {
                    continue;
                }
                let destination = target.join(&name);
                if fs::symlink_metadata(&destination).is_ok()
                    || fs::rename(child.path(), &destination).is_err()
                {
                    stranded.push(name);
                }
            }
        }
        Err(_) => stranded.push(rollback.to_path_buf()),
    }
    if stranded.count == 0 {
        return fs::remove_dir_all(rollback)
            .map_err(|error| format!("remove completed restore rollback: {error}"));
    }
    let kept = kept_previous_path(target)?;
    fs::rename(rollback, &kept)
        .map_err(|error| format!("keep files from before the replacement: {error}"))?;
    stranded.report(
        "Vault replacement",
        &format!("that stay in '{}'", kept.display()),
    );
    Ok(())
}

/// Visible sibling that keeps a replaced folder whose own files could not move.
fn kept_previous_path(target: &Path) -> Result<PathBuf, String> {
    let parent = target
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
    let folder_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Ganbaru");
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("name the kept folder: {error}"))?
        .as_secs();
    Ok(parent.join(format!("{folder_name} before replacement {seconds}")))
}

#[cfg(target_os = "android")]
pub(crate) fn android_handoff_staging_path(
    app: &tauri::AppHandle,
    transfer_id: &str,
) -> Result<PathBuf, String> {
    let target = default_vault_path(app)?;
    let parent = target
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
    Ok(parent.join(format!(".ganbaru.handoff-{transfer_id}.staging")))
}

#[cfg(desktop)]
pub(crate) fn active_handoff_staging_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    transfer_id: &str,
) -> Result<PathBuf, String> {
    let target = super::active_vault_path(app)?;
    let parent = target
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
    Ok(parent.join(format!(".ganbaru.handoff-{transfer_id}.staging")))
}

#[cfg(target_os = "android")]
pub(crate) async fn activate_android_handoff(
    app: &tauri::AppHandle,
    staging: &Path,
    transfer_id: &str,
    expected_vault_id: &str,
    preserve_previous: bool,
) -> Result<VaultInfo, String> {
    let target = default_vault_path(app)?;
    activate_handoff_at_path(
        app,
        staging,
        &target,
        transfer_id,
        expected_vault_id,
        preserve_previous,
    )
    .await
}

#[cfg(desktop)]
pub(crate) async fn activate_active_handoff<R: Runtime>(
    app: &tauri::AppHandle<R>,
    staging: &Path,
    transfer_id: &str,
    expected_vault_id: &str,
    preserve_previous: bool,
) -> Result<VaultInfo, String> {
    let target = super::active_vault_path(app)?;
    activate_handoff_at_path(
        app,
        staging,
        &target,
        transfer_id,
        expected_vault_id,
        preserve_previous,
    )
    .await
}

/// Activates an ownership snapshot a linked owner uploaded to this coordinator. The coordinator's
/// own copy of the linked vault is read-only while another device owns it, and its Quick notes
/// changes carry forward.
#[cfg(desktop)]
pub(crate) async fn activate_uploaded_ownership<R: Runtime>(
    app: &tauri::AppHandle<R>,
    staging: &Path,
    transfer_id: &str,
    expected_vault_id: &str,
) -> Result<VaultInfo, String> {
    let target = super::active_vault_path(app)?;
    activate_handoff_at_path(app, staging, &target, transfer_id, expected_vault_id, false).await
}

/// Whether the folder at `path` is the vault `vault_id`.
pub(super) fn holds_vault(path: &Path, vault_id: &str) -> bool {
    vault_info_from_path(path).is_ok_and(|info| info.vault_id == vault_id)
}

/// Whether replacing `target` discards a vault other than `vault_id`. A folder whose marker
/// cannot be read counts as another vault, so it is kept.
fn replaces_independent_vault(target: &Path, vault_id: &str) -> bool {
    target.exists() && !holds_vault(target, vault_id)
}

/// Replaces `target` with the staged handoff. A folder that holds any vault other than
/// `expected_vault_id` is always kept whole beside it, whatever the caller asked, so a handoff can
/// never delete an unrelated vault.
async fn activate_handoff_at_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    staging: &Path,
    target: &Path,
    transfer_id: &str,
    expected_vault_id: &str,
    preserve_previous: bool,
) -> Result<VaultInfo, String> {
    let parent = target
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create app data directory: {error}"))?;
    let preserve_previous =
        preserve_previous || replaces_independent_vault(target, expected_vault_id);
    let rollback = if preserve_previous {
        preserved_handoff_path(parent, target, transfer_id)
    } else {
        parent.join(".ganbaru.handoff-refresh-rollback")
    };
    if rollback.exists() && staging.exists() {
        return Err("a previous handoff copy still requires recovery".to_string());
    }
    let quiescence = super::quiescence::begin_snapshot_quiescence(app).await?;
    if let Err(error) = carry_forward_local_changes(app, staging, target, expected_vault_id).await {
        return Err(match quiescence.finish() {
            Ok(()) => error,
            Err(resume) => format!("{error}; {resume}"),
        });
    }
    let flagged = !preserve_previous
        && staging.exists()
        && holds_vault(target, expected_vault_id)
        && replacement_shrinks_drastically(target, staging).await;
    let activation_app = app.clone();
    let staging = staging.to_path_buf();
    let target = target.to_path_buf();
    let expected_vault_id = expected_vault_id.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(desktop)]
        if super::active_vault_path(&activation_app)? != target {
            return Err("active vault changed while preparing handoff activation".to_string());
        }
        if staging.exists() {
            if preserve_previous {
                replace_vault_preserving_previous(&staging, &target, &rollback)?;
            } else {
                replace_vault_flagging(&staging, &target, &rollback, flagged)?;
            }
        }
        let info = vault_info_from_path(&target)?;
        if info.vault_id != expected_vault_id {
            return Err("activated handoff belongs to a different vault".to_string());
        }
        select_quiesced_vault(&activation_app, &info, &quiescence)?;
        quiescence.finish()?;
        Ok(info)
    })
    .await
    .map_err(|error| format!("handoff activation worker failed: {error}"))?
}

/// Clears installation-local sync state from a snapshot copy before it leaves this device.
async fn prepare_snapshot_copy(snapshot: &Path) -> Result<(), String> {
    use sqlx::Connection;
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(snapshot);
    let mut conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| format!("open handoff snapshot: {error}"))?;
    let result = ganbaru_sync::local::prepare_snapshot_copy(&mut conn)
        .await
        .map_err(|error| format!("prepare handoff snapshot: {error}"));
    let closed = conn
        .close()
        .await
        .map_err(|error| format!("close handoff snapshot: {error}"));
    result.and(closed)
}

/// Exports local sync operations the staged replacement lacks, so the next service start
/// imports them into the replacement, and copies the Quick note images only this device holds.
/// Only a replacement of the same vault carries anything.
async fn carry_forward_local_changes<R: Runtime>(
    app: &tauri::AppHandle<R>,
    staging: &Path,
    target: &Path,
    expected_vault_id: &str,
) -> Result<(), String> {
    if !staging.exists() || !holds_vault(target, expected_vault_id) {
        return Ok(());
    }
    let files = crate::sync::sync_files(app, expected_vault_id)?;
    crate::sync::export_for_replacement(
        app,
        &database_path(target),
        &database_path(staging),
        &files,
    )
    .await
    .map_err(|error| format!("keep local Quick notes changes: {error}"))?;
    let (current, replacement) = (target.to_path_buf(), staging.to_path_buf());
    tauri::async_runtime::spawn_blocking(move || {
        ganbaru_quick_notes::images::carry_images(&current, &replacement)
    })
    .await
    .map_err(|error| format!("keep local Quick note images: {error}"))?
    .map(|_| ())
    .map_err(|error| format!("keep local Quick note images: {error}"))
}

fn preserved_handoff_path(parent: &Path, target: &Path, transfer_id: &str) -> PathBuf {
    #[cfg(desktop)]
    {
        let folder_name = target
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Ganbaru");
        parent.join(format!("{folder_name} before linking {transfer_id}"))
    }
    #[cfg(mobile)]
    {
        let _ = target;
        parent.join(format!(".ganbaru.handoff-previous-{transfer_id}"))
    }
}

fn replace_vault_preserving_previous(
    staging: &Path,
    target: &Path,
    previous: &Path,
) -> Result<(), String> {
    let had_target = target.exists();
    if had_target {
        fs::rename(target, previous)
            .map_err(|error| format!("preserve current data before handoff: {error}"))?;
    }
    if let Err(error) = fs::rename(staging, target) {
        if had_target {
            let _ = fs::rename(previous, target);
        }
        return Err(format!("activate handed-off data: {error}"));
    }
    Ok(())
}

fn recover_interrupted_restore(target: &Path, rollback: &Path) -> Result<(), String> {
    if !rollback.exists() {
        return Ok(());
    }
    if target.exists() {
        finish_retained_replacement(target, rollback, false)
    } else {
        fs::rename(rollback, target)
            .map_err(|error| format!("recover interrupted data restore: {error}"))
    }
}

#[cfg(target_os = "android")]
pub(crate) fn recover_interrupted_restore_for_app(app: &tauri::AppHandle) -> Result<(), String> {
    let target = default_vault_path(app)?;
    let parent = target
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
    recover_interrupted_restore(&target, &parent.join(".ganbaru.rollback"))
}

#[cfg(target_os = "android")]
#[tauri::command]
/// Save a consistent portable copy of the active vault to Android Downloads.
pub async fn vault_backup_to_downloads(
    app: tauri::AppHandle,
) -> Result<VaultBackupOutcome, String> {
    let transfer = unique_transfer_directory(&app, "backup")?;
    let result = async {
        let vault_root = active_vault_path(&app)?;
        let database_snapshot = transfer.join("database.sqlite");
        let archive_path = transfer.join("backup.ganbaru-backup");
        create_database_snapshot(&app, &database_snapshot).await?;
        create_backup_archive(&vault_root, &database_snapshot, &archive_path)?;
        let file_name = backup_file_name();
        let archive_path = path_to_string(&archive_path, "backup archive")?;
        let saved_name = app.mobile_documents().save_file_download(
            &archive_path,
            &file_name,
            BACKUP_MAX_BYTES,
            &[BACKUP_EXTENSION],
            BACKUP_MIME_TYPE,
            "Ganbaru backup",
        )?;
        Ok(VaultBackupOutcome {
            file_name: saved_name,
            destination: "downloads",
        })
    }
    .await;
    let _ = fs::remove_dir_all(&transfer);
    result
}

#[cfg(target_os = "android")]
#[tauri::command]
/// Pick, validate, and transactionally restore a portable Android vault backup.
pub async fn vault_pick_and_restore_backup(
    app: tauri::AppHandle,
) -> Result<Option<VaultInfo>, String> {
    let transition = super::quiescence::reserve_vault_transition()?;
    let expected_path = super::active_vault_path(&app)?;
    let expected_vault_id = super::active_vault_id(&app)?;
    let ownership_manager = app.state::<super::ownership::VaultOwnershipManager>();
    ownership_manager.require_writable(&expected_vault_id)?;
    let expected_ownership = ownership_manager.status(&expected_vault_id)?;
    let transfer = unique_transfer_directory(&app, "restore")?;
    let archive_path = transfer.join("selected.ganbaru-backup");
    let result = async {
        let target = default_vault_path(&app)?;
        let preparation_app = app.clone();
        let preparation_target = target.clone();
        let preparation_transition = transition.clone();
        let prepared = tauri::async_runtime::spawn_blocking(
            move || -> Result<Option<(PathBuf, PathBuf)>, String> {
                let _transition = preparation_transition;
                let parent = preparation_target
                    .parent()
                    .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
                fs::create_dir_all(parent)
                    .map_err(|error| format!("create app data directory: {error}"))?;
                let staging = parent.join(".ganbaru.restore");
                let rollback = parent.join(".ganbaru.rollback");
                recover_interrupted_restore(&preparation_target, &rollback)?;
                if staging.exists() {
                    fs::remove_dir_all(&staging)
                        .map_err(|error| format!("remove stale restore staging: {error}"))?;
                }
                let archive_path_string = path_to_string(&archive_path, "selected backup")?;
                let selected = preparation_app.mobile_documents().pick_document_to_path(
                    &archive_path_string,
                    BACKUP_MAX_BYTES,
                    &[BACKUP_EXTENSION],
                    &[BACKUP_MIME_TYPE, "application/octet-stream"],
                    "Ganbaru backup",
                )?;
                if selected.is_none() {
                    return Ok(None);
                }
                extract_backup_archive(&archive_path, &staging, None)?
                    .report("Backup restore", "outside the vault layout");
                Ok(Some((staging, rollback)))
            },
        )
        .await
        .map_err(|error| format!("backup preparation worker failed: {error}"))??;
        let Some((staging, rollback)) = prepared else {
            return Ok(None);
        };
        validate_restored_vault(&staging).await?;
        let quiescence = super::quiescence::begin_reserved_quiescence(
            &app,
            transition.clone(),
            super::quiescence::RunningTerminals::Block,
        )
        .await?;
        let activation_app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let ownership_manager =
                activation_app.state::<super::ownership::VaultOwnershipManager>();
            ownership_manager.require_writable(&expected_vault_id)?;
            let ownership = ownership_manager.status(&expected_vault_id)?;
            if super::active_vault_path(&activation_app)? != expected_path
                || super::active_vault_id(&activation_app)? != expected_vault_id
                || ownership.generation != expected_ownership.generation
            {
                return Err(
                    "active vault authority changed while preparing backup restore".to_string(),
                );
            }
            ensure_vault_skeleton(&staging)?;
            replace_vault(&staging, &target, &rollback)?;
            let info = vault_info_from_path(&target)?;
            select_quiesced_vault(&activation_app, &info, &quiescence)?;
            quiescence.finish()?;
            Ok(Some(info))
        })
        .await
        .map_err(|error| format!("backup activation worker failed: {error}"))?
    }
    .await;
    let cleanup = tauri::async_runtime::spawn_blocking(move || {
        let _transition = transition;
        let target = default_vault_path(&app)?;
        let parent = target
            .parent()
            .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
        let staging = parent.join(".ganbaru.restore");
        if staging.exists() {
            fs::remove_dir_all(staging)
                .map_err(|error| format!("remove backup restore staging: {error}"))?;
        }
        fs::remove_dir_all(transfer)
            .map_err(|error| format!("remove backup restore transfer: {error}"))
    })
    .await
    .map_err(|error| format!("backup cleanup worker failed: {error}"))
    .and_then(|cleanup| cleanup);
    match (result, cleanup) {
        (Ok(info), Ok(())) => Ok(info),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => {
            Err(format!("{error}; backup cleanup also failed: {cleanup}"))
        }
    }
}

#[cfg(all(not(target_os = "android"), any(test, target_os = "ios")))]
#[tauri::command]
/// Report that portable mobile backup is unavailable outside Android.
pub async fn vault_backup_to_downloads() -> Result<VaultBackupOutcome, String> {
    Err("portable mobile backups are only available on Android".to_string())
}

#[cfg(all(not(target_os = "android"), any(test, target_os = "ios")))]
#[tauri::command]
/// Report that portable mobile restore is unavailable outside Android.
pub async fn vault_pick_and_restore_backup() -> Result<Option<VaultInfo>, String> {
    Err("portable mobile backup restore is only available on Android".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ganbaru-vault-backup-test-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn safe_archive_paths_reject_escape_and_absolute_paths() {
        assert_eq!(
            safe_archive_path(Path::new("notes/page.md")).unwrap(),
            Path::new("notes/page.md")
        );
        assert!(safe_archive_path(Path::new("../vault.json")).is_err());
        assert!(safe_archive_path(Path::new("/vault.json")).is_err());
        assert!(safe_archive_path(Path::new(".")).is_err());
    }

    #[cfg(desktop)]
    #[test]
    fn first_link_preserves_desktop_data_in_a_visible_sibling_folder() {
        let parent = Path::new("/documents");
        let preserved = preserved_handoff_path(parent, &parent.join("Ganbaru"), "transfer-123");

        assert_eq!(
            preserved,
            parent.join("Ganbaru before linking transfer-123")
        );
    }

    #[test]
    fn replaced_folder_is_independent_only_when_it_holds_another_or_unreadable_vault() {
        let parent = unique_test_path("independent");
        let target = parent.join("Ganbaru");
        assert!(!replaces_independent_vault(&target, "vault-linked"));

        fs::create_dir_all(&target).unwrap();
        let linked = crate::vault::initialize_vault(&target).unwrap();
        assert!(!replaces_independent_vault(&target, &linked.vault_id));
        assert!(replaces_independent_vault(&target, "vault-other"));

        fs::write(target.join("vault.json"), "invalid json").unwrap();
        assert!(replaces_independent_vault(&target, &linked.vault_id));
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn backup_archive_uses_consistent_database_snapshot() {
        let root = unique_test_path("root");
        let snapshot = unique_test_path("snapshot.sqlite");
        let archive_path = unique_test_path("backup.ganbaru-backup");
        fs::create_dir_all(root.join("notes")).unwrap();
        fs::write(root.join("vault.json"), b"vault").unwrap();
        fs::write(root.join(APP_SQLITE_FILE), b"live database").unwrap();
        fs::write(root.join("ganbaru.sqlite-wal"), b"wal").unwrap();
        fs::write(root.join("notes/page.md"), b"note").unwrap();
        fs::write(&snapshot, b"snapshot database").unwrap();

        create_backup_archive(&root, &snapshot, &archive_path).unwrap();
        let file = fs::File::open(&archive_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut database = String::new();
        archive
            .by_name(APP_SQLITE_FILE)
            .unwrap()
            .read_to_string(&mut database)
            .unwrap();
        assert_eq!(database, "snapshot database");
        assert!(archive.by_name("ganbaru.sqlite-wal").is_err());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(snapshot);
        let _ = fs::remove_file(archive_path);
    }

    #[test]
    fn restore_rejects_zip_slip_entries() {
        let archive_path = unique_test_path("unsafe.ganbaru-backup");
        let destination = unique_test_path("destination");
        let file = fs::File::create(&archive_path).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("../outside", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"unsafe").unwrap();
        writer.finish().unwrap();

        assert!(extract_backup_archive(&archive_path, &destination, None).is_err());
        assert!(!destination.join("../outside").exists());
        let _ = fs::remove_file(archive_path);
        let _ = fs::remove_dir_all(destination);
    }

    fn write_test_archive(path: &Path, entries: &[(&str, &[u8])]) {
        let mut writer = ZipWriter::new(fs::File::create(path).unwrap());
        for (name, contents) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
    }

    fn test_manifest(vault_id: &str) -> Vec<u8> {
        format!(r#"{{"app":"ganbaru","schemaVersion":1,"vaultId":"{vault_id}","displayName":"Vault","createdAt":"2026-10-20T00:00:00Z"}}"#).into_bytes()
    }

    #[test]
    fn restore_checks_identity_and_declared_size_before_writing_anything() {
        let payload = vec![0_u8; 4 * 1024 * 1024];
        let manifest = test_manifest("vault-a");
        let archive_path = unique_test_path("identity.ganbaru-backup");
        write_test_archive(
            &archive_path,
            &[
                ("vault.json", manifest.as_slice()),
                (APP_SQLITE_FILE, b"database"),
                ("notes/payload.bin", payload.as_slice()),
            ],
        );
        let destination = unique_test_path("identity-destination");
        assert_eq!(
            extract_backup_archive(&archive_path, &destination, Some("vault-b")).unwrap_err(),
            DIFFERENT_VAULT_ERROR
        );
        assert!(!destination.exists());
        assert!(
            extract_backup_archive_within(&archive_path, &destination, None, 1024 * 1024)
                .unwrap_err()
                .contains("size limit")
        );
        assert!(!destination.exists());
        extract_backup_archive(&archive_path, &destination, Some("vault-a")).unwrap();
        assert_eq!(
            fs::metadata(destination.join("notes/payload.bin"))
                .unwrap()
                .len(),
            payload.len() as u64
        );
        let _ = fs::remove_dir_all(&destination);

        for entries in [
            vec![(APP_SQLITE_FILE, b"database".as_slice())],
            vec![("vault.json", manifest.as_slice())],
            vec![
                ("vault.json", b"not a marker".as_slice()),
                (APP_SQLITE_FILE, b"database".as_slice()),
            ],
        ] {
            write_test_archive(&archive_path, &entries);
            assert!(extract_backup_archive(&archive_path, &destination, None).is_err());
            assert!(!destination.exists());
        }
        let _ = fs::remove_file(archive_path);
    }

    #[test]
    fn interrupted_restore_recovers_only_copy() {
        let parent = unique_test_path("interrupted");
        let target = parent.join("Ganbaru");
        let rollback = parent.join(".ganbaru.rollback");
        fs::create_dir_all(&rollback).unwrap();
        fs::write(rollback.join("vault.json"), b"current").unwrap();

        recover_interrupted_restore(&target, &rollback).unwrap();

        assert_eq!(fs::read(target.join("vault.json")).unwrap(), b"current");
        assert!(!rollback.exists());
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn completed_restore_discards_old_rollback() {
        let parent = unique_test_path("completed");
        let target = parent.join("Ganbaru");
        let rollback = parent.join(".ganbaru.rollback");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&rollback).unwrap();
        fs::write(target.join("vault.json"), b"restored").unwrap();
        fs::write(rollback.join("vault.json"), b"old").unwrap();

        recover_interrupted_restore(&target, &rollback).unwrap();

        assert_eq!(fs::read(target.join("vault.json")).unwrap(), b"restored");
        assert!(!rollback.exists());
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn bundle_layout_admits_the_vault_skeleton_and_project_folders_only() {
        for directory in crate::vault::VAULT_SKELETON_DIRECTORIES {
            let path = Path::new(directory);
            for ancestor in path
                .ancestors()
                .filter(|ancestor| !ancestor.as_os_str().is_empty())
            {
                assert!(
                    is_bundle_layout_entry(ancestor, true),
                    "skeleton directory '{}' is outside the bundle layout",
                    ancestor.display()
                );
            }
        }
        for (path, directory) in [
            ("vault.json", false),
            ("config.json", false),
            (APP_SQLITE_FILE, false),
            ("notes/daily/2026-10-18.md", false),
            ("assets/chat/attachments/context.txt", false),
            ("projects/project-a", true),
            ("projects/project-a/.git/config", false),
            ("projects/project-a/.claude", true),
        ] {
            assert!(is_bundle_layout_entry(Path::new(path), directory), "{path}");
        }
        for (path, directory) in [
            (".git", true),
            (".git/config", false),
            (".claude/settings.json", false),
            (".codex", true),
            (".vscode/settings.json", false),
            ("README.md", false),
            ("notes", false),
            ("vault.json", true),
            ("projects/stray-file", false),
            ("unknown/file.txt", false),
        ] {
            assert!(
                !is_bundle_layout_entry(Path::new(path), directory),
                "{path}"
            );
        }
    }

    #[test]
    fn database_sidecars_are_recognized_by_name() {
        assert!(is_database_sidecar(Path::new("ganbaru.sqlite-wal")));
        assert!(is_database_sidecar(Path::new("ganbaru.sqlite-shm")));
        assert!(is_database_sidecar(Path::new("ganbaru.sqlite-journal")));
        assert!(!is_database_sidecar(Path::new(APP_SQLITE_FILE)));
        assert!(!is_database_sidecar(Path::new("ganbaru.sqlite.bak")));
    }

    #[test]
    fn replacement_keeps_the_previous_folder_when_a_local_entry_cannot_move() {
        let parent = unique_test_path("stranded");
        let target = parent.join("Ganbaru");
        let rollback = parent.join(".ganbaru.rollback");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&rollback).unwrap();
        fs::write(target.join("README.md"), b"replacement").unwrap();
        fs::write(rollback.join("README.md"), b"local").unwrap();
        fs::write(rollback.join("todo.txt"), b"local todo").unwrap();

        finish_replacement(&target, &rollback).unwrap();

        assert!(!rollback.exists());
        assert_eq!(fs::read(target.join("README.md")).unwrap(), b"replacement");
        assert_eq!(fs::read(target.join("todo.txt")).unwrap(), b"local todo");
        let kept = fs::read_dir(&parent)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path != &target)
            .expect("the previous folder should stay visible");
        assert!(
            kept.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("Ganbaru before replacement ")
        );
        assert_eq!(fs::read(kept.join("README.md")).unwrap(), b"local");
        let _ = fs::remove_dir_all(parent);
    }

    #[cfg(all(unix, desktop))]
    #[tokio::test]
    async fn received_bundle_keeps_to_the_vault_layout_and_project_git_stays_inert() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let parent = unique_test_path("bundle-layout");
        let source = parent.join("sender");
        let staging = parent.join("staging");
        let target = parent.join("receiver");
        let rollback = parent.join("rollback");
        let snapshot = parent.join("snapshot.sqlite");
        let archive_path = parent.join("handoff.zip");
        let marker = parent.join("git-hook-ran");
        let hook = parent.join("hook.sh");
        fs::create_dir_all(&source).unwrap();
        let source_info = super::super::initialize_vault(&source).unwrap();
        fs::write(
            &hook,
            format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();

        let project = source.join("projects/received-project");
        fs::create_dir_all(&project).unwrap();
        let git = |root: &Path, arguments: &[&str]| {
            let status = Command::new("git")
                .args(arguments)
                .current_dir(root)
                .stdin(std::process::Stdio::null())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap()
                .status;
            assert!(status.success(), "git {arguments:?} failed");
        };
        git(&project, &["init", "-q"]);
        git(
            &project,
            &["config", "core.fsmonitor", hook.to_str().unwrap()],
        );
        git(
            &project,
            &["config", "filter.planted.clean", hook.to_str().unwrap()],
        );
        git(
            &project,
            &["config", "filter.planted.smudge", hook.to_str().unwrap()],
        );
        fs::write(project.join(".gitattributes"), "* filter=planted\n").unwrap();
        fs::write(project.join("tracked.txt"), "tracked\n").unwrap();
        fs::remove_file(&marker).ok();

        for (relative, contents) in [
            (".git/config", "[core]\n\tfsmonitor = planted\n"),
            (".claude/settings.json", "{}"),
            ("projects/stray-file", "stray"),
        ] {
            let path = source.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        create_handoff_archive(&source, &snapshot, &archive_path)
            .await
            .unwrap();
        let mut archive = ZipArchive::new(fs::File::open(&archive_path).unwrap()).unwrap();
        for name in [
            ".git/config",
            ".claude/settings.json",
            "projects/stray-file",
        ] {
            assert!(archive.by_name(name).is_err(), "{name} left the sender");
        }
        assert!(
            archive
                .by_name("projects/received-project/.git/config")
                .is_ok()
        );
        drop(archive);

        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&archive_path)
            .unwrap();
        let mut writer = ZipWriter::new_append(file).unwrap();
        for name in [
            ".git/config",
            ".claude/settings.json",
            "projects/stray-file",
        ] {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer
                .write_all(b"[core]\n\tfsmonitor = planted\n")
                .unwrap();
        }
        writer.finish().unwrap();

        fs::create_dir_all(&target).unwrap();
        super::super::initialize_vault(&target).unwrap();
        fs::write(target.join("README.md"), b"local notes about this folder").unwrap();
        fs::create_dir_all(target.join(".git")).unwrap();
        fs::write(target.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
        fs::write(target.join(format!("{APP_SQLITE_FILE}-wal")), b"stale wal").unwrap();

        let info = stage_handoff_archive(&archive_path, &staging, &source_info.vault_id)
            .await
            .unwrap();
        assert_eq!(info.vault_id, source_info.vault_id);
        for name in [".git", ".claude", "projects/stray-file"] {
            assert!(!staging.join(name).exists(), "{name} was extracted");
        }
        replace_vault(&staging, &target, &rollback).unwrap();

        assert!(!rollback.exists());
        assert_eq!(
            fs::read(target.join("README.md")).unwrap(),
            b"local notes about this folder"
        );
        assert_eq!(
            fs::read(target.join(".git/HEAD")).unwrap(),
            b"ref: refs/heads/main\n"
        );
        assert!(!target.join(".claude").exists());
        assert!(!target.join(format!("{APP_SQLITE_FILE}-wal")).exists());

        let received = target.join("projects/received-project");
        let output = ganbaru_chat::git::hardened::internal_read_blocking(
            &received,
            &["status", "--porcelain=v2", "--untracked-files=all"],
            None,
            1 << 20,
        )
        .unwrap();
        assert!(output.status.success());
        assert!(
            !marker.exists(),
            "received Git configuration ran a command through internal Git"
        );

        git(&received, &["status", "--porcelain"]);
        assert!(
            marker.exists(),
            "the planted configuration should run under unhardened Git"
        );
        fs::remove_dir_all(parent).unwrap();
    }

    #[tokio::test]
    async fn vacuum_database_creates_a_consistent_readable_snapshot() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

        let source = unique_test_path("source.sqlite");
        let snapshot = unique_test_path("vacuum.sqlite");
        let options = SqliteConnectOptions::new()
            .filename(&source)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE backup_test (value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO backup_test (value) VALUES ('preserved')")
            .execute(&pool)
            .await
            .unwrap();

        vacuum_database(&pool, &snapshot).await.unwrap();
        pool.close().await;

        let snapshot_options = SqliteConnectOptions::new().filename(&snapshot);
        let snapshot_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(snapshot_options)
            .await
            .unwrap();
        let value: String = sqlx::query_scalar("SELECT value FROM backup_test")
            .fetch_one(&snapshot_pool)
            .await
            .unwrap();
        assert_eq!(value, "preserved");
        snapshot_pool.close().await;

        let _ = fs::remove_file(source);
        let _ = fs::remove_file(snapshot);
    }

    #[tokio::test]
    async fn whole_vault_snapshot_restores_database_and_managed_files() {
        let parent = unique_test_path("whole-vault");
        let source = parent.join("source");
        let staging = parent.join("staging");
        let target = parent.join("receiver");
        let rollback = parent.join("rollback");
        let snapshot = parent.join("snapshot.sqlite");
        let archive_path = parent.join("handoff.ganbaru-backup");
        fs::create_dir_all(&source).unwrap();
        let source_info = super::super::initialize_vault(&source).unwrap();

        let source_registry = ganbaru_db::DatabasePoolRegistry::default();
        let source_pool = source_registry
            .connect_path(database_path(&source))
            .await
            .unwrap();
        for statement in [
            "INSERT INTO calendar_events (id, title, start_time, end_time) VALUES ('handoff-calendar', 'Portable calendar', '2026-09-14T09:00:00Z', '2026-09-14T10:00:00Z')",
            "INSERT INTO calendar_event_alarms (id, event_id, trigger_value) VALUES ('handoff-alarm', 'handoff-calendar', '-PT10M')",
            "INSERT INTO calendar_event_pomodoro_configs (event_id, rhythm_kind, rhythm_source, preset_key) VALUES ('handoff-calendar', 'count', 'preset', 'balanced')",
            "INSERT INTO pomodoro_runs (id, event_id, original_event_id, event_date, planned_start, planned_end, started_at, ended_at, end_reason, rhythm_kind, rhythm_source, preset_key, last_heartbeat) VALUES ('handoff-run', 'handoff-calendar', 'handoff-calendar', '2026-09-14', '2026-09-14T09:00:00Z', '2026-09-14T10:00:00Z', '2026-09-14T09:00:00Z', '2026-09-14T09:45:00Z', 'completed', 'count', 'preset', 'balanced', '2026-09-14T09:45:00Z')",
            "INSERT INTO project_tasks (id, project_id, section_id, status_id, title) VALUES ('handoff-task', 'project-routine-learning', 'section-routine-learning-general', 'status-routine-learning-todo', 'Portable project task')",
            "INSERT INTO notes_pages (id, parent_type, title) VALUES ('handoff-note', 'workspace', 'Portable note')",
            "INSERT INTO chat_conversations (id, project_id, conversation_kind, last_activity_at, created_at, updated_at) VALUES ('handoff-conversation', 'project-routine-learning', 'channel', '2026-09-14T09:00:00Z', '2026-09-14T09:00:00Z', '2026-09-14T09:00:00Z')",
            "INSERT INTO chat_channels (id, project_id, conversation_id, name, created_at, updated_at) VALUES ('handoff-channel', 'project-routine-learning', 'handoff-conversation', 'Portable chat', '2026-09-14T09:00:00Z', '2026-09-14T09:00:00Z')",
            "INSERT INTO quick_notes (id, title, body_plain_text, order_key) VALUES ('handoff-quick-note', 'Portable quick note', 'Portable quick-note body', 'a0')",
            "INSERT INTO themes (id, display_name, blend_canvas, seed_blend_canvas, derivation_engine_version, created_at_ms, updated_at_ms, icon_label, seed_icon_label) VALUES ('handoff-theme', 'Portable theme', '{}', '{}', 1, 1, 1, 'dark', 'dark')",
            "INSERT INTO distractions_usage_samples (id, source_type, source_key, display_name, started_at_ms, elapsed_seconds, local_date, created_at_ms) VALUES ('handoff-usage', 'mobile-app', 'app.example', 'Portable usage', 1, 45, '2026-09-14', 1)",
            "INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms) VALUES ('handoff-playlist', 'Portable playlist', 1, 1)",
            "INSERT INTO music_library_items (id, identity_key, source_kind, original_title, discovered_at_ms, updated_at_ms) VALUES ('handoff-track', 'local:portable-track', 'local-file', 'Portable track', 1, 1)",
            "INSERT INTO music_playlist_memberships (id, playlist_id, item_id, position, created_at_ms, updated_at_ms) VALUES ('handoff-membership', 'handoff-playlist', 'handoff-track', 0, 1, 1)",
        ] {
            sqlx::query(statement).execute(&source_pool).await.unwrap();
        }
        fs::write(
            source.join("config.json"),
            br#"{"preferences":{"language":"es"},"theme":"handoff-theme"}"#,
        )
        .unwrap();
        let managed_files = [
            (
                "assets/chat/attachments/context.txt",
                b"chat attachment".as_slice(),
            ),
            (
                "assets/notes/files/note.txt",
                b"notes attachment".as_slice(),
            ),
            ("assets/project-icons/icon.txt", b"project icon".as_slice()),
            (
                "projects/project-routine-learning/brief.md",
                b"project document".as_slice(),
            ),
        ];
        for (relative, contents) in managed_files {
            let path = source.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let external_music = parent.join("external-music.mp3");
        fs::write(&external_music, b"external music bytes").unwrap();

        let live_wal = source.join(format!("{APP_SQLITE_FILE}-wal"));
        let live_shm = source.join(format!("{APP_SQLITE_FILE}-shm"));
        assert!(live_wal.exists(), "source should have an active WAL");
        assert!(live_shm.exists(), "source should have active shared memory");

        vacuum_database(&source_pool, &snapshot).await.unwrap();
        create_backup_archive(&source, &snapshot, &archive_path).unwrap();

        let file = fs::File::open(&archive_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        assert!(archive.by_name(&format!("{APP_SQLITE_FILE}-wal")).is_err());
        assert!(archive.by_name(&format!("{APP_SQLITE_FILE}-shm")).is_err());
        drop(archive);

        extract_backup_archive(&archive_path, &staging, None).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&staging).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "restore staging is private");
        }
        validate_restored_vault(&staging).await.unwrap();
        replace_vault(&staging, &target, &rollback).unwrap();

        let restored_info = vault_info_from_path(&target).unwrap();
        assert_eq!(restored_info.vault_id, source_info.vault_id);
        let restored_registry = ganbaru_db::DatabasePoolRegistry::default();
        let restored_pool = restored_registry
            .connect_path(database_path(&target))
            .await
            .unwrap();
        for (table, key, identifier) in [
            ("calendar_events", "id", "handoff-calendar"),
            ("calendar_event_alarms", "id", "handoff-alarm"),
            (
                "calendar_event_pomodoro_configs",
                "event_id",
                "handoff-calendar",
            ),
            ("pomodoro_runs", "id", "handoff-run"),
            ("project_tasks", "id", "handoff-task"),
            ("notes_pages", "id", "handoff-note"),
            ("chat_channels", "id", "handoff-channel"),
            ("quick_notes", "id", "handoff-quick-note"),
            ("themes", "id", "handoff-theme"),
            ("distractions_usage_samples", "id", "handoff-usage"),
            ("music_playlists", "id", "handoff-playlist"),
            ("music_library_items", "id", "handoff-track"),
            ("music_playlist_memberships", "id", "handoff-membership"),
        ] {
            let query = format!("SELECT COUNT(*) FROM {table} WHERE {key} = ?");
            let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(query.as_str()))
                .bind(identifier)
                .fetch_one(&restored_pool)
                .await
                .unwrap();
            assert_eq!(count, 1, "missing portable row in {table}");
        }
        assert_eq!(
            fs::read(target.join("config.json")).unwrap(),
            br#"{"preferences":{"language":"es"},"theme":"handoff-theme"}"#
        );
        for (relative, contents) in managed_files {
            assert_eq!(fs::read(target.join(relative)).unwrap(), contents);
        }
        assert!(!target.join("external-music.mp3").exists());

        restored_registry.close_all().await.unwrap();
        source_registry.close_all().await.unwrap();
        fs::remove_dir_all(parent).unwrap();
    }

    #[tokio::test]
    async fn restored_vault_with_an_extra_trigger_is_rejected() {
        let staging = unique_test_path("restore-extra-trigger");
        fs::create_dir_all(&staging).unwrap();
        super::super::initialize_vault(&staging).unwrap();
        let registry = ganbaru_db::DatabasePoolRegistry::default();
        let pool = registry
            .connect_path(database_path(&staging))
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TRIGGER restored_payload AFTER INSERT ON notes_link_facts_state
             BEGIN DELETE FROM notes_search_index_state; END",
        )
        .execute(&pool)
        .await
        .unwrap();
        registry.close_all().await.unwrap();

        let error = validate_restored_vault(&staging).await.unwrap_err();

        assert!(error.contains("schema differs"), "{error}");
        fs::remove_dir_all(&staging).unwrap();
    }

    #[tokio::test]
    async fn desktop_bundle_activates_an_owner_and_refreshes_a_read_only_replica() {
        use crate::vault::ownership::VaultOwnershipManager;

        let parent = unique_test_path("desktop-android-handoff");
        let source = parent.join("desktop");
        let owner_staging = parent.join("owner-staging");
        let owner_target = parent.join("owner-target");
        let refresh_staging = parent.join("refresh-staging");
        let refresh_target = parent.join("refresh-target");
        let snapshot = parent.join("snapshot.sqlite");
        let archive = parent.join("handoff.zip");
        let refresh_snapshot = parent.join("refresh-snapshot.sqlite");
        let refresh_archive = parent.join("refresh.zip");
        fs::create_dir_all(&source).unwrap();
        let source_info = super::super::initialize_vault(&source).unwrap();
        let registry = ganbaru_db::DatabasePoolRegistry::default();
        let pool = registry.connect_path(database_path(&source)).await.unwrap();
        sqlx::query("INSERT INTO notes_link_facts_state (key, value) VALUES ('handoff_h04_probe', 'desktop')")
            .execute(&pool)
            .await
            .unwrap();
        let asset = source.join("assets/notes/files/h04.txt");
        fs::create_dir_all(asset.parent().unwrap()).unwrap();
        fs::write(&asset, b"desktop asset").unwrap();
        registry.close_all().await.unwrap();

        create_handoff_archive(&source, &snapshot, &archive)
            .await
            .unwrap();
        stage_handoff_archive(&archive, &owner_staging, &source_info.vault_id)
            .await
            .unwrap();

        let desktop_ownership = VaultOwnershipManager::default();
        desktop_ownership
            .initialize_from_path(parent.join("desktop-ownership.json"), "desktop".to_string())
            .unwrap();
        let phone_ownership = VaultOwnershipManager::default();
        phone_ownership
            .initialize_from_path(parent.join("phone-ownership.json"), "phone".to_string())
            .unwrap();
        phone_ownership
            .register_remote_owner(&source_info.vault_id, "desktop".to_string(), 0)
            .unwrap();
        desktop_ownership
            .begin_outgoing(
                &source_info.vault_id,
                0,
                "ownership-transfer".to_string(),
                "phone".to_string(),
            )
            .unwrap();
        let generation = desktop_ownership
            .commit_outgoing(&source_info.vault_id, "ownership-transfer")
            .unwrap();
        phone_ownership
            .accept_incoming_grant(
                &source_info.vault_id,
                "ownership-transfer".to_string(),
                "desktop".to_string(),
                generation,
            )
            .unwrap();
        replace_vault(
            &owner_staging,
            &owner_target,
            &parent.join("owner-rollback"),
        )
        .unwrap();
        phone_ownership
            .finalize_incoming(&source_info.vault_id, "ownership-transfer", generation)
            .unwrap();
        desktop_ownership
            .finish_outgoing_acknowledgement(
                &source_info.vault_id,
                "ownership-transfer",
                generation,
            )
            .unwrap();
        assert!(
            !desktop_ownership
                .status(&source_info.vault_id)
                .unwrap()
                .can_write
        );
        assert!(
            phone_ownership
                .status(&source_info.vault_id)
                .unwrap()
                .can_write
        );
        assert_eq!(
            fs::read(owner_target.join("assets/notes/files/h04.txt")).unwrap(),
            b"desktop asset"
        );

        let refresh_desktop = VaultOwnershipManager::default();
        refresh_desktop
            .initialize_from_path(parent.join("refresh-desktop.json"), "desktop".to_string())
            .unwrap();
        let refresh_phone = VaultOwnershipManager::default();
        refresh_phone
            .initialize_from_path(parent.join("refresh-phone.json"), "phone".to_string())
            .unwrap();
        refresh_phone
            .register_remote_owner(&source_info.vault_id, "desktop".to_string(), 0)
            .unwrap();
        let initial_staging = parent.join("initial-refresh-staging");
        stage_handoff_archive(&archive, &initial_staging, &source_info.vault_id)
            .await
            .unwrap();
        replace_vault(
            &initial_staging,
            &refresh_target,
            &parent.join("initial-refresh-rollback"),
        )
        .unwrap();

        let source_registry = ganbaru_db::DatabasePoolRegistry::default();
        let source_pool = source_registry
            .connect_path(database_path(&source))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE notes_link_facts_state SET value = 'refreshed' WHERE key = 'handoff_h04_probe'",
        )
        .execute(&source_pool)
        .await
        .unwrap();
        source_registry.close_all().await.unwrap();
        fs::write(&asset, b"refreshed asset").unwrap();
        create_handoff_archive(&source, &refresh_snapshot, &refresh_archive)
            .await
            .unwrap();
        stage_handoff_archive(&refresh_archive, &refresh_staging, &source_info.vault_id)
            .await
            .unwrap();
        replace_vault(
            &refresh_staging,
            &refresh_target,
            &parent.join("refresh-rollback"),
        )
        .unwrap();

        assert!(
            refresh_desktop
                .status(&source_info.vault_id)
                .unwrap()
                .can_write
        );
        assert!(
            !refresh_phone
                .status(&source_info.vault_id)
                .unwrap()
                .can_write
        );
        assert_eq!(
            fs::read(refresh_target.join("assets/notes/files/h04.txt")).unwrap(),
            b"refreshed asset"
        );
        let restored = ganbaru_db::DatabasePoolRegistry::default();
        let restored_pool = restored
            .connect_path_read_only(database_path(&refresh_target))
            .await
            .unwrap();
        let value: String = sqlx::query_scalar(
            "SELECT value FROM notes_link_facts_state WHERE key = 'handoff_h04_probe'",
        )
        .fetch_one(&restored_pool)
        .await
        .unwrap();
        assert_eq!(value, "refreshed");
        restored.close_all().await.unwrap();
        fs::remove_dir_all(parent).unwrap();
    }

    #[tokio::test]
    async fn android_owner_refreshes_and_returns_the_whole_vault_to_desktop() {
        use crate::vault::ownership::VaultOwnershipManager;

        let parent = unique_test_path("android-desktop-return");
        let source = parent.join("android");
        let desktop = parent.join("desktop");
        let refresh_staging = parent.join("refresh-staging");
        let return_staging = parent.join("return-staging");
        fs::create_dir_all(&source).unwrap();
        let info = super::super::initialize_vault(&source).unwrap();
        let registry = ganbaru_db::DatabasePoolRegistry::default();
        let pool = registry.connect_path(database_path(&source)).await.unwrap();
        sqlx::query("INSERT INTO notes_link_facts_state (key, value) VALUES ('handoff_h05_probe', 'android edit')")
            .execute(&pool)
            .await
            .unwrap();
        registry.close_all().await.unwrap();
        let asset = source.join("assets/chat/attachments/android-return.txt");
        fs::create_dir_all(asset.parent().unwrap()).unwrap();
        fs::write(&asset, b"Android managed asset").unwrap();

        let phone = VaultOwnershipManager::default();
        phone
            .initialize_from_path(parent.join("phone-owner.json"), "phone".to_string())
            .unwrap();
        let desktop_owner = VaultOwnershipManager::default();
        desktop_owner
            .initialize_from_path(parent.join("desktop-owner.json"), "desktop".to_string())
            .unwrap();
        desktop_owner
            .register_remote_owner(&info.vault_id, "phone".to_string(), 0)
            .unwrap();

        let refresh_snapshot = parent.join("refresh.sqlite");
        let refresh_archive = parent.join("refresh.zip");
        create_handoff_archive(&source, &refresh_snapshot, &refresh_archive)
            .await
            .unwrap();
        stage_handoff_archive(&refresh_archive, &refresh_staging, &info.vault_id)
            .await
            .unwrap();
        replace_vault(&refresh_staging, &desktop, &parent.join("refresh-rollback")).unwrap();
        assert!(phone.status(&info.vault_id).unwrap().can_write);
        assert!(!desktop_owner.status(&info.vault_id).unwrap().can_write);

        let source_registry = ganbaru_db::DatabasePoolRegistry::default();
        let source_pool = source_registry
            .connect_path(database_path(&source))
            .await
            .unwrap();
        sqlx::query("UPDATE notes_link_facts_state SET value = 'returned edit' WHERE key = 'handoff_h05_probe'")
            .execute(&source_pool)
            .await
            .unwrap();
        source_registry.close_all().await.unwrap();
        let return_snapshot = parent.join("return.sqlite");
        let return_archive = parent.join("return.zip");
        create_handoff_archive(&source, &return_snapshot, &return_archive)
            .await
            .unwrap();
        stage_handoff_archive(&return_archive, &return_staging, &info.vault_id)
            .await
            .unwrap();

        phone
            .begin_outgoing(
                &info.vault_id,
                0,
                "return-transfer".to_string(),
                "desktop".to_string(),
            )
            .unwrap();
        let generation = phone
            .commit_outgoing(&info.vault_id, "return-transfer")
            .unwrap();
        desktop_owner
            .accept_incoming_grant(
                &info.vault_id,
                "return-transfer".to_string(),
                "phone".to_string(),
                generation,
            )
            .unwrap();
        replace_vault(&return_staging, &desktop, &parent.join("return-rollback")).unwrap();
        desktop_owner
            .finalize_incoming(&info.vault_id, "return-transfer", generation)
            .unwrap();
        phone
            .finish_outgoing_acknowledgement(&info.vault_id, "return-transfer", generation)
            .unwrap();

        assert!(!phone.status(&info.vault_id).unwrap().can_write);
        assert!(desktop_owner.status(&info.vault_id).unwrap().can_write);
        assert_eq!(
            fs::read(desktop.join("assets/chat/attachments/android-return.txt")).unwrap(),
            b"Android managed asset"
        );
        let restored = ganbaru_db::DatabasePoolRegistry::default();
        let restored_pool = restored
            .connect_path_read_only(database_path(&desktop))
            .await
            .unwrap();
        let value: String = sqlx::query_scalar(
            "SELECT value FROM notes_link_facts_state WHERE key = 'handoff_h05_probe'",
        )
        .fetch_one(&restored_pool)
        .await
        .unwrap();
        assert_eq!(value, "returned edit");
        restored.close_all().await.unwrap();
        fs::remove_dir_all(parent).unwrap();
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    use crate::vault::database_rollback::{RollbackOrigin, list_rollbacks, restore_rollback};

    const PREVIOUS_MARKER: i64 = 1;
    const REPLACEMENT_MARKER: i64 = 2;

    fn unique_parent(name: &str) -> PathBuf {
        let parent = crate::vault::tests::unique_path(name);
        fs::create_dir_all(&parent).unwrap();
        parent
    }

    async fn write_marker(vault: &Path, marker: i64) {
        let pragma = match marker {
            PREVIOUS_MARKER => "PRAGMA application_id = 1",
            _ => "PRAGMA application_id = 2",
        };
        let registry = ganbaru_db::DatabasePoolRegistry::default();
        let pool = registry.connect_path(database_path(vault)).await.unwrap();
        sqlx::query(pragma).execute(&pool).await.unwrap();
        registry.close_all().await.unwrap();
    }

    async fn read_marker(vault: &Path) -> i64 {
        let registry = ganbaru_db::DatabasePoolRegistry::default();
        let pool = registry
            .connect_path_read_only(database_path(vault))
            .await
            .unwrap();
        let marker = sqlx::query_scalar("PRAGMA application_id")
            .fetch_one(&pool)
            .await
            .unwrap();
        registry.close_all().await.unwrap();
        marker
    }

    /// Creates the current vault at `parent/Ganbaru` and a staged copy of the same vault.
    async fn same_vault_pair(parent: &Path) -> (PathBuf, PathBuf) {
        let target = parent.join("Ganbaru");
        fs::create_dir_all(&target).unwrap();
        crate::vault::initialize_vault(&target).unwrap();
        write_marker(&target, PREVIOUS_MARKER).await;
        let staging = parent.join(".ganbaru.handoff-transfer.staging");
        fs::create_dir_all(&staging).unwrap();
        fs::copy(
            target.join(VAULT_MANIFEST_FILE),
            staging.join(VAULT_MANIFEST_FILE),
        )
        .unwrap();
        write_marker(&staging, REPLACEMENT_MARKER).await;
        (target, staging)
    }

    #[test]
    fn drastic_shrink_needs_a_meaningful_vault_that_loses_most_of_its_data() {
        assert!(is_drastic_shrink(MIN_FLAGGED_SHRINK_BYTES, 0));
        assert!(!is_drastic_shrink(MIN_FLAGGED_SHRINK_BYTES - 1, 0));
        let previous = 10 * MIN_FLAGGED_SHRINK_BYTES;
        assert!(!is_drastic_shrink(
            previous,
            previous / DRASTIC_SHRINK_DIVISOR
        ));
        assert!(is_drastic_shrink(
            previous,
            previous / DRASTIC_SHRINK_DIVISOR - 1
        ));
        assert!(!is_drastic_shrink(previous, previous * 2));
    }

    #[tokio::test]
    async fn activating_a_new_bundle_keeps_the_previous_database_restorable() {
        let parent = unique_parent("retain-replacement");
        let (target, staging) = same_vault_pair(&parent).await;
        assert!(!replacement_shrinks_drastically(&target, &staging).await);

        replace_vault_flagging(&staging, &target, &parent.join("rollback"), false).unwrap();

        assert_eq!(read_marker(&target).await, REPLACEMENT_MARKER);
        let rollbacks = list_rollbacks(&target).unwrap();
        assert_eq!(rollbacks.len(), 1);
        assert_eq!(rollbacks[0].origin, RollbackOrigin::Replacement);
        assert!(!rollbacks[0].flagged);
        restore_rollback(&target, &rollbacks[0].id, chrono::Utc::now())
            .await
            .unwrap();
        assert_eq!(read_marker(&target).await, PREVIOUS_MARKER);
        let _ = fs::remove_dir_all(parent);
    }

    #[tokio::test]
    async fn an_upload_that_removes_most_data_is_flagged_and_restorable_whole() {
        let parent = unique_parent("flag-shrink");
        let (target, staging) = same_vault_pair(&parent).await;
        let note = Path::new("notes").join("daily").join("kept.bin");
        let database = database_used_bytes(&database_path(&target)).await.unwrap();
        let note_size = (database + MIN_FLAGGED_SHRINK_BYTES) * DRASTIC_SHRINK_DIVISOR;
        let note_bytes = vec![7_u8; usize::try_from(note_size).unwrap()];
        fs::write(target.join(&note), &note_bytes).unwrap();
        fs::write(target.join("README.md"), b"device file").unwrap();
        assert!(replacement_shrinks_drastically(&target, &staging).await);

        replace_vault_flagging(&staging, &target, &parent.join("rollback"), true).unwrap();

        assert!(!target.join(&note).exists());
        assert_eq!(fs::read(target.join("README.md")).unwrap(), b"device file");
        let rollbacks = list_rollbacks(&target).unwrap();
        assert_eq!(rollbacks.len(), 1);
        assert!(rollbacks[0].flagged && !rollbacks[0].dismissed);
        restore_rollback(&target, &rollbacks[0].id, chrono::Utc::now())
            .await
            .unwrap();
        assert_eq!(read_marker(&target).await, PREVIOUS_MARKER);
        assert_eq!(fs::read(target.join(&note)).unwrap(), note_bytes);
        let after = list_rollbacks(&target).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].origin, RollbackOrigin::Reset);
        let _ = fs::remove_dir_all(parent);
    }

    #[tokio::test]
    async fn replacing_another_vault_keeps_no_rollback_inside_the_replacement() {
        let parent = unique_parent("retain-other");
        let (target, staging) = same_vault_pair(&parent).await;
        fs::remove_file(staging.join(VAULT_MANIFEST_FILE)).unwrap();
        let other = parent.join("other");
        fs::create_dir_all(&other).unwrap();
        crate::vault::initialize_vault(&other).unwrap();
        fs::copy(
            other.join(VAULT_MANIFEST_FILE),
            staging.join(VAULT_MANIFEST_FILE),
        )
        .unwrap();

        replace_vault_flagging(&staging, &target, &parent.join("rollback"), false).unwrap();

        assert!(list_rollbacks(&target).unwrap().is_empty());
        let _ = fs::remove_dir_all(parent);
    }
}
