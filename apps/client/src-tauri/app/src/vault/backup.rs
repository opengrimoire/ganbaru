//! Whole-vault snapshots and activation, with portable Android backup commands.

use super::select_quiesced_vault;
use super::{APP_SQLITE_FILE, VAULT_MANIFEST_FILE, VaultInfo};
use super::{MAX_VAULT_MANIFEST_BYTES, database_path, parse_vault_manifest, vault_info_from_path};
#[cfg(target_os = "android")]
use super::{default_vault_path, path_to_string};
#[cfg(target_os = "android")]
use chrono::{SecondsFormat, Utc};
#[cfg(target_os = "android")]
use ganbaru_mobile_documents::MobileDocumentsExt;
#[cfg(any(desktop, test))]
use ganbaru_sync_replica::backup::ArchiveGate;
use ganbaru_sync_replica::backup::{
    BACKUP_MAX_BYTES, BACKUP_MAX_DEPTH, BACKUP_MAX_ENTRIES, COPY_BUFFER_BYTES,
    is_bundle_layout_entry, is_database_sidecar,
};
use sqlx::Row;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use tauri::Runtime;
use zip::ZipArchive;

#[cfg(target_os = "android")]
const BACKUP_EXTENSION: &str = "ganbaru-backup";
#[cfg(target_os = "android")]
const BACKUP_MIME_TYPE: &str = "application/zip";
/// Most skipped bundle entries named in one log line.
const MAX_REPORTED_SKIPPED_ENTRIES: usize = 8;
const DIFFERENT_VAULT_ERROR: &str = "received bundle belongs to a different vault";

/// Entries left out of an extracted bundle or a vault replacement.
#[derive(Debug, Default)]
pub(super) struct SkippedEntries {
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
    pub(super) fn report(&self, operation: &str, reason: &str) {
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
fn backup_file_name() -> String {
    let timestamp: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    let timestamp = timestamp
        .to_rfc3339_opts(SecondsFormat::Secs, true)
        .replace(':', "-");
    format!("ganbaru-backup-{timestamp}.{BACKUP_EXTENSION}")
}

/// Creates one consistent whole-vault archive after the caller excludes vault writers.
#[cfg(desktop)]
pub(crate) async fn create_handoff_archive(
    vault_root: &Path,
    database_snapshot: &Path,
    archive_path: &Path,
) -> Result<(), String> {
    ganbaru_sync_replica::backup::create_snapshot_archive(
        &ArchiveGate::process(),
        vault_root,
        database_snapshot,
        archive_path,
    )
    .await
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
pub(super) fn extract_backup_archive(
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
pub(super) async fn replacement_shrinks_drastically(target: &Path, staging: &Path) -> bool {
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

#[cfg(test)]
fn replace_vault(staging: &Path, target: &Path, rollback: &Path) -> Result<(), String> {
    replace_vault_flagging(staging, target, rollback, false)
}

/// Replaces `target` with `staging`, which holds another vault. The whole replaced vault is
/// kept as a rollback copy inside the new folder, so it stays restorable from Settings > Data.
pub(super) fn replace_with_other_vault(
    staging: &Path,
    target: &Path,
    rollback: &Path,
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
    if had_target && let Err(error) = finish_other_vault_replacement(target, rollback) {
        eprintln!("Vault replacement cleanup: {error}");
    }
    Ok(())
}

/// Keeps `rollback`, a whole vault that another vault at `target` replaced, as a rollback copy
/// of `target`. When it cannot be kept, the replaced folder stays as a visible sibling.
fn finish_other_vault_replacement(target: &Path, rollback: &Path) -> Result<(), String> {
    let now = super::now_utc();
    if let Err(error) = super::database_rollback::retain_other_vault(rollback, now) {
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

/// Replaces `target` with `staging`. When both hold the same vault, the replaced data is kept as
/// a rollback inside the new folder: the database, or the whole layout when `flagged` marks a
/// drastic shrink.
pub(super) fn replace_vault_flagging(
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
/// such entries, so they are files this device kept in its vault folder. The
/// vault lock file stays behind, since the replacement carries its own. When
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
                if is_bundle_layout_entry(&name, directory)
                    || is_database_sidecar(&name)
                    || name == Path::new(super::vault_lock::VAULT_LOCK_FILE)
                {
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
    Ok(handoff_staging_path_in(parent, transfer_id))
}

/// Staging folder for a received vault inside `parent`, the folder that will hold the vault, so
/// activation is a rename on one file system.
#[cfg(desktop)]
pub(crate) fn handoff_staging_path_in(parent: &Path, transfer_id: &str) -> PathBuf {
    parent.join(format!(".ganbaru.handoff-{transfer_id}.staging"))
}

/// Error returned when the folder chosen for a linked vault already holds files. The webview
/// maps it to a localized message, so the text is stable.
pub(crate) const LINK_FOLDER_NOT_EMPTY: &str =
    "the folder chosen for the linked vault is not empty";

/// Error returned when the folder chosen for a linked vault lies inside another vault. The
/// webview maps it to a localized message, so the text is stable.
#[cfg(any(test, desktop))]
pub(crate) const LINK_FOLDER_INSIDE_VAULT: &str =
    "the folder chosen for the linked vault is inside another vault";

/// Whether `path`, or a folder that holds it, carries a vault marker. Symbolic links in the part
/// of `path` that exists are resolved first.
#[cfg(any(test, desktop))]
fn lies_inside_vault(path: &Path) -> bool {
    let Some(existing) = path.ancestors().find(|ancestor| ancestor.exists()) else {
        return false;
    };
    let resolved = fs::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
    resolved
        .ancestors()
        .any(|folder| folder.join(VAULT_MANIFEST_FILE).is_file())
}

/// Most numbered siblings tried before a first link gives up on a free folder name.
#[cfg(any(test, desktop))]
const MAX_LINK_FOLDER_SUFFIX: u32 = 100;

/// Whether `path` can receive a vault: it is missing or an empty folder.
pub(super) fn is_free_folder(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        Ok(metadata) => metadata.is_dir() && super::folder_is_empty(path).unwrap_or(false),
    }
}

/// Folder name for a vault display name: characters no supported file system accepts become
/// spaces, and a name with nothing left falls back to the default folder name.
#[cfg(any(test, desktop))]
pub(super) fn vault_folder_name(display_name: &str, fallback: &str) -> String {
    let cleaned: String = display_name
        .chars()
        .map(|character| {
            if character.is_control() || r#"<>:"/\|?*"#.contains(character) {
                ' '
            } else {
                character
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').trim();
    if cleaned.is_empty() || cleaned == ".." {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

/// Chooses the folder a first link on a computer activates the linked vault in. A folder the
/// user chose must be missing or empty and must not lie inside another vault. Otherwise the
/// default platform folder is used while it is missing or empty, and else an empty sibling named
/// after the vault, numbered when that name is taken. The folder of the vault in use is never
/// chosen, so it stays untouched. An empty folder holds no vault, so only the folders around it
/// are checked.
#[cfg(any(test, desktop))]
pub(crate) fn resolve_link_folder(
    chosen: Option<&Path>,
    default: &Path,
    vault_display_name: &str,
) -> Result<PathBuf, String> {
    if let Some(chosen) = chosen {
        return if !is_free_folder(chosen) {
            Err(LINK_FOLDER_NOT_EMPTY.to_string())
        } else if lies_inside_vault(chosen) {
            Err(LINK_FOLDER_INSIDE_VAULT.to_string())
        } else {
            Ok(chosen.to_path_buf())
        };
    }
    if is_free_folder(default) {
        return Ok(default.to_path_buf());
    }
    let parent = default
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
    let fallback = default
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Ganbaru");
    let name = vault_folder_name(vault_display_name, fallback);
    (1..=MAX_LINK_FOLDER_SUFFIX)
        .map(|number| {
            if number == 1 {
                parent.join(&name)
            } else {
                parent.join(format!("{name} {number}"))
            }
        })
        .find(|candidate| is_free_folder(candidate))
        .ok_or_else(|| "no free folder is available for the linked vault".to_string())
}

/// What happens to a folder that holds another vault when a received vault replaces it. A copy
/// of the same vault is always replaced with carry-forward and a rollback copy.
#[derive(Clone, Debug)]
pub(crate) enum ReplacedVault {
    /// Keep a folder that holds another vault whole beside the vault. Used on computers, where a
    /// first link moves into its own folder instead.
    #[cfg_attr(not(desktop), allow(dead_code))]
    KeepOther,
    /// Write a portable backup of another vault into this folder first, refusing the replacement
    /// when the backup fails, then discard the replaced folder. A folder that is still the
    /// untouched Android starter when the swap begins is discarded without a backup. The starter
    /// is re-read inside snapshot quiescence, so a starter the user entered while the copy
    /// downloaded is backed up.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    BackUpUnlessStarter(PathBuf),
}

/// Whether replacing the vault folder `target` needs a backup first. Only a readable vault that
/// is still the untouched starter (`untouched_starter`) is discarded without one.
fn replaced_vault_needs_backup(untouched_starter: bool, target: &Path) -> bool {
    !(untouched_starter && vault_info_from_path(target).is_ok())
}

/// Where a received vault is activated.
#[derive(Clone, Debug)]
enum HandoffPlacement {
    /// Replaces the folder at `target`.
    Replace {
        target: PathBuf,
        previous: ReplacedVault,
    },
    /// Moves the vault into `target`, a missing or empty folder, and leaves the vault in use
    /// where it is.
    #[cfg_attr(not(desktop), allow(dead_code))]
    NewFolder { target: PathBuf },
}

impl HandoffPlacement {
    fn target(&self) -> &Path {
        match self {
            Self::Replace { target, .. } | Self::NewFolder { target } => target,
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) async fn activate_android_handoff(
    app: &tauri::AppHandle,
    staging: &Path,
    transfer_id: &str,
    expected_vault_id: &str,
    previous: ReplacedVault,
) -> Result<VaultInfo, String> {
    let target = default_vault_path(app)?;
    activate_handoff_at_path(
        app,
        staging,
        HandoffPlacement::Replace { target, previous },
        transfer_id,
        expected_vault_id,
    )
    .await
}

#[cfg(desktop)]
pub(crate) async fn activate_active_handoff<R: Runtime>(
    app: &tauri::AppHandle<R>,
    staging: &Path,
    transfer_id: &str,
    expected_vault_id: &str,
    previous: ReplacedVault,
) -> Result<VaultInfo, String> {
    let target = super::active_vault_path(app)?;
    activate_handoff_at_path(
        app,
        staging,
        HandoffPlacement::Replace { target, previous },
        transfer_id,
        expected_vault_id,
    )
    .await
}

/// Activates a received vault in `target`, a missing or empty folder, and makes it the vault in
/// use. The previous vault folder is not touched and stays in the vault picker.
#[cfg(desktop)]
pub(crate) async fn activate_handoff_in_new_folder<R: Runtime>(
    app: &tauri::AppHandle<R>,
    staging: &Path,
    target: PathBuf,
    transfer_id: &str,
    expected_vault_id: &str,
) -> Result<VaultInfo, String> {
    activate_handoff_at_path(
        app,
        staging,
        HandoffPlacement::NewFolder { target },
        transfer_id,
        expected_vault_id,
    )
    .await
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

/// Activates the staged handoff at `placement`. A folder that holds any vault other than
/// `expected_vault_id` is deleted only after its replacement policy wrote a backup of it, or when
/// it is the untouched Android starter; otherwise it is kept whole beside the vault, whatever the
/// caller asked.
async fn activate_handoff_at_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    staging: &Path,
    placement: HandoffPlacement,
    transfer_id: &str,
    expected_vault_id: &str,
) -> Result<VaultInfo, String> {
    let target = placement.target().to_path_buf();
    let parent = target
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?
        .to_path_buf();
    fs::create_dir_all(&parent).map_err(|error| format!("create app data directory: {error}"))?;
    let swap = plan_swap(&placement, &parent, staging, transfer_id, expected_vault_id)?;
    #[cfg(desktop)]
    let expected_active = super::active_vault_path(app)?;
    let quiescence = super::quiescence::begin_snapshot_quiescence(app).await?;
    if let Err(error) = prepare_replacement(app, staging, &placement, expected_vault_id).await {
        return Err(match quiescence.finish() {
            Ok(()) => error,
            Err(resume) => format!("{error}; {resume}"),
        });
    }
    let flagged = matches!(
        swap,
        Swap::Replace {
            preserve: false,
            ..
        }
    ) && staging.exists()
        && holds_vault(&target, expected_vault_id)
        && replacement_shrinks_drastically(&target, staging).await;
    let activation_app = app.clone();
    let staging = staging.to_path_buf();
    let expected_vault_id = expected_vault_id.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(desktop)]
        if super::active_vault_path(&activation_app)? != expected_active {
            return Err("active vault changed while preparing handoff activation".to_string());
        }
        if staging.exists() {
            super::vault_lock::process_vault_lock().replace_with(&staging, &target, || {
                run_swap(&swap, &staging, &target, flagged)
            })?;
        }
        let info = vault_info_from_path(&target)?;
        if info.vault_id != expected_vault_id {
            return Err("activated handoff belongs to a different vault".to_string());
        }
        select_quiesced_vault(&activation_app, &info, &quiescence)?;
        quiescence.finish()?;
        if let Swap::Replace {
            rollback,
            discard_preserved: true,
            ..
        } = &swap
            && rollback.exists()
            && let Err(error) = discard_replaced_folder(&target, rollback)
        {
            eprintln!(
                "Remove the replaced vault folder '{}': {error}",
                rollback.display()
            );
        }
        Ok(info)
    })
    .await
    .map_err(|error| format!("handoff activation worker failed: {error}"))?
}

/// Deletes the replaced vault folder `replaced` only after the activated vault at `target` and
/// the renames into their parent are on stable storage, so a crash never leaves neither vault.
/// A failed flush keeps `replaced`.
fn discard_replaced_folder(target: &Path, replaced: &Path) -> Result<(), String> {
    let durable = |folder: &Path| {
        ganbaru_atomic_files::sync_directory_entries(folder)
            .map_err(|error| format!("flush '{}': {error}", folder.display()))
    };
    durable(target)?;
    if let Some(parent) = target.parent() {
        durable(parent)?;
    }
    fs::remove_dir_all(replaced).map_err(|error| error.to_string())
}

/// How the staged folder takes its place.
#[derive(Debug)]
enum Swap {
    /// Renamed into a missing or empty folder.
    IntoNewFolder,
    /// Renamed over the target. With `preserve`, the target moves to `rollback` whole and is
    /// deleted after activation only with `discard_preserved`; otherwise `rollback` holds the
    /// rollback copy of a replacement of the same vault.
    Replace {
        rollback: PathBuf,
        preserve: bool,
        discard_preserved: bool,
    },
}

/// Decides the swap for `placement` before any folder changes.
fn plan_swap(
    placement: &HandoffPlacement,
    parent: &Path,
    staging: &Path,
    transfer_id: &str,
    expected_vault_id: &str,
) -> Result<Swap, String> {
    let (target, previous) = match placement {
        HandoffPlacement::NewFolder { target } => {
            return if is_free_folder(target) {
                Ok(Swap::IntoNewFolder)
            } else {
                Err(LINK_FOLDER_NOT_EMPTY.to_string())
            };
        }
        HandoffPlacement::Replace { target, previous } => (target, previous),
    };
    let replaces_other = replaces_independent_vault(target, expected_vault_id);
    let preserve = replaces_other;
    let rollback = if preserve {
        preserved_handoff_path(parent, target, transfer_id)
    } else {
        parent.join(".ganbaru.handoff-refresh-rollback")
    };
    if rollback.exists() && staging.exists() {
        return Err("a previous handoff copy still requires recovery".to_string());
    }
    let discard_preserved = replaces_other
        && match previous {
            ReplacedVault::BackUpUnlessStarter(_) => true,
            ReplacedVault::KeepOther => false,
        };
    Ok(Swap::Replace {
        rollback,
        preserve,
        discard_preserved,
    })
}

fn run_swap(swap: &Swap, staging: &Path, target: &Path, flagged: bool) -> Result<(), String> {
    match swap {
        Swap::IntoNewFolder => move_into_new_folder(staging, target),
        Swap::Replace {
            rollback,
            preserve: true,
            ..
        } => replace_vault_preserving_previous(staging, target, rollback),
        Swap::Replace { rollback, .. } => {
            replace_vault_flagging(staging, target, rollback, flagged)
        }
    }
}

/// Runs inside snapshot quiescence before the swap: carries this device's changes forward into a
/// replacement of the same vault, and writes the backup a replacement policy asks for. A failed
/// backup refuses the replacement.
async fn prepare_replacement<R: Runtime>(
    app: &tauri::AppHandle<R>,
    staging: &Path,
    placement: &HandoffPlacement,
    expected_vault_id: &str,
) -> Result<(), String> {
    let HandoffPlacement::Replace { target, previous } = placement else {
        return Ok(());
    };
    carry_forward_local_changes(app, staging, target, expected_vault_id).await?;
    let root = match previous {
        ReplacedVault::BackUpUnlessStarter(root) => {
            replaced_vault_needs_backup(super::untouched_starter_vault(app)?, target)
                .then_some(root)
        }
        ReplacedVault::KeepOther => None,
    };
    if let Some(root) = root
        && staging.exists()
        && replaces_independent_vault(target, expected_vault_id)
    {
        let now_ms = i64::try_from(crate::sync::now_ms()?)
            .map_err(|_| "current time is out of range".to_string())?;
        super::replaced_vaults::write_replaced_vault_backup(root, target, now_ms).await?;
    }
    Ok(())
}

/// Moves `staging` into `target`, which is missing or an empty folder.
pub(super) fn move_into_new_folder(staging: &Path, target: &Path) -> Result<(), String> {
    if target.exists() {
        fs::remove_dir(target)
            .map_err(|error| format!("prepare the linked vault folder: {error}"))?;
    }
    fs::rename(staging, target).map_err(|error| format!("activate the linked vault: {error}"))
}

/// Carries this device's synced changes, device state, and local managed assets into the staged
/// replacement when `target` holds the same vault. A replacement by another vault carries nothing.
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
    carry_into_replacement(
        &ganbaru_sync::Engine::vault(),
        staging,
        target,
        &files,
        crate::sync::now_ms()?,
    )
    .await
}

/// Runs inside snapshot quiescence, before the staged folder is renamed over `target`: stores
/// and applies the operations of `target` that the staged database lacks, deletes the carry
/// bundle, copies the `Local` rows and columns of `target` into the staged database, and copies
/// the managed asset files only this device holds. `target` stays the vault until the rename, so
/// a failure at any step keeps it whole and a retry repeats the steps harmlessly.
async fn carry_into_replacement(
    engine: &ganbaru_sync::Engine,
    staging: &Path,
    target: &Path,
    files: &ganbaru_sync_replica::writer::SyncFiles,
    now_ms: u64,
) -> Result<(), String> {
    use ganbaru_sync_replica::carry_forward;
    let (current, replacement) = (database_path(target), database_path(staging));
    carry_forward::carry_into_replacement(engine, &current, &replacement, files, now_ms)
        .await
        .map_err(|error| format!("keep local synced changes: {error}"))?;
    failpoints::check("replace.before_restore")?;
    carry_forward::restore_into_replacement(engine, &current, &replacement)
        .await
        .map_err(|error| format!("keep device state: {error}"))?;
    let (current, replacement) = (target.to_path_buf(), staging.to_path_buf());
    let manifest = engine.manifest();
    tauri::async_runtime::spawn_blocking(move || {
        carry_forward::carry_assets(manifest, &current, &replacement)
    })
    .await
    .map_err(|error| format!("keep local managed assets: {error}"))?
    .map_err(|error| format!("keep local managed assets: {error}"))?;
    failpoints::check("replace.before_rename")
}

/// Failpoints on the replacement sequence. Tests arm one on their own thread; outside tests
/// every check passes.
mod failpoints {
    #[cfg(test)]
    thread_local! {
        static ARMED: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
    }

    /// Arms `name` on this thread until it is cleared or another failpoint is armed.
    #[cfg(test)]
    pub(super) fn arm(name: &'static str) {
        ARMED.set(Some(name));
    }

    /// Clears the armed failpoint of this thread.
    #[cfg(test)]
    pub(super) fn clear() {
        ARMED.set(None);
    }

    /// Fails when `name` is armed on this thread.
    pub(super) fn check(name: &'static str) -> Result<(), String> {
        if armed(name) {
            Err(format!("failpoint {name}"))
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    fn armed(name: &'static str) -> bool {
        ARMED.get() == Some(name)
    }

    #[cfg(not(test))]
    const fn armed(_name: &'static str) -> bool {
        false
    }
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

/// Finishes a replacement an exit interrupted: puts `rollback` back when `target` is missing,
/// and otherwise keeps it as a rollback copy of `target`, a whole copy when it holds another
/// vault, so an interrupted restore of another vault never deletes the replaced one.
pub(super) fn recover_interrupted_restore(target: &Path, rollback: &Path) -> Result<(), String> {
    if !rollback.exists() {
        return Ok(());
    }
    if target.exists() {
        let other_vault = matches!(
            (vault_info_from_path(target), vault_info_from_path(rollback)),
            (Ok(current), Ok(replaced)) if current.vault_id != replaced.vault_id
        );
        if other_vault {
            finish_other_vault_replacement(target, rollback)
        } else {
            finish_retained_replacement(target, rollback, false)
        }
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
    recover_interrupted_restore(&target, &parent.join(".ganbaru.rollback"))?;
    let backups = super::replaced_vaults::replaced_vaults_root(app)?;
    for leftover in superseded_handoff_leftovers(parent, &target, &backups) {
        if let Err(error) = fs::remove_dir_all(&leftover) {
            eprintln!(
                "Remove the replaced vault folder '{}': {error}",
                leftover.display()
            );
        }
    }
    Ok(())
}

/// Name prefix of the folder an Android handoff moves a replaced vault into.
#[cfg(any(test, target_os = "android"))]
const HANDOFF_PREVIOUS_PREFIX: &str = ".ganbaru.handoff-previous-";

/// Replaced vault folders in `parent` that an interrupted Android handoff left behind and that
/// are safe to delete: the vault at `target` is a different readable vault, so the swap
/// finished, and `backups` keeps a backup of the replaced vault written after its last change.
/// A folder that is unreadable, still the active vault, or without such a backup is kept.
#[cfg(any(test, target_os = "android"))]
fn superseded_handoff_leftovers(parent: &Path, target: &Path, backups: &Path) -> Vec<PathBuf> {
    let Ok(active) = vault_info_from_path(target) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(HANDOFF_PREVIOUS_PREFIX))
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .map(|entry| entry.path())
        .filter(|leftover| {
            let Ok(replaced) = vault_info_from_path(leftover) else {
                return false;
            };
            replaced.vault_id != active.vault_id
                && last_vault_change_ms(leftover).is_some_and(|changed_ms| {
                    super::replaced_vaults::has_backup_since(
                        backups,
                        &replaced.vault_id,
                        changed_ms,
                    )
                })
        })
        .collect()
}

/// Latest modification time, in Unix milliseconds, of the vault marker and database files in
/// `vault`, or `None` when none of them can be read.
#[cfg(any(test, target_os = "android"))]
fn last_vault_change_ms(vault: &Path) -> Option<i64> {
    [
        VAULT_MANIFEST_FILE.to_string(),
        APP_SQLITE_FILE.to_string(),
        format!("{APP_SQLITE_FILE}-wal"),
    ]
    .iter()
    .filter_map(|name| {
        fs::metadata(vault.join(name))
            .and_then(|meta| meta.modified())
            .ok()
    })
    .filter_map(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
    .filter_map(|elapsed| i64::try_from(elapsed.as_millis()).ok())
    .max()
}

/// Saves the portable backup archive at `archive_path` to Android Downloads under a fresh
/// backup file name.
#[cfg(target_os = "android")]
pub(crate) fn save_backup_to_downloads(
    app: &tauri::AppHandle,
    archive_path: &Path,
) -> Result<VaultBackupOutcome, String> {
    let file_name = backup_file_name();
    let archive_path = path_to_string(archive_path, "backup archive")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::{SimpleFileOptions, ZipWriter};

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
    fn vault_folder_names_drop_reserved_characters_and_fall_back_when_empty() {
        assert_eq!(vault_folder_name("Work vault", "Ganbaru"), "Work vault");
        assert_eq!(vault_folder_name("a/b:c*", "Ganbaru"), "a b c");
        assert_eq!(vault_folder_name("  Notes.  ", "Ganbaru"), "Notes");
        assert_eq!(vault_folder_name("<>|?", "Ganbaru"), "Ganbaru");
        assert_eq!(vault_folder_name("..", "Ganbaru"), "Ganbaru");
    }

    #[test]
    fn link_folder_uses_the_default_when_free_and_an_empty_named_sibling_otherwise() {
        let parent = unique_test_path("link-folder");
        let default = parent.join("Ganbaru");
        assert_eq!(
            resolve_link_folder(None, &default, "Home").unwrap(),
            default
        );

        fs::create_dir_all(&default).unwrap();
        assert_eq!(
            resolve_link_folder(None, &default, "Home").unwrap(),
            default
        );

        fs::write(default.join("vault.json"), "{}").unwrap();
        assert_eq!(
            resolve_link_folder(None, &default, "Home").unwrap(),
            parent.join("Home")
        );

        fs::create_dir_all(parent.join("Home")).unwrap();
        fs::write(parent.join("Home").join("note.md"), "kept").unwrap();
        assert_eq!(
            resolve_link_folder(None, &default, "Home").unwrap(),
            parent.join("Home 2")
        );
        assert_eq!(
            fs::read_to_string(parent.join("Home").join("note.md")).unwrap(),
            "kept"
        );
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn chosen_link_folder_must_be_missing_or_empty() {
        let parent = unique_test_path("chosen-link-folder");
        let default = parent.join("Ganbaru");
        let chosen = parent.join("Elsewhere");
        assert_eq!(
            resolve_link_folder(Some(&chosen), &default, "Home").unwrap(),
            chosen
        );

        fs::create_dir_all(&chosen).unwrap();
        fs::write(chosen.join("file.txt"), "busy").unwrap();
        assert_eq!(
            resolve_link_folder(Some(&chosen), &default, "Home").unwrap_err(),
            LINK_FOLDER_NOT_EMPTY
        );
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn chosen_link_folder_must_not_lie_inside_another_vault() {
        let parent = unique_test_path("link-folder-inside-vault");
        let default = parent.join("Ganbaru");
        let vault = parent.join("Vault");
        fs::create_dir_all(&vault).unwrap();
        crate::vault::initialize_vault(&vault).unwrap();

        let empty_inside = vault.join("projects").join("Linked");
        fs::create_dir_all(&empty_inside).unwrap();
        for chosen in [empty_inside, vault.join("missing").join("Linked")] {
            assert_eq!(
                resolve_link_folder(Some(&chosen), &default, "Home").unwrap_err(),
                LINK_FOLDER_INSIDE_VAULT,
                "{}",
                chosen.display()
            );
        }
        let beside = parent.join("Beside");
        assert_eq!(
            resolve_link_folder(Some(&beside), &default, "Home").unwrap(),
            beside
        );
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn new_folder_placement_refuses_a_folder_that_gained_files() {
        let parent = unique_test_path("new-folder-swap");
        let target = parent.join("Linked");
        let staging = parent.join("staging");
        let placement = HandoffPlacement::NewFolder {
            target: target.clone(),
        };
        assert!(matches!(
            plan_swap(&placement, &parent, &staging, "t1", "vault-linked").unwrap(),
            Swap::IntoNewFolder
        ));

        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("file.txt"), "busy").unwrap();
        assert_eq!(
            plan_swap(&placement, &parent, &staging, "t1", "vault-linked").unwrap_err(),
            LINK_FOLDER_NOT_EMPTY
        );
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn replaced_other_vault_is_discarded_only_after_a_backup_or_as_the_starter() {
        let parent = unique_test_path("replaced-policy");
        let target = parent.join("Ganbaru");
        let staging = parent.join("staging");
        fs::create_dir_all(&target).unwrap();
        crate::vault::initialize_vault(&target).unwrap();
        let discards = |previous: ReplacedVault| {
            let placement = HandoffPlacement::Replace {
                target: target.clone(),
                previous,
            };
            match plan_swap(&placement, &parent, &staging, "t1", "vault-linked").unwrap() {
                Swap::Replace {
                    preserve,
                    discard_preserved,
                    ..
                } => (preserve, discard_preserved),
                Swap::IntoNewFolder => panic!("a replacement never moves into a new folder"),
            }
        };

        assert_eq!(discards(ReplacedVault::KeepOther), (true, false));
        assert_eq!(
            discards(ReplacedVault::BackUpUnlessStarter(parent.join("backups"))),
            (true, true)
        );
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn a_replaced_starter_skips_its_backup_only_while_untouched_and_readable() {
        let parent = unique_test_path("replaced-starter");
        let target = parent.join("Ganbaru");
        fs::create_dir_all(&target).unwrap();
        crate::vault::initialize_vault(&target).unwrap();

        assert!(!replaced_vault_needs_backup(true, &target));
        assert!(
            replaced_vault_needs_backup(false, &target),
            "a starter the user entered while the copy downloaded is backed up"
        );
        fs::write(target.join("vault.json"), "invalid json").unwrap();
        assert!(replaced_vault_needs_backup(true, &target));
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn handoff_leftovers_are_removed_only_when_superseded_and_backed_up() {
        let parent = unique_test_path("handoff-leftovers");
        let target = parent.join("Ganbaru");
        let backed_up = parent.join(format!("{HANDOFF_PREVIOUS_PREFIX}backed-up"));
        let without_backup = parent.join(format!("{HANDOFF_PREVIOUS_PREFIX}without-backup"));
        let same_vault = parent.join(format!("{HANDOFF_PREVIOUS_PREFIX}same-vault"));
        let unreadable = parent.join(format!("{HANDOFF_PREVIOUS_PREFIX}unreadable"));
        let backups = parent.join("replaced-vaults");
        for folder in [
            &target,
            &backed_up,
            &without_backup,
            &same_vault,
            &unreadable,
            &backups,
        ] {
            fs::create_dir_all(folder).unwrap();
        }
        for folder in [&target, &backed_up, &without_backup] {
            crate::vault::initialize_vault(folder).unwrap();
        }
        fs::write(unreadable.join("vault.json"), "invalid json").unwrap();
        fs::copy(target.join("vault.json"), same_vault.join("vault.json")).unwrap();
        let active = fs::read(target.join("vault.json")).unwrap();
        write_test_archive(
            &backups.join("replaced-9000000000000.ganbaru-backup"),
            &[("vault.json", active.as_slice())],
        );

        let backed_up_id = vault_info_from_path(&backed_up).unwrap().vault_id;
        let changed_ms = last_vault_change_ms(&backed_up).unwrap();
        let manifest = fs::read(backed_up.join("vault.json")).unwrap();
        write_test_archive(
            &backups.join(format!("replaced-{}.ganbaru-backup", changed_ms + 1)),
            &[("vault.json", manifest.as_slice())],
        );
        let stale = fs::read(without_backup.join("vault.json")).unwrap();
        let stale_ms = last_vault_change_ms(&without_backup).unwrap();
        write_test_archive(
            &backups.join(format!("replaced-{}.ganbaru-backup", stale_ms - 1)),
            &[("vault.json", stale.as_slice())],
        );

        assert_eq!(
            superseded_handoff_leftovers(&parent, &target, &backups),
            vec![backed_up.clone()]
        );
        assert!(
            vault_info_from_path(&backed_up).unwrap().vault_id == backed_up_id,
            "the decision itself deletes nothing"
        );
        fs::write(target.join("vault.json"), "invalid json").unwrap();
        assert!(
            superseded_handoff_leftovers(&parent, &target, &backups).is_empty(),
            "an unreadable active vault keeps every leftover"
        );
        let _ = fs::remove_dir_all(parent);
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
            ("config.json", false),
            ("notes", false),
            ("vault.json", true),
            (super::super::vault_lock::VAULT_LOCK_FILE, false),
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
    fn replacement_discards_the_previous_vault_lock_file() {
        let parent = unique_test_path("lock-file");
        let target = parent.join("Ganbaru");
        let rollback = parent.join(".ganbaru.rollback");
        let lock_file = super::super::vault_lock::VAULT_LOCK_FILE;
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&rollback).unwrap();
        fs::write(target.join(lock_file), b"").unwrap();
        fs::write(rollback.join(lock_file), b"").unwrap();
        fs::write(rollback.join("vault.json"), b"old").unwrap();

        finish_replacement(&target, &rollback).unwrap();

        assert!(!rollback.exists());
        assert_eq!(
            fs::read_dir(&parent).unwrap().count(),
            1,
            "only the replacement should remain"
        );
        let _ = fs::remove_dir_all(parent);
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
        let source_info = initialize_migrated_vault(&source).await;
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
    async fn whole_vault_snapshot_restores_database_and_managed_files() {
        let parent = unique_test_path("whole-vault");
        let source = parent.join("source");
        let staging = parent.join("staging");
        let target = parent.join("receiver");
        let rollback = parent.join("rollback");
        let snapshot = parent.join("snapshot.sqlite");
        let archive_path = parent.join("handoff.ganbaru-backup");
        fs::create_dir_all(&source).unwrap();
        let source_info = initialize_migrated_vault(&source).await;

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
            "INSERT INTO project_tasks (id, project_id, section_id, status_id, title, section_order_key, status_order_key) VALUES ('handoff-task', 'project-routine-learning', 'section-routine-learning-general', 'status-routine-learning-todo', 'Portable project task', 'a1', 'a1')",
            "INSERT INTO notes_pages (id, parent_type, title) VALUES ('handoff-note', 'workspace', 'Portable note')",
            "INSERT INTO chat_conversations (id, project_id, conversation_kind, last_activity_at, created_at, updated_at) VALUES ('handoff-conversation', 'project-routine-learning', 'channel', '2026-09-14T09:00:00Z', '2026-09-14T09:00:00Z', '2026-09-14T09:00:00Z')",
            "INSERT INTO chat_channels (id, project_id, conversation_id, name, created_at, updated_at) VALUES ('handoff-channel', 'project-routine-learning', 'handoff-conversation', 'Portable chat', '2026-09-14T09:00:00Z', '2026-09-14T09:00:00Z')",
            "INSERT INTO quick_notes (id, title, body_plain_text, order_key) VALUES ('handoff-quick-note', 'Portable quick note', 'Portable quick-note body', 'a0')",
            "INSERT INTO themes (id, display_name, blend_canvas, seed_blend_canvas, derivation_engine_version, created_at_ms, updated_at_ms, icon_label, seed_icon_label) VALUES ('handoff-theme', 'Portable theme', '{}', '{}', 1, 1, 1, 'dark', 'dark')",
            "INSERT INTO distractions_usage_samples (id, source_type, source_key, display_name, started_at_ms, elapsed_seconds, local_date, created_at_ms) VALUES ('handoff-usage', 'mobile-app', 'app.example', 'Portable usage', 1, 45, '2026-09-14', 1)",
            "INSERT INTO music_playlists (id, name, order_key, created_at_ms, updated_at_ms) VALUES ('handoff-playlist', 'Portable playlist', 'a0', 1, 1)",
            "INSERT INTO music_library_items (id, identity_key, source_kind, original_title, discovered_at_ms, updated_at_ms) VALUES ('handoff-track', 'local:portable-track', 'local-file', 'Portable track', 1, 1)",
            "INSERT INTO music_playlist_memberships (id, playlist_id, item_id, order_key, created_at_ms, updated_at_ms) VALUES ('handoff-membership', 'handoff-playlist', 'handoff-track', 'a0', 1, 1)",
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

        ganbaru_sync_replica::backup::create_backup_archive(
            &ArchiveGate::default(),
            &source_pool,
            &source,
            &snapshot,
            &archive_path,
        )
        .await
        .unwrap();

        let file = fs::File::open(&archive_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        assert!(archive.by_name(&format!("{APP_SQLITE_FILE}-wal")).is_err());
        assert!(archive.by_name(&format!("{APP_SQLITE_FILE}-shm")).is_err());
        assert!(archive.by_name("config.json").is_err());
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
        assert!(!target.join("config.json").exists());
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
        initialize_migrated_vault(&staging).await;
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
    async fn a_vault_copy_activates_and_a_fresh_copy_replaces_it() {
        let parent = unique_test_path("desktop-android-handoff");
        let source = parent.join("desktop");
        let first_staging = parent.join("first-staging");
        let first_target = parent.join("first-target");
        let refresh_staging = parent.join("refresh-staging");
        let refresh_target = parent.join("refresh-target");
        let snapshot = parent.join("snapshot.sqlite");
        let archive = parent.join("handoff.zip");
        let refresh_snapshot = parent.join("refresh-snapshot.sqlite");
        let refresh_archive = parent.join("refresh.zip");
        fs::create_dir_all(&source).unwrap();
        let source_info = initialize_migrated_vault(&source).await;
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
        stage_handoff_archive(&archive, &first_staging, &source_info.vault_id)
            .await
            .unwrap();

        replace_vault(
            &first_staging,
            &first_target,
            &parent.join("first-rollback"),
        )
        .unwrap();
        assert_eq!(
            fs::read(first_target.join("assets/notes/files/h04.txt")).unwrap(),
            b"desktop asset"
        );

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
    async fn a_copy_from_android_carries_the_whole_vault_to_a_desktop() {
        let parent = unique_test_path("android-desktop-return");
        let source = parent.join("android");
        let desktop = parent.join("desktop");
        let refresh_staging = parent.join("refresh-staging");
        let return_staging = parent.join("return-staging");
        fs::create_dir_all(&source).unwrap();
        let info = initialize_migrated_vault(&source).await;
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

        let refresh_snapshot = parent.join("refresh.sqlite");
        let refresh_archive = parent.join("refresh.zip");
        create_handoff_archive(&source, &refresh_snapshot, &refresh_archive)
            .await
            .unwrap();
        stage_handoff_archive(&refresh_archive, &refresh_staging, &info.vault_id)
            .await
            .unwrap();
        replace_vault(&refresh_staging, &desktop, &parent.join("refresh-rollback")).unwrap();

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

        replace_vault(&return_staging, &desktop, &parent.join("return-rollback")).unwrap();
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

    /// Initializes a vault at `path` whose database holds the shared migrated template, so
    /// opening it skips the migration chain.
    async fn initialize_migrated_vault(path: &Path) -> VaultInfo {
        let info = super::super::initialize_vault(path).unwrap();
        ganbaru_db::test_support::write_migrated_database(database_path(path))
            .await
            .unwrap();
        info
    }
}

#[cfg(test)]
mod carry_tests;

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

    /// Writes a migrated database marked with `marker` into a vault that has none yet.
    async fn write_marker(vault: &Path, marker: i64) {
        let pragma = match marker {
            PREVIOUS_MARKER => "PRAGMA application_id = 1",
            _ => "PRAGMA application_id = 2",
        };
        ganbaru_db::test_support::write_migrated_database(database_path(vault))
            .await
            .unwrap();
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
