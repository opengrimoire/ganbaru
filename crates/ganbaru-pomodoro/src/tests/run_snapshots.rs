use super::super::*;
use super::helpers::*;
use sqlx::Row;

#[test]
fn insert_run_snapshots_count_rhythm() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 40,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        let segment = initial_segment();

        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        tx.commit().await.unwrap();

        let saved: (String, i64, i64) = sqlx::query_as(
            "SELECT r.rhythm_kind, c.focus_duration_minutes, c.long_break_after_focus_count
                 FROM pomodoro_runs r
                 JOIN pomodoro_run_count_rhythms c ON c.run_id = r.id
                 WHERE r.id = 'run-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(saved, ("count".to_string(), 40, 4));
    });
}

#[test]
fn insert_run_snapshots_sequence_rhythm() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let run = run_write(PomodoroRunRhythm::Sequence {
            steps: vec![
                PomodoroRunSequenceStep {
                    focus_duration_minutes: 25,
                    break_phase: "short_break".to_string(),
                    break_duration_minutes: 5,
                },
                PomodoroRunSequenceStep {
                    focus_duration_minutes: 35,
                    break_phase: "long_break".to_string(),
                    break_duration_minutes: 12,
                },
            ],
        });
        let segment = initial_segment();

        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        tx.commit().await.unwrap();

        let steps: Vec<(i64, i64, String, i64)> = sqlx::query_as(
            "SELECT step_index, focus_duration_minutes, break_phase, break_duration_minutes
                 FROM pomodoro_run_sequence_steps
                 WHERE run_id = 'run-1'
                 ORDER BY step_index",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            steps,
            vec![
                (0, 25, "short_break".to_string(), 5),
                (1, 35, "long_break".to_string(), 12),
            ],
        );
    });
}

#[test]
fn insert_run_snapshots_adaptive_decision() {
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

        let row = sqlx::query(
            "SELECT rs.policy_version, cs.time_of_day, d.decision_mode,
                        v.selected_numeric_value, ss.confidence
                 FROM pomodoro_run_adaptive_snapshots rs
                 JOIN pomodoro_adaptive_context_snapshots cs ON cs.id = rs.context_snapshot_id
                 JOIN pomodoro_adaptive_decisions d ON d.id = rs.decision_id
                 JOIN pomodoro_adaptive_decision_values v ON v.decision_id = d.id
                 JOIN pomodoro_adaptive_decision_state_scores ss ON ss.decision_id = d.id
                 WHERE rs.run_id = 'run-1' AND v.value_key = 'focus_duration_minutes'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let policy_version: i64 = row.try_get("policy_version").unwrap();
        let time_of_day: String = row.try_get("time_of_day").unwrap();
        let decision_mode: String = row.try_get("decision_mode").unwrap();
        let selected_focus: f64 = row.try_get("selected_numeric_value").unwrap();
        let confidence: f64 = row.try_get("confidence").unwrap();
        assert_eq!(policy_version, 1);
        assert_eq!(time_of_day, "morning");
        assert_eq!(decision_mode, "fallback");
        assert_eq!(selected_focus, 40.0);
        assert_eq!(confidence, 0.0);

        let counts: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT
                    (SELECT COUNT(*) FROM pomodoro_adaptive_policy_bounds),
                    (SELECT COUNT(*) FROM pomodoro_adaptive_context_snapshot_features),
                    (SELECT COUNT(*) FROM pomodoro_adaptive_data_quality_flags),
                    (SELECT COUNT(*) FROM pomodoro_adaptive_decision_reasons)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(counts, (4, 1, 1, 2));
    });
}

#[test]
fn insert_run_snapshots_records_adaptive_candidate_id() {
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
        let mut snapshot = adaptive_snapshot();
        snapshot.decision.candidate_id = Some("focus-growth-with-short-break-support".to_string());
        snapshot.decision.decision_mode = "explore".to_string();
        snapshot.decision.reason_codes = vec!["replay_candidate".to_string()];
        snapshot.decision.values[0].selected_numeric_value = 45.0;
        run.adaptive_snapshot = Some(snapshot);
        let segment = initial_segment();

        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        tx.commit().await.unwrap();

        let row = sqlx::query(
            "SELECT candidate_id, decision_mode
                 FROM pomodoro_adaptive_decisions
                 WHERE id = 'decision-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            row.try_get::<String, _>("candidate_id").unwrap(),
            "focus-growth-with-short-break-support"
        );
        assert_eq!(
            row.try_get::<String, _>("decision_mode").unwrap(),
            "explore"
        );

        let reason_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*)
                 FROM pomodoro_adaptive_decision_reasons
                 WHERE decision_id = 'decision-1' AND reason_code = 'replay_candidate'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reason_count.0, 1);
    });
}

#[test]
fn reject_invalid_adaptive_snapshot_before_insert() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 40,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        let mut snapshot = adaptive_snapshot();
        snapshot.decision.state_scores.confidence = 1.5;
        run.adaptive_snapshot = Some(snapshot);
        let segment = initial_segment();

        let mut tx = pool.begin().await.unwrap();
        let result = insert_run_tx(&mut tx, &run, &segment).await;
        assert!(result.is_err());
        tx.rollback().await.unwrap();

        let run_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_runs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(run_count, 0);
    });
}

#[test]
fn insert_run_persists_provided_adaptive_planned_blocks() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 40,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        let mut snapshot = adaptive_snapshot();
        snapshot.planned_blocks = vec![adaptive_planned_block()];
        run.adaptive_snapshot = Some(snapshot);
        let segment = initial_segment();

        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        tx.commit().await.unwrap();

        let saved: (String, String, String) = sqlx::query_as(
            "SELECT event_id, original_event_id, source_kind
                 FROM pomodoro_adaptive_planned_blocks
                 WHERE event_date = '2026-05-29'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            saved,
            (
                "event-1::2026-05-29".to_string(),
                "event-1".to_string(),
                "scheduler_snapshot".to_string(),
            ),
        );
    });
}

#[test]
fn reject_invalid_adaptive_planned_block_before_insert() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 40,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        let mut snapshot = adaptive_snapshot();
        let mut block = adaptive_planned_block();
        block.event_date = "2026-05-30".to_string();
        snapshot.planned_blocks = vec![block];
        run.adaptive_snapshot = Some(snapshot);
        let segment = initial_segment();

        let mut tx = pool.begin().await.unwrap();
        let result = insert_run_tx(&mut tx, &run, &segment).await;
        assert!(result.is_err());
        tx.rollback().await.unwrap();

        let run_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_runs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(run_count, 0);
    });
}
