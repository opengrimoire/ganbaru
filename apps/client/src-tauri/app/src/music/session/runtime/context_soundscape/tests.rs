use super::*;

const BROWN_PRESET: &str = "preset-generated-brown-noise";
const WHITE_PRESET: &str = "preset-generated-white-noise";

#[tokio::test]
async fn native_music_background_assignment_and_selection_roll_back_with_the_soundtrack_transaction()
 {
    let pool = super::super::context::test_pool().await;
    let before: (i64, bool, Option<String>) = sqlx::query_as(
        "SELECT version, desired_playing, active_preset_id FROM music_soundscape_state",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let prepared = prepare(
        &mut transaction,
        &BTreeMap::new(),
        "play-selected",
        Some(BROWN_PRESET),
    )
    .await
    .unwrap();
    assert!(!prepared.missing);
    let request = prepared.action.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(request.source_id, "generated-brown-noise");
    assert_eq!(request.preset_level, 1.25);
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
        "SELECT version, desired_playing, active_preset_id FROM music_soundscape_state",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn native_music_background_pause_and_unchosen_phases_stop_automatic_output_and_keep_selection()
 {
    let pool = super::super::context::test_pool().await;
    let mut transaction = pool.begin().await.unwrap();
    let playing = prepare(
        &mut transaction,
        &BTreeMap::new(),
        "play-selected",
        Some(WHITE_PRESET),
    )
    .await
    .unwrap();
    let unchosen = prepare(
        &mut transaction,
        &BTreeMap::new(),
        "keep-current-soundscape",
        None,
    )
    .await
    .unwrap();
    assert!(matches!(unchosen.action, Some(None)));
    assert_eq!(unchosen.version, playing.version.map(|version| version + 1));
    let already_quiet = prepare(&mut transaction, &BTreeMap::new(), "inherit", None)
        .await
        .unwrap();
    assert!(already_quiet.action.is_none());
    let paused = prepare(&mut transaction, &BTreeMap::new(), "pause-soundscape", None)
        .await
        .unwrap();
    assert!(matches!(paused.action, Some(None)));
    assert_eq!(paused.version, playing.version.map(|version| version + 2));
    transaction.commit().await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT automatic_intent FROM music_soundscape_state")
            .fetch_one(&pool)
            .await
            .unwrap()
    );
    let state: (bool, String) =
        sqlx::query_as("SELECT desired_playing, active_preset_id FROM music_soundscape_state")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, (false, WHITE_PRESET.to_string()));
    let persisted = ganbaru_music_library::soundscapes::state(&pool)
        .await
        .unwrap();
    let manual = ganbaru_music_library::soundscapes::update_state(
        &pool,
        ganbaru_music_library::MusicSoundscapeStateWrite {
            active_preset_id: persisted.active_preset_id,
            desired_playing: false,
            expected_version: persisted.version,
            updated_at_ms: persisted.updated_at_ms + 1,
        },
    )
    .await
    .unwrap();
    assert!(!manual.automatic_intent);
}

#[cfg(desktop)]
#[tokio::test]
async fn native_music_ended_activity_releases_only_an_automatic_soundscape_once() {
    let pool = super::super::context::test_pool().await;
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(release_automatic(&mut connection).await.unwrap(), None);
    let playing = prepare(
        &mut connection,
        &BTreeMap::new(),
        "play-selected",
        Some(WHITE_PRESET),
    )
    .await
    .unwrap();
    assert!(automatic_playing(&mut connection).await.unwrap());
    let released = release_automatic(&mut connection).await.unwrap();
    assert_eq!(released, playing.version.map(|version| version + 1));
    assert!(!automatic_playing(&mut connection).await.unwrap());
    assert_eq!(release_automatic(&mut connection).await.unwrap(), None);
    drop(connection);
    let after = ganbaru_music_library::soundscapes::state(&pool)
        .await
        .unwrap();
    assert!(after.automatic_intent && !after.desired_playing);
    assert_eq!(after.active_preset_id.as_deref(), Some(WHITE_PRESET));

    let manual = ganbaru_music_library::soundscapes::update_state(
        &pool,
        ganbaru_music_library::MusicSoundscapeStateWrite {
            active_preset_id: Some(WHITE_PRESET.into()),
            desired_playing: true,
            expected_version: after.version,
            updated_at_ms: after.updated_at_ms + 1,
        },
    )
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    assert!(!automatic_playing(&mut connection).await.unwrap());
    assert_eq!(release_automatic(&mut connection).await.unwrap(), None);
    drop(connection);
    let kept = ganbaru_music_library::soundscapes::state(&pool)
        .await
        .unwrap();
    assert_eq!(kept.version, manual.version);
    assert!(kept.desired_playing);
}

#[tokio::test]
async fn native_music_unchosen_phases_leave_a_hand_started_soundscape_playing() {
    let pool = super::super::context::test_pool().await;
    let persisted = ganbaru_music_library::soundscapes::state(&pool)
        .await
        .unwrap();
    let manual = ganbaru_music_library::soundscapes::update_state(
        &pool,
        ganbaru_music_library::MusicSoundscapeStateWrite {
            active_preset_id: Some(WHITE_PRESET.into()),
            desired_playing: true,
            expected_version: persisted.version,
            updated_at_ms: persisted.updated_at_ms + 1,
        },
    )
    .await
    .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    for behavior in ["inherit", "keep-current-soundscape"] {
        let kept = prepare(&mut transaction, &BTreeMap::new(), behavior, None)
            .await
            .unwrap();
        assert!(kept.action.is_none());
    }
    transaction.commit().await.unwrap();
    let after = ganbaru_music_library::soundscapes::state(&pool)
        .await
        .unwrap();
    assert_eq!(after.version, manual.version);
    assert!(after.desired_playing && !after.automatic_intent);
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
        let missing = prepare(&mut transaction, &BTreeMap::new(), "play-selected", id)
            .await
            .unwrap();
        assert!(missing.missing);
        assert!(missing.action.is_none());
    }
    assert!(
        prepare(&mut transaction, &BTreeMap::new(), "invalid", None)
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

#[tokio::test]
async fn native_music_background_preset_layers_available_sounds_in_order_with_their_mix() {
    let pool = super::super::context::test_pool().await;
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM music_soundscapes WHERE generated_kind IN ('brown', 'pink') ORDER BY generated_kind",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO music_soundscapes (id, source_kind, name, availability, created_at_ms, updated_at_ms, version) VALUES ('local-storm', 'local-loop', 'Storm', 'missing', 1, 1, 1)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO music_soundscape_presets (id, name, icon, level, created_at_ms, updated_at_ms) VALUES ('preset', 'Desk', 'lucide:cloud', 1.5, 1, 1), ('empty', 'Empty', 'lucide:cloud', 0.5, 1, 1)")
        .execute(&pool).await.unwrap();
    for (position, (id, level)) in [
        (&ids[1], 0.8),
        (&"local-storm".to_string(), 1.0),
        (&ids[0], 0.3),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query("INSERT INTO music_soundscape_preset_sounds (preset_id, position, soundscape_id, level) VALUES ('preset', ?, ?, ?)")
            .bind(position as i64).bind(id).bind(level).execute(&pool).await.unwrap();
    }
    let mut transaction = pool.begin().await.unwrap();
    for preset in ["empty", "deleted"] {
        let missing = prepare(
            &mut transaction,
            &BTreeMap::new(),
            "play-selected",
            Some(preset),
        )
        .await
        .unwrap();
        assert!(missing.missing);
    }
    let prepared = prepare(
        &mut transaction,
        &BTreeMap::new(),
        "play-selected",
        Some("preset"),
    )
    .await
    .unwrap();
    assert!(!prepared.missing);
    let request = prepared.action.unwrap().unwrap();
    assert_eq!(request.source_id, ids[1]);
    assert_eq!(request.level, 0.8);
    assert_eq!(request.preset_level, 1.5);
    assert_eq!(
        request
            .extra_sources
            .iter()
            .map(|layer| (layer.source_id.as_str(), layer.level))
            .collect::<Vec<_>>(),
        vec![(ids[0].as_str(), 0.3)]
    );
    transaction.commit().await.unwrap();
    let state = ganbaru_music_library::soundscapes::state(&pool)
        .await
        .unwrap();
    assert_eq!(state.active_preset_id.as_deref(), Some("preset"));
    assert!(state.desired_playing);
}

#[tokio::test]
async fn native_music_background_local_loop_plays_only_from_this_devices_recorded_file() {
    let pool = super::super::context::test_pool().await;
    let file = std::env::temp_dir().join(format!(
        "ganbaru-context-storm-{}-{}.wav",
        std::process::id(),
        now_ms()
    ));
    std::fs::write(&file, b"loop").unwrap();
    sqlx::query("INSERT INTO music_soundscapes (id, source_kind, name, availability, created_at_ms, updated_at_ms, version) VALUES ('local-storm', 'local-loop', 'Storm', 'available', 1, 1, 1)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO music_soundscape_presets (id, name, icon, level, created_at_ms, updated_at_ms) VALUES ('storm', 'Storm', 'lucide:cloud', 1.0, 1, 1)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO music_soundscape_preset_sounds (preset_id, position, soundscape_id, level) VALUES ('storm', 0, 'local-storm', 0.7)")
        .execute(&pool).await.unwrap();
    let recorded = BTreeMap::from([(
        "local-storm".to_string(),
        file.to_string_lossy().into_owned(),
    )]);
    let gone = BTreeMap::from([(
        "local-storm".to_string(),
        file.with_extension("missing.wav")
            .to_string_lossy()
            .into_owned(),
    )]);

    let mut transaction = pool.begin().await.unwrap();
    for paths in [BTreeMap::new(), gone] {
        let missing = prepare(&mut transaction, &paths, "play-selected", Some("storm"))
            .await
            .unwrap();
        assert!(missing.missing);
    }
    let prepared = prepare(&mut transaction, &recorded, "play-selected", Some("storm"))
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    std::fs::remove_file(&file).unwrap();

    let request = prepared.action.unwrap().unwrap();
    assert_eq!(request.source_id, "local-storm");
    assert_eq!(request.local_path.as_deref(), file.to_str());
    assert_eq!(request.level, 0.7);
}
