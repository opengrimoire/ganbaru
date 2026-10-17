use super::*;

#[tokio::test]
async fn native_music_background_assignment_and_selection_roll_back_with_the_soundtrack_transaction()
 {
    let pool = super::super::context::test_pool().await;
    let id: String =
        sqlx::query_scalar("SELECT id FROM music_soundscapes WHERE generated_kind = 'brown'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let before: (i64, bool, Option<String>) = sqlx::query_as(
        "SELECT version, desired_playing, active_soundscape_id FROM music_soundscape_state",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let prepared = prepare(&mut transaction, "device", "play-selected", Some(&id))
        .await
        .unwrap();
    assert!(!prepared.missing);
    assert_eq!(
        prepared
            .action
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .source_id,
        id
    );
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT automatic_intent FROM music_soundscape_state")
            .fetch_one(&mut *transaction)
            .await
            .unwrap()
    );
    transaction.rollback().await.unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT automatic_intent FROM music_soundscape_state")
            .fetch_one(&pool)
            .await
            .unwrap()
    );
    let after: (i64, bool, Option<String>) = sqlx::query_as(
        "SELECT version, desired_playing, active_soundscape_id FROM music_soundscape_state",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after, before);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM music_soundscape_active_selections")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn native_music_background_pause_preserves_selection_and_keep_does_not_mutate_state() {
    let pool = super::super::context::test_pool().await;
    let id: String =
        sqlx::query_scalar("SELECT id FROM music_soundscapes WHERE generated_kind = 'white'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    let playing = prepare(&mut transaction, "device", "play-selected", Some(&id))
        .await
        .unwrap();
    let kept = prepare(&mut transaction, "device", "keep-current-soundscape", None)
        .await
        .unwrap();
    assert!(kept.action.is_none());
    let paused = prepare(&mut transaction, "device", "pause-soundscape", None)
        .await
        .unwrap();
    assert!(matches!(paused.action, Some(None)));
    assert_eq!(paused.version, playing.version.map(|version| version + 1));
    transaction.commit().await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT automatic_intent FROM music_soundscape_state")
            .fetch_one(&pool)
            .await
            .unwrap()
    );
    let state: (bool, String) =
        sqlx::query_as("SELECT desired_playing, active_soundscape_id FROM music_soundscape_state")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, (false, id));
    let persisted = ganbaru_music_library::soundscapes::state(&pool)
        .await
        .unwrap();
    let manual = ganbaru_music_library::soundscapes::update_state(
        &pool,
        ganbaru_music_library::MusicSoundscapeStateWrite {
            active_soundscape_id: persisted.active_soundscape_id,
            active_ids: persisted.active_ids,
            multiple_enabled: persisted.multiple_enabled,
            generated_level: persisted.generated_level,
            local_level: persisted.local_level,
            desired_playing: false,
            volume: persisted.volume,
            expected_version: persisted.version,
            updated_at_ms: persisted.updated_at_ms + 1,
        },
    )
    .await
    .unwrap();
    assert!(!manual.automatic_intent);
}

#[tokio::test]
async fn native_music_missing_background_source_keeps_current_output_intent_and_reports_unavailability()
 {
    let pool = super::super::context::test_pool().await;
    let before: i64 = sqlx::query_scalar("SELECT version FROM music_soundscape_state")
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    for id in [None, Some("deleted")] {
        let missing = prepare(&mut transaction, "device", "play-selected", id)
            .await
            .unwrap();
        assert!(missing.missing);
        assert!(missing.action.is_none());
    }
    assert!(
        prepare(&mut transaction, "device", "invalid", None)
            .await
            .is_err()
    );
    transaction.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT version FROM music_soundscape_state")
            .fetch_one(&pool)
            .await
            .unwrap(),
        before
    );
}
