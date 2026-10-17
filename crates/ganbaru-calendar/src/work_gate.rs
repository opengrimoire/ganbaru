//! Single-flight admission for bounded Calendar workers.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use tokio::sync::{AcquireError, OwnedSemaphorePermit, Semaphore, TryAcquireError};

/// Admits one worker at a time for one kind of Calendar work across the process.
///
/// Unit tests each own an isolated database, so every test thread gets its own gate instead: parallel tests do not
/// reject each other's work, while each test still observes single-flight admission.
pub struct WorkGate {
    semaphore: LazyLock<Arc<Semaphore>>,
}

impl WorkGate {
    /// Creates a gate with one permit.
    pub const fn new() -> Self {
        Self {
            semaphore: LazyLock::new(single_permit),
        }
    }

    /// Takes the permit without waiting, failing while another worker holds it.
    pub fn try_acquire(&self) -> Result<OwnedSemaphorePermit, TryAcquireError> {
        self.semaphore().try_acquire_owned()
    }

    /// Waits for the permit.
    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit, AcquireError> {
        self.semaphore().acquire_owned().await
    }

    /// Waits up to `limit` for the permit, returning `None` when it is still held.
    ///
    /// Callers must not hold a SQLite connection while waiting, and should take the permit only
    /// once their own SQLite work is done: the vault has one connection, so a holder that still
    /// needs it makes waiters' limits depend on unrelated database work. The limit keeps a stalled
    /// holder from blocking waiters forever.
    pub async fn acquire_within(&self, limit: Duration) -> Option<OwnedSemaphorePermit> {
        tokio::time::timeout(limit, self.semaphore().acquire_owned())
            .await
            .ok()?
            .ok()
    }

    #[cfg(not(test))]
    fn semaphore(&self) -> Arc<Semaphore> {
        Arc::clone(&self.semaphore)
    }

    #[cfg(test)]
    fn semaphore(&self) -> Arc<Semaphore> {
        use std::cell::RefCell;
        use std::collections::HashMap;

        thread_local! {
            static TEST_GATES: RefCell<HashMap<usize, Arc<Semaphore>>> = RefCell::default();
        }
        // Each gate's own allocation identifies it, since distinct statics may not have distinct addresses.
        let key = Arc::as_ptr(&self.semaphore).addr();
        TEST_GATES
            .with(|gates| Arc::clone(gates.borrow_mut().entry(key).or_insert_with(single_permit)))
    }
}

impl Default for WorkGate {
    fn default() -> Self {
        Self::new()
    }
}

fn single_permit() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(1))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::WorkGate;

    static FIRST: WorkGate = WorkGate::new();
    static SECOND: WorkGate = WorkGate::new();

    #[test]
    fn admits_one_worker_per_gate_and_keeps_gates_independent() {
        let held = FIRST.try_acquire().unwrap();
        assert!(FIRST.try_acquire().is_err());
        let other = SECOND.try_acquire().unwrap();
        drop(held);
        assert!(FIRST.try_acquire().is_ok());
        drop(other);
    }

    #[test]
    fn bounded_wait_admits_once_the_holder_releases() {
        crate::test_support::block_on(async {
            let held = FIRST.try_acquire().unwrap();
            let release = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                drop(held);
            });
            let permit = FIRST.acquire_within(Duration::from_secs(5)).await;
            assert!(permit.is_some());
            release.await.unwrap();
        });
    }

    #[test]
    fn bounded_wait_gives_up_while_the_holder_stalls() {
        crate::test_support::block_on(async {
            let held = FIRST.try_acquire().unwrap();
            assert!(
                FIRST
                    .acquire_within(Duration::from_millis(20))
                    .await
                    .is_none()
            );
            drop(held);
            assert!(
                FIRST
                    .acquire_within(Duration::from_millis(20))
                    .await
                    .is_some()
            );
        });
    }

    #[test]
    fn gives_each_test_thread_its_own_permit() {
        let held = FIRST.try_acquire().unwrap();
        std::thread::spawn(|| assert!(FIRST.try_acquire().is_ok()))
            .join()
            .unwrap();
        drop(held);
    }
}
