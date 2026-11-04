//! Executable lookup shared by provider resolution and Settings discovery.
//!
//! Lookup skips `PATH` entries that resolve against the current directory and candidates that
//! any local user could replace, so a planted program never becomes a provider silently.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Package manager folders outside the user's home that exist only on one platform.
#[cfg(target_os = "macos")]
pub const PLATFORM_EXECUTABLE_DIRECTORIES: &[&str] = &["/opt/homebrew/bin"];
/// Package manager folders outside the user's home that exist only on one platform.
#[cfg(target_os = "linux")]
pub const PLATFORM_EXECUTABLE_DIRECTORIES: &[&str] = &["/home/linuxbrew/.linuxbrew/bin"];
/// Package manager folders outside the user's home that exist only on one platform.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub const PLATFORM_EXECUTABLE_DIRECTORIES: &[&str] = &[];

/// Absolute directories named by a `PATH` value, in order.
///
/// Empty and relative entries are dropped because they resolve against whatever directory the
/// process happens to run in.
pub fn path_search_directories(value: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(value)
        .filter(|directory| directory.is_absolute())
        .collect()
}

/// Whether `candidate` is an executable regular file that other local users cannot replace.
///
/// On Unix the file, its directory, and the directory of its resolved target must not be
/// world-writable. Group write stays allowed because user private groups make it normal.
pub fn is_safe_executable_candidate(candidate: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        const WORLD_WRITABLE: u32 = 0o002;
        const ANY_EXECUTE: u32 = 0o111;
        let Ok(metadata) = std::fs::metadata(candidate) else {
            return false;
        };
        let mode = metadata.permissions().mode();
        if !metadata.is_file() || mode & ANY_EXECUTE == 0 || mode & WORLD_WRITABLE != 0 {
            return false;
        }
        let directory_is_world_writable = |directory: Option<&Path>| {
            directory.is_none_or(|directory| {
                std::fs::metadata(directory)
                    .map(|metadata| metadata.permissions().mode() & WORLD_WRITABLE != 0)
                    .unwrap_or(true)
            })
        };
        if directory_is_world_writable(candidate.parent()) {
            return false;
        }
        let Ok(resolved) = std::fs::canonicalize(candidate) else {
            return false;
        };
        !directory_is_world_writable(resolved.parent())
    }
    #[cfg(not(unix))]
    {
        candidate.is_file()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn program(directory: &Path, name: &str, mode: u32) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn path_search_skips_empty_and_relative_entries() {
        let directories = path_search_directories(OsStr::new("/usr/bin::bin:./tools:/opt/x/bin"));
        assert_eq!(
            directories,
            vec![PathBuf::from("/usr/bin"), PathBuf::from("/opt/x/bin")]
        );
    }

    #[test]
    fn world_writable_programs_and_folders_are_skipped_while_group_write_is_allowed() {
        let root = crate::probe_directory::ProbeDirectory::create().unwrap();
        let private = root.join("private");
        std::fs::create_dir(&private).unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(is_safe_executable_candidate(&program(
            &private, "owner", 0o755
        )));
        assert!(is_safe_executable_candidate(&program(
            &private, "group", 0o775
        )));
        assert!(!is_safe_executable_candidate(&program(
            &private, "world", 0o777
        )));
        assert!(!is_safe_executable_candidate(&program(
            &private, "plain", 0o644
        )));
        assert!(!is_safe_executable_candidate(&private));

        let shared = root.join("shared");
        std::fs::create_dir(&shared).unwrap();
        let planted = program(&shared, "planted", 0o755);
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o1777)).unwrap();
        assert!(!is_safe_executable_candidate(&planted));

        let link = private.join("linked");
        std::os::unix::fs::symlink(&planted, &link).unwrap();
        assert!(!is_safe_executable_candidate(&link));
    }
}
