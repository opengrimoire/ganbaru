//! Durable device-local ownership state for single-writer vault handoff.

use super::{active_vault_id, ensure_device_id};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, Runtime};

const OWNERSHIP_STATE_FILE: &str = "vault-ownership.json";
const OWNERSHIP_STATE_SCHEMA_VERSION: u32 = 1;
pub(crate) const READ_ONLY_ERROR: &str = "This vault is read-only on this device";
const MANAGED_WRITE_DRAIN_BUDGET: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub(crate) enum TransferPhase {
    Stable,
    PreparingOutgoing {
        transfer_id: String,
        receiver_device_id: String,
        next_generation: u64,
    },
    OutgoingCommitted {
        transfer_id: String,
        receiver_device_id: String,
        committed_generation: u64,
    },
    IncomingCommitted {
        transfer_id: String,
        source_device_id: String,
        committed_generation: u64,
    },
    RecoveryRequired {
        transfer_id: String,
        reason: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnershipRecord {
    vault_id: String,
    owner_device_id: String,
    generation: u64,
    transfer_phase: TransferPhase,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnershipStateFile {
    schema_version: u32,
    vaults: BTreeMap<String, OwnershipRecord>,
}

impl Default for OwnershipStateFile {
    fn default() -> Self {
        Self {
            schema_version: OWNERSHIP_STATE_SCHEMA_VERSION,
            vaults: BTreeMap::new(),
        }
    }
}

#[derive(Default)]
struct OwnershipManagerInner {
    storage_path: Option<PathBuf>,
    device_id: Option<String>,
    state: OwnershipStateFile,
    // A replaced file cannot safely be rolled back only in memory.
    persistence_uncertain: Option<String>,
    #[cfg(test)]
    fail_next_directory_sync: bool,
}

/// Reports whether this device is linked to other devices of a vault.
pub(crate) type LinkProbe = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Process-local facade over the platform-private ownership state file.
#[derive(Default)]
pub(crate) struct VaultOwnershipManager {
    inner: Mutex<OwnershipManagerInner>,
    write_fence: Arc<ManagedWriteFenceControl>,
    link_probe: Mutex<Option<LinkProbe>>,
}

#[derive(Default)]
struct ManagedWriteFenceControl {
    state: Mutex<ManagedWriteFenceState>,
    drained: Condvar,
}

#[derive(Default)]
struct ManagedWriteFenceState {
    fenced: bool,
    active_writers: usize,
}

pub(crate) struct ManagedVaultWritePermit {
    control: Arc<ManagedWriteFenceControl>,
}

impl Drop for ManagedVaultWritePermit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.control.state.lock() {
            state.active_writers = state.active_writers.saturating_sub(1);
            if state.active_writers == 0 {
                self.control.drained.notify_all();
            }
        }
    }
}

pub(crate) struct ManagedVaultWriteFence {
    control: Arc<ManagedWriteFenceControl>,
}

impl Drop for ManagedVaultWriteFence {
    fn drop(&mut self) {
        if let Ok(mut state) = self.control.state.lock() {
            state.fenced = false;
            self.control.drained.notify_all();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VaultDatabaseAccess {
    ReadOnly,
    ReadWrite,
    /// Writable for replicated tables only, with read-only guards on every other table.
    Guarded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultOwnershipStatus {
    pub vault_id: String,
    pub device_id: String,
    pub owner_device_id: String,
    pub generation: u64,
    pub role: &'static str,
    pub can_write: bool,
    /// Replicated tables accept local writes while the rest of the vault is read-only.
    pub replicated_writes: bool,
    pub transfer_phase: TransferPhase,
}

impl VaultOwnershipManager {
    pub(crate) fn initialize<R: Runtime>(&self, app: &tauri::AppHandle<R>) -> Result<(), String> {
        let storage_path = app
            .path()
            .app_config_dir()
            .map_err(|error| format!("find app config directory: {error}"))?
            .join(OWNERSHIP_STATE_FILE);
        let device_id = ensure_device_id(app)?;
        self.initialize_from_path(storage_path, device_id)
    }

    pub(crate) fn initialize_from_path(
        &self,
        storage_path: PathBuf,
        device_id: String,
    ) -> Result<(), String> {
        let mut state = read_state_file(&storage_path)?;
        validate_state(&state, &device_id)?;
        let mut recovered = false;
        for record in state.vaults.values_mut() {
            if matches!(
                record.transfer_phase,
                TransferPhase::PreparingOutgoing { .. }
            ) {
                record.transfer_phase = TransferPhase::Stable;
                recovered = true;
            }
        }
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "vault ownership lock is unavailable".to_string())?;
        inner.storage_path = Some(storage_path);
        inner.device_id = Some(device_id);
        inner.state = state;
        inner.persistence_uncertain = None;
        if recovered {
            persist_inner(&mut inner)?;
        }
        Ok(())
    }

    /// Installs the probe that decides whether a stable replica is linked to its vault.
    pub(crate) fn set_link_probe(&self, probe: Option<LinkProbe>) -> Result<(), String> {
        *self
            .link_probe
            .lock()
            .map_err(|_| "vault link probe lock is unavailable".to_string())? = probe;
        Ok(())
    }

    /// Whether this device shares the vault with a linked peer or a pinned coordinator. Call it
    /// outside the ownership lock, because the probe takes the pairing lock.
    pub(crate) fn is_linked(&self, vault_id: &str) -> bool {
        self.link_probe
            .lock()
            .ok()
            .and_then(|probe| probe.clone())
            .is_some_and(|probe| probe(vault_id))
    }

    /// Marks a stable, linked replica as able to write replicated tables. The probe runs
    /// after the ownership lock is released so it never nests inside it.
    fn with_replicated_writes(&self, mut status: VaultOwnershipStatus) -> VaultOwnershipStatus {
        let replica = status.role == "read-only"
            && !status.can_write
            && status.transfer_phase == TransferPhase::Stable;
        status.replicated_writes = replica && self.is_linked(&status.vault_id);
        status
    }

    pub(crate) fn status(&self, vault_id: &str) -> Result<VaultOwnershipStatus, String> {
        let status = self.with_record(vault_id, |device_id, record| {
            Ok(status_from_record(device_id, record))
        })?;
        Ok(self.with_replicated_writes(status))
    }

    /// Read known committed authority without materializing a record or accessing storage.
    pub(crate) fn cached_status(
        &self,
        vault_id: &str,
    ) -> Result<Option<VaultOwnershipStatus>, String> {
        let inner = self.inner.try_lock().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => "vault ownership is busy".to_owned(),
            std::sync::TryLockError::Poisoned(_) => {
                "vault ownership lock is unavailable".to_owned()
            }
        })?;
        let Some(record) = inner.state.vaults.get(vault_id) else {
            return Ok(None);
        };
        let device_id = initialized_device_id(&inner)?;
        let status = status_from_record(device_id, record);
        drop(inner);
        Ok(Some(self.with_replicated_writes(status)))
    }

    pub(crate) fn database_access(&self, vault_id: &str) -> Result<VaultDatabaseAccess, String> {
        let status = self.status(vault_id)?;
        Ok(if status.can_write {
            VaultDatabaseAccess::ReadWrite
        } else if status.replicated_writes {
            VaultDatabaseAccess::Guarded
        } else {
            VaultDatabaseAccess::ReadOnly
        })
    }

    pub(crate) fn require_writable(&self, vault_id: &str) -> Result<(), String> {
        if self.status(vault_id)?.can_write {
            Ok(())
        } else {
            Err(READ_ONLY_ERROR.to_string())
        }
    }

    pub(crate) fn ensure_can_recover_local_copy(&self, vault_id: &str) -> Result<(), String> {
        self.with_record(vault_id, |device_id, record| {
            if record.owner_device_id == device_id {
                return Err("this device already owns the vault".to_string());
            }
            if !matches!(
                record.transfer_phase,
                TransferPhase::Stable | TransferPhase::RecoveryRequired { .. }
            ) {
                return Err("finish or retry the committed handoff before recovery".to_string());
            }
            Ok(())
        })
    }

    pub(crate) fn recover_local_copy(&self, vault_id: &str) -> Result<u64, String> {
        self.mutate_record(vault_id, |device_id, record| {
            if record.owner_device_id == device_id {
                return Err("this device already owns the vault".to_string());
            }
            if !matches!(
                record.transfer_phase,
                TransferPhase::Stable | TransferPhase::RecoveryRequired { .. }
            ) {
                return Err("finish or retry the committed handoff before recovery".to_string());
            }
            let generation = record
                .generation
                .checked_add(1)
                .ok_or_else(|| "vault ownership generation is exhausted".to_string())?;
            record.owner_device_id = device_id.to_string();
            record.generation = generation;
            record.transfer_phase = TransferPhase::Stable;
            Ok(generation)
        })
    }

    pub(crate) fn register_remote_owner(
        &self,
        vault_id: &str,
        owner_device_id: String,
        generation: u64,
    ) -> Result<(), String> {
        require_identifier(vault_id, "vault id")?;
        require_identifier(&owner_device_id, "owner device id")?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "vault ownership lock is unavailable".to_string())?;
        let device_id = initialized_device_id(&inner)?.to_string();
        if owner_device_id == device_id {
            return Err("linked vault owner must be a different device".to_string());
        }
        match inner.state.vaults.get(vault_id) {
            Some(record)
                if record.owner_device_id == owner_device_id
                    && record.generation == generation
                    && record.transfer_phase == TransferPhase::Stable =>
            {
                return Ok(());
            }
            Some(_) => return Err("linked vault conflicts with local ownership state".to_string()),
            None => {}
        }
        inner.state.vaults.insert(
            vault_id.to_string(),
            OwnershipRecord {
                vault_id: vault_id.to_string(),
                owner_device_id,
                generation,
                transfer_phase: TransferPhase::Stable,
            },
        );
        if let Err(error) = persist_inner(&mut inner) {
            if inner.persistence_uncertain.is_none() {
                inner.state.vaults.remove(vault_id);
            }
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn register_remote_owner_if_missing(
        &self,
        vault_id: &str,
        owner_device_id: String,
        generation: u64,
    ) -> Result<(), String> {
        require_identifier(vault_id, "vault id")?;
        require_identifier(&owner_device_id, "owner device id")?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "vault ownership lock is unavailable".to_string())?;
        initialized_device_id(&inner)?;
        if inner.state.vaults.contains_key(vault_id) {
            return Ok(());
        }
        let device_id = initialized_device_id(&inner)?.to_string();
        if owner_device_id == device_id {
            return Err("linked vault owner must be a different device".to_string());
        }
        inner.state.vaults.insert(
            vault_id.to_string(),
            OwnershipRecord {
                vault_id: vault_id.to_string(),
                owner_device_id,
                generation,
                transfer_phase: TransferPhase::Stable,
            },
        );
        if let Err(error) = persist_inner(&mut inner) {
            if inner.persistence_uncertain.is_none() {
                inner.state.vaults.remove(vault_id);
            }
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn acquire_managed_write(
        &self,
        vault_id: &str,
    ) -> Result<ManagedVaultWritePermit, String> {
        self.require_writable(vault_id)?;
        let permit = {
            let mut state = self
                .write_fence
                .state
                .lock()
                .map_err(|_| "managed vault write fence is unavailable".to_string())?;
            if state.fenced {
                return Err(READ_ONLY_ERROR.to_string());
            }
            state.active_writers = state
                .active_writers
                .checked_add(1)
                .ok_or_else(|| "managed vault writer count is exhausted".to_string())?;
            ManagedVaultWritePermit {
                control: Arc::clone(&self.write_fence),
            }
        };
        if let Err(error) = self.require_writable(vault_id) {
            drop(permit);
            return Err(error);
        }
        Ok(permit)
    }

    pub(crate) fn fence_managed_writes(&self) -> Result<ManagedVaultWriteFence, String> {
        self.fence_managed_writes_with_budget(MANAGED_WRITE_DRAIN_BUDGET)
    }

    fn fence_managed_writes_with_budget(
        &self,
        budget: Duration,
    ) -> Result<ManagedVaultWriteFence, String> {
        let started = Instant::now();
        let mut state = self
            .write_fence
            .state
            .lock()
            .map_err(|_| "managed vault write fence is unavailable".to_string())?;
        if state.fenced {
            return Err("managed vault writes are already fenced".to_string());
        }
        state.fenced = true;
        while state.active_writers != 0 {
            let Some(remaining) = budget.checked_sub(started.elapsed()) else {
                state.fenced = false;
                self.write_fence.drained.notify_all();
                return Err(
                    "Managed vault writes have not drained; vault operation cancelled".to_string(),
                );
            };
            let (next_state, _) = self
                .write_fence
                .drained
                .wait_timeout(state, remaining)
                .map_err(|_| "managed vault write fence is unavailable".to_string())?;
            state = next_state;
        }
        Ok(ManagedVaultWriteFence {
            control: Arc::clone(&self.write_fence),
        })
    }

    pub(crate) fn begin_outgoing(
        &self,
        vault_id: &str,
        expected_generation: u64,
        transfer_id: String,
        receiver_device_id: String,
    ) -> Result<(), String> {
        require_identifier(&transfer_id, "transfer id")?;
        require_identifier(&receiver_device_id, "receiver device id")?;
        self.mutate_record(vault_id, |device_id, record| {
            require_stable_owner(device_id, record, expected_generation)?;
            if receiver_device_id == device_id {
                return Err("receiver must be a different device".to_string());
            }
            let next_generation = record
                .generation
                .checked_add(1)
                .ok_or_else(|| "vault ownership generation is exhausted".to_string())?;
            record.transfer_phase = TransferPhase::PreparingOutgoing {
                transfer_id,
                receiver_device_id,
                next_generation,
            };
            Ok(())
        })
    }

    pub(crate) fn abort_outgoing(&self, vault_id: &str, transfer_id: &str) -> Result<(), String> {
        self.mutate_record(vault_id, |device_id, record| {
            if record.owner_device_id != device_id {
                return Err("only the current owner can abort a transfer".to_string());
            }
            match &record.transfer_phase {
                TransferPhase::PreparingOutgoing {
                    transfer_id: active,
                    ..
                } if active == transfer_id => {
                    record.transfer_phase = TransferPhase::Stable;
                    Ok(())
                }
                _ => Err("outgoing transfer is not in its pre-commit phase".to_string()),
            }
        })
    }

    pub(crate) fn commit_outgoing(&self, vault_id: &str, transfer_id: &str) -> Result<u64, String> {
        self.mutate_record(vault_id, |device_id, record| {
            if record.owner_device_id != device_id {
                return Err("only the current owner can commit a transfer".to_string());
            }
            let (receiver_device_id, committed_generation) = match &record.transfer_phase {
                TransferPhase::PreparingOutgoing {
                    transfer_id: active,
                    receiver_device_id,
                    next_generation,
                } if active == transfer_id => (receiver_device_id.clone(), *next_generation),
                TransferPhase::OutgoingCommitted {
                    transfer_id: active,
                    receiver_device_id,
                    committed_generation,
                } if active == transfer_id => {
                    return Ok(*committed_generation);
                }
                _ => return Err("outgoing transfer is not ready to commit".to_string()),
            };
            record.owner_device_id = receiver_device_id.clone();
            record.generation = committed_generation;
            record.transfer_phase = TransferPhase::OutgoingCommitted {
                transfer_id: transfer_id.to_string(),
                receiver_device_id,
                committed_generation,
            };
            Ok(committed_generation)
        })
    }

    #[cfg(any(test, desktop))]
    pub(crate) fn accept_incoming_grant(
        &self,
        vault_id: &str,
        transfer_id: String,
        source_device_id: String,
        generation: u64,
    ) -> Result<(), String> {
        require_identifier(&transfer_id, "transfer id")?;
        require_identifier(&source_device_id, "source device id")?;
        self.mutate_record(vault_id, |device_id, record| {
            if generation == record.generation {
                return match &record.transfer_phase {
                    TransferPhase::IncomingCommitted {
                        transfer_id: active,
                        source_device_id: source,
                        committed_generation,
                    } if active == &transfer_id
                        && source == &source_device_id
                        && *committed_generation == generation
                        && record.owner_device_id == device_id =>
                    {
                        Ok(())
                    }
                    _ => Err("ownership grant generation is stale".to_string()),
                };
            }
            if generation < record.generation {
                return Err("ownership grant generation is stale".to_string());
            }
            if record.owner_device_id != source_device_id {
                return Err("ownership grant source is not the current owner".to_string());
            }
            if generation != record.generation.saturating_add(1) {
                return Err("ownership grant generation is not the next generation".to_string());
            }
            record.owner_device_id = device_id.to_string();
            record.generation = generation;
            record.transfer_phase = TransferPhase::IncomingCommitted {
                transfer_id,
                source_device_id,
                committed_generation: generation,
            };
            Ok(())
        })
    }

    pub(crate) fn accept_incoming_coordinator_grant(
        &self,
        vault_id: &str,
        transfer_id: String,
        coordinator_device_id: String,
        generation: u64,
    ) -> Result<(), String> {
        require_identifier(&transfer_id, "transfer id")?;
        require_identifier(&coordinator_device_id, "coordinator device id")?;
        self.mutate_record(vault_id, |device_id, record| {
            if generation == record.generation {
                return match &record.transfer_phase {
                    TransferPhase::IncomingCommitted {
                        transfer_id: active,
                        source_device_id,
                        committed_generation,
                    } if active == &transfer_id
                        && source_device_id == &coordinator_device_id
                        && *committed_generation == generation
                        && record.owner_device_id == device_id =>
                    {
                        Ok(())
                    }
                    _ => Err("ownership grant generation is stale".to_string()),
                };
            }
            if generation < record.generation {
                return Err("ownership grant generation is stale".to_string());
            }
            if record.owner_device_id != coordinator_device_id {
                return Err("ownership grant source is not the linked coordinator".to_string());
            }
            record.owner_device_id = device_id.to_string();
            record.generation = generation;
            record.transfer_phase = TransferPhase::IncomingCommitted {
                transfer_id,
                source_device_id: coordinator_device_id,
                committed_generation: generation,
            };
            Ok(())
        })
    }

    pub(crate) fn advance_remote_coordinator(
        &self,
        vault_id: &str,
        coordinator_device_id: &str,
        generation: u64,
    ) -> Result<(), String> {
        require_identifier(coordinator_device_id, "coordinator device id")?;
        self.mutate_record(vault_id, |device_id, record| {
            if record.owner_device_id != coordinator_device_id
                || coordinator_device_id == device_id
                || !matches!(record.transfer_phase, TransferPhase::Stable)
            {
                return Err("remote coordinator state conflicts with local ownership".to_string());
            }
            if generation < record.generation {
                return Err("remote coordinator generation is stale".to_string());
            }
            record.generation = generation;
            Ok(())
        })
    }

    pub(crate) fn finalize_incoming(
        &self,
        vault_id: &str,
        transfer_id: &str,
        generation: u64,
    ) -> Result<(), String> {
        self.mutate_record(vault_id, |device_id, record| {
            if record.owner_device_id != device_id || record.generation != generation {
                return Err("incoming ownership grant does not match local state".to_string());
            }
            match &record.transfer_phase {
                TransferPhase::IncomingCommitted {
                    transfer_id: active,
                    committed_generation,
                    ..
                } if active == transfer_id && *committed_generation == generation => {
                    record.transfer_phase = TransferPhase::Stable;
                    Ok(())
                }
                _ => Err("incoming transfer is not ready to activate".to_string()),
            }
        })
    }

    pub(crate) fn finish_outgoing_acknowledgement(
        &self,
        vault_id: &str,
        transfer_id: &str,
        generation: u64,
    ) -> Result<(), String> {
        self.mutate_record(vault_id, |_device_id, record| {
            match &record.transfer_phase {
                TransferPhase::OutgoingCommitted {
                    transfer_id: active,
                    committed_generation,
                    ..
                } if active == transfer_id && *committed_generation == generation => {
                    record.transfer_phase = TransferPhase::Stable;
                    Ok(())
                }
                _ => Err("outgoing acknowledgement does not match local state".to_string()),
            }
        })
    }

    fn with_record<T>(
        &self,
        vault_id: &str,
        read: impl FnOnce(&str, &OwnershipRecord) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "vault ownership lock is unavailable".to_string())?;
        ensure_record(&mut inner, vault_id)?;
        let device_id = initialized_device_id(&inner)?.to_string();
        let record = inner
            .state
            .vaults
            .get(vault_id)
            .ok_or_else(|| "vault ownership state is unavailable".to_string())?;
        read(&device_id, record)
    }

    fn mutate_record<T>(
        &self,
        vault_id: &str,
        mutate: impl FnOnce(&str, &mut OwnershipRecord) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "vault ownership lock is unavailable".to_string())?;
        ensure_record(&mut inner, vault_id)?;
        let device_id = initialized_device_id(&inner)?.to_string();
        let record = inner
            .state
            .vaults
            .get_mut(vault_id)
            .ok_or_else(|| "vault ownership state is unavailable".to_string())?;
        let previous = record.clone();
        let result = match mutate(&device_id, record) {
            Ok(result) => result,
            Err(error) => {
                *record = previous.clone();
                return Err(error);
            }
        };
        if let Err(error) = persist_inner(&mut inner) {
            if inner.persistence_uncertain.is_none() {
                inner.state.vaults.insert(vault_id.to_string(), previous);
            }
            return Err(error);
        }
        Ok(result)
    }
}

fn ensure_record(inner: &mut OwnershipManagerInner, vault_id: &str) -> Result<(), String> {
    require_identifier(vault_id, "vault id")?;
    let device_id = initialized_device_id(inner)?.to_string();
    if !inner.state.vaults.contains_key(vault_id) {
        inner.state.vaults.insert(
            vault_id.to_string(),
            OwnershipRecord {
                vault_id: vault_id.to_string(),
                owner_device_id: device_id,
                generation: 0,
                transfer_phase: TransferPhase::Stable,
            },
        );
        if let Err(error) = persist_inner(inner) {
            if inner.persistence_uncertain.is_none() {
                inner.state.vaults.remove(vault_id);
            }
            return Err(error);
        }
    }
    Ok(())
}

fn initialized_device_id(inner: &OwnershipManagerInner) -> Result<&str, String> {
    if let Some(error) = &inner.persistence_uncertain {
        return Err(error.clone());
    }
    inner
        .device_id
        .as_deref()
        .ok_or_else(|| "vault ownership manager is not initialized".to_string())
}

fn require_stable_owner(
    device_id: &str,
    record: &OwnershipRecord,
    expected_generation: u64,
) -> Result<(), String> {
    if record.owner_device_id != device_id
        || record.generation != expected_generation
        || record.transfer_phase != TransferPhase::Stable
    {
        return Err("local device is not the stable owner at the expected generation".to_string());
    }
    Ok(())
}

fn status_from_record(device_id: &str, record: &OwnershipRecord) -> VaultOwnershipStatus {
    let stable = record.transfer_phase == TransferPhase::Stable;
    let local_owner = record.owner_device_id == device_id;
    let recovery = matches!(
        record.transfer_phase,
        TransferPhase::RecoveryRequired { .. }
    );
    VaultOwnershipStatus {
        vault_id: record.vault_id.clone(),
        device_id: device_id.to_string(),
        owner_device_id: record.owner_device_id.clone(),
        generation: record.generation,
        role: if recovery {
            "recovery"
        } else if local_owner && stable {
            "owner"
        } else {
            "read-only"
        },
        can_write: local_owner && stable,
        replicated_writes: false,
        transfer_phase: record.transfer_phase.clone(),
    }
}

fn require_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 256 {
        Err(format!("{label} is invalid"))
    } else {
        Ok(())
    }
}

fn read_state_file(path: &Path) -> Result<OwnershipStateFile, String> {
    recover_state_file(path)?;
    if !path.exists() {
        return Ok(OwnershipStateFile::default());
    }
    let raw = fs::read_to_string(path).map_err(|error| format!("read vault ownership: {error}"))?;
    let state: OwnershipStateFile =
        serde_json::from_str(&raw).map_err(|error| format!("parse vault ownership: {error}"))?;
    if state.schema_version != OWNERSHIP_STATE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported vault ownership schema version {}",
            state.schema_version
        ));
    }
    // Confirm the visible replacement or recovered rollback before admitting writes.
    sync_parent_directory(path)?;
    Ok(state)
}

fn validate_state(state: &OwnershipStateFile, device_id: &str) -> Result<(), String> {
    require_identifier(device_id, "device id")?;
    for (vault_id, record) in &state.vaults {
        require_identifier(vault_id, "vault id")?;
        require_identifier(&record.owner_device_id, "owner device id")?;
        if record.vault_id != *vault_id {
            return Err("vault ownership record identity does not match its key".to_string());
        }
        match &record.transfer_phase {
            TransferPhase::Stable | TransferPhase::RecoveryRequired { .. } => {}
            TransferPhase::PreparingOutgoing {
                receiver_device_id,
                next_generation,
                ..
            } => {
                if record.owner_device_id != device_id
                    || receiver_device_id == device_id
                    || record.generation.checked_add(1) != Some(*next_generation)
                {
                    return Err("preparing ownership transfer state is inconsistent".to_string());
                }
            }
            TransferPhase::OutgoingCommitted {
                receiver_device_id,
                committed_generation,
                ..
            } => {
                if record.owner_device_id != *receiver_device_id
                    || receiver_device_id == device_id
                    || record.generation != *committed_generation
                {
                    return Err("committed outgoing ownership state is inconsistent".to_string());
                }
            }
            TransferPhase::IncomingCommitted {
                committed_generation,
                ..
            } => {
                if record.owner_device_id != device_id || record.generation != *committed_generation
                {
                    return Err("committed incoming ownership state is inconsistent".to_string());
                }
            }
        }
    }
    Ok(())
}

fn recover_state_file(path: &Path) -> Result<(), String> {
    let rollback = path.with_extension("json.rollback");
    if !rollback.exists() {
        return Ok(());
    }
    if path.exists() {
        fs::remove_file(rollback)
            .map_err(|error| format!("remove completed vault ownership rollback: {error}"))
    } else {
        fs::rename(rollback, path)
            .map_err(|error| format!("recover vault ownership state: {error}"))
    }
}

fn persist_inner(inner: &mut OwnershipManagerInner) -> Result<(), String> {
    let path = inner
        .storage_path
        .as_deref()
        .ok_or_else(|| "vault ownership manager is not initialized".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create vault ownership directory: {error}"))?;
    }
    let temporary = path.with_extension("json.tmp");
    let rollback = path.with_extension("json.rollback");
    recover_state_file(path)?;
    if temporary.exists() {
        fs::remove_file(&temporary)
            .map_err(|error| format!("remove stale vault ownership temporary file: {error}"))?;
    }
    let bytes = serde_json::to_vec_pretty(&inner.state)
        .map_err(|error| format!("serialize vault ownership: {error}"))?;
    let mut file = fs::File::create(&temporary)
        .map_err(|error| format!("create vault ownership state: {error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("write vault ownership state: {error}"))?;
    let had_current = path.exists();
    if had_current {
        fs::rename(path, &rollback)
            .map_err(|error| format!("prepare vault ownership state replacement: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_current {
            if let Err(rollback_error) = fs::rename(&rollback, path) {
                let message = format!(
                    "activate vault ownership state: {error}; restore previous state: {rollback_error}; restart after resolving the storage error"
                );
                inner.persistence_uncertain = Some(message.clone());
                return Err(message);
            }
        }
        return Err(format!("activate vault ownership state: {error}"));
    }
    #[cfg(test)]
    let sync_result = if std::mem::take(&mut inner.fail_next_directory_sync) {
        Err("injected vault ownership directory sync failure".to_string())
    } else {
        sync_parent_directory(path)
    };
    #[cfg(not(test))]
    let sync_result = sync_parent_directory(path);
    if let Err(error) = sync_result {
        let message = format!(
            "vault ownership replacement durability is uncertain: {error}; restart after resolving the storage error"
        );
        inner.persistence_uncertain = Some(message.clone());
        return Err(message);
    }
    if had_current {
        if let Err(error) = fs::remove_file(&rollback) {
            eprintln!("remove completed vault ownership rollback: {error}");
        }
    }
    Ok(())
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "vault ownership state has no parent directory".to_string())?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("sync vault ownership directory: {error}"))
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

pub(crate) fn initialize<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    app.state::<VaultOwnershipManager>().initialize(app)
}

pub(crate) fn active_status<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<VaultOwnershipStatus, String> {
    let vault_id = active_vault_id(app)?;
    app.state::<VaultOwnershipManager>().status(&vault_id)
}

#[tauri::command]
pub(crate) fn vault_ownership_status<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<VaultOwnershipStatus, String> {
    active_status(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ganbaru-vault-ownership-{}-{}-{name}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn load_manager(path: &Path, device_id: &str) -> VaultOwnershipManager {
        let manager = VaultOwnershipManager::default();
        manager
            .initialize_from_path(path.to_path_buf(), device_id.to_string())
            .unwrap();
        manager
    }

    #[test]
    fn transfer_phase_serializes_variant_fields_in_camel_case() {
        let phase = TransferPhase::PreparingOutgoing {
            transfer_id: "transfer".into(),
            receiver_device_id: "phone".into(),
            next_generation: 3,
        };
        let json = serde_json::to_value(&phase).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "preparingOutgoing",
                "transferId": "transfer",
                "receiverDeviceId": "phone",
                "nextGeneration": 3,
            })
        );
        assert_eq!(
            serde_json::from_value::<TransferPhase>(json).unwrap(),
            phase
        );
    }

    #[test]
    fn native_callback_fails_closed_without_waiting_behind_ownership_io() {
        let manager = VaultOwnershipManager::default();
        let _io = manager.inner.lock().unwrap();
        assert_eq!(
            manager.cached_status("vault").unwrap_err(),
            "vault ownership is busy"
        );
    }

    #[test]
    fn native_callback_cached_authority_never_materializes_unknown_vaults_and_tracks_revocation() {
        let path = test_path("cached-callback");
        let manager = load_manager(&path, "desktop");
        assert_eq!(manager.cached_status("unknown").unwrap(), None);
        assert!(manager.inner.lock().unwrap().state.vaults.is_empty());
        let accepted = manager.status("vault").unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(manager.cached_status("vault").unwrap(), Some(accepted));
        assert!(!path.exists());
        manager
            .begin_outgoing("vault", 0, "transfer".into(), "phone".into())
            .unwrap();
        assert!(!manager.cached_status("vault").unwrap().unwrap().can_write);
        manager.inner.lock().unwrap().persistence_uncertain = Some("uncertain ownership".into());
        assert_eq!(
            manager.cached_status("vault").unwrap_err(),
            "uncertain ownership"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn failure_before_replacement_preserves_the_durable_owner() {
        let path = test_path("before-replacement");
        let manager = load_manager(&path, "desktop");
        assert!(manager.status("vault").unwrap().can_write);
        let temporary = path.with_extension("json.tmp");
        fs::create_dir(&temporary).unwrap();
        assert!(
            manager
                .begin_outgoing("vault", 0, "transfer".into(), "phone".into())
                .is_err()
        );
        assert!(manager.status("vault").unwrap().can_write);
        assert!(
            load_manager(&path, "desktop")
                .status("vault")
                .unwrap()
                .can_write
        );
        fs::remove_dir(temporary).unwrap();
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn uncertain_outgoing_commit_never_restores_the_previous_owner_in_memory() {
        let path = test_path("uncertain-outgoing");
        let manager = load_manager(&path, "desktop");
        manager
            .begin_outgoing("vault", 0, "transfer".into(), "phone".into())
            .unwrap();
        manager.inner.lock().unwrap().fail_next_directory_sync = true;
        assert!(
            manager
                .commit_outgoing("vault", "transfer")
                .unwrap_err()
                .contains("durability is uncertain")
        );
        assert!(manager.status("vault").is_err());
        assert!(manager.acquire_managed_write("vault").is_err());
        assert!(manager.abort_outgoing("vault", "transfer").is_err());
        assert!(manager.commit_outgoing("vault", "transfer").is_err());
        assert!(
            manager
                .register_remote_owner_if_missing("vault", "phone".into(), 1)
                .is_err()
        );
        {
            let inner = manager.inner.lock().unwrap();
            assert_eq!(inner.state.vaults["vault"].owner_device_id, "phone");
            assert_eq!(inner.state.vaults["vault"].generation, 1);
        }
        let restarted = load_manager(&path, "desktop");
        let status = restarted.status("vault").unwrap();
        assert!(!status.can_write);
        assert_eq!(status.owner_device_id, "phone");
        assert_eq!(status.generation, 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn linked_stable_replicas_get_guarded_access_and_owners_stay_read_write() {
        let path = test_path("guarded-access");
        let replica = load_manager(&path, "phone");
        replica
            .register_remote_owner("vault", "desktop".into(), 0)
            .unwrap();
        assert_eq!(
            replica.database_access("vault").unwrap(),
            VaultDatabaseAccess::ReadOnly
        );
        replica
            .set_link_probe(Some(Arc::new(|vault_id: &str| vault_id == "vault")))
            .unwrap();
        let status = replica.status("vault").unwrap();
        assert!(status.replicated_writes && !status.can_write);
        assert!(
            replica
                .cached_status("vault")
                .unwrap()
                .unwrap()
                .replicated_writes
        );
        assert_eq!(
            replica.database_access("vault").unwrap(),
            VaultDatabaseAccess::Guarded
        );
        assert!(replica.require_writable("vault").is_err());

        replica
            .accept_incoming_grant("vault", "transfer".into(), "desktop".into(), 1)
            .unwrap();
        assert!(!replica.status("vault").unwrap().replicated_writes);
        assert_eq!(
            replica.database_access("vault").unwrap(),
            VaultDatabaseAccess::ReadOnly
        );
        fs::remove_file(&path).unwrap();

        let owner = load_manager(&path, "desktop");
        owner
            .set_link_probe(Some(Arc::new(|_: &str| true)))
            .unwrap();
        let status = owner.status("vault").unwrap();
        assert!(status.can_write && !status.replicated_writes);
        assert_eq!(
            owner.database_access("vault").unwrap(),
            VaultDatabaseAccess::ReadWrite
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn uncertain_incoming_activation_blocks_writes_until_durable_reload() {
        let path = test_path("uncertain-incoming");
        let manager = load_manager(&path, "phone");
        manager
            .register_remote_owner("vault", "desktop".into(), 0)
            .unwrap();
        manager
            .accept_incoming_grant("vault", "transfer".into(), "desktop".into(), 1)
            .unwrap();
        manager.inner.lock().unwrap().fail_next_directory_sync = true;
        assert!(manager.finalize_incoming("vault", "transfer", 1).is_err());
        assert!(manager.acquire_managed_write("vault").is_err());
        assert!(manager.database_access("vault").is_err());
        assert!(
            load_manager(&path, "phone")
                .status("vault")
                .unwrap()
                .can_write
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn uncertain_initial_record_is_not_removed_or_recreated_as_writable() {
        let path = test_path("uncertain-initial");
        let manager = load_manager(&path, "desktop");
        manager.inner.lock().unwrap().fail_next_directory_sync = true;
        assert!(manager.status("vault").is_err());
        assert!(
            manager
                .inner
                .lock()
                .unwrap()
                .state
                .vaults
                .contains_key("vault")
        );
        assert!(manager.status("vault").is_err());
        assert!(
            load_manager(&path, "desktop")
                .status("vault")
                .unwrap()
                .can_write
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn pre_commit_failure_restores_source_owner() {
        let path = test_path("abort");
        let manager = load_manager(&path, "desktop");
        manager
            .begin_outgoing("vault", 0, "transfer".into(), "phone".into())
            .unwrap();
        assert!(!manager.status("vault").unwrap().can_write);

        manager.abort_outgoing("vault", "transfer").unwrap();

        let status = manager.status("vault").unwrap();
        assert!(status.can_write);
        assert_eq!(status.generation, 0);
        assert_eq!(status.owner_device_id, "desktop");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn committed_source_stays_read_only_after_restart() {
        let path = test_path("committed-restart");
        let manager = load_manager(&path, "desktop");
        manager
            .begin_outgoing("vault", 0, "transfer".into(), "phone".into())
            .unwrap();
        assert_eq!(manager.commit_outgoing("vault", "transfer").unwrap(), 1);
        drop(manager);

        let restarted = load_manager(&path, "desktop");
        let status = restarted.status("vault").unwrap();
        assert!(!status.can_write);
        assert_eq!(status.generation, 1);
        assert_eq!(status.owner_device_id, "phone");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn uncommitted_source_recovers_as_owner_after_restart() {
        let path = test_path("precommit-restart");
        let manager = load_manager(&path, "desktop");
        manager
            .begin_outgoing("vault", 0, "transfer".into(), "phone".into())
            .unwrap();
        drop(manager);

        let restarted = load_manager(&path, "desktop");
        assert!(restarted.status("vault").unwrap().can_write);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn stale_grant_cannot_restore_write_access() {
        let path = test_path("stale");
        let manager = load_manager(&path, "phone");
        manager
            .register_remote_owner("vault", "desktop".into(), 0)
            .unwrap();
        manager
            .accept_incoming_grant("vault", "first".into(), "desktop".into(), 1)
            .unwrap();
        manager.finalize_incoming("vault", "first", 1).unwrap();
        assert!(manager.status("vault").unwrap().can_write);

        assert!(
            manager
                .accept_incoming_grant("vault", "stale".into(), "desktop".into(), 1)
                .is_err()
        );
        assert!(manager.status("vault").unwrap().can_write);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn incoming_owner_is_not_writable_until_activation_finishes() {
        let path = test_path("incoming");
        let manager = load_manager(&path, "phone");
        manager
            .register_remote_owner("vault", "desktop".into(), 0)
            .unwrap();
        manager
            .accept_incoming_grant("vault", "transfer".into(), "desktop".into(), 1)
            .unwrap();
        assert!(!manager.status("vault").unwrap().can_write);

        manager.finalize_incoming("vault", "transfer", 1).unwrap();
        assert!(manager.status("vault").unwrap().can_write);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn committed_receiver_resumes_same_grant_after_restart() {
        let path = test_path("incoming-restart");
        let manager = load_manager(&path, "phone");
        manager
            .register_remote_owner("vault", "desktop".into(), 0)
            .unwrap();
        manager
            .accept_incoming_grant("vault", "transfer".into(), "desktop".into(), 1)
            .unwrap();
        drop(manager);

        let restarted = load_manager(&path, "phone");
        let status = restarted.status("vault").unwrap();
        assert!(!status.can_write);
        assert!(matches!(
            status.transfer_phase,
            TransferPhase::IncomingCommitted { ref transfer_id, .. } if transfer_id == "transfer"
        ));
        restarted
            .accept_incoming_grant("vault", "transfer".into(), "desktop".into(), 1)
            .unwrap();
        restarted.finalize_incoming("vault", "transfer", 1).unwrap();
        assert!(restarted.status("vault").unwrap().can_write);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn managed_write_fence_drains_and_rejects_new_writers() {
        let path = test_path("write-fence");
        let manager = Arc::new(load_manager(&path, "desktop"));
        let permit = manager.acquire_managed_write("vault").unwrap();
        let waiting_manager = Arc::clone(&manager);
        let waiter = std::thread::spawn(move || waiting_manager.fence_managed_writes().unwrap());
        std::thread::yield_now();
        assert!(!waiter.is_finished());

        drop(permit);
        let fence = waiter.join().unwrap();
        assert!(manager.acquire_managed_write("vault").is_err());
        drop(fence);
        assert!(manager.acquire_managed_write("vault").is_ok());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn stalled_managed_writer_cancels_transition_and_restores_admission() {
        let path = test_path("write-fence-timeout");
        let manager = load_manager(&path, "desktop");
        let permit = manager.acquire_managed_write("vault").unwrap();
        let error = manager
            .fence_managed_writes_with_budget(Duration::from_millis(1))
            .err()
            .unwrap();
        assert!(error.contains("have not drained"));
        assert!(manager.acquire_managed_write("vault").is_ok());
        drop(permit);
        let fence = manager.fence_managed_writes().unwrap();
        assert!(manager.acquire_managed_write("vault").is_err());
        drop(fence);
        assert!(manager.acquire_managed_write("vault").is_ok());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn committed_generation_is_writable_on_exactly_one_device() {
        let desktop_path = test_path("desktop");
        let phone_path = test_path("phone");
        let desktop = load_manager(&desktop_path, "desktop");
        let phone = load_manager(&phone_path, "phone");
        phone
            .register_remote_owner("vault", "desktop".into(), 0)
            .unwrap();
        desktop
            .begin_outgoing("vault", 0, "transfer".into(), "phone".into())
            .unwrap();
        let generation = desktop.commit_outgoing("vault", "transfer").unwrap();
        phone
            .accept_incoming_grant("vault", "transfer".into(), "desktop".into(), generation)
            .unwrap();
        phone
            .finalize_incoming("vault", "transfer", generation)
            .unwrap();

        assert!(!desktop.status("vault").unwrap().can_write);
        assert!(phone.status("vault").unwrap().can_write);
        assert!(
            desktop
                .accept_incoming_grant("vault", "stale".into(), "phone".into(), generation,)
                .is_err()
        );
        assert!(!desktop.status("vault").unwrap().can_write);
        let _ = fs::remove_file(desktop_path);
        let _ = fs::remove_file(phone_path);
    }

    #[test]
    fn incoming_grant_requires_current_owner_and_exact_next_generation() {
        let path = test_path("grant-boundary");
        let manager = load_manager(&path, "phone");
        manager
            .register_remote_owner("vault", "desktop".into(), 4)
            .unwrap();

        assert!(
            manager
                .accept_incoming_grant("vault", "wrong-source".into(), "other".into(), 5)
                .is_err()
        );
        assert!(
            manager
                .accept_incoming_grant("vault", "skipped".into(), "desktop".into(), 6)
                .is_err()
        );
        let status = manager.status("vault").unwrap();
        assert_eq!(status.owner_device_id, "desktop");
        assert_eq!(status.generation, 4);
        assert!(!status.can_write);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn coordinator_grant_can_advance_across_intermediate_owner_generations() {
        let path = test_path("coordinator-grant");
        let manager = load_manager(&path, "phone-two");
        manager
            .register_remote_owner("vault", "desktop".into(), 2)
            .unwrap();

        manager
            .accept_incoming_coordinator_grant("vault", "transfer".into(), "desktop".into(), 5)
            .unwrap();
        manager.finalize_incoming("vault", "transfer", 5).unwrap();

        let status = manager.status("vault").unwrap();
        assert!(status.can_write);
        assert_eq!(status.owner_device_id, "phone-two");
        assert_eq!(status.generation, 5);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn coordinator_grant_rejects_an_untrusted_source_and_stale_generation() {
        let path = test_path("coordinator-grant-boundary");
        let manager = load_manager(&path, "phone-two");
        manager
            .register_remote_owner("vault", "desktop".into(), 4)
            .unwrap();

        assert!(
            manager
                .accept_incoming_coordinator_grant(
                    "vault",
                    "wrong-source".into(),
                    "other".into(),
                    5,
                )
                .is_err()
        );
        assert!(
            manager
                .accept_incoming_coordinator_grant("vault", "stale".into(), "desktop".into(), 3,)
                .is_err()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn remote_coordinator_generation_advances_after_relayed_refresh() {
        let path = test_path("coordinator-refresh");
        let manager = load_manager(&path, "phone-two");
        manager
            .register_remote_owner("vault", "desktop".into(), 2)
            .unwrap();

        manager
            .advance_remote_coordinator("vault", "desktop", 6)
            .unwrap();

        let status = manager.status("vault").unwrap();
        assert!(!status.can_write);
        assert_eq!(status.owner_device_id, "desktop");
        assert_eq!(status.generation, 6);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn linked_owner_repair_only_creates_a_missing_record() {
        let path = test_path("linked-owner-repair");
        let manager = load_manager(&path, "phone");
        manager
            .register_remote_owner_if_missing("vault", "desktop".into(), 4)
            .unwrap();
        manager
            .register_remote_owner_if_missing("vault", "other".into(), 0)
            .unwrap();

        let status = manager.status("vault").unwrap();
        assert_eq!(status.owner_device_id, "desktop");
        assert_eq!(status.generation, 4);
        assert!(!status.can_write);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn explicit_lost_device_recovery_creates_a_new_local_generation() {
        let path = test_path("explicit-recovery");
        let manager = load_manager(&path, "phone");
        manager
            .register_remote_owner("vault", "desktop".into(), 4)
            .unwrap();

        assert_eq!(manager.recover_local_copy("vault").unwrap(), 5);
        let status = manager.status("vault").unwrap();
        assert!(status.can_write);
        assert_eq!(status.owner_device_id, "phone");
        assert_eq!(status.generation, 5);

        assert!(manager.recover_local_copy("vault").is_err());
        let _ = fs::remove_file(path);
    }
}
