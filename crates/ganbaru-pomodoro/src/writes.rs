use std::collections::HashMap;

use chrono::{Duration, NaiveDate};
use sqlx::{Row, Sqlite, Transaction};

use super::time::{bounded_overlap_seconds, iso_is_before, iso_seconds_between};
use super::validation::{
    canonical_event_id, validate_event_type, validate_pause, validate_phase,
    validate_run_adaptive_snapshot_for_segment,
};
use super::*;

pub(super) struct RunEventInsert<'a> {
    pub(super) run_id: &'a str,
    pub(super) segment_id: Option<&'a str>,
    pub(super) event_type: &'a str,
    pub(super) occurred_at: &'a str,
    pub(super) phase: Option<&'a str>,
    pub(super) reason: Option<&'a str>,
    pub(super) duration_seconds: Option<i64>,
}

struct AdaptiveRunSnapshotForClose {
    policy_id: String,
    decision_id: String,
    context_key: String,
    state_scores: PomodoroAdaptiveStateScoresWrite,
}

#[derive(Default)]
struct AdaptiveRunOutcomeSummary {
    segments: Vec<AdaptiveSegmentOutcomeSummary>,
    completed_focus_segments: i64,
    interrupted_focus_segments: i64,
    focus_failure_count: i64,
    clean_focus_seconds: i64,
    planned_focus_seconds: i64,
    idle_pause_count: i64,
    idle_pause_seconds: i64,
    manual_pause_count: i64,
    manual_pause_seconds: i64,
    suspend_pause_count: i64,
    suspend_pause_seconds: i64,
    break_started_count: i64,
    break_completed_count: i64,
    break_skipped_count: i64,
    short_break_overtime_seconds: i64,
    long_break_overtime_seconds: i64,
    blocked_attempt_count: i64,
}

struct AdaptiveSegmentOutcomeSummary {
    segment_id: String,
    phase: String,
    status: String,
    end_reason: Option<String>,
    measured_at: String,
    planned_seconds: i64,
    actual_seconds: i64,
    clean_focus_seconds: i64,
    idle_pause_count: i64,
    idle_pause_seconds: i64,
    manual_pause_count: i64,
    manual_pause_seconds: i64,
    suspend_pause_count: i64,
    suspend_pause_seconds: i64,
    break_overtime_seconds: i64,
    blocked_attempt_count: i64,
}

#[derive(Default)]
struct AdaptiveNextDayOutcomeSummary {
    run_count: i64,
    completed_run_count: i64,
    stopped_run_count: i64,
    clean_focus_seconds: i64,
    completed_focus_segments: i64,
    interrupted_focus_segments: i64,
    focus_failure_count: i64,
    break_skipped_count: i64,
    break_overtime_seconds: i64,
    blocked_attempt_count: i64,
}

#[derive(Default)]
struct AdaptiveDayOutcomeSummary {
    run_count: i64,
    completed_run_count: i64,
    stopped_run_count: i64,
    clean_focus_seconds: i64,
    completed_focus_segments: i64,
    interrupted_focus_segments: i64,
    focus_failure_count: i64,
    break_skipped_count: i64,
    break_overtime_seconds: i64,
    blocked_attempt_count: i64,
    planned_pomodoro_event_count: i64,
    started_planned_pomodoro_event_count: i64,
    missed_planned_pomodoro_event_count: i64,
    planned_pomodoro_minutes: i64,
}

struct AdaptivePlannedPomodoroBlock {
    event_id: Option<String>,
    original_event_id: String,
    planned_start: String,
    planned_end: String,
    source_kind: String,
}

struct AdaptiveDecisionForPhaseOutcome {
    decision_id: String,
    segment_id: String,
}

struct AdaptiveDecisionForNextDayOutcome {
    decision_id: String,
    event_date: String,
}

struct AdaptiveDecisionForDayOutcome {
    decision_id: String,
    event_date: String,
}

pub(super) async fn insert_run_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run: &PomodoroRunWrite,
    initial_segment: &PomodoroSegmentWrite,
) -> Result<(), String> {
    validate_run_adaptive_snapshot_for_segment(run, initial_segment)?;
    let canonical_event_id = canonical_event_id(&run.event_id)?.to_string();
    sqlx::query(
        "INSERT INTO pomodoro_runs
            (id, event_id, original_event_id, event_date, planned_start, planned_end,
             started_at, rhythm_kind, rhythm_source, preset_key,
             idle_timeout_minutes, last_heartbeat, event_title_snapshot,
             inherited_focus_minutes, inherited_rhythm_position, inherited_from_run_id, start_trigger)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&run.id)
    .bind(&canonical_event_id)
    .bind(&run.event_id)
    .bind(&run.event_date)
    .bind(&run.planned_start)
    .bind(&run.planned_end)
    .bind(&run.started_at)
    .bind(run_rhythm_kind(&run.rhythm))
    .bind(&run.rhythm_source)
    .bind(&run.preset_key)
    .bind(run.idle_timeout_minutes)
    .bind(&run.started_at)
    .bind(&run.event_title_snapshot)
    .bind(run.inherited_focus_minutes)
    .bind(run.inherited_rhythm_position)
    .bind(&run.inherited_from_run_id)
    .bind(&run.start_trigger)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert pomodoro run: {e}"))?;

    insert_run_rhythm_snapshot_tx(tx, run).await?;

    if let Some(snapshot) = &run.adaptive_snapshot {
        if snapshot.planned_blocks.is_empty() {
            capture_adaptive_planned_blocks_for_day_tx(
                tx,
                &run.id,
                &run.event_date,
                &run.started_at,
            )
            .await?;
        } else {
            insert_adaptive_planned_blocks_tx(
                tx,
                &run.id,
                &run.event_date,
                &run.started_at,
                &snapshot.planned_blocks,
            )
            .await?;
        }
    }

    insert_segment_tx(tx, initial_segment).await?;
    if let Some(snapshot) = &run.adaptive_snapshot {
        insert_run_adaptive_snapshot_tx(tx, run, snapshot).await?;
    }
    insert_run_event_tx(
        tx,
        RunEventInsert {
            run_id: &run.id,
            segment_id: None,
            event_type: "start",
            occurred_at: &run.started_at,
            phase: None,
            reason: Some(&run.start_trigger),
            duration_seconds: None,
        },
    )
    .await?;
    let phase_start = initial_segment
        .actual_start
        .as_deref()
        .unwrap_or(&initial_segment.planned_start);
    insert_run_event_tx(
        tx,
        RunEventInsert {
            run_id: &run.id,
            segment_id: Some(&initial_segment.id),
            event_type: "phase_start",
            occurred_at: phase_start,
            phase: Some(&initial_segment.phase),
            reason: None,
            duration_seconds: None,
        },
    )
    .await?;
    insert_segment_pause_events(tx, &run.id, initial_segment).await
}

pub(super) async fn insert_segment_tx(
    tx: &mut Transaction<'_, Sqlite>,
    segment: &PomodoroSegmentWrite,
) -> Result<(), String> {
    let event_id = canonical_event_id(&segment.event_id)?.to_string();
    if segment.status == "active" {
        let actual_start = segment
            .actual_start
            .as_deref()
            .unwrap_or(&segment.planned_start);
        sqlx::query(
            "UPDATE pomodoro_segments
             SET status = 'completed',
                 actual_end = COALESCE(actual_end, ?),
                 end_reason = COALESCE(end_reason, 'completed')
             WHERE run_id = ? AND status = 'active' AND id <> ?",
        )
        .bind(actual_start)
        .bind(&segment.run_id)
        .bind(&segment.id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("close previous active pomodoro segment: {e}"))?;
    }
    sqlx::query(
        "INSERT INTO pomodoro_segments
            (id, event_id, event_date, run_id, rhythm_position, phase,
             planned_start, planned_end, actual_start, actual_end, status, end_reason)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&segment.id)
    .bind(event_id)
    .bind(&segment.event_date)
    .bind(&segment.run_id)
    .bind(segment.rhythm_position)
    .bind(&segment.phase)
    .bind(&segment.planned_start)
    .bind(&segment.planned_end)
    .bind(&segment.actual_start)
    .bind(&segment.actual_end)
    .bind(&segment.status)
    .bind(&segment.end_reason)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert pomodoro segment: {e}"))?;
    replace_segment_pauses(tx, &segment.id, &segment.pauses).await
}

pub(super) async fn close_run_tx(
    tx: &mut Transaction<'_, Sqlite>,
    closure: &PomodoroRunClosure,
) -> Result<(), String> {
    let ended_at = normalized_close_run_ended_at(tx, closure).await?;

    sqlx::query(
        "UPDATE pomodoro_pauses
         SET ended_at = CASE
           WHEN started_at > ? THEN started_at
           ELSE ?
         END
         WHERE ended_at IS NULL
           AND segment_id IN (
             SELECT id FROM pomodoro_segments WHERE run_id = ?
           )",
    )
    .bind(&ended_at)
    .bind(&ended_at)
    .bind(&closure.run_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("close pomodoro pauses: {e}"))?;

    sqlx::query(
        "UPDATE pomodoro_segments
         SET status = ?,
             actual_end = COALESCE(actual_end, CASE
               WHEN actual_start IS NOT NULL AND actual_start > ? THEN actual_start
               ELSE ?
             END),
             end_reason = ?
         WHERE run_id = ? AND status = 'active'",
    )
    .bind(&closure.segment_status)
    .bind(&ended_at)
    .bind(&ended_at)
    .bind(&closure.segment_end_reason)
    .bind(&closure.run_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("close active pomodoro segment: {e}"))?;

    let result = sqlx::query(
        "UPDATE pomodoro_runs
         SET ended_at = ?, end_reason = ?, last_heartbeat = ?
         WHERE id = ? AND ended_at IS NULL",
    )
    .bind(&ended_at)
    .bind(&closure.end_reason)
    .bind(&ended_at)
    .bind(&closure.run_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("close pomodoro run: {e}"))?;
    if result.rows_affected() == 0 {
        return Err(format!("open pomodoro run not found: {}", closure.run_id));
    }

    insert_run_event_tx(
        tx,
        RunEventInsert {
            run_id: &closure.run_id,
            segment_id: None,
            event_type: &closure.event_type,
            occurred_at: &ended_at,
            phase: None,
            reason: Some(&closure.end_reason),
            duration_seconds: None,
        },
    )
    .await?;

    record_adaptive_run_close_tx(tx, closure, &ended_at).await
}

async fn insert_adaptive_outcome_numeric_tx(
    tx: &mut Transaction<'_, Sqlite>,
    decision_id: &str,
    assignment_id: Option<&str>,
    outcome_window: &str,
    outcome_key: &str,
    numeric_value: f64,
    measured_at: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO pomodoro_adaptive_outcomes
            (id, decision_id, assignment_id, outcome_window, outcome_key,
             numeric_value, boolean_value, categorical_value, measured_at)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, ?, NULL, NULL, ?)",
    )
    .bind(decision_id)
    .bind(assignment_id)
    .bind(outcome_window)
    .bind(outcome_key)
    .bind(numeric_value)
    .bind(measured_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert adaptive numeric outcome {outcome_key}: {e}"))?;
    Ok(())
}

async fn insert_adaptive_outcome_boolean_tx(
    tx: &mut Transaction<'_, Sqlite>,
    decision_id: &str,
    assignment_id: Option<&str>,
    outcome_window: &str,
    outcome_key: &str,
    boolean_value: bool,
    measured_at: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO pomodoro_adaptive_outcomes
            (id, decision_id, assignment_id, outcome_window, outcome_key,
             numeric_value, boolean_value, categorical_value, measured_at)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, NULL, ?, NULL, ?)",
    )
    .bind(decision_id)
    .bind(assignment_id)
    .bind(outcome_window)
    .bind(outcome_key)
    .bind(if boolean_value { 1_i64 } else { 0_i64 })
    .bind(measured_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert adaptive boolean outcome {outcome_key}: {e}"))?;
    Ok(())
}

async fn insert_adaptive_outcome_categorical_tx(
    tx: &mut Transaction<'_, Sqlite>,
    decision_id: &str,
    assignment_id: Option<&str>,
    outcome_window: &str,
    outcome_key: &str,
    categorical_value: &str,
    measured_at: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO pomodoro_adaptive_outcomes
            (id, decision_id, assignment_id, outcome_window, outcome_key,
             numeric_value, boolean_value, categorical_value, measured_at)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, NULL, NULL, ?, ?)",
    )
    .bind(decision_id)
    .bind(assignment_id)
    .bind(outcome_window)
    .bind(outcome_key)
    .bind(categorical_value)
    .bind(measured_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert adaptive categorical outcome {outcome_key}: {e}"))?;
    Ok(())
}

fn pause_seconds_for_segment(
    pauses: &[PomodoroPauseWrite],
    segment_start: &str,
    segment_end: &str,
) -> i64 {
    pauses
        .iter()
        .map(|pause| {
            let pause_end = pause.ended_at.as_deref().unwrap_or(segment_end);
            bounded_overlap_seconds(&pause.started_at, pause_end, segment_start, segment_end)
        })
        .sum()
}

fn pause_duration_seconds(pause: &PomodoroPauseWrite, fallback_end: &str) -> i64 {
    iso_seconds_between(
        &pause.started_at,
        pause.ended_at.as_deref().unwrap_or(fallback_end),
    )
    .unwrap_or(0)
    .max(0)
}

pub(crate) mod requests;
mod run_outcomes;
use run_outcomes::*;
mod day_outcomes;
pub(super) use day_outcomes::*;
mod context_state;
use context_state::*;
mod snapshots;
pub(super) use snapshots::*;
mod run_events;
pub(super) use run_events::*;
