use super::*;

#[tokio::test]
async fn native_music_context_preserves_canonical_event_identifiers_containing_occurrence_delimiters()
 {
    let pool = test_pool().await;
    sqlx::query("INSERT INTO music_context_assignments(owner_kind, owner_id, phase, behavior, provenance_kind, updated_at_ms) VALUES ('event-override', 'event::source', 'focus', 'pause-music', 'explicit', 1)")
        .execute(&pool).await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    assert!(
        resolve_assignment(&mut connection, "event::source", MusicActivityPhase::Focus)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        resolve_assignment(&mut connection, "event", MusicActivityPhase::Focus)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn native_music_context_rejects_oversized_persisted_references_before_loading_assignment_text()
 {
    let pool = test_pool().await;
    sqlx::query("INSERT INTO music_context_assignments(owner_kind, owner_id, phase, behavior, playlist_id, provenance_kind, updated_at_ms) VALUES ('event-override', 'event', 'focus', 'play-automatically', ?, 'explicit', 1)")
        .bind("x".repeat(MAX_CONTEXT_REFERENCE_BYTES as usize + 1)).execute(&pool).await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    assert!(
        resolve_assignment(&mut connection, "event", MusicActivityPhase::Focus)
            .await
            .unwrap_err()
            .to_string()
            .contains("size limit")
    );
}

#[tokio::test]
async fn native_music_context_resolves_precedence_for_every_committed_phase_and_background_only_override()
 {
    let pool = test_pool().await;
    sqlx::query("INSERT INTO calendar_events(id, start_time, end_time, environment_id) VALUES ('event', '2026-10-04T10:00:00Z', '2026-10-04T11:00:00Z', 'environment')")
        .execute(&pool).await.unwrap();
    for phase in [
        MusicActivityPhase::Focus,
        MusicActivityPhase::ShortBreak,
        MusicActivityPhase::LongBreak,
    ] {
        for (owner, id) in [
            ("event-snapshot", "event"),
            ("work-environment", "environment"),
            ("event-override", "event"),
        ] {
            sqlx::query("INSERT INTO music_context_assignments(owner_kind, owner_id, phase, behavior, provenance_kind, updated_at_ms) VALUES (?, ?, ?, 'pause-music', 'explicit', 1)")
                .bind(owner).bind(id).bind(phase.as_ref()).execute(&pool).await.unwrap();
            let mut connection = pool.acquire().await.unwrap();
            let resolved = resolve_assignment(&mut connection, "event", phase)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(resolved.source, owner);
        }
        sqlx::query("UPDATE music_context_assignments SET behavior = 'inherit', soundscape_behavior = 'play-selected', soundscape_id = 'background' WHERE owner_kind = 'event-override' AND phase = ?")
            .bind(phase.as_ref()).execute(&pool).await.unwrap();
        {
            let mut connection = pool.acquire().await.unwrap();
            let resolved = resolve_assignment(&mut connection, "event", phase)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(resolved.source, "event-override");
            assert_eq!(resolved.behavior, "inherit");
            assert_eq!(resolved.soundscape_id.as_deref(), Some("background"));
        }
        sqlx::query("UPDATE music_context_assignments SET soundscape_behavior = 'inherit' WHERE owner_kind = 'event-override' AND phase = ?")
            .bind(phase.as_ref()).execute(&pool).await.unwrap();
        let mut connection = pool.acquire().await.unwrap();
        assert_eq!(
            resolve_assignment(&mut connection, "event", phase)
                .await
                .unwrap()
                .unwrap()
                .source,
            "work-environment"
        );
    }
}

#[tokio::test]
async fn native_music_context_never_inherits_another_event_or_phase_assignment() {
    let pool = test_pool().await;
    sqlx::query("INSERT INTO music_context_assignments(owner_kind, owner_id, phase, behavior, provenance_kind, updated_at_ms) VALUES ('event-override', 'other', 'focus', 'pause-music', 'explicit', 1), ('event-override', 'event', 'short-break', 'pause-music', 'explicit', 1)")
        .execute(&pool).await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    assert!(
        resolve_assignment(&mut connection, "event", MusicActivityPhase::Focus)
            .await
            .unwrap()
            .is_none()
    );
}
