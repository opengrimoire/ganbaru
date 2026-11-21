use super::ThreadRuntimeSnapshot;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::Notify;

/// The runtime state of one thread, shared by its owner, worker, and event sink. Every change
/// goes through [`SnapshotCell::update`], which wakes the tasks waiting in
/// [`SnapshotCell::wait_until`].
pub(super) struct SnapshotCell {
    state: Mutex<ThreadRuntimeSnapshot>,
    changed: Notify,
}

impl SnapshotCell {
    pub(super) fn new(state: ThreadRuntimeSnapshot) -> Self {
        Self {
            state: Mutex::new(state),
            changed: Notify::new(),
        }
    }

    /// Reads the state, or returns `None` when the lock is poisoned.
    pub(super) fn read<T>(&self, read: impl FnOnce(&ThreadRuntimeSnapshot) -> T) -> Option<T> {
        self.state.lock().ok().map(|state| read(&state))
    }

    /// Reads the state, or the state `poisoned` returns when the lock is poisoned.
    pub(super) fn read_or<T>(
        &self,
        poisoned: impl FnOnce() -> ThreadRuntimeSnapshot,
        read: impl FnOnce(&ThreadRuntimeSnapshot) -> T,
    ) -> T {
        match self.state.lock() {
            Ok(state) => read(&state),
            Err(_) => read(&poisoned()),
        }
    }

    /// Changes the state and wakes every waiter, or returns `None` when the lock is poisoned.
    pub(super) fn update<T>(
        &self,
        update: impl FnOnce(&mut ThreadRuntimeSnapshot) -> T,
    ) -> Option<T> {
        let result = self.state.lock().ok().map(|mut state| update(&mut state));
        self.changed.notify_waiters();
        result
    }

    /// Waits until `predicate` holds for the state, for at most `timeout`. Returns false when it
    /// does not hold in time or the lock is poisoned.
    pub(super) async fn wait_until(
        &self,
        predicate: impl Fn(&ThreadRuntimeSnapshot) -> bool,
        timeout: Duration,
    ) -> bool {
        let wait = async {
            loop {
                // The waiter registers before it checks, so a change between the check and the
                // wait still wakes it.
                let changed = self.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                match self.read(&predicate) {
                    Some(true) => return true,
                    Some(false) => changed.await,
                    None => return false,
                }
            }
        };
        tokio::time::timeout(timeout, wait).await.unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn a_waiter_sees_a_change_made_after_it_started_waiting() {
        let cell = Arc::new(SnapshotCell::new(ThreadRuntimeSnapshot::stopped()));
        let waiter = {
            let cell = Arc::clone(&cell);
            tokio::spawn(async move {
                cell.wait_until(|state| state.session_id.is_some(), Duration::from_secs(5))
                    .await
            })
        };
        tokio::task::yield_now().await;
        cell.update(|state| {
            state.session_id =
                Some(crate::models::ProviderSessionId::new("session-1".to_string()).unwrap());
        });
        assert!(waiter.await.unwrap());
    }

    #[tokio::test]
    async fn a_waiter_returns_at_once_when_the_state_already_holds_and_false_after_the_timeout() {
        let cell = SnapshotCell::new(ThreadRuntimeSnapshot::stopped());
        assert!(
            cell.wait_until(|state| state.session_id.is_none(), Duration::from_millis(1))
                .await
        );
        assert!(
            !cell
                .wait_until(
                    |state| state.session_id.is_some(),
                    Duration::from_millis(20)
                )
                .await
        );
    }
}
