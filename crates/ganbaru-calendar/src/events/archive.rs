#[cfg(test)]
use super::time::current_utc_iso;
use super::types::{CalendarEventMutationContext, CalendarEventMutationTarget};

pub(crate) async fn archive_or_delete_calendar_events_for_calendar(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    calendar_id: Option<&str>,
    clock: crate::recurrence::canonical::ScopeClock,
) -> Result<(), String> {
    ensure_no_open_runs_for_scope(tx, calendar_id).await?;
    let now = crate::reads::native_window::utc(clock.epoch_ms)?;
    let event_ids = load_event_ids_for_scope(tx, calendar_id).await?;
    for id in event_ids {
        let target = CalendarEventMutationTarget { id };
        let context = load_mutation_context(tx, &target).await?;
        let all_day: bool =
            sqlx::query_scalar("SELECT all_day != 0 FROM calendar_events WHERE id=?")
                .bind(&context.source_event_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(|error| format!("read Calendar removal date kind: {error}"))?;
        let started = if all_day {
            let today = clock
                .floating_today
                .ok_or("Calendar removal requires the native device date")?;
            let start_ms = super::time::calendar_timestamp_millis(&context.start_time)
                .ok_or("Calendar removal requires a valid floating date")?;
            chrono::DateTime::from_timestamp_millis(start_ms)
                .ok_or("Calendar removal floating date is outside its supported range")?
                .date_naive()
                <= today
        } else {
            super::time::calendar_timestamp_millis(&context.start_time)
                .ok_or("Calendar removal requires a valid occurrence start")?
                <= clock.epoch_ms
        };
        if started || has_protected_references(tx, &context).await? {
            archive_loaded_event(tx, &context, &now).await?;
        } else {
            hard_delete_loaded_event(tx, &context, &now).await?;
        }
    }
    Ok(())
}
#[cfg(test)]
pub(super) async fn delete_calendar_event_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    target: &CalendarEventMutationTarget,
) -> Result<(), String> {
    let now = current_utc_iso(tx).await?;
    let context = load_mutation_context(tx, target).await?;
    ensure_no_open_runs_for_event(tx, &context).await?;
    if is_protected_event(tx, &context, &now).await? {
        return Err(format!(
            "calendar event '{}' is protected; archive it instead",
            context.id
        ));
    }
    hard_delete_loaded_event(tx, &context, &now).await
}
#[cfg(test)]
pub(super) async fn archive_calendar_event_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    target: &CalendarEventMutationTarget,
) -> Result<(), String> {
    let now = current_utc_iso(tx).await?;
    let context = load_mutation_context(tx, target).await?;
    ensure_no_open_runs_for_event(tx, &context).await?;
    archive_loaded_event(tx, &context, &now).await
}
pub(super) async fn hard_delete_loaded_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    context: &CalendarEventMutationContext,
    now: &str,
) -> Result<(), String> {
    if context.synthetic {
        let occurrence_date = context
            .occurrence_date
            .as_deref()
            .ok_or_else(|| "synthetic occurrence date is required".to_string())?;
        add_exdate(tx, &context.source_event_id, occurrence_date, now).await?;
        return Ok(());
    }

    sqlx::query(
        "DELETE FROM music_context_assignments
         WHERE owner_id = ? AND owner_kind IN ('event-snapshot', 'event-override')",
    )
    .bind(&context.source_event_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("delete calendar music assignments: {e}"))?;
    sqlx::query("DELETE FROM calendar_events WHERE id = ?")
        .bind(&context.source_event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("delete calendar event: {e}"))?;
    Ok(())
}
pub(super) async fn archive_loaded_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    context: &CalendarEventMutationContext,
    archived_at: &str,
) -> Result<(), String> {
    archive_event_snapshot(tx, context, archived_at).await?;
    if context.synthetic {
        let occurrence_date = context
            .occurrence_date
            .as_deref()
            .ok_or_else(|| "synthetic occurrence date is required".to_string())?;
        add_exdate(tx, &context.source_event_id, occurrence_date, archived_at).await?;
        null_pomodoro_live_refs_for_synthetic(
            tx,
            &context.source_event_id,
            &context.canonical_id,
            occurrence_date,
        )
        .await?;
        return Ok(());
    }

    null_pomodoro_live_refs_for_event(tx, &context.source_event_id).await?;
    sqlx::query("DELETE FROM music_context_assignments WHERE owner_id=? AND owner_kind IN ('event-snapshot', 'event-override')")
        .bind(&context.source_event_id).execute(&mut **tx).await.map_err(|error| format!("clear archived live Music assignments: {error}"))?;
    sqlx::query("DELETE FROM calendar_events WHERE id = ?")
        .bind(&context.source_event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("delete archived calendar event: {e}"))?;
    Ok(())
}
pub(super) async fn load_mutation_context(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    target: &CalendarEventMutationTarget,
) -> Result<CalendarEventMutationContext, String> {
    super::occurrence::load_context(tx, &target.id).await
}
#[cfg(test)]
pub(super) async fn is_protected_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    context: &CalendarEventMutationContext,
    now: &str,
) -> Result<bool, String> {
    let start_ms = super::time::calendar_timestamp_millis(&context.start_time)
        .ok_or("Calendar protection requires a valid occurrence start")?;
    let now_ms = super::time::calendar_timestamp_millis(now)
        .ok_or("Calendar protection requires a valid native clock")?;
    if start_ms <= now_ms {
        return Ok(true);
    }
    has_protected_references(tx, context).await
}

async fn has_protected_references(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    context: &CalendarEventMutationContext,
) -> Result<bool, String> {
    let referenced: bool = sqlx::query_scalar("SELECT
        EXISTS(SELECT 1 FROM project_task_event_links WHERE event_id=?1)
        OR EXISTS(SELECT 1 FROM calendar_events WHERE id=?1 AND icalendar_component_id IS NOT NULL)
        OR EXISTS(SELECT 1 FROM calendar_event_alarms WHERE event_id=?1 AND icalendar_component_id IS NOT NULL)
        OR EXISTS(SELECT 1 FROM calendar_event_attendees WHERE event_id=?1 AND icalendar_component_id IS NOT NULL)
        OR EXISTS(SELECT 1 FROM calendar_event_overrides WHERE parent_event_id=?1 AND icalendar_component_id IS NOT NULL)")
        .bind(&context.source_event_id).fetch_one(&mut **tx).await
        .map_err(|error| format!("check protected Calendar references: {error}"))?;
    if referenced {
        return Ok(true);
    }
    event_has_pomodoro_history(tx, context).await
}
pub(super) async fn event_has_pomodoro_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    context: &CalendarEventMutationContext,
) -> Result<bool, String> {
    // Every segment owns a run through a non-null foreign key. Run identity is
    // sufficient evidence, and EXISTS avoids counting an entire execution log.
    if context.synthetic {
        let date = context
            .occurrence_date
            .as_deref()
            .ok_or("Calendar occurrence date is missing")?;
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pomodoro_runs
            WHERE COALESCE(current_occurrence_id, original_event_id) = ?1 OR original_event_id = ?1
                OR (current_occurrence_id IS NULL AND original_event_id = ?2 AND event_date = ?3))",
        )
        .bind(&context.canonical_id)
        .bind(&context.source_event_id)
        .bind(date)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| format!("read Calendar occurrence history: {error}"))
    } else {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pomodoro_runs WHERE event_id = ?1
            OR original_event_id = ?1 OR (original_event_id >= ?1 || '::' AND original_event_id < ?1 || ':;'))")
            .bind(&context.source_event_id).fetch_one(&mut **tx).await
            .map_err(|error| format!("read Calendar source history: {error}"))
    }
}
#[cfg(test)]
pub(super) async fn ensure_no_open_runs_for_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    context: &CalendarEventMutationContext,
) -> Result<(), String> {
    if event_has_open_pomodoro_run(tx, context).await? {
        return Err(format!(
            "calendar event '{}' has an active pomodoro run; stop it before deleting or archiving",
            context.id
        ));
    }
    Ok(())
}
#[cfg(test)]
pub(super) async fn event_has_open_pomodoro_run(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    context: &CalendarEventMutationContext,
) -> Result<bool, String> {
    let count: i64 = if context.synthetic {
        let occurrence_date = context
            .occurrence_date
            .as_deref()
            .ok_or_else(|| "synthetic occurrence date is required".to_string())?;
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM pomodoro_runs
             WHERE ended_at IS NULL
               AND event_id = ?
               AND (COALESCE(current_occurrence_id, original_event_id) = ?2
                    OR (current_occurrence_id IS NULL AND original_event_id = ?1 AND event_date = ?3))",
        )
        .bind(&context.source_event_id)
        .bind(&context.canonical_id)
        .bind(occurrence_date)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("count active synthetic pomodoro runs: {e}"))?
    } else {
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM pomodoro_runs
             WHERE ended_at IS NULL
               AND (event_id = ? OR original_event_id = ?)",
        )
        .bind(&context.source_event_id)
        .bind(&context.id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("count active pomodoro runs: {e}"))?
    };
    Ok(count > 0)
}
pub(super) async fn ensure_no_open_runs_for_scope(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    calendar_id: Option<&str>,
) -> Result<(), String> {
    let count: i64 = if let Some(calendar_id) = calendar_id {
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM pomodoro_runs
             WHERE ended_at IS NULL
               AND event_id IN (
                    SELECT id FROM calendar_events WHERE calendar_id = ?
               )",
        )
        .bind(calendar_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("count active pomodoro runs for calendar: {e}"))?
    } else {
        sqlx::query_scalar("SELECT COUNT(*) FROM pomodoro_runs WHERE ended_at IS NULL")
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("count active pomodoro runs: {e}"))?
    };
    if count > 0 {
        return Err(
            "active pomodoro runs must be stopped before clearing or removing calendar events"
                .to_string(),
        );
    }
    Ok(())
}
pub(super) async fn load_event_ids_for_scope(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    calendar_id: Option<&str>,
) -> Result<Vec<String>, String> {
    if let Some(calendar_id) = calendar_id {
        sqlx::query_scalar(
            "SELECT id FROM calendar_events WHERE calendar_id = ? ORDER BY start_time ASC",
        )
        .bind(calendar_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| format!("load calendar event ids: {e}"))
    } else {
        sqlx::query_scalar("SELECT id FROM calendar_events ORDER BY start_time ASC")
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| format!("load calendar event ids: {e}"))
    }
}
pub(super) async fn archive_event_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    context: &CalendarEventMutationContext,
    archived_at: &str,
) -> Result<(), String> {
    use super::metadata::Metadata;
    use super::occurrence::ReadBudget;
    use super::scope::{SCOPE_GATE, SCOPE_WORKER_TIMEOUT};

    let permit = SCOPE_GATE
        .try_acquire()
        .map_err(|_| "A Calendar archive is being prepared; retry after it finishes")?;
    let metadata = Metadata::read(tx, &context.source_event_id, &mut ReadBudget::default()).await?;
    let context = CalendarEventMutationContext {
        id: context.id.clone(),
        canonical_id: context.canonical_id.clone(),
        source_event_id: context.source_event_id.clone(),
        occurrence_date: context.occurrence_date.clone(),
        start_time: context.start_time.clone(),
        end_time: context.end_time.clone(),
        rrule: context.rrule.clone(),
        repeat_until: context.repeat_until.clone(),
        synthetic: context.synthetic,
    };
    let worker = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        metadata.prepare_archive(&context.id, &context)
    });
    let prepared = tokio::time::timeout(SCOPE_WORKER_TIMEOUT, worker)
        .await
        .map_err(|_| "Calendar archive preparation timed out")?
        .map_err(|error| format!("Calendar archive worker: {error}"))??;
    prepared.write(tx, archived_at).await
}
pub(super) async fn add_exdate(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event_id: &str,
    occurrence_date: &str,
    updated_at: &str,
) -> Result<(), String> {
    super::metadata::add_exdate(tx, event_id, occurrence_date).await?;
    sqlx::query("UPDATE calendar_events SET updated_at = ? WHERE id = ?")
        .bind(updated_at)
        .bind(event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("touch recurring parent: {e}"))?;
    Ok(())
}
pub(super) async fn null_pomodoro_live_refs_for_synthetic(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source_event_id: &str,
    exact_event_id: &str,
    occurrence_date: &str,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE pomodoro_runs
         SET event_id = NULL
         WHERE event_id = ?
           AND (COALESCE(current_occurrence_id, original_event_id) = ?2
                OR (current_occurrence_id IS NULL AND original_event_id = ?1 AND event_date = ?3))",
    )
    .bind(source_event_id)
    .bind(exact_event_id)
    .bind(occurrence_date)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("clear archived synthetic run references: {e}"))?;

    sqlx::query(
        "UPDATE pomodoro_segments
         SET event_id = NULL
         WHERE event_id = ?1 AND run_id IN (SELECT id FROM pomodoro_runs
             WHERE event_id IS NULL AND (COALESCE(current_occurrence_id, original_event_id) = ?2
                 OR (current_occurrence_id IS NULL AND original_event_id = ?1 AND event_date = ?3)))",
    )
    .bind(source_event_id)
    .bind(exact_event_id)
    .bind(occurrence_date)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("clear archived synthetic segment references: {e}"))?;
    Ok(())
}
pub(super) async fn null_pomodoro_live_refs_for_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event_id: &str,
) -> Result<(), String> {
    sqlx::query("UPDATE pomodoro_runs SET event_id = NULL WHERE event_id = ?")
        .bind(event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear archived run references: {e}"))?;
    sqlx::query("UPDATE pomodoro_segments SET event_id = NULL WHERE event_id = ?")
        .bind(event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear archived segment references: {e}"))?;
    Ok(())
}
