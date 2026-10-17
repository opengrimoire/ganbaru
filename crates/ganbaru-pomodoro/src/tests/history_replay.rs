use super::super::*;
use super::helpers::*;

#[test]
fn load_adaptive_history_reads_recent_signals() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        sqlx::query(
            "INSERT INTO pomodoro_runs
                    (id, event_id, original_event_id, event_date, planned_start, planned_end,
                     started_at, ended_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
                     end_reason, start_trigger)
                 VALUES ('run-1', 'event-1', 'event-1', '2026-05-29',
                         '2026-05-29T10:00:00Z', '2026-05-29T11:00:00Z',
                         '2026-05-29T10:00:00Z', '2026-05-29T10:20:00Z',
                         'count', 'preset', 'adaptive', '2026-05-29T10:20:00Z',
                         'stopped', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_segments
                    (id, event_id, event_date, run_id, rhythm_position, phase,
                     planned_start, planned_end, actual_start, actual_end, status, end_reason)
                 VALUES ('segment-1', 'event-1', '2026-05-29', 'run-1', 1, 'focus',
                         '2026-05-29T10:00:00Z', '2026-05-29T10:40:00Z',
                         '2026-05-29T10:00:00Z', '2026-05-29T10:20:00Z',
                         'interrupted', 'focus_failed')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_pauses (id, segment_id, started_at, ended_at, reason)
                 VALUES ('pause-1', 'segment-1', '2026-05-29T10:10:00Z',
                         '2026-05-29T10:14:00Z', 'idle')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
                "INSERT INTO pomodoro_run_events
                    (id, run_id, segment_id, event_type, occurred_at, phase, reason, duration_seconds)
                 VALUES ('run-event-1', 'run-1', 'segment-1', 'focus_failed',
                         '2026-05-29T10:20:00Z', 'focus', 'long_idle', 300)",
            )
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_policies
                    (id, status, policy_version, model_version)
                 VALUES ('local-adaptive-policy-v1', 'active', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_context_states
                    (policy_id, context_key, readiness, strain, recovery_debt,
                     avoidance_pressure, momentum, confidence, updated_at)
                 VALUES ('local-adaptive-policy-v1', 'morning:first', 0.2, 0.7, 0.6,
                         0.4, 0.1, 0.8, '2026-05-29T10:30:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO distractions_block_events
                    (id, run_id, segment_id, occurred_at, source_type, source_key,
                     phase, decision)
                 VALUES ('block-1', 'run-1', 'segment-1', '2026-05-29T10:05:00Z',
                         'browser', 'youtube.com', 'focus', 'blocked')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let history = load_adaptive_history_from_pool(
            &pool,
            "2026-05-29T11:00:00Z",
            "local-adaptive-policy-v1",
            20,
        )
        .await
        .unwrap();

        assert_eq!(history.segments.len(), 1);
        assert_eq!(history.segments[0].run_id, "run-1");
        assert_eq!(history.segments[0].rhythm_position, 1);
        assert_eq!(
            history.segments[0].end_reason.as_deref(),
            Some("focus_failed")
        );
        assert_eq!(history.segments[0].pause_log.len(), 1);
        assert_eq!(history.run_events.len(), 1);
        assert_eq!(history.run_events[0].event_type, "focus_failed");
        assert_eq!(history.block_events.len(), 1);
        assert_eq!(history.block_events[0].source_key, "youtube.com");
        assert_eq!(history.previous_states.len(), 1);
        assert_eq!(history.previous_states[0].context_key, "morning:first");
        assert_eq!(history.previous_states[0].confidence, 0.8);
        assert!(history.experiment_states.is_empty());
        assert!(history.experiment_outcomes.is_empty());
        assert!(history.experiment_assignments.is_empty());
    });
}

#[test]
fn adaptive_history_does_not_use_segments_or_events_observed_after_cutoff() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut tx = pool.begin().await.unwrap();
        writes::insert_run_tx(
            &mut tx,
            &run_write(adaptive::models::CountRhythm::BASELINE.into_rhythm()),
            &initial_segment(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("UPDATE pomodoro_segments SET status = 'completed', actual_end = '2026-05-29T10:40:00Z', end_reason = 'completed'").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO pomodoro_run_events (id, run_id, segment_id, event_type, occurred_at, phase) VALUES ('future-stop', 'run-1', 'segment-1', 'stop', '2026-05-29T12:00:00Z', 'focus')").execute(&pool).await.unwrap();
        let during = load_adaptive_history_from_pool(
            &pool,
            "2026-05-29T10:20:00Z",
            adaptive::decision::POLICY_ID,
            80,
        )
        .await
        .unwrap();
        assert!(during.segments.is_empty());
        let after = load_adaptive_history_from_pool(
            &pool,
            "2026-05-29T11:00:00Z",
            adaptive::decision::POLICY_ID,
            80,
        )
        .await
        .unwrap();
        assert_eq!(after.segments.len(), 1);
        assert!(!after.run_events.is_empty());
        assert!(
            after
                .run_events
                .iter()
                .all(|event| event.event_type != "stop")
        );
    });
}

#[test]
fn adaptive_history_rejects_oversized_pause_evidence_without_truncating_it() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        let mut tx = pool.begin().await.unwrap();
        writes::insert_run_tx(
            &mut tx,
            &run_write(adaptive::models::CountRhythm::BASELINE.into_rhythm()),
            &initial_segment(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("UPDATE pomodoro_segments SET status = 'completed', actual_end = '2026-05-29T10:40:00Z', end_reason = 'completed'").execute(&pool).await.unwrap();
        sqlx::query("WITH RECURSIVE indices(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM indices WHERE n < 4096) INSERT INTO pomodoro_pauses (id, segment_id, started_at, ended_at, reason) SELECT 'pause-' || n, 'segment-1', '2026-05-29T10:10:00Z', '2026-05-29T10:11:00Z', 'manual' FROM indices").execute(&pool).await.unwrap();
        let error = load_adaptive_history_from_pool(
            &pool,
            "2026-05-29T11:00:00Z",
            adaptive::decision::POLICY_ID,
            80,
        )
        .await
        .unwrap_err();
        assert!(error.contains("pause evidence exceeds its row limit"));
        let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_pauses")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(retained, 4097);
    });
}

#[test]
fn load_adaptive_history_reads_experiment_outcome_summaries() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        sqlx::query(
            "INSERT INTO pomodoro_runs
                    (id, event_id, original_event_id, event_date, planned_start, planned_end,
                     started_at, ended_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
                     end_reason, start_trigger)
                 VALUES ('run-1', 'event-1', 'event-1', '2026-05-29',
                         '2026-05-29T10:00:00Z', '2026-05-29T11:00:00Z',
                         '2026-05-29T10:00:00Z', '2026-05-29T10:45:00Z',
                         'count', 'preset', 'adaptive', '2026-05-29T10:45:00Z',
                         'completed', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_policies
                    (id, status, policy_version, model_version)
                 VALUES ('local-adaptive-policy-v1', 'active', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_context_snapshots
                    (id, run_id, local_started_at, time_of_day, session_position,
                     event_length, workload, energy)
                 VALUES ('context-control', 'run-1', '2026-05-29T10:00:00Z',
                         'morning', 'first', 'medium', 'low', 'unknown'),
                        ('context-treatment', 'run-1', '2026-05-29T11:00:00Z',
                         'morning', 'first', 'medium', 'low', 'unknown')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_experiments
                    (id, policy_id, parameter_key, assignment_unit, status, started_at)
                 VALUES ('run-focus-duration-40-vs-45-v1', 'local-adaptive-policy-v1',
                         'focus_duration_minutes', 'run', 'active', '2026-05-29T10:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_experiment_variants
                    (experiment_id, variant_key, numeric_value, is_control)
                 VALUES ('run-focus-duration-40-vs-45-v1', 'control_40', 40, 1),
                        ('run-focus-duration-40-vs-45-v1', 'focus_45', 45, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_assignments
                    (id, experiment_id, variant_key, run_id, context_snapshot_id,
                     assignment_seed, assigned_at)
                 VALUES ('assignment-control', 'run-focus-duration-40-vs-45-v1',
                         'control_40', 'run-1', 'context-control', 'seed-control',
                         '2026-05-29T10:00:00Z'),
                        ('assignment-treatment', 'run-focus-duration-40-vs-45-v1',
                         'focus_45', 'run-1', 'context-treatment', 'seed-treatment',
                         '2026-05-29T11:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_outcomes
                    (id, assignment_id, outcome_window, outcome_key, numeric_value,
                     boolean_value, measured_at)
                 VALUES
                    ('outcome-control-completed', 'assignment-control', 'run',
                     'run_completed', NULL, 1, '2026-05-29T10:40:00Z'),
                    ('outcome-control-clean', 'assignment-control', 'run',
                     'clean_focus_seconds', 2400, NULL, '2026-05-29T10:40:00Z'),
                    ('outcome-control-blocked', 'assignment-control', 'run',
                     'blocked_attempt_count', 1, NULL, '2026-05-29T10:40:00Z'),
                    ('outcome-control-short-break-overtime', 'assignment-control', 'run',
                     'short_break_overtime_seconds', 90, NULL, '2026-05-29T10:40:00Z'),
                    ('outcome-control-day', 'assignment-control', 'day',
                     'day_observed', NULL, 1, '2026-05-30T09:00:00Z'),
                    ('outcome-control-day-started', 'assignment-control', 'day',
                     'day_started_planned_pomodoro_event_count', 2, NULL,
                     '2026-05-30T09:00:00Z'),
                    ('outcome-control-day-missed', 'assignment-control', 'day',
                     'day_missed_planned_pomodoro_event_count', 0, NULL,
                     '2026-05-30T09:00:00Z'),
                    ('outcome-control-day-clean', 'assignment-control', 'day',
                     'day_clean_focus_seconds', 4800, NULL, '2026-05-30T09:00:00Z'),
                    ('outcome-treatment-completed', 'assignment-treatment', 'run',
                     'run_completed', NULL, 0, '2026-05-29T11:45:00Z'),
                    ('outcome-treatment-stopped', 'assignment-treatment', 'run',
                     'run_stopped', NULL, 1, '2026-05-29T11:45:00Z'),
                    ('outcome-treatment-clean', 'assignment-treatment', 'run',
                     'clean_focus_seconds', 1800, NULL, '2026-05-29T11:45:00Z'),
                    ('outcome-treatment-short-break-overtime', 'assignment-treatment', 'run',
                     'short_break_overtime_seconds', 30, NULL, '2026-05-29T11:45:00Z'),
                    ('outcome-treatment-day', 'assignment-treatment', 'day',
                     'day_observed', NULL, 1, '2026-05-30T09:00:00Z'),
                    ('outcome-treatment-day-started', 'assignment-treatment', 'day',
                     'day_started_planned_pomodoro_event_count', 1, NULL,
                     '2026-05-30T09:00:00Z'),
                    ('outcome-treatment-day-missed', 'assignment-treatment', 'day',
                     'day_missed_planned_pomodoro_event_count', 1, NULL,
                     '2026-05-30T09:00:00Z'),
                    ('outcome-treatment-day-blocked', 'assignment-treatment', 'day',
                     'day_blocked_attempt_count', 4, NULL, '2026-05-30T09:00:00Z'),
                    ('outcome-treatment-next-day', 'assignment-treatment', 'next_day',
                     'next_day_observed', NULL, 1, '2026-05-31T09:00:00Z'),
                    ('outcome-treatment-next-day-started', 'assignment-treatment', 'next_day',
                     'next_day_started_run', NULL, 0, '2026-05-31T09:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let history = load_adaptive_history_from_pool(
            &pool,
            "2026-06-01T00:00:00Z",
            "local-adaptive-policy-v1",
            20,
        )
        .await
        .unwrap();

        assert_eq!(history.experiment_states.len(), 1);
        assert_eq!(
            history.experiment_states[0].experiment_id,
            "run-focus-duration-40-vs-45-v1"
        );
        assert_eq!(history.experiment_states[0].status, "active");
        assert_eq!(history.experiment_assignments.len(), 2);
        assert_eq!(
            history.experiment_assignments[0].experiment_id,
            "run-focus-duration-40-vs-45-v1"
        );
        assert_eq!(history.experiment_assignments[0].variant_key, "focus_45");
        assert_eq!(
            history.experiment_assignments[0].context_key,
            "morning:first:medium:low:unknown:none"
        );
        assert_eq!(
            history.experiment_assignments[0].assigned_at,
            "2026-05-29T11:00:00Z"
        );
        assert_eq!(history.experiment_outcomes.len(), 2);
        let control = history
            .experiment_outcomes
            .iter()
            .find(|outcome| outcome.variant_key == "control_40")
            .unwrap();
        let treatment = history
            .experiment_outcomes
            .iter()
            .find(|outcome| outcome.variant_key == "focus_45")
            .unwrap();
        assert_eq!(control.assignment_count, 1);
        assert_eq!(control.context_key, "morning:first:medium:low:unknown:none");
        assert_eq!(control.run_observed_count, 1);
        assert_eq!(control.run_completed_count, 1);
        assert_eq!(control.clean_focus_seconds_sum, 2400.0);
        assert_eq!(control.clean_focus_seconds_square_sum, 5_760_000.0);
        assert_eq!(control.blocked_attempt_count_sum, 1.0);
        assert_eq!(control.blocked_attempt_count_square_sum, 1.0);
        assert_eq!(control.short_break_overtime_seconds_sum, 90.0);
        assert_eq!(control.short_break_overtime_seconds_square_sum, 8_100.0);
        assert_eq!(control.day_observed_count, 1);
        assert_eq!(control.day_started_planned_pomodoro_count_sum, 2.0);
        assert_eq!(control.day_missed_planned_pomodoro_count_sum, 0.0);
        assert_eq!(control.day_missed_planned_pomodoro_count_square_sum, 0.0);
        assert_eq!(control.day_clean_focus_seconds_sum, 4800.0);
        assert_eq!(treatment.assignment_count, 1);
        assert_eq!(treatment.run_observed_count, 1);
        assert_eq!(treatment.run_completed_count, 0);
        assert_eq!(treatment.run_stopped_count, 1);
        assert_eq!(treatment.short_break_overtime_seconds_sum, 30.0);
        assert_eq!(treatment.short_break_overtime_seconds_square_sum, 900.0);
        assert_eq!(treatment.day_observed_count, 1);
        assert_eq!(treatment.day_missed_planned_pomodoro_count_sum, 1.0);
        assert_eq!(treatment.day_missed_planned_pomodoro_count_square_sum, 1.0);
        assert_eq!(treatment.day_blocked_attempt_count_sum, 4.0);
        assert_eq!(treatment.day_blocked_attempt_count_square_sum, 16.0);
        assert_eq!(treatment.next_day_observed_count, 1);
        assert_eq!(treatment.next_day_started_run_count, 0);
    });
}

#[test]
fn load_adaptive_replay_dataset_reads_run_start_decisions_and_only_available_outcomes() {
    super::block_on(async {
        let pool = migrated_pool_with_event().await;
        sqlx::query(
            "INSERT INTO pomodoro_runs
                    (id, event_id, original_event_id, event_date, planned_start, planned_end,
                     started_at, ended_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
                     end_reason, start_trigger)
                 VALUES ('run-1', 'event-1', 'event-1', '2026-05-29',
                         '2026-05-29T10:00:00Z', '2026-05-29T11:00:00Z',
                         '2026-05-29T10:00:00Z', '2026-05-29T10:45:00Z',
                         'count', 'preset', 'adaptive', '2026-05-29T10:45:00Z',
                         'completed', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_policies
                    (id, status, policy_version, model_version)
                 VALUES ('local-adaptive-policy-v1', 'active', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_runs
                    (id, event_id, original_event_id, event_date, planned_start, planned_end,
                     started_at, ended_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
                     end_reason, start_trigger)
                 VALUES ('run-prior', 'event-1', 'event-1', '2026-05-28',
                         '2026-05-28T09:00:00Z', '2026-05-28T10:00:00Z',
                         '2026-05-28T09:00:00Z', '2026-05-28T09:40:00Z',
                         'count', 'preset', 'adaptive', '2026-05-28T09:40:00Z',
                         'completed', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_segments
                    (id, event_id, event_date, run_id, rhythm_position, phase,
                     planned_start, planned_end, actual_start, actual_end, status, end_reason)
                 VALUES ('segment-prior', 'event-1', '2026-05-28', 'run-prior', 1,
                         'focus', '2026-05-28T09:00:00Z', '2026-05-28T09:40:00Z',
                         '2026-05-28T09:00:00Z', '2026-05-28T09:40:00Z',
                         'completed', 'completed')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_context_state_history
                    (id, policy_id, context_key, observed_at, readiness, strain,
                     recovery_debt, avoidance_pressure, momentum, confidence)
                 VALUES ('state-before', 'local-adaptive-policy-v1',
                         'morning:first:medium:low:unknown:none',
                         '2026-05-28T10:00:00Z', 0.5, 0.2, 0.2, 0.1, 0.6, 0.7),
                        ('state-after', 'local-adaptive-policy-v1',
                         'morning:first:medium:low:unknown:none',
                         '2026-05-29T11:00:00Z', 0.9, 0.1, 0.1, 0.0, 0.9, 0.95)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_context_snapshots
                    (id, run_id, local_started_at, time_of_day, session_position,
                     event_length, workload, energy)
                 VALUES ('context-1', 'run-1', '2026-05-29T10:00:00Z',
                         'morning', 'first', 'medium', 'low', 'unknown')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_decisions
                    (id, policy_id, run_id, context_snapshot_id, opportunity_kind,
                     candidate_id, decision_mode, policy_version, model_version, occurred_at)
                 VALUES ('decision-1', 'local-adaptive-policy-v1', 'run-1',
                         'context-1', 'run_start',
                         'focus-growth-with-short-break-support',
                         'explore', 1, 1,
                         '2026-05-29T10:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_decision_values
                    (decision_id, value_key, previous_numeric_value, selected_numeric_value,
                     value_unit)
                 VALUES
                    ('decision-1', 'focus_duration_minutes', 40, 45, 'minutes'),
                    ('decision-1', 'short_break_minutes', 5, 5, 'minutes'),
                    ('decision-1', 'long_break_minutes', 10, 10, 'minutes'),
                    ('decision-1', 'long_break_after_focus_count', 4, 4, 'count')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_outcomes
                    (id, decision_id, outcome_window, outcome_key, numeric_value,
                     boolean_value, measured_at)
                 VALUES
                    ('outcome-clean', 'decision-1', 'run', 'clean_focus_seconds',
                     2700, NULL, '2026-05-29T10:45:00Z'),
                    ('outcome-completed', 'decision-1', 'run', 'run_completed',
                     NULL, 1, '2026-05-29T10:45:00Z'),
                    ('outcome-future-stop', 'decision-1', 'run', 'run_stopped',
                     NULL, 1, '2026-05-31T10:45:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let prior_segment_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
                 FROM pomodoro_segments
                 WHERE planned_start < '2026-05-29T10:00:00Z'
                   AND status IN ('completed', 'interrupted')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(prior_segment_count, 1);

        let dataset = load_adaptive_replay_dataset_from_pool(
            &pool,
            "2026-05-30T00:00:00Z",
            "local-adaptive-policy-v1",
            20,
            20,
        )
        .await
        .unwrap();

        assert_eq!(dataset.opportunities.len(), 1);
        let opportunity = &dataset.opportunities[0];
        assert_eq!(opportunity.id, "decision-1");
        assert_eq!(opportunity.run_id, "run-1");
        assert_eq!(opportunity.started_at, "2026-05-29T10:00:00Z");
        assert_eq!(
            opportunity.candidate_id.as_deref(),
            Some("focus-growth-with-short-break-support")
        );
        match &opportunity.current_rhythm {
            PomodoroRunRhythm::Count {
                focus_duration_minutes,
                short_break_minutes,
                long_break_minutes,
                long_break_after_focus_count,
            } => {
                assert_eq!(*focus_duration_minutes, 40);
                assert_eq!(*short_break_minutes, 5);
                assert_eq!(*long_break_minutes, 10);
                assert_eq!(*long_break_after_focus_count, 4);
            }
            PomodoroRunRhythm::Sequence { .. } => panic!("expected count rhythm"),
        }
        match &opportunity.selected_rhythm {
            PomodoroRunRhythm::Count {
                focus_duration_minutes,
                ..
            } => assert_eq!(*focus_duration_minutes, 45),
            PomodoroRunRhythm::Sequence { .. } => panic!("expected count rhythm"),
        }
        assert_eq!(dataset.outcomes.len(), 2);
        let completed = dataset
            .outcomes
            .iter()
            .find(|outcome| outcome.outcome_key == "run_completed")
            .unwrap();
        assert_eq!(completed.boolean_value, Some(true));
        assert_eq!(dataset.histories.len(), 1);
        assert_eq!(dataset.histories[0].opportunity_id, "decision-1");
        assert_eq!(dataset.histories[0].history.segments.len(), 1);
        assert_eq!(dataset.histories[0].history.segments[0].run_id, "run-prior");
        assert_eq!(dataset.histories[0].history.previous_states.len(), 1);
        assert_eq!(
            dataset.histories[0].history.previous_states[0].confidence,
            0.7
        );
    });
}
