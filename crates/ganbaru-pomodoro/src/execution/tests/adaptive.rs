use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use chrono::Timelike;

use super::*;
use crate::adaptive::models::{CountRhythm, LocalTimeFact, LocalTimeFacts};

#[derive(Default)]
struct TestLocalTime {
    fail: AtomicBool,
    calls: AtomicUsize,
    incomplete: bool,
}

impl FocusLocalTimeResolver for TestLocalTime {
    fn resolve(
        &self,
        instants: BTreeSet<i64>,
    ) -> Pin<Box<dyn Future<Output = Result<LocalTimeFacts, FocusExecutionError>> + Send + '_>>
    {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.fail.load(Ordering::Relaxed) {
                return Err(decisions::invalid_state("Injected native timezone failure"));
            }
            if self.incomplete {
                return Ok(LocalTimeFacts::new());
            }
            Ok(instants
                .into_iter()
                .map(|epoch_ms| {
                    let local = chrono::DateTime::from_timestamp_millis(epoch_ms).unwrap();
                    (
                        epoch_ms,
                        LocalTimeFact {
                            epoch_ms,
                            date_key: local.format("%Y-%m-%d").to_string(),
                            date_string: local.format("%a %b %d %Y").to_string(),
                            hour: local.hour() as u8,
                        },
                    )
                })
                .collect())
        })
    }
}

fn adaptive_context(now: i64, resolver: Arc<TestLocalTime>) -> FocusExecutionContext {
    let mut value = context(now);
    value.local_time = Some(resolver);
    let commitment = value.commitment.as_mut().unwrap();
    commitment.end_ms = START + 240 * MINUTE;
    commitment.configuration.rhythm = CountRhythm::BASELINE.into_rhythm();
    commitment.configuration.rhythm_source = "preset".to_owned();
    commitment.configuration.preset_key = Some("adaptive".to_owned());
    value.planned_blocks = vec![crate::PomodoroAdaptivePlannedBlockWrite {
        event_date: commitment.event_date.clone(),
        event_id: Some(commitment.occurrence_id.clone()),
        original_event_id: commitment.event_id.clone(),
        planned_start: persistence::format_timestamp(START).unwrap(),
        planned_end: persistence::format_timestamp(commitment.end_ms).unwrap(),
        source_kind: "scheduler_snapshot".to_owned(),
    }];
    value
}

fn adaptive_start_command() -> FocusCommand {
    command(
        "adaptive-start",
        0,
        FocusIntent::StartScheduled {
            occurrence_id: None,
        },
    )
}

async fn seed_completed_focus_history(pool: &SqlitePool) {
    seed_completed_focus_history_before(pool, START).await;
}

async fn seed_completed_focus_history_before(pool: &SqlitePool, before: i64) {
    use crate::{PomodoroRunClosure, PomodoroRunWrite, PomodoroSegmentWrite, writes};
    const DAY: i64 = 24 * 60 * MINUTE;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    for index in 1..=10 {
        let start = before - index * DAY;
        let end = start + 40 * MINUTE;
        let date = chrono::DateTime::from_timestamp_millis(start)
            .unwrap()
            .format("%Y-%m-%d")
            .to_string();
        let run = PomodoroRunWrite {
            id: format!("history-run-{index}"),
            event_id: "focus-event".to_owned(),
            event_date: date.clone(),
            planned_start: persistence::format_timestamp(start).unwrap(),
            planned_end: persistence::format_timestamp(end).unwrap(),
            started_at: persistence::format_timestamp(start).unwrap(),
            rhythm: CountRhythm::BASELINE.into_rhythm(),
            rhythm_source: "custom".to_owned(),
            preset_key: None,
            idle_timeout_minutes: Some(1),
            event_title_snapshot: None,
            inherited_focus_minutes: 0,
            inherited_rhythm_position: 1,
            inherited_from_run_id: None,
            start_trigger: "manual".to_owned(),
            adaptive_snapshot: None,
        };
        let segment = PomodoroSegmentWrite {
            id: format!("history-segment-{index}"),
            event_id: "focus-event".to_owned(),
            event_date: date,
            run_id: run.id.clone(),
            rhythm_position: 1,
            phase: "focus".to_owned(),
            planned_start: persistence::format_timestamp(start).unwrap(),
            planned_end: persistence::format_timestamp(end).unwrap(),
            actual_start: Some(persistence::format_timestamp(start).unwrap()),
            actual_end: None,
            pauses: Vec::new(),
            status: "active".to_owned(),
            end_reason: None,
        };
        writes::insert_run_tx(&mut tx, &run, &segment)
            .await
            .unwrap();
        writes::close_run_tx(
            &mut tx,
            &PomodoroRunClosure {
                run_id: run.id,
                ended_at: persistence::format_timestamp(end).unwrap(),
                end_reason: "completed".to_owned(),
                segment_status: "completed".to_owned(),
                segment_end_reason: "completed".to_owned(),
                event_type: "complete".to_owned(),
            },
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

async fn seed_accepted_replay_candidates(pool: &SqlitePool, resolver: &TestLocalTime) {
    use crate::adaptive::{
        decision::{AdaptiveDecisionInput, POLICY_ID, decision_local_instants},
        replay::{decide_candidate, default_candidates},
        snapshots::{SnapshotIds, run_start_snapshot},
    };
    use crate::{
        PomodoroRunClosure, PomodoroRunWrite, PomodoroSegmentWrite,
        reads::load_adaptive_history_tx, writes,
    };
    const DAY: i64 = 24 * 60 * MINUTE;
    seed_completed_focus_history_before(pool, START - 10 * DAY).await;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    for index in (1..=4).rev() {
        let start = START - index * DAY;
        let at = persistence::format_timestamp(start).unwrap();
        let end = persistence::format_timestamp(start + 240 * MINUTE).unwrap();
        let history = load_adaptive_history_tx(&mut tx, &at, POLICY_ID, 80)
            .await
            .unwrap();
        let input = AdaptiveDecisionInput {
            started_at: at.clone(),
            planned_start: at.clone(),
            planned_end: end.clone(),
            current_rhythm: CountRhythm::BASELINE,
            idle_detection_enabled: true,
            history: Some(history.into()),
        };
        let facts = resolver
            .resolve(decision_local_instants(&input).unwrap())
            .await
            .unwrap();
        let decision = decide_candidate(&input, &default_candidates()[0], &facts).unwrap();
        let run_id = format!("replay-run-{index}");
        let segment_id = format!("replay-segment-{index}");
        let snapshot = run_start_snapshot(
            &decision,
            SnapshotIds {
                run: run_id.clone(),
                segment: segment_id.clone(),
                context: format!("replay-context-{index}"),
                decision: format!("replay-decision-{index}"),
                assignment: format!("replay-assignment-{index}"),
            },
            Vec::new(),
        );
        let date = chrono::DateTime::from_timestamp_millis(start)
            .unwrap()
            .format("%Y-%m-%d")
            .to_string();
        let ended = persistence::format_timestamp(
            start + decision.selected_rhythm.focus_duration_minutes * MINUTE,
        )
        .unwrap();
        writes::insert_run_tx(
            &mut tx,
            &PomodoroRunWrite {
                id: run_id.clone(),
                event_id: "focus-event".to_owned(),
                event_date: date.clone(),
                planned_start: at.clone(),
                planned_end: end,
                started_at: at.clone(),
                rhythm: decision.selected_rhythm.into_rhythm(),
                rhythm_source: "preset".to_owned(),
                preset_key: Some("adaptive".to_owned()),
                idle_timeout_minutes: Some(1),
                event_title_snapshot: None,
                inherited_focus_minutes: 0,
                inherited_rhythm_position: 1,
                inherited_from_run_id: None,
                start_trigger: "manual".to_owned(),
                adaptive_snapshot: Some(snapshot),
            },
            &PomodoroSegmentWrite {
                id: segment_id,
                event_id: "focus-event".to_owned(),
                event_date: date,
                run_id: run_id.clone(),
                rhythm_position: 1,
                phase: "focus".to_owned(),
                planned_start: at.clone(),
                planned_end: ended.clone(),
                actual_start: Some(at),
                actual_end: None,
                pauses: Vec::new(),
                status: "active".to_owned(),
                end_reason: None,
            },
        )
        .await
        .unwrap();
        writes::close_run_tx(
            &mut tx,
            &PomodoroRunClosure {
                run_id,
                ended_at: ended,
                end_reason: "completed".to_owned(),
                segment_status: "completed".to_owned(),
                segment_end_reason: "completed".to_owned(),
                event_type: "complete".to_owned(),
            },
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

#[test]
fn adaptive_execution_commits_a_replay_approved_candidate_and_retries_its_receipt() {
    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime::default());
        seed_accepted_replay_candidates(&pool, &resolver).await;
        let configured = adaptive_context(START, resolver.clone());
        let initial = execute(&pool, &adaptive_start_command(), &configured)
            .await
            .unwrap();
        let candidate: Option<String> = sqlx::query_scalar("SELECT candidate_id FROM pomodoro_adaptive_decisions WHERE run_id = ? AND opportunity_kind = 'run_start'")
            .bind(&initial.run.as_ref().unwrap().id).fetch_one(&pool).await.unwrap();
        assert_eq!(
            candidate.as_deref(),
            Some("focus-growth-with-short-break-support")
        );
        let rhythm =
            CountRhythm::from_rhythm(&initial.run.as_ref().unwrap().configuration.rhythm).unwrap();
        assert_eq!(rhythm.short_break_minutes, 7);
        resolver.fail.store(true, Ordering::Relaxed);
        assert_eq!(
            execute(&pool, &adaptive_start_command(), &configured)
                .await
                .unwrap(),
            initial
        );
    });
}

#[test]
fn adaptive_execution_vetoes_replay_candidates_with_harmful_actual_exposure() {
    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime::default());
        seed_accepted_replay_candidates(&pool, &resolver).await;
        sqlx::query("UPDATE pomodoro_adaptive_outcomes SET boolean_value = 1 WHERE decision_id LIKE 'replay-decision-%' AND outcome_window = 'run' AND outcome_key = 'run_stopped'").execute(&pool).await.unwrap();
        let initial = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver),
        )
        .await
        .unwrap();
        let candidate: Option<String> = sqlx::query_scalar("SELECT candidate_id FROM pomodoro_adaptive_decisions WHERE run_id = ? AND opportunity_kind = 'run_start'")
            .bind(&initial.run.as_ref().unwrap().id).fetch_one(&pool).await.unwrap();
        assert_eq!(candidate, None);
    });
}

#[test]
fn adaptive_execution_rejects_fractional_replay_evidence_and_can_retry_after_repair() {
    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime::default());
        seed_accepted_replay_candidates(&pool, &resolver).await;
        sqlx::query("UPDATE pomodoro_adaptive_decision_values SET previous_numeric_value = 40.5 WHERE decision_id = 'replay-decision-1' AND value_key = 'focus_duration_minutes'").execute(&pool).await.unwrap();
        let configured = adaptive_context(START, resolver);
        let error = execute(&pool, &adaptive_start_command(), &configured)
            .await
            .unwrap_err();
        assert!(error.message.contains("finite nonnegative integer"));
        let open: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_runs WHERE ended_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        let receipt_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_execution_receipts")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(open, 0);
        assert_eq!(receipt_count, 0);
        sqlx::query("UPDATE pomodoro_adaptive_decision_values SET previous_numeric_value = 40 WHERE decision_id = 'replay-decision-1' AND value_key = 'focus_duration_minutes'").execute(&pool).await.unwrap();
        let result = execute(&pool, &adaptive_start_command(), &configured)
            .await
            .unwrap();
        assert_eq!(result.mode, FocusMode::Running);
    });
}

#[test]
fn oversized_adaptive_aggregate_rolls_back_execution_revision_and_receipt_before_retry() {
    use crate::adaptive::decision::POLICY_ID;
    use crate::execution::persistence::format_timestamp;

    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime::default());
        seed_accepted_replay_candidates(&pool, &resolver).await;
        sqlx::query("INSERT INTO pomodoro_adaptive_experiments (id, policy_id, parameter_key, assignment_unit, status, started_at) VALUES ('budget-experiment', ?, 'focus_duration_minutes', 'run', 'active', ?)")
            .bind(POLICY_ID).bind(format_timestamp(START - MINUTE).unwrap()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO pomodoro_adaptive_experiment_variants (experiment_id, variant_key, numeric_value, is_control) VALUES ('budget-experiment', 'control', 40, 1)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO pomodoro_adaptive_assignments (id, experiment_id, variant_key, context_snapshot_id, assignment_seed, assigned_at) SELECT 'budget-assignment', 'budget-experiment', 'control', id, 'seed', ? FROM pomodoro_adaptive_context_snapshots ORDER BY id LIMIT 1")
            .bind(format_timestamp(START - MINUTE).unwrap()).execute(&pool).await.unwrap();
        sqlx::query("WITH RECURSIVE items(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM items WHERE n < 100001) INSERT INTO pomodoro_adaptive_outcomes (id, assignment_id, outcome_window, outcome_key, numeric_value, measured_at) SELECT 'budget-outcome-' || n, 'budget-assignment', 'run', 'clean_focus_seconds', 1, ? FROM items")
            .bind(format_timestamp(START - MINUTE).unwrap()).execute(&pool).await.unwrap();
        let configured = adaptive_context(START, resolver);
        let request = adaptive_start_command();
        let error = execute(&pool, &request, &configured).await.unwrap_err();
        assert!(
            error
                .message
                .contains("aggregate exceeds its input work budget")
        );
        let open: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_runs WHERE ended_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM pomodoro_execution_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        let receipt_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_execution_receipts")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((open, revision, receipt_count), (0, 0, 0));
        sqlx::query(
            "DELETE FROM pomodoro_adaptive_outcomes WHERE assignment_id = 'budget-assignment'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let result = execute(&pool, &request, &configured).await.unwrap();
        assert_eq!(result.mode, FocusMode::Running);
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM pomodoro_execution_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(revision, 1);
        let retry = execute(&pool, &request, &configured).await.unwrap();
        assert_eq!(retry, result);
    });
}

#[test]
fn adaptive_boundary_preserves_the_initial_rhythm_and_recovers_the_latest_accepted_choice() {
    block_on(async {
        let pool = pool().await;
        seed_completed_focus_history(&pool).await;
        let resolver = Arc::new(TestLocalTime::default());
        let mut configured = adaptive_context(START, resolver);
        configured.commitment.as_mut().unwrap().configuration.rhythm = CountRhythm {
            focus_duration_minutes: 45,
            ..CountRhythm::BASELINE
        }
        .into_rhythm();
        let initial = execute(&pool, &adaptive_start_command(), &configured)
            .await
            .unwrap();
        configured.now_ms = initial.phase_deadline_ms.unwrap();
        let boundary = observe(&pool, FocusObservation::Deadline, &configured).await;
        assert_ne!(
            boundary.run.as_ref().unwrap().configuration.rhythm,
            initial.run.as_ref().unwrap().configuration.rhythm
        );
        let original: i64 = sqlx::query_scalar(
            "SELECT focus_duration_minutes FROM pomodoro_run_count_rhythms WHERE run_id = ?",
        )
        .bind(&initial.run.as_ref().unwrap().id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(original, 50);
        let restored = focus_read_execution_snapshot(&pool, configured.now_ms + 1)
            .await
            .unwrap();
        assert_eq!(restored.run, boundary.run);
        assert_eq!(restored.segment, boundary.segment);
    });
}

#[test]
fn adaptive_recovery_rejects_missing_accepted_values_instead_of_reusing_the_initial_rhythm() {
    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime::default());
        let initial = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver),
        )
        .await
        .unwrap();
        sqlx::query("DELETE FROM pomodoro_adaptive_decision_values WHERE decision_id IN (SELECT id FROM pomodoro_adaptive_decisions WHERE run_id = ?)")
            .bind(&initial.run.unwrap().id).execute(&pool).await.unwrap();
        let error = focus_read_execution_snapshot(&pool, START + MINUTE)
            .await
            .unwrap_err();
        assert!(error.message.contains("four values"));
    });
}

#[test]
fn adaptive_reconfiguration_enters_break_when_accepted_focus_is_shorter_than_inherited_progress() {
    block_on(async {
        let pool = pool().await;
        seed_completed_focus_history(&pool).await;
        sqlx::query("UPDATE pomodoro_segments SET status='interrupted', end_reason='focus_failed', actual_end=strftime('%Y-%m-%dT%H:%M:%fZ', planned_start, '+5 minutes') WHERE id LIKE 'history-segment-%'").execute(&pool).await.unwrap();
        let resolver = Arc::new(TestLocalTime::default());
        let mut configured = adaptive_context(START, resolver);
        configured.commitment.as_mut().unwrap().configuration = FocusConfiguration {
            rhythm: CountRhythm {
                focus_duration_minutes: 60,
                ..CountRhythm::BASELINE
            }
            .into_rhythm(),
            rhythm_source: "custom".to_owned(),
            preset_key: None,
            idle_timeout_minutes: Some(1),
        };
        let initial = execute(&pool, &adaptive_start_command(), &configured)
            .await
            .unwrap();
        configured.now_ms += 58 * MINUTE;
        let bucket = crate::adaptive::features::derive_context_bucket(
            chrono::DateTime::from_timestamp_millis(configured.now_ms)
                .unwrap()
                .hour() as u8,
            240.0,
            2,
            58.0,
            None,
            None,
        );
        let key = crate::adaptive::experiments::selection::experiment_context_key(&bucket);
        let at = persistence::format_timestamp(configured.now_ms).unwrap();
        sqlx::query("INSERT INTO pomodoro_adaptive_policies (id, status, policy_version, model_version, exploration_budget_per_week, created_at, updated_at) VALUES (?, 'active', 1, 1, 2, ?, ?)")
            .bind(crate::adaptive::decision::POLICY_ID).bind(&at).bind(&at).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO pomodoro_adaptive_context_states (policy_id, context_key, readiness, strain, recovery_debt, avoidance_pressure, momentum, confidence, updated_at) VALUES (?, ?, 0.1, 1.0, 1.0, 0.1, 0.1, 1.0, ?)")
            .bind(crate::adaptive::decision::POLICY_ID).bind(key).bind(at).execute(&pool).await.unwrap();
        configured
            .commitment
            .as_mut()
            .unwrap()
            .configuration
            .rhythm_source = "preset".to_owned();
        configured
            .commitment
            .as_mut()
            .unwrap()
            .configuration
            .preset_key = Some("adaptive".to_owned());
        let reconfigured = observe(&pool, FocusObservation::CalendarChanged, &configured).await;
        assert_ne!(
            reconfigured.run.as_ref().unwrap().id,
            initial.run.as_ref().unwrap().id
        );
        assert_eq!(
            reconfigured.segment.as_ref().unwrap().phase,
            FocusPhase::ShortBreak,
            "{reconfigured:?}"
        );
        assert_eq!(reconfigured.run.as_ref().unwrap().inherited_phase_ms, 0);
        assert_eq!(
            reconfigured.run.as_ref().unwrap().inherited_focus_ms,
            58 * MINUTE
        );
        assert!(reconfigured.remaining_ms > MINUTE);
    });
}

#[test]
fn adaptive_selected_rhythm_survives_restart_and_is_not_a_calendar_reconfiguration() {
    block_on(async {
        let pool = pool().await;
        seed_completed_focus_history(&pool).await;
        let resolver = Arc::new(TestLocalTime::default());
        let mut configured = adaptive_context(START, resolver.clone());
        configured.commitment.as_mut().unwrap().configuration.rhythm = CountRhythm {
            focus_duration_minutes: 45,
            ..CountRhythm::BASELINE
        }
        .into_rhythm();
        let initial = execute(&pool, &adaptive_start_command(), &configured)
            .await
            .unwrap();
        assert_eq!(
            initial.run.as_ref().unwrap().configuration.rhythm,
            CountRhythm {
                focus_duration_minutes: 50,
                ..CountRhythm::BASELINE
            }
            .into_rhythm()
        );
        configured.now_ms += MINUTE;
        let heartbeat = observe(&pool, FocusObservation::Heartbeat, &configured).await;
        assert_eq!(heartbeat.run, initial.run);
        assert_eq!(heartbeat.revision, initial.revision);
        let restored = focus_read_execution_snapshot(&pool, configured.now_ms)
            .await
            .unwrap();
        assert_eq!(restored.run, initial.run);
        configured.now_ms += MINUTE;
        configured.commitment.as_mut().unwrap().configuration.rhythm =
            CountRhythm::BASELINE.into_rhythm();
        let reconfigured = observe(&pool, FocusObservation::CalendarChanged, &configured).await;
        assert_ne!(
            reconfigured.run.as_ref().unwrap().id,
            initial.run.as_ref().unwrap().id
        );
        let former: String =
            sqlx::query_scalar("SELECT end_reason FROM pomodoro_runs WHERE id = ?")
                .bind(&initial.run.as_ref().unwrap().id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(former, "reconfigured");
    });
}

#[test]
fn adaptive_exploration_assignment_and_exposure_commit_once_with_native_execution() {
    block_on(async {
        let pool = pool().await;
        seed_completed_focus_history(&pool).await;
        let resolver = Arc::new(TestLocalTime::default());
        let initial = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver.clone()),
        )
        .await
        .unwrap();
        let assignment: (String, String, String) = sqlx::query_as("SELECT experiment_id, variant_key, assignment_seed FROM pomodoro_adaptive_assignments WHERE run_id = ?")
            .bind(&initial.run.as_ref().unwrap().id).fetch_one(&pool).await.unwrap();
        assert_eq!(assignment.0, "run-focus-duration-40-vs-45-v1");
        assert!(matches!(assignment.1.as_str(), "control_40" | "focus_45"));
        assert!(!assignment.2.is_empty());
        let retry = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START + MINUTE, resolver.clone()),
        )
        .await
        .unwrap();
        assert_eq!(retry, initial);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_adaptive_assignments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        let stopped = execute(
            &pool,
            &command("adaptive-stop", initial.revision, FocusIntent::Stop),
            &adaptive_context(START + 2 * MINUTE, resolver),
        )
        .await
        .unwrap();
        assert_eq!(stopped.mode, FocusMode::Stopped);
        let observation_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pomodoro_adaptive_outcomes WHERE outcome_window = 'run' AND outcome_key = 'run_stopped' AND boolean_value = 1 AND assignment_id IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(observation_count, 1);
    });
}

#[test]
fn adaptive_execution_commits_native_policy_features_plan_and_receipt_together() {
    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime::default());
        let result = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver.clone()),
        )
        .await
        .unwrap();
        assert_eq!(result.mode, FocusMode::Running);
        assert_eq!(
            result.run.as_ref().unwrap().configuration.rhythm,
            CountRhythm::BASELINE.into_rhythm()
        );
        let row: (i64, i64, i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM pomodoro_run_adaptive_snapshots), (SELECT COUNT(*) FROM pomodoro_adaptive_decision_values), (SELECT COUNT(*) FROM pomodoro_adaptive_context_snapshot_features), (SELECT COUNT(*) FROM pomodoro_execution_receipts)")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(row, (1, 4, 47, 1));
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_adaptive_planned_blocks")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1);
        let flags: Vec<String> = sqlx::query_scalar(
            "SELECT flag FROM pomodoro_adaptive_data_quality_flags ORDER BY flag",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(flags, vec!["diary_missing"]);
        let run_id = result.run.as_ref().unwrap().id.clone();
        let heartbeat = observe(
            &pool,
            FocusObservation::Heartbeat,
            &adaptive_context(START + 30_000, resolver.clone()),
        )
        .await;
        assert_eq!(heartbeat.run.as_ref().unwrap().id, run_id);
        assert_eq!(heartbeat.revision, result.revision);
        let restored = focus_read_execution_snapshot(&pool, START + 31_000)
            .await
            .unwrap();
        assert_eq!(restored.run.unwrap().id, run_id);
        assert_eq!(resolver.calls.load(Ordering::Relaxed), 1);
    });
}

#[test]
fn adaptive_boundary_reads_the_outgoing_phase_and_commits_its_new_snapshot_atomically() {
    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime::default());
        let initial = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver.clone()),
        )
        .await
        .unwrap();
        let result = execute(
            &pool,
            &command("adaptive-break", initial.revision, FocusIntent::Advance),
            &adaptive_context(START + 2 * MINUTE, resolver.clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            result.segment.as_ref().unwrap().phase,
            FocusPhase::ShortBreak
        );
        let observed: (String, f64, f64) = sqlx::query_as("SELECT d.opportunity_kind, completed.numeric_value, clean.numeric_value FROM pomodoro_adaptive_decisions d JOIN pomodoro_adaptive_context_snapshot_features completed ON completed.snapshot_id = d.context_snapshot_id AND completed.feature_key = 'completed_focus_segments' JOIN pomodoro_adaptive_context_snapshot_features clean ON clean.snapshot_id = d.context_snapshot_id AND clean.feature_key = 'clean_focus_seconds' WHERE d.segment_id = ?")
            .bind(&result.segment.as_ref().unwrap().id).fetch_one(&pool).await.unwrap();
        assert_eq!(observed, ("break_start".to_owned(), 1.0, 120.0));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_adaptive_decisions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
        resolver.fail.store(true, Ordering::Relaxed);
        let retry = execute(
            &pool,
            &command("adaptive-break", initial.revision, FocusIntent::Advance),
            &adaptive_context(START + 3 * MINUTE, resolver.clone()),
        )
        .await
        .unwrap();
        assert_eq!(retry, result);
        assert_eq!(resolver.calls.load(Ordering::Relaxed), 2);
    });
}

#[test]
fn adaptive_snapshot_failure_rolls_back_the_run_plan_and_reserved_revision() {
    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime::default());
        sqlx::query("CREATE TRIGGER fail_adaptive_features BEFORE INSERT ON pomodoro_adaptive_context_snapshot_features BEGIN SELECT RAISE(ABORT, 'injected adaptive feature failure'); END").execute(&pool).await.unwrap();
        let error = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver.clone()),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("injected adaptive feature failure"));
        let counts: (i64, i64, i64, i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM pomodoro_runs), (SELECT COUNT(*) FROM pomodoro_segments), (SELECT COUNT(*) FROM pomodoro_adaptive_planned_blocks), (SELECT COUNT(*) FROM pomodoro_adaptive_context_snapshots), (SELECT COUNT(*) FROM pomodoro_execution_receipts)").fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (0, 0, 0, 0, 0));
        assert_eq!(
            focus_read_execution_snapshot(&pool, START)
                .await
                .unwrap()
                .revision,
            0
        );
        sqlx::query("DROP TRIGGER fail_adaptive_features")
            .execute(&pool)
            .await
            .unwrap();
        let accepted = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver),
        )
        .await
        .unwrap();
        assert_eq!(accepted.revision, 1);
    });
}

#[test]
fn adaptive_boundary_failure_preserves_the_previous_phase_rhythm_and_receipt_set() {
    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime::default());
        let initial = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver.clone()),
        )
        .await
        .unwrap();
        sqlx::query("CREATE TRIGGER fail_adaptive_boundary BEFORE INSERT ON pomodoro_adaptive_decisions WHEN NEW.opportunity_kind <> 'run_start' BEGIN SELECT RAISE(ABORT, 'injected adaptive boundary failure'); END").execute(&pool).await.unwrap();
        let error = execute(
            &pool,
            &command("adaptive-break", initial.revision, FocusIntent::Advance),
            &adaptive_context(START + 2 * MINUTE, resolver),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("injected adaptive boundary failure"));
        let canonical = focus_read_execution_snapshot(&pool, START + 2 * MINUTE)
            .await
            .unwrap();
        assert_eq!(canonical.revision, initial.revision);
        assert_eq!(canonical.run, initial.run);
        assert_eq!(
            canonical.segment.as_ref().unwrap().id,
            initial.segment.as_ref().unwrap().id
        );
        assert_eq!(canonical.segment.as_ref().unwrap().status, "active");
        let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM pomodoro_segments), (SELECT COUNT(*) FROM pomodoro_adaptive_decisions), (SELECT COUNT(*) FROM pomodoro_execution_receipts)").fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (1, 1, 1));
    });
}

#[test]
fn adaptive_native_local_time_failure_cannot_create_execution_or_block_an_accepted_retry() {
    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime::default());
        resolver.fail.store(true, Ordering::Relaxed);
        let error = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver.clone()),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("native timezone failure"));
        assert_eq!(
            focus_read_execution_snapshot(&pool, START)
                .await
                .unwrap()
                .revision,
            0
        );
        resolver.fail.store(false, Ordering::Relaxed);
        let accepted = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver.clone()),
        )
        .await
        .unwrap();
        resolver.fail.store(true, Ordering::Relaxed);
        let mut unavailable = adaptive_context(START + MINUTE, resolver.clone());
        unavailable.commitment = None;
        unavailable.local_time = None;
        assert_eq!(
            execute(&pool, &adaptive_start_command(), &unavailable)
                .await
                .unwrap(),
            accepted
        );
        assert_eq!(resolver.calls.load(Ordering::Relaxed), 2);
    });
}

#[test]
fn adaptive_incomplete_native_date_facts_are_rejected_before_snapshot_persistence() {
    block_on(async {
        let pool = pool().await;
        let resolver = Arc::new(TestLocalTime {
            incomplete: true,
            ..TestLocalTime::default()
        });
        let error = execute(
            &pool,
            &adaptive_start_command(),
            &adaptive_context(START, resolver),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("incomplete or invalid"));
        assert_eq!(
            focus_read_execution_snapshot(&pool, START)
                .await
                .unwrap()
                .revision,
            0
        );
    });
}
