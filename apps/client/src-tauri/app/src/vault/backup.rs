//! Whole-vault snapshots and activation, with portable Android backup commands.

#[cfg(target_os = "android")]
use super::active_vault_path;
use super::select_quiesced_vault;
use super::{APP_SQLITE_FILE, CONFIG_LOCK, VaultInfo};
use super::{database_path, vault_info_from_path};
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
    format!("ganbaru-ai-backup-{timestamp}.{BACKUP_EXTENSION}")
}

#[cfg(target_os = "android")]
async fn create_database_snapshot<R: Runtime>(
    app: &tauri::AppHandle<R>,
    destination: &Path,
) -> Result<(), String> {
    let pool = db::connect_sqlite(app.clone(), format!("sqlite:{APP_SQLITE_FILE}")).await?;
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
            return Err("Ganbaru AI folder contains too many backup entries".to_string());
        }
        let source = child.path();
        let relative = source
            .strip_prefix(root)
            .map_err(|_| "backup entry escaped the Ganbaru AI folder".to_string())?
            .to_path_buf();
        let depth = relative.components().count();
        if depth > BACKUP_MAX_DEPTH {
            return Err("Ganbaru AI folder is nested too deeply to back up".to_string());
        }
        let metadata = fs::symlink_metadata(&source)
            .map_err(|error| format!("inspect backup entry '{}': {error}", source.display()))?;
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
            let file_name = relative.file_name().and_then(|value| value.to_str());
            if matches!(
                file_name,
                Some("ganbaru-ai.sqlite-wal" | "ganbaru-ai.sqlite-shm")
            ) {
                continue;
            }
            *total_bytes = total_bytes.saturating_add(metadata.len());
            if *total_bytes > BACKUP_MAX_BYTES {
                return Err("Ganbaru AI folder exceeds the backup size limit".to_string());
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

fn extract_backup_archive(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = fs::File::open(archive_path).map_err(|error| format!("open backup: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("open backup archive: {error}"))?;
    if archive.len() > BACKUP_MAX_ENTRIES {
        return Err("backup contains too many entries".to_string());
    }
    fs::create_dir(destination)
        .map_err(|error| format!("create restore staging folder: {error}"))?;
    let mut total_bytes = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];

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
        loop {
            let count = entry
                .read(&mut buffer)
                .map_err(|error| format!("read backup file '{}': {error}", relative.display()))?;
            if count == 0 {
                break;
            }
            total_bytes = total_bytes.saturating_add(count as u64);
            if total_bytes > BACKUP_MAX_BYTES {
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
    Ok(())
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
    if let Err(error) = extract_backup_archive(archive_path, destination) {
        let _ = fs::remove_dir_all(destination);
        return Err(error);
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
        return Err("received bundle belongs to a different vault".to_string());
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
        ganbaru_db::validate_current_schema(&pool).await
    }
    .await;
    let close_result = registry.close_all().await;
    validation?;
    close_result?;
    Ok(info)
}

fn replace_vault(staging: &Path, target: &Path, rollback: &Path) -> Result<(), String> {
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
    if had_target {
        let _ = fs::remove_dir_all(rollback);
    }
    Ok(())
}

#[cfg(target_os = "android")]
pub(crate) fn android_handoff_staging_path(
    app: &tauri::AppHandle,
    transfer_id: &str,
) -> Result<PathBuf, String> {
    let target = default_vault_path(app)?;
    let parent = target
        .parent()
        .ok_or_else(|| "Ganbaru AI folder has no parent directory".to_string())?;
    Ok(parent.join(format!(".ganbaru-ai.handoff-{transfer_id}.staging")))
}

#[cfg(desktop)]
pub(crate) fn active_handoff_staging_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    transfer_id: &str,
) -> Result<PathBuf, String> {
    let target = super::active_vault_path(app)?;
    let parent = target
        .parent()
        .ok_or_else(|| "Ganbaru AI folder has no parent directory".to_string())?;
    Ok(parent.join(format!(".ganbaru-ai.handoff-{transfer_id}.staging")))
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

/// Activates an ownership snapshot a linked owner uploaded to this coordinator. The replaced
/// folder is kept only when it holds a different vault: the coordinator's own copy of the linked
/// vault is read-only while another device owns it, and its Quick notes changes carry forward.
#[cfg(desktop)]
pub(crate) async fn activate_uploaded_ownership<R: Runtime>(
    app: &tauri::AppHandle<R>,
    staging: &Path,
    transfer_id: &str,
    expected_vault_id: &str,
) -> Result<VaultInfo, String> {
    let target = super::active_vault_path(app)?;
    let preserve_previous = replaces_independent_vault(&target, expected_vault_id);
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

/// Whether the folder at `path` is the vault `vault_id`.
fn holds_vault(path: &Path, vault_id: &str) -> bool {
    vault_info_from_path(path).is_ok_and(|info| info.vault_id == vault_id)
}

/// Whether replacing `target` discards a vault other than `vault_id`. A folder whose marker
/// cannot be read counts as another vault, so it is kept.
#[cfg(any(desktop, test))]
fn replaces_independent_vault(target: &Path, vault_id: &str) -> bool {
    target.exists() && !holds_vault(target, vault_id)
}

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
        .ok_or_else(|| "Ganbaru AI folder has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create app data directory: {error}"))?;
    let rollback = if preserve_previous {
        preserved_handoff_path(parent, target, transfer_id)
    } else {
        parent.join(".ganbaru-ai.handoff-refresh-rollback")
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
                replace_vault(&staging, &target, &rollback)?;
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
/// imports them into the replacement. Only a replacement of the same vault carries anything.
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
    crate::sync::export_for_replacement(&database_path(target), &database_path(staging), &files)
        .await
        .map(|_| ())
        .map_err(|error| format!("keep local Quick notes changes: {error}"))
}

fn preserved_handoff_path(parent: &Path, target: &Path, transfer_id: &str) -> PathBuf {
    #[cfg(desktop)]
    {
        let folder_name = target
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Ganbaru AI");
        parent.join(format!("{folder_name} before linking {transfer_id}"))
    }
    #[cfg(mobile)]
    {
        let _ = target;
        parent.join(format!(".ganbaru-ai.handoff-previous-{transfer_id}"))
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
        fs::remove_dir_all(rollback)
            .map_err(|error| format!("remove completed restore rollback: {error}"))
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
        .ok_or_else(|| "Ganbaru AI folder has no parent directory".to_string())?;
    recover_interrupted_restore(&target, &parent.join(".ganbaru-ai.rollback"))
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
            "Ganbaru AI backup",
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
                    .ok_or_else(|| "Ganbaru AI folder has no parent directory".to_string())?;
                fs::create_dir_all(parent)
                    .map_err(|error| format!("create app data directory: {error}"))?;
                let staging = parent.join(".ganbaru-ai.restore");
                let rollback = parent.join(".ganbaru-ai.rollback");
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
                    "Ganbaru AI backup",
                )?;
                if selected.is_none() {
                    return Ok(None);
                }
                extract_backup_archive(&archive_path, &staging)?;
                Ok(Some((staging, rollback)))
            },
        )
        .await
        .map_err(|error| format!("backup preparation worker failed: {error}"))??;
        let Some((staging, rollback)) = prepared else {
            return Ok(None);
        };
        validate_restored_vault(&staging).await?;
        let quiescence =
            super::quiescence::begin_reserved_quiescence(&app, transition.clone()).await?;
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
            .ok_or_else(|| "Ganbaru AI folder has no parent directory".to_string())?;
        let staging = parent.join(".ganbaru-ai.restore");
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
        let preserved = preserved_handoff_path(parent, &parent.join("Ganbaru AI"), "transfer-123");

        assert_eq!(
            preserved,
            parent.join("Ganbaru AI before linking transfer-123")
        );
    }

    #[test]
    fn replaced_folder_is_independent_only_when_it_holds_another_or_unreadable_vault() {
        let parent = unique_test_path("independent");
        let target = parent.join("Ganbaru AI");
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
        fs::write(root.join("ganbaru-ai.sqlite-wal"), b"wal").unwrap();
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
        assert!(archive.by_name("ganbaru-ai.sqlite-wal").is_err());

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

        assert!(extract_backup_archive(&archive_path, &destination).is_err());
        assert!(!destination.join("../outside").exists());
        let _ = fs::remove_file(archive_path);
        let _ = fs::remove_dir_all(destination);
    }

    #[test]
    fn interrupted_restore_recovers_only_copy() {
        let parent = unique_test_path("interrupted");
        let target = parent.join("Ganbaru AI");
        let rollback = parent.join(".ganbaru-ai.rollback");
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
        let target = parent.join("Ganbaru AI");
        let rollback = parent.join(".ganbaru-ai.rollback");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&rollback).unwrap();
        fs::write(target.join("vault.json"), b"restored").unwrap();
        fs::write(rollback.join("vault.json"), b"old").unwrap();

        recover_interrupted_restore(&target, &rollback).unwrap();

        assert_eq!(fs::read(target.join("vault.json")).unwrap(), b"restored");
        assert!(!rollback.exists());
        let _ = fs::remove_dir_all(parent);
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

        extract_backup_archive(&archive_path, &staging).unwrap();
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
            let count: i64 = sqlx::query_scalar(&query)
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
        sqlx::query("CREATE TABLE handoff_h04_probe (value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO handoff_h04_probe (value) VALUES ('desktop')")
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
        sqlx::query("UPDATE handoff_h04_probe SET value = 'refreshed'")
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
        let value: String = sqlx::query_scalar("SELECT value FROM handoff_h04_probe")
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
        sqlx::query("CREATE TABLE handoff_h05_probe (value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO handoff_h05_probe (value) VALUES ('android edit')")
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
        sqlx::query("UPDATE handoff_h05_probe SET value = 'returned edit'")
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
        let value: String = sqlx::query_scalar("SELECT value FROM handoff_h05_probe")
            .fetch_one(&restored_pool)
            .await
            .unwrap();
        assert_eq!(value, "returned edit");
        restored.close_all().await.unwrap();
        fs::remove_dir_all(parent).unwrap();
    }
}
