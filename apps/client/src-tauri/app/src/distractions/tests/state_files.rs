use super::*;

#[test]
fn accepts_supported_phases() {
    for phase in ["inactive", "focus", "short_break", "long_break"] {
        assert!(validate_state(&state(phase)).is_ok());
    }
}

#[test]
fn rejects_unknown_phase() {
    assert!(validate_state(&state("planning")).is_err());
}

#[test]
fn accepted_focus_validity_expires_at_the_deadline_even_with_a_fresh_heartbeat() {
    let dir = std::env::temp_dir().join(format!("ganbaru-phase-expiry-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("distractions-state.json");
    let updated = "2026-05-26T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let mut phase = state("focus");
    phase.valid_until_ms = Some(updated.timestamp_millis() + 500);
    std::fs::write(&path, serde_json::to_vec(&phase).unwrap()).unwrap();
    assert!(
        read_fresh_runtime_state(&path, updated + chrono::Duration::milliseconds(499)).is_some()
    );
    assert!(
        read_fresh_runtime_state(&path, updated + chrono::Duration::milliseconds(500)).is_none()
    );
    phase.paused = true;
    phase.pause_reason = Some("manual".into());
    std::fs::write(&path, serde_json::to_vec(&phase).unwrap()).unwrap();
    assert!(
        read_fresh_runtime_state(&path, updated + chrono::Duration::milliseconds(500)).is_none()
    );
    phase.valid_until_ms = None;
    phase.paused = false;
    phase.pause_reason = None;
    phase.remaining_seconds = Some(1);
    std::fs::write(&path, serde_json::to_vec(&phase).unwrap()).unwrap();
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
        ownership_generation: 3,
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
    let running = crate::distractions::state_files::committed_focus_state(&effect, 1000).unwrap();
    assert!(running.active);
    assert_eq!(running.valid_until_ms, Some(1500));
    assert!(
        !crate::distractions::state_files::committed_focus_state(&effect, 1500)
            .unwrap()
            .active
    );
    effect.mode = FocusMode::ManualPause;
    effect.remaining_ms = 42_000;
    let paused = crate::distractions::state_files::committed_focus_state(&effect, 1000).unwrap();
    assert!(paused.active && paused.paused);
    assert_eq!(paused.pause_reason.as_deref(), Some("manual"));
    assert_eq!(paused.remaining_seconds, Some(42));
    assert_eq!(paused.valid_until_ms, Some(2000));
    for mode in [
        FocusMode::IdleFailed,
        FocusMode::ReturnWait,
        FocusMode::Expired,
        FocusMode::Stopped,
    ] {
        effect.mode = mode;
        let rules = crate::distractions::state_files::committed_focus_state(&effect, 1000).unwrap();
        assert!(!rules.active);
        assert_eq!(rules.phase, "inactive");
    }
    effect.mode = FocusMode::Running;
    effect.valid_until_ms = 0;
    assert!(
        !crate::distractions::state_files::committed_focus_state(&effect, 1000)
            .unwrap()
            .active
    );
}

#[test]
fn rejects_negative_remaining_seconds() {
    let mut state = state("focus");
    state.remaining_seconds = Some(-1);
    assert!(validate_state(&state).is_err());
}

#[test]
fn accepts_supported_pause_reasons() {
    for pause_reason in [None, Some("manual"), Some("idle"), Some("suspend")] {
        let mut state = state("focus");
        state.paused = pause_reason.is_some();
        state.pause_reason = pause_reason.map(ToOwned::to_owned);
        assert!(validate_state(&state).is_ok());
    }
}

#[test]
fn rejects_unknown_pause_reason() {
    let mut state = state("focus");
    state.paused = true;
    state.pause_reason = Some("network".to_string());
    assert!(validate_state(&state).is_err());
}

#[test]
fn clears_shutdown_enforcement_state_files() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ganbaru-ai-distractions-cleanup-{}-{suffix}",
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
    let database_path = dir.join("ganbaru-ai.sqlite");
    std::fs::write(&runtime_path, serde_json::to_vec(&state("focus")).unwrap()).unwrap();
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
        "ganbaru-ai-distractions-stale-limit-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("distractions-limit-state.json");
    let database_path = dir.join("ganbaru-ai.sqlite");
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
        "ganbaru-ai-distractions-atomic-{}",
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
fn oversized_authorization_config_is_rejected() {
    let path = std::env::temp_dir().join(format!(
        "ganbaru-ai-distractions-oversized-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &path,
        vec![b' '; MAX_AUTHORIZATION_CONFIG_BYTES as usize + 1],
    )
    .unwrap();

    assert_eq!(
        read_bounded_authorization_config(&path)
            .unwrap_err()
            .to_string(),
        "persisted distraction configuration is too large"
    );
    std::fs::remove_file(path).unwrap();
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
