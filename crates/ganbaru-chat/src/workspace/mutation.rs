//! Cross-surface coordination for workspace and Git mutations.

use crate::models::{ChatError, ChatErrorCode, ChatResult, ChatThreadId, ChatTurnId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{OwnedRwLockWriteGuard, RwLock};

#[derive(Default)]
pub struct ChatWorkspaceMutationRegistry {
    inner: Arc<ChatWorkspaceMutationRegistryInner>,
}

#[derive(Default)]
struct ChatWorkspaceMutationRegistryInner {
    locks: Mutex<HashMap<PathBuf, Weak<RwLock<()>>>>,
    provider_turns: Mutex<HashMap<String, ProviderTurnReservationEntry>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ProviderTurnReservationOwner {
    PendingCommand,
    RuntimeCommand,
    EventSink,
}

struct ProviderTurnReservationEntry {
    owner: ProviderTurnReservationOwner,
    _guard: OwnedRwLockWriteGuard<()>,
}

#[must_use = "dropping the pending reservation releases the workspace"]
pub struct PendingProviderTurnReservation {
    inner: Arc<ChatWorkspaceMutationRegistryInner>,
    key: String,
    active: bool,
}

#[must_use = "the runtime handoff must be completed or dropped"]
pub struct ProviderTurnReservationHandoff {
    inner: Arc<ChatWorkspaceMutationRegistryInner>,
    key: String,
    active: bool,
}

pub struct ThreadReservationCleanup {
    inner: Arc<ChatWorkspaceMutationRegistryInner>,
    thread_id: ChatThreadId,
    armed: bool,
}

impl ChatWorkspaceMutationRegistry {
    /// Reserves a workspace for one provider turn until its terminal event is ingested.
    pub fn begin_provider_turn(
        &self,
        root: &Path,
        thread_id: &ChatThreadId,
        turn_id: &ChatTurnId,
    ) -> ChatResult<PendingProviderTurnReservation> {
        let key = turn_key(thread_id, turn_id);
        let mut turns = self
            .inner
            .provider_turns
            .lock()
            .map_err(|_| registry_error())?;
        if turns.contains_key(&key) {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "This provider turn already owns the workspace mutation reservation",
                true,
            ));
        }
        let guard = self
            .lock_for(root)?
            .try_write_owned()
            .map_err(|_| workspace_busy_error())?;
        turns.insert(
            key.clone(),
            ProviderTurnReservationEntry {
                owner: ProviderTurnReservationOwner::PendingCommand,
                _guard: guard,
            },
        );
        Ok(PendingProviderTurnReservation {
            inner: Arc::clone(&self.inner),
            key,
            active: true,
        })
    }

    /// Releases the provider reservation after post-turn checkpoint capture settles.
    pub fn finish_provider_turn(&self, thread_id: &ChatThreadId, turn_id: &ChatTurnId) {
        if let Ok(mut turns) = self.inner.provider_turns.lock() {
            turns.remove(&turn_key(thread_id, turn_id));
        }
    }

    /// Releases runtime and event-sink reservations after a thread has stopped.
    ///
    /// Pending sends keep their own cancellation guards, so stopping an older
    /// session cannot unprotect a turn that has not reached the runtime.
    pub fn finish_thread(&self, thread_id: &ChatThreadId) {
        self.inner.finish_thread(thread_id);
    }

    pub fn thread_cleanup(&self, thread_id: &ChatThreadId) -> ThreadReservationCleanup {
        ThreadReservationCleanup {
            inner: Arc::clone(&self.inner),
            thread_id: thread_id.clone(),
            armed: false,
        }
    }

    /// Acquires an exclusive mutation reservation without waiting behind an active agent.
    pub fn try_mutation(&self, root: &Path) -> ChatResult<OwnedRwLockWriteGuard<()>> {
        self.lock_for(root)?
            .try_write_owned()
            .map_err(|_| workspace_busy_error())
    }

    /// Waits for an active provider turn to settle, with a bounded deadline.
    pub async fn mutation_with_timeout(
        &self,
        root: &Path,
        timeout: Duration,
    ) -> ChatResult<OwnedRwLockWriteGuard<()>> {
        tokio::time::timeout(timeout, self.lock_for(root)?.write_owned())
            .await
            .map_err(|_| {
                ChatError::new(
                    ChatErrorCode::Timeout,
                    "The workspace is still being changed by an active coding agent",
                    true,
                )
            })
    }

    fn lock_for(&self, root: &Path) -> ChatResult<Arc<RwLock<()>>> {
        let mut locks = self.inner.locks.lock().map_err(|_| registry_error())?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(root).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(RwLock::new(()));
        locks.insert(root.to_path_buf(), Arc::downgrade(&lock));
        Ok(lock)
    }
}

impl ChatWorkspaceMutationRegistryInner {
    fn transition(
        &self,
        key: &str,
        from: ProviderTurnReservationOwner,
        to: ProviderTurnReservationOwner,
    ) -> ChatResult<bool> {
        let mut turns = self.provider_turns.lock().map_err(|_| registry_error())?;
        let Some(entry) = turns.get_mut(key) else {
            return Ok(false);
        };
        if entry.owner != from {
            return Err(registry_error());
        }
        entry.owner = to;
        Ok(true)
    }

    fn remove(&self, key: &str) {
        if let Ok(mut turns) = self.provider_turns.lock() {
            turns.remove(key);
        }
    }

    fn finish_thread(&self, thread_id: &ChatThreadId) {
        let prefix = format!("{}\0", thread_id.as_str());
        if let Ok(mut turns) = self.provider_turns.lock() {
            turns.retain(|key, entry| {
                !key.starts_with(&prefix)
                    || entry.owner == ProviderTurnReservationOwner::PendingCommand
            });
        }
    }
}

impl PendingProviderTurnReservation {
    pub fn handoff_to_runtime(mut self) -> ChatResult<ProviderTurnReservationHandoff> {
        if !self.inner.transition(
            &self.key,
            ProviderTurnReservationOwner::PendingCommand,
            ProviderTurnReservationOwner::RuntimeCommand,
        )? {
            return Err(registry_error());
        }
        self.active = false;
        Ok(ProviderTurnReservationHandoff {
            inner: Arc::clone(&self.inner),
            key: self.key.clone(),
            active: true,
        })
    }
}

impl Drop for PendingProviderTurnReservation {
    fn drop(&mut self) {
        if self.active {
            self.inner.remove(&self.key);
        }
    }
}

impl ProviderTurnReservationHandoff {
    pub fn handoff_to_event_sink(mut self) {
        match self.inner.transition(
            &self.key,
            ProviderTurnReservationOwner::RuntimeCommand,
            ProviderTurnReservationOwner::EventSink,
        ) {
            Ok(true) | Ok(false) => self.active = false,
            Err(_) => {}
        }
    }
}

impl Drop for ProviderTurnReservationHandoff {
    fn drop(&mut self) {
        if self.active {
            self.inner.remove(&self.key);
        }
    }
}

impl ThreadReservationCleanup {
    pub fn arm(&mut self) {
        self.armed = true;
    }
}

impl Drop for ThreadReservationCleanup {
    fn drop(&mut self) {
        if self.armed {
            self.inner.finish_thread(&self.thread_id);
        }
    }
}

fn turn_key(thread_id: &ChatThreadId, turn_id: &ChatTurnId) -> String {
    format!("{}\0{}", thread_id.as_str(), turn_id.as_str())
}

fn workspace_busy_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Conflict,
        "The workspace is being changed by another Chat or source-control operation",
        true,
    )
}

fn registry_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Workspace mutation coordination is unavailable",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_turn_blocks_mutations_until_it_finishes() {
        let registry = ChatWorkspaceMutationRegistry::default();
        let root = Path::new("/tmp/ganbaru-workspace-mutation-test");
        let thread = ChatThreadId::new("thread:test").expect("thread ID should be valid");
        let turn = ChatTurnId::new("turn:test").expect("turn ID should be valid");
        let other_thread = ChatThreadId::new("thread:other").expect("thread ID should be valid");
        let other_turn = ChatTurnId::new("turn:other").expect("turn ID should be valid");

        let pending = registry
            .begin_provider_turn(root, &thread, &turn)
            .expect("provider reservation should start");
        assert!(registry.begin_provider_turn(root, &thread, &turn).is_err());
        assert!(
            registry
                .begin_provider_turn(root, &other_thread, &other_turn)
                .is_err()
        );
        assert!(registry.try_mutation(root).is_err());
        registry.finish_thread(&thread);
        assert!(registry.try_mutation(root).is_err());
        drop(pending);
        assert!(registry.try_mutation(root).is_ok());

        let handoff = registry
            .begin_provider_turn(root, &thread, &turn)
            .expect("provider reservation should restart")
            .handoff_to_runtime()
            .expect("runtime should own the reservation");
        registry.finish_thread(&thread);
        assert!(registry.try_mutation(root).is_ok());
        drop(handoff);

        let handoff = registry
            .begin_provider_turn(root, &thread, &turn)
            .expect("provider reservation should restart")
            .handoff_to_runtime()
            .expect("runtime should own the reservation");
        drop(handoff);
        assert!(registry.try_mutation(root).is_ok());

        let handoff = registry
            .begin_provider_turn(root, &thread, &turn)
            .expect("provider reservation should restart")
            .handoff_to_runtime()
            .expect("runtime should own the reservation");
        handoff.handoff_to_event_sink();
        assert!(registry.try_mutation(root).is_err());
        registry.finish_provider_turn(&thread, &turn);
        assert!(registry.try_mutation(root).is_ok());
    }
}
