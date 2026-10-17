use super::super::*;
use super::helpers::*;
use sqlx::Row;

#[test]
fn insert_run_records_adaptive_experiment_assignment() {
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
        let segment = initial_segment();

        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        tx.commit().await.unwrap();

        let assignment: (String, String, String) = sqlx::query_as(
            "SELECT experiment_id, variant_key, assignment_seed
                 FROM pomodoro_adaptive_assignments
                 WHERE id = 'assignment-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            assignment,
            (
                "run-focus-duration-40-vs-45-v1".to_string(),
                "focus_45".to_string(),
                "seed-1".to_string()
            )
        );

        let variant_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
                 FROM pomodoro_adaptive_experiment_variants
                 WHERE experiment_id = 'run-focus-duration-40-vs-45-v1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(variant_count, 2);

        let selected_focus: f64 = sqlx::query_scalar(
            "SELECT selected_numeric_value
                 FROM pomodoro_adaptive_decision_values
                 WHERE decision_id = 'decision-1'
                   AND value_key = 'focus_duration_minutes'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(selected_focus, 45.0);
    });
}

#[test]
fn insert_run_records_adaptive_bundle_experiment_assignment() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 45,
            short_break_minutes: 7,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        run.rhythm_source = "preset".to_string();
        run.preset_key = Some("adaptive".to_string());
        let mut snapshot = adaptive_snapshot();
        snapshot.decision.decision_mode = "explore".to_string();
        snapshot
            .decision
            .reason_codes
            .push("experiment_assignment".to_string());
        snapshot.decision.values[0].selected_numeric_value = 45.0;
        snapshot.decision.values[1].selected_numeric_value = 7.0;
        snapshot.experiment_assignments = vec![PomodoroAdaptiveExperimentAssignmentWrite {
            experiment: adaptive_bundle_experiment("active"),
            assignment: PomodoroAdaptiveAssignmentWrite {
                id: "assignment-bundle".to_string(),
                experiment_id: "run-focus-short-break-support-40-5-vs-45-7-v1".to_string(),
                variant_key: "focus_45_short_break_7".to_string(),
                run_id: "run-1".to_string(),
                segment_id: Some("segment-1".to_string()),
                context_snapshot_id: "context-1".to_string(),
                assignment_seed: "seed-bundle".to_string(),
                assigned_at: "2026-05-29T10:00:00Z".to_string(),
            },
        }];
        run.adaptive_snapshot = Some(snapshot);
        let segment = initial_segment();

        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        tx.commit().await.unwrap();

        let experiment: (String, String) = sqlx::query_as(
            "SELECT parameter_key, status
                 FROM pomodoro_adaptive_experiments
                 WHERE id = 'run-focus-short-break-support-40-5-vs-45-7-v1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            experiment,
            ("rhythm_bundle".to_string(), "active".to_string())
        );

        let assignment_variant: String = sqlx::query_scalar(
            "SELECT variant_key
                 FROM pomodoro_adaptive_assignments
                 WHERE id = 'assignment-bundle'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(assignment_variant, "focus_45_short_break_7");

        let selected_short_break: f64 = sqlx::query_scalar(
            "SELECT selected_numeric_value
                 FROM pomodoro_adaptive_decision_values
                 WHERE decision_id = 'decision-1'
                   AND value_key = 'short_break_minutes'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(selected_short_break, 7.0);
    });
}

#[test]
fn insert_run_records_adaptive_experiment_status_update_without_assignment() {
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
        let mut snapshot = adaptive_snapshot();
        snapshot.experiment_updates = vec![adaptive_experiment("completed")];
        run.adaptive_snapshot = Some(snapshot);
        let segment = initial_segment();

        let mut tx = pool.begin().await.unwrap();
        insert_run_tx(&mut tx, &run, &segment).await.unwrap();
        tx.commit().await.unwrap();

        let lifecycle: (String, String) = sqlx::query_as(
            "SELECT status, ended_at
                 FROM pomodoro_adaptive_experiments
                 WHERE id = 'run-focus-duration-40-vs-45-v1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let assignment_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
                 FROM pomodoro_adaptive_assignments
                 WHERE experiment_id = 'run-focus-duration-40-vs-45-v1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(
            lifecycle,
            ("completed".to_string(), "2026-05-29T10:00:00Z".to_string())
        );
        assert_eq!(assignment_count, 0);
    });
}

#[test]
fn insert_segment_with_adaptive_decision_records_boundary_decision() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut tx = pool.begin().await.unwrap();
        let run = run_write(PomodoroRunRhythm::Count {
            focus_duration_minutes: 40,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        });
        let initial_segment = initial_segment();
        insert_run_tx(&mut tx, &run, &initial_segment)
            .await
            .unwrap();
        tx.commit().await.unwrap();

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
        validate_adaptive_decision_envelope_for_segment(&envelope, &break_segment).unwrap();
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

        let row = sqlx::query(
            "SELECT d.opportunity_kind, d.segment_id, v.value_key, v.selected_numeric_value
                 FROM pomodoro_adaptive_decisions d
                 JOIN pomodoro_adaptive_decision_values v ON v.decision_id = d.id
                 WHERE d.id = 'decision-boundary-1'
                   AND v.value_key = 'short_break_minutes'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            row.try_get::<String, _>("opportunity_kind").unwrap(),
            "break_start"
        );
        assert_eq!(row.try_get::<String, _>("segment_id").unwrap(), "segment-2");
        assert_eq!(
            row.try_get::<String, _>("value_key").unwrap(),
            "short_break_minutes"
        );
        assert_eq!(
            row.try_get::<f64, _>("selected_numeric_value").unwrap(),
            5.0
        );

        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pomodoro_run_events
                 WHERE run_id = 'run-1'
                   AND segment_id = 'segment-2'
                   AND event_type = 'phase_start'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(event_count, 1);
    });
}
