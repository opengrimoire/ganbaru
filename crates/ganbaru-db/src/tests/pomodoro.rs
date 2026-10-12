use super::helpers::{insert_event, insert_open_run, migrated_memory_pool};

#[test]
fn schema_accepts_current_pomodoro_preset_keys() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_event(&pool).await;

        sqlx::query(
            "INSERT INTO calendar_event_pomodoro_configs
                (event_id, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes)
             VALUES ('event-1', 'count', 'preset', 'adaptive', 3)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "UPDATE calendar_event_pomodoro_configs SET preset_key = 'balanced' WHERE event_id = 'event-1'",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_runs
                (id, event_id, original_event_id, event_date, planned_start, planned_end,
                 started_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat, start_trigger)
             VALUES ('run-balanced', 'event-1', 'event-1', '2026-05-23',
                     '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z',
                     '2026-05-23T09:00:00Z', 'count', 'preset', 'balanced',
                     '2026-05-23T09:00:00Z', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO calendar_event_archives
                (id, source_event_id, archived_at, title, start_time, end_time,
                 calendar_id, created_at, updated_at)
             VALUES ('archive-1', 'event-1', '2026-05-23T11:00:00Z', 'Focus block',
                     '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z',
                     'local', '2026-05-23T08:00:00Z', '2026-05-23T08:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO calendar_event_archive_pomodoro_configs
                (archive_event_id, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes)
             VALUES ('archive-1', 'count', 'preset', 'adaptive', 3)",
        )
        .execute(&pool)
        .await
        .unwrap();
    });
}

#[test]
fn schema_keeps_pomodoro_foreign_key_targets() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let references = [
            (
                "calendar_event_pomodoro_config_count_rhythms",
                "calendar_event_pomodoro_configs",
            ),
            (
                "calendar_event_pomodoro_config_sequence_steps",
                "calendar_event_pomodoro_configs",
            ),
            ("pomodoro_runs", "pomodoro_runs"),
            ("pomodoro_run_count_rhythms", "pomodoro_runs"),
            ("pomodoro_run_sequence_steps", "pomodoro_runs"),
            ("pomodoro_segments", "pomodoro_runs"),
            ("pomodoro_run_events", "pomodoro_runs"),
            (
                "calendar_event_archive_pomodoro_config_count_rhythms",
                "calendar_event_archive_pomodoro_configs",
            ),
            (
                "calendar_event_archive_pomodoro_config_sequence_steps",
                "calendar_event_archive_pomodoro_configs",
            ),
            ("pomodoro_run_adaptive_snapshots", "pomodoro_runs"),
            ("pomodoro_adaptive_context_snapshots", "pomodoro_runs"),
            ("pomodoro_adaptive_decisions", "pomodoro_runs"),
            ("pomodoro_adaptive_planned_blocks", "pomodoro_runs"),
            ("pomodoro_adaptive_assignments", "pomodoro_runs"),
            ("distractions_block_events", "pomodoro_runs"),
        ];

        for (child_table, target_table) in references {
            let query = format!(
                "SELECT COUNT(*) AS count
                 FROM pragma_foreign_key_list('{child_table}')
                 WHERE \"table\" = '{target_table}'",
            );
            let count: i64 = sqlx::query_scalar(&query).fetch_one(&pool).await.unwrap();
            assert!(count > 0, "{child_table} should reference {target_table}",);
        }
    });
}

#[test]
fn schema_allows_only_one_open_pomodoro_run() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_event(&pool).await;

        insert_open_run(&pool, "run-1").await.unwrap();
        assert!(insert_open_run(&pool, "run-2").await.is_err());

        sqlx::query(
            "UPDATE pomodoro_runs
             SET ended_at = '2026-05-23T09:30:00Z', end_reason = 'stopped'
             WHERE id = 'run-1'",
        )
        .execute(&pool)
        .await
        .unwrap();

        insert_open_run(&pool, "run-2").await.unwrap();
    });
}

#[test]
fn schema_allows_only_one_active_pomodoro_segment() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_event(&pool).await;
        insert_open_run(&pool, "run-1").await.unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_segments
                (id, event_id, event_date, run_id, rhythm_position, phase,
                 planned_start, planned_end, actual_start, status)
             VALUES ('segment-1', 'event-1', '2026-05-23', 'run-1', 1, 'focus',
                     '2026-05-23T09:00:00Z', '2026-05-23T09:40:00Z',
                     '2026-05-23T09:00:00Z', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let second_active = sqlx::query(
            "INSERT INTO pomodoro_segments
                (id, event_id, event_date, run_id, rhythm_position, phase,
                 planned_start, planned_end, actual_start, status)
             VALUES ('segment-2', 'event-1', '2026-05-23', 'run-1', 1, 'short_break',
                     '2026-05-23T09:40:00Z', '2026-05-23T09:45:00Z',
                     '2026-05-23T09:40:00Z', 'active')",
        )
        .execute(&pool)
        .await;

        assert!(second_active.is_err());
    });
}

#[test]
fn schema_accepts_focus_failed_pomodoro_history() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_event(&pool).await;
        insert_open_run(&pool, "run-1").await.unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_segments
                (id, event_id, event_date, run_id, rhythm_position, phase,
                 planned_start, planned_end, actual_start, actual_end, status, end_reason)
             VALUES ('segment-1', 'event-1', '2026-05-23', 'run-1', 1, 'focus',
                     '2026-05-23T09:00:00Z', '2026-05-23T09:40:00Z',
                     '2026-05-23T09:00:00Z', '2026-05-23T09:10:00Z',
                     'interrupted', 'focus_failed')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_run_events
                (id, run_id, segment_id, event_type, occurred_at, phase, reason, duration_seconds)
             VALUES ('event-1', 'run-1', 'segment-1', 'focus_failed',
                     '2026-05-23T09:11:00Z', 'focus', 'long_idle', 60)",
        )
        .execute(&pool)
        .await
        .unwrap();
    });
}

#[test]
fn schema_creates_pomodoro_adaptive_tables() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let adaptive_tables = [
            "pomodoro_adaptive_policies",
            "pomodoro_adaptive_policy_bounds",
            "pomodoro_adaptive_context_states",
            "pomodoro_adaptive_context_state_history",
            "pomodoro_adaptive_context_snapshots",
            "pomodoro_adaptive_context_snapshot_features",
            "pomodoro_adaptive_data_quality_flags",
            "pomodoro_adaptive_decisions",
            "pomodoro_adaptive_decision_values",
            "pomodoro_adaptive_decision_reasons",
            "pomodoro_adaptive_decision_state_scores",
            "pomodoro_run_adaptive_snapshots",
            "pomodoro_adaptive_planned_blocks",
            "pomodoro_adaptive_experiments",
            "pomodoro_adaptive_experiment_variants",
            "pomodoro_adaptive_assignments",
            "pomodoro_adaptive_outcomes",
            "distractions_block_events",
            "distractions_block_event_rule_snapshots",
        ];

        for table in adaptive_tables {
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?")
                    .bind(table)
                    .fetch_optional(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, Some(1), "{table} should exist");
        }
    });
}
