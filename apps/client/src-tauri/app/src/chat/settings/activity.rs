//! Provider activity that turns report and the provider order and limits views read.

use crate::chat::device_state::{
    record_provider_usage_limits, record_provider_use, update_active_device_scope,
};
use ganbaru_chat_contracts::events::{CanonicalEvent, CanonicalRuntimeEvent, RateLimitStatusEvent};
use ganbaru_chat_contracts::models::{ProviderInstanceId, UtcTimestamp};
use tauri::Emitter;

/// Event that tells views a provider's stored limits or recent use changed on this device.
pub(crate) const CHAT_PROVIDER_ACTIVITY_EVENT: &str = "chat://provider-activity";

/// What a runtime event tells this device about its provider.
pub(crate) enum ProviderActivity {
    /// A turn started, which makes the provider the most recently used.
    Used(ProviderInstanceId),
    /// The provider reported its usage limits.
    UsageLimits(ProviderInstanceId, RateLimitStatusEvent),
}

impl ProviderActivity {
    /// Reads the provider activity a runtime event carries, if any.
    pub(crate) fn of(event: &CanonicalRuntimeEvent) -> Option<Self> {
        let instance_id = event.provider_instance_id.clone();
        match &event.event {
            CanonicalEvent::TurnStarted(_) => Some(Self::Used(instance_id)),
            CanonicalEvent::RateLimitStatus(status) => {
                Some(Self::UsageLimits(instance_id, status.clone()))
            }
            _ => None,
        }
    }
}

/// Stores provider activity in device state and tells views when the stored state changed.
///
/// Providers already report limits only when they change, and a turn start writes only when
/// it changes the most recent provider, so this writes rarely. Failures are logged because the
/// event was already ingested and the activity only orders and annotates provider views.
pub(crate) fn record_provider_activity(
    app: &tauri::AppHandle,
    activity: ProviderActivity,
    observed_at: UtcTimestamp,
) {
    let recorded = update_active_device_scope(app, |scope| {
        Ok(match activity {
            ProviderActivity::Used(instance_id) => {
                record_provider_use(scope, &instance_id, observed_at)
            }
            ProviderActivity::UsageLimits(instance_id, status) => {
                record_provider_usage_limits(scope, &instance_id, status, observed_at)
            }
        })
    });
    match recorded {
        Ok(true) => {
            if app.emit(CHAT_PROVIDER_ACTIVITY_EVENT, ()).is_err() {
                eprintln!("Chat provider activity notification failed");
            }
        }
        Ok(false) => {}
        Err(error) => eprintln!("Chat provider activity was not stored: {error}"),
    }
}
