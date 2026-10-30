//! Portable backups of the vaults a first link replaced on Android.
//!
//! Android keeps one vault in application-private storage, so a first link that brings another
//! vault replaces it. Before that replacement, a portable backup of the replaced vault is written
//! under the private `replaced-vaults/` folder, and only the newest backups are kept. Settings >
//! Data lists them with export to Downloads and delete. When the backup cannot be written, the
//! replacement is refused.

use super::{MAX_VAULT_MANIFEST_BYTES, VAULT_MANIFEST_FILE, parse_vault_manifest};
use ganbaru_sync_replica::backup::{ArchiveGate, create_backup_archive};
use serde::Serialize;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

/// Error prefix returned when the backup of a replaced vault cannot be written. The webview maps
/// it to a localized storage message, so the text is stable.
pub(crate) const REPLACED_VAULT_BACKUP_FAILED: &str =
    "the vault this link replaces could not be backed up on this device";

/// Folder under the private application data folder that holds the backups.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const REPLACED_VAULTS_DIRECTORY: &str = "replaced-vaults";

/// Most backups kept. Writing another one removes the oldest.
pub(crate) const MAX_REPLACED_VAULT_BACKUPS: usize = 2;

const BACKUP_PREFIX: &str = "replaced-";
const BACKUP_SUFFIX: &str = ".ganbaru-backup";
const PARTIAL_PREFIX: &str = ".partial-";

/// One kept backup of a replaced vault.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReplacedVaultBackup {
    /// File name, which is also the identifier commands accept.
    pub id: String,
    /// Display name of the replaced vault, when its marker in the backup is readable.
    pub display_name: Option<String>,
    /// When the vault was replaced, in Unix milliseconds.
    pub replaced_at_ms: i64,
    /// Size of the backup file in bytes.
    pub size_bytes: u64,
}

fn backup_id(replaced_at_ms: i64) -> String {
    format!("{BACKUP_PREFIX}{replaced_at_ms}{BACKUP_SUFFIX}")
}

/// Returns the replacement time an identifier names, or `None` for anything that is not exactly
/// a backup file name, so an identifier can never name a path outside the folder.
pub(crate) fn parse_backup_id(id: &str) -> Option<i64> {
    let digits = id
        .strip_prefix(BACKUP_PREFIX)?
        .strip_suffix(BACKUP_SUFFIX)?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Writes a portable backup of the vault at `vault_root` into `root`, then keeps only the newest
/// [`MAX_REPLACED_VAULT_BACKUPS`]. The caller excludes vault writers first. Every failure is
/// reported with the [`REPLACED_VAULT_BACKUP_FAILED`] prefix and leaves no partial file behind.
pub(crate) async fn write_replaced_vault_backup(
    root: &Path,
    vault_root: &Path,
    now_ms: i64,
) -> Result<ReplacedVaultBackup, String> {
    write_backup(root, vault_root, now_ms)
        .await
        .map_err(|error| format!("{REPLACED_VAULT_BACKUP_FAILED}: {error}"))
}

async fn write_backup(
    root: &Path,
    vault_root: &Path,
    now_ms: i64,
) -> Result<ReplacedVaultBackup, String> {
    ganbaru_atomic_files::create_private_directories(root)
        .map_err(|error| format!("create the backup folder: {error}"))?;
    let mut replaced_at_ms = now_ms;
    while root.join(backup_id(replaced_at_ms)).exists() {
        replaced_at_ms = replaced_at_ms
            .checked_add(1)
            .ok_or_else(|| "backup time is exhausted".to_string())?;
    }
    let id = backup_id(replaced_at_ms);
    let partial = root.join(format!("{PARTIAL_PREFIX}{id}"));
    let snapshot = root.join(format!("{PARTIAL_PREFIX}{replaced_at_ms}.sqlite"));
    let result = archive_vault(vault_root, &snapshot, &partial).await;
    remove_if_present(&snapshot);
    if let Err(error) = result {
        remove_if_present(&partial);
        return Err(error);
    }
    let destination = root.join(&id);
    if let Err(error) = fs::rename(&partial, &destination) {
        remove_if_present(&partial);
        return Err(format!("keep the backup: {error}"));
    }
    if let Err(error) = prune(root, MAX_REPLACED_VAULT_BACKUPS) {
        eprintln!("Replaced vault backup cleanup: {error}");
    }
    describe(&destination, id, replaced_at_ms)
}

async fn archive_vault(vault_root: &Path, snapshot: &Path, archive: &Path) -> Result<(), String> {
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry
        .connect_path(super::database_path(vault_root))
        .await?;
    let result = create_backup_archive(
        &ArchiveGate::process(),
        &pool,
        vault_root,
        snapshot,
        archive,
    )
    .await;
    let closed = registry.close_all().await;
    result.and(closed)
}

fn remove_if_present(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != ErrorKind::NotFound
    {
        eprintln!(
            "Replaced vault backup cleanup of '{}': {error}",
            path.display()
        );
    }
}

/// Removes leftover partial files and every backup beyond the newest `keep`.
fn prune(root: &Path, keep: usize) -> Result<(), String> {
    let mut backups = Vec::new();
    for entry in read_folder(root)? {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(PARTIAL_PREFIX) {
            remove_if_present(&entry.path());
        } else if let Some(replaced_at_ms) = parse_backup_id(name) {
            backups.push((replaced_at_ms, entry.path()));
        }
    }
    backups.sort_by_key(|(replaced_at_ms, _)| std::cmp::Reverse(*replaced_at_ms));
    for (_, path) in backups.into_iter().skip(keep) {
        fs::remove_file(&path)
            .map_err(|error| format!("remove an older replaced vault backup: {error}"))?;
    }
    Ok(())
}

fn read_folder(root: &Path) -> Result<Vec<fs::DirEntry>, String> {
    match fs::read_dir(root) {
        Ok(entries) => entries
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read the replaced vault backups: {error}")),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("read the replaced vault backups: {error}")),
    }
}

/// Lists the kept backups in `root`, newest first.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn list_replaced_vault_backups(root: &Path) -> Result<Vec<ReplacedVaultBackup>, String> {
    let mut backups = Vec::new();
    for entry in read_folder(root)? {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(replaced_at_ms) = parse_backup_id(name) else {
            continue;
        };
        if !entry
            .file_type()
            .map_err(|error| format!("inspect a replaced vault backup: {error}"))?
            .is_file()
        {
            continue;
        }
        backups.push(describe(&entry.path(), name.to_string(), replaced_at_ms)?);
    }
    backups.sort_by_key(|backup| std::cmp::Reverse(backup.replaced_at_ms));
    Ok(backups)
}

fn describe(path: &Path, id: String, replaced_at_ms: i64) -> Result<ReplacedVaultBackup, String> {
    let size_bytes = fs::metadata(path)
        .map_err(|error| format!("inspect a replaced vault backup: {error}"))?
        .len();
    Ok(ReplacedVaultBackup {
        id,
        display_name: archived_display_name(path),
        replaced_at_ms,
        size_bytes,
    })
}

/// Reads the display name from the vault marker inside a backup, or `None` when it is unreadable.
fn archived_display_name(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let manifest = archive.by_name(VAULT_MANIFEST_FILE).ok()?;
    if !manifest.is_file() || manifest.size() > MAX_VAULT_MANIFEST_BYTES {
        return None;
    }
    let mut contents = Vec::new();
    manifest
        .take(MAX_VAULT_MANIFEST_BYTES)
        .read_to_end(&mut contents)
        .ok()?;
    parse_vault_manifest(&contents)
        .ok()
        .map(|manifest| manifest.display_name)
        .filter(|name| !name.trim().is_empty())
}

/// Resolves the backup an identifier names inside `root`.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn replaced_vault_backup_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    parse_backup_id(id).ok_or_else(|| "replaced vault backup is unknown".to_string())?;
    let path = root.join(id);
    if !path.is_file() {
        return Err("replaced vault backup is unknown".to_string());
    }
    Ok(path)
}

/// Deletes the backup an identifier names.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn delete_replaced_vault_backup(root: &Path, id: &str) -> Result<(), String> {
    let path = replaced_vault_backup_path(root, id)?;
    fs::remove_file(path).map_err(|error| format!("delete the replaced vault backup: {error}"))
}

#[cfg(target_os = "android")]
pub(crate) fn replaced_vaults_root<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("find private application data folder: {error}"))?
        .join(REPLACED_VAULTS_DIRECTORY))
}

/// Lists the backups of vaults a first link replaced on this device, newest first.
#[cfg(target_os = "android")]
#[tauri::command]
pub fn vault_replaced_backups(app: tauri::AppHandle) -> Result<Vec<ReplacedVaultBackup>, String> {
    list_replaced_vault_backups(&replaced_vaults_root(&app)?)
}

/// Saves one backup of a replaced vault to Android Downloads.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn vault_export_replaced_backup(
    app: tauri::AppHandle,
    id: String,
) -> Result<super::backup::VaultBackupOutcome, String> {
    let path = replaced_vault_backup_path(&replaced_vaults_root(&app)?, &id)?;
    super::backup::save_backup_to_downloads(&app, &path)
}

/// Deletes one backup of a replaced vault.
#[cfg(target_os = "android")]
#[tauri::command]
pub fn vault_delete_replaced_backup(app: tauri::AppHandle, id: String) -> Result<(), String> {
    delete_replaced_vault_backup(&replaced_vaults_root(&app)?, &id)
}

#[cfg(all(not(target_os = "android"), any(test, target_os = "ios")))]
#[tauri::command]
/// Report that replaced vault backups exist only on Android.
pub fn vault_replaced_backups() -> Result<Vec<ReplacedVaultBackup>, String> {
    Ok(Vec::new())
}

#[cfg(all(not(target_os = "android"), any(test, target_os = "ios")))]
#[tauri::command]
/// Report that replaced vault backups exist only on Android.
pub async fn vault_export_replaced_backup(
    id: String,
) -> Result<super::backup::VaultBackupOutcome, String> {
    let _ = id;
    Err("replaced vault backups are only available on Android".to_string())
}

#[cfg(all(not(target_os = "android"), any(test, target_os = "ios")))]
#[tauri::command]
/// Report that replaced vault backups exist only on Android.
pub fn vault_delete_replaced_backup(id: String) -> Result<(), String> {
    let _ = id;
    Err("replaced vault backups are only available on Android".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_root(name: &str) -> PathBuf {
        crate::vault::tests::unique_path(name)
    }

    fn vault_at(parent: &Path, name: &str) -> PathBuf {
        let vault = parent.join(name);
        fs::create_dir_all(&vault).unwrap();
        crate::vault::initialize_vault(&vault).unwrap();
        vault
    }

    #[test]
    fn backup_identifiers_name_only_backup_files() {
        assert_eq!(
            parse_backup_id(&backup_id(1_700_000_000_000)),
            Some(1_700_000_000_000)
        );
        for id in [
            "",
            "replaced-.ganbaru-backup",
            "replaced-12a.ganbaru-backup",
            "replaced-12.ganbaru-backup.partial",
            "../replaced-12.ganbaru-backup",
            "replaced-12/../x.ganbaru-backup",
            ".partial-replaced-12.ganbaru-backup",
            "replaced--12.ganbaru-backup",
        ] {
            assert_eq!(parse_backup_id(id), None, "{id}");
        }
        assert!(replaced_vault_backup_path(Path::new("/backups"), "../vault.json").is_err());
    }

    #[tokio::test]
    async fn writing_backups_keeps_the_newest_two_and_lists_the_vault_name() {
        let parent = unique_root("replaced-keep");
        let vault = vault_at(&parent, "Ganbaru");
        let root = parent.join(REPLACED_VAULTS_DIRECTORY);

        for at in [1_000, 2_000, 3_000] {
            write_replaced_vault_backup(&root, &vault, at)
                .await
                .unwrap();
        }
        let listed = list_replaced_vault_backups(&root).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|backup| backup.replaced_at_ms)
                .collect::<Vec<_>>(),
            vec![3_000, 2_000]
        );
        assert!(listed.iter().all(|backup| backup.size_bytes > 0));
        assert!(
            listed
                .iter()
                .all(|backup| backup.display_name.as_deref() == Some("Ganbaru"))
        );
        let names = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 2, "{names:?}");

        delete_replaced_vault_backup(&root, &listed[0].id).unwrap();
        assert_eq!(list_replaced_vault_backups(&root).unwrap().len(), 1);
        assert!(delete_replaced_vault_backup(&root, &listed[0].id).is_err());
        let _ = fs::remove_dir_all(parent);
    }

    #[tokio::test]
    async fn a_backup_written_in_the_same_millisecond_gets_its_own_file() {
        let parent = unique_root("replaced-same-time");
        let vault = vault_at(&parent, "Ganbaru");
        let root = parent.join(REPLACED_VAULTS_DIRECTORY);

        let first = write_replaced_vault_backup(&root, &vault, 5_000)
            .await
            .unwrap();
        let second = write_replaced_vault_backup(&root, &vault, 5_000)
            .await
            .unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(list_replaced_vault_backups(&root).unwrap().len(), 2);
        let _ = fs::remove_dir_all(parent);
    }

    #[tokio::test]
    async fn a_failed_backup_is_reported_and_leaves_nothing_behind() {
        let parent = unique_root("replaced-failed");
        let vault = vault_at(&parent, "Ganbaru");
        fs::remove_file(crate::vault::database_path(&vault)).unwrap();
        fs::create_dir(crate::vault::database_path(&vault)).unwrap();
        let root = parent.join(REPLACED_VAULTS_DIRECTORY);

        let error = write_replaced_vault_backup(&root, &vault, 1_000)
            .await
            .unwrap_err();
        assert!(error.starts_with(REPLACED_VAULT_BACKUP_FAILED), "{error}");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn listing_a_missing_folder_is_empty() {
        assert!(
            list_replaced_vault_backups(&unique_root("replaced-missing"))
                .unwrap()
                .is_empty()
        );
    }
}
