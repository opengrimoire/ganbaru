use super::fixtures::*;

#[test]
fn archived_event_restore_relinks_pomodoro_history() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2000-05-09T10:00:00Z",
            "2000-05-09T11:00:00Z",
            None,
        )
        .await;
        insert_test_completed_pomodoro_history(&pool, "event-1", "event-1", "2000-05-09").await;

        let target = CalendarEventMutationTarget {
            id: "event-1".to_string(),
        };
        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(&mut tx, &target).await.unwrap();
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
        let run_event_id: Option<String> =
            sqlx::query_scalar("SELECT event_id FROM pomodoro_runs WHERE id = 'run-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let segment_event_id: Option<String> =
            sqlx::query_scalar("SELECT event_id FROM pomodoro_segments WHERE id = 'segment-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(live_count, 0);
        assert_eq!(archive_count, 1);
        assert_eq!(run_event_id, None);
        assert_eq!(segment_event_id, None);

        let mut tx = pool.begin().await.unwrap();
        restore_archived_calendar_event_tx(&mut tx, &target)
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
        let run: (Option<String>, String) = sqlx::query_as(
            "SELECT event_id, original_event_id FROM pomodoro_runs WHERE id = 'run-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let segment_event_id: Option<String> =
            sqlx::query_scalar("SELECT event_id FROM pomodoro_segments WHERE id = 'segment-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(live_count, 1);
        assert_eq!(archive_count, 0);
        assert_eq!(run.0, Some("event-1".to_string()));
        assert_eq!(run.1, "event-1");
        assert_eq!(segment_event_id, Some("event-1".to_string()));
    });
}

#[test]
fn archived_synthetic_restore_removes_exception_and_relinks_history() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2000-05-09T10:00:00Z",
            "2000-05-09T11:00:00Z",
            Some("FREQ=DAILY"),
        )
        .await;
        insert_test_completed_pomodoro_history(
            &pool,
            "event-1",
            "event-1::2000-05-10",
            "2000-05-10",
        )
        .await;

        let target = CalendarEventMutationTarget {
            id: "event-1::2000-05-10".to_string(),
        };
        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(&mut tx, &target).await.unwrap();
        tx.commit().await.unwrap();

        let parent_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events WHERE id = 'event-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let archive_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM calendar_event_archives WHERE id = 'event-1::2000-05-10'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let exdate_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM calendar_event_exdates
             WHERE event_id = 'event-1' AND occurrence_date = '2000-05-10'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let run_event_id: Option<String> =
            sqlx::query_scalar("SELECT event_id FROM pomodoro_runs WHERE id = 'run-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(parent_count, 1);
        assert_eq!(archive_count, 1);
        assert_eq!(exdate_count, 1);
        assert_eq!(run_event_id, None);

        let mut tx = pool.begin().await.unwrap();
        restore_archived_calendar_event_tx(&mut tx, &target)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let archive_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM calendar_event_archives WHERE id = 'event-1::2000-05-10'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let exdate_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM calendar_event_exdates
             WHERE event_id = 'event-1' AND occurrence_date = '2000-05-10'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let run: (Option<String>, String) = sqlx::query_as(
            "SELECT event_id, original_event_id FROM pomodoro_runs WHERE id = 'run-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let segment_event_id: Option<String> =
            sqlx::query_scalar("SELECT event_id FROM pomodoro_segments WHERE id = 'segment-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(archive_count, 0);
        assert_eq!(exdate_count, 0);
        assert_eq!(run.0, Some("event-1".to_string()));
        assert_eq!(run.1, "event-1::2000-05-10");
        assert_eq!(segment_event_id, Some("event-1".to_string()));
    });
}

#[test]
fn synthetic_future_delete_adds_exception_without_deleting_parent() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-09T10:00:00Z",
            "2099-05-09T11:00:00Z",
            Some("FREQ=DAILY"),
        )
        .await;

        let mut tx = pool.begin().await.unwrap();
        delete_calendar_event_tx(
            &mut tx,
            &CalendarEventMutationTarget {
                id: "event-1::2099-05-10".to_string(),
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
        let exdate_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM calendar_event_exdates
             WHERE event_id = 'event-1' AND occurrence_date = '2099-05-10'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(live_count, 1);
        assert_eq!(exdate_count, 1);
    });
}

#[test]
fn synthetic_archive_uses_id_date_when_utc_start_is_next_day() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-09T02:00:00Z",
            "2099-05-09T03:00:00Z",
            Some("FREQ=DAILY"),
        )
        .await;

        let target = CalendarEventMutationTarget {
            id: "event-1::2099-05-10".to_string(),
        };
        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(&mut tx, &target).await.unwrap();
        tx.commit().await.unwrap();

        let local_date_exdates: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM calendar_event_exdates
             WHERE event_id = 'event-1' AND occurrence_date = '2099-05-10'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let utc_date_exdates: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM calendar_event_exdates
             WHERE event_id = 'event-1' AND occurrence_date = '2099-05-11'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(local_date_exdates, 1);
        assert_eq!(utc_date_exdates, 0);

        let mut tx = pool.begin().await.unwrap();
        restore_archived_calendar_event_tx(&mut tx, &target)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let remaining_exdates: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM calendar_event_exdates WHERE event_id = 'event-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_exdates, 0);
    });
}
