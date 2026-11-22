//! Whole vault folders kept beside the vault in use instead of being deleted.
//!
//! A replacement whose replaced folder cannot be kept as a rollback copy, such as a restore over
//! a vault whose database is gone, keeps that folder whole as a sibling named
//! `{folder} before replacement {seconds}`. Starting a new vault after the database was lost
//! sets the old folder aside the same way. On Android these siblings live in application-private
//! storage, where no file manager reaches them, so Settings > Data lists them.

use super::backup::swap_journal::{self, Replacement};
use super::{MAX_VAULT_MANIFEST_BYTES, parse_vault_manifest};
use serde::Serialize;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Text between the vault folder name and the time in a kept folder's name.
const KEPT_FOLDER_INFIX: &str = " before replacement ";
/// Folder name used when the vault folder has no usable name.
const FALLBACK_FOLDER_NAME: &str = "Ganbaru";
const MILLIS_PER_SECOND: i64 = 1000;

/// One whole vault folder kept beside the vault in use.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeptVaultFolder {
    /// Folder name, which is also the identifier.
    pub id: String,
    /// Display name of the kept vault, when its marker is readable.
    pub display_name: Option<String>,
    /// When the folder was kept, in Unix milliseconds.
    pub kept_at_ms: i64,
    /// Bytes of the regular files inside it, without following links.
    pub size_bytes: u64,
}

fn folder_name(target: &Path) -> &str {
    target
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(FALLBACK_FOLDER_NAME)
}

fn parent_of(target: &Path) -> Result<&Path, String> {
    target
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())
}

fn kept_folder_name(target: &Path, seconds: u64) -> String {
    format!("{}{KEPT_FOLDER_INFIX}{seconds}", folder_name(target))
}

/// The time a kept folder of `target` named `name` was kept, in seconds, or `None` for any name
/// that is not exactly such a folder's name.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn parse_kept_folder_name(target: &Path, name: &str) -> Option<u64> {
    let digits = name
        .strip_prefix(folder_name(target))?
        .strip_prefix(KEPT_FOLDER_INFIX)?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn unix_seconds() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|error| format!("name the kept folder: {error}"))
}

/// A free sibling of `target` that keeps a replaced folder whole, named for the current time and
/// moved later when an earlier folder already took that second.
pub(crate) fn kept_folder_path(target: &Path) -> Result<PathBuf, String> {
    kept_folder_path_at(target, unix_seconds()?)
}

fn kept_folder_path_at(target: &Path, mut seconds: u64) -> Result<PathBuf, String> {
    let parent = parent_of(target)?;
    loop {
        let candidate = parent.join(kept_folder_name(target, seconds));
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(candidate),
            Err(error) => return Err(format!("name the kept folder: {error}")),
            Ok(_) => {
                seconds = seconds
                    .checked_add(1)
                    .ok_or_else(|| "kept folder time is exhausted".to_string())?;
            }
        }
    }
}

/// Lists the folders kept beside `target`, newest first. Links and files that merely carry such
/// a name are left out.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn list_kept_vault_folders(target: &Path) -> Result<Vec<KeptVaultFolder>, String> {
    let parent = parent_of(target)?;
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("read the kept vault folders: {error}")),
    };
    let mut folders = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("read the kept vault folders: {error}"))?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(seconds) = parse_kept_folder_name(target, &name) else {
            continue;
        };
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path();
        folders.push(KeptVaultFolder {
            display_name: kept_display_name(&path),
            kept_at_ms: i64::try_from(seconds)
                .ok()
                .and_then(|seconds| seconds.checked_mul(MILLIS_PER_SECOND))
                .unwrap_or(i64::MAX),
            size_bytes: folder_size(&path),
            id: name,
        });
    }
    folders.sort_by(|left, right| {
        right
            .kept_at_ms
            .cmp(&left.kept_at_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(folders)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn kept_display_name(folder: &Path) -> Option<String> {
    let contents = ganbaru_atomic_files::read_regular_file_capped(
        &super::manifest_path(folder),
        MAX_VAULT_MANIFEST_BYTES,
        ganbaru_atomic_files::FinalSymlink::Refuse,
    )
    .ok()?;
    parse_vault_manifest(&contents)
        .ok()
        .map(|manifest| manifest.display_name)
        .filter(|name| !name.trim().is_empty())
}

/// Sums the regular files below `folder` without following links. Entries that cannot be read
/// count as empty, so one unreadable file never hides the folder.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn folder_size(folder: &Path) -> u64 {
    let mut total = 0_u64;
    let mut pending = vec![folder.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file()
                && let Ok(metadata) = entry.metadata()
            {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

/// Error of a request to set aside a vault whose database is still there.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const VAULT_NOT_LOST: &str =
    "only a vault whose database is missing can be set aside to start a new one";

/// Error of a new vault start that finds an earlier staging folder holding data.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
const STAGING_HOLDS_DATA: &str =
    "an earlier new vault folder holds data, so a new vault was not started";

/// Name prefix of the folder a new vault is prepared in beside the lost one.
const NEW_VAULT_STAGING_PREFIX: &str = ".ganbaru.new-vault-";

/// The folder beside `target` where a new vault is prepared before it takes the place of the
/// lost one. It is named after the target, so vaults sharing a parent never share it.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn new_vault_staging_path(target: &Path) -> Result<PathBuf, String> {
    Ok(parent_of(target)?.join(format!(
        "{NEW_VAULT_STAGING_PREFIX}{}.staging",
        folder_name(target)
    )))
}

/// Refuses unless `target` is a vault whose marker records data while its database is gone, and
/// no unfinished rollback restore still needs it.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn refuse_unless_lost(target: &Path) -> Result<(), String> {
    match super::refuse_lost_database(target) {
        Err(error) if error == super::VAULT_DATABASE_MISSING => {}
        Err(error) => return Err(error),
        Ok(()) => return Err(VAULT_NOT_LOST.to_string()),
    }
    if super::database_rollback::has_unfinished_restore(target) {
        return Err(super::backup::swap_journal::SWAP_RECOVERY_PENDING.to_string());
    }
    Ok(())
}

/// Whether the folder at `staging` holds nothing but what preparing a new vault writes before
/// it is ever opened: directories, empty files, the lock file, and a marker that records no data.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn holds_only_an_unopened_vault(staging: &Path) -> Result<bool, String> {
    let read_error = |error: std::io::Error| format!("read the earlier new vault folder: {error}");
    let mut pending = vec![staging.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(read_error)? {
            let entry = entry.map_err(read_error)?;
            let kind = entry.file_type().map_err(read_error)?;
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
                continue;
            }
            if !kind.is_file() {
                return Ok(false);
            }
            let at_root = directory == staging;
            let name = entry.file_name();
            if at_root && name == super::vault_lock::VAULT_LOCK_FILE {
                continue;
            }
            if at_root && name == super::VAULT_MANIFEST_FILE {
                if super::read_vault_manifest(staging).is_ok_and(|manifest| !manifest.has_data) {
                    continue;
                }
                return Ok(false);
            }
            if entry.metadata().map_err(read_error)?.len() > 0 {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Removes the folder an interrupted new vault start left at `staging`, which no swap moved into
/// place, and refuses with [`STAGING_HOLDS_DATA`] when it holds anything such a start does not
/// write.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn clear_leftover_staging(staging: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(staging) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("read the earlier new vault folder: {error}")),
    };
    if !metadata.is_dir() || !holds_only_an_unopened_vault(staging)? {
        return Err(STAGING_HOLDS_DATA.to_string());
    }
    fs::remove_dir_all(staging)
        .map_err(|error| format!("remove the earlier new vault folder: {error}"))
}

/// A new vault started in place of a lost one.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Debug)]
pub(crate) struct StartedVault {
    /// The new vault, now at the place of the lost one.
    pub info: super::VaultInfo,
    /// Name of the kept folder that holds the lost vault whole.
    pub kept: String,
}

/// Starts a new empty vault in place of the vault at `target`, whose marker records data while
/// its database is gone, and keeps the lost vault whole in a kept sibling folder named for
/// `seconds`. Nothing is deleted.
///
/// The new vault is prepared beside the lost one first, and `activate` swaps it into place
/// through the crash-safe swap journal, so a crash at any point leaves either the lost vault or
/// the new one at `target`, and startup recovery settles the journal. An earlier unfinished swap
/// is settled first. Any other vault, and a vault an unfinished restore still needs, is refused
/// before anything moves.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn start_new_vault_at(
    target: &Path,
    seconds: u64,
    activate: impl FnOnce(&Path, &Path, Replacement) -> Result<(), String>,
) -> Result<StartedVault, String> {
    swap_journal::recover_or_refuse(target)?;
    refuse_unless_lost(target)?;
    let staging = new_vault_staging_path(target)?;
    clear_leftover_staging(&staging)?;
    ganbaru_atomic_files::create_private_directories(&staging)
        .map_err(|error| format!("create the new vault folder: {error}"))?;
    let staged =
        super::initialize_named_vault(&staging, Some(super::display_name_from_path(target)))?;
    let kept = kept_folder_path_at(target, seconds)?;
    let kept_name = kept
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| "the kept folder name is not valid text".to_string())?;
    activate(
        &staging,
        target,
        Replacement::Preserve {
            folder: kept,
            discard: false,
        },
    )?;
    // The preserved folder is never discarded, so its journal is settled right away.
    if let swap_journal::Recovery::Pending(reason) = swap_journal::recover(target)? {
        eprintln!("New vault start cleanup: {reason}");
    }
    let info = super::vault_info_from_path(target)?;
    if info.vault_id != staged.vault_id {
        return Err("the new vault did not take the place of the lost one".to_string());
    }
    Ok(StartedVault {
        info,
        kept: kept_name,
    })
}

/// Error of a new vault started after a lost database while this Android device is linked. The
/// vault error formatter recognizes it and asks the user to unlink first.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const LINKED_ANDROID_NEW_VAULT_ERROR: &str =
    "this phone is linked to other devices; unlink it before starting a new vault";

/// Refuses starting a new vault in place of the lost one while this Android device is `linked`,
/// since the linked devices still expect the lost vault here.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn refuse_new_vault_while_linked(
    platform: super::encrypted_backup::RestorePlatform,
    linked: bool,
) -> Result<(), String> {
    if platform == super::encrypted_backup::RestorePlatform::Android && linked {
        return Err(LINKED_ANDROID_NEW_VAULT_ERROR.to_string());
    }
    Ok(())
}

/// Lists the whole vault folders kept beside the one vault this Android device keeps.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn vault_kept_folders(app: tauri::AppHandle) -> Result<Vec<KeptVaultFolder>, String> {
    let target = super::default_vault_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || list_kept_vault_folders(&target))
        .await
        .map_err(|error| format!("kept vault folder worker failed: {error}"))?
}

/// Starts a new empty vault in place of the vault whose database is lost and keeps the lost one
/// whole beside it, through the crash-safe swap of [`start_new_vault_at`]. It is not recorded as
/// a starter vault, so a first link backs it up like any other vault. A linked phone is refused
/// before anything moves.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn vault_start_new_after_lost_database(
    app: tauri::AppHandle,
) -> Result<super::VaultInfo, String> {
    let transition = super::quiescence::reserve_vault_transition()?;
    refuse_new_vault_while_linked(
        super::encrypted_backup::PLATFORM,
        super::encrypted_backup::device_linked(&app)?,
    )?;
    crate::db::close_all_sqlite_pools_for_restore(&app).await?;
    let target = super::default_vault_path(&app)?;
    let creation_transition = transition.clone();
    let info = tauri::async_runtime::spawn_blocking(move || {
        let _transition = creation_transition;
        let started =
            start_new_vault_at(&target, unix_seconds()?, |staging, target, replacement| {
                // The lock moves with the swapped folders, so the new vault is fenced throughout.
                super::vault_lock::process_vault_lock().replace_with(staging, target, || {
                    swap_journal::swap(staging, target, replacement)
                })
            })?;
        eprintln!(
            "Vault with a lost database kept whole as '{}'",
            started.kept
        );
        Ok::<_, String>(started.info)
    })
    .await
    .map_err(|error| format!("new vault worker failed: {error}"))??;
    super::select_reserved_vault(&app, &info, transition).await?;
    Ok(info)
}

/// Reports that kept vault folders are listed only on Android.
#[cfg(all(not(target_os = "android"), any(test, target_os = "ios")))]
#[tauri::command]
pub fn vault_kept_folders() -> Result<Vec<KeptVaultFolder>, String> {
    Ok(Vec::new())
}

/// Reports that starting a new vault after a lost database is offered only on Android.
#[cfg(all(not(target_os = "android"), any(test, target_os = "ios")))]
#[tauri::command]
pub fn vault_start_new_after_lost_database() -> Result<super::VaultInfo, String> {
    Err("starting a new vault after a lost database is only available on Android".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn only_a_linked_android_device_is_refused_a_new_vault() {
        use crate::vault::encrypted_backup::RestorePlatform;
        assert_eq!(
            refuse_new_vault_while_linked(RestorePlatform::Android, true).unwrap_err(),
            LINKED_ANDROID_NEW_VAULT_ERROR
        );
        assert!(refuse_new_vault_while_linked(RestorePlatform::Android, false).is_ok());
        assert!(refuse_new_vault_while_linked(RestorePlatform::Desktop, true).is_ok());
    }

    fn vault_in(name: &str) -> PathBuf {
        let parent = crate::vault::tests::unique_path(name);
        fs::create_dir_all(&parent).unwrap();
        let target = parent.join("Ganbaru");
        fs::create_dir_all(&target).unwrap();
        crate::vault::initialize_vault(&target).unwrap();
        target
    }

    fn lose_database(target: &Path) {
        crate::vault::record_vault_data(target, true).unwrap();
        let database = crate::vault::database_path(target);
        if database.exists() {
            fs::remove_file(database).unwrap();
        }
    }

    /// Every file below `root` with its contents, keyed by its path relative to `root`.
    fn inventory(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory).unwrap().flatten() {
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    pending.push(path);
                } else {
                    let relative = path.strip_prefix(root).unwrap().to_path_buf();
                    files.insert(relative, fs::read(&path).unwrap());
                }
            }
        }
        files
    }

    /// Starts a new vault in place of `target` with the real swap.
    fn start_new(target: &Path, seconds: u64) -> Result<StartedVault, String> {
        start_new_vault_at(target, seconds, swap_journal::swap)
    }

    fn kept_path(target: &Path, name: &str) -> PathBuf {
        target.parent().unwrap().join(name)
    }

    /// Asserts that `target` holds a new empty vault named after its folder and that nothing a
    /// start leaves while unfinished remains beside it.
    fn assert_new_vault_in_place(target: &Path) {
        let info = crate::vault::vault_info_from_path(target).unwrap();
        assert_eq!(info.display_name, "Ganbaru");
        assert!(!crate::vault::read_vault_manifest(target).unwrap().has_data);
        assert!(crate::vault::refuse_lost_database(target).is_ok());
        assert!(!swap_journal::has_unfinished_swap(target));
        assert!(!new_vault_staging_path(target).unwrap().exists());
    }

    #[test]
    fn a_lost_vault_moves_aside_whole_and_is_listed() {
        let target = vault_in("kept-folders-lost");
        lose_database(&target);
        fs::create_dir_all(target.join("assets/profile")).unwrap();
        fs::write(target.join("assets/profile/avatar.png"), b"image").unwrap();
        let lost_id = crate::vault::vault_info_from_path(&target)
            .unwrap()
            .vault_id;
        let before = inventory(&target);

        let started = start_new(&target, 1_700_000_000).unwrap();

        assert_eq!(started.kept, "Ganbaru before replacement 1700000000");
        assert_ne!(started.info.vault_id, lost_id);
        assert_eq!(
            fs::canonicalize(&started.info.path).unwrap(),
            fs::canonicalize(&target).unwrap()
        );
        assert_new_vault_in_place(&target);
        let kept = kept_path(&target, &started.kept);
        assert_eq!(inventory(&kept), before, "nothing was deleted or changed");
        let listed = list_kept_vault_folders(&target).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, started.kept);
        assert_eq!(listed[0].kept_at_ms, 1_700_000_000_000);
        assert_eq!(listed[0].display_name.as_deref(), Some("Ganbaru"));
        assert_eq!(
            listed[0].size_bytes,
            before.values().map(|bytes| bytes.len() as u64).sum::<u64>()
        );
    }

    #[test]
    fn setting_aside_in_the_same_second_takes_the_next_free_name() {
        let target = vault_in("kept-folders-collision");
        lose_database(&target);
        let first = start_new(&target, 50).unwrap().kept;
        lose_database(&target);

        let second = start_new(&target, 50).unwrap().kept;

        assert_eq!(first, "Ganbaru before replacement 50");
        assert_eq!(second, "Ganbaru before replacement 51");
        let ids: Vec<String> = list_kept_vault_folders(&target)
            .unwrap()
            .into_iter()
            .map(|folder| folder.id)
            .collect();
        assert_eq!(ids, vec![second, first]);
    }

    #[test]
    fn a_vault_whose_database_is_there_is_never_set_aside() {
        let target = vault_in("kept-folders-healthy");
        let before = inventory(&target);

        assert_eq!(start_new(&target, 7).unwrap_err(), VAULT_NOT_LOST);

        fs::write(crate::vault::database_path(&target), b"SQLite format 3\0").unwrap();
        crate::vault::record_vault_data(&target, true).unwrap();
        assert_eq!(
            start_new(&target, 7).unwrap_err(),
            VAULT_NOT_LOST,
            "a vault holding data with its database is refused too"
        );
        assert!(target.exists());
        assert!(list_kept_vault_folders(&target).unwrap().is_empty());
        assert!(!new_vault_staging_path(&target).unwrap().exists());
        let after = inventory(&target);
        assert!(
            before.keys().all(|path| after.contains_key(path)),
            "every file stays in place"
        );
    }

    #[test]
    fn a_folder_without_a_vault_marker_is_refused() {
        let parent = crate::vault::tests::unique_path("kept-folders-unmarked");
        let target = parent.join("Ganbaru");
        fs::create_dir_all(&target).unwrap();

        assert!(start_new(&target, 7).is_err());
        assert!(target.exists());
        assert!(!new_vault_staging_path(&target).unwrap().exists());
    }

    #[test]
    fn a_failed_swap_leaves_the_lost_vault_in_place() {
        let target = vault_in("kept-folders-failed-swap");
        lose_database(&target);
        let before = inventory(&target);

        let error = start_new_vault_at(&target, 9, |_, _, _| Err("swap failed".to_string()));

        assert_eq!(error.unwrap_err(), "swap failed");
        assert_eq!(inventory(&target), before);
        assert!(list_kept_vault_folders(&target).unwrap().is_empty());
        start_new(&target, 9).unwrap();
        assert_new_vault_in_place(&target);
    }

    #[test]
    fn an_earlier_new_vault_folder_holding_data_is_never_removed() {
        let target = vault_in("kept-folders-staging-data");
        lose_database(&target);
        let before = inventory(&target);
        let staging = new_vault_staging_path(&target).unwrap();
        fs::create_dir_all(staging.join("assets")).unwrap();
        fs::write(crate::vault::database_path(&staging), b"SQLite format 3\0").unwrap();
        fs::write(staging.join("assets/photo.png"), b"image").unwrap();
        let staged = inventory(&staging);

        assert_eq!(start_new(&target, 3).unwrap_err(), STAGING_HOLDS_DATA);

        assert_eq!(inventory(&staging), staged);
        assert_eq!(inventory(&target), before);
        fs::remove_dir_all(&staging).unwrap();
        fs::create_dir_all(&staging).unwrap();
        crate::vault::initialize_vault(&staging).unwrap();
        crate::vault::record_vault_data(&staging, true).unwrap();
        assert_eq!(
            start_new(&target, 3).unwrap_err(),
            STAGING_HOLDS_DATA,
            "a marker that records data is kept too"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_earlier_new_vault_folder_holding_a_link_is_never_removed() {
        let target = vault_in("kept-folders-staging-link");
        lose_database(&target);
        let staging = new_vault_staging_path(&target).unwrap();
        fs::create_dir_all(&staging).unwrap();
        std::os::unix::fs::symlink(&target, staging.join("linked")).unwrap();

        assert_eq!(start_new(&target, 3).unwrap_err(), STAGING_HOLDS_DATA);
        assert!(fs::symlink_metadata(staging.join("linked")).is_ok());
    }

    #[test]
    fn the_listing_takes_only_exact_kept_folder_names() {
        let target = vault_in("kept-folders-listing");
        let parent = target.parent().unwrap();
        for name in [
            "Ganbaru before replacement 10",
            "Ganbaru before replacement ",
            "Ganbaru before replacement 1x",
            "Other before replacement 10",
            "Ganbaru before replacement -1",
            ".ganbaru.swap-Ganbaru.replaced",
        ] {
            fs::create_dir_all(parent.join(name)).unwrap();
        }
        fs::write(parent.join("Ganbaru before replacement 11"), b"file").unwrap();

        let listed = list_kept_vault_folders(&target).unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "Ganbaru before replacement 10");
        assert_eq!(
            listed[0].display_name, None,
            "a folder without a marker has no name"
        );
        assert_eq!(listed[0].size_bytes, 0);
    }

    #[cfg(unix)]
    #[test]
    fn the_listing_never_follows_links() {
        let target = vault_in("kept-folders-links");
        let parent = target.parent().unwrap();
        let outside = crate::vault::tests::unique_path("kept-folders-links-outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("large"), vec![0_u8; 4096]).unwrap();
        std::os::unix::fs::symlink(&outside, parent.join("Ganbaru before replacement 3")).unwrap();
        let kept = parent.join("Ganbaru before replacement 4");
        fs::create_dir_all(&kept).unwrap();
        std::os::unix::fs::symlink(&outside, kept.join("linked")).unwrap();

        let listed = list_kept_vault_folders(&target).unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "Ganbaru before replacement 4");
        assert_eq!(listed[0].size_bytes, 0);
    }

    /// A point where the process may end while a new vault starts.
    #[derive(Clone, Copy, Debug)]
    enum CrashWindow {
        /// Preparing the new vault created its skeleton but no marker.
        StagingPartial,
        /// The new vault is prepared and no swap began.
        StagingReady,
        /// The swap reached `crash`.
        Swap(swap_journal::PreserveCrash),
        /// The swap finished and its journal was not settled yet.
        Swapped,
    }

    /// Leaves the folders as a process that ended at `window` while starting a new vault in
    /// place of `target` would.
    fn crash_at(target: &Path, window: CrashWindow) {
        let staging = new_vault_staging_path(target).unwrap();
        if let CrashWindow::StagingPartial = window {
            fs::create_dir_all(staging.join("assets")).unwrap();
            fs::create_dir_all(staging.join("projects")).unwrap();
            fs::write(crate::vault::database_path(&staging), b"").unwrap();
            return;
        }
        let crashed = start_new_vault_at(target, 20, |staging, target, replacement| {
            let Replacement::Preserve { folder, .. } = &replacement else {
                panic!("a new vault preserves the lost one");
            };
            match window {
                CrashWindow::StagingPartial | CrashWindow::StagingReady => {}
                CrashWindow::Swap(crash) => {
                    swap_journal::crash_preserve_swap(staging, target, folder, crash)?;
                }
                CrashWindow::Swapped => swap_journal::swap(staging, target, replacement)?,
            }
            Err("the process ended".to_string())
        });
        assert_eq!(crashed.unwrap_err(), "the process ended");
    }

    #[test]
    fn a_crash_in_every_window_leaves_a_vault_in_place_and_keeps_the_lost_one() {
        use swap_journal::PreserveCrash;
        let windows = [
            (CrashWindow::StagingPartial, false),
            (CrashWindow::StagingReady, false),
            (CrashWindow::Swap(PreserveCrash::Journaled), false),
            (CrashWindow::Swap(PreserveCrash::TargetMoved), false),
            (CrashWindow::Swap(PreserveCrash::BothMoved), true),
            (CrashWindow::Swap(PreserveCrash::Activated), true),
            (CrashWindow::Swapped, true),
        ];
        for (window, swapped) in windows {
            let target = vault_in("kept-folders-crash");
            lose_database(&target);
            fs::create_dir_all(target.join("assets/profile")).unwrap();
            fs::write(target.join("assets/profile/avatar.png"), b"image").unwrap();
            let lost = inventory(&target);

            crash_at(&target, window);
            // Startup recovery runs before anything opens the vault.
            assert_eq!(
                swap_journal::recover(&target).unwrap(),
                swap_journal::Recovery::Settled,
                "{window:?}"
            );

            assert!(
                crate::vault::vault_info_from_path(&target).is_ok(),
                "{window:?} leaves a vault in place"
            );
            if swapped {
                assert_new_vault_in_place(&target);
            } else {
                assert_eq!(inventory(&target), lost, "{window:?} keeps the lost vault");
                assert!(list_kept_vault_folders(&target).unwrap().is_empty());
                // Starting over finishes what the crash interrupted.
                start_new(&target, 30).unwrap();
                assert_new_vault_in_place(&target);
            }
            let kept = list_kept_vault_folders(&target).unwrap();
            assert_eq!(kept.len(), 1, "{window:?}");
            assert_eq!(
                inventory(&kept_path(&target, &kept[0].id)),
                lost,
                "{window:?} keeps every file of the lost vault"
            );
        }
    }
}
