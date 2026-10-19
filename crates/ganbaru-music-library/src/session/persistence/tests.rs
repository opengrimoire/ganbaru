use crate::session::{models::*, persistence, policy::*, queue};
use crate::{MusicListeningOutcome, MusicListeningUpdate, MusicSelectionKind};
use sqlx::SqlitePool;
use std::collections::BTreeMap;

async fn pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::run_migrations(&pool).await.unwrap();
    sqlx::query("INSERT INTO music_library_items (id, identity_key, source_kind, original_title, availability, review_state, discovered_at_ms, updated_at_ms) VALUES ('item', 'portable:árbol', 'local-file', 'Árbol', 'available', 'unreviewed', 1, 1)")
        .execute(&pool).await.unwrap();
    pool
}

fn transition() -> Transition {
    Transition {
        effects: Vec::new(),
        changed: true,
        listening: vec![MusicListeningUpdate {
            playlist_id: None,
            item_id: "item".into(),
            selection_kind: MusicSelectionKind::Manual,
            outcome: MusicListeningOutcome::Started,
            occurred_at: 10,
        }],
    }
}

#[tokio::test]
async fn native_music_receipt_failure_rolls_back_checkpoint_and_listening() {
    let pool = pool().await;
    sqlx::raw_sql("CREATE TRIGGER fail_receipt BEFORE INSERT ON music_session_receipts BEGIN SELECT RAISE(ABORT, 'receipt fixture failure'); END")
        .execute(&pool).await.unwrap();
    let state = SessionPolicy::new("session".into(), 1);
    assert!(
        persistence::commit(
            &pool,
            "device",
            &state,
            &transition(),
            Some(("action", "hash")),
            10
        )
        .await
        .is_err()
    );
    assert!(
        persistence::load_checkpoint(&pool, "device")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM music_listening_statistics")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    sqlx::raw_sql("DROP TRIGGER fail_receipt")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        persistence::commit(
            &pool,
            "device",
            &state,
            &transition(),
            Some(("action", "hash")),
            10
        )
        .await
        .unwrap()
    );
    assert!(
        !persistence::commit(
            &pool,
            "device",
            &state,
            &transition(),
            Some(("action", "hash")),
            10
        )
        .await
        .unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT play_count FROM music_listening_statistics WHERE item_id = 'item'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    assert!(
        persistence::commit(
            &pool,
            "device",
            &state,
            &transition(),
            Some(("action", "different")),
            10
        )
        .await
        .is_err()
    );
    pool.close().await;
}

#[tokio::test]
async fn native_music_recovery_is_device_scoped_and_excludes_resolved_paths() {
    let pool = pool().await;
    let mut state = SessionPolicy::new("session".into(), 1);
    let definition = SessionQueueIntent::LibraryItems {
        item_ids: vec!["item".into()],
        selected_item_id: Some("item".into()),
        name: "Local".into(),
    };
    let mut transaction = pool.begin().await.unwrap();
    queue::load_queue(
        &mut transaction,
        &BTreeMap::new(),
        &definition,
        &mut state,
        10,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let entry = &mut std::sync::Arc::make_mut(&mut state.queue)[0];
    entry.source.path = Some("/private/device/music/árbol.flac".into());
    entry.source.original_input = "content://private-device-document".into();
    entry.bound = true;
    state.initial(Some("item"), None, true, 10);
    state.position_ms = 12_345;
    persistence::commit(&pool, "device-a", &state, &Transition::default(), None, 10)
        .await
        .unwrap();
    assert!(
        persistence::load_checkpoint(&pool, "device-b")
            .await
            .unwrap()
            .is_none()
    );
    let saved = persistence::load_checkpoint(&pool, "device-a")
        .await
        .unwrap()
        .unwrap();
    let encoded = serde_json::to_string(&saved).unwrap();
    assert!(!encoded.contains("/private") && !encoded.contains("content://"));
    let mut restarted = state.clone();
    persistence::restore(
        &mut restarted,
        &saved,
        persistence::RestoreSelection::Saved,
        20,
    );
    assert_eq!(restarted.status, SessionStatus::Paused);
    assert_eq!(restarted.position_ms, 12_345);
    assert!(restarted.generation > state.generation);
    assert_eq!(restarted.issue, Some(SessionIssue::Restored));
    // A live owner returning from handoff must outrank its previous stop effect as well.
    let mut live = state.clone();
    live.generation = saved.generation + 100;
    let previous_generation = live.generation;
    persistence::restore(&mut live, &saved, persistence::RestoreSelection::Saved, 30);
    assert!(live.generation > previous_generation);
    pool.close().await;
}

#[tokio::test]
async fn native_music_canonical_queue_rejects_unknown_duplicates_and_excess_work() {
    let pool = pool().await;
    let mut state = SessionPolicy::new("session".into(), 1);
    let mut transaction = pool.begin().await.unwrap();
    for ids in [
        vec!["missing".into()],
        vec!["item".into(), "item".into()],
        vec!["item".into(); MAX_QUEUE_ENTRIES + 1],
    ] {
        let definition = SessionQueueIntent::LibraryItems {
            item_ids: ids,
            selected_item_id: None,
            name: "Queue".into(),
        };
        assert!(
            queue::load_queue(
                &mut transaction,
                &BTreeMap::new(),
                &definition,
                &mut state,
                10
            )
            .await
            .is_err()
        );
    }
    let definition = SessionQueueIntent::LibraryItems {
        item_ids: vec!["item".into()],
        selected_item_id: None,
        name: "Queue".into(),
    };
    queue::load_queue(
        &mut transaction,
        &BTreeMap::new(),
        &definition,
        &mut state,
        10,
    )
    .await
    .unwrap();
    assert_eq!(state.queue[0].source.title, "Árbol");
    assert!(!state.queue[0].bound);
    assert!(state.eligible(10).is_empty());
    transaction.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn native_music_queue_sources_prefer_edited_artist_and_album_over_tags() {
    let pool = pool().await;
    sqlx::query("UPDATE music_library_items SET original_artist = 'Tag artist', original_album = 'Tag album', artist_override = 'Edited artist', album_override = '' WHERE id = 'item'")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO music_library_items (id, identity_key, source_kind, original_title, availability, review_state, discovered_at_ms, updated_at_ms) VALUES ('untagged', 'portable:untagged', 'local-file', 'Untagged', 'available', 'unreviewed', 1, 1)")
        .execute(&pool).await.unwrap();
    let mut state = SessionPolicy::new("session".into(), 1);
    let definition = SessionQueueIntent::LibraryItems {
        item_ids: vec!["item".into(), "untagged".into()],
        selected_item_id: None,
        name: "Queue".into(),
    };
    let mut transaction = pool.begin().await.unwrap();
    queue::load_queue(
        &mut transaction,
        &BTreeMap::new(),
        &definition,
        &mut state,
        10,
    )
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    let tagged = &state.queue[0].source;
    assert_eq!(tagged.artist.as_deref(), Some("Edited artist"));
    // An empty override falls back to the tag rather than blanking it.
    assert_eq!(tagged.album.as_deref(), Some("Tag album"));
    // Empty tags stay absent so the decoder's own file metadata can show.
    let untagged = &state.queue[1].source;
    assert_eq!(
        (untagged.artist.as_deref(), untagged.album.as_deref()),
        (None, None)
    );
    pool.close().await;
}

fn queued(count: usize, order: PlaybackOrder) -> SessionPolicy {
    let mut state = SessionPolicy::new("session".into(), 17);
    state.queue = std::sync::Arc::new(
        (0..count)
            .map(|index| SessionQueueEntry {
                item_id: Some(format!("item-{index}")),
                membership_id: Some(format!("member-{index}")),
                source: SessionSource {
                    kind: SourceKind::LocalFile,
                    identity: format!("local:{index}"),
                    original_input: String::new(),
                    title: format!("Track {index}"),
                    artist: None,
                    album: None,
                    path: Some(format!("/music/{index}.flac")),
                    artwork_path: None,
                    video_id: None,
                    playlist_id: None,
                    start_ms: None,
                    end_ms: None,
                },
                backend: SessionBackend::NativeAudio,
                availability: crate::MusicItemAvailability::Available,
                enabled: true,
                weight: crate::MusicWeight::Normal,
                snoozed_until: None,
                snoozed_indefinitely: false,
                embedding_blocked: false,
                bound: true,
                phase_allowed: true,
                skip_ranges: Vec::new(),
                volume: None,
                rate: None,
            })
            .collect(),
    );
    state.definition = Some(SessionQueueIntent::LibraryItems {
        item_ids: (0..count).map(|index| format!("item-{index}")).collect(),
        selected_item_id: None,
        name: "Queue".into(),
    });
    state.order = order;
    state
}

/// Saves `state` and restores it into a fresh copy of the same canonical queue.
fn relaunch(
    state: &SessionPolicy,
    selection: persistence::RestoreSelection,
) -> (SessionPolicy, Transition) {
    let saved = persistence::checkpoint(state);
    let mut restored = SessionPolicy::new("fresh".into(), 3);
    restored.queue = state.queue.clone();
    restored.definition = state.definition.clone();
    let transition = persistence::restore(&mut restored, &saved, selection, 500);
    (restored, transition)
}

fn loaded_index(state: &SessionPolicy, transition: &Transition) -> Option<usize> {
    transition.effects.iter().find_map(|effect| match effect {
        SessionEffect::Load {
            source, autoplay, ..
        } => {
            assert!(!autoplay, "a restored selection never starts by itself");
            state
                .queue
                .iter()
                .position(|entry| entry.source.identity == source.identity)
        }
        _ => None,
    })
}

#[test]
fn native_music_relaunch_continues_in_order_on_the_next_track_from_its_start() {
    let mut state = queued(3, PlaybackOrder::InOrder);
    state.initial(Some("item-1"), None, true, 100);
    state.position_ms = 42_000;
    let (restored, transition) = relaunch(&state, persistence::RestoreSelection::Next);
    assert_eq!(restored.current, Some(2));
    assert_eq!(loaded_index(&restored, &transition), Some(2));
    assert_eq!(restored.position_ms, 0);
    assert_eq!(restored.status, SessionStatus::Paused);
    assert_eq!(restored.issue, Some(SessionIssue::Restored));
    // Previous returns to the track that was playing when the app closed.
    assert_eq!(restored.history.last(), Some(&1));
}

#[test]
fn native_music_relaunch_after_the_last_in_order_track_starts_the_queue_over() {
    let mut state = queued(3, PlaybackOrder::InOrder);
    state.initial(Some("item-2"), None, true, 100);
    let (restored, transition) = relaunch(&state, persistence::RestoreSelection::Next);
    assert_eq!(restored.current, Some(0));
    assert_eq!(loaded_index(&restored, &transition), Some(0));
}

#[test]
fn native_music_relaunch_continues_the_saved_shuffle_pass() {
    let mut state = queued(5, PlaybackOrder::Shuffle);
    state.initial(None, None, true, 100);
    let left = state.current.unwrap();
    let remaining = state.shuffle.clone();
    let (restored, transition) = relaunch(&state, persistence::RestoreSelection::Next);
    assert_eq!(restored.current, Some(remaining[0]));
    assert_eq!(loaded_index(&restored, &transition), Some(remaining[0]));
    assert_eq!(restored.shuffle, remaining[1..]);
    assert_eq!(restored.history.last(), Some(&left));
}

#[test]
fn native_music_relaunch_after_a_finished_shuffle_pass_starts_a_new_one_elsewhere() {
    let mut state = queued(4, PlaybackOrder::Shuffle);
    state.initial(None, None, true, 100);
    for _ in 0..3 {
        state.apply(SessionIntent::Next, 200);
    }
    assert!(state.shuffle.is_empty());
    let left = state.current.unwrap();
    let (restored, _) = relaunch(&state, persistence::RestoreSelection::Next);
    assert_ne!(restored.current, Some(left));
    assert_eq!(restored.shuffle.len(), 2);
    assert!(!restored.shuffle.contains(&left));
}

#[test]
fn native_music_relaunch_with_one_eligible_track_reselects_it_from_its_start() {
    let mut state = queued(1, PlaybackOrder::Mix);
    state.initial(Some("item-0"), None, true, 100);
    state.position_ms = 30_000;
    let (restored, transition) = relaunch(&state, persistence::RestoreSelection::Next);
    assert_eq!(restored.current, Some(0));
    assert_eq!(loaded_index(&restored, &transition), Some(0));
    assert_eq!(restored.position_ms, 0);
}

#[test]
fn native_music_reopened_context_keeps_the_saved_track_and_position() {
    let mut state = queued(3, PlaybackOrder::InOrder);
    state.initial(Some("item-1"), None, true, 100);
    state.position_ms = 42_000;
    let (restored, transition) = relaunch(&state, persistence::RestoreSelection::Saved);
    assert_eq!(restored.current, Some(1));
    assert_eq!(loaded_index(&restored, &transition), Some(1));
    assert_eq!(restored.position_ms, 42_000);
}

#[test]
fn native_music_restored_browser_selection_reloads_once_its_host_is_available() {
    for selection in [
        persistence::RestoreSelection::Next,
        persistence::RestoreSelection::Saved,
    ] {
        let mut state = queued(2, PlaybackOrder::InOrder);
        for entry in std::sync::Arc::make_mut(&mut state.queue) {
            entry.backend = SessionBackend::Browser;
        }
        state.browser_host = true;
        state.initial(Some("item-0"), None, true, 100);
        // The restore-time load is published before any WebView hosts browser media.
        let (mut restored, _) = relaunch(&state, selection);
        assert!(!restored.browser_host);
        let host = restored.apply(SessionIntent::BrowserHost { available: true }, 600);
        assert!(
            matches!(
                host.effects.as_slice(),
                [SessionEffect::Load { autoplay: false, generation, .. }] if *generation == restored.generation
            ),
            "{selection:?}"
        );
        assert_eq!(restored.issue, None);
        let play = restored.apply(SessionIntent::Play, 700);
        assert_eq!(
            play.effects,
            vec![SessionEffect::Play {
                generation: restored.generation
            }]
        );
    }
}
