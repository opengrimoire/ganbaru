use super::models::*;
use super::policy::*;
use crate::{MusicItemAvailability, MusicListeningOutcome, MusicRepeatMode, MusicWeight};
use std::sync::Arc;

fn entry(index: usize) -> SessionQueueEntry {
    SessionQueueEntry {
        item_id: Some(format!("item-{index}")),
        membership_id: Some(format!("member-{index}")),
        source: SessionSource {
            kind: SourceKind::LocalFile,
            identity: format!("local:{index}"),
            original_input: String::new(),
            title: format!("Track {index}"),
            path: Some(format!("/music/{index}.flac")),
            artwork_path: None,
            video_id: None,
            playlist_id: None,
            start_ms: None,
            end_ms: None,
        },
        backend: SessionBackend::NativeAudio,
        availability: MusicItemAvailability::Available,
        enabled: true,
        weight: MusicWeight::Normal,
        snoozed_until: None,
        snoozed_indefinitely: false,
        embedding_blocked: false,
        bound: true,
        phase_allowed: true,
        skip_ranges: Vec::new(),
        volume: None,
        rate: None,
    }
}

fn session(count: usize) -> SessionPolicy {
    let mut session = SessionPolicy::new("session".to_string(), 17);
    session.queue = Arc::new((0..count).map(entry).collect());
    session.order = PlaybackOrder::InOrder;
    session
}

#[cfg(desktop)]
#[test]
fn native_music_activation_failure_does_not_interrupt_playback_or_automatic_queue_progress() {
    for error in [
        super::policy::ContextFailure::Calendar("Calendar snapshot unavailable".into()),
        super::policy::ContextFailure::Soundscape("Background source unavailable".into()),
    ] {
        let mut state = session(2);
        state.initial(None, None, true, 100);
        state.apply(observe(&state, 1, SessionStatus::Playing, 10), 110);
        state.context_error = Some(error);
        let snapshot = state.projection(false, 111);
        assert_eq!(snapshot.status, SessionStatus::Playing);
        assert!(snapshot.error.is_some());
        let transition = state.apply(observe(&state, 2, SessionStatus::Ended, 100), 120);
        assert_eq!(state.current, Some(1));
        assert!(
            transition
                .effects
                .iter()
                .any(|effect| matches!(effect, SessionEffect::Load { autoplay: true, .. }))
        );
        assert!(state.projection(false, 121).error.is_some());
    }
}

fn observe(
    session: &SessionPolicy,
    sequence: u64,
    status: SessionStatus,
    position_ms: u64,
) -> SessionIntent {
    SessionIntent::Observe {
        observation: SessionObservation {
            session_id: session.session_id.clone(),
            generation: session.generation,
            sequence,
            source_identity: session.current_entry().unwrap().source.identity.clone(),
            status,
            position_ms,
            duration_ms: Some(60_000),
            error: None,
        },
    }
}

#[test]
fn unknown_android_interruption_reasons_are_rejected_before_session_mutation() {
    let known: AndroidInterruption = serde_json::from_str("\"source-resolution-timeout\"").unwrap();
    assert!(known.message().contains("document resolution"));
    for value in [
        "\"service_stopped\"",
        "\"\"",
        "\"source-failed\"",
        "null",
        "{}",
        "1",
    ] {
        assert!(
            serde_json::from_str::<AndroidInterruption>(value).is_err(),
            "{value}"
        );
    }
}

#[test]
fn late_playing_observations_cannot_reverse_pause_or_open_a_silent_selection() {
    for initially_playing in [true, false] {
        let mut state = session(2);
        state.initial(Some("item-0"), None, initially_playing, 100);
        if initially_playing {
            state.apply(SessionIntent::Pause, 200);
        }
        let manual_revision = state.manual_revision;
        let transition = state.apply(observe(&state, 1, SessionStatus::Playing, 1000), 300);
        assert_eq!(state.status, SessionStatus::Paused);
        assert!(!state.autoplay_requested);
        assert!(transition.listening.is_empty());
        assert_eq!(
            transition.effects,
            vec![SessionEffect::Pause {
                generation: state.generation
            }]
        );
        assert_eq!(state.manual_revision, manual_revision);
    }
}

#[test]
fn late_completion_or_source_failure_prepares_the_next_item_without_reversing_pause() {
    for status in [SessionStatus::Ended, SessionStatus::Error] {
        let mut state = session(2);
        state.initial(Some("item-0"), None, true, 100);
        state.apply(observe(&state, 1, SessionStatus::Playing, 0), 150);
        state.apply(SessionIntent::Pause, 200);
        let transition = state.apply(observe(&state, 2, status, 60_000), 300);
        assert_eq!(state.current, Some(1));
        assert!(!state.autoplay_requested);
        assert_eq!(state.status, SessionStatus::Ready);
        assert!(matches!(
            transition.effects.as_slice(),
            [SessionEffect::Load {
                autoplay: false,
                ..
            }]
        ));
        assert_eq!(transition.listening.len(), 1);
        assert_eq!(
            transition.listening[0].outcome,
            if status == SessionStatus::Ended {
                MusicListeningOutcome::Completed
            } else {
                MusicListeningOutcome::Skipped
            }
        );
        let explicit = state.apply(SessionIntent::Play, 400);
        assert!(state.autoplay_requested);
        assert!(matches!(
            explicit.effects.as_slice(),
            [SessionEffect::Play { .. }]
        ));
    }
}

#[test]
fn native_music_repeat_one_applies_to_automatic_end_but_not_explicit_next() {
    let mut state = session(3);
    state.repeat_mode = MusicRepeatMode::One;
    state.initial(Some("item-0"), None, true, 100);
    state.apply(observe(&state, 1, SessionStatus::Playing, 0), 100);
    let ended = state.apply(observe(&state, 2, SessionStatus::Ended, 60_000), 200);
    assert_eq!(state.current, Some(0));
    assert_eq!(ended.listening[0].outcome, MusicListeningOutcome::Completed);
    state.apply(SessionIntent::Next, 300);
    assert_eq!(state.current, Some(1));
}

#[test]
fn native_music_ignores_late_source_session_and_sequence_observations() {
    let mut state = session(2);
    state.initial(Some("item-0"), None, true, 100);
    let stale = observe(&state, 10, SessionStatus::Ended, 60_000);
    state.apply(SessionIntent::Next, 200);
    assert!(!state.apply(stale, 300).changed);
    state.apply(observe(&state, 4, SessionStatus::Playing, 1_000), 400);
    assert!(
        !state
            .apply(observe(&state, 3, SessionStatus::Paused, 0), 500)
            .changed
    );
    assert_eq!(state.position_ms, 1_000);
}

#[test]
fn native_service_loss_keeps_the_selection_without_skipping_or_changing_manual_ownership() {
    let mut state = session(3);
    state.initial(Some("item-0"), None, true, 100);
    state.apply(observe(&state, 1, SessionStatus::Playing, 12_000), 200);
    state.manual_revision = 4;
    let old = state.clone();
    let lost = state.interrupt_backend();
    assert_eq!(state.current, old.current);
    assert_eq!(state.position_ms, 12_000);
    assert_eq!(state.manual_revision, 4);
    assert_eq!(state.status, SessionStatus::Paused);
    assert_eq!(state.issue, Some(SessionIssue::Interrupted));
    assert!(!state.autoplay_requested);
    assert!(state.failed.is_empty());
    assert!(lost.effects.is_empty());
    assert!(lost.listening.is_empty());
    let late = state.apply(observe(&old, 2, SessionStatus::Ended, 60_000), 300);
    assert!(!late.changed);
    assert_eq!(state.current, Some(0));
    assert_eq!(state.position_ms, 12_000);
}

#[test]
fn native_service_reconnection_reloads_the_same_position_before_explicit_play_and_fences_old_events()
 {
    let mut state = session(3);
    state.initial(Some("item-0"), None, true, 100);
    state.apply(observe(&state, 20, SessionStatus::Playing, 12_000), 200);
    state.interrupt_backend();
    let old = state.clone();
    let reload = state.reconnect_backend();
    assert!(
        matches!(&reload.effects[..], [SessionEffect::Load { source, position_ms: 12_000, autoplay: false, .. }] if source.identity == "local:0")
    );
    assert!(reload.listening.is_empty());
    assert_eq!(state.generation, old.generation + 1);
    assert_eq!(state.last_sequence, 0);
    assert_eq!(state.current, old.current);
    assert_eq!(state.manual_revision, old.manual_revision);
    assert!(state.issue.is_none());
    assert!(state.error.is_none());
    assert!(!state.autoplay_requested);
    assert!(
        !state
            .apply(observe(&old, 100, SessionStatus::Ended, 60_000), 300)
            .changed
    );
    let play = state.apply(SessionIntent::Play, 400);
    assert!(matches!(&play.effects[..], [SessionEffect::Play { .. }]));
    assert!(state.autoplay_requested);
    state.apply(observe(&state, 1, SessionStatus::Playing, 12_010), 500);
    assert_eq!(state.last_sequence, 1);
    assert_eq!(state.current, Some(0));
}

#[test]
fn native_music_failed_sources_do_not_repeat_or_count_late_failures_twice() {
    let mut state = session(2);
    state.repeat_mode = MusicRepeatMode::One;
    state.initial(Some("item-0"), None, true, 100);
    state.apply(observe(&state, 1, SessionStatus::Playing, 500), 150);
    let failure = observe(&state, 2, SessionStatus::Error, 500);
    let first = state.apply(failure.clone(), 200);
    assert_eq!(state.current, Some(1));
    assert!(state.failed.contains(&0));
    assert_eq!(first.listening[0].outcome, MusicListeningOutcome::Skipped);
    let late = state.apply(failure, 201);
    assert!(!late.changed);
    assert!(late.listening.is_empty());
    let final_failure = state.apply(observe(&state, 1, SessionStatus::Error, 0), 300);
    assert!(state.eligible(300).is_empty());
    assert_eq!(state.issue, Some(SessionIssue::NoEligibleItems));
    // A decoder that fails before playing contributes no invented listening evidence.
    assert!(final_failure.listening.is_empty());
    let generation = state.generation;
    let retry = state.apply(SessionIntent::Play, 400);
    assert_eq!(state.current, Some(1));
    assert!(state.generation > generation);
    assert!(!state.failed.contains(&1));
    assert!(
        retry
            .effects
            .iter()
            .any(|effect| matches!(effect, SessionEffect::Load { autoplay: true, .. }))
    );
}

#[test]
fn native_music_executes_skip_and_membership_end_without_frontend_polling() {
    let mut state = session(2);
    Arc::make_mut(&mut state.queue)[0].source.start_ms = Some(1_000);
    Arc::make_mut(&mut state.queue)[0].source.end_ms = Some(30_000);
    Arc::make_mut(&mut state.queue)[0].skip_ranges = vec![SessionSkipRange {
        start_ms: 10_000,
        end_ms: 15_000,
    }];
    state.initial(Some("item-0"), None, true, 100);
    assert_eq!(state.position_ms, 1_000);
    let skipped = state.apply(observe(&state, 1, SessionStatus::Playing, 12_000), 200);
    assert!(skipped.effects.iter().any(|effect| matches!(
        effect,
        SessionEffect::Seek {
            position_ms: 15_000,
            ..
        }
    )));
    let repeated = state.apply(observe(&state, 2, SessionStatus::Playing, 12_000), 201);
    assert!(repeated.effects.is_empty());
    let completed = state.apply(observe(&state, 3, SessionStatus::Playing, 30_000), 300);
    assert_eq!(state.current, Some(1));
    assert_eq!(
        completed.listening[0].outcome,
        MusicListeningOutcome::Completed
    );
}

#[test]
fn native_music_snooze_expiry_and_explicit_selection_keep_disabled_protection() {
    let mut state = session(2);
    Arc::make_mut(&mut state.queue)[0].snoozed_until = Some(500);
    assert_eq!(state.eligible(100), vec![1]);
    assert_eq!(state.eligible(500), vec![0, 1]);
    state.initial(Some("item-0"), None, true, 100);
    assert_eq!(state.current, Some(0));
    Arc::make_mut(&mut state.queue)[0].enabled = false;
    assert_eq!(state.skip_reason(0, true, 100), Some(SkipReason::Disabled));
}

#[test]
fn native_music_shuffle_pass_never_repeats_and_respects_repeat_off() {
    let mut state = session(5);
    state.order = PlaybackOrder::Shuffle;
    state.initial(None, None, true, 100);
    let mut seen = std::collections::HashSet::from([state.current.unwrap()]);
    for _ in 0..4 {
        state.apply(SessionIntent::Next, 200);
        assert!(seen.insert(state.current.unwrap()));
    }
    state.apply(SessionIntent::Next, 300);
    assert_eq!(state.status, SessionStatus::Paused);
    assert_eq!(seen.len(), 5);
}

#[test]
fn native_music_mix_matches_shared_selection_fixtures() {
    #[derive(serde::Deserialize)]
    struct Fixture {
        weights: Vec<MusicWeight>,
        recent: Vec<usize>,
        draw: f64,
        expected: usize,
    }
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/client/src/lib/music/music-queue-policy-fixtures.json"
    )))
    .unwrap();
    for fixture in fixtures {
        let mut state = session(fixture.weights.len());
        for (entry, weight) in Arc::make_mut(&mut state.queue)
            .iter_mut()
            .zip(fixture.weights)
        {
            entry.weight = weight;
        }
        let recent: Vec<_> = fixture
            .recent
            .into_iter()
            .map(|index| state.queue[index].source.identity.clone())
            .collect();
        assert_eq!(
            select_mix(&state.queue, &state.eligible(100), &recent, fixture.draw),
            Some(fixture.expected)
        );
    }
}

#[test]
fn native_music_preserves_browser_selection_when_its_host_is_unavailable() {
    let mut state = session(3);
    Arc::make_mut(&mut state.queue)[1].backend = SessionBackend::Browser;
    state.initial(Some("item-0"), None, true, 100);
    state.apply(SessionIntent::Next, 200);
    assert_eq!(state.current, Some(1));
    assert_eq!(state.status, SessionStatus::Paused);
    assert_eq!(state.issue, Some(SessionIssue::BrowserHostUnavailable));
    state.apply(SessionIntent::BrowserHost { available: true }, 300);
    assert_eq!(state.current, Some(1));
    assert_eq!(state.status, SessionStatus::Ready);
}

#[test]
fn native_music_review_restore_uses_its_native_checkpoint_once() {
    let mut state = session(2);
    state.initial(Some("item-0"), None, true, 100);
    state.apply(observe(&state, 1, SessionStatus::Playing, 8_000), 200);
    state.apply(SessionIntent::SuspendReview, 300);
    let checkpoint_id = state.review_checkpoint_id.clone().unwrap();
    state.queue = Arc::new(vec![entry(99)]);
    state.current = Some(0);
    state.apply(
        SessionIntent::RestoreReview {
            checkpoint_id: checkpoint_id.clone(),
        },
        400,
    );
    assert_eq!(
        state.current_entry().unwrap().item_id.as_deref(),
        Some("item-0")
    );
    assert_eq!(state.position_ms, 8_000);
    assert!(
        !state
            .apply(SessionIntent::RestoreReview { checkpoint_id }, 500)
            .changed
    );
}

#[test]
fn native_music_membership_settings_and_manual_adjustment_follow_the_selected_item() {
    let mut state = session(2);
    Arc::make_mut(&mut state.queue)[0].volume = Some(0.3);
    Arc::make_mut(&mut state.queue)[0].rate = Some(1.5);
    state.initial(Some("item-0"), None, true, 100);
    assert_eq!(state.projection(false, 100).volume, 0.3);
    assert_eq!(state.projection(false, 100).rate, 1.5);
    state.apply(SessionIntent::Volume { volume: 0.6 }, 200);
    assert_eq!(state.projection(false, 200).volume, 0.6);
    state.apply(SessionIntent::Next, 300);
    assert_eq!(state.projection(false, 300).volume, 0.6);
    assert_eq!(state.projection(false, 300).rate, 1.0);
}

#[test]
fn native_music_refresh_remaps_history_and_advances_newly_snoozed_current_item() {
    let mut state = session(3);
    state.initial(Some("item-0"), None, true, 100);
    state.apply(SessionIntent::Next, 200);
    state.apply(observe(&state, 1, SessionStatus::Playing, 400), 300);
    let mut reordered = state.clone();
    Arc::make_mut(&mut reordered.queue).swap(0, 2);
    reordered.reconcile_queue(&state, 300);
    assert_eq!(reordered.history, vec![2]);
    assert_eq!(reordered.current, Some(1));
    let mut snoozed = reordered.clone();
    Arc::make_mut(&mut snoozed.queue)[1].snoozed_until = Some(1_000);
    let transition = snoozed.reconcile_queue(&reordered, 400);
    assert_eq!(snoozed.current, Some(2));
    assert_eq!(
        transition.listening[0].outcome,
        MusicListeningOutcome::Skipped
    );
    assert_eq!(transition.listening[0].item_id, "item-1");
}
