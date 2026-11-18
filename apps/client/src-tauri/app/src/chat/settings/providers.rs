use crate::chat::device_state::ChatProviderDeviceState;
use ganbaru_chat_contracts::models::{ProbeState, ProviderModelCatalog, ProviderProbeResult};
use ganbaru_chat_providers::{DriverCancellation, DriverOperationContext};
use std::time::{Duration, Instant};

const PROVIDER_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn apply_provider_probe(
    device: &mut ChatProviderDeviceState,
    probe: &ProviderProbeResult,
    model_catalog: Option<ProviderModelCatalog>,
) {
    let mut probe = probe.clone();
    if let Some(status) = probe.usage_limits.take() {
        device.observe_usage_limits(status, probe.checked_at.clone());
    }
    device.last_probe = Some(probe.clone());
    if probe.state == ProbeState::Healthy {
        device.last_successful_probe_at = Some(probe.checked_at.clone());
    }
    if let Some(catalog) = model_catalog {
        device.model_catalog = Some(catalog.without_deprecated_models());
    }
}

pub(crate) fn operation_context(operation_id: &str) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline: Instant::now() + PROVIDER_OPERATION_TIMEOUT,
        cancellation: DriverCancellation::default(),
    }
}
