use super::*;

fn lease(start: Instant) -> FocusLease {
    let effect = CommittedFocusEffect {
        vault_id: "vault".into(),
        vault_generation: 7,
        ownership_generation: 3,
        execution_revision: 10,
        run_id: Some("run".into()),
        segment_id: Some("segment".into()),
        event_id: Some("event".into()),
        occurrence_id: Some("occurrence".into()),
        event_title: None,
        phase: Some(FocusPhase::Focus),
        mode: FocusMode::Running,
        phase_deadline_ms: Some(50_000),
        event_deadline_ms: Some(100_000),
        remaining_ms: 40_000,
        valid_until_ms: 25_000,
    };
    FocusLease {
        expires_at: lease_deadline(&effect, 10_000, start),
        effect,
        activation_key: "run:segment:event".into(),
        manual_revision: 4,
        paused_by_focus: false,
        applied: true,
        expired: false,
    }
}

#[test]
fn native_music_duplicate_focus_delivery_cannot_restart_monotonic_validity() {
    let start = Instant::now();
    let mut lease = lease(start);
    let duplicate = lease.effect.clone();
    lease.renew(&duplicate, 1_000, start + Duration::from_secs(10));
    assert_eq!(lease.expires_at, start + Duration::from_secs(15));
    let mut older = duplicate.clone();
    older.valid_until_ms -= 5_000;
    lease.renew(&older, 1_000, start + Duration::from_secs(14));
    assert_eq!(lease.effect.valid_until_ms, 25_000);
    assert_eq!(lease.expires_at, start + Duration::from_secs(15));
    let mut renewal = duplicate;
    renewal.valid_until_ms = 35_000;
    lease.renew(&renewal, 20_000, start + Duration::from_secs(10));
    assert_eq!(lease.expires_at, start + Duration::from_secs(25));
}

#[test]
fn native_music_delayed_focus_delivery_retains_only_the_remaining_phase_allowance() {
    let start = Instant::now();
    let lease = lease(start);
    assert_eq!(
        lease_deadline(&lease.effect, 24_000, start),
        start + Duration::from_secs(1)
    );
    assert_eq!(lease_deadline(&lease.effect, 25_000, start), start);
    let mut effect = lease.effect;
    effect.valid_until_ms = 65_000;
    assert_eq!(
        lease_deadline(&effect, 45_000, start),
        start + Duration::from_secs(5)
    );
    effect.mode = FocusMode::ManualPause;
    effect.event_deadline_ms = Some(48_000);
    assert_eq!(
        lease_deadline(&effect, 45_000, start),
        start + Duration::from_secs(3)
    );
    // Even a rolled-back observation cannot create an excessive monotonic lease.
    assert_eq!(
        lease_deadline(&effect, i64::MIN, start),
        start + Duration::from_secs(15)
    );
}
