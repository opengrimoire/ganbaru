//! Whole vault folders kept beside the vault in use instead of being deleted.
//!
//! A replacement whose replaced folder cannot be kept as a rollback copy, such as a restore over
//! a vault whose database is gone, keeps that folder whole as a sibling named
//! `{folder} before replacement {seconds}`. Starting a new vault after the database was lost
//! sets the old folder aside the same way. On Android these siblings live in application-private
//! storage, where no file manager reaches them, so Settings > Data lists them.

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

/// Moves the vault at `target`, whose marker records data while its database is gone, whole into
/// a kept sibling folder and returns that folder's name. Nothing is deleted. Any other vault, and
/// a vault an unfinished replacement or restore still needs, is refused before anything moves.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn set_aside_lost_vault(target: &Path) -> Result<String, String> {
    set_aside_lost_vault_at(target, unix_seconds()?)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn set_aside_lost_vault_at(target: &Path, seconds: u64) -> Result<String, String> {
    match super::refuse_lost_database(target) {
        Err(error) if error == super::VAULT_DATABASE_MISSING => {}
        Err(error) => return Err(error),
        Ok(()) => return Err(VAULT_NOT_LOST.to_string()),
    }
    if super::backup::swap_journal::has_unfinished_swap(target)
        || super::database_rollback::has_unfinished_restore(target)
    {
        return Err(super::backup::swap_journal::SWAP_RECOVERY_PENDING.to_string());
    }
    let kept = kept_folder_path_at(target, seconds)?;
    fs::rename(target, &kept).map_err(|error| format!("set the lost vault aside: {error}"))?;
    ganbaru_atomic_files::sync_directory_entries(parent_of(target)?)
        .map_err(|error| format!("set the lost vault aside: {error}"))?;
    kept.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| "the kept folder name is not valid text".to_string())
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

/// Sets the vault whose database is lost aside whole and starts a new empty vault in its place.
/// It is not recorded as a starter vault, so a first link backs it up like any other vault. A
/// linked phone is refused before anything moves.
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
        let kept = set_aside_lost_vault(&target)?;
        eprintln!("Vault with a lost database kept whole as '{kept}'");
        // The held lock names the folder that moved aside, so the new vault takes its own.
        super::vault_lock::process_vault_lock().release();
        ganbaru_atomic_files::create_private_directories(&target)
            .map_err(|error| format!("create Ganbaru folder: {error}"))?;
        super::initialize_vault(&target)
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

    #[test]
    fn a_lost_vault_moves_aside_whole_and_is_listed() {
        let target = vault_in("kept-folders-lost");
        lose_database(&target);
        fs::create_dir_all(target.join("assets/profile")).unwrap();
        fs::write(target.join("assets/profile/avatar.png"), b"image").unwrap();
        let before = inventory(&target);

        let id = set_aside_lost_vault_at(&target, 1_700_000_000).unwrap();

        assert_eq!(id, "Ganbaru before replacement 1700000000");
        assert!(!target.exists(), "the vault folder is free for a new vault");
        let kept = target.parent().unwrap().join(&id);
        assert_eq!(inventory(&kept), before, "nothing was deleted or changed");
        let listed = list_kept_vault_folders(&target).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
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
        let first = set_aside_lost_vault_at(&target, 50).unwrap();
        fs::create_dir_all(&target).unwrap();
        crate::vault::initialize_vault(&target).unwrap();
        lose_database(&target);

        let second = set_aside_lost_vault_at(&target, 50).unwrap();

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

        assert_eq!(
            set_aside_lost_vault_at(&target, 7).unwrap_err(),
            VAULT_NOT_LOST
        );

        fs::write(crate::vault::database_path(&target), b"SQLite format 3\0").unwrap();
        crate::vault::record_vault_data(&target, true).unwrap();
        assert_eq!(
            set_aside_lost_vault_at(&target, 7).unwrap_err(),
            VAULT_NOT_LOST,
            "a vault holding data with its database is refused too"
        );
        assert!(target.exists());
        assert!(list_kept_vault_folders(&target).unwrap().is_empty());
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

        assert!(set_aside_lost_vault_at(&target, 7).is_err());
        assert!(target.exists());
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
}
