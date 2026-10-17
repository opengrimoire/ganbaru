//! Serialized platform worker policy with injectable delivery for failure tests.

use ganbaru_pomodoro::FocusExecutionError;
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::sync::watch;

use super::super::effects;
use super::{DeliveryStatus, DesiredDelivery};

pub(super) trait DeliveryBackend: Send + 'static {
    fn apply(
        &mut self,
        desired: &DesiredDelivery,
    ) -> impl Future<Output = Result<(), FocusExecutionError>> + Send;
    fn revoke(&mut self) -> impl Future<Output = Result<(), FocusExecutionError>> + Send;
    fn invalidate_preferences(&mut self);
    fn next_deadline(&self) -> Option<i64>;
}

pub(super) struct NativeDelivery {
    app: tauri::AppHandle,
    effects: effects::FocusEffects,
}

impl NativeDelivery {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            effects: effects::FocusEffects::default(),
        }
    }
}

impl DeliveryBackend for NativeDelivery {
    async fn apply(&mut self, desired: &DesiredDelivery) -> Result<(), FocusExecutionError> {
        let accepted = desired
            .accepted
            .as_ref()
            .ok_or_else(|| "Native Focus delivery has no accepted projection".to_owned())?;
        let mut projection = accepted.projection.clone();
        if let Some(closed) = desired.closed_phase.as_ref().filter(|closed| {
            Instant::now().saturating_duration_since(closed.captured_at)
                < Duration::from_millis(super::super::EFFECT_LEASE_MS as u64)
        }) && let Some(snapshot) = &mut projection.snapshot
            && !snapshot
                .changed_segments
                .iter()
                .any(|segment| segment.id == closed.segment.id)
        {
            snapshot.changed_segments.push(closed.segment.clone());
        }
        #[cfg(not(target_os = "ios"))]
        if let Some(copy) = &desired.copy {
            self.effects
                .configure_notification_copy(&self.app, copy.clone())
                .await?;
        }
        self.effects
            .apply(
                &self.app,
                &projection,
                super::super::now_ms()?,
                super::super::EFFECT_LEASE_MS,
                accepted.ownership_generation,
                accepted.pool.as_ref(),
            )
            .await
    }

    async fn revoke(&mut self) -> Result<(), FocusExecutionError> {
        self.effects.revoke(&self.app).await
    }
    fn invalidate_preferences(&mut self) {
        self.effects.invalidate_preferences();
    }
    fn next_deadline(&self) -> Option<i64> {
        self.effects.next_presentation_deadline()
    }
}

pub(super) async fn run(
    mut backend: impl DeliveryBackend,
    mut input: watch::Receiver<DesiredDelivery>,
    feedback: watch::Sender<DeliveryStatus>,
) {
    let mut retry = effects::DeliveryRetry::default();
    let mut preferences_revision = 0;
    let mut wake = Instant::now();
    loop {
        tokio::select! {
            biased;
            changed = input.changed() => { if changed.is_err() { break; } }
            _ = tokio::time::sleep_until(wake.into()) => {}
        }
        let desired = input.borrow_and_update().clone();
        if desired.preferences_revision != preferences_revision {
            backend.invalidate_preferences();
            preferences_revision = desired.preferences_revision;
            retry.clear();
        }
        let Some(accepted) = &desired.accepted else {
            let result = backend.revoke().await;
            feedback.send_replace(DeliveryStatus {
                sequence: desired.sequence,
                preferences_revision,
                quiescent: result.is_ok(),
                message: result.as_ref().err().map(ToString::to_string),
                ..DeliveryStatus::default()
            });
            retry.clear();
            wake = Instant::now()
                + if result.is_ok() {
                    Duration::from_secs(60 * 60)
                } else {
                    Duration::from_millis(super::super::ERROR_RETRY_INTERVAL_MS as u64)
                };
            continue;
        };
        let projection = &accepted.projection;
        let Some(snapshot) = &projection.snapshot else {
            wake = Instant::now()
                + Duration::from_millis(super::super::ERROR_RETRY_INTERVAL_MS as u64);
            continue;
        };
        let generation = projection.vault_generation;
        let revision = snapshot.revision;
        if retry.ready(generation, revision, Instant::now()) {
            feedback.send_replace(DeliveryStatus {
                sequence: desired.sequence,
                generation: Some(generation),
                revision: Some(revision),
                preferences_revision,
                started_at: Some(Instant::now()),
                ..DeliveryStatus::default()
            });
            match backend.apply(&desired).await {
                Ok(()) => retry.clear(),
                Err(error) => retry.failed(generation, revision, Instant::now(), error.to_string()),
            }
            feedback.send_replace(DeliveryStatus {
                sequence: desired.sequence,
                generation: Some(generation),
                revision: Some(revision),
                preferences_revision,
                quiescent: false,
                message: retry.message().map(str::to_owned),
                completed_phase: retry
                    .message()
                    .is_none()
                    .then(|| {
                        desired
                            .closed_phase
                            .as_ref()
                            .map(|closed| closed.segment.id.clone())
                    })
                    .flatten(),
                started_at: None,
            });
        }
        let alert = super::super::now_ms().ok().and_then(|now| {
            backend
                .next_deadline()
                .map(|deadline| Duration::from_millis(deadline.saturating_sub(now).max(0) as u64))
        });
        let delay = retry.next_delay(
            Duration::from_millis(super::super::ERROR_RETRY_INTERVAL_MS as u64),
            alert,
            Instant::now(),
        );
        wake = Instant::now() + delay;
    }
}
