use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{Row, Sqlite, SqlitePool, Transaction, types::Json};

use super::super::{PomodoroRunRhythm, PomodoroRunSequenceStep};
use super::models::*;

pub(super) const MAX_SEGMENT_PAUSES: i64 = 1024;
pub(super) fn format_timestamp(ms: i64) -> Result<String, FocusExecutionError> {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| {
            execution_error(
                FocusErrorCode::InvalidState,
                "Focus timestamp is outside its supported range",
            )
        })
}

pub(super) fn parse_timestamp_ms(value: &str) -> Result<i64, FocusExecutionError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.timestamp_millis())
        .map_err(|error| {
            execution_error(
                FocusErrorCode::InvalidState,
                format!("Invalid canonical Focus timestamp: {error}"),
            )
        })
}

pub(super) fn phase(value: &str) -> Result<FocusPhase, FocusExecutionError> {
    match value {
        "focus" => Ok(FocusPhase::Focus),
        "short_break" => Ok(FocusPhase::ShortBreak),
        "long_break" => Ok(FocusPhase::LongBreak),
        _ => Err(execution_error(
            FocusErrorCode::InvalidState,
            "Invalid canonical Focus phase",
        )),
    }
}

pub(super) async fn load_state(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(i64, ExecutionState), FocusExecutionError> {
    let (revision, Json(state)) = sqlx::query_as::<_, (i64, Json<ExecutionState>)>(
        "SELECT revision, state_json FROM pomodoro_execution_state WHERE singleton = 1",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| format!("Read Focus execution state: {error}"))?;
    Ok((revision, state))
}

pub(super) async fn load_receipt(
    tx: &mut Transaction<'_, Sqlite>,
    command: &FocusCommand,
) -> Result<Option<FocusExecutionSnapshot>, FocusExecutionError> {
    let receipt = sqlx::query_as::<_, (Json<FocusCommand>, Json<FocusExecutionSnapshot>)>(
        "SELECT request_json, result_json FROM pomodoro_execution_receipts WHERE command_id = ?",
    )
    .bind(&command.command_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| format!("Read Focus command receipt: {error}"))?;
    match receipt {
        None => Ok(None),
        Some((Json(previous), Json(result))) if previous == *command => Ok(Some(result)),
        Some(_) => Err(execution_error(
            FocusErrorCode::CommandIdentityConflict,
            "Focus command identity was already used for a different intent",
        )),
    }
}

pub(super) async fn reserve_revision(
    tx: &mut Transaction<'_, Sqlite>,
    expected: i64,
) -> Result<i64, FocusExecutionError> {
    let next = expected.checked_add(1).ok_or_else(|| {
        execution_error(FocusErrorCode::InvalidState, "Focus revision is exhausted")
    })?;
    let changed = sqlx::query(
        "UPDATE pomodoro_execution_state SET revision = ? WHERE singleton = 1 AND revision = ?",
    )
    .bind(next)
    .bind(expected)
    .execute(&mut **tx)
    .await
    .map_err(|error| format!("Reserve Focus execution revision: {error}"))?;
    if changed.rows_affected() != 1 {
        return Err(execution_error(
            FocusErrorCode::StaleRevision,
            "Focus execution changed before this command could be accepted",
        ));
    }
    Ok(next)
}

pub(super) async fn save_state(
    tx: &mut Transaction<'_, Sqlite>,
    state: &ExecutionState,
    revision: i64,
    now_ms: i64,
) -> Result<(), FocusExecutionError> {
    let changed = sqlx::query("UPDATE pomodoro_execution_state SET state_json = ?, updated_at_ms = ? WHERE singleton = 1 AND revision = ?")
        .bind(Json(state)).bind(now_ms).bind(revision).execute(&mut **tx).await
        .map_err(|error| format!("Persist Focus execution state: {error}"))?;
    if changed.rows_affected() != 1 {
        return Err(execution_error(
            FocusErrorCode::StaleRevision,
            "Focus execution revision changed during commit",
        ));
    }
    Ok(())
}

pub(super) async fn save_receipt(
    tx: &mut Transaction<'_, Sqlite>,
    command: &FocusCommand,
    snapshot: &FocusExecutionSnapshot,
) -> Result<(), FocusExecutionError> {
    sqlx::query("INSERT INTO pomodoro_execution_receipts (command_id, request_json, result_json, revision, committed_at_ms) VALUES (?, ?, ?, ?, ?)")
        .bind(&command.command_id).bind(Json(command)).bind(Json(snapshot)).bind(snapshot.revision)
        .bind(snapshot.observed_at_ms).execute(&mut **tx).await
        .map_err(|error| format!("Persist Focus command receipt: {error}"))?;
    Ok(())
}

pub(super) async fn allocate_id(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<String, FocusExecutionError> {
    sqlx::query_scalar("SELECT lower(hex(randomblob(16)))")
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| format!("Allocate Focus identity: {error}").into())
}

pub(super) async fn load_run(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<FocusRunSnapshot, FocusExecutionError> {
    let row = sqlx::query("SELECT id, event_id,
        COALESCE(current_occurrence_id, original_event_id) AS occurrence_id,
        COALESCE(current_event_date, event_date) AS current_date,
        COALESCE(current_event_title, event_title_snapshot) AS current_title,
        started_at, planned_start, planned_end, ended_at, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes,
        inherited_focus_milliseconds AS inherited_focus_ms,
        inherited_phase_milliseconds AS inherited_phase_ms
        FROM pomodoro_runs WHERE id = ?")
        .bind(id).fetch_one(&mut **tx).await.map_err(|error| format!("Read canonical Focus run: {error}"))?;
    let kind: String = row
        .try_get("rhythm_kind")
        .map_err(|error| error.to_string())?;
    let mut rhythm = match kind.as_str() {
        "count" => {
            let (focus_duration_minutes, short_break_minutes, long_break_minutes, long_break_after_focus_count) =
                sqlx::query_as::<_, (i64, i64, i64, i64)>("SELECT focus_duration_minutes, short_break_minutes, long_break_minutes, long_break_after_focus_count FROM pomodoro_run_count_rhythms WHERE run_id = ?")
                    .bind(id).fetch_one(&mut **tx).await.map_err(|error| format!("Read Focus count rhythm: {error}"))?;
            PomodoroRunRhythm::Count {
                focus_duration_minutes,
                short_break_minutes,
                long_break_minutes,
                long_break_after_focus_count,
            }
        }
        "sequence" => {
            let steps = sqlx::query_as::<_, (i64, String, i64)>("SELECT focus_duration_minutes, break_phase, break_duration_minutes FROM pomodoro_run_sequence_steps WHERE run_id = ? ORDER BY step_index LIMIT 13")
                .bind(id).fetch_all(&mut **tx).await.map_err(|error| format!("Read Focus sequence rhythm: {error}"))?
                .into_iter().map(|(focus_duration_minutes, break_phase, break_duration_minutes)| PomodoroRunSequenceStep { focus_duration_minutes, break_phase, break_duration_minutes }).collect();
            PomodoroRunRhythm::Sequence { steps }
        }
        _ => {
            return Err(execution_error(
                FocusErrorCode::InvalidState,
                "Invalid canonical Focus rhythm",
            ));
        }
    };
    let preset_key: Option<String> = row
        .try_get("preset_key")
        .map_err(|error| error.to_string())?;
    if preset_key.as_deref() == Some("adaptive") {
        if let Some(selected) = load_latest_adaptive_rhythm(tx, id).await? {
            rhythm = selected;
        }
    }
    super::super::validation::validate_run_rhythm(&rhythm)
        .map_err(|error| execution_error(FocusErrorCode::InvalidState, error))?;
    let started_at: String = row
        .try_get("started_at")
        .map_err(|error| error.to_string())?;
    let planned_start: String = row
        .try_get("planned_start")
        .map_err(|error| error.to_string())?;
    let planned_end: String = row
        .try_get("planned_end")
        .map_err(|error| error.to_string())?;
    let ended_at: Option<String> = row.try_get("ended_at").map_err(|error| error.to_string())?;
    Ok(FocusRunSnapshot {
        id: row.try_get("id").map_err(|error| error.to_string())?,
        event_id: row.try_get("event_id").map_err(|error| error.to_string())?,
        occurrence_id: row
            .try_get("occurrence_id")
            .map_err(|error| error.to_string())?,
        event_date: row
            .try_get("current_date")
            .map_err(|error| error.to_string())?,
        title: row
            .try_get("current_title")
            .map_err(|error| error.to_string())?,
        started_at_ms: parse_timestamp_ms(&started_at)?,
        planned_start_ms: parse_timestamp_ms(&planned_start)?,
        planned_end_ms: parse_timestamp_ms(&planned_end)?,
        ended_at_ms: ended_at.as_deref().map(parse_timestamp_ms).transpose()?,
        inherited_focus_ms: row
            .try_get::<Option<i64>, _>("inherited_focus_ms")
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                super::decisions::invalid_state("Focus run is missing inherited focus duration")
            })?,
        inherited_phase_ms: row
            .try_get::<Option<i64>, _>("inherited_phase_ms")
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                super::decisions::invalid_state("Focus run is missing inherited phase duration")
            })?,
        configuration: FocusConfiguration {
            rhythm,
            rhythm_source: row
                .try_get("rhythm_source")
                .map_err(|error| error.to_string())?,
            preset_key,
            idle_timeout_minutes: row
                .try_get("idle_timeout_minutes")
                .map_err(|error| error.to_string())?,
        },
    })
}

/// The run-start rhythm remains immutable history. Current execution derives its
/// rhythm from the latest accepted adaptive decision, including after restart.
async fn load_latest_adaptive_rhythm(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
) -> Result<Option<PomodoroRunRhythm>, FocusExecutionError> {
    let decision: Option<String> = sqlx::query_scalar("SELECT id FROM pomodoro_adaptive_decisions WHERE run_id = ? AND opportunity_kind IN ('run_start', 'focus_start', 'break_start') ORDER BY julianday(occurred_at) DESC, rowid DESC LIMIT 1")
        .bind(run_id).fetch_optional(&mut **tx).await.map_err(|error| format!("Read latest accepted adaptive decision: {error}"))?;
    let Some(decision) = decision else {
        return Ok(None);
    };
    let rows = sqlx::query_as::<_, (String, f64)>("SELECT value_key, selected_numeric_value FROM pomodoro_adaptive_decision_values WHERE decision_id = ? LIMIT 5")
        .bind(decision).fetch_all(&mut **tx).await.map_err(|error| format!("Read accepted adaptive rhythm: {error}"))?;
    if rows.len() != 4 {
        return Err(super::decisions::invalid_state(
            "Accepted adaptive rhythm must have four values",
        ));
    }
    let mut values = std::collections::BTreeMap::new();
    for (key, value) in rows {
        if !value.is_finite() || value < 0.0 || value > i64::MAX as f64 || value.fract() != 0.0 {
            return Err(super::decisions::invalid_state(
                "Accepted adaptive rhythm contains an invalid integer",
            ));
        }
        values.insert(key, value as i64);
    }
    let mut take = |key: &str| {
        values.remove(key).ok_or_else(|| {
            super::decisions::invalid_state("Accepted adaptive rhythm is missing a required value")
        })
    };
    Ok(Some(PomodoroRunRhythm::Count {
        focus_duration_minutes: take("focus_duration_minutes")?,
        short_break_minutes: take("short_break_minutes")?,
        long_break_minutes: take("long_break_minutes")?,
        long_break_after_focus_count: take("long_break_after_focus_count")?,
    }))
}

pub(super) async fn load_segment(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<FocusSegmentSnapshot, FocusExecutionError> {
    let row = sqlx::query("SELECT id, run_id, event_id, event_date, phase, rhythm_position, planned_start,
        planned_end, actual_start, actual_end, chosen_duration_ms, status, end_reason FROM pomodoro_segments WHERE id = ?")
        .bind(id).fetch_one(&mut **tx).await.map_err(|error| format!("Read canonical Focus segment: {error}"))?;
    let phase_name: String = row.try_get("phase").map_err(|error| error.to_string())?;
    let planned_start: String = row
        .try_get("planned_start")
        .map_err(|error| error.to_string())?;
    let planned_end: String = row
        .try_get("planned_end")
        .map_err(|error| error.to_string())?;
    let actual_start: String = row
        .try_get("actual_start")
        .map_err(|error| error.to_string())?;
    let actual_end: Option<String> = row
        .try_get("actual_end")
        .map_err(|error| error.to_string())?;
    let pause_rows = sqlx::query_as::<_, (String, Option<String>, String)>(
        "SELECT started_at, ended_at, reason FROM pomodoro_pauses WHERE segment_id = ? ORDER BY started_at, id LIMIT ?",
    ).bind(id).bind(MAX_SEGMENT_PAUSES + 1).fetch_all(&mut **tx).await
        .map_err(|error| format!("Read canonical Focus pauses: {error}"))?;
    if pause_rows.len() > MAX_SEGMENT_PAUSES as usize {
        return Err(execution_error(
            FocusErrorCode::InvalidState,
            "Focus segment exceeds its pause limit",
        ));
    }
    let pauses = pause_rows
        .into_iter()
        .map(|(start, end, reason)| {
            Ok(FocusPauseSnapshot {
                started_at_ms: parse_timestamp_ms(&start)?,
                ended_at_ms: end.as_deref().map(parse_timestamp_ms).transpose()?,
                reason,
            })
        })
        .collect::<Result<Vec<_>, FocusExecutionError>>()?;
    let planned_start_ms = parse_timestamp_ms(&planned_start)?;
    let planned_end_ms = parse_timestamp_ms(&planned_end)?;
    Ok(FocusSegmentSnapshot {
        id: row.try_get("id").map_err(|error| error.to_string())?,
        run_id: row.try_get("run_id").map_err(|error| error.to_string())?,
        event_id: row.try_get("event_id").map_err(|error| error.to_string())?,
        event_date: row
            .try_get("event_date")
            .map_err(|error| error.to_string())?,
        phase: phase(&phase_name)?,
        rhythm_position: row
            .try_get("rhythm_position")
            .map_err(|error| error.to_string())?,
        planned_start_ms,
        planned_end_ms,
        actual_start_ms: parse_timestamp_ms(&actual_start)?,
        actual_end_ms: actual_end.as_deref().map(parse_timestamp_ms).transpose()?,
        chosen_duration_ms: row
            .try_get::<Option<i64>, _>("chosen_duration_ms")
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                super::decisions::invalid_state("Focus segment is missing its accepted duration")
            })?,
        status: row.try_get("status").map_err(|error| error.to_string())?,
        end_reason: row
            .try_get("end_reason")
            .map_err(|error| error.to_string())?,
        pauses,
    })
}

/// Measure only accepted active time. Open pauses stop progress, and overlap is rejected.
pub(super) fn segment_elapsed_ms(
    segment: &FocusSegmentSnapshot,
    now_ms: i64,
) -> Result<i64, FocusExecutionError> {
    let end = segment
        .actual_end_ms
        .unwrap_or(now_ms)
        .min(now_ms)
        .max(segment.actual_start_ms);
    let mut previous_end = segment.actual_start_ms;
    let mut paused_ms = 0i64;
    for pause in &segment.pauses {
        if pause.started_at_ms < previous_end
            || pause.started_at_ms < segment.actual_start_ms
            || pause
                .ended_at_ms
                .is_some_and(|ended| ended < pause.started_at_ms)
        {
            return Err(execution_error(
                FocusErrorCode::InvalidState,
                "Canonical Focus pauses overlap or precede their segment",
            ));
        }
        let pause_end = pause
            .ended_at_ms
            .unwrap_or(end)
            .min(end)
            .max(pause.started_at_ms);
        paused_ms = paused_ms.saturating_add(pause_end.saturating_sub(pause.started_at_ms));
        previous_end = pause.ended_at_ms.unwrap_or(i64::MAX);
    }
    Ok(end
        .saturating_sub(segment.actual_start_ms)
        .saturating_sub(paused_ms)
        .max(0))
}

pub(super) async fn load_snapshot(
    tx: &mut Transaction<'_, Sqlite>,
    revision: i64,
    state: &ExecutionState,
    now_ms: i64,
    changed_ids: &[String],
) -> Result<FocusExecutionSnapshot, FocusExecutionError> {
    let run = match &state.run_id {
        Some(id) => Some(load_run(tx, id).await?),
        None => None,
    };
    let segment = match &state.segment_id {
        Some(id) => Some(load_segment(tx, id).await?),
        None => None,
    };
    if segment
        .as_ref()
        .is_some_and(|segment| run.as_ref().is_none_or(|run| segment.run_id != run.id))
    {
        return Err(execution_error(
            FocusErrorCode::InvalidState,
            "Focus execution references a segment owned by another run",
        ));
    }
    let mut changed_segments = Vec::new();
    for id in changed_ids {
        changed_segments.push(load_segment(tx, id).await?);
    }
    let elapsed_ms = segment
        .as_ref()
        .map(|segment| segment_elapsed_ms(segment, now_ms))
        .transpose()?
        .unwrap_or(0);
    let work_remaining_ms = segment
        .as_ref()
        .filter(|segment| segment.status == "active")
        .map(|segment| segment.chosen_duration_ms.saturating_sub(elapsed_ms).max(0))
        .unwrap_or(0);
    let event_end = run.as_ref().map(|run| run.planned_end_ms);
    let remaining_ms = event_end
        .map(|end| work_remaining_ms.min(end.saturating_sub(now_ms).max(0)))
        .unwrap_or(0);
    let phase_deadline_ms =
        (state.mode == FocusMode::Running).then(|| now_ms.saturating_add(remaining_ms));
    let completed_focus_count = if let Some(run) = &run {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM pomodoro_segments WHERE run_id = ? AND phase = 'focus' AND status = 'completed'")
            .bind(&run.id).fetch_one(&mut **tx).await.map_err(|error| format!("Count completed Focus phases: {error}"))?
    } else {
        0
    };
    let effective_idle_timeout_minutes = if state.idle_timeout_override_set {
        state.idle_timeout_override_minutes
    } else {
        run.as_ref()
            .and_then(|run| run.configuration.idle_timeout_minutes)
    };
    Ok(FocusExecutionSnapshot {
        revision,
        observed_at_ms: now_ms,
        mode: state.mode,
        run,
        segment,
        changed_segments,
        phase_deadline_ms,
        remaining_ms,
        elapsed_ms,
        completed_focus_count,
        skip_next_break: state.skip_next_break,
        focus_extension_used: state.focus_extension_used,
        break_extension_ms: state.break_extension_ms,
        dismissed_occurrence_id: state.dismissed_occurrence_id.clone(),
        automatic_admission_suppressed: state.automatic_admission_suppressed,
        paused_prompts_dismissed: state.paused_prompts_dismissed,
        idle_started_at_ms: state.idle_started_at_ms,
        idle_detected_at_ms: state.idle_detected_at_ms,
        idle_overlay_visible_at_ms: state.idle_overlay_visible_at_ms,
        focus_failed_at_ms: state.focus_failed_at_ms,
        suspend_started_at_ms: state.suspend_started_at_ms,
        suspend_returned_at_ms: state.suspend_returned_at_ms,
        return_started_at_ms: state.return_started_at_ms,
        activity_source_unavailable: state.activity_source_unavailable,
        effective_idle_timeout_minutes,
    })
}

/// Read current execution in the caller's authorized transaction without mutating
/// or advancing it.
///
/// A historical command receipt is separate from this projection and cannot replace it.
pub async fn focus_read_execution_snapshot_tx(
    tx: &mut Transaction<'_, Sqlite>,
    now_ms: i64,
) -> Result<FocusExecutionSnapshot, FocusExecutionError> {
    format_timestamp(now_ms)?;
    if now_ms < 0 {
        return Err(execution_error(
            FocusErrorCode::InvalidState,
            "Focus clock precedes the supported epoch",
        ));
    }
    let (revision, state) = load_state(tx).await?;
    load_snapshot(tx, revision, &state, now_ms, &[]).await
}

/// Read the current canonical projection in one consistent read transaction.
pub async fn focus_read_execution_snapshot(
    pool: &SqlitePool,
    now_ms: i64,
) -> Result<FocusExecutionSnapshot, FocusExecutionError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("Begin Focus snapshot: {error}"))?;
    let result = focus_read_execution_snapshot_tx(&mut tx, now_ms).await?;
    tx.commit()
        .await
        .map_err(|error| format!("Commit Focus snapshot: {error}"))?;
    Ok(result)
}
