use super::*;

#[cfg(not(any(unix, windows)))]
mod portable;
#[cfg(unix)]
mod unix;
#[cfg(any(windows, test))]
mod windows;

#[cfg(not(any(unix, windows)))]
pub(super) use portable::{
    create_workspace_file_exclusively, delete_workspace_file_atomically, secure_directory_entries,
    secure_workspace_file, workspace_regular_file_permissions, write_workspace_text_atomically,
};
#[cfg(unix)]
pub(super) use unix::{
    create_workspace_file_exclusively, delete_workspace_file_atomically, secure_directory_entries,
    secure_workspace_file, workspace_regular_file_permissions, write_workspace_text_atomically,
};
#[cfg(all(test, unix))]
pub(super) use unix::{open_regular_file_at, secure_workspace_parent};
#[cfg(windows)]
pub(super) use windows::{
    create_workspace_file_exclusively, delete_workspace_file_atomically, secure_directory_entries,
    secure_workspace_file, workspace_regular_file_permissions, write_workspace_text_atomically,
};

pub(super) struct SecureDirectoryEntry {
    pub(super) display_name: String,
    pub(super) directory: bool,
    pub(super) byte_size: Option<u64>,
}

pub(super) struct SecureWorkspaceFile {
    pub(super) file: File,
    #[cfg(windows)]
    pub(super) _parent_handles: Vec<File>,
}

impl SecureWorkspaceFile {
    pub(super) fn metadata(&self) -> std::io::Result<fs::Metadata> {
        self.file.metadata()
    }
}

impl Read for SecureWorkspaceFile {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buffer)
    }
}

pub(super) fn revision_and_permissions(
    file: &mut File,
    relative_path: &str,
) -> ChatResult<(String, fs::Permissions)> {
    let before = file.metadata().map_err(|_| workspace_file_write_error())?;
    if !before.is_file() || before.len() > MAX_PREVIEW_BYTES {
        return Err(ChatError::validation(
            "relativePath",
            "Workspace path is not bounded editable text",
        ));
    }
    let modified = before.modified().ok();
    file.seek(SeekFrom::Start(0))
        .map_err(|_| workspace_file_write_error())?;
    let mut bytes = Vec::with_capacity(usize::try_from(before.len()).unwrap_or_default());
    (&mut *file)
        .take(MAX_PREVIEW_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| workspace_file_write_error())?;
    let after = file.metadata().map_err(|_| workspace_file_write_error())?;
    if before.len() != after.len()
        || bytes.len() as u64 != after.len()
        || modified.is_some() && after.modified().ok() != modified
    {
        return Err(stale_workspace_file_error());
    }
    Ok((
        workspace_file_revision(relative_path, &bytes),
        after.permissions(),
    ))
}
