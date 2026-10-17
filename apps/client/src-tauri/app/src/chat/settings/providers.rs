use crate::chat::device_state::ChatProviderDeviceState;
use ganbaru_chat_contracts::models::{
    ModelId, ProbeState, ProviderModelCatalog, ProviderProbeResult,
};
use ganbaru_chat_providers::{DriverCancellation, DriverOperationContext};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const PROVIDER_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn apply_provider_probe(
    device: &mut ChatProviderDeviceState,
    probe: &ProviderProbeResult,
    model_catalog: Option<ProviderModelCatalog>,
) {
    device.last_probe = Some(probe.clone());
    if probe.state == ProbeState::Healthy {
        device.last_successful_probe_at = Some(probe.checked_at.clone());
    }
    if let Some(catalog) = model_catalog {
        device.model_catalog = Some(catalog.without_deprecated_models());
    }
}

pub(crate) fn unique_model_ids(values: Vec<ModelId>) -> Vec<ModelId> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.as_str().to_string()))
        .collect()
}

pub(crate) fn operation_context(operation_id: &str) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline: Instant::now() + PROVIDER_OPERATION_TIMEOUT,
        cancellation: DriverCancellation::default(),
    }
}
