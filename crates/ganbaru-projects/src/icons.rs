//! Exact sizes of managed project icon files referenced by groups, projects, and custom emoji.
//!
//! A reference to a `project-icons` file records the file size beside it, so the file can be transferred and
//! checked as a managed asset.

use std::fs;
use std::io;
use std::path::Path;

/// Largest project icon file, in bytes.
pub const PROJECT_ICON_MAX_BYTES: u64 = 3 * 1024 * 1024;
/// Folder under the vault `assets` folder that holds project icon files.
pub const PROJECT_ICON_DIR: &str = "project-icons";
/// Prefix of a group or project icon that references a managed icon file.
pub const PROJECT_ICON_REFERENCE_PREFIX: &str = "asset:";
/// Icon a group or project falls back to when its icon file is gone: the canonical encoding for no icon.
pub const MISSING_ICON_FALLBACK: &str = "none";

/// Relative asset path such as `project-icons/<file>` named by a group or project icon, when it names one.
pub fn icon_asset_path(icon: &str) -> Option<&str> {
    icon.trim()
        .strip_prefix(PROJECT_ICON_REFERENCE_PREFIX)
        .filter(|path| is_project_icon_path(path))
}

fn is_project_icon_path(relative_path: &str) -> bool {
    relative_path
        .strip_prefix(PROJECT_ICON_DIR)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|file_name| {
            !file_name.is_empty()
                && !file_name.contains('/')
                && !file_name.contains('\\')
                && file_name != "."
                && file_name != ".."
        })
}

/// Size of the icon file at `relative_path` under the vault, or `None` when the file is missing, is not a regular
/// file, is empty, or exceeds [`PROJECT_ICON_MAX_BYTES`].
pub fn project_icon_byte_size(
    vault_root: &Path,
    relative_path: &str,
) -> Result<Option<i64>, String> {
    let relative_path = relative_path.trim();
    if !is_project_icon_path(relative_path) {
        return Err("project icon path must name a file under project-icons".to_string());
    }
    let path = vault_root.join("assets").join(relative_path);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read project icon size: {error}")),
    };
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > PROJECT_ICON_MAX_BYTES
    {
        return Ok(None);
    }
    i64::try_from(metadata.len())
        .map(Some)
        .map_err(|_| "project icon size is out of range".to_string())
}

/// Size to store beside a group or project icon. Icons that name no file store no size, and a named file that
/// cannot be stored is refused.
pub(crate) fn required_icon_byte_size(
    vault_root: &Path,
    icon: &str,
) -> Result<Option<i64>, String> {
    match icon_asset_path(icon) {
        None => Ok(None),
        Some(relative_path) => required_asset_byte_size(vault_root, relative_path).map(Some),
    }
}

/// Size to store beside a custom emoji asset path. A missing or oversized file is refused.
pub(crate) fn required_asset_byte_size(
    vault_root: &Path,
    relative_path: &str,
) -> Result<i64, String> {
    project_icon_byte_size(vault_root, relative_path)?.ok_or_else(|| {
        "project icon file is missing or larger than the icon size limit".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_asset_path_reads_only_managed_icon_references() {
        assert_eq!(
            icon_asset_path("asset:project-icons/a.png"),
            Some("project-icons/a.png")
        );
        assert_eq!(icon_asset_path("lucide:folder"), None);
        assert_eq!(icon_asset_path("asset:project-icons/../a.png"), None);
        assert_eq!(icon_asset_path("asset:other/a.png"), None);
    }

    #[test]
    fn project_icon_byte_size_reads_regular_files_within_the_limit() {
        let root =
            std::env::temp_dir().join(format!("ganbaru-projects-icons-{}", std::process::id()));
        let folder = root.join("assets").join(PROJECT_ICON_DIR);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("icon.png"), [1_u8, 2, 3]).unwrap();
        fs::write(folder.join("empty.png"), []).unwrap();
        assert_eq!(
            project_icon_byte_size(&root, "project-icons/icon.png").unwrap(),
            Some(3)
        );
        assert_eq!(
            project_icon_byte_size(&root, "project-icons/empty.png").unwrap(),
            None
        );
        assert_eq!(
            project_icon_byte_size(&root, "project-icons/missing.png").unwrap(),
            None
        );
        assert!(project_icon_byte_size(&root, "project-icons/../icon.png").is_err());
        assert!(required_asset_byte_size(&root, "project-icons/missing.png").is_err());
        assert_eq!(
            required_icon_byte_size(&root, "lucide:folder").unwrap(),
            None
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
