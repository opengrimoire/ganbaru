use crate::models::*;
use crate::{DriverFuture, DriverOperationContext, ProviderDriver, ProviderEventSink};
use std::sync::Arc;

pub struct UnsupportedProviderDriver {
    metadata: ProviderFamilyMetadataRead,
    configuration: ProviderInstanceConfig,
}

impl UnsupportedProviderDriver {
    pub fn new(
        metadata: ProviderFamilyMetadataRead,
        configuration: ProviderInstanceConfig,
    ) -> Self {
        Self {
            metadata,
            configuration,
        }
    }

    fn unavailable<'a, T>(&self, operation: &'static str) -> DriverFuture<'a, T> {
        let family_id = self.configuration.family_id.to_string();
        Box::pin(async move {
            Err(ChatError::driver_unavailable(format!(
                "provider '{family_id}' cannot perform {operation} because its driver is unavailable"
            )))
        })
    }
}

impl ProviderDriver for UnsupportedProviderDriver {
    fn metadata(&self) -> ProviderFamilyMetadataRead {
        self.metadata.clone()
    }

    fn instance_configuration(&self) -> &ProviderInstanceConfig {
        &self.configuration
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            entries: self
                .metadata
                .potential_capabilities
                .iter()
                .copied()
                .map(|capability| ProviderCapabilitySupport {
                    capability,
                    supported: false,
                    explanation: Some(
                        "The provider driver has not been implemented yet.".to_string(),
                    ),
                })
                .collect(),
        }
    }

    fn probe<'a>(
        &'a mut self,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderProbeResult> {
        self.unavailable("probe")
    }

    fn discover_models<'a>(
        &'a mut self,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderModelCatalog> {
        self.unavailable("model discovery")
    }

    fn derive_continuation_group<'a>(
        &'a mut self,
        _request: ContinuationGroupRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ContinuationGroupId> {
        self.unavailable("continuation-group derivation")
    }

    fn start_session<'a>(
        &'a mut self,
        _request: StartSessionRequest,
        _event_sink: Arc<dyn ProviderEventSink>,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot> {
        self.unavailable("session start")
    }

    fn resume_session<'a>(
        &'a mut self,
        _request: ResumeSessionRequest,
        _event_sink: Arc<dyn ProviderEventSink>,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot> {
        self.unavailable("session resume")
    }

    fn send_turn<'a>(
        &'a mut self,
        _request: SendTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, TurnDispatchReceipt> {
        self.unavailable("turn send")
    }

    fn steer_turn<'a>(
        &'a mut self,
        _request: SteerTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        self.unavailable("turn steering")
    }

    fn interrupt_turn<'a>(
        &'a mut self,
        _request: InterruptTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        self.unavailable("turn interruption")
    }

    fn resolve_approval<'a>(
        &'a mut self,
        _request: ResolveApprovalRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        self.unavailable("approval resolution")
    }

    fn resolve_user_input<'a>(
        &'a mut self,
        _request: ResolveUserInputRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        self.unavailable("user-input resolution")
    }

    fn rollback<'a>(
        &'a mut self,
        _request: RollbackRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        self.unavailable("rollback")
    }

    fn read_history<'a>(
        &'a mut self,
        _request: ReadHistoryRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderHistoryPage> {
        self.unavailable("history read")
    }

    fn stop_session<'a>(
        &'a mut self,
        _request: StopSessionRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        self.unavailable("session stop")
    }
}
