//! Atomic file replacement through no-follow temporary files, and bounded regular-file reads.
//!
//! A write creates a randomly named temporary file next to its target with `create_new`,
//! which refuses any existing entry, a symlink or dangling symlink included, so a link
//! planted in a shared folder cannot redirect the bytes. The temporary file is synced and
//! renamed over the target; the rename replaces a symlinked target instead of following it.
//! A staged file follows the same steps for content written in chunks over time.
//! Managed folders also refuse symlinked directories between their root and the target,
//! because `create_new` only protects the final path component.
//! Bounded reads open the file once and check its type and size through that handle.

use ring::rand::{SecureRandom, SystemRandom};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

/// Random bytes in each temporary file name.
const TEMPORARY_NONCE_BYTES: usize = 16;

/// Random names tried before a private directory creation gives up. A collision of
/// 128-bit names means something else is creating entries, so a few attempts suffice.
const PRIVATE_DIRECTORY_ATTEMPTS: u32 = 4;

/// Permissions given to a newly written file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileAccess {
    /// Readable and writable only by the current user on Unix. Used for app-owned state.
    OwnerOnly,
    /// The process default (umask on Unix). Used for files the user exports for other tools.
    Default,
}

/// Replaces `path` with `bytes` atomically.
///
/// # Errors
///
/// Returns an error when the path has no parent or file name, the temporary file cannot be
/// created exclusively, or writing, syncing, or renaming fails. The temporary file is removed
/// on every error path.
pub fn write_file_atomically(path: &Path, bytes: &[u8], access: FileAccess) -> io::Result<()> {
    write_file_atomically_with(path, access, |file| file.write_all(bytes))
}

/// Replaces `path` with the bytes `write` produces into a fresh temporary file.
///
/// The temporary file is only renamed over `path` when `write` succeeds.
///
/// # Errors
///
/// Returns the error from `write`, or an I/O error from creating, syncing, or renaming the
/// temporary file. The temporary file is removed on every error path.
pub fn write_file_atomically_with<T, E, F>(
    path: &Path,
    access: FileAccess,
    write: F,
) -> Result<T, E>
where
    F: FnOnce(&mut File) -> Result<T, E>,
    E: From<io::Error>,
{
    let parent = parent_directory(path)?;
    let temporary = temporary_path(parent, path)?;
    let mut file = open_temporary(&temporary, access)?;
    let outcome = (|| -> Result<T, E> {
        let value = write(&mut file)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(value)
    })();
    match outcome {
        Ok(value) => {
            sync_directory(parent);
            Ok(value)
        }
        Err(error) => {
            remove_temporary(&temporary);
            Err(error)
        }
    }
}

/// Replaces `path` atomically after creating its missing parent directories below `root`
/// without traversing a symlink.
///
/// # Errors
///
/// Returns an error when `path` is outside `root`, a directory between them is a symlink or
/// not a directory, or the write fails.
pub fn write_file_atomically_within(
    root: &Path,
    path: &Path,
    bytes: &[u8],
    access: FileAccess,
) -> io::Result<()> {
    create_directories_within(root, parent_directory(path)?)?;
    write_file_atomically(path, bytes, access)
}

/// A replacement of a file written in pieces, for content that arrives in chunks and is only
/// known to be valid once complete.
///
/// The bytes go to a randomly named temporary file created exclusively next to the target, as
/// in [`write_file_atomically`]. [`StagedFile::commit`] syncs it and renames it over the
/// target; dropping an uncommitted staged file removes the temporary file, so an abandoned or
/// refused transfer leaves nothing behind.
#[derive(Debug)]
pub struct StagedFile {
    file: Option<File>,
    temporary: PathBuf,
    target: PathBuf,
    written: u64,
}

impl StagedFile {
    /// Starts replacing `path`.
    ///
    /// # Errors
    ///
    /// Returns an error when the path has no parent or file name, or the temporary file cannot
    /// be created exclusively.
    pub fn create(path: &Path, access: FileAccess) -> io::Result<Self> {
        let parent = parent_directory(path)?;
        let temporary = temporary_path(parent, path)?;
        let file = open_temporary(&temporary, access)?;
        Ok(Self {
            file: Some(file),
            temporary,
            target: path.to_path_buf(),
            written: 0,
        })
    }

    /// The file the staged bytes replace.
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Bytes written so far.
    pub fn written(&self) -> u64 {
        self.written
    }

    /// Appends `bytes`.
    ///
    /// # Errors
    ///
    /// Returns the write error; the staged file should then be dropped.
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("staged file is already closed"))?;
        file.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    /// Syncs the staged bytes and renames them over the target.
    ///
    /// # Errors
    ///
    /// Returns an error when syncing or renaming fails. The temporary file is removed and the
    /// target is left as it was.
    pub fn commit(mut self) -> io::Result<()> {
        let file = self
            .file
            .take()
            .ok_or_else(|| io::Error::other("staged file is already closed"))?;
        file.sync_all()?;
        drop(file);
        fs::rename(&self.temporary, &self.target)?;
        // The temporary path is now the target, so dropping must not remove it.
        self.temporary = PathBuf::new();
        if let Ok(parent) = parent_directory(&self.target) {
            sync_directory(parent);
        }
        Ok(())
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.temporary.as_os_str().is_empty() {
            remove_temporary(&self.temporary);
        }
    }
}

/// Creates `directory` and its missing ancestors below `root`, refusing to traverse a
/// symlink. `root` itself may be reached through a symlink chosen by the user.
///
/// # Errors
///
/// Returns an error when `directory` is outside `root`, contains a non-normal component, or
/// has an ancestor below `root` that is a symlink or not a directory.
pub fn create_directories_within(root: &Path, directory: &Path) -> io::Result<()> {
    let mut current = root.to_path_buf();
    for name in components_within(root, directory)? {
        current.push(name);
        match fs::create_dir(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                require_real_directory(&current)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Checks that every directory from `root` down to `directory` is a real directory, so a
/// path below them cannot be redirected through a symlink. `root` itself may be a symlink.
///
/// # Errors
///
/// Returns an error when `directory` is outside `root`, contains a non-normal component, or
/// has an ancestor below `root` that is missing, a symlink, or not a directory.
pub fn require_directories_within(root: &Path, directory: &Path) -> io::Result<()> {
    let mut current = root.to_path_buf();
    for name in components_within(root, directory)? {
        current.push(name);
        require_real_directory(&current)?;
    }
    Ok(())
}

/// Removes the file at `path` below `root` without traversing a symlinked directory. A final
/// symlink is removed itself, never its target. A missing file is not an error.
///
/// # Errors
///
/// Returns an error when `path` is outside `root`, a directory between them is a symlink or
/// not a directory, or the removal fails for another reason.
pub fn remove_file_within(root: &Path, path: &Path) -> io::Result<()> {
    match require_directories_within(root, parent_directory(path)?) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        result => result?,
    }
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// Normal components of `directory` relative to `root`.
fn components_within<'a>(root: &Path, directory: &'a Path) -> io::Result<Vec<&'a OsStr>> {
    let relative = directory.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is outside {}", directory.display(), root.display()),
        )
    })?;
    let mut names = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => names.push(name),
            Component::CurDir => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{} is not a plain relative path", relative.display()),
                ));
            }
        }
    }
    Ok(names)
}

/// Creates a new directory in `parent` named `prefix` plus a random suffix.
///
/// Creation is exclusive and the name is unpredictable, so another local user cannot
/// pre-create, link, or guess it in a shared temporary directory. On Unix the directory is
/// accessible only by the current user (mode 0700). On Windows it inherits the access of
/// `parent`, which for the per-user temporary directory is already private.
///
/// # Errors
///
/// Returns an error when the secure random generator is unavailable, the directory cannot
/// be created, or every attempt collides with an existing entry.
pub fn create_private_temporary_directory(parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    for _ in 0..PRIVATE_DIRECTORY_ATTEMPTS {
        let path = parent.join(format!("{prefix}{}", random_hex()?));
        match private_dir_builder().create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "no free private directory name in {} after {PRIVATE_DIRECTORY_ATTEMPTS} attempts",
            parent.display()
        ),
    ))
}

/// Creates `directory` and any missing ancestors, each accessible only by the current user.
///
/// On Unix every directory this call creates gets mode 0700; existing directories keep their
/// permissions. On Windows new directories inherit the access of their parent, which under the
/// user profile is already private.
///
/// # Errors
///
/// Returns an error when a directory cannot be created or an ancestor is not a directory.
pub fn create_private_directories(directory: &Path) -> io::Result<()> {
    private_dir_builder().recursive(true).create(directory)
}

/// Creates the single directory `directory`, accessible only by the current user.
///
/// Unlike [`create_private_directories`] this refuses an existing entry, so the caller
/// knows the directory and its permissions are its own.
///
/// # Errors
///
/// Returns an error when `directory` already exists or cannot be created.
pub fn create_private_directory(directory: &Path) -> io::Result<()> {
    private_dir_builder().create(directory)
}

/// A directory builder whose new directories get mode 0700.
#[cfg(unix)]
fn private_dir_builder() -> fs::DirBuilder {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder
}

/// A directory builder whose new directories inherit the access of their parent.
#[cfg(not(unix))]
fn private_dir_builder() -> fs::DirBuilder {
    fs::DirBuilder::new()
}

/// Creates the new file `path`, readable and writable only by the current user on Unix.
///
/// # Errors
///
/// Returns an error when `path` already exists, a symlink included, or cannot be created.
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// Removes group and other permissions from an existing file or directory on Unix, leaving
/// the owner's bits as they are. Other platforms keep their access control lists.
///
/// # Errors
///
/// Returns an error when the metadata cannot be read or the file system refuses the change,
/// as FAT and exFAT mounts and files owned by another user do.
pub fn restrict_to_owner(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(mode & !0o077))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::metadata(path).map(|_| ())
    }
}

/// Reports whether `path` is owned by the effective user of this process.
///
/// On platforms without Unix ownership it always returns `true`; their per-user profile
/// access control lists already keep other accounts out.
///
/// # Errors
///
/// Returns an error when the metadata of `path` cannot be read.
pub fn is_owned_by_current_user(path: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let owner = fs::metadata(path)?.uid();
        Ok(owner == rustix::process::geteuid().as_raw())
    }
    #[cfg(not(unix))]
    {
        fs::metadata(path).map(|_| true)
    }
}

/// Fails when `path` is a symlink. A missing path passes.
///
/// # Errors
///
/// Returns an error when `path` is a symlink or its metadata cannot be read.
pub fn reject_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(symlink_error(path)),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// How [`read_regular_file_capped`] treats a symlink at the final path component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalSymlink {
    /// Read the link target. Used for paths the user chose in a native picker.
    Follow,
    /// Refuse the link. Used for paths named by imported content or found in a vault.
    Refuse,
}

/// Reads a regular file of at most `max_bytes` through one handle.
///
/// The type and size checks use the opened handle, and the read stops one byte past the
/// limit, so a file swapped or grown after the check cannot pass a larger body. On Unix the
/// open does not block on a FIFO or device, which then fails the regular-file check. With
/// [`FinalSymlink::Refuse`], Unix opens with `O_NOFOLLOW`; Windows refuses a symlink or
/// junction found just before a normal open, because creating one there needs elevated
/// rights or developer mode, and a reparse-point open would bypass cloud file hydration.
///
/// # Errors
///
/// Returns [`io::ErrorKind::FileTooLarge`] past `max_bytes`, [`io::ErrorKind::InvalidInput`]
/// for a refused symlink or a path that is not a regular file, and open or read errors.
pub fn read_regular_file_capped(
    path: &Path,
    max_bytes: u64,
    final_symlink: FinalSymlink,
) -> io::Result<Vec<u8>> {
    let file = open_for_bounded_read(path, final_symlink)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    let too_large = || {
        io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("{} exceeds {max_bytes} bytes", path.display()),
        )
    };
    if metadata.len() > max_bytes {
        return Err(too_large());
    }
    let capacity = usize::try_from(metadata.len()).unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(too_large());
    }
    Ok(bytes)
}

#[cfg(unix)]
fn open_for_bounded_read(path: &Path, final_symlink: FinalSymlink) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut flags = libc::O_NONBLOCK;
    if final_symlink == FinalSymlink::Refuse {
        flags |= libc::O_NOFOLLOW;
    }
    OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)
        .map_err(|error| {
            if final_symlink == FinalSymlink::Refuse && error.raw_os_error() == Some(libc::ELOOP) {
                symlink_error(path)
            } else {
                error
            }
        })
}

#[cfg(not(unix))]
fn open_for_bounded_read(path: &Path, final_symlink: FinalSymlink) -> io::Result<File> {
    if final_symlink == FinalSymlink::Refuse {
        reject_symlink(path)?;
    }
    File::open(path)
}

/// Reports whether `path` is a regular file reached without following a final symlink.
/// Content-addressed writers use it to skip a write only when a real file already exists.
pub fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn require_real_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(symlink_error(path));
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a directory", path.display()),
        ));
    }
    Ok(())
}

fn symlink_error(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{} is a symbolic link", path.display()),
    )
}

fn parent_directory(path: &Path) -> io::Result<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} has no parent directory", path.display()),
            )
        })
}

fn temporary_path(parent: &Path, path: &Path) -> io::Result<PathBuf> {
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no file name", path.display()),
        )
    })?;
    let mut name = OsString::from(".");
    name.push(file_name);
    name.push(format!(".{}.tmp", random_hex()?));
    Ok(parent.join(name))
}

/// Fills `bytes` from the operating system's secure random generator, for unpredictable names of
/// files and directories created beside user or workspace content.
pub fn fill_random(bytes: &mut [u8]) -> io::Result<()> {
    SystemRandom::new()
        .fill(bytes)
        .map_err(|_| io::Error::other("secure random generator is unavailable"))
}

fn random_hex() -> io::Result<String> {
    let mut nonce = [0_u8; TEMPORARY_NONCE_BYTES];
    fill_random(&mut nonce)?;
    Ok(nonce.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn open_temporary(path: &Path, access: FileAccess) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // `create_new` already refuses a final-component symlink; O_NOFOLLOW keeps that true
        // independently of how the platform maps exclusive creation.
        options.custom_flags(libc::O_NOFOLLOW);
        if access == FileAccess::OwnerOnly {
            options.mode(0o600);
        }
    }
    #[cfg(not(unix))]
    let _ = access;
    options.open(path)
}

fn remove_temporary(path: &Path) {
    // The original error is the useful one; a leftover temporary file has a random name and
    // never shadows the target.
    let _ = fs::remove_file(path);
}

#[cfg(unix)]
fn sync_directory(directory: &Path) {
    // The rename already committed the new contents. A directory sync failure only weakens
    // crash durability, and some filesystems reject it, so it does not fail the write.
    if let Ok(handle) = File::open(directory) {
        let _ = handle.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) {}

#[cfg(test)]
mod tests;
