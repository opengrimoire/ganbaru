use super::*;

fn write_state(path: &Path, state: &DistractionsRuntimeState) {
    std::fs::write(path, serde_json::to_vec(state).unwrap()).unwrap();
}

#[test]
fn malformed_runtime_files_read_as_no_authority() {
    let dir = std::env::temp_dir().join(format!("ganbaru-phase-malformed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("distractions-state.json");
    let checked_at = "2026-05-26T00:00:10Z".parse::<DateTime<Utc>>().unwrap();
    write_state(&path, &focus_state());
    assert!(read_fresh_runtime_state(&path, checked_at).is_some());
    let malformed = [
        serde_json::json!({
            "active": true, "paused": false, "pauseReason": null, "phase": "focus",
            "activeRunId": "run-1", "activeOccurrenceId": null, "remainingSeconds": 30,
            "updatedAt": "2026-05-26T00:00:00.000Z", "validUntilMs": i64::MAX
        }),
        serde_json::json!({
            "authority": {"status": "active", "phase": "focus", "pauseReason": "network",
                "runId": "run-1", "occurrenceId": null, "remainingSeconds": 30},
            "updatedAt": "2026-05-26T00:00:00.000Z", "validUntilMs": i64::MAX
        }),
        serde_json::json!({
            "authority": {"status": "active", "phase": "planning", "pauseReason": null,
                "runId": "run-1", "occurrenceId": null, "remainingSeconds": 30},
            "updatedAt": "2026-05-26T00:00:00.000Z", "validUntilMs": i64::MAX
        }),
        serde_json::json!({
            "authority": {"status": "active", "phase": "focus", "pauseReason": null,
                "runId": "run-1", "occurrenceId": null, "remainingSeconds": -1},
            "updatedAt": "2026-05-26T00:00:00.000Z", "validUntilMs": i64::MAX
        }),
    ];
    for contents in malformed {
        std::fs::write(&path, contents.to_string()).unwrap();
        assert!(
            read_fresh_runtime_state(&path, checked_at).is_none(),
            "{contents} must not authorize"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn accepted_focus_validity_expires_at_the_deadline_even_with_a_fresh_heartbeat() {
    let dir = std::env::temp_dir().join(format!("ganbaru-phase-expiry-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("distractions-state.json");
    let updated = "2026-05-26T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let mut phase = focus_state();
    phase.valid_until_ms = updated.timestamp_millis() + 500;
    write_state(&path, &phase);
    assert!(
        read_fresh_runtime_state(&path, updated + chrono::Duration::milliseconds(499)).is_some()
    );
    assert!(
        read_fresh_runtime_state(&path, updated + chrono::Duration::milliseconds(500)).is_none()
    );
    let mut paused = state(BlockingPhase::Focus, Some(PauseReason::Manual));
    paused.valid_until_ms = phase.valid_until_ms;
    write_state(&path, &paused);
    assert!(
        read_fresh_runtime_state(&path, updated + chrono::Duration::milliseconds(500)).is_none()
    );
    let mut short = focus_state();
    if let RuntimeAuthority::Active(active) = &mut short.authority {
        active.remaining_seconds = 1;
    }
    write_state(&path, &short);
    assert!(
        read_fresh_runtime_state(&path, updated + chrono::Duration::milliseconds(999)).is_some()
    );
    assert!(read_fresh_runtime_state(&path, updated + chrono::Duration::seconds(1)).is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_phase_rules_are_bounded_by_accepted_phase_event_and_owner_validity() {
    use ganbaru_pomodoro::{CommittedFocusEffect, FocusMode, FocusPhase};
    let mut effect = CommittedFocusEffect {
        vault_id: "vault".into(),
        vault_generation: 47,
        execution_revision: 6,
        run_id: Some("run".into()),
        segment_id: Some("segment".into()),
        event_id: Some("event".into()),
        occurrence_id: Some("event::date".into()),
        event_title: None,
        phase: Some(FocusPhase::Focus),
        mode: FocusMode::Running,
        phase_deadline_ms: Some(1500),
        event_deadline_ms: Some(2000),
        remaining_ms: 500,
        valid_until_ms: 16000,
    };
    let committed = |effect: &CommittedFocusEffect, now_ms: i64| {
        crate::distractions::state_files::committed_focus_state(effect, now_ms).unwrap()
    };
    let running = committed(&effect, 1000);
    assert_eq!(
        running.authority,
        RuntimeAuthority::Active(ActivePhase {
            phase: BlockingPhase::Focus,
            pause_reason: None,
            run_id: "run".into(),
            occurrence_id: Some("event::date".into()),
            remaining_seconds: 1,
        })
    );
    assert_eq!(running.valid_until_ms, 1500);
    assert_eq!(
        committed(&effect, 1500).authority,
        RuntimeAuthority::Inactive {}
    );
    for (mode, reason) in [
        (FocusMode::ManualPause, PauseReason::Manual),
        (FocusMode::IdlePause, PauseReason::Idle),
        (FocusMode::Suspended, PauseReason::Suspend),
    ] {
        effect.mode = mode;
        effect.remaining_ms = 42_000;
        let paused = committed(&effect, 1000);
        let active = paused.authority.active_phase().unwrap();
        assert_eq!(active.pause_reason, Some(reason));
        assert_eq!(active.remaining_seconds, 42);
        assert_eq!(paused.valid_until_ms, 2000);
    }
    for mode in [
        FocusMode::IdleFailed,
        FocusMode::ReturnWait,
        FocusMode::Expired,
        FocusMode::Stopped,
    ] {
        effect.mode = mode;
        assert_eq!(
            committed(&effect, 1000).authority,
            RuntimeAuthority::Inactive {}
        );
    }
    effect.mode = FocusMode::Running;
    effect.run_id = None;
    assert_eq!(
        committed(&effect, 1000).authority,
        RuntimeAuthority::Inactive {}
    );
    effect.run_id = Some("run".into());
    effect.valid_until_ms = 0;
    assert_eq!(
        committed(&effect, 1000).authority,
        RuntimeAuthority::Inactive {}
    );
}

#[test]
fn clears_shutdown_enforcement_state_files() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ganbaru-distractions-cleanup-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let state_path = dir.join("distractions-state.json");
    let limit_state_path = dir.join("distractions-limit-state.json");
    std::fs::write(&state_path, "{}").unwrap();
    std::fs::write(&limit_state_path, "{}").unwrap();

    clear_enforcement_state_files(&state_path, &limit_state_path).unwrap();

    assert!(!state_path.exists());
    assert!(!limit_state_path.exists());
    clear_enforcement_state_files(&state_path, &limit_state_path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn future_runtime_and_budget_snapshots_cannot_authorize_execution() {
    let dir = std::env::temp_dir().join(format!(
        "ganbaru-distractions-future-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let runtime_path = dir.join("distractions-state.json");
    let limit_path = dir.join("distractions-limit-state.json");
    let database_path = dir.join("ganbaru.sqlite");
    std::fs::write(&runtime_path, serde_json::to_vec(&focus_state()).unwrap()).unwrap();
    let value = serde_json::json!({"localDate": "2026-05-26", "weekStartLocalDate": "2026-05-25",
        "updatedAt": "2026-05-26T00:00:00.000Z", "databasePath": database_path, "limits": []});
    std::fs::write(&limit_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let before = DateTime::parse_from_rfc3339("2026-05-25T23:59:59.999Z")
        .unwrap()
        .with_timezone(&Utc);
    assert!(read_fresh_runtime_state(&runtime_path, before).is_none());
    assert!(read_fresh_limit_state(&limit_path, before, &database_path).is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn stale_limit_state_is_not_authoritative() {
    let dir = std::env::temp_dir().join(format!(
        "ganbaru-distractions-stale-limit-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("distractions-limit-state.json");
    let database_path = dir.join("ganbaru.sqlite");
    let value = serde_json::json!({
        "localDate": "2026-05-26",
        "weekStartLocalDate": "2026-05-25",
        "updatedAt": "2026-05-26T00:00:00.000Z",
        "databasePath": database_path,
        "limits": []
    });
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let checked_at = DateTime::parse_from_rfc3339("2026-05-26T00:00:21.000Z")
        .unwrap()
        .with_timezone(&Utc);

    assert!(read_fresh_limit_state(&path, checked_at, &database_path).is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn atomic_state_write_does_not_follow_predictable_temp_symlinks() {
    use std::os::unix::fs::symlink;

    let dir = std::env::temp_dir().join(format!(
        "ganbaru-distractions-atomic-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("distractions-state.json");
    let target = dir.join("protected.txt");
    let predictable_temporary = dir.join("distractions-state.json.tmp");
    std::fs::write(&target, "protected").unwrap();
    symlink(&target, &predictable_temporary).unwrap();

    write_text_file_atomically(&path, "new state").unwrap();

    assert_eq!(std::fs::read_to_string(&target).unwrap(), "protected");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new state");
    assert!(
        std::fs::symlink_metadata(&predictable_temporary)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recent_extension_status_is_connected() {
    let checked_at = DateTime::parse_from_rfc3339("2026-05-26T00:01:00.000Z")
        .unwrap()
        .with_timezone(&Utc);
    let status = extension_status_from_file_contents(
        r#"{"lastSeenAt":"2026-05-26T00:00:05.000Z","lastMessageType":"get_state"}"#,
        checked_at,
        None,
    );

    assert!(status.connected);
    assert_eq!(status.last_message_type.as_deref(), Some("get_state"));
    assert_eq!(
        status.last_seen_at.as_deref(),
        Some("2026-05-26T00:00:05.000Z")
    );
    assert!(status.reason.is_none());
}

#[test]
fn stale_extension_status_is_disconnected() {
    let checked_at = DateTime::parse_from_rfc3339("2026-05-26T00:03:00.000Z")
        .unwrap()
        .with_timezone(&Utc);
    let status = extension_status_from_file_contents(
        r#"{"lastSeenAt":"2026-05-26T00:00:00.000Z","lastMessageType":"get_state"}"#,
        checked_at,
        None,
    );

    assert!(!status.connected);
    assert_eq!(status.reason.as_deref(), Some("connection status is stale"));
}

#[test]
fn previous_app_session_extension_status_is_disconnected() {
    let checked_at = DateTime::parse_from_rfc3339("2026-05-26T00:01:00.000Z")
        .unwrap()
        .with_timezone(&Utc);
    let fresh_after = DateTime::parse_from_rfc3339("2026-05-26T00:00:30.000Z")
        .unwrap()
        .with_timezone(&Utc);
    let status = extension_status_from_file_contents(
        r#"{"lastSeenAt":"2026-05-26T00:00:05.000Z","lastMessageType":"get_state"}"#,
        checked_at,
        Some(fresh_after),
    );

    assert!(!status.connected);
    assert_eq!(
        status.reason.as_deref(),
        Some("connection is from an older app session")
    );
}

#[test]
fn an_oversized_extension_status_file_reads_as_disconnected_without_being_read_whole() {
    let dir = std::env::temp_dir().join(format!(
        "ganbaru-extension-status-bound-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("distractions-extension-status.json");
    let checked_at = DateTime::parse_from_rfc3339("2026-05-26T00:01:00.000Z")
        .unwrap()
        .with_timezone(&Utc);
    let record = r#"{"lastSeenAt":"2026-05-26T00:00:05.000Z","lastMessageType":"get_state"}"#;

    std::fs::write(&path, record).unwrap();
    assert!(
        extension_status_at(&path, checked_at, None)
            .unwrap()
            .connected
    );

    let mut oversized = record.as_bytes().to_vec();
    oversized.resize(2 * 1024 * 1024, b' ');
    std::fs::write(&path, oversized).unwrap();
    let status = extension_status_at(&path, checked_at, None).unwrap();
    assert!(!status.connected);
    assert_eq!(
        status.reason.as_deref(),
        Some("connection status file is invalid")
    );

    std::fs::remove_file(&path).unwrap();
    let status = extension_status_at(&path, checked_at, None).unwrap();
    assert_eq!(
        status.reason.as_deref(),
        Some("no extension connection has been recorded")
    );
    std::fs::remove_dir(dir).unwrap();
}
