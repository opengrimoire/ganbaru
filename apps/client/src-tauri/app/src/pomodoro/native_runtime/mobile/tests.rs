use super::*;
use serde_json::{Value, json};

fn copy() -> FocusNotificationCopy {
    serde_json::from_value(json!({
        "channelName": "Focus", "channelDescription": "Progress",
        "alertsChannelName": "Alerts", "alertsChannelDescription": "Phase alerts",
        "focusTitle": "Focus", "shortBreakTitle": "Short break", "longBreakTitle": "Long break",
        "pausedText": "Paused", "focusCompleteTitle": "Focus complete",
        "breakCompleteTitle": "Return", "sessionCompleteText": "Open the app to continue"
    }))
    .unwrap()
}

fn snapshot(mode: FocusMode) -> FocusExecutionSnapshot {
    serde_json::from_value(json!({
        "revision": 1, "observedAtMs": 100_000, "mode": mode,
        "run": {
            "id": "run", "eventId": "event", "occurrenceId": "event::2026-10-03",
            "eventDate": "2026-10-03", "title": "Work", "startedAtMs": 100_000,
            "plannedStartMs": 100_000, "plannedEndMs": 500_000, "endedAtMs": null,
            "inheritedFocusMs": 0, "inheritedPhaseMs": 0,
            "configuration": {"rhythm": {"kind": "count", "focusDurationMinutes": 2,
                "shortBreakMinutes": 1, "longBreakMinutes": 2, "longBreakAfterFocusCount": 2},
                "rhythmSource": "custom", "presetKey": null, "idleTimeoutMinutes": null}
        },
        "segment": {"id": "phase", "runId": "run", "eventId": "event", "eventDate": "2026-10-03",
            "phase": "focus", "rhythmPosition": 1, "plannedStartMs": 100_000,
            "plannedEndMs": 220_000, "actualStartMs": 100_000, "actualEndMs": null,
            "chosenDurationMs": 120_000, "status": "active", "endReason": null, "pauses": []},
        "changedSegments": [], "phaseDeadlineMs": 220_000, "remainingMs": 120_000,
        "elapsedMs": 0, "completedFocusCount": 0, "skipNextBreak": false,
        "focusExtensionUsed": false, "breakExtensionMs": 0, "dismissedOccurrenceId": null,
        "automaticAdmissionSuppressed": false, "pausedPromptsDismissed": false,
        "idleStartedAtMs": null, "idleDetectedAtMs": null, "focusFailedAtMs": null,
        "suspendStartedAtMs": null, "suspendReturnedAtMs": null, "returnStartedAtMs": null,
        "activitySourceUnavailable": false, "effectiveIdleTimeoutMinutes": null
    }))
    .unwrap()
}

fn encoded(snapshot: &FocusExecutionSnapshot, now: i64) -> Option<Value> {
    accepted_notification(snapshot, now, copy())
        .unwrap()
        .map(|value| serde_json::to_value(value).unwrap())
}

#[test]
fn android_publishes_one_accepted_phase_with_a_latency_adjusted_deadline() {
    let notification = encoded(&snapshot(FocusMode::Running), 100_001).unwrap();
    assert_eq!(notification["phases"].as_array().unwrap().len(), 1);
    assert_eq!(notification["phases"][0]["id"], "phase");
    assert_eq!(notification["phases"][0]["endsAtEpochMs"], 220_000);
    assert_eq!(notification["remainingSeconds"], 120);
    assert_eq!(notification["totalSeconds"], 120);
    assert_eq!(notification["generatedAtEpochMs"], 100_001);
    let later = encoded(&snapshot(FocusMode::Running), 101_001).unwrap();
    assert_eq!(later["remainingSeconds"], 119);
    assert!(encoded(&snapshot(FocusMode::Running), 220_000).is_none());
    let configuration: Value =
        serde_json::from_str(notification["configJson"].as_str().unwrap()).unwrap();
    assert_eq!(configuration["rhythm"]["focusDurationMinutes"], 2);
}

#[test]
fn paused_android_phases_keep_fixed_work_remaining_and_expire_at_the_event_end() {
    for mode in [
        FocusMode::ManualPause,
        FocusMode::IdlePause,
        FocusMode::Suspended,
    ] {
        let snapshot = snapshot(mode);
        let notification = encoded(&snapshot, 300_000).unwrap();
        assert_eq!(notification["isRunning"], false);
        assert_eq!(notification["remainingSeconds"], 120);
        assert_eq!(notification["phases"][0]["endsAtEpochMs"], 500_000);
        assert_eq!(encoded(&snapshot, 499_999).unwrap()["remainingSeconds"], 1);
        assert!(encoded(&snapshot, 500_000).is_none());
    }
}

#[test]
fn waiting_failed_stopped_and_expired_modes_cannot_publish_successor_phases() {
    for mode in [
        FocusMode::ReturnWait,
        FocusMode::IdleFailed,
        FocusMode::Stopped,
        FocusMode::Expired,
    ] {
        assert!(encoded(&snapshot(mode), 100_000).is_none());
    }
    let mut closed = snapshot(FocusMode::Running);
    closed.run.as_mut().unwrap().ended_at_ms = Some(100_001);
    assert!(encoded(&closed, 100_001).is_none());
}

#[test]
fn malformed_accepted_phase_identity_and_future_evidence_are_rejected() {
    let mut snapshot = snapshot(FocusMode::Running);
    snapshot.segment.as_mut().unwrap().run_id = "other-run".into();
    assert!(accepted_notification(&snapshot, 100_000, copy()).is_err());
    snapshot.segment.as_mut().unwrap().run_id = "run".into();
    assert!(accepted_notification(&snapshot, 99_999, copy()).is_err());
    snapshot.phase_deadline_ms = None;
    assert!(accepted_notification(&snapshot, 100_000, copy()).is_err());
}

#[test]
fn android_deadlines_and_unicode_titles_obey_platform_bounds() {
    let mut snapshot = snapshot(FocusMode::Running);
    snapshot.run.as_mut().unwrap().planned_end_ms = 101_500;
    snapshot.run.as_mut().unwrap().title = Some("🦀".repeat(100));
    let notification = encoded(&snapshot, 100_000).unwrap();
    assert_eq!(notification["phases"][0]["endsAtEpochMs"], 101_500);
    assert_eq!(notification["remainingSeconds"], 2);
    assert_eq!(
        notification["eventTitle"]
            .as_str()
            .unwrap()
            .encode_utf16()
            .count(),
        160
    );
    assert!(encoded(&snapshot, 101_500).is_none());
}

#[test]
fn notification_language_rejects_execution_fields_and_overlong_text() {
    let mut value = serde_json::to_value(copy()).unwrap();
    value["runId"] = json!("forged-run");
    assert!(serde_json::from_value::<FocusNotificationCopy>(value).is_err());
    let mut copy = copy();
    copy.focus_title = "🦀".repeat(81);
    assert!(copy.validate().is_err());
    copy.focus_title = " ".into();
    assert!(copy.validate().is_err());
}

#[test]
fn committed_boundaries_prepare_reminders_without_fabricating_an_active_phase() {
    let mut ended = snapshot(FocusMode::ReturnWait);
    ended.revision = 2;
    let phase = ended.segment.as_mut().unwrap();
    phase.actual_end_ms = Some(220_000);
    phase.status = "completed".into();
    phase.end_reason = Some("completed".into());
    let reminder = completion_notification(
        ended.run.as_ref().unwrap(),
        ended.segment.as_ref().unwrap(),
        220_000,
        copy(),
    )
    .unwrap();
    let reminder = serde_json::to_value(reminder).unwrap();
    assert_eq!(reminder["remainingSeconds"], 0);
    assert_eq!(reminder["isRunning"], false);
    assert_eq!(reminder["phases"][0]["id"], "phase");
    assert_eq!(reminder["phases"][0]["endsAtEpochMs"], 220_000);
    assert_eq!(ended.mode, FocusMode::ReturnWait);
    assert_eq!(ended.segment.as_ref().unwrap().status, "completed");
    assert!(
        completion_notification(
            ended.run.as_ref().unwrap(),
            ended.segment.as_ref().unwrap(),
            219_999,
            copy()
        )
        .is_err()
    );
}

#[test]
fn completion_delivery_requires_a_new_close_of_the_previously_published_phase() {
    let previous = PublishedPhase {
        generation: 1,
        revision: 1,
        run_id: Some("run".into()),
        phase_id: Some("phase".into()),
        active: true,
    };
    let mut ended = snapshot(FocusMode::ReturnWait);
    ended.revision = 2;
    ended.segment.as_mut().unwrap().status = "completed".into();
    ended.segment.as_mut().unwrap().actual_end_ms = Some(220_000);
    assert_eq!(
        newly_closed_phase(Some(&previous), 1, &ended).unwrap().id,
        "phase"
    );
    assert!(newly_closed_phase(None, 1, &ended).is_none());
    assert!(newly_closed_phase(Some(&previous), 2, &ended).is_none());
    ended.segment.as_mut().unwrap().actual_end_ms = Some(100_000);
    assert!(newly_closed_phase(Some(&previous), 1, &ended).is_none());
    ended.segment.as_mut().unwrap().actual_end_ms = Some(220_000);
    ended.revision = 1;
    assert!(newly_closed_phase(Some(&previous), 1, &ended).is_none());
    ended.revision = 2;
    ended.mode = FocusMode::Stopped;
    assert!(newly_closed_phase(Some(&previous), 1, &ended).is_none());
    ended.mode = FocusMode::Running;
    let closed = ended.segment.take().unwrap();
    ended.changed_segments.push(closed);
    ended.segment = snapshot(FocusMode::Running).segment;
    ended.segment.as_mut().unwrap().id = "next-phase".into();
    assert_eq!(
        newly_closed_phase(Some(&previous), 1, &ended).unwrap().id,
        "phase"
    );
    ended.run.as_mut().unwrap().id = "other-run".into();
    assert!(newly_closed_phase(Some(&previous), 1, &ended).is_none());
}
