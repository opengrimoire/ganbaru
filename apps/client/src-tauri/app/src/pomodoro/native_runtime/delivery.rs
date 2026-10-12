//! One retained delivery request and one worker, independently of semantic execution.

use std::time::{Duration, Instant};

use ganbaru_pomodoro::{FocusErrorCode, FocusExecutionError, FocusMode, FocusSegmentSnapshot};
use tokio::sync::watch;

use super::{FocusProjection, error};

mod worker;

const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct AcceptedDelivery {
    projection: FocusProjection,
    ownership_generation: Option<u64>,
    pool: Option<sqlx::SqlitePool>,
}

#[derive(Clone, Default)]
struct DesiredDelivery {
    sequence: u64,
    preferences_revision: u64,
    accepted: Option<AcceptedDelivery>,
    closed_phase: Option<ClosedPhaseEvidence>,
    #[cfg(not(target_os = "ios"))]
    copy: Option<super::NotificationCopy>,
}

#[derive(Clone)]
struct ClosedPhaseEvidence {
    segment: FocusSegmentSnapshot,
    captured_at: Instant,
}

#[derive(Clone, Default)]
pub(super) struct DeliveryStatus {
    sequence: u64,
    generation: Option<u64>,
    revision: Option<i64>,
    preferences_revision: u64,
    quiescent: bool,
    message: Option<String>,
    completed_phase: Option<String>,
    started_at: Option<Instant>,
}

/// The handle never spawns replacement work or accumulates a delivery queue.
pub(super) struct DeliveryHandle {
    desired: watch::Sender<DesiredDelivery>,
    pub feedback: watch::Receiver<DeliveryStatus>,
}

impl DeliveryHandle {
    pub fn start(app: &tauri::AppHandle) -> Self {
        let (desired, input) = watch::channel(DesiredDelivery::default());
        let (feedback, status) = watch::channel(DeliveryStatus::default());
        tauri::async_runtime::spawn(worker::run(
            worker::NativeDelivery::new(app.clone()),
            input,
            feedback,
        ));
        Self {
            desired,
            feedback: status,
        }
    }

    pub fn publish(
        &self,
        projection: &FocusProjection,
        ownership_generation: Option<u64>,
        pool: Option<&sqlx::SqlitePool>,
    ) -> Result<(), FocusExecutionError> {
        self.require_worker()?;
        let completed = self.feedback.borrow().clone();
        self.desired.send_modify(|desired| {
            retain_closed_phase(desired, projection, &completed, Instant::now());
            desired.sequence = desired.sequence.saturating_add(1);
            desired.accepted = Some(AcceptedDelivery {
                projection: projection.clone(),
                ownership_generation,
                pool: pool.cloned(),
            });
        });
        Ok(())
    }

    pub fn invalidate_preferences(&self) {
        self.desired.send_modify(|desired| {
            desired.preferences_revision = desired.preferences_revision.saturating_add(1);
        });
    }

    #[cfg(not(target_os = "ios"))]
    pub fn configure_copy(&self, copy: super::NotificationCopy) -> Result<(), FocusExecutionError> {
        copy.validate()?;
        self.require_worker()?;
        self.desired.send_modify(|desired| {
            desired.copy = Some(copy);
            desired.preferences_revision = desired.preferences_revision.saturating_add(1);
        });
        Ok(())
    }

    /// A timeout stops the handoff; it never cancels a blocking platform operation.
    pub async fn freeze(&mut self) -> Result<(), FocusExecutionError> {
        self.freeze_with_timeout(QUIESCENCE_TIMEOUT).await
    }

    async fn freeze_with_timeout(&mut self, timeout: Duration) -> Result<(), FocusExecutionError> {
        let sequence = self.begin_freeze()?;
        let wait = async {
            loop {
                if self.quiescent(sequence) {
                    return Ok(());
                }
                let status = self.feedback.borrow().clone();
                if status.sequence == sequence
                    && let Some(message) = status.message
                {
                    return Err(error(FocusErrorCode::Unavailable, message));
                }
                self.feedback.changed().await.map_err(|_| {
                    error(
                        FocusErrorCode::Unavailable,
                        "Native Focus delivery worker stopped",
                    )
                })?;
            }
        };
        tokio::time::timeout(timeout, wait).await.map_err(|_| {
            error(
                FocusErrorCode::Busy,
                "Native Focus platform delivery has not quiesced",
            )
        })?
    }

    /// Request revocation without blocking semantic resume processing.
    pub fn begin_freeze(&self) -> Result<u64, FocusExecutionError> {
        self.require_worker()?;
        self.desired.send_if_modified(|desired| {
            if desired.accepted.is_none() {
                return false;
            }
            desired.sequence = desired.sequence.saturating_add(1);
            desired.accepted = None;
            desired.closed_phase = None;
            true
        });
        Ok(self.desired.borrow().sequence)
    }

    pub fn is_quiescent(&self) -> bool {
        self.desired.borrow().accepted.is_none() && self.quiescent(self.desired.borrow().sequence)
    }

    fn quiescent(&self, sequence: u64) -> bool {
        let status = self.feedback.borrow();
        status.sequence == sequence && status.quiescent
    }

    pub fn message(&self, projection: &FocusProjection) -> Option<String> {
        if self.desired.is_closed() {
            return Some("Native Focus delivery worker stopped".into());
        }
        let desired = self.desired.borrow();
        let status = self.feedback.borrow();
        if desired.accepted.is_some()
            && status.started_at.is_some_and(|started| {
                started.elapsed() >= Duration::from_millis(super::EFFECT_LEASE_MS as u64)
            })
        {
            return Some("Native Focus platform delivery is still pending".into());
        }
        (status.generation == Some(projection.vault_generation)
            && status.revision
                == projection
                    .snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.revision)
            && status.preferences_revision == desired.preferences_revision)
            .then(|| status.message.clone())
            .flatten()
    }

    fn require_worker(&self) -> Result<(), FocusExecutionError> {
        if self.desired.is_closed() {
            Err(error(
                FocusErrorCode::Unavailable,
                "Native Focus delivery worker stopped",
            ))
        } else {
            Ok(())
        }
    }
}

/// Preserve one recent canonical closure through heartbeat coalescing, never history.
fn retain_closed_phase(
    desired: &mut DesiredDelivery,
    projection: &FocusProjection,
    completed: &DeliveryStatus,
    now: Instant,
) {
    let compatible = projection.snapshot.as_ref().is_some_and(|snapshot| {
        matches!(
            snapshot.mode,
            FocusMode::Running | FocusMode::ReturnWait | FocusMode::Expired
        ) && snapshot.run.is_some()
            && desired.accepted.as_ref().is_some_and(|previous| {
                previous.projection.vault_generation == projection.vault_generation
                    && previous
                        .projection
                        .snapshot
                        .as_ref()
                        .and_then(|previous| previous.run.as_ref())
                        .map(|run| &run.id)
                        == snapshot.run.as_ref().map(|run| &run.id)
            })
    });
    if !compatible {
        desired.closed_phase = None;
        return;
    }
    if desired.closed_phase.as_ref().is_some_and(|closed| {
        now.saturating_duration_since(closed.captured_at)
            >= Duration::from_millis(super::EFFECT_LEASE_MS as u64)
            || (completed.generation == Some(projection.vault_generation)
                && completed.completed_phase.as_deref() == Some(closed.segment.id.as_str()))
    }) {
        desired.closed_phase = None;
    }
    if desired.closed_phase.is_some() {
        return;
    }
    let previous = desired
        .accepted
        .as_ref()
        .and_then(|accepted| accepted.projection.snapshot.as_ref())
        .and_then(|snapshot| snapshot.segment.as_ref());
    let Some(previous) = previous.filter(|segment| segment.actual_end_ms.is_none()) else {
        return;
    };
    let Some(snapshot) = &projection.snapshot else {
        return;
    };
    if let Some(segment) = snapshot
        .changed_segments
        .iter()
        .chain(snapshot.segment.iter())
        .find(|segment| {
            segment.id == previous.id
                && segment.run_id == previous.run_id
                && segment
                    .actual_end_ms
                    .is_some_and(|ended| ended > segment.actual_start_ms)
        })
    {
        desired.closed_phase = Some(ClosedPhaseEvidence {
            segment: segment.clone(),
            captured_at: now,
        });
    }
}

#[cfg(test)]
mod tests;
