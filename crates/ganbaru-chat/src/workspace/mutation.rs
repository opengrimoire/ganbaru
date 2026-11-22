//! Cross-surface coordination for workspace and Git mutations.

use crate::models::{ChatError, ChatErrorCode, ChatResult, ChatThreadId, ChatTurnId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// Source of [`ReservationToken`]s, so every reservation of one turn key is distinct.
    next_token: AtomicU64,
}

/// Identity of one reservation of a turn key. A handle moves or releases only the entry that
/// still carries its token, so a stale handle can never touch a later reservation of the same
/// turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReservationToken(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProviderTurnReservationOwner {
    PendingCommand,
    RuntimeCommand,
    EventSink,
}

struct ProviderTurnReservationEntry {
    token: ReservationToken,
    owner: ProviderTurnReservationOwner,
    _guard: OwnedRwLockWriteGuard<()>,
}

/// Why a reservation handle could not move its entry to the next owner.
#[derive(Debug, Eq, PartialEq)]
enum ReservationMoveRejection {
    /// The reservation was released, by its turn finishing or its thread stopping, before the
    /// move. The key may now belong to a later reservation.
    Released,
    /// The entry with this token has another owner, which no handle sequence produces.
    WrongOwner,
    /// The registry lock is poisoned.
    Unavailable,
}

#[must_use = "dropping the pending reservation releases the workspace"]
pub struct PendingProviderTurnReservation {
    inner: Arc<ChatWorkspaceMutationRegistryInner>,
    key: String,
    token: ReservationToken,
    active: bool,
}

#[must_use = "the runtime handoff must be completed or dropped"]
pub struct ProviderTurnReservationHandoff {
    inner: Arc<ChatWorkspaceMutationRegistryInner>,
    key: String,
    token: ReservationToken,
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
        let token = ReservationToken(self.inner.next_token.fetch_add(1, Ordering::Relaxed));
        turns.insert(
            key.clone(),
            ProviderTurnReservationEntry {
                token,
                owner: ProviderTurnReservationOwner::PendingCommand,
                _guard: guard,
            },
        );
        Ok(PendingProviderTurnReservation {
            inner: Arc::clone(&self.inner),
            key,
            token,
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
    /// Moves the reservation `token` holds on `key` from `from` to `to`.
    fn transition(
        &self,
        key: &str,
        token: ReservationToken,
        from: ProviderTurnReservationOwner,
        to: ProviderTurnReservationOwner,
    ) -> Result<(), ReservationMoveRejection> {
        let mut turns = self
            .provider_turns
            .lock()
            .map_err(|_| ReservationMoveRejection::Unavailable)?;
        let entry = turns
            .get_mut(key)
            .filter(|entry| entry.token == token)
            .ok_or(ReservationMoveRejection::Released)?;
        if entry.owner != from {
            return Err(ReservationMoveRejection::WrongOwner);
        }
        entry.owner = to;
        Ok(())
    }

    /// Releases the reservation `token` holds on `key`, leaving a later reservation in place.
    fn remove(&self, key: &str, token: ReservationToken) {
        if let Ok(mut turns) = self.provider_turns.lock()
            && turns.get(key).is_some_and(|entry| entry.token == token)
        {
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
    /// Hands the reservation to the runtime command. Fails with a conflict when the reservation
    /// was released before the runtime could take it.
    pub fn handoff_to_runtime(mut self) -> ChatResult<ProviderTurnReservationHandoff> {
        match self.inner.transition(
            &self.key,
            self.token,
            ProviderTurnReservationOwner::PendingCommand,
            ProviderTurnReservationOwner::RuntimeCommand,
        ) {
            Ok(()) => {}
            Err(ReservationMoveRejection::Released) => {
                self.active = false;
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "The provider turn's workspace reservation was released before the runtime took it",
                    true,
                ));
            }
            Err(ReservationMoveRejection::WrongOwner | ReservationMoveRejection::Unavailable) => {
                return Err(registry_error());
            }
        }
        self.active = false;
        Ok(ProviderTurnReservationHandoff {
            inner: Arc::clone(&self.inner),
            key: self.key.clone(),
            token: self.token,
            active: true,
        })
    }
}

impl Drop for PendingProviderTurnReservation {
    fn drop(&mut self) {
        if self.active {
            self.inner.remove(&self.key, self.token);
        }
    }
}

impl ProviderTurnReservationHandoff {
    /// Hands the reservation to the event sink, which keeps it until the turn finishes. A
    /// reservation already released stays released; one that cannot move is released when this
    /// handle drops, so the workspace never stays reserved without an owner.
    pub fn handoff_to_event_sink(mut self) {
        match self.inner.transition(
            &self.key,
            self.token,
            ProviderTurnReservationOwner::RuntimeCommand,
            ProviderTurnReservationOwner::EventSink,
        ) {
            Ok(()) | Err(ReservationMoveRejection::Released) => self.active = false,
            Err(ReservationMoveRejection::WrongOwner | ReservationMoveRejection::Unavailable) => {}
        }
    }
}

impl Drop for ProviderTurnReservationHandoff {
    fn drop(&mut self) {
        if self.active {
            self.inner.remove(&self.key, self.token);
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

    #[test]
    fn a_stale_reservation_never_releases_a_newer_reservation_of_the_same_turn() {
        let registry = ChatWorkspaceMutationRegistry::default();
        let root = Path::new("/tmp/ganbaru-workspace-mutation-stale-test");
        let thread = ChatThreadId::new("thread:test").expect("thread ID should be valid");
        let turn = ChatTurnId::new("turn:test").expect("turn ID should be valid");

        let stale_handoff = registry
            .begin_provider_turn(root, &thread, &turn)
            .expect("provider reservation should start")
            .handoff_to_runtime()
            .expect("runtime should own the reservation");
        registry.finish_thread(&thread);
        let newer = registry
            .begin_provider_turn(root, &thread, &turn)
            .expect("the released turn should reserve again");
        drop(stale_handoff);
        assert!(registry.try_mutation(root).is_err());

        let stale_pending = newer;
        registry.finish_provider_turn(&thread, &turn);
        let newest = registry
            .begin_provider_turn(root, &thread, &turn)
            .expect("the released turn should reserve again");
        let error = match stale_pending.handoff_to_runtime() {
            Ok(_) => panic!("a released reservation must not reach the runtime"),
            Err(error) => error,
        };
        assert_eq!(error.code, ChatErrorCode::Conflict);
        assert!(registry.try_mutation(root).is_err());
        let handoff = newest
            .handoff_to_runtime()
            .expect("the live reservation should reach the runtime");
        handoff.handoff_to_event_sink();
        assert!(registry.try_mutation(root).is_err());
        registry.finish_provider_turn(&thread, &turn);
        assert!(registry.try_mutation(root).is_ok());
    }
}
