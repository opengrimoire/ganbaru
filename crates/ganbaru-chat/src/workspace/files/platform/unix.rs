use super::*;
use std::ptr::NonNull;

#[derive(Clone, Copy)]
enum ExistingEntryKind {
    Directory,
    Regular,
}

fn validate_relative_name(name: &CStr) -> std::io::Result<()> {
    let bytes = name.to_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    Ok(())
}

fn open_existing_at(
    directory: &File,
    name: &CStr,
    kind: ExistingEntryKind,
) -> std::io::Result<File> {
    validate_relative_name(name)?;
    let type_flags = match kind {
        ExistingEntryKind::Directory => libc::O_DIRECTORY,
        ExistingEntryKind::Regular => libc::O_NONBLOCK,
    };
    // SAFETY: `directory` keeps a valid directory descriptor open for the call,
    // `name` is NUL-terminated, and these fixed flags never request creation, so
    // the variadic mode argument is neither required nor read by `openat`.
    // O_NONBLOCK prevents a substituted FIFO from stalling a regular-file open.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_RDONLY | type_flags,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: A successful `openat` returns a new descriptor owned by this call.
    // No other Rust value owns it, so `File` may close it exactly once.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

// SAFETY: Each configured symbol is the supported target C runtime's documented
// errno-location ABI. It returns non-null, aligned, thread-local storage for one
// `c_int`; Ganbaru borrows it only for the synchronous write below.
#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
unsafe extern "C" {
    #[cfg_attr(target_os = "linux", link_name = "__errno_location")]
    #[cfg_attr(target_os = "android", link_name = "__errno")]
    #[cfg_attr(target_vendor = "apple", link_name = "__error")]
    fn ganbaru_errno_location() -> *mut libc::c_int;
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
fn clear_errno() {
    // SAFETY: The platform C runtime returns this thread's live errno storage.
    // Writing zero is required to distinguish readdir EOF from an error.
    unsafe {
        *ganbaru_errno_location() = 0;
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn clear_errno() {}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
fn readdir_end_or_error() -> std::io::Result<Option<CString>> {
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(0) {
        Ok(None)
    } else {
        Err(error)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn readdir_end_or_error() -> std::io::Result<Option<CString>> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

fn create_new_file_at(directory: &File, name: &CStr) -> std::io::Result<File> {
    validate_relative_name(name)?;
    // SAFETY: `directory` keeps a valid directory descriptor open for the call,
    // `name` is NUL-terminated, and the mode argument is present because O_CREAT
    // is set. O_EXCL and O_NOFOLLOW prevent replacement or symlink traversal.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_CLOEXEC | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_WRONLY,
            0o600,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: A successful `openat` returns a new descriptor owned by this call.
    // No other Rust value owns it, so `File` may close it exactly once.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
fn secure_workspace_directory(root: &Path, relative_path: &str) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW);
    let mut directory = options.open(root)?;
    for component in Path::new(relative_path).components() {
        let Component::Normal(name) = component else {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
        };
        let name = CString::new(name.as_bytes())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        directory = open_existing_at(&directory, &name, ExistingEntryKind::Directory)?;
    }
    Ok(directory)
}

#[cfg(unix)]
struct SecureDirectoryStream(NonNull<libc::DIR>);

#[cfg(unix)]
impl SecureDirectoryStream {
    fn open(directory: &File) -> std::io::Result<Self> {
        let duplicate = directory.try_clone()?;
        // SAFETY: `duplicate` owns a valid, readable directory descriptor. It
        // remains RAII-owned unless fdopendir succeeds and assumes ownership.
        let stream = unsafe { libc::fdopendir(duplicate.as_raw_fd()) };
        let Some(stream) = NonNull::new(stream) else {
            return Err(std::io::Error::last_os_error());
        };
        let transferred_descriptor = duplicate.into_raw_fd();
        debug_assert!(transferred_descriptor >= 0);
        Ok(Self(stream))
    }

    fn next_name(&mut self) -> std::io::Result<Option<CString>> {
        clear_errno();
        // SAFETY: `self.0` is a live stream owned exclusively through `&mut self`.
        // The returned entry remains valid until the next operation on this stream.
        let Some(entry) = NonNull::new(unsafe { libc::readdir(self.0.as_ptr()) }) else {
            return readdir_end_or_error();
        };
        // SAFETY: POSIX requires `d_name` in a successful readdir result to be a
        // NUL-terminated name. Copying it now prevents the borrowed pointer from
        // escaping or surviving the next readdir call.
        Ok(Some(
            unsafe { CStr::from_ptr((*entry.as_ptr()).d_name.as_ptr()) }.to_owned(),
        ))
    }
}

#[cfg(unix)]
impl Drop for SecureDirectoryStream {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from one successful fdopendir call and remains
        // exclusively owned by this wrapper. closedir also closes its descriptor.
        unsafe {
            libc::closedir(self.0.as_ptr());
        }
    }
}

#[cfg(unix)]
fn metadata_at(directory: &File, name: &CStr) -> std::io::Result<libc::stat> {
    validate_relative_name(name)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `directory` and `name` remain valid for the call, and `stat` points
    // to enough writable storage. AT_SYMLINK_NOFOLLOW inspects the entry itself.
    let status = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if status != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: fstatat returned success and therefore initialized the output.
    Ok(unsafe { stat.assume_init() })
}

#[cfg(unix)]
pub(in crate::workspace::files) fn secure_directory_entries(
    root: &Path,
    relative_path: &str,
) -> ChatResult<(Vec<SecureDirectoryEntry>, bool)> {
    let directory = secure_workspace_directory(root, relative_path).map_err(file_error)?;
    let mut stream = SecureDirectoryStream::open(&directory).map_err(file_error)?;
    let mut entries = Vec::new();
    let mut truncated = false;
    loop {
        let name = match stream.next_name() {
            Ok(Some(name)) => name,
            Ok(None) => break,
            Err(error) => return Err(file_error(error)),
        };
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        let Ok(display_name) = name.to_str() else {
            continue;
        };
        let stat = match metadata_at(&directory, &name) {
            Ok(stat) => stat,
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENOENT) | Some(libc::ESTALE)
                ) =>
            {
                continue;
            }
            Err(error) => return Err(file_error(error)),
        };
        let kind = stat.st_mode & libc::S_IFMT;
        let directory = kind == libc::S_IFDIR;
        let regular = kind == libc::S_IFREG;
        if !directory && !regular {
            continue;
        }
        entries.push(SecureDirectoryEntry {
            display_name: display_name.to_string(),
            directory,
            byte_size: regular.then(|| u64::try_from(stat.st_size).unwrap_or_default()),
        });
        if entries.len() >= MAX_DIRECTORY_ENTRIES {
            truncated = true;
            break;
        }
    }
    Ok((entries, truncated))
}

#[cfg(unix)]
pub(in crate::workspace::files) fn secure_workspace_file(
    root: &Path,
    relative_path: &str,
) -> ChatResult<SecureWorkspaceFile> {
    let parent = secure_workspace_parent(root, relative_path).map_err(file_error)?;
    open_regular_file_at(&parent)
        .map(|file| SecureWorkspaceFile { file })
        .map_err(file_error)
}

#[cfg(unix)]
pub(in crate::workspace::files) struct SecureWorkspaceParent {
    directory: File,
    file_name: CString,
}

#[cfg(unix)]
pub(in crate::workspace::files) fn secure_workspace_parent(
    root: &Path,
    relative_path: &str,
) -> std::io::Result<SecureWorkspaceParent> {
    use std::os::unix::fs::OpenOptionsExt;

    let relative = Path::new(relative_path);
    let mut root_options = OpenOptions::new();
    root_options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW);
    let mut directory = root_options.open(root)?;
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(name) = component else {
                return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
            };
            let name = CString::new(name.as_bytes())
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
            directory = open_existing_at(&directory, &name, ExistingEntryKind::Directory)?;
        }
    }
    let file_name = relative
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    Ok(SecureWorkspaceParent {
        directory,
        file_name: CString::new(file_name.as_bytes())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?,
    })
}

#[cfg(unix)]
pub(in crate::workspace::files) fn open_regular_file_at(
    parent: &SecureWorkspaceParent,
) -> std::io::Result<File> {
    let file = open_existing_at(
        &parent.directory,
        &parent.file_name,
        ExistingEntryKind::Regular,
    )?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    Ok(file)
}

#[cfg(unix)]
enum CreateFileAtError {
    Create(std::io::Error),
    Prepare,
}

#[cfg(unix)]
fn create_file_at(
    parent: &SecureWorkspaceParent,
    file_name: &CString,
    mode: u32,
) -> Result<File, CreateFileAtError> {
    let file =
        create_new_file_at(&parent.directory, file_name).map_err(CreateFileAtError::Create)?;
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(|_| CreateFileAtError::Prepare)?;
    Ok(file)
}

#[cfg(unix)]
fn unlink_at(parent: &SecureWorkspaceParent, file_name: &CString) -> std::io::Result<()> {
    validate_relative_name(file_name)?;
    // SAFETY: The parent descriptor is live and `file_name` is NUL-terminated.
    // The validated name is relative, and flags zero request removal of a file.
    if unsafe { libc::unlinkat(parent.directory.as_raw_fd(), file_name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn rename_noreplace_at(
    parent: &SecureWorkspaceParent,
    source: &CString,
    destination: &CString,
) -> std::io::Result<()> {
    validate_relative_name(source)?;
    validate_relative_name(destination)?;
    #[cfg(target_os = "linux")]
    let flags = libc::RENAME_NOREPLACE;
    #[cfg(target_os = "android")]
    let flags: libc::c_uint = libc::RENAME_NOREPLACE.try_into().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "RENAME_NOREPLACE is not representable on this target",
        )
    })?;
    renameat2_at(parent, source, destination, flags)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn rename_noreplace_at(
    parent: &SecureWorkspaceParent,
    source: &CString,
    destination: &CString,
) -> std::io::Result<()> {
    validate_relative_name(source)?;
    validate_relative_name(destination)?;
    // SAFETY: Both names are NUL-terminated relative names and the parent
    // descriptor remains open. RENAME_EXCL forbids replacement atomically.
    if unsafe {
        libc::renameatx_np(
            parent.directory.as_raw_fd(),
            source.as_ptr(),
            parent.directory.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))
))]
fn rename_noreplace_at(
    _parent: &SecureWorkspaceParent,
    _source: &CString,
    _destination: &CString,
) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn exchange_at(
    parent: &SecureWorkspaceParent,
    left: &CString,
    right: &CString,
) -> std::io::Result<()> {
    validate_relative_name(left)?;
    validate_relative_name(right)?;
    #[cfg(target_os = "linux")]
    let flags = libc::RENAME_EXCHANGE;
    #[cfg(target_os = "android")]
    let flags: libc::c_uint = libc::RENAME_EXCHANGE.try_into().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "RENAME_EXCHANGE is not representable on this target",
        )
    })?;
    renameat2_at(parent, left, right, flags)
}

#[cfg(target_os = "linux")]
fn renameat2_at(
    parent: &SecureWorkspaceParent,
    source: &CString,
    destination: &CString,
    flags: libc::c_uint,
) -> std::io::Result<()> {
    // SAFETY: Both names were validated as NUL-terminated relative components
    // and the parent descriptor remains open. The caller supplies one documented
    // renameat2 flag, which makes the requested operation atomic.
    let result = unsafe {
        libc::renameat2(
            parent.directory.as_raw_fd(),
            source.as_ptr(),
            parent.directory.as_raw_fd(),
            destination.as_ptr(),
            flags,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "android")]
fn renameat2_at(
    parent: &SecureWorkspaceParent,
    source: &CString,
    destination: &CString,
    flags: libc::c_uint,
) -> std::io::Result<()> {
    // SAFETY: Both names were validated as NUL-terminated relative components
    // and the parent descriptor remains open. The caller supplies one documented
    // renameat2 flag. Calling the kernel directly avoids a bionic symbol that is
    // newer than the application's minimum Android API level.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent.directory.as_raw_fd(),
            source.as_ptr(),
            parent.directory.as_raw_fd(),
            destination.as_ptr(),
            flags,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn exchange_at(
    parent: &SecureWorkspaceParent,
    left: &CString,
    right: &CString,
) -> std::io::Result<()> {
    validate_relative_name(left)?;
    validate_relative_name(right)?;
    // SAFETY: Both names are NUL-terminated relative names and the parent
    // descriptor remains open. RENAME_SWAP swaps existing entries atomically.
    if unsafe {
        libc::renameatx_np(
            parent.directory.as_raw_fd(),
            left.as_ptr(),
            parent.directory.as_raw_fd(),
            right.as_ptr(),
            libc::RENAME_SWAP,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))
))]
fn exchange_at(
    _parent: &SecureWorkspaceParent,
    _left: &CString,
    _right: &CString,
) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

#[cfg(unix)]
pub(in crate::workspace::files) fn workspace_regular_file_permissions(
    root: &Path,
    relative_path: &str,
    field: &str,
) -> ChatResult<fs::Permissions> {
    let parent = secure_workspace_parent(root, relative_path).map_err(|_| {
        ChatError::validation(field, "Workspace file parent is unavailable or symbolic")
    })?;
    open_regular_file_at(&parent)
        .and_then(|file| file.metadata())
        .map(|metadata| metadata.permissions())
        .map_err(|_| ChatError::validation(field, "Workspace path is not a regular file"))
}

#[cfg(unix)]
pub(in crate::workspace::files) fn create_workspace_file_exclusively(
    root: &Path,
    relative_path: &str,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
    conflict_message: &str,
) -> ChatResult<()> {
    let parent =
        secure_workspace_parent(root, relative_path).map_err(|_| workspace_file_write_error())?;
    let mode = permissions.map_or(0o600, |value| value.mode() & 0o777);
    let mut file =
        create_file_at(&parent, &parent.file_name, mode).map_err(|error| match error {
            CreateFileAtError::Create(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                ChatError::new(ChatErrorCode::Conflict, conflict_message, true)
            }
            CreateFileAtError::Create(_) => workspace_file_write_error(),
            CreateFileAtError::Prepare => workspace_file_recovery_error(),
        })?;
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        return Err(workspace_file_recovery_error());
    }
    Ok(())
}

#[cfg(unix)]
pub(in crate::workspace::files) fn delete_workspace_file_atomically(
    root: &Path,
    relative_path: &str,
    expected_revision: &str,
) -> ChatResult<()> {
    let parent =
        secure_workspace_parent(root, relative_path).map_err(|_| workspace_file_write_error())?;
    let mut current = open_regular_file_at(&parent).map_err(|_| workspace_file_write_error())?;
    let (current_revision, _) = revision_and_permissions(&mut current, relative_path)?;
    if current_revision != expected_revision {
        return Err(stale_workspace_file_error());
    }
    let metadata = current
        .metadata()
        .map_err(|_| workspace_file_write_error())?;
    let identity = (metadata.dev(), metadata.ino());
    let generation = WORKSPACE_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let backup_name = CString::new(format!(
        ".ganbaru.{}.{}.{}.backup",
        std::process::id(),
        generation,
        nonce,
    ))
    .map_err(|_| workspace_file_write_error())?;
    rename_noreplace_at(&parent, &parent.file_name, &backup_name)
        .map_err(|_| workspace_file_write_error())?;
    let verification = (|| {
        let mut displaced =
            open_existing_at(&parent.directory, &backup_name, ExistingEntryKind::Regular)
                .map_err(|_| workspace_file_write_error())?;
        let displaced_metadata = displaced
            .metadata()
            .map_err(|_| workspace_file_write_error())?;
        if !displaced_metadata.is_file()
            || (displaced_metadata.dev(), displaced_metadata.ino()) != identity
        {
            return Err(stale_workspace_file_error());
        }
        let (revision, _) = revision_and_permissions(&mut displaced, relative_path)?;
        if revision != expected_revision {
            return Err(stale_workspace_file_error());
        }
        Ok(())
    })();
    if verification.is_err() {
        // The backup name is no longer proven to identify the displaced file.
        // Preserve it for explicit recovery instead of installing unknown bytes.
        return Err(workspace_file_recovery_error());
    }
    unlink_at(&parent, &backup_name).map_err(|_| workspace_file_recovery_error())
}

#[cfg(unix)]
pub(in crate::workspace::files) fn write_workspace_text_atomically(
    root: &Path,
    relative_path: &str,
    contents: &str,
    expected_revision: &str,
) -> ChatResult<()> {
    let parent =
        secure_workspace_parent(root, relative_path).map_err(|_| workspace_file_write_error())?;
    let mut current = open_regular_file_at(&parent).map_err(|_| workspace_file_write_error())?;
    let (current_revision, _) = revision_and_permissions(&mut current, relative_path)?;
    if current_revision != expected_revision {
        return Err(stale_workspace_file_error());
    }
    let current_metadata = current
        .metadata()
        .map_err(|_| workspace_file_write_error())?;
    let current_identity = (current_metadata.dev(), current_metadata.ino());

    let generation = WORKSPACE_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary_name = CString::new(format!(
        ".ganbaru.{}.{}.{}.tmp",
        std::process::id(),
        generation,
        nonce,
    ))
    .map_err(|_| workspace_file_write_error())?;
    let mut replacement =
        create_file_at(&parent, &temporary_name, 0o600).map_err(|error| match error {
            CreateFileAtError::Create(_) => workspace_file_write_error(),
            CreateFileAtError::Prepare => workspace_file_recovery_error(),
        })?;
    let write_result = replacement
        .write_all(contents.as_bytes())
        .and_then(|_| replacement.sync_all());
    if write_result.is_err() {
        drop(replacement);
        return Err(workspace_file_recovery_error());
    }
    let replacement_metadata = match replacement.metadata() {
        Ok(metadata) => metadata,
        Err(_) => {
            drop(replacement);
            return Err(workspace_file_recovery_error());
        }
    };
    let replacement_identity = (replacement_metadata.dev(), replacement_metadata.ino());
    let replacement_revision = workspace_file_revision(relative_path, contents.as_bytes());

    if exchange_at(&parent, &temporary_name, &parent.file_name).is_err() {
        drop(replacement);
        return Err(workspace_file_recovery_error());
    }
    let installed_is_replacement = (|| -> ChatResult<bool> {
        let mut installed = open_existing_at(
            &parent.directory,
            &parent.file_name,
            ExistingEntryKind::Regular,
        )
        .map_err(|_| workspace_file_write_error())?;
        let installed_metadata = installed
            .metadata()
            .map_err(|_| workspace_file_write_error())?;
        if (installed_metadata.dev(), installed_metadata.ino()) != replacement_identity {
            return Ok(false);
        }
        let (installed_revision, _) = revision_and_permissions(&mut installed, relative_path)?;
        Ok(installed_revision == replacement_revision)
    })()
    .unwrap_or(false);
    let displaced_permissions = (|| -> ChatResult<fs::Permissions> {
        let mut displaced = open_existing_at(
            &parent.directory,
            &temporary_name,
            ExistingEntryKind::Regular,
        )
        .map_err(|_| workspace_file_write_error())?;
        let displaced_metadata = displaced
            .metadata()
            .map_err(|_| workspace_file_write_error())?;
        if (displaced_metadata.dev(), displaced_metadata.ino()) != current_identity {
            return Err(stale_workspace_file_error());
        }
        let (displaced_revision, displaced_permissions) =
            revision_and_permissions(&mut displaced, relative_path)?;
        if displaced_revision != expected_revision {
            return Err(stale_workspace_file_error());
        }
        Ok(displaced_permissions)
    })();
    let Ok(displaced_permissions) = displaced_permissions else {
        // An unverified temporary name must never be installed as a rollback.
        return Err(workspace_file_recovery_error());
    };
    if !installed_is_replacement {
        let _ = exchange_at(&parent, &temporary_name, &parent.file_name);
        return Err(workspace_file_recovery_error());
    }
    let finalized = replacement
        .set_permissions(fs::Permissions::from_mode(
            displaced_permissions.mode() & 0o777,
        ))
        .and_then(|_| replacement.sync_all())
        .and_then(|_| unlink_at(&parent, &temporary_name));
    if finalized.is_err() {
        // The displaced file was verified before rollback. After any exchange,
        // preserve the new temporary artifact because its name can be substituted.
        let _ = exchange_at(&parent, &temporary_name, &parent.file_name);
        return Err(workspace_file_recovery_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_relative_name;
    use std::ffi::CString;

    #[test]
    fn relative_native_names_reject_path_syntax() {
        for value in [b"".as_slice(), b".", b"..", b"/absolute", b"nested/name"] {
            let name = CString::new(value).expect("fixture name must not contain NUL");
            assert!(validate_relative_name(&name).is_err(), "accepted {value:?}");
        }

        let ordinary = CString::new("ordinary.txt").expect("fixture name must be valid");
        assert!(validate_relative_name(&ordinary).is_ok());
    }
}
