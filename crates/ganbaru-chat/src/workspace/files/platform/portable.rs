use super::*;

#[cfg(not(any(unix, windows)))]
pub(in crate::workspace::files) fn secure_workspace_file(
    root: &Path,
    relative_path: &str,
) -> ChatResult<SecureWorkspaceFile> {
    if workspace_path_contains_symlink(root, relative_path)? {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace symlinks are not available for Chat preview",
            false,
        ));
    }
    let path = root.join(relative_path);
    let canonical = path.canonicalize().map_err(file_error)?;
    if !canonical.starts_with(root) {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace file resolves outside the working folder",
            false,
        ));
    }
    File::open(canonical)
        .map(|file| SecureWorkspaceFile { file })
        .map_err(file_error)
}

#[cfg(not(any(unix, windows)))]
pub(in crate::workspace::files) fn secure_directory_entries(
    root: &Path,
    relative_path: &str,
) -> ChatResult<(Vec<SecureDirectoryEntry>, bool)> {
    if !relative_path.is_empty() && workspace_path_contains_symlink(root, relative_path)? {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace symlinks are not available for Chat browsing",
            false,
        ));
    }
    let path = root.join(relative_path);
    let canonical = path.canonicalize().map_err(file_error)?;
    if !canonical.starts_with(root) {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace directory resolves outside the working folder",
            false,
        ));
    }
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in fs::read_dir(canonical).map_err(file_error)? {
        let entry = entry.map_err(file_error)?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(file_error)?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            continue;
        }
        let Some(display_name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
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

#[cfg(not(any(unix, windows)))]
fn workspace_path_contains_symlink(root: &Path, relative_path: &str) -> ChatResult<bool> {
    let mut candidate = root.to_path_buf();
    for component in Path::new(relative_path).components() {
        let Component::Normal(name) = component else {
            return Err(ChatError::validation(
                "relativePath",
                "Workspace path must be a normalized relative path",
            ));
        };
        candidate.push(name);
        let metadata = fs::symlink_metadata(&candidate).map_err(file_error)?;
        if metadata.file_type().is_symlink() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(not(any(unix, windows)))]
fn workspace_parent_contains_symlink(root: &Path, relative_path: &str) -> ChatResult<bool> {
    let Some(parent) = Path::new(relative_path).parent() else {
        return Ok(false);
    };
    if parent.as_os_str().is_empty() {
        return Ok(false);
    }
    let parent = parent
        .to_str()
        .ok_or_else(|| ChatError::validation("relativePath", "Workspace path is unsupported"))?;
    workspace_path_contains_symlink(root, parent)
}

#[cfg(not(any(unix, windows)))]
pub(in crate::workspace::files) fn workspace_regular_file_permissions(
    root: &Path,
    relative_path: &str,
    field: &str,
) -> ChatResult<fs::Permissions> {
    if workspace_path_contains_symlink(root, relative_path)? {
        return Err(ChatError::validation(
            field,
            "Workspace path cannot be symbolic",
        ));
    }
    let requested = root.join(relative_path);
    let metadata = fs::symlink_metadata(&requested)
        .map_err(|_| ChatError::validation(field, "Workspace path is unavailable"))?;
    let canonical = fs::canonicalize(&requested)
        .map_err(|_| ChatError::validation(field, "Workspace path is unavailable"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || !canonical.starts_with(root) {
        return Err(ChatError::validation(
            field,
            "Workspace path is not a regular file",
        ));
    }
    Ok(metadata.permissions())
}

#[cfg(not(any(unix, windows)))]
pub(in crate::workspace::files) fn create_workspace_file_exclusively(
    root: &Path,
    relative_path: &str,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
    conflict_message: &str,
) -> ChatResult<()> {
    if workspace_parent_contains_symlink(root, relative_path)? {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace file parent cannot be symbolic",
            false,
        ));
    }
    let relative = Path::new(relative_path);
    let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent = if parent_relative.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        fs::canonicalize(root.join(parent_relative)).map_err(|_| workspace_file_write_error())?
    };
    if !parent.starts_with(root) || !parent.is_dir() {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace file path resolves outside the working folder",
            false,
        ));
    }
    let target = parent.join(
        relative
            .file_name()
            .ok_or_else(|| ChatError::validation("relativePath", "Workspace path is invalid"))?,
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ChatError::new(ChatErrorCode::Conflict, conflict_message, true)
            } else {
                workspace_file_write_error()
            }
        })?;
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        let _ = fs::remove_file(&target);
        return Err(workspace_file_write_error());
    }
    if let Some(permissions) = permissions {
        if fs::set_permissions(&target, permissions).is_err() {
            drop(file);
            let _ = fs::remove_file(&target);
            return Err(workspace_file_write_error());
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(in crate::workspace::files) fn delete_workspace_file_atomically(
    root: &Path,
    relative_path: &str,
    expected_revision: &str,
) -> ChatResult<()> {
    if workspace_path_contains_symlink(root, relative_path)? {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace file path cannot be symbolic",
            false,
        ));
    }
    let target = root.join(relative_path);
    let canonical = fs::canonicalize(&target).map_err(|_| workspace_file_write_error())?;
    if !canonical.starts_with(root) {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace file path resolves outside the working folder",
            false,
        ));
    }
    let current = fs::read(&canonical).map_err(|_| workspace_file_write_error())?;
    if workspace_file_revision(relative_path, &current) != expected_revision {
        return Err(stale_workspace_file_error());
    }
    let generation = WORKSPACE_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let backup = target.with_extension(format!(
        "ganbaru.{}.{}.backup",
        std::process::id(),
        generation
    ));
    fs::rename(&target, &backup).map_err(|_| workspace_file_write_error())?;
    let displaced_matches = fs::symlink_metadata(&backup)
        .ok()
        .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .and_then(|_| fs::read(&backup).ok())
        .is_some_and(|bytes| workspace_file_revision(relative_path, &bytes) == expected_revision);
    if !displaced_matches {
        if fs::symlink_metadata(&target).is_err() && fs::rename(&backup, &target).is_ok() {
            return Err(stale_workspace_file_error());
        }
        return Err(workspace_file_recovery_error());
    }
    fs::remove_file(&backup).map_err(|_| workspace_file_recovery_error())
}

#[cfg(not(any(unix, windows)))]
pub(in crate::workspace::files) fn write_workspace_text_atomically(
    root: &Path,
    relative_path: &str,
    contents: &str,
    expected_revision: &str,
) -> ChatResult<()> {
    if workspace_path_contains_symlink(root, relative_path)? {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace file path cannot be symbolic",
            false,
        ));
    }
    let requested = root.join(relative_path);
    let metadata = fs::symlink_metadata(&requested).map_err(|_| workspace_file_write_error())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ChatError::validation(
            "relativePath",
            "Workspace path is not an editable regular file",
        ));
    }
    let path = fs::canonicalize(&requested).map_err(|_| workspace_file_write_error())?;
    if !path.starts_with(root) {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace file path resolves outside the working folder",
            false,
        ));
    }
    let parent = path.parent().ok_or_else(workspace_file_write_error)?;
    let file_name = path
        .file_name()
        .ok_or_else(workspace_file_write_error)?
        .to_string_lossy();
    let generation = WORKSPACE_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{file_name}.ganbaru.{}.{}.tmp",
        std::process::id(),
        generation
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| workspace_file_write_error())?;
    let result = (|| {
        file.write_all(contents.as_bytes())
            .map_err(|_| workspace_file_write_error())?;
        file.sync_all().map_err(|_| workspace_file_write_error())?;
        let current = fs::read(&path).map_err(|_| workspace_file_write_error())?;
        if workspace_file_revision(relative_path, &current) != expected_revision {
            return Err(stale_workspace_file_error());
        }
        fs::set_permissions(&temporary, metadata.permissions())
            .map_err(|_| workspace_file_write_error())?;
        replace_workspace_file(&temporary, &path, relative_path, expected_revision)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(any(unix, windows)))]
fn replace_workspace_file(
    temporary: &Path,
    target: &Path,
    relative_path: &str,
    expected_revision: &str,
) -> ChatResult<()> {
    let generation = WORKSPACE_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let backup = target.with_extension(format!(
        "ganbaru.{}.{}.backup",
        std::process::id(),
        generation
    ));
    fs::rename(target, &backup).map_err(|_| workspace_file_write_error())?;
    if fs::rename(temporary, target).is_err() {
        return if fs::rename(&backup, target).is_ok() {
            Err(workspace_file_write_error())
        } else {
            Err(workspace_file_recovery_error())
        };
    }
    let displaced = fs::read(&backup).map_err(|_| workspace_file_write_error())?;
    if workspace_file_revision(relative_path, &displaced) != expected_revision {
        if fs::remove_file(target).is_ok() && fs::rename(&backup, target).is_ok() {
            return Err(stale_workspace_file_error());
        }
        return Err(workspace_file_recovery_error());
    }
    fs::remove_file(backup).map_err(|_| workspace_file_write_error())
}
