use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{mpsc, oneshot};

fn projection(generation: u64, revision: i64) -> FocusProjection {
    FocusProjection {
        vault_id: Some("vault".into()),
        vault_generation: generation,
        error: None,
        snapshot: Some(
            serde_json::from_value(serde_json::json!({
            "revision": revision, "observedAtMs": 1000, "mode": "stopped",
                "changedSegments": [], "remainingMs": 0, "elapsedMs": 0,
                "completedFocusCount": 0, "skipNextBreak": false,
                "focusExtensionUsed": false, "breakExtensionMs": 0,
                "automaticAdmissionSuppressed": false, "pausedPromptsDismissed": false,
                "activitySourceUnavailable": false
            }))
            .unwrap(),
        ),
    }
}

struct TestBackend {
    started: mpsc::Sender<i64>,
    release: Option<oneshot::Receiver<()>>,
    calls: Arc<AtomicUsize>,
    fail: bool,
}

impl worker::DeliveryBackend for TestBackend {
    async fn apply(&mut self, desired: &DesiredDelivery) -> Result<(), FocusExecutionError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let revision = desired
            .accepted
            .as_ref()
            .unwrap()
            .projection
            .snapshot
            .as_ref()
            .unwrap()
            .revision;
        self.started.send(revision).await.unwrap();
        if let Some(release) = self.release.take() {
            release.await.unwrap();
        }
        if self.fail {
            Err(error(FocusErrorCode::Unavailable, "Platform unavailable"))
        } else {
            Ok(())
        }
    }
    async fn revoke(&mut self) -> Result<(), FocusExecutionError> {
        Ok(())
    }
    fn invalidate_preferences(&mut self) {}
    fn next_deadline(&self) -> Option<i64> {
        None
    }
}

fn start(backend: TestBackend) -> (DeliveryHandle, tokio::task::JoinHandle<()>) {
    let (desired, input) = watch::channel(DesiredDelivery::default());
    let (feedback, status) = watch::channel(DeliveryStatus::default());
    (
        DeliveryHandle {
            desired,
            feedback: status,
        },
        tokio::spawn(worker::run(backend, input, feedback)),
    )
}

async fn wait_for_status(handle: &mut DeliveryHandle, predicate: impl Fn(&DeliveryStatus) -> bool) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if predicate(&handle.feedback.borrow()) {
                break;
            }
            handle.feedback.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_delivery_remains_single_flight_and_coalesces_while_a_platform_call_waits() {
    let (started, mut starts) = mpsc::channel(4);
    let (release, barrier) = oneshot::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let (mut handle, task) = start(TestBackend {
        started,
        release: Some(barrier),
        calls: Arc::clone(&calls),
        fail: false,
    });
    handle.publish(&projection(1, 1), Some(3), None).unwrap();
    assert_eq!(starts.recv().await, Some(1));
    for revision in 2..=100 {
        handle
            .publish(&projection(1, revision), Some(3), None)
            .unwrap();
    }
    tokio::time::timeout(
        Duration::from_millis(50),
        tokio::time::sleep(Duration::from_millis(1)),
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(
        handle
            .desired
            .borrow()
            .accepted
            .as_ref()
            .unwrap()
            .projection
            .snapshot
            .as_ref()
            .unwrap()
            .revision,
        100
    );
    release.send(()).unwrap();
    assert_eq!(starts.recv().await, Some(100));
    wait_for_status(&mut handle, |status| status.revision == Some(100)).await;
    assert_eq!(calls.load(Ordering::Acquire), 2);
    drop(handle);
    task.await.unwrap();
}

#[tokio::test]
async fn native_delivery_handoff_timeout_preserves_the_worker_until_quiescence() {
    let (started, mut starts) = mpsc::channel(4);
    let (release, barrier) = oneshot::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let (mut handle, task) = start(TestBackend {
        started,
        release: Some(barrier),
        calls: Arc::clone(&calls),
        fail: false,
    });
    handle.publish(&projection(1, 1), Some(3), None).unwrap();
    assert_eq!(starts.recv().await, Some(1));
    assert_eq!(
        handle
            .freeze_with_timeout(Duration::from_millis(5))
            .await
            .unwrap_err()
            .code,
        FocusErrorCode::Busy
    );
    assert!(!handle.is_quiescent());
    assert!(!task.is_finished());
    assert_eq!(calls.load(Ordering::Acquire), 1);
    release.send(()).unwrap();
    handle
        .freeze_with_timeout(Duration::from_secs(1))
        .await
        .unwrap();
    assert!(handle.is_quiescent());
    assert_eq!(calls.load(Ordering::Acquire), 1);
    drop(handle);
    task.await.unwrap();
}

#[tokio::test]
async fn native_delivery_backoff_cannot_retry_on_presentation_traffic_or_pollute_a_new_revision() {
    let (started, mut starts) = mpsc::channel(4);
    let calls = Arc::new(AtomicUsize::new(0));
    let (mut handle, task) = start(TestBackend {
        started,
        release: None,
        calls: Arc::clone(&calls),
        fail: true,
    });
    handle.publish(&projection(1, 1), Some(3), None).unwrap();
    assert_eq!(starts.recv().await, Some(1));
    wait_for_status(&mut handle, |status| status.message.is_some()).await;
    assert_eq!(
        handle.message(&projection(1, 1)),
        Some("Platform unavailable".into())
    );
    assert_eq!(handle.message(&projection(1, 2)), None);
    assert_eq!(handle.message(&projection(2, 1)), None);
    for _ in 0..100 {
        handle.publish(&projection(1, 1), Some(3), None).unwrap();
    }
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(calls.load(Ordering::Acquire), 1);
    handle.publish(&projection(1, 2), Some(3), None).unwrap();
    assert_eq!(starts.recv().await, Some(2));
    assert_eq!(calls.load(Ordering::Acquire), 2);
    drop(handle);
    task.await.unwrap();
}

fn active_projection() -> FocusProjection {
    let mut projection = projection(1, 1);
    let snapshot = projection.snapshot.as_mut().unwrap();
    snapshot.mode = FocusMode::Running;
    snapshot.run = Some(
        serde_json::from_value(serde_json::json!({
            "id": "run", "eventId": "event", "occurrenceId": "event", "eventDate": "2026-10-03",
            "title": null, "startedAtMs": 10_000, "plannedStartMs": 10_000, "plannedEndMs": 700_000,
            "endedAtMs": null, "inheritedFocusMs": 0, "inheritedPhaseMs": 0,
            "configuration": {"rhythm": {"kind": "count", "focusDurationMinutes": 25,
                "shortBreakMinutes": 5, "longBreakMinutes": 15, "longBreakAfterFocusCount": 4},
                "rhythmSource": "custom", "presetKey": null, "idleTimeoutMinutes": 1}
        }))
        .unwrap(),
    );
    snapshot.segment = Some(serde_json::from_value(serde_json::json!({
        "id": "focus", "runId": "run", "eventId": "event", "eventDate": "2026-10-03",
        "phase": "focus", "rhythmPosition": 1, "plannedStartMs": 10_000, "plannedEndMs": 100_000,
        "actualStartMs": 10_000, "actualEndMs": null, "chosenDurationMs": 90_000,
        "status": "active", "endReason": null, "pauses": []
    })).unwrap());
    projection
}

fn break_projection() -> FocusProjection {
    let mut projection = active_projection();
    let snapshot = projection.snapshot.as_mut().unwrap();
    snapshot.revision += 1;
    let mut closed = snapshot.segment.clone().unwrap();
    closed.actual_end_ms = Some(100_000);
    closed.status = "completed".into();
    snapshot.changed_segments = vec![closed];
    let segment = snapshot.segment.as_mut().unwrap();
    segment.id = "break".into();
    segment.phase = ganbaru_pomodoro::FocusPhase::ShortBreak;
    segment.actual_start_ms = 100_000;
    projection
}

#[test]
fn native_delivery_coalescing_retains_one_recent_canonical_phase_closure_until_acknowledged() {
    let start = Instant::now();
    let mut desired = DesiredDelivery {
        accepted: Some(AcceptedDelivery {
            projection: active_projection(),
            ownership_generation: Some(3),
            pool: None,
        }),
        ..DesiredDelivery::default()
    };
    let mut next = break_projection();
    retain_closed_phase(&mut desired, &next, &DeliveryStatus::default(), start);
    assert_eq!(desired.closed_phase.as_ref().unwrap().segment.id, "focus");
    desired.accepted.as_mut().unwrap().projection = next.clone();
    next.snapshot.as_mut().unwrap().changed_segments.clear();
    next.snapshot.as_mut().unwrap().revision += 1;
    retain_closed_phase(
        &mut desired,
        &next,
        &DeliveryStatus::default(),
        start + Duration::from_secs(1),
    );
    assert_eq!(desired.closed_phase.as_ref().unwrap().segment.id, "focus");
    let acknowledged = DeliveryStatus {
        generation: Some(1),
        completed_phase: Some("focus".into()),
        ..DeliveryStatus::default()
    };
    retain_closed_phase(
        &mut desired,
        &next,
        &acknowledged,
        start + Duration::from_secs(2),
    );
    assert!(desired.closed_phase.is_none());
}

#[test]
fn native_delivery_phase_closure_cannot_survive_expiry_stop_or_a_new_vault() {
    let start = Instant::now();
    for reason in ["expiry", "stop", "vault"] {
        let mut desired = DesiredDelivery {
            accepted: Some(AcceptedDelivery {
                projection: active_projection(),
                ownership_generation: Some(3),
                pool: None,
            }),
            ..DesiredDelivery::default()
        };
        let mut next = break_projection();
        retain_closed_phase(&mut desired, &next, &DeliveryStatus::default(), start);
        desired.accepted.as_mut().unwrap().projection = next.clone();
        next.snapshot.as_mut().unwrap().changed_segments.clear();
        let now = match reason {
            "expiry" => start + Duration::from_millis(super::super::EFFECT_LEASE_MS as u64),
            "stop" => {
                next.snapshot.as_mut().unwrap().mode = FocusMode::Stopped;
                start
            }
            "vault" => {
                next.vault_generation += 1;
                start
            }
            _ => unreachable!(),
        };
        retain_closed_phase(&mut desired, &next, &DeliveryStatus::default(), now);
        assert!(desired.closed_phase.is_none());
    }
}

#[test]
fn native_delivery_reports_a_stalled_worker_for_newer_pending_revisions_without_reusing_its_error()
{
    let (desired, _input) = watch::channel(DesiredDelivery {
        accepted: Some(AcceptedDelivery {
            projection: projection(1, 2),
            ownership_generation: Some(3),
            pool: None,
        }),
        ..DesiredDelivery::default()
    });
    let (feedback, status) = watch::channel(DeliveryStatus {
        generation: Some(1),
        revision: Some(1),
        started_at: Some(
            Instant::now() - Duration::from_millis(super::super::EFFECT_LEASE_MS as u64),
        ),
        ..DeliveryStatus::default()
    });
    let handle = DeliveryHandle {
        desired,
        feedback: status,
    };
    assert_eq!(
        handle.message(&projection(1, 2)),
        Some("Native Focus platform delivery is still pending".into())
    );
    feedback.send_replace(DeliveryStatus {
        generation: Some(1),
        revision: Some(1),
        message: Some("Old failure".into()),
        ..DeliveryStatus::default()
    });
    assert_eq!(handle.message(&projection(1, 2)), None);
}
