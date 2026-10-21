//! Private empty working directories for provider probes.

use crate::models::{ChatError, ChatErrorCode, ChatResult};
use std::ops::Deref;
use std::path::{Path, PathBuf};

const PROBE_DIRECTORY_PREFIX: &str = "ganbaru-provider-probe-";

/// Empty directory that only the current user can access, used as the working
/// directory of provider probes and thread lifecycle calls. Provider CLIs load
/// project configuration from their working directory, so probes never run
/// where the app was launched. The directory is removed when dropped.
#[derive(Debug)]
pub struct ProbeDirectory {
    path: PathBuf,
}

impl ProbeDirectory {
    /// Creates a fresh directory under the system temporary directory.
    pub fn create() -> ChatResult<Self> {
        Self::create_in(&std::env::temp_dir())
    }

    fn create_in(parent: &Path) -> ChatResult<Self> {
        let created = ganbaru_atomic_files::create_private_temporary_directory(
            parent,
            PROBE_DIRECTORY_PREFIX,
        )
        .map_err(|_| probe_directory_error())?;
        match std::fs::canonicalize(&created) {
            Ok(path) => Ok(Self { path }),
            Err(_) => {
                let _ = std::fs::remove_dir_all(&created);
                Err(probe_directory_error())
            }
        }
    }
}

impl Deref for ProbeDirectory {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for ProbeDirectory {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn probe_directory_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ConfigurationInvalid,
        "Provider probe directory is unavailable",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::ProbeDirectory;

    #[test]
    fn probe_directories_are_fresh_private_empty_and_removed_after_use() {
        let first = ProbeDirectory::create().expect("first probe directory");
        let second = ProbeDirectory::create().expect("second probe directory");
        assert_ne!(&*first, &*second);
        assert!(first.is_absolute());
        assert_eq!(std::fs::canonicalize(&*first).unwrap(), first.to_path_buf());
        assert_ne!(std::fs::canonicalize(".").unwrap(), first.to_path_buf());
        assert_eq!(std::fs::read_dir(&*first).unwrap().count(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&*first).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let path = first.to_path_buf();
        std::fs::write(path.join("provider-state"), b"left behind").unwrap();
        drop(first);
        assert!(!path.exists());
    }
}
