use super::*;
use ganbaru_pomodoro::FocusPhase;

fn effect() -> CommittedFocusEffect {
    CommittedFocusEffect {
        vault_id: "vault".into(),
        vault_generation: 7,
        ownership_generation: 3,
        execution_revision: 10,
        run_id: Some("run".into()),
        segment_id: Some("segment".into()),
        event_id: Some("event".into()),
        occurrence_id: Some("event::date".into()),
        event_title: None,
        phase: Some(FocusPhase::Focus),
        mode: FocusMode::Running,
        phase_deadline_ms: Some(50_000),
        event_deadline_ms: Some(100_000),
        remaining_ms: 40_000,
        valid_until_ms: 25_000,
    }
}

#[test]
fn native_focus_queued_effect_cannot_pass_a_newer_commit_or_vault_fence() {
    let candidate = effect();
    let mut current = candidate.clone();
    assert!(same_revision(&current, &candidate));
    current.execution_revision += 1;
    assert!(!same_revision(&current, &candidate));
    current.execution_revision = candidate.execution_revision;
    current.vault_generation += 1;
    assert!(!same_revision(&current, &candidate));
    current.vault_generation = candidate.vault_generation;
    current.ownership_generation += 1;
    assert!(!same_revision(&current, &candidate));
    current.ownership_generation = candidate.ownership_generation;
    current.vault_id = "other-vault".into();
    assert!(!same_revision(&current, &candidate));
}

#[test]
fn native_focus_authority_expires_monotonically_despite_civil_clock_rollback() {
    let start = Instant::now();
    let authority = EffectAuthority::new(effect(), 10_000, start);
    assert!(
        authority
            .current(10_000, start + Duration::from_millis(14_999))
            .is_some()
    );
    assert!(
        authority
            .current(1_000, start + Duration::from_secs(15))
            .is_none()
    );
    assert!(
        authority
            .current(25_000, start + Duration::from_secs(1))
            .is_none()
    );
}

#[test]
fn native_focus_phase_deadline_and_event_deadline_clip_the_publication_lease() {
    let start = Instant::now();
    let mut value = effect();
    value.phase_deadline_ms = Some(12_000);
    let authority = EffectAuthority::new(value.clone(), 10_000, start);
    assert!(
        authority
            .current(1_000, start + Duration::from_secs(2))
            .is_none()
    );
    value.mode = FocusMode::ManualPause;
    value.event_deadline_ms = Some(13_000);
    let authority = EffectAuthority::new(value, 10_000, start);
    assert!(
        authority
            .current(10_000, start + Duration::from_secs(2))
            .is_some()
    );
    assert!(
        authority
            .current(1_000, start + Duration::from_secs(3))
            .is_none()
    );
}

#[test]
fn native_focus_stop_revokes_music_while_active_transitions_preserve_policy_choice() {
    let candidate = effect();
    let mut current = candidate.clone();
    current.execution_revision += 1;
    assert!(allows_existing_lease(&current, &candidate));
    assert!(!same_revision(&current, &candidate));
    for mode in [
        FocusMode::ManualPause,
        FocusMode::IdlePause,
        FocusMode::Suspended,
    ] {
        current.mode = mode;
        assert!(allows_existing_lease(&current, &candidate));
    }
    current.mode = FocusMode::Running;
    current.segment_id = Some("new-break".into());
    current.phase = Some(FocusPhase::ShortBreak);
    assert!(allows_existing_lease(&current, &candidate));
    for mode in [
        FocusMode::Stopped,
        FocusMode::Expired,
        FocusMode::ReturnWait,
        FocusMode::IdleFailed,
    ] {
        current.mode = mode;
        assert!(!allows_existing_lease(&current, &candidate));
    }
    current.mode = FocusMode::Running;
    current.run_id = Some("another-run".into());
    assert!(!allows_existing_lease(&current, &candidate));
    current.run_id = candidate.run_id.clone();
    current.vault_generation += 1;
    assert!(!allows_existing_lease(&current, &candidate));
}

#[test]
fn queued_music_delivery_accepts_heartbeat_but_rejects_new_phase_pause_or_stop() {
    let candidate = effect();
    let mut current = candidate.clone();
    current.valid_until_ms += 1_000;
    assert!(allows_music_delivery(&current, &candidate));
    current.segment_id = Some("break-segment".into());
    assert!(!allows_music_delivery(&current, &candidate));
    current.segment_id = candidate.segment_id.clone();
    current.phase = Some(FocusPhase::ShortBreak);
    assert!(!allows_music_delivery(&current, &candidate));
    current.phase = candidate.phase;
    current.mode = FocusMode::ManualPause;
    assert!(!allows_music_delivery(&current, &candidate));
    current.mode = FocusMode::Stopped;
    assert!(!allows_music_delivery(&current, &candidate));
    // Pause and resume can restore the same phase/mode. Their durable revision
    // still prevents a pre-pause queued start from becoming current again.
    current.mode = candidate.mode;
    current.execution_revision += 2;
    assert!(!allows_music_delivery(&current, &candidate));
}

#[test]
fn native_focus_current_authority_coalesces_and_freeze_clears_pending_consumers() {
    let start = Instant::now();
    let candidate = effect();
    let (sender, receiver) =
        tokio::sync::watch::channel(Some(EffectAuthority::new(candidate.clone(), 10_000, start)));
    let mut stopped = candidate.clone();
    stopped.execution_revision += 1;
    stopped.mode = FocusMode::Stopped;
    sender.send_replace(Some(EffectAuthority::new(stopped, 10_000, start)));
    {
        let current = receiver.borrow();
        let current = current.as_ref().unwrap().current(10_000, start).unwrap();
        assert!(!same_revision(current, &candidate));
        assert!(!allows_existing_lease(current, &candidate));
    }
    sender.send_replace(None);
    assert!(receiver.borrow().is_none());
}
