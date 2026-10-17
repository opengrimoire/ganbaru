use super::super::*;
use super::helpers::*;

const POLICY: &str = "local-adaptive-policy-v1";
const CUTOFF: &str = "2026-05-31T00:00:00Z";

/// Seed a real run snapshot and its experiment assignment through native writes.
async fn assigned_pool() -> sqlx::SqlitePool {
    let pool = migrated_pool_with_event().await;
    let mut run = run_write(PomodoroRunRhythm::Count {
        focus_duration_minutes: 45,
        short_break_minutes: 5,
        long_break_minutes: 10,
        long_break_after_focus_count: 4,
    });
    run.rhythm_source = "preset".into();
    run.preset_key = Some("adaptive".into());
    run.adaptive_snapshot = Some(adaptive_snapshot_with_assignment());
    let mut segment = initial_segment();
    segment.planned_end = "2026-05-29T10:45:00Z".into();
    let mut tx = pool.begin().await.unwrap();
    insert_run_tx(&mut tx, &run, &segment).await.unwrap();
    tx.commit().await.unwrap();
    pool
}

/// Each outcome belongs to one assignment and therefore one output group.
async fn insert_outcomes(pool: &sqlx::SqlitePool, count: i64) {
    sqlx::query(
        "WITH RECURSIVE items(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM items WHERE n < ?)
         INSERT INTO pomodoro_adaptive_outcomes
            (id, assignment_id, outcome_window, outcome_key, numeric_value, measured_at)
         SELECT 'budget-outcome-' || n, 'assignment-1', 'run', 'clean_focus_seconds', 1,
                '2026-05-29T10:40:00Z' FROM items",
    )
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn history_admits_complete_boundary_aggregate_and_rejects_one_extra_input_without_writes() {
    super::block_on(async {
        let pool = assigned_pool().await;
        insert_outcomes(&pool, 100_000).await;
        let history = load_adaptive_history_from_pool(&pool, CUTOFF, POLICY, 80)
            .await
            .unwrap();
        assert_eq!(history.experiment_outcomes.len(), 1);
        assert_eq!(history.experiment_outcomes[0].assignment_count, 1);
        assert_eq!(
            history.experiment_outcomes[0].clean_focus_seconds_sum,
            100_000.0
        );
        assert_eq!(
            history.experiment_outcomes[0].clean_focus_seconds_square_sum,
            100_000.0
        );
        sqlx::query("INSERT INTO pomodoro_adaptive_outcomes (id, assignment_id, outcome_window, outcome_key, numeric_value, measured_at) VALUES ('extra', 'assignment-1', 'run', 'clean_focus_seconds', 1, '2026-05-29T10:40:00Z')")
            .execute(&pool).await.unwrap();
        let error = load_adaptive_history_from_pool(&pool, CUTOFF, POLICY, 80)
            .await
            .unwrap_err();
        assert!(error.contains("experiment outcome aggregate exceeds its input work budget"));
        let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_adaptive_outcomes")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(retained, 100_001);
        sqlx::query("DELETE FROM pomodoro_adaptive_outcomes WHERE id = 'extra'")
            .execute(&pool)
            .await
            .unwrap();
        let retried = load_adaptive_history_from_pool(&pool, CUTOFF, POLICY, 80)
            .await
            .unwrap();
        assert_eq!(
            retried.experiment_outcomes[0].clean_focus_seconds_sum,
            100_000.0
        );
    });
}

#[test]
fn replay_shares_aggregate_allowance_across_opportunities_and_can_retry_a_smaller_read() {
    super::block_on(async {
        let pool = assigned_pool().await;
        insert_outcomes(&pool, 50_000).await;
        sqlx::query(
            "WITH RECURSIVE items(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM items WHERE n < 21)
             INSERT INTO pomodoro_adaptive_decisions
                (id, policy_id, run_id, segment_id, context_snapshot_id, opportunity_kind,
                 candidate_id, decision_mode, policy_version, model_version, occurred_at)
             SELECT 'later-decision-' || n, d.policy_id, d.run_id, d.segment_id, d.context_snapshot_id,
                    d.opportunity_kind, d.candidate_id, d.decision_mode, d.policy_version,
                    d.model_version, '2026-05-30T10:00:00Z'
             FROM items CROSS JOIN pomodoro_adaptive_decisions d WHERE d.id = 'decision-1'",
        ).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_decision_values
                (decision_id, value_key, previous_numeric_value, selected_numeric_value, value_unit)
             SELECT d.id, v.value_key, v.previous_numeric_value, v.selected_numeric_value, v.value_unit
             FROM pomodoro_adaptive_decisions d CROSS JOIN pomodoro_adaptive_decision_values v
             WHERE d.id GLOB 'later-decision-*' AND v.decision_id = 'decision-1'",
        ).execute(&pool).await.unwrap();
        let error = load_adaptive_replay_dataset_from_pool(&pool, CUTOFF, POLICY, 50, 80)
            .await
            .unwrap_err();
        assert!(error.contains("experiment outcome aggregate exceeds its input work budget"));
        let smaller = load_adaptive_replay_dataset_from_pool(&pool, CUTOFF, POLICY, 1, 80)
            .await
            .unwrap();
        assert_eq!(smaller.histories.len(), 1);
        assert_eq!(
            smaller.histories[0].history.experiment_outcomes[0].clean_focus_seconds_sum,
            50_000.0
        );
        let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_adaptive_outcomes")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(retained, 50_000);
    });
}

#[test]
fn later_outcomes_are_absent_from_values_but_still_bounded_as_scan_work() {
    super::block_on(async {
        let pool = assigned_pool().await;
        insert_outcomes(&pool, 100_001).await;
        sqlx::query("UPDATE pomodoro_adaptive_outcomes SET measured_at = '2030-01-01T00:00:00Z'")
            .execute(&pool)
            .await
            .unwrap();
        let error = load_adaptive_history_from_pool(&pool, CUTOFF, POLICY, 80)
            .await
            .unwrap_err();
        assert!(error.contains("experiment outcome aggregate exceeds its input work budget"));
        sqlx::query("DELETE FROM pomodoro_adaptive_outcomes WHERE id <> 'budget-outcome-1'")
            .execute(&pool)
            .await
            .unwrap();
        let history = load_adaptive_history_from_pool(&pool, CUTOFF, POLICY, 80)
            .await
            .unwrap();
        assert_eq!(history.experiment_outcomes[0].assignment_count, 1);
        assert_eq!(history.experiment_outcomes[0].clean_focus_seconds_sum, 0.0);
        assert_eq!(history.experiment_outcomes[0].run_observed_count, 0);
    });
}

#[test]
fn aggregate_admission_uses_policy_and_source_indexes_before_looking_up_outcomes() {
    use sqlx::Row;

    super::block_on(async {
        let pool = assigned_pool().await;
        let query = format!(
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM ({} LIMIT ?)",
            super::super::reads::EXPERIMENT_OUTCOME_INPUT_SQL
        );
        let rows = sqlx::query(&query)
            .bind(POLICY)
            .bind(CUTOFF)
            .bind(100_001_i64)
            .fetch_all(&pool)
            .await
            .unwrap();
        let plan: Vec<String> = rows
            .into_iter()
            .map(|row| row.try_get("detail").unwrap())
            .collect();
        for index in [
            "idx_pomodoro_adaptive_experiments_policy",
            "idx_pomodoro_adaptive_assignments_experiment",
            "idx_pomodoro_adaptive_outcomes_assignment",
        ] {
            assert!(
                plan.iter()
                    .any(|step| step.starts_with("SEARCH ") && step.contains(index)),
                "Aggregate admission did not use {index}: {plan:?}"
            );
        }
    });
}
