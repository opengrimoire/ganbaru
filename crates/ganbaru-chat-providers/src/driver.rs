use crate::{events::CanonicalRuntimeEvent, models::*};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tokio::sync::Notify;

pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = ChatResult<T>> + Send + 'a>>;

/// Shared cancellation flag for one driver operation.
///
/// Cloned handles observe the same flag. Synchronous callers check `is_cancelled`, and
/// asynchronous callers await `cancelled`, which wakes as soon as `cancel` runs.
#[derive(Clone, Debug, Default)]
pub struct DriverCancellation {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl DriverCancellation {
    /// Marks the operation cancelled and wakes every task awaiting `cancelled`.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    /// Reports whether `cancel` has run on any clone of this handle.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Completes once the operation is cancelled, at once when it already is.
    ///
    /// Interest is registered before the flag is checked again, so a `cancel` that runs
    /// between the first check and the await still wakes this task.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

#[derive(Clone, Debug)]
pub struct DriverOperationContext {
    pub operation_id: String,
    pub deadline: Instant,
    pub cancellation: DriverCancellation,
}

impl DriverOperationContext {
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled() || Instant::now() >= self.deadline
    }
}

pub trait ProviderEventSink: Send + Sync {
    fn emit<'a>(&'a self, event: CanonicalRuntimeEvent) -> DriverFuture<'a, ()>;

    fn flush(&self) -> DriverFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

/// One resolved provider CLI invocation outside a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCliCommand {
    pub executable: std::path::PathBuf,
    /// Launcher prefix arguments followed by the requested ones.
    pub arguments: Vec<String>,
    pub environment: std::collections::BTreeMap<String, String>,
}

impl ProviderCliCommand {
    /// An invocation of `executable` with `prefix` and then `arguments`.
    pub(crate) fn new(
        executable: std::path::PathBuf,
        prefix: &[String],
        arguments: &[&str],
        environment: std::collections::BTreeMap<String, String>,
    ) -> Self {
        Self {
            executable,
            arguments: prefix
                .iter()
                .cloned()
                .chain(arguments.iter().map(|argument| (*argument).to_string()))
                .collect(),
            environment,
        }
    }
}

pub trait ProviderDriver: Send {
    fn metadata(&self) -> ProviderFamilyMetadataRead;

    fn instance_configuration(&self) -> &ProviderInstanceConfig;

    /// The provider CLI with `arguments` after any launcher prefix, resolved with the same
    /// executable search and environment as the driver's own processes, for a maintenance
    /// command outside a session such as an update.
    ///
    /// # Errors
    ///
    /// Fails as a probe would when the program is missing or the configuration is invalid, and
    /// with `Unsupported` for a driver that runs no maintenance commands.
    fn cli_command(&self, arguments: &[&str]) -> ChatResult<ProviderCliCommand> {
        let _ = arguments;
        Err(ChatError::unsupported(
            "This provider runs no maintenance commands",
        ))
    }

    fn capabilities(&self) -> ProviderCapabilities;

    fn authority_support(&self) -> ProviderAuthoritySupport {
        ProviderAuthoritySupport::default()
    }

    /// How this provider honors a restricted project configuration. A driver
    /// overrides the default only after verifying its switch with the pinned
    /// provider version.
    fn project_configuration_restriction(&self) -> ProjectConfigurationRestriction {
        ProjectConfigurationRestriction::Unavailable
    }

    /// Project provider configuration paths this provider loads at start. A
    /// path past the digest bounds refuses only the providers that list it,
    /// so a driver narrows the default only to paths it never loads.
    fn project_configuration_paths(&self) -> &'static [ProviderConfigurationPath] {
        ProviderConfigurationPath::ALL
    }

    fn cached_model_catalog(&self) -> Option<ProviderModelCatalog> {
        None
    }

    fn prompt_catalog(&self) -> ChatResult<Vec<ChatPromptCatalogEntry>> {
        Ok(Vec::new())
    }

    fn compact_context<'a>(
        &'a mut self,
        _request: CompactContextRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async {
            Err(ChatError::unsupported(
                "This provider does not expose direct context compaction",
            ))
        })
    }

    fn read_mcp_status<'a>(
        &'a mut self,
        _request: McpStatusRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, McpStatusRead> {
        Box::pin(async {
            Err(ChatError::unsupported(
                "This provider does not expose direct MCP status",
            ))
        })
    }

    fn probe<'a>(
        &'a mut self,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderProbeResult>;

    fn discover_models<'a>(
        &'a mut self,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderModelCatalog>;

    fn derive_continuation_group<'a>(
        &'a mut self,
        request: ContinuationGroupRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ContinuationGroupId>;

    fn start_session<'a>(
        &'a mut self,
        request: StartSessionRequest,
        event_sink: Arc<dyn ProviderEventSink>,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot>;

    fn resume_session<'a>(
        &'a mut self,
        request: ResumeSessionRequest,
        event_sink: Arc<dyn ProviderEventSink>,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot>;

    fn fork_thread<'a>(
        &'a mut self,
        _request: ProviderForkThreadRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderThreadId> {
        Box::pin(async {
            Err(ChatError::unsupported(
                "This provider does not expose native thread forking",
            ))
        })
    }

    fn rename_thread<'a>(
        &'a mut self,
        _request: ProviderThreadLifecycleRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async {
            Err(ChatError::unsupported(
                "This provider does not expose native thread renaming",
            ))
        })
    }

    fn archive_thread<'a>(
        &'a mut self,
        _request: ProviderThreadLifecycleRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async {
            Err(ChatError::unsupported(
                "This provider does not expose native thread archiving",
            ))
        })
    }

    fn delete_thread<'a>(
        &'a mut self,
        _request: ProviderThreadLifecycleRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async {
            Err(ChatError::unsupported(
                "This provider does not expose native thread deletion",
            ))
        })
    }

    fn unsubscribe_thread<'a>(
        &'a mut self,
        _request: ProviderThreadLifecycleRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async {
            Err(ChatError::unsupported(
                "This provider does not expose native thread unsubscribe",
            ))
        })
    }

    fn cleanup_thread<'a>(
        &'a mut self,
        request: ProviderThreadLifecycleRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        self.unsubscribe_thread(request, context)
    }

    fn send_turn<'a>(
        &'a mut self,
        request: SendTurnRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, TurnDispatchReceipt>;

    fn steer_turn<'a>(
        &'a mut self,
        request: SteerTurnRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt>;

    fn interrupt_turn<'a>(
        &'a mut self,
        request: InterruptTurnRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt>;

    fn resolve_approval<'a>(
        &'a mut self,
        request: ResolveApprovalRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt>;

    fn resolve_user_input<'a>(
        &'a mut self,
        request: ResolveUserInputRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt>;

    fn rollback<'a>(
        &'a mut self,
        request: RollbackRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt>;

    fn read_history<'a>(
        &'a mut self,
        request: ReadHistoryRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderHistoryPage>;

    fn stop_session<'a>(
        &'a mut self,
        request: StopSessionRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt>;
}

pub trait ProviderDriverFactory: Send + Sync {
    fn create_driver(
        &self,
        configuration: ProviderInstanceConfig,
    ) -> ChatResult<Box<dyn ProviderDriver>>;
}

#[cfg(test)]
mod tests {
    use super::DriverCancellation;
    use std::time::Duration;

    #[test]
    fn cancelled_wakes_after_cancel() {
        crate::test_block_on(async {
            let cancellation = DriverCancellation::default();
            let waiter = cancellation.clone();
            let task = tokio::spawn(async move { waiter.cancelled().await });
            tokio::task::yield_now().await;
            assert!(!task.is_finished(), "an uncancelled wait stays pending");
            cancellation.cancel();
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .expect("cancel wakes the waiter")
                .unwrap();
            assert!(cancellation.is_cancelled());
        });
    }

    #[test]
    fn cancelled_returns_when_already_cancelled() {
        crate::test_block_on(async {
            let cancellation = DriverCancellation::default();
            cancellation.cancel();
            tokio::time::timeout(Duration::from_secs(5), cancellation.clone().cancelled())
                .await
                .expect("an earlier cancel completes the wait at once");
            tokio::time::timeout(Duration::from_secs(5), cancellation.cancelled())
                .await
                .expect("every later wait completes at once");
        });
    }
}
