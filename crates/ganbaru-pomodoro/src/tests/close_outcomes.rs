use super::super::*;
use super::helpers::*;
use sqlx::Row;

#[test]
fn close_run_rolls_back_earlier_writes_after_late_event_failure() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 40,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        let mut segment = initial_segment();
        segment.pauses.push(PomodoroPauseWrite {
            started_at: "2026-05-29T10:10:00Z".to_string(),
            ended_at: None,
            reason: "manual".to_string(),
        });
        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        tx.commit().await.unwrap();

        sqlx::query(
            "CREATE TRIGGER fail_close_event
             BEFORE INSERT ON pomodoro_run_events
             WHEN NEW.event_type = 'stop'
             BEGIN
               SELECT RAISE(FAIL, 'forced late close failure');
             END",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let error = close_run_tx(
            &mut tx,
            &PomodoroRunClosure {
                run_id: "run-1".to_string(),
                ended_at: "2026-05-29T10:40:00Z".to_string(),
                end_reason: "stopped".to_string(),
                segment_status: "interrupted".to_string(),
                segment_end_reason: "stopped".to_string(),
                event_type: "stop".to_string(),
            },
        )
        .await
        .unwrap_err();
        assert!(error.contains("forced late close failure"));
        tx.rollback().await.unwrap();

        let run_end: Option<String> =
            sqlx::query_scalar("SELECT ended_at FROM pomodoro_runs WHERE id = 'run-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let segment_state: (String, Option<String>) = sqlx::query_as(
            "SELECT status, actual_end FROM pomodoro_segments WHERE id = 'segment-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let pause_end: Option<String> = sqlx::query_scalar(
            "SELECT ended_at FROM pomodoro_pauses WHERE segment_id = 'segment-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(run_end, None);
        assert_eq!(segment_state, ("active".to_string(), None));
        assert_eq!(pause_end, None);
    });
}

#[test]
fn close_run_records_adaptive_outcomes_and_context_state() {
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
        tx.commit().await.unwrap();

        sqlx::query(
            "INSERT INTO distractions_block_events
                    (id, run_id, segment_id, occurred_at, source_type, source_key,
                     phase, decision)
                 VALUES ('block-1', 'run-1', 'segment-1', '2026-05-29T10:10:00Z',
                         'browser', 'youtube.com', 'focus', 'blocked')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
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

        let clean_focus: f64 = sqlx::query_scalar(
            "SELECT numeric_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'run'
                   AND outcome_key = 'clean_focus_seconds'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let blocked_attempts: f64 = sqlx::query_scalar(
            "SELECT numeric_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'run'
                   AND outcome_key = 'blocked_attempt_count'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let run_completed: i64 = sqlx::query_scalar(
            "SELECT boolean_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'run'
                   AND outcome_key = 'run_completed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let run_end_reason: String = sqlx::query_scalar(
            "SELECT categorical_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'run'
                   AND outcome_key = 'run_end_reason'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let phase_clean_focus: f64 = sqlx::query_scalar(
            "SELECT numeric_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'phase'
                   AND outcome_key = 'phase_clean_focus_seconds'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let phase_kind: String = sqlx::query_scalar(
            "SELECT categorical_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'phase'
                   AND outcome_key = 'phase_kind'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(clean_focus, 40.0 * 60.0);
        assert_eq!(blocked_attempts, 1.0);
        assert_eq!(run_completed, 1);
        assert_eq!(run_end_reason, "completed");
        assert_eq!(phase_clean_focus, 40.0 * 60.0);
        assert_eq!(phase_kind, "focus");

        let state = sqlx::query(
            "SELECT context_key, readiness, avoidance_pressure, confidence
                 FROM pomodoro_adaptive_context_states
                 WHERE policy_id = 'local-adaptive-policy-v1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let context_key: String = state.try_get("context_key").unwrap();
        let readiness: f64 = state.try_get("readiness").unwrap();
        let avoidance_pressure: f64 = state.try_get("avoidance_pressure").unwrap();
        let confidence: f64 = state.try_get("confidence").unwrap();
        assert_eq!(context_key, "morning:first:medium:low:unknown:none");
        assert!(readiness > 0.0);
        assert!(avoidance_pressure > 0.0);
        assert!(confidence > 0.0);

        let history_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pomodoro_adaptive_context_state_history
                 WHERE policy_id = 'local-adaptive-policy-v1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(history_count, 1);
    });
}

#[test]
fn close_run_links_adaptive_outcomes_to_assignment() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 45,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        run.rhythm_source = "preset".to_string();
        run.preset_key = Some("adaptive".to_string());
        run.adaptive_snapshot = Some(adaptive_snapshot_with_assignment());
        let mut segment = initial_segment();
        segment.planned_end = "2026-05-29T10:45:00Z".to_string();

        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        close_run_tx(
            &mut tx,
            &PomodoroRunClosure {
                run_id: "run-1".to_string(),
                ended_at: "2026-05-29T10:45:00Z".to_string(),
                end_reason: "completed".to_string(),
                segment_status: "completed".to_string(),
                segment_end_reason: "completed".to_string(),
                event_type: "complete".to_string(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let assignment_id: String = sqlx::query_scalar(
            "SELECT assignment_id
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'run'
                   AND outcome_key = 'clean_focus_seconds'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let phase_assignment_id: String = sqlx::query_scalar(
            "SELECT assignment_id
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-1'
                   AND outcome_window = 'phase'
                   AND outcome_key = 'phase_clean_focus_seconds'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(assignment_id, "assignment-1");
        assert_eq!(phase_assignment_id, "assignment-1");
    });
}

#[test]
fn close_run_records_boundary_phase_outcomes() {
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
        let initial_segment = initial_segment();
        let break_segment = PomodoroSegmentWrite {
            id: "segment-2".to_string(),
            event_id: "event-1".to_string(),
            event_date: "2026-05-29".to_string(),
            run_id: "run-1".to_string(),
            rhythm_position: 1,
            phase: "short_break".to_string(),
            planned_start: "2026-05-29T10:40:00Z".to_string(),
            planned_end: "2026-05-29T10:45:00Z".to_string(),
            actual_start: Some("2026-05-29T10:40:00Z".to_string()),
            actual_end: None,
            pauses: Vec::new(),
            status: "active".to_string(),
            end_reason: None,
        };
        let envelope = adaptive_boundary_envelope(&break_segment);

        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &initial_segment)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        sqlx::query(
            "UPDATE pomodoro_segments
                 SET status = 'completed',
                     actual_end = '2026-05-29T10:40:00Z',
                     end_reason = 'completed'
                 WHERE id = 'segment-1'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        insert_segment_tx(&mut tx, &break_segment).await.unwrap();
        insert_adaptive_decision_envelope_tx(&mut tx, &envelope)
            .await
            .unwrap();
        insert_run_event_tx(
            &mut tx,
            RunEventInsert {
                run_id: "run-1",
                segment_id: Some("segment-2"),
                event_type: "phase_start",
                occurred_at: "2026-05-29T10:40:00Z",
                phase: Some("short_break"),
                reason: None,
                duration_seconds: None,
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        sqlx::query(
            "INSERT INTO distractions_block_events
                    (id, run_id, segment_id, occurred_at, source_type, source_key,
                     phase, decision)
                 VALUES ('block-break-1', 'run-1', 'segment-2', '2026-05-29T10:46:00Z',
                         'browser', 'youtube.com', 'short_break', 'blocked')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        close_run_tx(
            &mut tx,
            &PomodoroRunClosure {
                run_id: "run-1".to_string(),
                ended_at: "2026-05-29T10:47:00Z".to_string(),
                end_reason: "completed".to_string(),
                segment_status: "completed".to_string(),
                segment_end_reason: "completed".to_string(),
                event_type: "complete".to_string(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let phase_kind: String = sqlx::query_scalar(
            "SELECT categorical_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-boundary-1'
                   AND outcome_window = 'phase'
                   AND outcome_key = 'phase_kind'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let break_overtime: f64 = sqlx::query_scalar(
            "SELECT numeric_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-boundary-1'
                   AND outcome_window = 'phase'
                   AND outcome_key = 'phase_break_overtime_seconds'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let blocked_attempts: f64 = sqlx::query_scalar(
            "SELECT numeric_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-boundary-1'
                   AND outcome_window = 'phase'
                   AND outcome_key = 'phase_blocked_attempt_count'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let next_focus_observed: i64 = sqlx::query_scalar(
            "SELECT boolean_value
                 FROM pomodoro_adaptive_outcomes
                 WHERE decision_id = 'decision-boundary-1'
                   AND outcome_window = 'phase'
                   AND outcome_key = 'post_break_next_focus_observed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(phase_kind, "short_break");
        assert_eq!(break_overtime, 120.0);
        assert_eq!(blocked_attempts, 1.0);
        assert_eq!(next_focus_observed, 0);
    });
}
