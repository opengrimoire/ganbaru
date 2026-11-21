//! File storage shared by the journals of multi-step vault operations: a bounded read that
//! refuses links, an atomic owner-only write, and a removal that reaches stable storage.

use ganbaru_atomic_files::{FileAccess, FinalSymlink};
use std::fs;
use std::io;
use std::path::Path;

/// Largest journal read; a real journal is a few hundred bytes.
const MAX_JOURNAL_BYTES: u64 = 4 * 1024;

/// Reads the journal at `path`, `None` when there is none. A link, an oversized file, or any
/// other read failure is an error, so a journal is never mistaken for an absent one.
pub(crate) fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match ganbaru_atomic_files::read_regular_file_capped(
        path,
        MAX_JOURNAL_BYTES,
        FinalSymlink::Refuse,
    ) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Replaces the journal at `path` with `bytes` in one atomic, owner-only write.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    ganbaru_atomic_files::write_file_atomically(path, bytes, FileAccess::OwnerOnly)
}

/// Removes the journal at `path`, if any, and flushes its folder so the removal outlasts a crash.
pub(crate) fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "journal has no folder"))?;
    ganbaru_atomic_files::sync_directory_entries(folder)
}

/// Whether a journal, readable or not, is at `path`.
pub(crate) fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
