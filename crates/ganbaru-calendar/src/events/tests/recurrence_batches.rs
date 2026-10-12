use super::fixtures::*;

#[test]
fn hard_delete_rejects_protected_rows() {
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
    });
}

#[test]
fn archive_rejects_active_pomodoro_rows() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event(&pool, "event-1", "").await;
        insert_test_open_pomodoro_run(&pool).await;
        insert_test_active_pomodoro_segment(&pool).await;

        let mut tx = pool.begin().await.unwrap();
        let err = archive_calendar_event_tx(
            &mut tx,
            &CalendarEventMutationTarget {
                id: "event-1".to_string(),
            },
        )
        .await
        .unwrap_err();
        tx.rollback().await.unwrap();
        assert!(err.contains("active pomodoro run"));
    });
}

#[test]
fn scoped_archive_preserves_original_event_id() {
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

        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(
            &mut tx,
            &CalendarEventMutationTarget {
                id: "event-1::2000-05-10".to_string(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let archived_source: String = sqlx::query_scalar(
            "SELECT source_event_id FROM calendar_event_archives WHERE id = 'event-1::2000-05-10'",
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

        assert_eq!(archived_source, "event-1");
        assert_eq!(exdate_count, 1);
        assert_eq!(run.0, None);
        assert_eq!(run.1, "event-1::2000-05-10");
    });
}
