use super::super::*;
use super::helpers::*;

async fn completed_run(pool: &sqlx::SqlitePool, id: &str, occurrence: &str) {
    sqlx::query(
        "INSERT INTO pomodoro_runs
         (id, event_id, original_event_id, event_date, planned_start, planned_end,
          started_at, ended_at, rhythm_kind, rhythm_source, last_heartbeat)
         VALUES (?, 'event-1', ?, '2026-05-28', '2026-05-29T00:00:00Z',
          '2026-05-29T01:00:00Z', '2026-05-29T00:00:00Z', '2026-05-29T01:00:00Z',
          'count', 'custom', '2026-05-29T01:00:00Z')",
    )
    .bind(id)
    .bind(occurrence)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO pomodoro_segments
         (id, event_id, event_date, run_id, rhythm_position, phase,
          planned_start, planned_end, actual_start, actual_end, status)
         VALUES (?, 'event-1', '2026-05-28', ?, 1, 'focus',
          '2026-05-29T00:00:00Z', '2026-05-29T01:00:00Z',
          '2026-05-29T00:00:00Z', '2026-05-29T01:00:00Z', 'completed')",
    )
    .bind(id)
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn exact_occurrence_history_separates_anchor_home_date_and_current_alias() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        completed_run(&pool, "anchor", "event-1").await;
        completed_run(&pool, "later", "event-1::2026-05-29").await;
        completed_run(&pool, "retargeted", "event-1::2026-05-30").await;
        sqlx::query("UPDATE pomodoro_runs SET current_occurrence_id = 'event-1::2026-06-02' WHERE id = 'retargeted'")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO pomodoro_pauses (id, segment_id, started_at, ended_at, reason) VALUES ('pause', 'later', '2026-05-29T00:05:00Z', '2026-05-29T00:06:00Z', 'manual')")
            .execute(&pool).await.unwrap();
        for (occurrence, run) in [
            ("event-1", "anchor"),
            ("event-1::2026-05-29", "later"),
            ("event-1::2026-06-02", "retargeted"),
        ] {
            let rows = pomodoro_load_segments_for_events(pool.clone(), vec![occurrence.into()])
                .await
                .unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].run_id, run);
            assert_eq!(rows[0].event_id, occurrence);
            assert_eq!(rows[0].event_date, "2026-05-28");
            assert_eq!(rows[0].pauses.len(), usize::from(run == "later"));
        }
        assert!(
            pomodoro_load_segments_for_events(pool.clone(), vec!["event-1::2026-05-30".into()])
                .await
                .unwrap()
                .is_empty()
        );
        let physical: String =
            sqlx::query_scalar("SELECT event_id FROM pomodoro_segments WHERE id = 'retargeted'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(physical, "event-1");
    });
}

#[test]
fn unrelated_series_history_cannot_exhaust_the_visible_occurrence_budget() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        completed_run(&pool, "visible", "event-1::2026-05-29").await;
        completed_run(&pool, "old", "event-1::2026-05-28").await;
        sqlx::query(
            "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i < 10001)
             INSERT INTO pomodoro_segments (id, event_id, event_date, run_id, rhythm_position, phase,
              planned_start, planned_end, actual_start, actual_end, status)
             SELECT 'old-' || i, 'event-1', '2026-05-28', 'old', i, 'focus',
              '2026-05-28T00:00:00Z', '2026-05-28T00:01:00Z',
              '2026-05-28T00:00:00Z', '2026-05-28T00:01:00Z', 'completed' FROM n",
        ).execute(&pool).await.unwrap();
        let visible =
            pomodoro_load_segments_for_events(pool.clone(), vec!["event-1::2026-05-29".into()])
                .await
                .unwrap();
        assert_eq!(visible.len(), 1);
        let oversized =
            pomodoro_load_segments_for_events(pool, vec!["event-1::2026-05-28".into()]).await;
        assert!(oversized.err().unwrap().contains("row budget"));
    });
}
