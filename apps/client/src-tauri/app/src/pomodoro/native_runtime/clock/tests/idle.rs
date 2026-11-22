use super::*;
use ganbaru_pomodoro::FocusModeState;

fn idle() -> FocusExecutionSnapshot {
    serde_json::from_value(serde_json::json!({
        "revision": 1, "observedAtMs": 100_000, "mode": "idle_pause",
        "run": {"id": "run", "eventId": "event", "occurrenceId": "event", "eventDate": "2026-10-03",
            "title": null, "startedAtMs": 10_000, "plannedStartMs": 10_000, "plannedEndMs": 700_000,
            "endedAtMs": null, "inheritedFocusMs": 0, "inheritedPhaseMs": 0,
            "configuration": {"rhythm": {"kind": "count", "focusDurationMinutes": 25,
                "shortBreakMinutes": 5, "longBreakMinutes": 15, "longBreakAfterFocusCount": 4},
                "rhythmSource": "custom", "presetKey": null, "idleTimeoutMinutes": 1}},
        "segment": {"id": "segment", "runId": "run", "eventId": "event", "eventDate": "2026-10-03",
            "phase": "focus", "rhythmPosition": 1, "plannedStartMs": 10_000, "plannedEndMs": 700_000,
            "actualStartMs": 10_000, "actualEndMs": null, "chosenDurationMs": 690_000,
            "status": "active", "endReason": null, "pauses": []},
        "changedSegments": [], "phaseDeadlineMs": null, "remainingMs": 690_000, "elapsedMs": 0,
        "completedFocusCount": 0, "skipNextBreak": false, "focusExtensionUsed": false,
        "breakExtensionMs": 0, "dismissedOccurrenceId": null, "automaticAdmissionSuppressed": false,
        "pausedPromptsDismissed": false, "idleStartedAtMs": 10_000, "idleDetectedAtMs": 70_000,
        "idleOverlayVisibleAtMs": 100_000, "focusFailedAtMs": null, "suspendStartedAtMs": null,
        "suspendReturnedAtMs": null, "returnStartedAtMs": null, "activitySourceUnavailable": false,
        "effectiveIdleTimeoutMinutes": 1
    })).unwrap()
}

#[test]
fn native_idle_grace_survives_civil_clock_changes_and_repeated_publications() {
    let mut snapshot = idle();
    let start = Instant::now();
    let mut clock = IdleGraceClock::default();
    clock.update(4, Some(&snapshot), start);
    snapshot.observed_at_ms += 300_000;
    snapshot.revision += 1;
    clock.update(4, Some(&snapshot), start + Duration::from_secs(10));
    assert_eq!(
        clock.remaining(start + Duration::from_secs(10)),
        Some(Duration::from_secs(50))
    );
    snapshot.observed_at_ms = 105_000;
    clock.update(4, Some(&snapshot), start + Duration::from_secs(59));
    assert!(
        clock
            .elapsed_observation(4, &snapshot, start + Duration::from_millis(59_999))
            .is_none()
    );
    assert!(matches!(
        clock.elapsed_observation(4, &snapshot, start + Duration::from_secs(60)),
        Some(FocusObservation::IdleGraceElapsed {
            visible_at_ms: 100_000,
            elapsed_ms: 60_000,
            ..
        })
    ));
    assert!(
        clock
            .elapsed_observation(5, &snapshot, start + Duration::from_secs(60))
            .is_none()
    );
}

#[test]
fn native_idle_grace_revokes_on_resume_and_resets_across_generations() {
    let mut snapshot = idle();
    let start = Instant::now();
    let mut clock = IdleGraceClock::default();
    clock.update(4, Some(&snapshot), start);
    clock.update(5, Some(&snapshot), start + Duration::from_secs(30));
    assert_eq!(
        clock.remaining(start + Duration::from_secs(30)),
        Some(Duration::from_secs(60))
    );
    snapshot.mode = FocusModeState::Running;
    clock.update(5, Some(&snapshot), start + Duration::from_secs(40));
    assert_eq!(clock.remaining(start + Duration::from_secs(50)), None);
    snapshot.mode = idle_pause(None);
    clock.update(5, Some(&snapshot), start + Duration::from_secs(50));
    assert_eq!(clock.remaining(start + Duration::from_secs(90)), None);
    snapshot.mode = idle_pause(Some(100_000));
    clock.update(5, Some(&snapshot), start + Duration::from_secs(100));
    assert_eq!(
        clock.remaining(start + Duration::from_secs(100)),
        Some(Duration::from_secs(60))
    );
    clock.update(5, None, start + Duration::from_secs(101));
    assert_eq!(clock.remaining(start + Duration::from_secs(102)), None);
}

/// The fixture's idle episode with the given warning visibility.
fn idle_pause(idle_overlay_visible_at_ms: Option<i64>) -> FocusModeState {
    FocusModeState::IdlePause {
        idle_started_at_ms: 10_000,
        idle_detected_at_ms: 70_000,
        idle_overlay_visible_at_ms,
        paused_prompts_dismissed: false,
    }
}
