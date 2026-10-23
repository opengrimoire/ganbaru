//! Exact sizes of managed project icon files referenced by groups, projects, and custom emoji.
//!
//! A reference to a `project-icons` file records the file size beside it, so the file can be transferred and
//! checked as a managed asset. Paths follow the sync asset class exactly, `project-icons/{sha256}.{ext}`.

use std::fs;
use std::io;
use std::path::Path;

use ganbaru_sync::domains::projects::{
    ICON_REFERENCE_PREFIX, MAX_ICON_FILE_BYTES, parse_icon_path,
};
use ganbaru_sync::manifest::vault::projects::ICON_CLASS;

/// Largest project icon file, in bytes.
pub const PROJECT_ICON_MAX_BYTES: u64 = MAX_ICON_FILE_BYTES as u64;
/// Folder under the vault `assets` folder that holds project icon files.
pub const PROJECT_ICON_DIR: &str = ICON_CLASS.name;
/// Prefix of a group or project icon that references a managed icon file.
pub const PROJECT_ICON_REFERENCE_PREFIX: &str = ICON_REFERENCE_PREFIX;
/// Icon a group or project falls back to when its icon file is gone: the canonical encoding for no icon.
pub const MISSING_ICON_FALLBACK: &str = "none";

/// Relative asset path such as `project-icons/<file>` named by a group or project icon, when it names one exactly.
pub fn icon_asset_path(icon: &str) -> Option<&str> {
    icon.strip_prefix(PROJECT_ICON_REFERENCE_PREFIX)
        .filter(|path| is_project_icon_path(path))
}

/// Whether a relative path is exactly a managed icon file, `project-icons/{sha256}.{ext}`.
pub fn is_project_icon_path(relative_path: &str) -> bool {
    parse_icon_path(relative_path).is_ok()
}

/// Size of the icon file at `relative_path` under the vault, or `None` when the file is missing, is not a regular
/// file, is empty, or exceeds [`PROJECT_ICON_MAX_BYTES`].
pub fn project_icon_byte_size(
    vault_root: &Path,
    relative_path: &str,
) -> Result<Option<i64>, String> {
    if !is_project_icon_path(relative_path) {
        return Err(
            "project icon path must name a content-addressed file under project-icons".to_string(),
        );
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

    fn icon_name(digit: char, extension: &str) -> String {
        format!("{}.{extension}", digit.to_string().repeat(64))
    }

    #[test]
    fn icon_asset_path_reads_only_managed_icon_references() {
        let path = format!("project-icons/{}", icon_name('a', "png"));
        assert_eq!(
            icon_asset_path(&format!("asset:{path}")),
            Some(path.as_str())
        );
        assert_eq!(icon_asset_path(&format!(" asset:{path}")), None);
        assert_eq!(icon_asset_path("asset:project-icons/a.png"), None);
        assert_eq!(icon_asset_path("lucide:folder"), None);
        assert_eq!(icon_asset_path("asset:project-icons/../a.png"), None);
        assert_eq!(
            icon_asset_path(&format!("asset:other/{}", icon_name('a', "png"))),
            None
        );
    }

    #[test]
    fn project_icon_byte_size_reads_regular_files_within_the_limit() {
        let root =
            std::env::temp_dir().join(format!("ganbaru-projects-icons-{}", std::process::id()));
        let folder = root.join("assets").join(PROJECT_ICON_DIR);
        fs::create_dir_all(&folder).unwrap();
        let (icon, empty, missing) = (
            icon_name('1', "png"),
            icon_name('2', "png"),
            icon_name('3', "png"),
        );
        fs::write(folder.join(&icon), [1_u8, 2, 3]).unwrap();
        fs::write(folder.join(&empty), []).unwrap();
        assert_eq!(
            project_icon_byte_size(&root, &format!("project-icons/{icon}")).unwrap(),
            Some(3)
        );
        assert_eq!(
            project_icon_byte_size(&root, &format!("project-icons/{empty}")).unwrap(),
            None
        );
        assert_eq!(
            project_icon_byte_size(&root, &format!("project-icons/{missing}")).unwrap(),
            None
        );
        assert!(project_icon_byte_size(&root, "project-icons/../icon.png").is_err());
        assert!(required_asset_byte_size(&root, &format!("project-icons/{missing}")).is_err());
        assert_eq!(
            required_icon_byte_size(&root, "lucide:folder").unwrap(),
            None
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
