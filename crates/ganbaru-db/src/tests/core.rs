use super::helpers::{insert_event, insert_open_run, migrated_memory_pool};
use crate::run_migrations;
use sqlx::Row;

const BASELINE_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/migrations/20261004220000_baseline_schema.sql"
));
/// Count the embedded forward migrations so fresh-install checks follow the actual registry.
fn expected_migration_count() -> i64 {
    i64::try_from(
        crate::MIGRATOR
            .iter()
            .filter(|migration| !migration.migration_type.is_down_migration())
            .count(),
    )
    .unwrap()
}

#[test]
fn fresh_database_applies_clean_baseline() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let migration_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(migration_count, expected_migration_count());
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(integrity, "ok");
        let foreign_key_errors: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(foreign_key_errors, 0);

        for object in [
            "calendar_events",
            "project_working_folders",
            "chat_threads",
            "chat_turns",
            "chat_messages",
            "chat_activities",
            "chat_pending_requests",
            "chat_plans",
            "chat_drafts",
            "chat_attachments",
            "chat_attachment_references",
            "chat_queued_followups",
            "chat_queued_attachment_references",
            "chat_user_input_drafts",
            "chat_events",
            "chat_command_receipts",
            "chat_checkpoints",
            "chat_cleanup_jobs",
            "chat_channels",
            "chat_participants",
            "contacts_local_identity",
            "contacts",
            "contact_requests",
            "chat_conversations",
            "chat_conversation_items",
            "chat_reply_threads",
            "chat_work_assignments",
            "chat_agent_runs",
            "chat_communication_search_fts",
            "project_tasks",
            "music_playlists",
            "music_library_items",
            "music_local_roots",
            "music_local_locations",
            "music_source_collections",
            "music_source_collection_items",
            "music_playlist_memberships",
            "music_membership_break_items",
            "music_snoozes",
            "music_listening_statistics",
            "music_recent_selections",
            "music_context_assignments",
            "music_soundscapes",
            "music_soundscape_locations",
            "music_soundscape_state",
            "music_soundscape_active_selections",
            "music_search_fts",
            "notes_pages",
            "notes_blocks",
            "notes_data_source_row_hierarchy",
            "idx_notes_data_source_row_hierarchy_parent",
            "notes_row_hierarchy_detach_moved_page",
            "notes_search_fts",
            "notes_folders_validate_parent_insert",
            "idx_notes_project_history_dirty_deadlines",
        ] {
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE name = ?")
                    .bind(object)
                    .fetch_optional(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, Some(1), "{object} should exist");
        }
        for obsolete in [
            "chat_workspaces",
            "idx_chat_drafts_new_working_folder",
            "project_labels",
            "project_task_label_links",
            "project_view_preferences_new",
            "notes_blocks_next",
            "notes_pages_next",
            "notes_page_history_settings_next",
            "music_playlist_tracks",
            "music_track_skip_ranges",
            "music_track_break_sources",
            "music_library_repair_issues",
            "chat_organizational_drafts",
            "idx_chat_organizational_drafts_destination",
            "chat_source_control_state",
        ] {
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE name = ?")
                    .bind(obsolete)
                    .fetch_optional(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, None, "{obsolete} should not exist");
        }
        let removed_idle_column: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM pragma_table_info('projects') WHERE name = 'default_idle_timeout_minutes'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(removed_idle_column, None);
        let noncanonical_icons: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE icon NOT GLOB '*:*'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(noncanonical_icons, 0);
        assert!(!BASELINE_SCHEMA.contains("ALTER TABLE"));
        assert!(!BASELINE_SCHEMA.contains("DROP TABLE"));
        assert!(!BASELINE_SCHEMA.contains("legacy"));
    });
}

#[test]
fn fresh_file_database_applies_clean_baseline() {
    super::block_on(async {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-ai-baseline-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        let migration_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(migration_count, expected_migration_count());
        pool.close().await;
        std::fs::remove_file(path).unwrap();
    });
}

#[test]
fn schema_normalizes_domain_storage_with_explicit_receipt_and_runtime_envelopes() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        let rows = sqlx::query(
            "SELECT m.name AS table_name, p.name AS column_name
             FROM sqlite_schema AS m, pragma_table_info(m.name) AS p
             WHERE m.type = 'table'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let forbidden_columns = [
            "pause_log",
            "raw_jcal",
            "skip_ranges_json",
            "break_source_json",
            "notifications",
            "exceptions",
            "categories",
            "geo",
            "rdate",
            "extended_properties",
            "organizer",
        ];
        let forbidden_tables = ["pomodoro_sessions"];
        // These bounded native control envelopes are not authoritative Calendar,
        // Notes, Project, Music library or Focus history records. Keep the
        // exception explicit per column so unrelated JSON storage still fails.
        let control_envelopes = [
            ("music_transfer_receipts", "result_json"),
            ("notes_edit_receipts", "result_json"),
            ("project_task_bulk_receipts", "request_json"),
            ("project_task_bulk_receipts", "response_json"),
            ("project_reorder_receipts", "request_json"),
            ("project_reorder_receipts", "response_json"),
            ("project_dependency_cascade_receipts", "request_json"),
            ("project_dependency_cascade_receipts", "response_json"),
            ("pomodoro_execution_state", "state_json"),
            ("pomodoro_execution_receipts", "request_json"),
            ("pomodoro_execution_receipts", "result_json"),
            ("music_session_checkpoints", "checkpoint_json"),
            ("music_session_receipts", "result_json"),
            ("calendar_edit_receipts", "result_json"),
        ];
        let mut observed_envelopes = std::collections::BTreeSet::new();
        for row in rows {
            let table_name: String = row.try_get("table_name").unwrap();
            let column_name: String = row.try_get("column_name").unwrap();
            assert!(
                !forbidden_tables.contains(&table_name.as_str()),
                "{table_name} should not be created as persisted storage",
            );
            let is_control =
                control_envelopes.contains(&(table_name.as_str(), column_name.as_str()));
            if is_control {
                observed_envelopes.insert((table_name.clone(), column_name.clone()));
            }
            assert!(
                !forbidden_columns.contains(&column_name.as_str())
                    && (!column_name.ends_with("_json") || is_control),
                "{table_name}.{column_name} should be normalized, not JSON storage",
            );
        }
        assert_eq!(observed_envelopes.len(), control_envelopes.len());
    });
}

#[test]
fn schema_allows_only_one_open_pause_per_segment() {
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

        sqlx::query(
            "INSERT INTO pomodoro_pauses (id, segment_id, started_at, reason)
             VALUES ('pause-1', 'segment-1', '2026-05-23T09:10:00Z', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let second_open_pause = sqlx::query(
            "INSERT INTO pomodoro_pauses (id, segment_id, started_at, reason)
             VALUES ('pause-2', 'segment-1', '2026-05-23T09:15:00Z', 'idle')",
        )
        .execute(&pool)
        .await;

        assert!(second_open_pause.is_err());
    });
}

#[test]
fn schema_enforces_adaptive_policy_and_decision_integrity() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_event(&pool).await;
        insert_open_run(&pool, "run-1").await.unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_policies
                (id, status, policy_version, model_version)
             VALUES ('policy-1', 'active', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let second_active_policy = sqlx::query(
            "INSERT INTO pomodoro_adaptive_policies
                (id, status, policy_version, model_version)
             VALUES ('policy-2', 'active', 1, 1)",
        )
        .execute(&pool)
        .await;
        assert!(second_active_policy.is_err());

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_policy_bounds
                (policy_id, parameter_key, min_value, max_value)
             VALUES ('policy-1', 'focus_duration_minutes', 15, 60)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let invalid_bounds = sqlx::query(
            "INSERT INTO pomodoro_adaptive_policy_bounds
                (policy_id, parameter_key, min_value, max_value)
             VALUES ('policy-1', 'short_break_minutes', 12, 3)",
        )
        .execute(&pool)
        .await;
        assert!(invalid_bounds.is_err());

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_context_snapshots
                (id, run_id, local_started_at, time_of_day, session_position,
                 event_length, workload, energy)
             VALUES ('snapshot-1', 'run-1', '2026-05-23T09:00:00Z',
                     'morning', 'first', 'medium', 'low', 'unknown')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_context_snapshot_features
                (snapshot_id, feature_key, numeric_value, source_kind)
             VALUES ('snapshot-1', 'clean_focus_seconds', 2400, 'pomodoro')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_data_quality_flags (snapshot_id, flag)
             VALUES ('snapshot-1', 'diary_missing')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_decisions
                (id, policy_id, run_id, context_snapshot_id, opportunity_kind,
                 decision_mode, policy_version, model_version, occurred_at)
             VALUES ('decision-1', 'policy-1', 'run-1', 'snapshot-1',
                     'run_start', 'fallback', 1, 1, '2026-05-23T09:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_decision_values
                (decision_id, value_key, previous_numeric_value, selected_numeric_value, value_unit)
             VALUES ('decision-1', 'focus_duration_minutes', 40, 40, 'minutes')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let invalid_bundle_value = sqlx::query(
            "INSERT INTO pomodoro_adaptive_decision_values
                (decision_id, value_key, previous_numeric_value, selected_numeric_value, value_unit)
             VALUES ('decision-1', 'rhythm_bundle', 0, 1, 'count')",
        )
        .execute(&pool)
        .await;
        assert!(invalid_bundle_value.is_err());

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_decision_reasons (decision_id, reason_code)
             VALUES ('decision-1', 'no_history')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let invalid_reason = sqlx::query(
            "INSERT INTO pomodoro_adaptive_decision_reasons (decision_id, reason_code)
             VALUES ('decision-1', 'raw_anxiety_label')",
        )
        .execute(&pool)
        .await;
        assert!(invalid_reason.is_err());

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_decision_state_scores
                (decision_id, readiness, strain, recovery_debt,
                 avoidance_pressure, momentum, confidence)
             VALUES ('decision-1', 0.2, 0.1, 0.1, 0.0, 0.2, 0.1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let invalid_score = sqlx::query(
            "INSERT INTO pomodoro_adaptive_decision_state_scores
                (decision_id, readiness, strain, recovery_debt,
                 avoidance_pressure, momentum, confidence)
             VALUES ('decision-bad', 1.2, 0.1, 0.1, 0.0, 0.2, 0.1)",
        )
        .execute(&pool)
        .await;
        assert!(invalid_score.is_err());

        sqlx::query(
            "INSERT INTO pomodoro_run_adaptive_snapshots
                (run_id, policy_id, policy_version, model_version,
                 context_snapshot_id, decision_id)
             VALUES ('run-1', 'policy-1', 1, 1, 'snapshot-1', 'decision-1')",
        )
        .execute(&pool)
        .await
        .unwrap();
    });
}

#[test]
fn schema_records_adaptive_experiments_and_outcomes() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_event(&pool).await;
        insert_open_run(&pool, "run-1").await.unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_policies
                (id, status, policy_version, model_version)
             VALUES ('policy-1', 'active', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_context_snapshots
                (id, run_id, local_started_at, time_of_day, session_position,
                 event_length, workload, energy)
             VALUES ('snapshot-1', 'run-1', '2026-05-23T09:00:00Z',
                     'morning', 'first', 'medium', 'low', 'unknown')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_experiments
                (id, policy_id, parameter_key, assignment_unit, status, started_at)
             VALUES ('experiment-1', 'policy-1', 'focus_duration_minutes',
                     'run', 'active', '2026-05-23T09:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_experiments
                (id, policy_id, parameter_key, assignment_unit, status, started_at)
             VALUES ('experiment-bundle', 'policy-1', 'rhythm_bundle',
                     'run', 'active', '2026-05-23T09:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let invalid_bundle_bound = sqlx::query(
            "INSERT INTO pomodoro_adaptive_policy_bounds
                (policy_id, parameter_key, min_value, max_value)
             VALUES ('policy-1', 'rhythm_bundle', 0, 1)",
        )
        .execute(&pool)
        .await;
        assert!(invalid_bundle_bound.is_err());

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_experiment_variants
                (experiment_id, variant_key, numeric_value, is_control)
             VALUES ('experiment-1', 'control', 40, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_experiment_variants
                (experiment_id, variant_key, numeric_value, is_control)
             VALUES ('experiment-1', 'shorter', 35, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let second_control = sqlx::query(
            "INSERT INTO pomodoro_adaptive_experiment_variants
                (experiment_id, variant_key, numeric_value, is_control)
             VALUES ('experiment-1', 'other-control', 45, 1)",
        )
        .execute(&pool)
        .await;
        assert!(second_control.is_err());

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_assignments
                (id, experiment_id, variant_key, run_id,
                 context_snapshot_id, assignment_seed, assigned_at)
             VALUES ('assignment-1', 'experiment-1', 'shorter', 'run-1',
                     'snapshot-1', 'seed-1', '2026-05-23T09:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let invalid_assignment = sqlx::query(
            "INSERT INTO pomodoro_adaptive_assignments
                (id, experiment_id, variant_key, assignment_seed, assigned_at)
             VALUES ('assignment-bad', 'experiment-1', 'missing',
                     'seed-2', '2026-05-23T09:00:00Z')",
        )
        .execute(&pool)
        .await;
        assert!(invalid_assignment.is_err());

        sqlx::query(
            "INSERT INTO pomodoro_adaptive_outcomes
                (id, assignment_id, outcome_window, outcome_key,
                 boolean_value, measured_at)
             VALUES ('outcome-1', 'assignment-1', 'run',
                     'clean_focus_completed', 1, '2026-05-23T10:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
    });
}
