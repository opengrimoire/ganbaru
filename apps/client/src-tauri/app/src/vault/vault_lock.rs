//! Cross-process vault fence: an advisory OS lock on `vault.lock` in the vault root.
//!
//! The process takes the lock before its first vault pool connects and keeps one handle for as
//! long as that vault is active, because two handles of one process conflict with each other.
//! Another Ganbaru process, such as a development build pointed at the same folder, is refused
//! instead of corrupting engine state. A file system without locks opens the vault anyway and
//! reports [`VaultLockStatus::Unsupported`] so Data settings can warn about it.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// File in the vault root that carries the cross-process lock. It never travels in bundles or
/// backups, because the archive layout is an allowlist that leaves it out.
pub(crate) const VAULT_LOCK_FILE: &str = "vault.lock";

/// Stable text of [`VaultLockError::OpenElsewhere`], which the frontend recognizes to show its
/// localized explanation.
pub(crate) const VAULT_OPEN_ELSEWHERE_MESSAGE: &str = "this vault is open in another Ganbaru app";

/// How firmly this process holds its active vault.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum VaultLockStatus {
    /// No vault has been opened by this process yet.
    NotHeld,
    /// The OS lock is held, so no other process can open the vault.
    Exclusive,
    /// The file system refused the lock, so another process cannot be detected.
    Unsupported,
}

/// Why a vault could not be fenced for this process.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum VaultLockError {
    /// Another process holds the lock on this vault.
    OpenElsewhere,
    /// The lock file could not be opened or created.
    Io(String),
}

impl std::fmt::Display for VaultLockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenElsewhere => formatter.write_str(VAULT_OPEN_ELSEWHERE_MESSAGE),
            Self::Io(message) => formatter.write_str(message),
        }
    }
}

impl From<VaultLockError> for String {
    fn from(error: VaultLockError) -> Self {
        error.to_string()
    }
}

/// The lock handle of one vault root. Dropping it releases the OS lock.
#[derive(Debug)]
struct HeldVaultLock {
    root: PathBuf,
    _file: File,
    exclusive: bool,
}

impl HeldVaultLock {
    fn status(&self) -> VaultLockStatus {
        if self.exclusive {
            VaultLockStatus::Exclusive
        } else {
            VaultLockStatus::Unsupported
        }
    }
}

/// The lock a process holds on its active vault, at most one at a time.
#[derive(Debug, Default)]
pub(crate) struct VaultLockRegistry {
    held: Mutex<Option<HeldVaultLock>>,
}

static PROCESS_VAULT_LOCK: VaultLockRegistry = VaultLockRegistry::new();

/// The registry of this process, shared by the app and a headless sync pass.
pub(crate) fn process_vault_lock() -> &'static VaultLockRegistry {
    &PROCESS_VAULT_LOCK
}

impl VaultLockRegistry {
    pub(crate) const fn new() -> Self {
        Self {
            held: Mutex::new(None),
        }
    }

    fn held(&self) -> MutexGuard<'_, Option<HeldVaultLock>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Holds the lock of the vault at `root`, keeping the current handle when it already names
    /// that vault. Taking another vault's lock releases the previous one only after the new one
    /// is held, so a refused switch leaves the active vault fenced.
    ///
    /// # Errors
    ///
    /// Returns [`VaultLockError::OpenElsewhere`] when another process holds the lock, and
    /// [`VaultLockError::Io`] when the root or the lock file cannot be opened.
    pub(crate) fn acquire(&self, root: &Path) -> Result<VaultLockStatus, VaultLockError> {
        let root = canonical_root(root)?;
        let mut held = self.held();
        if let Some(current) = held.as_ref().filter(|current| current.root == root) {
            return Ok(current.status());
        }
        let lock = lock_root(&root)?;
        let status = lock.status();
        let previous = held.replace(lock);
        drop(held);
        drop(previous);
        Ok(status)
    }

    /// Releases the held lock, if any.
    pub(crate) fn release(&self) {
        let previous = self.held().take();
        drop(previous);
    }

    /// Reports how this process holds its vault.
    pub(crate) fn status(&self) -> VaultLockStatus {
        self.held()
            .as_ref()
            .map_or(VaultLockStatus::NotHeld, HeldVaultLock::status)
    }

    /// Runs `swap`, which moves the folder `staging` to `target`, while keeping the vault fenced.
    ///
    /// On Unix the lock moves with its directory, so `staging/vault.lock` is locked first, the
    /// directories are swapped, and the old handle is released last: no other process can open
    /// the new vault in between. Windows cannot rename a directory that holds an open file, so
    /// there both handles are closed for the swap and the new vault is locked right after; a
    /// process that opens either folder in that moment keeps a file open in it, which makes the
    /// rename fail and roll back.
    ///
    /// # Errors
    ///
    /// Returns the lock or swap error. A failed swap keeps the previous lock.
    pub(crate) fn replace_with(
        &self,
        staging: &Path,
        target: &Path,
        swap: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        #[cfg(not(windows))]
        {
            let incoming = lock_root(&canonical_root(staging)?)?;
            let mut held = self.held();
            swap()?;
            let root = canonical_root(target)?;
            let previous = held.replace(HeldVaultLock { root, ..incoming });
            drop(held);
            drop(previous);
            Ok(())
        }
        #[cfg(windows)]
        {
            let _ = staging;
            let mut held = self.held();
            let previous_root = held.take().map(|previous| previous.root);
            let swapped = swap();
            let root = match &swapped {
                Ok(()) => Some(canonical_root(target)?),
                Err(_) => previous_root,
            };
            if let Some(root) = root {
                *held = Some(lock_root(&root)?);
            }
            swapped
        }
    }
}

fn canonical_root(root: &Path) -> Result<PathBuf, VaultLockError> {
    std::fs::canonicalize(root).map_err(|error| {
        VaultLockError::Io(format!(
            "find Ganbaru folder '{}' for its lock: {error}",
            root.display()
        ))
    })
}

/// Opens `root/vault.lock`, creating it owner-only when missing, and tries the exclusive lock.
fn lock_root(root: &Path) -> Result<HeldVaultLock, VaultLockError> {
    let file = open_lock_file(&root.join(VAULT_LOCK_FILE))
        .map_err(|error| VaultLockError::Io(format!("open the vault lock file: {error}")))?;
    let exclusive = match file.try_lock() {
        Ok(()) => true,
        Err(TryLockError::WouldBlock) => return Err(VaultLockError::OpenElsewhere),
        Err(TryLockError::Error(error)) => {
            eprintln!("Vault lock is unsupported in '{}': {error}", root.display());
            false
        }
    };
    Ok(HeldVaultLock {
        root: root.to_path_buf(),
        _file: file,
        exclusive,
    })
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    ganbaru_atomic_files::reject_symlink(path)?;
    match ganbaru_atomic_files::create_private_file(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            OpenOptions::new().read(true).write(true).open(path)
        }
        Err(error) => Err(error),
    }
}

/// Reports how this process holds its active vault, for the Data settings warning.
#[tauri::command]
pub fn vault_lock_status() -> VaultLockStatus {
    process_vault_lock().status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    /// Environment variable that turns [`child_holds_a_vault_lock`] into the lock holder.
    const CHILD_ROOT_ENV: &str = "GANBARU_VAULT_LOCK_CHILD_ROOT";
    const CHILD_READY: &str = "vault lock held";

    fn unique_root(name: &str) -> PathBuf {
        let root = crate::vault::tests::unique_path(&format!("lock-{name}"));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// Whether another handle of this process can lock `root`, which a held lock refuses.
    fn probe(root: &Path) -> Result<(), TryLockError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join(VAULT_LOCK_FILE))
            .unwrap();
        file.try_lock()
    }

    /// Holds the lock of the vault named by [`CHILD_ROOT_ENV`] until stdin closes. Without the
    /// variable it returns at once, so the normal test run passes it.
    #[test]
    fn child_holds_a_vault_lock() {
        let Some(root) = std::env::var_os(CHILD_ROOT_ENV) else {
            return;
        };
        let registry = VaultLockRegistry::new();
        assert_eq!(
            registry.acquire(Path::new(&root)),
            Ok(VaultLockStatus::Exclusive)
        );
        println!("{CHILD_READY}");
        io::stdout().flush().unwrap();
        let mut line = String::new();
        let _ = io::stdin().read_line(&mut line);
    }

    #[test]
    fn vault_lock_refuses_a_vault_another_process_holds() {
        let root = unique_root("other-process");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "vault::vault_lock::tests::child_holds_a_vault_lock",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_ROOT_ENV, &root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap())
            .lines()
            .map_while(Result::ok);
        let ready = lines.any(|line| line.contains(CHILD_READY));
        assert!(ready, "the child process never held the lock");

        let registry = VaultLockRegistry::new();
        assert_eq!(registry.acquire(&root), Err(VaultLockError::OpenElsewhere));
        assert_eq!(registry.status(), VaultLockStatus::NotHeld);
        assert_eq!(
            VaultLockError::OpenElsewhere.to_string(),
            VAULT_OPEN_ELSEWHERE_MESSAGE
        );

        drop(child.stdin.take());
        // Drain the child's remaining output so its test summary never meets a closed pipe.
        lines.for_each(drop);
        assert!(child.wait().unwrap().success());
        assert_eq!(registry.acquire(&root), Ok(VaultLockStatus::Exclusive));
        registry.release();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn vault_lock_is_kept_for_the_same_vault_and_released_on_switch() {
        let first = unique_root("first");
        let second = unique_root("second");
        let registry = VaultLockRegistry::new();

        assert_eq!(registry.acquire(&first), Ok(VaultLockStatus::Exclusive));
        // A second acquire of the same vault reuses the handle instead of conflicting with it.
        assert_eq!(
            registry.acquire(&first.join(".")),
            Ok(VaultLockStatus::Exclusive)
        );
        assert!(matches!(probe(&first), Err(TryLockError::WouldBlock)));

        assert_eq!(registry.acquire(&second), Ok(VaultLockStatus::Exclusive));
        assert!(probe(&first).is_ok());
        assert!(matches!(probe(&second), Err(TryLockError::WouldBlock)));

        registry.release();
        assert_eq!(registry.status(), VaultLockStatus::NotHeld);
        assert!(probe(&second).is_ok());
        let _ = std::fs::remove_dir_all(first);
        let _ = std::fs::remove_dir_all(second);
    }

    #[test]
    fn vault_lock_refused_switch_keeps_the_active_vault_fenced() {
        let active = unique_root("active");
        let busy = unique_root("busy");
        let other = VaultLockRegistry::new();
        other.acquire(&busy).unwrap();
        let registry = VaultLockRegistry::new();
        registry.acquire(&active).unwrap();

        assert_eq!(registry.acquire(&busy), Err(VaultLockError::OpenElsewhere));
        assert_eq!(registry.status(), VaultLockStatus::Exclusive);
        assert!(matches!(probe(&active), Err(TryLockError::WouldBlock)));

        other.release();
        registry.release();
        let _ = std::fs::remove_dir_all(active);
        let _ = std::fs::remove_dir_all(busy);
    }

    #[test]
    fn vault_lock_file_is_owner_only_and_never_a_symlink() {
        let root = unique_root("file");
        let registry = VaultLockRegistry::new();
        registry.acquire(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.join(VAULT_LOCK_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
        registry.release();

        #[cfg(unix)]
        {
            let linked = unique_root("linked");
            let elsewhere = linked.join("elsewhere");
            std::fs::write(&elsewhere, b"").unwrap();
            std::os::unix::fs::symlink(&elsewhere, linked.join(VAULT_LOCK_FILE)).unwrap();
            assert!(matches!(
                registry.acquire(&linked),
                Err(VaultLockError::Io(_))
            ));
            let _ = std::fs::remove_dir_all(linked);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn vault_lock_stays_held_across_a_replacement_swap() {
        let parent = unique_root("swap");
        let target = parent.join("Ganbaru");
        let staging = parent.join("staging");
        let rollback = parent.join("rollback");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("marker"), b"replacement").unwrap();
        let registry = VaultLockRegistry::new();
        registry.acquire(&target).unwrap();

        registry
            .replace_with(&staging, &target, || {
                // Mid-swap, both the vault being replaced and the incoming one stay fenced.
                #[cfg(not(windows))]
                {
                    assert!(matches!(probe(&target), Err(TryLockError::WouldBlock)));
                    assert!(matches!(probe(&staging), Err(TryLockError::WouldBlock)));
                }
                std::fs::rename(&target, &rollback).map_err(|error| error.to_string())?;
                std::fs::rename(&staging, &target).map_err(|error| error.to_string())
            })
            .unwrap();

        assert_eq!(
            std::fs::read(target.join("marker")).unwrap(),
            b"replacement"
        );
        assert_eq!(registry.status(), VaultLockStatus::Exclusive);
        assert!(matches!(probe(&target), Err(TryLockError::WouldBlock)));
        // The replaced folder's handle is released once the new vault is fenced.
        assert!(probe(&rollback).is_ok());
        // The registry names the new vault, so a later connect reuses the moved handle.
        assert_eq!(registry.acquire(&target), Ok(VaultLockStatus::Exclusive));

        registry.release();
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn vault_lock_failed_swap_keeps_the_previous_vault_fenced() {
        let parent = unique_root("failed-swap");
        let target = parent.join("Ganbaru");
        let staging = parent.join("staging");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        let registry = VaultLockRegistry::new();
        registry.acquire(&target).unwrap();

        let result = registry.replace_with(&staging, &target, || Err("swap failed".to_string()));

        assert_eq!(result, Err("swap failed".to_string()));
        assert!(matches!(probe(&target), Err(TryLockError::WouldBlock)));
        assert!(probe(&staging).is_ok());
        assert_eq!(registry.acquire(&target), Ok(VaultLockStatus::Exclusive));
        registry.release();
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn vault_lock_replacement_is_refused_while_another_process_holds_the_staging() {
        let parent = unique_root("busy-staging");
        let target = parent.join("Ganbaru");
        let staging = parent.join("staging");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        let other = VaultLockRegistry::new();
        other.acquire(&staging).unwrap();
        let registry = VaultLockRegistry::new();
        registry.acquire(&target).unwrap();

        let mut swapped = false;
        let result = registry.replace_with(&staging, &target, || {
            swapped = true;
            Ok(())
        });

        #[cfg(not(windows))]
        {
            assert_eq!(result, Err(VAULT_OPEN_ELSEWHERE_MESSAGE.to_string()));
            assert!(!swapped);
        }
        #[cfg(windows)]
        let _ = (result, swapped);
        other.release();
        registry.release();
        let _ = std::fs::remove_dir_all(parent);
    }
}
