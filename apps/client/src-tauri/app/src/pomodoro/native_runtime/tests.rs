use super::*;

fn snapshot() -> FocusExecutionSnapshot {
    serde_json::from_value(serde_json::json!({
        "revision": 10, "observedAtMs": 1000, "mode": "running",
        "run": {"id": "run", "occurrenceId": "event::2026-10-03", "eventDate": "2026-10-03",
            "startedAtMs": 1000, "plannedStartMs": 1000, "plannedEndMs": 100000,
            "inheritedFocusMs": 0, "inheritedPhaseMs": 0,
            "configuration": {"rhythm": {"kind": "count", "focusDurationMinutes": 25,
                "shortBreakMinutes": 5, "longBreakMinutes": 15, "longBreakAfterFocusCount": 4},
                "rhythmSource": "custom"}},
        "segment": {"id": "segment", "runId": "run", "eventDate": "2026-10-03",
            "phase": "focus", "rhythmPosition": 1, "plannedStartMs": 1000,
            "plannedEndMs": 100000, "actualStartMs": 1000, "chosenDurationMs": 99000,
            "status": "active", "pauses": []},
        "changedSegments": [], "remainingMs": 99000, "elapsedMs": 0,
        "completedFocusCount": 0, "skipNextBreak": false, "focusExtensionUsed": false,
        "breakExtensionMs": 0, "automaticAdmissionSuppressed": false,
        "pausedPromptsDismissed": false, "activitySourceUnavailable": false
    }))
    .unwrap()
}

fn context() -> FocusNativeContext {
    FocusNativeContext {
        vault_id: "vault".into(),
        vault_generation: 3,
        revision: 10,
        mode: FocusMode::Running,
        phase: Some(FocusPhase::Focus),
        run_id: Some("run".into()),
        segment_id: Some("segment".into()),
        idle_detected_at_ms: None,
    }
}

#[test]
fn native_tray_publication_requires_its_exact_snapshot_but_clicks_survive_heartbeats() {
    let context = context();
    let mut snapshot = snapshot();
    assert!(context.matches_snapshot(3, &snapshot));
    snapshot.revision += 1;
    snapshot.observed_at_ms += 1000;
    snapshot.remaining_ms -= 1000;
    assert!(!context.matches_snapshot(3, &snapshot));
    assert!(context.matches_phase(3, &snapshot));
}

#[test]
fn native_displayed_controls_reject_changed_run_segment_phase_mode_and_idle_episode() {
    let context = context();
    let snapshot = snapshot();
    assert!(!context.matches_phase(4, &snapshot));
    let mut changed = snapshot.clone();
    changed.run.as_mut().unwrap().id = "new-run".into();
    assert!(!context.matches_phase(3, &changed));
    changed = snapshot.clone();
    changed.segment.as_mut().unwrap().id = "new-segment".into();
    assert!(!context.matches_phase(3, &changed));
    changed = snapshot.clone();
    changed.segment.as_mut().unwrap().phase = FocusPhase::ShortBreak;
    assert!(!context.matches_phase(3, &changed));
    changed = snapshot.clone();
    changed.mode = FocusMode::ManualPause;
    assert!(!context.matches_phase(3, &changed));
    changed = snapshot;
    changed.idle_detected_at_ms = Some(2000);
    assert!(!context.matches_phase(3, &changed));
}

#[test]
fn quiet_focus_activity_counts_only_a_focus_phase_in_progress() {
    let mut running = snapshot();
    running.phase_deadline_ms = Some(100_000);
    assert_eq!(
        focus_phase_activity_of(Some(&running)),
        Some(FocusPhaseActivity {
            deadline_ms: Some(100_000)
        })
    );

    let mut paused = running.clone();
    paused.mode = FocusMode::ManualPause;
    assert_eq!(
        focus_phase_activity_of(Some(&paused)),
        Some(FocusPhaseActivity { deadline_ms: None })
    );

    let mut on_break = running.clone();
    if let Some(segment) = on_break.segment.as_mut() {
        segment.phase = FocusPhase::ShortBreak;
    }
    assert_eq!(focus_phase_activity_of(Some(&on_break)), None);

    let mut stopped = running.clone();
    stopped.mode = FocusMode::Stopped;
    assert_eq!(focus_phase_activity_of(Some(&stopped)), None);

    let mut without_segment = running;
    without_segment.segment = None;
    assert_eq!(focus_phase_activity_of(Some(&without_segment)), None);
    assert_eq!(focus_phase_activity_of(None), None);
}

#[test]
fn vault_access_is_frozen_until_attached_and_only_resuming_carries_a_revision() {
    assert!(!VaultAccess::Attached.is_frozen());
    assert!(VaultAccess::Frozen.is_frozen());
    assert!(VaultAccess::Resuming { revision: 7 }.is_frozen());
    assert_eq!(VaultAccess::Attached.resume_revision(), None);
    assert_eq!(VaultAccess::Frozen.resume_revision(), None);
    assert_eq!(
        VaultAccess::Resuming { revision: 7 }.resume_revision(),
        Some(7)
    );
}
