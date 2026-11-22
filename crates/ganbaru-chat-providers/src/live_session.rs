//! Lifecycle of one live provider session, shared by the drivers that own a provider process.
//!
//! A live session runs until the driver stops it or the session fails. The failure is a single
//! transition: whichever task fails the session first records why, every later command reports
//! that error, and a stop that is already under way is never turned into a failure.

use crate::models::ChatError;
use crate::models::ChatResult;
use crate::process::ProviderProcessHandle;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::oneshot;

/// Where a live session is in its lifecycle.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LiveLifecycle {
    Running,
    /// The driver is stopping the session, so the provider going away is expected.
    Stopping,
    /// The session failed with this error and accepts no further work.
    Failed(ChatError),
}

/// What happened to a live session.
#[derive(Clone, Debug)]
pub(crate) enum LifecycleEvent {
    /// The driver began stopping the session.
    Stop,
    /// A session-fatal error ended the session.
    Fail(ChatError),
}

impl LiveLifecycle {
    /// Applies `event` and returns whether it changed the lifecycle.
    ///
    /// A stop always wins: it ends a running or failed session, and a failure that arrives
    /// while stopping is the stop's own consequence, not a new failure. The first failure is
    /// kept, so a later one cannot replace why the session ended.
    pub(crate) fn transition(&mut self, event: LifecycleEvent) -> bool {
        match (&*self, event) {
            (Self::Running | Self::Failed(_), LifecycleEvent::Stop) => {
                *self = Self::Stopping;
                true
            }
            (Self::Running, LifecycleEvent::Fail(error)) => {
                *self = Self::Failed(error);
                true
            }
            (Self::Stopping, _) | (Self::Failed(_), LifecycleEvent::Fail(_)) => false,
        }
    }
}

/// The lifecycle of one live session, shared by the driver and the tasks that serve it.
#[derive(Clone)]
pub(crate) struct LiveSessionLifecycle {
    state: Arc<Mutex<LiveLifecycle>>,
}

impl Default for LiveSessionLifecycle {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(LiveLifecycle::Running)),
        }
    }
}

impl LiveSessionLifecycle {
    /// Marks the session as stopping. Returns false when it was already stopping.
    pub(crate) fn begin_stop(&self) -> bool {
        self.lock().transition(LifecycleEvent::Stop)
    }

    /// Fails the session with `error`. Returns true only for the call that failed it, which
    /// then owns reporting the failure and stopping the provider.
    pub(crate) fn fail(&self, error: ChatError) -> bool {
        self.lock().transition(LifecycleEvent::Fail(error))
    }

    /// Replaces the recorded failure with `error`, for when reporting the failure itself
    /// failed and the timeline never learned why the session ended.
    pub(crate) fn replace_failure(&self, error: ChatError) {
        let mut state = self.lock();
        if let LiveLifecycle::Failed(kept) = &mut *state {
            *kept = error;
        }
    }

    /// The error the session failed with, when it failed.
    pub(crate) fn failure(&self) -> Option<ChatError> {
        match &*self.lock() {
            LiveLifecycle::Failed(error) => Some(error.clone()),
            LiveLifecycle::Running | LiveLifecycle::Stopping => None,
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        matches!(&*self.lock(), LiveLifecycle::Running)
    }

    pub(crate) fn is_stopping(&self) -> bool {
        matches!(&*self.lock(), LiveLifecycle::Stopping)
    }

    /// The lifecycle is one value that every transition leaves whole, so a poisoned lock is
    /// still used.
    fn lock(&self) -> MutexGuard<'_, LiveLifecycle> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The one-shot signal that tells a connection's writer to close the provider's input.
#[derive(Clone)]
pub(crate) struct WriterShutdown(Arc<Mutex<Option<oneshot::Sender<()>>>>);

impl WriterShutdown {
    pub(crate) fn new(sender: oneshot::Sender<()>) -> Self {
        Self(Arc::new(Mutex::new(Some(sender))))
    }

    /// Signals the writer once; later calls do nothing.
    pub(crate) fn signal(&self) {
        let sender = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(sender) = sender {
            let _ = sender.send(());
        }
    }
}

/// The provider process a connection owns, shared with the handle that may stop it early.
#[derive(Clone)]
pub(crate) struct ProcessSlot(Arc<Mutex<Option<ProviderProcessHandle>>>);

impl ProcessSlot {
    pub(crate) fn new(process: Option<ProviderProcessHandle>) -> Self {
        Self(Arc::new(Mutex::new(process)))
    }

    /// Reads the process while the slot still holds it.
    pub(crate) fn with<T>(&self, read: impl FnOnce(&ProviderProcessHandle) -> T) -> Option<T> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(read)
    }

    /// Takes the process out of the slot and stops it, so no lock is held while it stops.
    /// Whichever caller runs first stops the process; the others find the slot empty. A
    /// poisoned slot is still emptied, because the process must stop either way.
    pub(crate) async fn stop(
        &self,
        graceful_deadline: Duration,
        force_deadline: Duration,
    ) -> ChatResult<()> {
        let process = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        match process {
            Some(mut process) => process.stop(graceful_deadline, force_deadline).await,
            None => Ok(()),
        }
    }
}

/// Stops a connection's provider from a task that does not own the connection.
///
/// The router holds one so a session-fatal error ends the provider without waiting for the
/// driver. It races the connection's own stop safely through the shared [`ProcessSlot`].
#[derive(Clone)]
pub struct ConnectionKill {
    writer_shutdown: WriterShutdown,
    process: ProcessSlot,
}

impl ConnectionKill {
    pub(crate) fn new(writer_shutdown: WriterShutdown, process: ProcessSlot) -> Self {
        Self {
            writer_shutdown,
            process,
        }
    }

    /// Closes the writer and force stops the process without a graceful wait.
    pub async fn kill(&self, force_deadline: Duration) -> ChatResult<()> {
        self.writer_shutdown.signal();
        self.process.stop(Duration::ZERO, force_deadline).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ChatErrorCode;

    fn error(message: &str) -> ChatError {
        ChatError::new(ChatErrorCode::TransportUnavailable, message, true)
    }

    #[test]
    fn the_first_failure_is_kept() {
        let lifecycle = LiveSessionLifecycle::default();

        assert!(lifecycle.fail(error("first")));
        assert!(!lifecycle.fail(error("second")));
        assert_eq!(lifecycle.failure().unwrap().message, "first");
        assert!(!lifecycle.is_running());
    }

    #[test]
    fn a_failure_while_stopping_is_not_a_failure() {
        let lifecycle = LiveSessionLifecycle::default();

        assert!(lifecycle.begin_stop());
        assert!(!lifecycle.fail(error("closed")));
        assert!(!lifecycle.begin_stop());
        assert!(lifecycle.is_stopping());
        assert_eq!(lifecycle.failure(), None);
    }

    #[test]
    fn stopping_a_failed_session_ends_it() {
        let lifecycle = LiveSessionLifecycle::default();
        lifecycle.fail(error("closed"));

        assert!(lifecycle.begin_stop());
        assert!(lifecycle.is_stopping());
        assert_eq!(lifecycle.failure(), None);
    }

    #[test]
    fn a_replaced_failure_applies_only_to_a_failed_session() {
        let lifecycle = LiveSessionLifecycle::default();
        lifecycle.replace_failure(error("ignored"));
        assert!(lifecycle.is_running());

        lifecycle.fail(error("closed"));
        lifecycle.replace_failure(error("emit failed"));
        assert_eq!(lifecycle.failure().unwrap().message, "emit failed");
    }
}
