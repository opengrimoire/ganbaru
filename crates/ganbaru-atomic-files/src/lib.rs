//! Atomic file replacement through no-follow temporary files.
//!
//! A write creates a randomly named temporary file next to its target with `create_new`,
//! which refuses any existing entry, a symlink or dangling symlink included, so a link
//! planted in a shared folder cannot redirect the bytes. The temporary file is synced and
//! renamed over the target; the rename replaces a symlinked target instead of following it.
//! Managed folders also refuse symlinked directories between their root and the target,
//! because `create_new` only protects the final path component.

use ring::rand::{SecureRandom, SystemRandom};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

/// Random bytes in each temporary file name.
const TEMPORARY_NONCE_BYTES: usize = 16;

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

/// Creates `directory` and its missing ancestors below `root`, refusing to traverse a
/// symlink. `root` itself may be reached through a symlink chosen by the user.
///
/// # Errors
///
/// Returns an error when `directory` is outside `root`, contains a non-normal component, or
/// has an ancestor below `root` that is a symlink or not a directory.
pub fn create_directories_within(root: &Path, directory: &Path) -> io::Result<()> {
    let relative = directory.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is outside {}", directory.display(), root.display()),
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(name) => current.push(name),
            Component::CurDir => continue,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{} is not a plain relative path", relative.display()),
                ));
            }
        }
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

fn random_hex() -> io::Result<String> {
    let mut nonce = [0_u8; TEMPORARY_NONCE_BYTES];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| io::Error::other("secure random generator is unavailable"))?;
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
