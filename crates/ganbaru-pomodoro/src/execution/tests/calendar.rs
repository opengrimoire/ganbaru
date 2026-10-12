use super::*;

#[test]
fn calendar_end_now_closes_a_paused_run_at_the_native_clock_and_preserves_provenance() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let paused = execute(
            &pool,
            &command("pause", initial.revision, FocusIntent::Pause),
            &context(START + 30_125),
        )
        .await
        .unwrap();
        let mut target = target(&pool, START + MINUTE).await;
        // Completion remains completion even if this Calendar action also
        // removes Focus configuration from its resulting row.
        target.commitment = None;
        let completion = FocusCalendarCompletion {
            reference: change(&paused),
            start_ms: START,
            end_ms: target.now_ms,
        };
        let mut tx = pool.begin().await.unwrap();
        let result = focus_complete_calendar_tx(&mut tx, paused.revision, &completion, &target)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(result.mode, FocusMode::Expired);
        let run = result.run.as_ref().unwrap();
        assert_eq!(run.id, initial.run.as_ref().unwrap().id);
        assert_eq!(run.planned_end_ms, target.now_ms);
        assert_eq!(run.ended_at_ms, Some(target.now_ms));
        assert_eq!(run.occurrence_id, "edited::2026-05-23");
        let segment = result.segment.as_ref().unwrap();
        assert_eq!(segment.actual_end_ms, Some(target.now_ms));
        assert_eq!(segment.pauses[0].started_at_ms, START + 30_125);
        assert_eq!(segment.pauses[0].ended_at_ms, Some(target.now_ms));
        let original: (String,String,Option<String>) = sqlx::query_as("SELECT original_event_id, event_date, event_title_snapshot FROM pomodoro_runs WHERE id = ?")
            .bind(&run.id).fetch_one(&pool).await.unwrap();
        assert_eq!(
            original,
            (
                "focus-event".into(),
                "2026-05-22".into(),
                Some("Focus".into())
            )
        );
        let reread = focus_read_execution_snapshot(&pool, target.now_ms + MINUTE)
            .await
            .unwrap();
        assert_eq!(reread.run, result.run);
        assert_eq!(reread.segment, result.segment);
        let segment_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_segments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(segment_count, 1);
    });
}

#[test]
fn calendar_end_now_rejects_changed_geometry_or_clock_before_committed_progress() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let paused = execute(
            &pool,
            &command("pause", initial.revision, FocusIntent::Pause),
            &context(START + MINUTE),
        )
        .await
        .unwrap();
        let mut target = target(&pool, START + 2 * MINUTE).await;
        for (start_ms, end_ms, now_ms) in [
            (START + 1, START + 2 * MINUTE, START + 2 * MINUTE),
            (START, START + MINUTE, START + 2 * MINUTE),
            (START, START + 1000, START + 1000),
        ] {
            target.now_ms = now_ms;
            let completion = FocusCalendarCompletion {
                reference: change(&paused),
                start_ms,
                end_ms,
            };
            let mut tx = pool.begin().await.unwrap();
            let error = focus_complete_calendar_tx(&mut tx, paused.revision, &completion, &target)
                .await
                .unwrap_err();
            assert_eq!(error.code, FocusErrorCode::IneligibleCommitment);
            tx.rollback().await.unwrap();
        }
        let result = focus_read_execution_snapshot(&pool, START + 2 * MINUTE)
            .await
            .unwrap();
        assert_eq!(result.revision, paused.revision);
        assert_eq!(result.run.as_ref().unwrap().occurrence_id, "focus-event");
        assert!(result.run.unwrap().ended_at_ms.is_none());
    });
}

#[test]
fn calendar_end_now_rolls_back_retarget_and_closure_when_completion_evidence_fails() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let target = target(&pool, START + MINUTE).await;
        sqlx::raw_sql("CREATE TRIGGER reject_calendar_completion BEFORE INSERT ON pomodoro_run_events WHEN NEW.event_type = 'complete' BEGIN SELECT RAISE(ABORT, 'completion evidence failed'); END;")
            .execute(&pool).await.unwrap();
        let completion = FocusCalendarCompletion {
            reference: change(&initial),
            start_ms: START,
            end_ms: target.now_ms,
        };
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("UPDATE calendar_events SET end_time = ? WHERE id = 'edited'")
            .bind(persistence::format_timestamp(target.now_ms).unwrap())
            .execute(&mut *tx)
            .await
            .unwrap();
        let result =
            focus_complete_calendar_tx(&mut tx, initial.revision, &completion, &target).await;
        assert!(
            result
                .unwrap_err()
                .message
                .contains("completion evidence failed")
        );
        tx.rollback().await.unwrap();
        let result = focus_read_execution_snapshot(&pool, target.now_ms)
            .await
            .unwrap();
        assert_eq!(result.revision, initial.revision);
        assert_eq!(result.run, initial.run);
        assert_eq!(result.segment, initial.segment);
        let calendar_end: String =
            sqlx::query_scalar("SELECT end_time FROM calendar_events WHERE id = 'edited'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            calendar_end,
            persistence::format_timestamp(START + 20 * MINUTE).unwrap()
        );
    });
}

#[test]
fn calendar_retarget_keeps_home_zone_identity_separate_from_the_device_day_plan() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let mut target = target(&pool, START + 1000).await;
        target.commitment.as_mut().unwrap().event_date = "2026-05-22".into();
        let mut change = change(&initial);
        change.target_event_date = "2026-05-22".into();
        let mut tx = pool.begin().await.unwrap();
        let result = focus_retarget_calendar_tx(&mut tx, initial.revision, &change, &target)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let run = result.run.unwrap();
        assert_eq!(run.event_date, "2026-05-22");
        assert_eq!(run.occurrence_id, "edited::2026-05-23");
    });
}

#[test]
fn calendar_retarget_preserves_an_open_manual_pause() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let paused = execute(
            &pool,
            &command("pause", initial.revision, FocusIntent::Pause),
            &context(START + 30_125),
        )
        .await
        .unwrap();
        let target = target(&pool, START + MINUTE).await;
        let mut tx = pool.begin().await.unwrap();
        let result =
            focus_retarget_calendar_tx(&mut tx, paused.revision, &change(&paused), &target)
                .await
                .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(result.mode, FocusMode::ManualPause);
        assert_eq!(result.remaining_ms, paused.remaining_ms);
        assert_eq!(
            result.segment.as_ref().unwrap().pauses,
            paused.segment.as_ref().unwrap().pauses
        );
        assert_eq!(
            result.run.as_ref().unwrap().occurrence_id,
            "edited::2026-05-23"
        );
    });
}

#[test]
fn removing_focus_configuration_closes_only_committed_execution_during_retarget() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let mut target = target(&pool, START + 30_125).await;
        target.commitment = None;
        let mut tx = pool.begin().await.unwrap();
        let result =
            focus_retarget_calendar_tx(&mut tx, initial.revision, &change(&initial), &target)
                .await
                .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(result.mode, FocusMode::Stopped);
        assert_eq!(
            result.run.as_ref().unwrap().ended_at_ms,
            Some(target.now_ms)
        );
        let original: String =
            sqlx::query_scalar("SELECT original_event_id FROM pomodoro_runs WHERE id = ?")
                .bind(&initial.run.unwrap().id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(original, "focus-event");
        let segment_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_segments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(segment_count, 1);
    });
}

async fn target(pool: &SqlitePool, now: i64) -> FocusExecutionContext {
    sqlx::query("INSERT INTO calendar_events (id, title, start_time, end_time, timezone, rrule) VALUES ('edited', 'Edited', ?, ?, 'Pacific/Kiritimati', 'FREQ=DAILY;COUNT=3')")
        .bind(persistence::format_timestamp(START).unwrap()).bind(persistence::format_timestamp(START + 20 * MINUTE).unwrap()).execute(pool).await.unwrap();
    let mut target = context(now);
    let commitment = target.commitment.as_mut().unwrap();
    commitment.event_id = "edited".into();
    commitment.occurrence_id = "edited::2026-05-23".into();
    commitment.event_date = "2026-05-23".into();
    commitment.title = Some("Edited".into());
    target
}

fn change(snapshot: &FocusExecutionSnapshot) -> FocusCalendarReferenceChange {
    let run = snapshot.run.as_ref().unwrap();
    FocusCalendarReferenceChange {
        run_id: run.id.clone(),
        expected_occurrence_id: run.occurrence_id.clone(),
        target_event_id: "edited".into(),
        target_occurrence_id: "edited::2026-05-23".into(),
        target_event_date: "2026-05-23".into(),
    }
}

#[test]
fn calendar_retarget_preserves_original_provenance_completed_phases_and_restart_identity() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let advanced = execute(
            &pool,
            &command("advance", initial.revision, FocusIntent::Advance),
            &context(START + MINUTE),
        )
        .await
        .unwrap();
        let old_segment = initial.segment.as_ref().unwrap();
        let before: (Option<String>,String,String,Option<String>,String) = sqlx::query_as("SELECT event_id, event_date, actual_start, actual_end, status FROM pomodoro_segments WHERE id = ?")
            .bind(&old_segment.id).fetch_one(&pool).await.unwrap();
        let target = target(&pool, START + MINUTE + 1000).await;
        let mut tx = pool.begin().await.unwrap();
        let result =
            focus_retarget_calendar_tx(&mut tx, advanced.revision, &change(&advanced), &target)
                .await
                .unwrap();
        tx.commit().await.unwrap();
        let run = result.run.as_ref().unwrap();
        assert_eq!(run.id, advanced.run.as_ref().unwrap().id);
        assert_eq!(run.occurrence_id, "edited::2026-05-23");
        assert_eq!(run.event_date, "2026-05-23");
        assert_eq!(run.title.as_deref(), Some("Edited"));
        assert_eq!(
            result.segment.as_ref().unwrap().id,
            advanced.segment.as_ref().unwrap().id
        );
        assert_eq!(
            result.segment.as_ref().unwrap().event_id.as_deref(),
            Some("edited")
        );
        assert_eq!(result.changed_segments.len(), 1);
        assert_eq!(result.revision, advanced.revision + 1);
        let original: (String,String,Option<String>) = sqlx::query_as("SELECT original_event_id, event_date, event_title_snapshot FROM pomodoro_runs WHERE id = ?")
            .bind(&run.id).fetch_one(&pool).await.unwrap();
        assert_eq!(
            original,
            (
                "focus-event".into(),
                "2026-05-22".into(),
                Some("Focus".into())
            )
        );
        let after: (Option<String>,String,String,Option<String>,String) = sqlx::query_as("SELECT event_id, event_date, actual_start, actual_end, status FROM pomodoro_segments WHERE id = ?")
            .bind(&old_segment.id).fetch_one(&pool).await.unwrap();
        assert_eq!(after, before);
        let reread = focus_read_execution_snapshot(&pool, target.now_ms)
            .await
            .unwrap();
        assert_eq!(reread.run, result.run);
    });
}

#[test]
fn calendar_retarget_rejects_stale_revision_wrong_run_and_changed_recorded_start() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let target = target(&pool, START + 1000).await;
        for case in 0..5 {
            let mut change = change(&initial);
            let mut context = context(target.now_ms);
            context.commitment.clone_from(&target.commitment);
            let revision = if case == 0 {
                initial.revision - 1
            } else {
                initial.revision
            };
            if case == 1 {
                change.run_id = "another-run".into();
            }
            if case == 2 {
                change.expected_occurrence_id = "another-occurrence".into();
            }
            if case == 3 {
                context.commitment.as_mut().unwrap().start_ms += 1;
            }
            if case == 4 {
                context.commitment.as_mut().unwrap().end_ms = context.now_ms - 1;
            }
            let mut tx = pool.begin().await.unwrap();
            assert!(
                focus_retarget_calendar_tx(&mut tx, revision, &change, &context)
                    .await
                    .is_err()
            );
            tx.rollback().await.unwrap();
        }
        let reread = focus_read_execution_snapshot(&pool, target.now_ms)
            .await
            .unwrap();
        assert_eq!(reread.run, initial.run);
        assert_eq!(reread.revision, initial.revision);
    });
}

#[test]
fn calendar_retarget_rolls_back_reference_and_revision_when_phase_write_fails() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let target = target(&pool, START + 1000).await;
        sqlx::query("CREATE TRIGGER fail_phase BEFORE UPDATE OF event_id ON pomodoro_segments BEGIN SELECT RAISE(ABORT, 'injected phase failure'); END").execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let error =
            focus_retarget_calendar_tx(&mut tx, initial.revision, &change(&initial), &target)
                .await
                .unwrap_err();
        assert!(
            error.message.contains("injected phase failure"),
            "{error:?}"
        );
        tx.rollback().await.unwrap();
        let result = focus_read_execution_snapshot(&pool, target.now_ms)
            .await
            .unwrap();
        assert_eq!(result.revision, initial.revision);
        assert_eq!(result.run, initial.run);
    });
}

#[test]
fn calendar_retarget_and_rhythm_change_carry_subminute_progress_atomically() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let mut target = target(&pool, START + 30_125).await;
        target.commitment.as_mut().unwrap().configuration.rhythm = PomodoroRunRhythm::Count {
            focus_duration_minutes: 3,
            short_break_minutes: 1,
            long_break_minutes: 2,
            long_break_after_focus_count: 2,
        };
        let mut tx = pool.begin().await.unwrap();
        let result =
            focus_retarget_calendar_tx(&mut tx, initial.revision, &change(&initial), &target)
                .await
                .unwrap();
        tx.commit().await.unwrap();
        let run = result.run.unwrap();
        assert_ne!(run.id, initial.run.as_ref().unwrap().id);
        assert_eq!(run.occurrence_id, "edited::2026-05-23");
        assert_eq!(run.inherited_focus_ms, 30_125);
        assert_eq!(result.remaining_ms, 3 * MINUTE - 30_125);
        let original: String =
            sqlx::query_scalar("SELECT original_event_id FROM pomodoro_runs WHERE id = ?")
                .bind(&initial.run.unwrap().id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(original, "focus-event");
    });
}

#[test]
fn calendar_retarget_cannot_extend_already_expired_execution() {
    block_on(async {
        let pool = pool().await;
        let initial = start(&pool).await;
        let mut target = target(&pool, START + 20 * MINUTE).await;
        target.commitment.as_mut().unwrap().end_ms += MINUTE;
        let mut tx = pool.begin().await.unwrap();
        let error =
            focus_retarget_calendar_tx(&mut tx, initial.revision, &change(&initial), &target)
                .await
                .unwrap_err();
        assert_eq!(error.code, FocusErrorCode::IneligibleCommitment);
        tx.rollback().await.unwrap();
    });
}
