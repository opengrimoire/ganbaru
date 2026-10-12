use super::*;

pub(crate) async fn record_adaptive_run_close_tx(
    tx: &mut Transaction<'_, Sqlite>,
    closure: &PomodoroRunClosure,
    ended_at: &str,
) -> Result<(), String> {
    let Some(snapshot) = load_adaptive_run_snapshot_for_close(tx, &closure.run_id).await? else {
        return Ok(());
    };
    let summary = load_adaptive_run_outcome_summary(tx, &closure.run_id, ended_at).await?;
    insert_adaptive_run_outcomes_tx(tx, &snapshot.decision_id, closure, ended_at, &summary).await?;
    insert_adaptive_phase_outcomes_tx(tx, &closure.run_id, &summary).await?;
    record_matured_adaptive_day_outcomes_tx(tx, &closure.run_id, ended_at).await?;
    record_matured_adaptive_next_day_outcomes_tx(tx, &closure.run_id, ended_at).await?;
    upsert_adaptive_context_state_tx(tx, &snapshot, ended_at, &summary, closure).await
}

async fn load_adaptive_run_snapshot_for_close(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
) -> Result<Option<AdaptiveRunSnapshotForClose>, String> {
    let row = sqlx::query(
        "SELECT rs.policy_id AS policy_id,
                rs.decision_id AS decision_id,
                cs.time_of_day AS time_of_day,
                cs.session_position AS session_position,
                cs.event_length AS event_length,
                cs.workload AS workload,
                cs.energy AS energy,
                cs.environment_id AS environment_id,
                ss.readiness AS readiness,
                ss.strain AS strain,
                ss.recovery_debt AS recovery_debt,
                ss.avoidance_pressure AS avoidance_pressure,
                ss.momentum AS momentum,
                ss.confidence AS confidence
         FROM pomodoro_run_adaptive_snapshots rs
         JOIN pomodoro_adaptive_context_snapshots cs ON cs.id = rs.context_snapshot_id
         JOIN pomodoro_adaptive_decision_state_scores ss ON ss.decision_id = rs.decision_id
         WHERE rs.run_id = ?",
    )
    .bind(run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load adaptive run snapshot for close: {e}"))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let policy_id: Option<String> = row
        .try_get("policy_id")
        .map_err(|e| format!("read adaptive close policy_id: {e}"))?;
    let decision_id: Option<String> = row
        .try_get("decision_id")
        .map_err(|e| format!("read adaptive close decision_id: {e}"))?;
    let Some(policy_id) = policy_id else {
        return Ok(None);
    };
    let Some(decision_id) = decision_id else {
        return Ok(None);
    };
    let environment_id: Option<String> = row
        .try_get("environment_id")
        .map_err(|e| format!("read adaptive close environment_id: {e}"))?;
    let context_key = format!(
        "{}:{}:{}:{}:{}:{}",
        row.try_get::<String, _>("time_of_day")
            .map_err(|e| format!("read adaptive close time_of_day: {e}"))?,
        row.try_get::<String, _>("session_position")
            .map_err(|e| format!("read adaptive close session_position: {e}"))?,
        row.try_get::<String, _>("event_length")
            .map_err(|e| format!("read adaptive close event_length: {e}"))?,
        row.try_get::<String, _>("workload")
            .map_err(|e| format!("read adaptive close workload: {e}"))?,
        row.try_get::<String, _>("energy")
            .map_err(|e| format!("read adaptive close energy: {e}"))?,
        environment_id.as_deref().unwrap_or("none"),
    );
    Ok(Some(AdaptiveRunSnapshotForClose {
        policy_id,
        decision_id,
        context_key,
        state_scores: PomodoroAdaptiveStateScoresWrite {
            readiness: row
                .try_get("readiness")
                .map_err(|e| format!("read adaptive close readiness: {e}"))?,
            strain: row
                .try_get("strain")
                .map_err(|e| format!("read adaptive close strain: {e}"))?,
            recovery_debt: row
                .try_get("recovery_debt")
                .map_err(|e| format!("read adaptive close recovery_debt: {e}"))?,
            avoidance_pressure: row
                .try_get("avoidance_pressure")
                .map_err(|e| format!("read adaptive close avoidance_pressure: {e}"))?,
            momentum: row
                .try_get("momentum")
                .map_err(|e| format!("read adaptive close momentum: {e}"))?,
            confidence: row
                .try_get("confidence")
                .map_err(|e| format!("read adaptive close confidence: {e}"))?,
        },
    }))
}

pub(crate) async fn load_adaptive_run_outcome_summary(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    ended_at: &str,
) -> Result<AdaptiveRunOutcomeSummary, String> {
    let segment_rows = sqlx::query(
        "SELECT id, phase, planned_start, planned_end, actual_start, actual_end, status, end_reason
         FROM pomodoro_segments
         WHERE run_id = ?
         ORDER BY planned_start ASC",
    )
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load adaptive run close segments: {e}"))?;
    let segment_ids = segment_rows
        .iter()
        .map(|row| row.try_get::<String, _>("id"))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read adaptive run close segment id: {e}"))?;
    let pauses_by_segment = load_pauses_by_segment_tx(tx, &segment_ids).await?;
    let mut summary = AdaptiveRunOutcomeSummary::default();

    for row in segment_rows {
        let segment_id: String = row
            .try_get("id")
            .map_err(|e| format!("read adaptive outcome segment id: {e}"))?;
        let phase: String = row
            .try_get("phase")
            .map_err(|e| format!("read adaptive outcome phase: {e}"))?;
        let planned_start: String = row
            .try_get("planned_start")
            .map_err(|e| format!("read adaptive outcome planned_start: {e}"))?;
        let planned_end: String = row
            .try_get("planned_end")
            .map_err(|e| format!("read adaptive outcome planned_end: {e}"))?;
        let actual_start: Option<String> = row
            .try_get("actual_start")
            .map_err(|e| format!("read adaptive outcome actual_start: {e}"))?;
        let actual_end: Option<String> = row
            .try_get("actual_end")
            .map_err(|e| format!("read adaptive outcome actual_end: {e}"))?;
        let status: String = row
            .try_get("status")
            .map_err(|e| format!("read adaptive outcome status: {e}"))?;
        let end_reason: Option<String> = row
            .try_get("end_reason")
            .map_err(|e| format!("read adaptive outcome end_reason: {e}"))?;
        let planned_seconds = iso_seconds_between(&planned_start, &planned_end)
            .unwrap_or(0)
            .max(0);
        let actual_end = actual_end.as_deref().unwrap_or(ended_at);
        let pauses = pauses_by_segment
            .get(&segment_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let actual_start_for_window = actual_start.as_deref().unwrap_or(&planned_start);
        let actual_seconds = iso_seconds_between(actual_start_for_window, actual_end)
            .unwrap_or(0)
            .max(0);
        let mut segment_outcome = AdaptiveSegmentOutcomeSummary {
            segment_id: segment_id.clone(),
            phase: phase.clone(),
            status: status.clone(),
            end_reason: end_reason.clone(),
            measured_at: actual_end.to_string(),
            planned_seconds,
            actual_seconds,
            clean_focus_seconds: 0,
            idle_pause_count: 0,
            idle_pause_seconds: 0,
            manual_pause_count: 0,
            manual_pause_seconds: 0,
            suspend_pause_count: 0,
            suspend_pause_seconds: 0,
            break_overtime_seconds: 0,
            blocked_attempt_count: count_blocked_attempts_for_interval_tx(
                tx,
                run_id,
                actual_start_for_window,
                actual_end,
            )
            .await?,
        };

        if phase == "focus" {
            summary.planned_focus_seconds += planned_seconds;
            if status == "completed" {
                summary.completed_focus_segments += 1;
            } else if status == "interrupted" {
                summary.interrupted_focus_segments += 1;
            }
            if end_reason.as_deref() == Some("focus_failed") {
                summary.focus_failure_count += 1;
            }
            if let Some(actual_start) = actual_start.as_deref() {
                let pause_seconds = pause_seconds_for_segment(pauses, actual_start, actual_end);
                let clean_focus_seconds = (actual_seconds - pause_seconds).max(0);
                summary.clean_focus_seconds += clean_focus_seconds;
                segment_outcome.clean_focus_seconds = clean_focus_seconds;
            }
        } else {
            if status != "skipped" {
                summary.break_started_count += 1;
            }
            if status == "completed" {
                summary.break_completed_count += 1;
            }
            if status == "skipped" || end_reason.as_deref() == Some("skipped_by_user") {
                summary.break_skipped_count += 1;
            }
            let overtime_seconds = iso_seconds_between(&planned_end, actual_end)
                .unwrap_or(0)
                .max(0);
            if phase == "short_break" {
                summary.short_break_overtime_seconds += overtime_seconds;
            } else if phase == "long_break" {
                summary.long_break_overtime_seconds += overtime_seconds;
            }
            segment_outcome.break_overtime_seconds = overtime_seconds;
        }

        for pause in pauses {
            let pause_seconds = pause_duration_seconds(pause, actual_end);
            match pause.reason.as_str() {
                "idle" => {
                    summary.idle_pause_count += 1;
                    summary.idle_pause_seconds += pause_seconds;
                    segment_outcome.idle_pause_count += 1;
                    segment_outcome.idle_pause_seconds += pause_seconds;
                }
                "manual" => {
                    summary.manual_pause_count += 1;
                    summary.manual_pause_seconds += pause_seconds;
                    segment_outcome.manual_pause_count += 1;
                    segment_outcome.manual_pause_seconds += pause_seconds;
                }
                "suspend" => {
                    summary.suspend_pause_count += 1;
                    summary.suspend_pause_seconds += pause_seconds;
                    segment_outcome.suspend_pause_count += 1;
                    segment_outcome.suspend_pause_seconds += pause_seconds;
                }
                _ => {}
            }
        }
        summary.segments.push(segment_outcome);
    }

    summary.blocked_attempt_count = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM distractions_block_events
         WHERE run_id = ?
           AND occurred_at <= ?
           AND decision IN ('blocked', 'limit_exhausted')",
    )
    .bind(run_id)
    .bind(ended_at)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("count adaptive close block events: {e}"))?;

    Ok(summary)
}

async fn count_blocked_attempts_for_interval_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    started_at: &str,
    ended_at: &str,
) -> Result<i64, String> {
    sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM distractions_block_events
         WHERE run_id = ?
           AND occurred_at >= ?
           AND occurred_at <= ?
           AND decision IN ('blocked', 'limit_exhausted')",
    )
    .bind(run_id)
    .bind(started_at)
    .bind(ended_at)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("count adaptive segment block events: {e}"))
}

async fn load_pauses_by_segment_tx(
    tx: &mut Transaction<'_, Sqlite>,
    segment_ids: &[String],
) -> Result<HashMap<String, Vec<PomodoroPauseWrite>>, String> {
    if segment_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat_n("?", segment_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT segment_id, started_at, ended_at, reason
         FROM pomodoro_pauses
         WHERE segment_id IN ({placeholders})
         ORDER BY started_at ASC"
    );
    let mut q = sqlx::query(&query);
    for id in segment_ids {
        q = q.bind(id);
    }
    let rows = q
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| format!("load adaptive close pauses: {e}"))?;
    let mut pauses_by_segment: HashMap<String, Vec<PomodoroPauseWrite>> = HashMap::new();
    for row in rows {
        let segment_id: String = row
            .try_get("segment_id")
            .map_err(|e| format!("read adaptive close pause segment_id: {e}"))?;
        pauses_by_segment
            .entry(segment_id)
            .or_default()
            .push(PomodoroPauseWrite {
                started_at: row
                    .try_get("started_at")
                    .map_err(|e| format!("read adaptive close pause started_at: {e}"))?,
                ended_at: row
                    .try_get("ended_at")
                    .map_err(|e| format!("read adaptive close pause ended_at: {e}"))?,
                reason: row
                    .try_get("reason")
                    .map_err(|e| format!("read adaptive close pause reason: {e}"))?,
            });
    }
    Ok(pauses_by_segment)
}

async fn insert_adaptive_run_outcomes_tx(
    tx: &mut Transaction<'_, Sqlite>,
    decision_id: &str,
    closure: &PomodoroRunClosure,
    ended_at: &str,
    summary: &AdaptiveRunOutcomeSummary,
) -> Result<(), String> {
    let assignment_id = load_adaptive_assignment_id_for_decision_tx(tx, decision_id).await?;
    let assignment_id = assignment_id.as_deref();
    for (key, value) in [
        ("completed_focus_segments", summary.completed_focus_segments),
        (
            "interrupted_focus_segments",
            summary.interrupted_focus_segments,
        ),
        ("focus_failure_count", summary.focus_failure_count),
        ("clean_focus_seconds", summary.clean_focus_seconds),
        ("planned_focus_seconds", summary.planned_focus_seconds),
        ("idle_pause_count", summary.idle_pause_count),
        ("idle_pause_seconds", summary.idle_pause_seconds),
        ("manual_pause_count", summary.manual_pause_count),
        ("manual_pause_seconds", summary.manual_pause_seconds),
        ("suspend_pause_count", summary.suspend_pause_count),
        ("suspend_pause_seconds", summary.suspend_pause_seconds),
        ("break_started_count", summary.break_started_count),
        ("break_completed_count", summary.break_completed_count),
        ("break_skipped_count", summary.break_skipped_count),
        (
            "short_break_overtime_seconds",
            summary.short_break_overtime_seconds,
        ),
        (
            "long_break_overtime_seconds",
            summary.long_break_overtime_seconds,
        ),
        ("blocked_attempt_count", summary.blocked_attempt_count),
    ] {
        insert_adaptive_outcome_numeric_tx(
            tx,
            decision_id,
            assignment_id,
            "run",
            key,
            value as f64,
            ended_at,
        )
        .await?;
    }
    insert_adaptive_outcome_boolean_tx(
        tx,
        decision_id,
        assignment_id,
        "run",
        "run_completed",
        closure.end_reason == "completed",
        ended_at,
    )
    .await?;
    insert_adaptive_outcome_boolean_tx(
        tx,
        decision_id,
        assignment_id,
        "run",
        "run_stopped",
        closure.end_reason == "stopped",
        ended_at,
    )
    .await?;
    insert_adaptive_outcome_categorical_tx(
        tx,
        decision_id,
        assignment_id,
        "run",
        "run_end_reason",
        &closure.end_reason,
        ended_at,
    )
    .await
}

async fn insert_adaptive_phase_outcomes_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    summary: &AdaptiveRunOutcomeSummary,
) -> Result<(), String> {
    let decisions = load_adaptive_phase_outcome_decisions_tx(tx, run_id).await?;
    for decision in decisions {
        let Some(segment) = summary
            .segments
            .iter()
            .find(|segment| segment.segment_id == decision.segment_id)
        else {
            continue;
        };
        insert_adaptive_segment_phase_outcomes_tx(tx, &decision.decision_id, segment).await?;
        if segment.phase == "short_break" || segment.phase == "long_break" {
            insert_adaptive_post_break_outcomes_tx(tx, &decision.decision_id, segment, summary)
                .await?;
        }
    }
    Ok(())
}

async fn load_adaptive_phase_outcome_decisions_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
) -> Result<Vec<AdaptiveDecisionForPhaseOutcome>, String> {
    let rows = sqlx::query(
        "SELECT id, segment_id
         FROM pomodoro_adaptive_decisions
         WHERE run_id = ?
           AND segment_id IS NOT NULL
           AND opportunity_kind IN ('run_start', 'focus_start', 'break_start')
         ORDER BY occurred_at ASC",
    )
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load adaptive phase outcome decisions: {e}"))?;
    rows.into_iter()
        .map(|row| {
            let segment_id: Option<String> = row
                .try_get("segment_id")
                .map_err(|e| format!("read adaptive phase outcome segment_id: {e}"))?;
            let Some(segment_id) = segment_id else {
                return Err("adaptive phase outcome decision missing segment_id".to_string());
            };
            Ok(AdaptiveDecisionForPhaseOutcome {
                decision_id: row
                    .try_get("id")
                    .map_err(|e| format!("read adaptive phase outcome decision id: {e}"))?,
                segment_id,
            })
        })
        .collect()
}

async fn insert_adaptive_segment_phase_outcomes_tx(
    tx: &mut Transaction<'_, Sqlite>,
    decision_id: &str,
    segment: &AdaptiveSegmentOutcomeSummary,
) -> Result<(), String> {
    let assignment_id = load_adaptive_assignment_id_for_decision_tx(tx, decision_id).await?;
    let assignment_id = assignment_id.as_deref();
    for (key, value) in [
        ("phase_planned_seconds", segment.planned_seconds),
        ("phase_actual_seconds", segment.actual_seconds),
        ("phase_clean_focus_seconds", segment.clean_focus_seconds),
        ("phase_idle_pause_count", segment.idle_pause_count),
        ("phase_idle_pause_seconds", segment.idle_pause_seconds),
        ("phase_manual_pause_count", segment.manual_pause_count),
        ("phase_manual_pause_seconds", segment.manual_pause_seconds),
        ("phase_suspend_pause_count", segment.suspend_pause_count),
        ("phase_suspend_pause_seconds", segment.suspend_pause_seconds),
        (
            "phase_break_overtime_seconds",
            segment.break_overtime_seconds,
        ),
        ("phase_blocked_attempt_count", segment.blocked_attempt_count),
    ] {
        insert_adaptive_outcome_numeric_tx(
            tx,
            decision_id,
            assignment_id,
            "phase",
            key,
            value as f64,
            &segment.measured_at,
        )
        .await?;
    }
    insert_adaptive_outcome_boolean_tx(
        tx,
        decision_id,
        assignment_id,
        "phase",
        "phase_completed",
        segment.status == "completed",
        &segment.measured_at,
    )
    .await?;
    insert_adaptive_outcome_boolean_tx(
        tx,
        decision_id,
        assignment_id,
        "phase",
        "phase_interrupted",
        segment.status == "interrupted",
        &segment.measured_at,
    )
    .await?;
    insert_adaptive_outcome_categorical_tx(
        tx,
        decision_id,
        assignment_id,
        "phase",
        "phase_kind",
        &segment.phase,
        &segment.measured_at,
    )
    .await?;
    insert_adaptive_outcome_categorical_tx(
        tx,
        decision_id,
        assignment_id,
        "phase",
        "phase_status",
        &segment.status,
        &segment.measured_at,
    )
    .await?;
    insert_adaptive_outcome_categorical_tx(
        tx,
        decision_id,
        assignment_id,
        "phase",
        "phase_end_reason",
        segment.end_reason.as_deref().unwrap_or("none"),
        &segment.measured_at,
    )
    .await
}

async fn insert_adaptive_post_break_outcomes_tx(
    tx: &mut Transaction<'_, Sqlite>,
    decision_id: &str,
    break_segment: &AdaptiveSegmentOutcomeSummary,
    summary: &AdaptiveRunOutcomeSummary,
) -> Result<(), String> {
    let assignment_id = load_adaptive_assignment_id_for_decision_tx(tx, decision_id).await?;
    let assignment_id = assignment_id.as_deref();
    let next_focus = next_focus_after_segment(summary, &break_segment.segment_id);
    insert_adaptive_outcome_boolean_tx(
        tx,
        decision_id,
        assignment_id,
        "phase",
        "post_break_next_focus_observed",
        next_focus.is_some(),
        &break_segment.measured_at,
    )
    .await?;

    let Some(next_focus) = next_focus else {
        return Ok(());
    };
    insert_adaptive_outcome_boolean_tx(
        tx,
        decision_id,
        assignment_id,
        "phase",
        "post_break_next_focus_completed",
        next_focus.status == "completed",
        &next_focus.measured_at,
    )
    .await?;
    insert_adaptive_outcome_numeric_tx(
        tx,
        decision_id,
        assignment_id,
        "phase",
        "post_break_next_focus_clean_seconds",
        next_focus.clean_focus_seconds as f64,
        &next_focus.measured_at,
    )
    .await?;
    insert_adaptive_outcome_numeric_tx(
        tx,
        decision_id,
        assignment_id,
        "phase",
        "post_break_next_focus_blocked_attempt_count",
        next_focus.blocked_attempt_count as f64,
        &next_focus.measured_at,
    )
    .await
}

fn next_focus_after_segment<'a>(
    summary: &'a AdaptiveRunOutcomeSummary,
    segment_id: &str,
) -> Option<&'a AdaptiveSegmentOutcomeSummary> {
    let start_index = summary
        .segments
        .iter()
        .position(|segment| segment.segment_id == segment_id)?;
    summary
        .segments
        .iter()
        .skip(start_index + 1)
        .find(|segment| segment.phase == "focus")
}
