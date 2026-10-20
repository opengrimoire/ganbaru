use std::future::Future;
use std::sync::Arc;
use tokio::sync::watch;

/// Shared stop signal for one Notion API import.
///
/// Clones observe the same signal. Canceling wakes pending request pacing, retry
/// delays, and in-flight requests, so an import stops promptly instead of after
/// its current wait finishes.
#[derive(Clone, Debug)]
pub struct NotionImportCancellation {
    sender: Arc<watch::Sender<bool>>,
}

impl Default for NotionImportCancellation {
    fn default() -> Self {
        Self::new()
    }
}

impl NotionImportCancellation {
    pub fn new() -> Self {
        let (sender, _) = watch::channel(false);
        Self {
            sender: Arc::new(sender),
        }
    }

    /// Requests that the import stop at its next request boundary or wait.
    pub fn cancel(&self) {
        self.sender.send_replace(true);
    }

    pub fn is_canceled(&self) -> bool {
        *self.sender.borrow()
    }

    /// Resolves once the import is canceled; resolves immediately when it already is.
    async fn canceled(&self) {
        let mut receiver = self.sender.subscribe();
        if receiver.wait_for(|canceled| *canceled).await.is_err() {
            std::future::pending::<()>().await;
        }
    }

    /// Runs `future` unless the import is canceled first; `None` means canceled.
    pub(crate) async fn unless_canceled<T>(&self, future: impl Future<Output = T>) -> Option<T> {
        if self.is_canceled() {
            return None;
        }
        tokio::select! {
            biased;
            () = self.canceled() => None,
            output = future => Some(output),
        }
    }
}

/// Why a Notion API import stopped before finishing every requested source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotionImportStop {
    Canceled,
    RequestLimit,
    BlockLimit,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn cancel_wakes_a_pending_wait() {
        sqlx::__rt::test_block_on(async {
            let cancellation = NotionImportCancellation::new();
            let canceler = cancellation.clone();
            let waiter = tokio::spawn(async move {
                cancellation
                    .unless_canceled(tokio::time::sleep(Duration::from_secs(60)))
                    .await
            });
            tokio::task::yield_now().await;
            canceler.cancel();
            let outcome = tokio::time::timeout(Duration::from_secs(5), waiter)
                .await
                .expect("cancel must wake the wait")
                .unwrap();
            assert_eq!(outcome, None);
        });
    }

    #[test]
    fn canceled_imports_skip_new_work() {
        sqlx::__rt::test_block_on(async {
            let cancellation = NotionImportCancellation::new();
            assert_eq!(cancellation.unless_canceled(async { 1 }).await, Some(1));
            cancellation.cancel();
            assert!(cancellation.is_canceled());
            assert_eq!(cancellation.unless_canceled(async { 2 }).await, None);
        });
    }
}
