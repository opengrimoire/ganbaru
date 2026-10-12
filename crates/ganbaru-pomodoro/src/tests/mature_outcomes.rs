use super::super::*;
use super::helpers::*;

#[test]
fn records_matured_day_outcomes_for_run_start_decisions() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 40,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        run.rhythm_source = "preset".to_string();
        run.preset_key = Some("adaptive".to_string());
        run.adaptive_snapshot = Some(adaptive_snapshot());
        let segment = initial_segment();

        sqlx::query(
            "INSERT INTO calendar_events (id, title, start_time, end_time)
                 VALUES ('event-2', 'Missed focus block',
                         '2026-05-29T13:00:00Z', '2026-05-29T14:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO calendar_event_pomodoro_configs
                    (event_id, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes)
                 VALUES ('event-1', 'count', 'preset', 'adaptive', NULL),
                        ('event-2', 'count', 'preset', 'adaptive', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        close_run_tx(
            &mut tx,
            &PomodoroRunClosure {
                run_id: "run-1".to_string(),
                ended_at: "2026-05-29T10:40:00Z".to_string(),
                end_reason: "completed".to_string(),
                segment_status: "completed".to_string(),
                segment_end_reason: "completed".to_string(),
                event_type: "complete".to_string(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        sqlx::query("DELETE FROM calendar_events WHERE id = 'event-2'")
            .execute(&pool)
            .await
            .unwrap();
        let captured_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
                 FROM pomodoro_adaptive_planned_blocks
                 WHERE event_date = '2026-05-29'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(captured_count, 2);
        sqlx::query(
            "INSERT INTO pomodoro_runs
                    (id, event_id, original_event_id, event_date, planned_start, planned_end,
                     started_at, ended_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
                     end_reason, start_trigger)
                 VALUES ('run-current', 'event-1', 'event-1', '2026-05-30',
                         '2026-05-30T10:00:00Z', '2026-05-30T10:10:00Z',
                         '2026-05-30T10:00:00Z', '2026-05-30T10:10:00Z',
                         'count', 'preset', 'adaptive', '2026-05-30T10:10:00Z',
                         'completed', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        record_matured_adaptive_day_outcomes_tx(&mut tx, "run-current", "2026-05-30T10:10:00Z")
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let observed: i64 = sqlx::query_scalar(
            "SELECT boolean_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'day'
                   AND outcome_key = 'day_observed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let clean_focus: f64 = sqlx::query_scalar(
            "SELECT numeric_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'day'
                   AND outcome_key = 'day_clean_focus_seconds'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let planned_count: f64 = sqlx::query_scalar(
            "SELECT numeric_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'day'
                   AND outcome_key = 'day_planned_pomodoro_event_count'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let missed_count: f64 = sqlx::query_scalar(
            "SELECT numeric_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'day'
                   AND outcome_key = 'day_missed_planned_pomodoro_event_count'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(observed, 1);
        assert_eq!(clean_focus, 40.0 * 60.0);
        assert_eq!(planned_count, 2.0);
        assert_eq!(missed_count, 1.0);
    });
}

#[test]
fn records_matured_next_day_outcomes_for_run_start_decisions() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 40,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        run.rhythm_source = "preset".to_string();
        run.preset_key = Some("adaptive".to_string());
        run.adaptive_snapshot = Some(adaptive_snapshot());
        let segment = initial_segment();

        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        close_run_tx(
            &mut tx,
            &PomodoroRunClosure {
                run_id: "run-1".to_string(),
                ended_at: "2026-05-29T10:40:00Z".to_string(),
                end_reason: "completed".to_string(),
                segment_status: "completed".to_string(),
                segment_end_reason: "completed".to_string(),
                event_type: "complete".to_string(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_runs
                    (id, event_id, original_event_id, event_date, planned_start, planned_end,
                     started_at, ended_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
                     end_reason, start_trigger)
                 VALUES ('run-next-day', 'event-1', 'event-1', '2026-05-30',
                         '2026-05-30T10:00:00Z', '2026-05-30T11:00:00Z',
                         '2026-05-30T10:00:00Z', '2026-05-30T10:30:00Z',
                         'count', 'preset', 'adaptive', '2026-05-30T10:30:00Z',
                         'completed', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_segments
                    (id, event_id, event_date, run_id, rhythm_position, phase,
                     planned_start, planned_end, actual_start, actual_end, status, end_reason)
                 VALUES ('segment-next-day', 'event-1', '2026-05-30', 'run-next-day',
                         1, 'focus', '2026-05-30T10:00:00Z',
                         '2026-05-30T10:30:00Z', '2026-05-30T10:00:00Z',
                         '2026-05-30T10:30:00Z', 'completed', 'completed')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_runs
                    (id, event_id, original_event_id, event_date, planned_start, planned_end,
                     started_at, ended_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
                     end_reason, start_trigger)
                 VALUES ('run-current', 'event-1', 'event-1', '2026-05-31',
                         '2026-05-31T10:00:00Z', '2026-05-31T10:10:00Z',
                         '2026-05-31T10:00:00Z', '2026-05-31T10:10:00Z',
                         'count', 'preset', 'adaptive', '2026-05-31T10:10:00Z',
                         'completed', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        record_matured_adaptive_next_day_outcomes_tx(
            &mut tx,
            "run-current",
            "2026-05-31T10:10:00Z",
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let observed: i64 = sqlx::query_scalar(
            "SELECT boolean_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'next_day'
                   AND outcome_key = 'next_day_observed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let clean_focus: f64 = sqlx::query_scalar(
            "SELECT numeric_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'next_day'
                   AND outcome_key = 'next_day_clean_focus_seconds'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let run_count: f64 = sqlx::query_scalar(
            "SELECT numeric_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'next_day'
                   AND outcome_key = 'next_day_run_count'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(observed, 1);
        assert_eq!(clean_focus, 30.0 * 60.0);
        assert_eq!(run_count, 1.0);
    });
}
