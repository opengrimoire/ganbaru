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

pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = ChatResult<T>> + Send + 'a>>;

#[derive(Clone, Debug, Default)]
pub struct DriverCancellation {
    cancelled: Arc<AtomicBool>,
}

impl DriverCancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
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

pub trait ProviderDriver: Send {
    fn metadata(&self) -> ProviderFamilyMetadataRead;

    fn instance_configuration(&self) -> &ProviderInstanceConfig;

    fn capabilities(&self) -> ProviderCapabilities;

    fn authority_support(&self) -> ProviderAuthoritySupport {
        ProviderAuthoritySupport::default()
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
