//! Process-local fence over managed vault writes.
//!
//! Commands that write the vault database or its managed assets hold a permit for the duration of
//! the write. Operations that replace the vault (bootstrap activation, backup restore, and rollback
//! restore) raise the fence, wait for every permit to drop, and refuse new permits until the fence
//! is dropped, so no command writes into a database that is being swapped out.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Error of a managed write refused while the vault is being replaced.
pub(crate) const VAULT_REPLACING_ERROR: &str = "the vault is being replaced";

/// How long raising the fence waits for active writers before it gives up.
const MANAGED_WRITE_DRAIN_BUDGET: Duration = Duration::from_secs(10);

/// Managed state shared by every managed writer and every vault replacement of this process.
/// Clones share one fence.
#[derive(Clone, Default)]
pub(crate) struct VaultWriteFence {
    control: Arc<FenceControl>,
}

#[derive(Default)]
struct FenceControl {
    state: Mutex<FenceState>,
    drained: Condvar,
}

#[derive(Default)]
struct FenceState {
    fenced: bool,
    active_writers: usize,
}

/// Admission of one managed write; dropping it lets a waiting fence proceed.
pub(crate) struct ManagedVaultWritePermit {
    control: Arc<FenceControl>,
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

/// A raised fence; managed writes are refused until it is dropped.
pub(crate) struct ManagedVaultWriteFence {
    control: Arc<FenceControl>,
}

impl Drop for ManagedVaultWriteFence {
    fn drop(&mut self) {
        if let Ok(mut state) = self.control.state.lock() {
            state.fenced = false;
            self.control.drained.notify_all();
        }
    }
}

impl VaultWriteFence {
    /// Admits one managed write, or refuses it while the vault is being replaced.
    pub(crate) fn acquire_managed_write(&self) -> Result<ManagedVaultWritePermit, String> {
        let mut state = self
            .control
            .state
            .lock()
            .map_err(|_| "managed vault write fence is unavailable".to_string())?;
        if state.fenced {
            return Err(VAULT_REPLACING_ERROR.to_string());
        }
        state.active_writers = state
            .active_writers
            .checked_add(1)
            .ok_or_else(|| "managed vault writer count is exhausted".to_string())?;
        Ok(ManagedVaultWritePermit {
            control: Arc::clone(&self.control),
        })
    }

    /// Raises the fence and waits for active managed writes to finish.
    pub(crate) fn fence_managed_writes(&self) -> Result<ManagedVaultWriteFence, String> {
        self.fence_managed_writes_with_budget(MANAGED_WRITE_DRAIN_BUDGET)
    }

    fn fence_managed_writes_with_budget(
        &self,
        budget: Duration,
    ) -> Result<ManagedVaultWriteFence, String> {
        let started = Instant::now();
        let mut state = self
            .control
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
                self.control.drained.notify_all();
                return Err(
                    "Managed vault writes have not drained; vault operation cancelled".to_string(),
                );
            };
            let (next_state, _) = self
                .control
                .drained
                .wait_timeout(state, remaining)
                .map_err(|_| "managed vault write fence is unavailable".to_string())?;
            state = next_state;
        }
        Ok(ManagedVaultWriteFence {
            control: Arc::clone(&self.control),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_write_fence_drains_and_rejects_new_writers() {
        let fence = VaultWriteFence::default();
        let permit = fence.acquire_managed_write().unwrap();
        let waiting = fence.clone();
        let waiter = std::thread::spawn(move || waiting.fence_managed_writes().unwrap());
        std::thread::yield_now();
        assert!(!waiter.is_finished());

        drop(permit);
        let raised = waiter.join().unwrap();
        assert_eq!(
            fence.acquire_managed_write().err().as_deref(),
            Some(VAULT_REPLACING_ERROR)
        );
        drop(raised);
        assert!(fence.acquire_managed_write().is_ok());
    }

    #[test]
    fn stalled_managed_writer_cancels_transition_and_restores_admission() {
        let fence = VaultWriteFence::default();
        let permit = fence.acquire_managed_write().unwrap();
        let error = fence
            .fence_managed_writes_with_budget(Duration::from_millis(1))
            .err()
            .unwrap();
        assert!(error.contains("have not drained"));
        assert!(fence.acquire_managed_write().is_ok());
        drop(permit);
        let raised = fence.fence_managed_writes().unwrap();
        assert!(fence.acquire_managed_write().is_err());
        drop(raised);
        assert!(fence.acquire_managed_write().is_ok());
    }

    #[test]
    fn a_second_fence_is_refused_while_one_is_raised() {
        let fence = VaultWriteFence::default();
        let raised = fence.fence_managed_writes().unwrap();
        assert!(fence.fence_managed_writes().is_err());
        drop(raised);
        assert!(fence.fence_managed_writes().is_ok());
    }
}
