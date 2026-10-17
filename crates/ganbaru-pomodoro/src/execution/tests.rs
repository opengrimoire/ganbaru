use sqlx::SqlitePool;

use super::*;
use crate::PomodoroRunRhythm;

mod adaptive;
mod calendar;

const START: i64 = 1_779_444_000_000;
const MINUTE: i64 = 60_000;

#[test]
fn phase_consumers_use_shared_ownership_and_cannot_renew_past_accepted_deadlines() {
    let mut effect = CommittedFocusEffect {
        vault_id: "vault".into(),
        vault_generation: 47,
        ownership_generation: 3,
        execution_revision: 6,
        run_id: Some("run".into()),
        segment_id: Some("segment".into()),
        event_id: Some("event".into()),
        occurrence_id: Some("event::date".into()),
        event_title: None,
        phase: Some(FocusPhase::Focus),
        mode: FocusMode::Running,
        phase_deadline_ms: Some(1500),
        event_deadline_ms: Some(2000),
        remaining_ms: 500,
        valid_until_ms: 16000,
    };
    assert!(effect.matches_vault_authority("vault", 3));
    assert!(!effect.matches_vault_authority("vault", 47));
    assert!(!effect.matches_vault_authority("other-vault", 3));
    assert_eq!(effect.effective_valid_until_ms(), 1500);
    effect.valid_until_ms = 32000;
    assert_eq!(effect.effective_valid_until_ms(), 1500);
    effect.mode = FocusMode::ManualPause;
    assert_eq!(effect.effective_valid_until_ms(), 2000);
    effect.valid_until_ms = 1200;
    assert_eq!(effect.effective_valid_until_ms(), 1200);
    effect.valid_until_ms = 0;
    assert_eq!(effect.effective_valid_until_ms(), 0);
    effect.mode = FocusMode::Running;
    effect.valid_until_ms = 16000;
    effect.phase_deadline_ms = None;
    assert_eq!(effect.effective_valid_until_ms(), 0);
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

async fn pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    ganbaru_db::run_migrations(&pool).await.unwrap();
    sqlx::query("INSERT INTO calendar_events (id, title, start_time, end_time) VALUES ('focus-event', 'Focus', ?, ?)")
        .bind(persistence::format_timestamp(START).unwrap()).bind(persistence::format_timestamp(START + 20 * MINUTE).unwrap())
        .execute(&pool).await.unwrap();
    pool
}

fn context(now_ms: i64) -> FocusExecutionContext {
    FocusExecutionContext {
        now_ms,
        platform: FocusPlatform::Desktop,
        foreground: true,
        local_time: None,
        planned_blocks: Vec::new(),
        commitment: Some(FocusCommitment {
            event_id: "focus-event".to_owned(),
            occurrence_id: "focus-event".to_owned(),
            event_date: "2026-05-22".to_owned(),
            title: Some("Focus".to_owned()),
            start_ms: START,
            end_ms: START + 20 * MINUTE,
            calendar_revision: "calendar-1".to_owned(),
            configuration: FocusConfiguration {
                rhythm: PomodoroRunRhythm::Count {
                    focus_duration_minutes: 2,
                    short_break_minutes: 1,
                    long_break_minutes: 2,
                    long_break_after_focus_count: 2,
                },
                rhythm_source: "custom".to_owned(),
                preset_key: None,
                idle_timeout_minutes: Some(1),
            },
        }),
    }
}

fn command(id: &str, expected_revision: i64, intent: FocusIntent) -> FocusCommand {
    FocusCommand {
        command_id: id.to_owned(),
        expected_revision,
        intent,
    }
}

async fn execute(
    pool: &SqlitePool,
    command: &FocusCommand,
    context: &FocusExecutionContext,
) -> Result<FocusExecutionSnapshot, FocusExecutionError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    match focus_execute_command_tx(&mut tx, command, context).await {
        Ok(result) => {
            tx.commit().await.unwrap();
            Ok(result)
        }
        Err(error) => {
            tx.rollback().await.unwrap();
            Err(error)
        }
    }
}

async fn observe(
    pool: &SqlitePool,
    observation: FocusObservation,
    context: &FocusExecutionContext,
) -> FocusExecutionSnapshot {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let result = focus_apply_observation_tx(&mut tx, &observation, context)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    result
}

async fn start(pool: &SqlitePool) -> FocusExecutionSnapshot {
    execute(
        pool,
        &command(
            "start",
            0,
            FocusIntent::StartScheduled {
                occurrence_id: None,
            },
        ),
        &context(START),
    )
    .await
    .unwrap()
}

#[test]
fn canonical_execution_rejects_missing_accepted_durations() {
    block_on(async {
        for query in [
            "UPDATE pomodoro_segments SET chosen_duration_ms = NULL",
            "UPDATE pomodoro_runs SET inherited_focus_milliseconds = NULL",
            "UPDATE pomodoro_runs SET inherited_phase_milliseconds = NULL",
        ] {
            let pool = pool().await;
            let initial = start(&pool).await;
            assert!(initial.segment.unwrap().chosen_duration_ms > 0);
            sqlx::query(query).execute(&pool).await.unwrap();

            let error = focus_read_execution_snapshot(&pool, START)
                .await
                .unwrap_err();
            assert_eq!(error.code, FocusErrorCode::InvalidState);
            let revision: i64 = sqlx::query_scalar("SELECT revision FROM pomodoro_execution_state")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(revision, initial.revision);
        }
    });
}

#[test]
fn receipt_recovery_does_not_require_current_calendar_or_clock_inputs() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let retry = command(
            "start",
            0,
            FocusIntent::StartScheduled {
                occurrence_id: None,
            },
        );
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert_eq!(
            focus_read_command_receipt_tx(&mut tx, &retry)
                .await
                .unwrap(),
            Some(initial.clone())
        );
        let unavailable = FocusExecutionContext {
            now_ms: i64::MAX,
            commitment: None,
            ..context(START)
        };
        assert_eq!(
            focus_execute_command_tx(&mut tx, &retry, &unavailable)
                .await
                .unwrap(),
            initial
        );
        let conflict = command("start", 0, FocusIntent::Stop);
        assert_eq!(
            focus_read_command_receipt_tx(&mut tx, &conflict)
                .await
                .unwrap_err()
                .code,
            FocusErrorCode::CommandIdentityConflict
        );
        assert!(
            focus_read_command_receipt_tx(&mut tx, &command("new", 1, FocusIntent::Pause))
                .await
                .unwrap()
                .is_none()
        );
        tx.commit().await.unwrap();
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM pomodoro_execution_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(revision, initial.revision);
    });
}

#[test]
fn execution_receipt_replays_original_result_after_later_transitions() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let paused = execute(
            &pool,
            &command("pause", 1, FocusIntent::Pause),
            &context(START + 20_000),
        )
        .await
        .unwrap();
        assert_eq!(paused.mode, FocusMode::ManualPause);
        let retry = execute(
            &pool,
            &command(
                "start",
                0,
                FocusIntent::StartScheduled {
                    occurrence_id: None,
                },
            ),
            &context(START + 30_000),
        )
        .await
        .unwrap();
        assert_eq!(retry, initial);
        let reused = execute(
            &pool,
            &command("start", 2, FocusIntent::Stop),
            &context(START + 30_000),
        )
        .await
        .unwrap_err();
        assert_eq!(reused.code, FocusErrorCode::CommandIdentityConflict);
        let stale = execute(
            &pool,
            &command("stale", 1, FocusIntent::Stop),
            &context(START + 30_000),
        )
        .await
        .unwrap_err();
        assert_eq!(stale.current_revision, Some(2));
        assert_eq!(
            focus_read_execution_snapshot(&pool, START + 30_000)
                .await
                .unwrap()
                .mode,
            FocusMode::ManualPause
        );
    });
}

#[test]
fn accepted_receipt_and_current_projection_remain_distinct_after_recovery() {
    block_on(async {
        let pool = pool().await;
        let accepted = start(&pool).await;
        let recovered = observe(&pool, FocusObservation::Recover, &context(START + MINUTE)).await;
        assert_eq!(recovered.mode, FocusMode::Stopped);
        let retry = command(
            "start",
            0,
            FocusIntent::StartScheduled {
                occurrence_id: None,
            },
        );
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let receipt = focus_read_command_receipt_tx(&mut tx, &retry)
            .await
            .unwrap()
            .unwrap();
        let current = focus_read_execution_snapshot_tx(&mut tx, START + MINUTE)
            .await
            .unwrap();
        assert_eq!(receipt, accepted);
        assert_eq!(current.mode, FocusMode::Stopped);
        assert_eq!(current.revision, recovered.revision);
        assert!(current.revision > receipt.revision);
        tx.commit().await.unwrap();
        assert_eq!(
            focus_read_execution_snapshot(&pool, START + MINUTE)
                .await
                .unwrap(),
            current
        );
    });
}

#[test]
fn committed_receipt_survives_a_corrupt_current_projection() {
    block_on(async {
        let pool = pool().await;
        let accepted = start(&pool).await;
        sqlx::query("UPDATE pomodoro_execution_state SET state_json = '{\"mode\":42}'")
            .execute(&pool)
            .await
            .unwrap();
        let retry = command(
            "start",
            0,
            FocusIntent::StartScheduled {
                occurrence_id: None,
            },
        );
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let receipt = focus_read_command_receipt_tx(&mut tx, &retry)
            .await
            .unwrap()
            .unwrap();
        assert!(
            focus_read_execution_snapshot_tx(&mut tx, START + MINUTE)
                .await
                .is_err()
        );
        tx.commit().await.unwrap();
        assert_eq!(receipt, accepted);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM pomodoro_execution_receipts")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    });
}

#[test]
fn execution_failure_rolls_back_outgoing_phase_events_revision_and_receipt() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        sqlx::raw_sql("CREATE TRIGGER reject_incoming BEFORE INSERT ON pomodoro_segments BEGIN SELECT RAISE(ABORT, 'injected incoming phase failure'); END;")
            .execute(&pool).await.unwrap();
        let error = execute(
            &pool,
            &command("advance", 1, FocusIntent::Advance),
            &context(START + 60_000),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, FocusErrorCode::Persistence);
        let current = focus_read_execution_snapshot(&pool, START + 60_000)
            .await
            .unwrap();
        assert_eq!(current.revision, initial.revision);
        assert_eq!(current.segment.unwrap().status, "active");
        let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_run_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(event_count, 2);
        let receipt_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_execution_receipts")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(receipt_count, 1);
    });
}

#[test]
fn execution_pause_and_resume_preserve_net_progress_and_hard_event_deadline() {
    block_on(async {
        let pool = pool().await;
        start(&pool).await;
        let paused = execute(
            &pool,
            &command("pause", 1, FocusIntent::Pause),
            &context(START + 30_000),
        )
        .await
        .unwrap();
        assert_eq!(paused.elapsed_ms, 30_000);
        let resumed = execute(
            &pool,
            &command("resume", 2, FocusIntent::Resume),
            &context(START + 90_000),
        )
        .await
        .unwrap();
        assert_eq!(resumed.remaining_ms, 90_000);
        assert_eq!(resumed.phase_deadline_ms, Some(START + 180_000));
        assert_eq!(resumed.run.unwrap().planned_end_ms, START + 20 * MINUTE);
        assert_eq!(
            resumed.segment.unwrap().pauses[0].ended_at_ms,
            Some(START + 90_000)
        );
        execute(
            &pool,
            &command("pause-again", 3, FocusIntent::Pause),
            &context(START + 100_000),
        )
        .await
        .unwrap();
        let expired = execute(
            &pool,
            &command("late-resume", 4, FocusIntent::Resume),
            &context(START + 21 * MINUTE),
        )
        .await
        .unwrap();
        assert_eq!(expired.mode, FocusMode::Expired);
        assert_eq!(expired.run.unwrap().ended_at_ms, Some(START + 20 * MINUTE));
        assert_eq!(
            expired.segment.unwrap().actual_end_ms,
            Some(START + 20 * MINUTE)
        );
    });
}

#[test]
fn execution_break_completion_waits_for_acceptance_and_exact_event_end_wins() {
    block_on(async {
        let pool = pool().await;
        start(&pool).await;
        let break_started = observe(
            &pool,
            FocusObservation::Deadline,
            &context(START + 2 * MINUTE),
        )
        .await;
        assert_eq!(break_started.segment.unwrap().phase, FocusPhase::ShortBreak);
        let waiting = observe(
            &pool,
            FocusObservation::Deadline,
            &context(START + 3 * MINUTE),
        )
        .await;
        assert_eq!(waiting.mode, FocusMode::ReturnWait);
        let repeated = observe(
            &pool,
            FocusObservation::Deadline,
            &context(START + 4 * MINUTE),
        )
        .await;
        assert_eq!(repeated.revision, waiting.revision);
        let accepted = execute(
            &pool,
            &command("return", waiting.revision, FocusIntent::Advance),
            &context(START + 4 * MINUTE),
        )
        .await
        .unwrap();
        assert_eq!(accepted.segment.unwrap().rhythm_position, 2);
        let expired = observe(
            &pool,
            FocusObservation::Deadline,
            &context(START + 20 * MINUTE),
        )
        .await;
        assert_eq!(expired.mode, FocusMode::Expired);
        let active_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_segments WHERE status = 'active'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(active_count, 0);
        let phase_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_segments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(phase_count, 3);
    });
}

#[test]
fn execution_idle_failure_is_backdated_and_missing_activity_cannot_pause_or_admit() {
    block_on(async {
        let pool = pool().await;
        let none = observe(
            &pool,
            FocusObservation::AutomaticAdmission(FocusActivityObservation {
                observed_at_ms: START,
                idle_ms: None,
                webcam_in_use: false,
            }),
            &context(START),
        )
        .await;
        assert_eq!(none.revision, 0);
        start(&pool).await;
        let unavailable = observe(
            &pool,
            FocusObservation::Activity(FocusActivityObservation {
                observed_at_ms: START + 10_000,
                idle_ms: None,
                webcam_in_use: false,
            }),
            &context(START + 10_000),
        )
        .await;
        assert!(unavailable.activity_source_unavailable);
        assert_eq!(unavailable.mode, FocusMode::Running);
        let idle = observe(
            &pool,
            FocusObservation::Activity(FocusActivityObservation {
                observed_at_ms: START + 80_000,
                idle_ms: Some(60_000),
                webcam_in_use: false,
            }),
            &context(START + 80_000),
        )
        .await;
        assert_eq!(idle.mode, FocusMode::IdlePause);
        assert_eq!(idle.elapsed_ms, 20_000);
        let unseen = observe(&pool, FocusObservation::Deadline, &context(START + 140_000)).await;
        assert_eq!(unseen.mode, FocusMode::IdlePause);
        assert_eq!(unseen.idle_overlay_visible_at_ms, None);
        let acknowledgement = FocusObservation::IdleOverlayVisible {
            run_id: idle.run.as_ref().unwrap().id.clone(),
            segment_id: idle.segment.as_ref().unwrap().id.clone(),
            detected_at_ms: idle.idle_detected_at_ms.unwrap(),
        };
        let wrong_episode = observe(
            &pool,
            FocusObservation::IdleOverlayVisible {
                run_id: idle.run.as_ref().unwrap().id.clone(),
                segment_id: idle.segment.as_ref().unwrap().id.clone(),
                detected_at_ms: START + 79_999,
            },
            &context(START + 144_000),
        )
        .await;
        assert_eq!(wrong_episode.idle_overlay_visible_at_ms, None);
        for (run_id, segment_id) in [
            (
                "previous-run".to_owned(),
                idle.segment.as_ref().unwrap().id.clone(),
            ),
            (
                idle.run.as_ref().unwrap().id.clone(),
                "previous-segment".to_owned(),
            ),
        ] {
            let stale = observe(
                &pool,
                FocusObservation::IdleOverlayVisible {
                    run_id,
                    segment_id,
                    detected_at_ms: idle.idle_detected_at_ms.unwrap(),
                },
                &context(START + 144_000),
            )
            .await;
            assert_eq!(stale.revision, idle.revision);
            assert_eq!(stale.idle_overlay_visible_at_ms, None);
        }
        let visible = observe(&pool, acknowledgement.clone(), &context(START + 145_000)).await;
        assert_eq!(visible.idle_overlay_visible_at_ms, Some(START + 145_000));
        let retried = observe(&pool, acknowledgement, &context(START + 175_000)).await;
        assert_eq!(retried.revision, visible.revision);
        assert_eq!(retried.idle_overlay_visible_at_ms, Some(START + 145_000));
        let wall_jump = observe(&pool, FocusObservation::Deadline, &context(START + 300_000)).await;
        assert_eq!(wall_jump.mode, FocusMode::IdlePause);
        let grace = |elapsed_ms| FocusObservation::IdleGraceElapsed {
            run_id: idle.run.as_ref().unwrap().id.clone(),
            segment_id: idle.segment.as_ref().unwrap().id.clone(),
            visible_at_ms: START + 145_000,
            elapsed_ms,
        };
        let before = observe(
            &pool,
            grace(FOCUS_IDLE_FAILURE_GRACE_MS - 1),
            &context(START + 300_000),
        )
        .await;
        assert_eq!(before.mode, FocusMode::IdlePause);
        let stale_grace = observe(
            &pool,
            FocusObservation::IdleGraceElapsed {
                run_id: idle.run.as_ref().unwrap().id.clone(),
                segment_id: idle.segment.as_ref().unwrap().id.clone(),
                visible_at_ms: START + 144_999,
                elapsed_ms: FOCUS_IDLE_FAILURE_GRACE_MS,
            },
            &context(START + 300_000),
        )
        .await;
        assert_eq!(stale_grace.mode, FocusMode::IdlePause);
        // A civil clock correction cannot extend a fully elapsed native grace period.
        let failed = observe(
            &pool,
            grace(FOCUS_IDLE_FAILURE_GRACE_MS),
            &context(START + 165_000),
        )
        .await;
        assert_eq!(failed.mode, FocusMode::IdleFailed);
        assert_eq!(failed.segment.unwrap().actual_end_ms, Some(START + 20_000));
        let resumed = execute(
            &pool,
            &command(
                "idle-return",
                failed.revision,
                FocusIntent::ResolveIdle { resume: true },
            ),
            &context(START + 215_000),
        )
        .await
        .unwrap();
        assert_eq!(resumed.mode, FocusMode::Running);
        assert_eq!(resumed.segment.unwrap().actual_start_ms, START + 215_000);
        assert_eq!(resumed.remaining_ms, 2 * MINUTE);
    });
}

#[test]
fn execution_suspend_does_not_overlap_an_existing_idle_pause() {
    block_on(async {
        let pool = pool().await;
        start(&pool).await;
        let suspended = observe(
            &pool,
            FocusObservation::Suspend {
                started_at_ms: START + 15_000,
                returned_at_ms: START + 200_000,
            },
            &context(START + 200_000),
        )
        .await;
        assert_eq!(suspended.mode, FocusMode::Suspended);
        assert_eq!(suspended.elapsed_ms, 15_000);
        let idle = observe(
            &pool,
            FocusObservation::Activity(FocusActivityObservation {
                observed_at_ms: START + 200_000,
                idle_ms: Some(190_000),
                webcam_in_use: false,
            }),
            &context(START + 200_000),
        )
        .await;
        assert_eq!(idle.segment.unwrap().pauses.len(), 1);
        let resumed = execute(
            &pool,
            &command(
                "suspend-return",
                suspended.revision,
                FocusIntent::ResolveSuspend { resume: true },
            ),
            &context(START + 210_000),
        )
        .await
        .unwrap();
        assert_eq!(resumed.remaining_ms, 105_000);
        assert_eq!(resumed.run.unwrap().planned_end_ms, START + 20 * MINUTE);
    });
}

#[test]
fn execution_idle_visibility_failure_rolls_back_and_retry_keeps_first_accepted_time() {
    block_on(async {
        let pool = pool().await;
        start(&pool).await;
        let idle = observe(
            &pool,
            FocusObservation::Activity(FocusActivityObservation {
                observed_at_ms: START + 80_000,
                idle_ms: Some(60_000),
                webcam_in_use: false,
            }),
            &context(START + 80_000),
        )
        .await;
        let acknowledgement = FocusObservation::IdleOverlayVisible {
            run_id: idle.run.as_ref().unwrap().id.clone(),
            segment_id: idle.segment.as_ref().unwrap().id.clone(),
            detected_at_ms: idle.idle_detected_at_ms.unwrap(),
        };
        sqlx::raw_sql("CREATE TRIGGER reject_visibility BEFORE UPDATE OF state_json ON pomodoro_execution_state BEGIN SELECT RAISE(ABORT, 'injected visibility failure'); END;")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert!(
            focus_apply_observation_tx(&mut tx, &acknowledgement, &context(START + 90_000))
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
        let unchanged = focus_read_execution_snapshot(&pool, START + 90_000)
            .await
            .unwrap();
        assert_eq!(unchanged.revision, idle.revision);
        assert_eq!(unchanged.idle_overlay_visible_at_ms, None);
        sqlx::raw_sql("DROP TRIGGER reject_visibility")
            .execute(&pool)
            .await
            .unwrap();
        let accepted = observe(&pool, acknowledgement.clone(), &context(START + 100_000)).await;
        let restored = focus_read_execution_snapshot(&pool, START + 110_000)
            .await
            .unwrap();
        assert_eq!(restored.idle_overlay_visible_at_ms, Some(START + 100_000));
        let retried = observe(&pool, acknowledgement, &context(START + 120_000)).await;
        assert_eq!(retried.revision, accepted.revision);
        assert_eq!(retried.idle_overlay_visible_at_ms, Some(START + 100_000));
    });
}

#[test]
fn recovery_does_not_adopt_runs_without_native_execution_state() {
    block_on(async {
        let pool = pool().await;
        start(&pool).await;
        sqlx::query("UPDATE pomodoro_execution_state SET state_json = ?")
            .bind(sqlx::types::Json(ExecutionState::default()))
            .execute(&pool)
            .await
            .unwrap();
        let recovered = observe(&pool, FocusObservation::Recover, &context(START + MINUTE)).await;
        assert!(recovered.run.is_none());
        assert!(recovered.segment.is_none());
    });
}

#[test]
fn execution_recovery_preserves_idle_pause_without_reusing_a_previous_controllers_visibility() {
    block_on(async {
        let pool = pool().await;
        start(&pool).await;
        let idle = observe(
            &pool,
            FocusObservation::Activity(FocusActivityObservation {
                observed_at_ms: START + 80_000,
                idle_ms: Some(60_000),
                webcam_in_use: false,
            }),
            &context(START + 80_000),
        )
        .await;
        let visible = observe(
            &pool,
            FocusObservation::IdleOverlayVisible {
                run_id: idle.run.as_ref().unwrap().id.clone(),
                segment_id: idle.segment.as_ref().unwrap().id.clone(),
                detected_at_ms: idle.idle_detected_at_ms.unwrap(),
            },
            &context(START + 90_000),
        )
        .await;
        assert_eq!(visible.idle_overlay_visible_at_ms, Some(START + 90_000));
        let mut recovery = context(START + 200_000);
        recovery.platform = FocusPlatform::Android;
        let restored = observe(&pool, FocusObservation::Recover, &recovery).await;
        assert_eq!(restored.mode, FocusMode::IdlePause);
        assert_eq!(restored.idle_overlay_visible_at_ms, None);
        assert_eq!(restored.elapsed_ms, idle.elapsed_ms);
        assert_eq!(
            restored.segment.as_ref().unwrap().pauses,
            idle.segment.as_ref().unwrap().pauses
        );
        let stale = observe(
            &pool,
            FocusObservation::IdleGraceElapsed {
                run_id: idle.run.unwrap().id,
                segment_id: idle.segment.unwrap().id,
                visible_at_ms: START + 90_000,
                elapsed_ms: FOCUS_IDLE_FAILURE_GRACE_MS,
            },
            &recovery,
        )
        .await;
        assert_eq!(stale.mode, FocusMode::IdlePause);
        assert_eq!(stale.revision, restored.revision);
    });
}

#[test]
fn execution_android_background_and_recovery_never_create_future_phases() {
    block_on(async {
        let pool = pool().await;
        start(&pool).await;
        let mut background = context(START + 3 * MINUTE);
        background.platform = FocusPlatform::Android;
        background.foreground = false;
        let recovered = observe(&pool, FocusObservation::Recover, &background).await;
        assert_eq!(recovered.mode, FocusMode::ReturnWait);
        let repeated = observe(&pool, FocusObservation::Deadline, &background).await;
        assert_eq!(repeated.revision, recovered.revision);
        let phase_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_segments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(phase_count, 1);
        let mut foreground = context(START + 4 * MINUTE);
        foreground.platform = FocusPlatform::Android;
        let opened = observe(
            &pool,
            FocusObservation::ForegroundChanged { foreground: true },
            &foreground,
        )
        .await;
        assert_eq!(opened.mode, FocusMode::ReturnWait);
        let accepted = execute(
            &pool,
            &command("accept-break", opened.revision, FocusIntent::Advance),
            &foreground,
        )
        .await
        .unwrap();
        assert_eq!(accepted.segment.unwrap().phase, FocusPhase::ShortBreak);
    });
}

#[test]
fn execution_reconfiguration_atomically_carries_subminute_progress() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let mut edited = context(START + 30_125);
        edited.commitment.as_mut().unwrap().configuration.rhythm = PomodoroRunRhythm::Count {
            focus_duration_minutes: 3,
            short_break_minutes: 1,
            long_break_minutes: 2,
            long_break_after_focus_count: 2,
        };
        let result = observe(&pool, FocusObservation::CalendarChanged, &edited).await;
        let run = result.run.unwrap();
        assert_ne!(run.id, initial.run.unwrap().id);
        assert_eq!(run.inherited_focus_ms, 30_125);
        assert_eq!(run.inherited_phase_ms, 30_125);
        assert_eq!(result.remaining_ms, 3 * MINUTE - 30_125);
        let active_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_runs WHERE ended_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(active_count, 1);
        assert_eq!(result.changed_segments.len(), 2);
    });
}

#[test]
fn execution_competing_starts_accept_only_one_expected_revision() {
    block_on(async {
        let pool = pool().await;
        let first = command(
            "first",
            0,
            FocusIntent::StartScheduled {
                occurrence_id: None,
            },
        );
        let second = command(
            "second",
            0,
            FocusIntent::StartScheduled {
                occurrence_id: None,
            },
        );
        let context = context(START);
        let (one, two) = tokio::join!(
            execute(&pool, &first, &context),
            execute(&pool, &second, &context)
        );
        assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
        let error = if let Err(error) = one {
            error
        } else {
            two.unwrap_err()
        };
        assert_eq!(error.code, FocusErrorCode::StaleRevision);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_runs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    });
}
