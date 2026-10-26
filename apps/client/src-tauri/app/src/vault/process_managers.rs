//! The pairing and ownership managers one process shares between the app runtime and headless
//! sync passes.

use super::handoff::pairing::PairingManager;
use super::ownership::VaultOwnershipManager;
use ganbaru_handoff::key_wrap::KeyWrapper;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// One pairing manager and one ownership manager for a process. Each manager saves its whole
/// in-memory state, so the Tauri runtime and the headless sync host hold clones of these same
/// instances: a write through one is visible to the other and never overwritten by a stale copy.
#[derive(Clone)]
pub(crate) struct ProcessManagers {
    pairing: PairingManager,
    ownership: VaultOwnershipManager,
    /// The platform key that seals this device's private key files.
    key_wrapper: Arc<dyn KeyWrapper>,
    /// Serializes loads, so two callers never both read the state files.
    loading: Arc<Mutex<()>>,
}

impl Default for ProcessManagers {
    fn default() -> Self {
        Self::with_key_wrapper(crate::sync::key_protection::platform_wrapper())
    }
}

impl ProcessManagers {
    /// Managers whose private key files `key_wrapper` seals.
    pub(crate) fn with_key_wrapper(key_wrapper: Arc<dyn KeyWrapper>) -> Self {
        Self {
            pairing: PairingManager::default(),
            ownership: VaultOwnershipManager::default(),
            key_wrapper,
            loading: Arc::default(),
        }
    }

    pub(crate) fn pairing(&self) -> &PairingManager {
        &self.pairing
    }

    pub(crate) fn ownership(&self) -> &VaultOwnershipManager {
        &self.ownership
    }

    /// The platform key that seals this device's private key files.
    pub(crate) fn key_wrapper(&self) -> Arc<dyn KeyWrapper> {
        self.key_wrapper.clone()
    }

    /// Loads pairing state from the app config directory `config_dir` unless this process
    /// already loaded it or turned linking off. A loaded state is kept: every write goes through
    /// it, so the file holds nothing newer. The linking key is kept in a sealed file, and key
    /// files found unsealed unlink the device; see
    /// [`crate::sync::key_protection::remove_unprotected_keys`].
    pub(crate) fn load_pairing(&self, config_dir: &Path, device_id: String) -> Result<(), String> {
        let _loading = self.lock_loading()?;
        if self.pairing.is_loaded()? {
            return Ok(());
        }
        self.pairing.set_identity_key_store(Arc::new(
            crate::sync::key_protection::identity_key_store(config_dir, self.key_wrapper()),
        ))?;
        self.pairing
            .initialize(config_dir.to_path_buf(), device_id)?;
        crate::sync::key_protection::remove_unprotected_keys(
            config_dir,
            &self.pairing,
            self.key_wrapper.as_ref(),
        );
        Ok(())
    }

    /// Loads ownership state from the app config directory `config_dir` unless this process
    /// already loaded it.
    pub(crate) fn load_ownership(&self, config_dir: &Path) -> Result<(), String> {
        let _loading = self.lock_loading()?;
        if self.ownership.is_loaded()? {
            return Ok(());
        }
        self.ownership.initialize_in(config_dir)
    }

    fn lock_loading(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        self.loading
            .lock()
            .map_err(|_| "process manager load lock is unavailable".to_string())
    }
}

/// The managers of this process, shared by the mobile runtime and headless sync passes.
#[cfg_attr(not(mobile), allow(dead_code))]
pub(crate) fn process_managers() -> &'static ProcessManagers {
    static MANAGERS: std::sync::OnceLock<ProcessManagers> = std::sync::OnceLock::new();
    MANAGERS.get_or_init(ProcessManagers::default)
}
