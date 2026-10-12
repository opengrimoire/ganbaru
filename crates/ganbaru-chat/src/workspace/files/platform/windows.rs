use super::*;
#[cfg(windows)]
use ::windows as windows_crate;

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowsFileIdentity {
    volume_serial_number: u64,
    file_id: [u8; 16],
}

#[cfg(windows)]
fn windows_file_identity(file: &File) -> std::io::Result<WindowsFileIdentity> {
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows_crate::Win32::Foundation::HANDLE;
    use windows_crate::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };

    let mut info = FILE_ID_INFO::default();
    let info_size = u32::try_from(size_of::<FILE_ID_INFO>())
        .map_err(|_| std::io::Error::other("Windows file identity size is unsupported"))?;
    // SAFETY: `file` keeps a live handle open for this call. `info` is initialized
    // writable storage of exactly `info_size` bytes, the selected information class
    // matches its type, and Windows does not retain the output pointer.
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            info_size,
        )
    }
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(WindowsFileIdentity {
        volume_serial_number: info.VolumeSerialNumber,
        file_id: info.FileId.Identifier,
    })
}

#[cfg(windows)]
fn windows_regular_file_state(
    path: &Path,
    relative_path: &str,
) -> ChatResult<(WindowsFileIdentity, String)> {
    use std::os::windows::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    let mut file = options
        .open(path)
        .map_err(|_| workspace_file_recovery_error())?;
    let metadata = file
        .metadata()
        .map_err(|_| workspace_file_recovery_error())?;
    if !metadata.is_file() || windows_metadata_is_reparse(&metadata) {
        return Err(workspace_file_recovery_error());
    }
    let identity = windows_file_identity(&file).map_err(|_| workspace_file_recovery_error())?;
    let (revision, _) = revision_and_permissions(&mut file, relative_path)?;
    Ok((identity, revision))
}

#[cfg(windows)]
fn windows_metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

#[cfg(windows)]
fn windows_open_directory(path: &Path) -> ChatResult<File> {
    use std::os::windows::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0);
    let file = options.open(path).map_err(file_error)?;
    let metadata = file.metadata().map_err(file_error)?;
    if !metadata.is_dir() || windows_metadata_is_reparse(&metadata) {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace directory is unavailable or uses a reparse point",
            false,
        ));
    }
    Ok(file)
}

#[cfg(windows)]
fn windows_workspace_directory_chain(
    root: &Path,
    relative_path: &str,
) -> ChatResult<(PathBuf, Vec<File>)> {
    let mut path = root.to_path_buf();
    let mut handles = vec![windows_open_directory(&path)?];
    for component in Path::new(relative_path).components() {
        let Component::Normal(name) = component else {
            return Err(ChatError::validation(
                "relativePath",
                "Workspace path must be a normalized relative path",
            ));
        };
        path.push(name);
        handles.push(windows_open_directory(&path)?);
    }
    Ok((path, handles))
}

#[cfg(windows)]
fn windows_open_regular_file(root: &Path, relative_path: &str) -> ChatResult<(Vec<File>, File)> {
    use std::os::windows::fs::OpenOptionsExt;

    let relative = Path::new(relative_path);
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent = parent
        .to_str()
        .ok_or_else(|| ChatError::validation("relativePath", "Workspace path is unsupported"))?;
    let (parent_path, handles) = windows_workspace_directory_chain(root, parent)?;
    let name = relative
        .file_name()
        .ok_or_else(|| ChatError::validation("relativePath", "Workspace path is invalid"))?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    let file = options.open(parent_path.join(name)).map_err(file_error)?;
    let metadata = file.metadata().map_err(file_error)?;
    if !metadata.is_file() || windows_metadata_is_reparse(&metadata) {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace file is unavailable or uses a reparse point",
            false,
        ));
    }
    Ok((handles, file))
}

#[cfg(windows)]
pub(in crate::workspace::files) fn secure_workspace_file(
    root: &Path,
    relative_path: &str,
) -> ChatResult<SecureWorkspaceFile> {
    let (parent_handles, file) = windows_open_regular_file(root, relative_path)?;
    Ok(SecureWorkspaceFile {
        file,
        _parent_handles: parent_handles,
    })
}

#[cfg(windows)]
pub(in crate::workspace::files) fn secure_directory_entries(
    root: &Path,
    relative_path: &str,
) -> ChatResult<(Vec<SecureDirectoryEntry>, bool)> {
    use std::os::windows::fs::OpenOptionsExt;

    let (directory, _handles) = windows_workspace_directory_chain(root, relative_path)?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in fs::read_dir(&directory).map_err(file_error)? {
        let entry = entry.map_err(file_error)?;
        let Some(display_name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        let mut options = OpenOptions::new();
        options
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0);
        let Ok(child) = options.open(entry.path()) else {
            continue;
        };
        let Ok(metadata) = child.metadata() else {
            continue;
        };
        if windows_metadata_is_reparse(&metadata) || (!metadata.is_file() && !metadata.is_dir()) {
            continue;
        }
        entries.push(SecureDirectoryEntry {
            display_name,
            directory: metadata.is_dir(),
            byte_size: metadata.is_file().then_some(metadata.len()),
        });
        if entries.len() >= MAX_DIRECTORY_ENTRIES {
            truncated = true;
            break;
        }
    }
    Ok((entries, truncated))
}

#[cfg(windows)]
pub(in crate::workspace::files) fn workspace_regular_file_permissions(
    root: &Path,
    relative_path: &str,
    field: &str,
) -> ChatResult<fs::Permissions> {
    let (_parents, file) = windows_open_regular_file(root, relative_path)
        .map_err(|_| ChatError::validation(field, "Workspace path is not a regular file"))?;
    file.metadata()
        .map(|metadata| metadata.permissions())
        .map_err(|_| ChatError::validation(field, "Workspace path is not a regular file"))
}

#[cfg(windows)]
pub(in crate::workspace::files) fn create_workspace_file_exclusively(
    root: &Path,
    relative_path: &str,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
    conflict_message: &str,
) -> ChatResult<()> {
    use std::os::windows::fs::OpenOptionsExt;

    let relative = Path::new(relative_path);
    let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent_relative = parent_relative
        .to_str()
        .ok_or_else(|| ChatError::validation("relativePath", "Workspace path is unsupported"))?;
    let (parent, _handles) = windows_workspace_directory_chain(root, parent_relative)?;
    let target = parent.join(
        relative
            .file_name()
            .ok_or_else(|| ChatError::validation("relativePath", "Workspace path is invalid"))?,
    );
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    let mut file = options.open(&target).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            ChatError::new(ChatErrorCode::Conflict, conflict_message, true)
        } else {
            workspace_file_write_error()
        }
    })?;
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        return Err(workspace_file_recovery_error());
    }
    if let Some(permissions) = permissions {
        if file.set_permissions(permissions).is_err() {
            drop(file);
            return Err(workspace_file_recovery_error());
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(in crate::workspace::files) fn delete_workspace_file_atomically(
    root: &Path,
    relative_path: &str,
    expected_revision: &str,
) -> ChatResult<()> {
    let relative = Path::new(relative_path);
    let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent_relative = parent_relative
        .to_str()
        .ok_or_else(|| ChatError::validation("relativePath", "Workspace path is unsupported"))?;
    let (parent, parent_handles) = windows_workspace_directory_chain(root, parent_relative)?;
    let target = parent.join(
        relative
            .file_name()
            .ok_or_else(|| ChatError::validation("relativePath", "Workspace path is invalid"))?,
    );
    let (_current_parents, mut current) = windows_open_regular_file(root, relative_path)?;
    let current_identity =
        windows_file_identity(&current).map_err(|_| workspace_file_write_error())?;
    let (current_revision, _) = revision_and_permissions(&mut current, relative_path)?;
    if current_revision != expected_revision {
        return Err(stale_workspace_file_error());
    }
    drop(current);
    let backup = windows_private_artifact_path(&parent, "backup")
        .map_err(|_| workspace_file_recovery_error())?;
    windows_move_file_no_replace(&target, &backup).map_err(|_| workspace_file_recovery_error())?;
    let displaced_matches =
        windows_regular_file_state(&backup, relative_path).is_ok_and(|(identity, revision)| {
            identity == current_identity && revision == expected_revision
        });
    if !displaced_matches {
        // The backup name is no longer proven to identify the displaced file.
        // Preserve it for explicit recovery instead of installing unknown bytes.
        return Err(workspace_file_recovery_error());
    }
    let removed = fs::remove_file(&backup).map_err(|_| workspace_file_recovery_error());
    drop(parent_handles);
    removed
}

#[cfg(windows)]
pub(in crate::workspace::files) fn write_workspace_text_atomically(
    root: &Path,
    relative_path: &str,
    contents: &str,
    expected_revision: &str,
) -> ChatResult<()> {
    use std::os::windows::fs::OpenOptionsExt;

    let relative = Path::new(relative_path);
    let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent_relative = parent_relative
        .to_str()
        .ok_or_else(|| ChatError::validation("relativePath", "Workspace path is unsupported"))?;
    let (parent, _parent_handles) = windows_workspace_directory_chain(root, parent_relative)?;
    let target = parent.join(
        relative
            .file_name()
            .ok_or_else(|| ChatError::validation("relativePath", "Workspace path is invalid"))?,
    );
    let (_current_parents, mut current) = windows_open_regular_file(root, relative_path)?;
    let current_identity =
        windows_file_identity(&current).map_err(|_| workspace_file_write_error())?;
    let (current_revision, permissions) = revision_and_permissions(&mut current, relative_path)?;
    if current_revision != expected_revision {
        return Err(stale_workspace_file_error());
    }
    let replacement_revision = workspace_file_revision(relative_path, contents.as_bytes());

    let temporary = windows_temporary_path(&parent);
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    let mut replacement = options
        .open(&temporary)
        .map_err(|_| workspace_file_write_error())?;
    let prepared = replacement
        .write_all(contents.as_bytes())
        .and_then(|_| replacement.set_permissions(permissions))
        .and_then(|_| replacement.sync_all());
    let replacement_identity = if prepared.is_ok() {
        windows_file_identity(&replacement).ok()
    } else {
        None
    };
    drop(replacement);
    let Some(replacement_identity) = replacement_identity else {
        return Err(workspace_file_recovery_error());
    };
    drop(current);

    let backup = windows_private_artifact_path(&parent, "backup")
        .map_err(|_| workspace_file_recovery_error())?;
    if windows_replace_file(&target, &temporary, &backup).is_err() {
        // ReplaceFileW can move the original to its backup before reporting failure.
        // Restore that documented partial state only after verifying the original bytes.
        let target_exists = fs::symlink_metadata(&target).is_ok();
        let backup_is_original =
            windows_regular_file_state(&backup, relative_path).is_ok_and(|(identity, revision)| {
                identity == current_identity && revision == expected_revision
            });
        if !target_exists && backup_is_original {
            if windows_move_file_no_replace(&backup, &target).is_err() {
                return Err(workspace_file_recovery_error());
            }
        } else if !target_exists || fs::symlink_metadata(&backup).is_ok() {
            return Err(workspace_file_recovery_error());
        }
        return Err(workspace_file_recovery_error());
    }

    let displaced_is_original =
        windows_regular_file_state(&backup, relative_path).is_ok_and(|(identity, revision)| {
            identity == current_identity && revision == expected_revision
        });
    let installed_is_replacement =
        windows_regular_file_state(&target, relative_path).is_ok_and(|(identity, revision)| {
            identity == replacement_identity && revision == replacement_revision
        });
    if !displaced_is_original {
        // Never install an unverified backup as a rollback target.
        return Err(workspace_file_recovery_error());
    }
    if !installed_is_replacement {
        let recovery = windows_private_artifact_path(&parent, "recovery")
            .map_err(|_| workspace_file_recovery_error())?;
        if windows_replace_file(&target, &backup, &recovery).is_err() {
            return Err(workspace_file_recovery_error());
        }
        // The path-based rollback cannot prove whether `recovery` contains this
        // writer's replacement or a concurrent writer's later target. Preserve
        // it for explicit recovery instead of risking deletion of external bytes.
        return Err(workspace_file_recovery_error());
    }
    fs::remove_file(&backup).map_err(|_| workspace_file_recovery_error())
}

#[cfg(windows)]
fn windows_temporary_path(parent: &Path) -> PathBuf {
    let generation = WORKSPACE_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    parent.join(format!(
        ".ganbaru.{}.{}.{}.tmp",
        std::process::id(),
        generation,
        nonce
    ))
}

#[cfg(windows)]
fn windows_private_artifact_path(parent: &Path, kind: &str) -> std::io::Result<PathBuf> {
    use windows_crate::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };

    windows_private_artifact_path_with(parent, kind, |random| {
        // SAFETY: The binding receives an exclusive, initialized output slice
        // that remains live for the synchronous call. A null algorithm handle
        // is required with BCRYPT_USE_SYSTEM_PREFERRED_RNG, and no pointer is
        // retained after BCryptGenRandom returns.
        let status = unsafe { BCryptGenRandom(None, random, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
        if status.0 < 0 {
            Err(std::io::Error::other(format!(
                "BCryptGenRandom failed with NTSTATUS {:#x}",
                status.0
            )))
        } else {
            Ok(())
        }
    })
}

#[cfg(any(windows, test))]
fn windows_private_artifact_path_with(
    parent: &Path,
    kind: &str,
    mut fill_random: impl FnMut(&mut [u8]) -> std::io::Result<()>,
) -> std::io::Result<std::path::PathBuf> {
    const RANDOM_BYTES: usize = 32;
    const GENERATION_ATTEMPTS: usize = 4;

    for _ in 0..GENERATION_ATTEMPTS {
        let mut random = [0_u8; RANDOM_BYTES];
        fill_random(&mut random)?;
        let token = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let candidate = parent.join(format!(".ganbaru.{kind}.{token}"));
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a private workspace recovery name",
    ))
}

#[cfg(windows)]
fn windows_wide_path(path: &Path) -> std::io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;

    let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workspace replacement path contains a NUL code unit",
        ));
    }
    value.push(0);
    Ok(value)
}

#[cfg(windows)]
fn windows_move_file_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use windows_crate::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
    use windows_crate::core::PCWSTR;

    let source = windows_wide_path(source)?;
    let destination = windows_wide_path(destination)?;
    // SAFETY: Both paths are live, immutable, NUL-terminated UTF-16 buffers
    // without interior NUL. MoveFileExW consumes them synchronously and does
    // not retain their pointers. Zero flags prohibit destination replacement.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .map_err(|error| std::io::Error::other(error.to_string()))
}

#[cfg(windows)]
fn windows_replace_file(target: &Path, replacement: &Path, backup: &Path) -> std::io::Result<()> {
    use windows_crate::Win32::Storage::FileSystem::{REPLACE_FILE_FLAGS, ReplaceFileW};
    use windows_crate::core::PCWSTR;

    let target = windows_wide_path(target)?;
    let replacement = windows_wide_path(replacement)?;
    let backup = windows_wide_path(backup)?;
    // SAFETY: Each path is a live, immutable, NUL-terminated UTF-16 buffer with
    // no interior NUL. ReplaceFileW consumes the strings synchronously, does not
    // retain their pointers, and the null reserved arguments are required.
    unsafe {
        ReplaceFileW(
            PCWSTR(target.as_ptr()),
            PCWSTR(replacement.as_ptr()),
            PCWSTR(backup.as_ptr()),
            REPLACE_FILE_FLAGS(0),
            None,
            None,
        )
    }
    .map_err(|error| std::io::Error::other(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::windows_private_artifact_path_with;
    use std::path::PathBuf;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should follow the Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "ganbaru-ai-windows-artifact-{name}-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("test directory should be created");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn private_artifact_generation_skips_an_existing_candidate() {
        let directory = TestDirectory::new("collision");
        let collision = directory
            .0
            .join(format!(".ganbaru.backup.{}", "00".repeat(32)));
        std::fs::write(&collision, "external bytes").expect("collision fixture should be written");
        let mut calls = 0_u8;

        let selected = windows_private_artifact_path_with(&directory.0, "backup", |random| {
            random.fill(calls);
            calls = calls.saturating_add(1);
            Ok(())
        })
        .expect("a later private name should be selected");

        assert_eq!(calls, 2);
        assert_eq!(
            selected.file_name().and_then(|name| name.to_str()),
            Some(format!(".ganbaru.backup.{}", "01".repeat(32)).as_str())
        );
        assert!(!selected.exists());
        assert_eq!(
            std::fs::read_to_string(collision).expect("collision fixture should remain readable"),
            "external bytes"
        );
    }

    #[test]
    fn private_artifact_generation_propagates_random_source_failure() {
        let directory = TestDirectory::new("random-failure");

        let error = windows_private_artifact_path_with(&directory.0, "backup", |_| {
            Err(std::io::Error::other("random source fixture failed"))
        })
        .expect_err("random source failure should stop artifact generation");

        assert_eq!(error.to_string(), "random source fixture failed");
    }

    #[cfg(windows)]
    #[test]
    fn no_replace_move_preserves_an_existing_destination() {
        use super::windows_move_file_no_replace;

        let directory = TestDirectory::new("no-replace");
        let source = directory.0.join("source.txt");
        let destination = directory.0.join("destination.txt");
        std::fs::write(&source, "source bytes").expect("source fixture should be written");
        std::fs::write(&destination, "destination bytes")
            .expect("destination fixture should be written");

        assert!(windows_move_file_no_replace(&source, &destination).is_err());
        assert_eq!(
            std::fs::read_to_string(source).expect("source should remain"),
            "source bytes"
        );
        assert_eq!(
            std::fs::read_to_string(destination).expect("destination should remain"),
            "destination bytes"
        );
    }
}
