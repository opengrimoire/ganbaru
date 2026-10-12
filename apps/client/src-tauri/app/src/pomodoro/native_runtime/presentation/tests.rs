use super::*;
use ganbaru_pomodoro::FocusExecutionSnapshot;
use serde_json::json;

fn snapshot(mode: FocusMode, phase: FocusPhase) -> FocusExecutionSnapshot {
    serde_json::from_value(json!({
        "revision": 1, "observedAtMs": 100_000, "mode": mode,
        "run": null, "segment": {
            "id": "phase", "runId": "run", "eventId": null, "eventDate": "2026-10-02",
            "phase": phase, "rhythmPosition": 1, "plannedStartMs": 100_000,
            "plannedEndMs": 200_000, "actualStartMs": 100_000, "actualEndMs": null,
            "chosenDurationMs": 100_000, "status": "active", "endReason": null, "pauses": []
        },
        "changedSegments": [], "phaseDeadlineMs": 200_000,
        "remainingMs": 100_000, "elapsedMs": 0, "completedFocusCount": 0,
        "skipNextBreak": false, "focusExtensionUsed": false, "breakExtensionMs": 0,
        "dismissedOccurrenceId": null, "automaticAdmissionSuppressed": false,
        "pausedPromptsDismissed": false, "idleStartedAtMs": 40_000, "idleDetectedAtMs": 100_000,
        "focusFailedAtMs": null, "suspendStartedAtMs": null, "suspendReturnedAtMs": null,
        "returnStartedAtMs": null, "activitySourceUnavailable": false, "effectiveIdleTimeoutMinutes": null
    })).unwrap()
}

#[test]
fn focus_warning_wakes_at_its_native_deadline_and_does_not_repeat_or_offer_used_extension() {
    let preferences = Preferences::default();
    let mut running = snapshot(FocusMode::Running, FocusPhase::Focus);
    let first = PresentationState::default().plan(1, &running, 100_000, &preferences);
    assert_eq!(first.notification, None);
    assert_eq!(first.next.next_warning_ms, Some(140_000));
    let warning = first.next.plan(1, &running, 140_000, &preferences);
    assert_eq!(
        warning.notification,
        Some(DesktopAlert::Ending {
            remaining_ms: 60_000,
            allow_extension: true
        })
    );
    assert_eq!(warning.sound, Some(AppSound::FocusEndingWarning));
    assert_eq!(warning.next.next_warning_ms, None);
    assert_eq!(
        warning
            .next
            .plan(1, &running, 141_000, &preferences)
            .notification,
        None
    );

    running.phase_deadline_ms = Some(380_000);
    running.focus_extension_used = true;
    let extended = warning.next.plan(1, &running, 180_000, &preferences);
    assert_eq!(extended.next.next_warning_ms, Some(320_000));
    assert_eq!(
        extended
            .next
            .plan(1, &running, 320_000, &preferences)
            .notification,
        Some(DesktopAlert::Ending {
            remaining_ms: 60_000,
            allow_extension: false
        })
    );
    assert_eq!(
        extended
            .next
            .plan(1, &running, 380_000, &preferences)
            .notification,
        None
    );
}

#[test]
fn paused_reminders_coalesce_late_wakes_and_follow_persisted_dismissal() {
    let preferences = Preferences::default();
    let mut paused = snapshot(FocusMode::ManualPause, FocusPhase::Focus);
    let first = PresentationState::default().plan(1, &paused, 100_000, &preferences);
    assert_eq!(first.notification, None);
    assert_eq!(first.next.next_alert_ms, Some(160_000));
    let delayed = first.next.plan(1, &paused, 500_000, &preferences);
    assert_eq!(delayed.notification, Some(DesktopAlert::Paused));
    assert_eq!(delayed.sound, Some(AppSound::EventNotification));
    assert_eq!(delayed.next.next_alert_ms, Some(560_000));
    paused.paused_prompts_dismissed = true;
    let dismissed = delayed.next.plan(1, &paused, 510_000, &preferences);
    assert_eq!(dismissed.notification, None);
    assert_eq!(dismissed.next.next_alert_ms, None);
    assert_eq!(
        dismissed
            .next
            .plan(1, &paused, 600_000, &preferences)
            .notification,
        None
    );
    assert_eq!(
        first
            .next
            .plan(
                1,
                &snapshot(FocusMode::Stopped, FocusPhase::Focus),
                170_000,
                &preferences
            )
            .notification,
        None
    );
}

#[test]
fn desktop_notification_copy_accepts_localization_and_rejects_blank_oversized_and_execution_fields()
{
    let copy = json!({
        "endingWarningTitle": "Tu sesión está por terminar", "extendFocusLabel": "Extender",
        "pausedReminderTitle": "Pausada", "pausedReminderBody": "Tu sesión sigue pausada",
        "resumeFocusLabel": "Reanudar", "dismissPromptsLabel": "Dejar de preguntar"
    });
    serde_json::from_value::<DesktopNotificationCopy>(copy.clone())
        .unwrap()
        .validate()
        .unwrap();
    for text in ["a".repeat(160), "\u{1d11e}".repeat(80)] {
        let mut boundary = copy.clone();
        boundary["endingWarningTitle"] = text.into();
        serde_json::from_value::<DesktopNotificationCopy>(boundary)
            .unwrap()
            .validate()
            .unwrap();
    }
    for text in [" ".to_string(), "a".repeat(161), "\u{1d11e}".repeat(81)] {
        let mut invalid = copy.clone();
        invalid["endingWarningTitle"] = text.into();
        assert!(
            serde_json::from_value::<DesktopNotificationCopy>(invalid)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    let mut supplied_clock = copy;
    supplied_clock["remainingSeconds"] = 60.into();
    assert!(serde_json::from_value::<DesktopNotificationCopy>(supplied_clock).is_err());
}

#[test]
fn idle_failure_requires_accepted_failure_instead_of_elapsed_presentation_time() {
    let preferences = Preferences::default();
    let idle = snapshot(FocusMode::IdlePause, FocusPhase::Focus);
    let first = PresentationState::default().plan(1, &idle, 100_000, &preferences);
    assert_eq!(first.surface, Some(Surface::Idle(60)));
    let late = first.next.plan(1, &idle, 170_000, &preferences);
    assert_eq!(late.surface, None);
    assert_eq!(late.sound, Some(AppSound::IdleAlert));
    let failure = late.next.plan(
        1,
        &snapshot(FocusMode::IdleFailed, FocusPhase::Focus),
        170_000,
        &preferences,
    );
    assert_eq!(failure.surface, Some(Surface::IdleFailed));
    assert_eq!(failure.sound, Some(AppSound::FocusSessionFailedLongIdle));
    assert_eq!(failure.next.next_alert_ms, None);
}

#[test]
fn reaching_a_visual_break_deadline_cannot_publish_return_wait() {
    let preferences = Preferences::default();
    let running = snapshot(FocusMode::Running, FocusPhase::ShortBreak);
    let first = PresentationState::default().plan(1, &running, 100_000, &preferences);
    assert_eq!(first.surface, Some(Surface::Break(200_000)));
    let late = first.next.plan(1, &running, 210_000, &preferences);
    assert_eq!(late.surface, None);
    assert_eq!(late.sound, None);
    let accepted = late.next.plan(
        1,
        &snapshot(FocusMode::ReturnWait, FocusPhase::ShortBreak),
        210_000,
        &preferences,
    );
    assert_eq!(accepted.surface, Some(Surface::ReturnWait));
    assert_eq!(accepted.sound, Some(AppSound::BreakFinished));
}

#[test]
fn break_warning_is_once_per_accepted_deadline_and_rearms_after_extension() {
    let preferences = Preferences::default();
    let mut running = snapshot(FocusMode::Running, FocusPhase::ShortBreak);
    let first = PresentationState::default().plan(1, &running, 100_000, &preferences);
    assert_eq!(
        first.next.plan(1, &running, 189_999, &preferences).sound,
        None
    );
    let warning = first.next.plan(1, &running, 190_000, &preferences);
    assert_eq!(warning.sound, Some(AppSound::BreakFinished));
    assert_eq!(
        warning.next.plan(1, &running, 190_001, &preferences).sound,
        None
    );
    running.phase_deadline_ms = Some(260_000);
    assert_eq!(
        warning.next.plan(1, &running, 250_000, &preferences).sound,
        Some(AppSound::BreakFinished)
    );
    assert_eq!(
        warning.next.plan(1, &running, 260_000, &preferences).sound,
        None
    );
}

#[test]
fn return_alerts_coalesce_delayed_wakes_and_respect_disabled_repetition() {
    let mut preferences = Preferences::default();
    let wait = snapshot(FocusMode::ReturnWait, FocusPhase::ShortBreak);
    let first = PresentationState::default().plan(1, &wait, 100_000, &preferences);
    let delayed = first.next.plan(1, &wait, 500_000, &preferences);
    assert_eq!(delayed.sound, Some(AppSound::BreakFinished));
    assert_eq!(delayed.next.next_alert_ms, Some(510_000));
    preferences.repeat_seconds = 0;
    let disabled = delayed.next.plan(1, &wait, 510_000, &preferences);
    assert_eq!(disabled.sound, None);
    assert_eq!(disabled.next.next_alert_ms, None);
}

#[test]
fn preferences_preserve_explicit_disabled_options_and_reject_unsupported_values() {
    let preferences = Preferences::parse(&json!({"preferences": {
        "focusBreakFinishedRepeatSeconds": 0, "focusBreakEndWarningSeconds": 999,
        "focusBreakEndEscPresses": null, "focusBreakExtensionLimit": 15
    }}));
    assert_eq!(preferences.repeat_seconds, 0);
    assert_eq!(
        preferences.warning_seconds,
        Preferences::default().warning_seconds
    );
    assert_eq!(preferences.esc_presses, None);
    assert_eq!(preferences.extension_limit, Some(15));
}

#[test]
fn native_completion_distinguishes_later_commitments_from_day_and_friday_endings() {
    use ganbaru_calendar::reads::focus_context::FocusPlannedBlock;
    let block = |id: &str, date: &str, start: &str| FocusPlannedBlock {
        event_date: date.into(),
        event_id: id.into(),
        original_event_id: id.into(),
        planned_start: start.into(),
        planned_end: "2026-10-02T18:00:00Z".into(),
        source_kind: "scheduler_snapshot",
    };
    let ended = DateTime::parse_from_rfc3339("2026-10-02T12:00:00Z")
        .unwrap()
        .timestamp_millis();
    assert_eq!(
        classify_completion("2026-10-02", "ended", ended, &[]).unwrap(),
        "workweek"
    );
    assert_eq!(
        classify_completion("2026-10-01", "ended", ended, &[]).unwrap(),
        "day"
    );
    assert_eq!(
        classify_completion(
            "2026-10-02",
            "ended",
            ended,
            &[
                block("ended", "2026-10-02", "2026-10-02T13:00:00Z"),
                block("earlier", "2026-10-02", "2026-10-02T11:00:00Z"),
                block("tomorrow", "2026-10-03", "2026-10-03T13:00:00Z"),
            ]
        )
        .unwrap(),
        "workweek"
    );
    assert_eq!(
        classify_completion(
            "2026-10-02",
            "ended",
            ended,
            &[block("later", "2026-10-02", "2026-10-02T12:00:00Z"),]
        )
        .unwrap(),
        "event"
    );
    assert!(
        classify_completion(
            "2026-10-02",
            "ended",
            ended,
            &[block("malformed", "2026-10-02", "invalid instant"),]
        )
        .is_err()
    );
}
