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
    persistence::restore(&mut restarted, &saved, 20);
    assert_eq!(restarted.status, SessionStatus::Paused);
    assert_eq!(restarted.position_ms, 12_345);
    assert!(restarted.generation > state.generation);
    assert_eq!(restarted.issue, Some(SessionIssue::Restored));
    // A live owner returning from handoff must outrank its previous stop effect as well.
    let mut live = state.clone();
    live.generation = saved.generation + 100;
    let previous_generation = live.generation;
    persistence::restore(&mut live, &saved, 30);
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
