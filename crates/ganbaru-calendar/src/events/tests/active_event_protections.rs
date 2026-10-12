use super::fixtures::*;

#[test]
fn deleting_event_rejects_active_pomodoro_run() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event(&pool, "event-1", "").await;
        insert_test_open_pomodoro_run(&pool).await;
        insert_test_active_pomodoro_segment(&pool).await;

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
        assert!(err.contains("active pomodoro run"));

        let run: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT ended_at, event_id
             FROM pomodoro_runs
             WHERE id = 'run-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let segment: (String, Option<String>) = sqlx::query_as(
            "SELECT status, event_id
             FROM pomodoro_segments
             WHERE id = 'segment-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(run.0.is_none());
        assert_eq!(run.1, Some("event-1".to_string()));
        assert_eq!(segment.0, "active");
        assert_eq!(segment.1, Some("event-1".to_string()));
    });
}
