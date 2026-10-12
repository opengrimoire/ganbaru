//! Calendar-owned reference changes share the execution revision and transaction.

use chrono::{Datelike, NaiveDate};
use sqlx::{Sqlite, Transaction};

use super::models::*;
use super::mutations::Session;
use super::persistence::*;

/// Native plan output, never a deserializable WebView mutation command.
pub struct FocusCalendarReferenceChange {
    pub run_id: String,
    pub expected_occurrence_id: String,
    pub target_event_id: String,
    pub target_occurrence_id: String,
    pub target_event_date: String,
}

/// A native Calendar end-now result. Geometry comes from the prepared Calendar
/// mutation and the owner's acceptance clock, never from WebView execution data.
pub struct FocusCalendarCompletion {
    pub reference: FocusCalendarReferenceChange,
    pub start_ms: i64,
    pub end_ms: i64,
}

enum ReferenceAction {
    Reconcile,
    Complete { start_ms: i64, end_ms: i64 },
}

/// End the exact owned run at the native Calendar cutoff in the caller's atomic
/// transaction. Configuration removal cannot turn explicit completion into stop.
pub async fn focus_complete_calendar_tx(
    tx: &mut Transaction<'_, Sqlite>,
    expected_revision: i64,
    completion: &FocusCalendarCompletion,
    context: &FocusExecutionContext,
) -> Result<FocusExecutionSnapshot, FocusExecutionError> {
    apply_reference(
        tx,
        expected_revision,
        &completion.reference,
        context,
        ReferenceAction::Complete {
            start_ms: completion.start_ms,
            end_ms: completion.end_ms,
        },
    )
    .await
}

/// Retarget exactly the currently owned run, then reconcile its canonical
/// configuration/deadline in the caller's Calendar transaction. Original run
/// identity, date and title, completed phases, pauses and elapsed evidence remain
/// intact. The caller owns retry receipts, rollback and post-commit publication.
pub async fn focus_retarget_calendar_tx(
    tx: &mut Transaction<'_, Sqlite>,
    expected_revision: i64,
    change: &FocusCalendarReferenceChange,
    context: &FocusExecutionContext,
) -> Result<FocusExecutionSnapshot, FocusExecutionError> {
    apply_reference(
        tx,
        expected_revision,
        change,
        context,
        ReferenceAction::Reconcile,
    )
    .await
}

async fn apply_reference(
    tx: &mut Transaction<'_, Sqlite>,
    expected_revision: i64,
    change: &FocusCalendarReferenceChange,
    context: &FocusExecutionContext,
    action: ReferenceAction,
) -> Result<FocusExecutionSnapshot, FocusExecutionError> {
    validate_change(change)?;
    format_timestamp(context.now_ms)?;
    let (revision, mut state) = load_state(tx).await?;
    if revision != expected_revision || state.run_id.as_deref() != Some(&change.run_id) {
        return Err(stale(revision));
    }
    let run = load_run(tx, &change.run_id).await?;
    if run.ended_at_ms.is_some() || run.occurrence_id != change.expected_occurrence_id {
        return Err(stale(revision));
    }
    if context.now_ms < run.started_at_ms
        || context.now_ms < state.last_transition_at_ms
        || context.now_ms >= run.planned_end_ms
    {
        return Err(execution_error(
            FocusErrorCode::IneligibleCommitment,
            "Refresh expired or clock-inconsistent Focus execution before changing its Calendar reference",
        ));
    }
    if let ReferenceAction::Complete { start_ms, end_ms } = action {
        if start_ms != run.planned_start_ms || end_ms != context.now_ms || end_ms <= start_ms {
            return Err(execution_error(
                FocusErrorCode::IneligibleCommitment,
                "Calendar completion must retain the recorded start and use the native acceptance clock",
            ));
        }
    } else if let Some(target) = &context.commitment {
        if target.event_id != change.target_event_id
            || target.occurrence_id != change.target_occurrence_id
            || target.event_date != change.target_event_date
            || target.start_ms != run.planned_start_ms
            || target.end_ms < context.now_ms
        {
            return Err(execution_error(
                FocusErrorCode::IneligibleCommitment,
                "Calendar retarget does not match the canonical commitment or recorded start",
            ));
        }
    }
    let next_revision = reserve_revision(tx, revision).await?;
    let changed = sqlx::query(
        "UPDATE pomodoro_runs SET event_id = ?1, current_occurrence_id = ?2,
            current_event_date = ?3,
            current_event_title = (SELECT title FROM calendar_events WHERE id = ?1)
         WHERE id = ?4 AND ended_at IS NULL
            AND COALESCE(current_occurrence_id, original_event_id) = ?5",
    )
    .bind(&change.target_event_id)
    .bind(&change.target_occurrence_id)
    .bind(&change.target_event_date)
    .bind(&change.run_id)
    .bind(&change.expected_occurrence_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| format!("Retarget current Focus occurrence: {error}"))?;
    if changed.rows_affected() != 1 {
        return Err(stale(revision));
    }
    let mut changed_segments = Vec::new();
    if let Some(segment_id) = &state.segment_id {
        let changed = sqlx::query(
            "UPDATE pomodoro_segments SET event_id = ? WHERE id = ? AND run_id = ?
             AND status = 'active' AND actual_end IS NULL",
        )
        .bind(&change.target_event_id)
        .bind(segment_id)
        .bind(&change.run_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| format!("Retarget accepted Focus phase: {error}"))?;
        if changed.rows_affected() == 1 {
            changed_segments.push(segment_id.clone());
        }
    }
    if state.dismissed_occurrence_id.as_deref() == Some(&change.expected_occurrence_id) {
        state.dismissed_occurrence_id = Some(change.target_occurrence_id.clone());
    }
    let mut session = Session::load(tx, state, context).await?;
    session.changed_segments = changed_segments;
    session
        .record_event(
            tx,
            "reconfigure",
            context.now_ms,
            Some("calendar_reference_changed"),
            None,
        )
        .await?;
    match action {
        ReferenceAction::Reconcile => {
            session.reconcile_calendar(tx, context).await?;
        }
        ReferenceAction::Complete { end_ms, .. } => {
            sqlx::query(
                "UPDATE pomodoro_runs SET planned_end = ? WHERE id = ? AND ended_at IS NULL",
            )
            .bind(format_timestamp(end_ms)?)
            .bind(&change.run_id)
            .execute(&mut **tx)
            .await
            .map_err(|error| format!("Apply native Calendar completion deadline: {error}"))?;
            session
                .run
                .as_mut()
                .ok_or_else(|| stale(revision))?
                .planned_end_ms = end_ms;
            if !session.expire(tx, end_ms).await? {
                return Err(execution_error(
                    FocusErrorCode::InvalidState,
                    "Calendar completion did not close the owned Focus run",
                ));
            }
        }
    }
    session.state.last_transition_at_ms = session.state.last_transition_at_ms.max(context.now_ms);
    save_state(tx, &session.state, next_revision, context.now_ms).await?;
    load_snapshot(
        tx,
        next_revision,
        &session.state,
        context.now_ms,
        &session.changed_segments,
    )
    .await
}

fn stale(revision: i64) -> FocusExecutionError {
    FocusExecutionError {
        code: FocusErrorCode::StaleRevision,
        message: "The owned Focus run changed before the Calendar edit was accepted".into(),
        current_revision: Some(revision),
    }
}

fn validate_change(change: &FocusCalendarReferenceChange) -> Result<(), FocusExecutionError> {
    const MAX_ID_BYTES: usize = 1024;
    const OCCURRENCE_SUFFIX_BYTES: usize = 12;
    for (id, limit) in [
        (&change.run_id, MAX_ID_BYTES),
        (
            &change.expected_occurrence_id,
            MAX_ID_BYTES + OCCURRENCE_SUFFIX_BYTES,
        ),
        (&change.target_event_id, MAX_ID_BYTES),
        (
            &change.target_occurrence_id,
            MAX_ID_BYTES + OCCURRENCE_SUFFIX_BYTES,
        ),
    ] {
        if id.trim().is_empty() || id.len() > limit {
            return Err(execution_error(
                FocusErrorCode::InvalidState,
                "Calendar Focus reference is empty or exceeds its identity budget",
            ));
        }
    }
    let date = NaiveDate::parse_from_str(&change.target_event_date, "%Y-%m-%d").map_err(|_| {
        execution_error(
            FocusErrorCode::InvalidState,
            "Calendar Focus reference requires an original recurrence date",
        )
    })?;
    if change.target_event_date.len() != 10
        || !(1..=9999).contains(&date.year())
        || change.target_event_id.contains("::")
    {
        return Err(execution_error(
            FocusErrorCode::InvalidState,
            "Calendar Focus reference has inconsistent occurrence identity",
        ));
    }
    if change.target_occurrence_id != change.target_event_id {
        let (root, date) = change
            .target_occurrence_id
            .split_once("::")
            .ok_or_else(|| {
                execution_error(
                    FocusErrorCode::InvalidState,
                    "Calendar Focus reference has no original recurrence identity",
                )
            })?;
        let valid_date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .is_ok_and(|value| date.len() == 10 && (1..=9999).contains(&value.year()));
        if root != change.target_event_id || !valid_date {
            return Err(execution_error(
                FocusErrorCode::InvalidState,
                "Calendar Focus reference has inconsistent occurrence identity",
            ));
        }
    }
    Ok(())
}
