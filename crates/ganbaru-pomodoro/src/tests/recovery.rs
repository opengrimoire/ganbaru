use super::super::*;
use super::helpers::*;
use ganbaru_db::run_migrations;
use sqlx::Row;

#[test]
fn close_run_clamps_end_to_the_latest_open_activity_boundary() {
    super::block_on(async {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO calendar_events (id, title, start_time, end_time)
                 VALUES ('event-1', 'Focus block', '2026-05-29T10:00:00Z', '2026-05-29T11:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_runs
                    (id, event_id, original_event_id, event_date, planned_start, planned_end,
                     started_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
                     start_trigger)
                 VALUES ('run-1', 'event-1', 'event-1', '2026-05-29',
                         '2026-05-29T10:00:00Z', '2026-05-29T11:00:00Z',
                         '2026-05-29T10:05:00Z', 'count', 'preset', 'adaptive',
                         '2026-05-29T10:05:00Z', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_segments
                    (id, event_id, event_date, run_id, rhythm_position, phase,
                     planned_start, planned_end, actual_start, status)
                 VALUES ('segment-1', 'event-1', '2026-05-29', 'run-1', 1, 'focus',
                         '2026-05-29T10:00:00Z', '2026-05-29T10:40:00Z',
                         '2026-05-29T10:05:00Z', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_pauses (id, segment_id, started_at, reason)
                 VALUES ('pause-1', 'segment-1', '2026-05-29T10:07:00Z', 'idle')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        close_run_tx(
            &mut tx,
            &PomodoroRunClosure {
                run_id: "run-1".to_string(),
                ended_at: "2026-05-29T10:00:00Z".to_string(),
                end_reason: "completed".to_string(),
                segment_status: "interrupted".to_string(),
                segment_end_reason: "event_expired".to_string(),
                event_type: "complete".to_string(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let row = sqlx::query(
            "SELECT r.ended_at AS run_end, s.actual_end AS segment_end, p.ended_at AS pause_end
                 FROM pomodoro_runs r
                 JOIN pomodoro_segments s ON s.run_id = r.id
                 JOIN pomodoro_pauses p ON p.segment_id = s.id
                 WHERE r.id = 'run-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let run_end: String = row.try_get("run_end").unwrap();
        let segment_end: String = row.try_get("segment_end").unwrap();
        let pause_end: String = row.try_get("pause_end").unwrap();
        assert_eq!(run_end, "2026-05-29T10:07:00Z");
        assert_eq!(segment_end, "2026-05-29T10:07:00Z");
        assert_eq!(pause_end, "2026-05-29T10:07:00Z");
    });
}

#[test]
fn crash_recovery_closes_run_and_segment_at_last_heartbeat() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut tx = pool.begin().await.unwrap();
        let run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 40,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        insert_run_tx(&mut tx, &run, &initial_segment())
            .await
            .unwrap();
        sqlx::query("UPDATE pomodoro_runs SET last_heartbeat = ? WHERE id = ?")
            .bind("2026-05-29T10:10:00Z")
            .bind("run-1")
            .execute(&mut *tx)
            .await
            .unwrap();
        close_run_tx(
            &mut tx,
            &PomodoroRunClosure {
                run_id: "run-1".to_string(),
                ended_at: "2026-05-29T10:10:00Z".to_string(),
                end_reason: "interrupted".to_string(),
                segment_status: "interrupted".to_string(),
                segment_end_reason: "crash_recovery".to_string(),
                event_type: "crash_recovery".to_string(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let row = sqlx::query(
            "SELECT r.ended_at AS run_end, s.actual_end AS segment_end,
                    s.status AS segment_status, s.end_reason AS segment_end_reason
             FROM pomodoro_runs r
             JOIN pomodoro_segments s ON s.run_id = r.id
             WHERE r.id = 'run-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("run_end"), "2026-05-29T10:10:00Z");
        assert_eq!(row.get::<String, _>("segment_end"), "2026-05-29T10:10:00Z");
        assert_eq!(row.get::<String, _>("segment_status"), "interrupted");
        assert_eq!(row.get::<String, _>("segment_end_reason"), "crash_recovery");
    });
}
