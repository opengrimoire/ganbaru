use super::fixtures::*;

#[test]
fn calendar_event_create_row_matches_current_schema() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        let mut event = event_create();
        event.environment_id = Some("environment-a".to_string());
        event.playlist_id = Some("playlist-a".to_string());
        let mut tx = pool.begin().await.unwrap();
        insert_calendar_event_row(&mut tx, &event).await.unwrap();
        tx.commit().await.unwrap();

        let saved: (String, Option<String>, Option<String>, i64, Option<String>) = sqlx::query_as(
            "SELECT title, environment_id, playlist_id, meeting_enabled, local_rsvp_status
             FROM calendar_events
             WHERE id = 'event-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(saved.0, "Focus");
        assert_eq!(saved.1, Some("environment-a".to_string()));
        assert_eq!(saved.2, Some("playlist-a".to_string()));
        assert_eq!(saved.3, 0);
        assert_eq!(saved.4, None);
    });
}

#[test]
fn future_untracked_event_hard_deletes() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-09T10:00:00Z",
            "2099-05-09T11:00:00Z",
            None,
        )
        .await;

        let mut tx = pool.begin().await.unwrap();
        delete_calendar_event_tx(
            &mut tx,
            &CalendarEventMutationTarget {
                id: "event-1".to_string(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let live_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events WHERE id = 'event-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let archive_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM calendar_event_archives WHERE id = 'event-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(live_count, 0);
        assert_eq!(archive_count, 0);
    });
}

#[test]
fn past_event_delete_rejects_and_archive_succeeds() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event(&pool, "event-1", "").await;

        let mut tx = pool.begin().await.unwrap();
        let err = delete_calendar_event_tx(
            &mut tx,
            &CalendarEventMutationTarget {
                id: "event-1".to_string(),
            },
        )
        .await
        .unwrap_err();
        tx.rollback().await.unwrap();
        assert!(err.contains("archive it instead"));

        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(
            &mut tx,
            &CalendarEventMutationTarget {
                id: "event-1".to_string(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let live_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events WHERE id = 'event-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let archived_title: String =
            sqlx::query_scalar("SELECT title FROM calendar_event_archives WHERE id = 'event-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(live_count, 0);
        assert_eq!(archived_title, "");
    });
}

#[test]
fn invalid_config_replacement_preserves_existing_child_rows() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2000-05-09T10:00:00Z",
            "2999-05-09T11:00:00Z",
            None,
        )
        .await;

        let mut tx = pool.begin().await.unwrap();
        insert_pomodoro_config(&mut tx, "event-1", &sequence_pomodoro_config())
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result =
            replace_pomodoro_config(&mut tx, "event-1", &invalid_sequence_pomodoro_config()).await;
        assert!(result.is_err());
        tx.rollback().await.unwrap();

        let saved: (String, i64) = sqlx::query_as(
            "SELECT pc.rhythm_kind, COUNT(pcss.step_index)
             FROM calendar_event_pomodoro_configs pc
             JOIN calendar_event_pomodoro_config_sequence_steps pcss ON pcss.event_id = pc.event_id
             WHERE pc.event_id = 'event-1'
             GROUP BY pc.rhythm_kind",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(saved, ("sequence".to_string(), 2));
    });
}
