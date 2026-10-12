use super::*;

pub(crate) async fn normalized_close_run_ended_at(
    tx: &mut Transaction<'_, Sqlite>,
    closure: &PomodoroRunClosure,
) -> Result<String, String> {
    let latest_boundary = sqlx::query_scalar::<_, String>(
        "SELECT boundary_at
         FROM (
             SELECT actual_start AS boundary_at
             FROM pomodoro_segments
             WHERE run_id = ? AND status = 'active'
             UNION ALL
             SELECT p.started_at AS boundary_at
             FROM pomodoro_pauses p
             JOIN pomodoro_segments s ON s.id = p.segment_id
             WHERE s.run_id = ? AND p.ended_at IS NULL
         )
         ORDER BY julianday(boundary_at) DESC
         LIMIT 1",
    )
    .bind(&closure.run_id)
    .bind(&closure.run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load active pomodoro segment start: {e}"))?;

    Ok(latest_boundary
        .filter(|start| iso_is_before(&closure.ended_at, start))
        .unwrap_or_else(|| closure.ended_at.clone()))
}

pub(crate) async fn replace_segment_pauses(
    tx: &mut Transaction<'_, Sqlite>,
    segment_id: &str,
    pauses: &[PomodoroPauseWrite],
) -> Result<(), String> {
    sqlx::query("DELETE FROM pomodoro_pauses WHERE segment_id = ?")
        .bind(segment_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear pomodoro pauses: {e}"))?;

    for (index, pause) in pauses.iter().enumerate() {
        validate_pause(pause)?;
        sqlx::query(
            "INSERT INTO pomodoro_pauses
                (id, segment_id, started_at, ended_at, reason, detected_at)
             VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, ?)",
        )
        .bind(segment_id)
        .bind(&pause.started_at)
        .bind(&pause.ended_at)
        .bind(&pause.reason)
        .bind(if pause.reason == "idle" {
            Some(pause.started_at.as_str())
        } else {
            None
        })
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert pomodoro pause {index}: {e}"))?;
    }
    Ok(())
}

pub(crate) async fn insert_segment_pause_events(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    segment: &PomodoroSegmentWrite,
) -> Result<(), String> {
    for pause in &segment.pauses {
        insert_pause_start_events(tx, run_id, &segment.id, &segment.phase, pause).await?;
        if let Some(ended_at) = &pause.ended_at {
            insert_run_event_tx(
                tx,
                RunEventInsert {
                    run_id,
                    segment_id: Some(&segment.id),
                    event_type: "pause_end",
                    occurred_at: ended_at,
                    phase: Some(&segment.phase),
                    reason: Some(&pause.reason),
                    duration_seconds: None,
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn insert_pause_start_events(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    segment_id: &str,
    phase: &str,
    pause: &PomodoroPauseWrite,
) -> Result<(), String> {
    insert_run_event_tx(
        tx,
        RunEventInsert {
            run_id,
            segment_id: Some(segment_id),
            event_type: "pause_start",
            occurred_at: &pause.started_at,
            phase: Some(phase),
            reason: Some(&pause.reason),
            duration_seconds: None,
        },
    )
    .await?;
    if pause.reason == "idle" {
        insert_run_event_tx(
            tx,
            RunEventInsert {
                run_id,
                segment_id: Some(segment_id),
                event_type: "idle_detected",
                occurred_at: &pause.started_at,
                phase: Some(phase),
                reason: Some(&pause.reason),
                duration_seconds: None,
            },
        )
        .await?;
    }
    if pause.reason == "suspend" {
        insert_run_event_tx(
            tx,
            RunEventInsert {
                run_id,
                segment_id: Some(segment_id),
                event_type: "suspend_detected",
                occurred_at: &pause.started_at,
                phase: Some(phase),
                reason: Some(&pause.reason),
                duration_seconds: None,
            },
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn insert_run_event_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event: RunEventInsert<'_>,
) -> Result<(), String> {
    validate_event_type(event.event_type)?;
    if let Some(phase) = event.phase {
        validate_phase(phase)?;
    }
    sqlx::query(
        "INSERT INTO pomodoro_run_events
            (id, run_id, segment_id, event_type, occurred_at, phase, reason, duration_seconds)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(event.run_id)
    .bind(event.segment_id)
    .bind(event.event_type)
    .bind(event.occurred_at)
    .bind(event.phase)
    .bind(event.reason)
    .bind(event.duration_seconds)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert pomodoro run event: {e}"))?;
    Ok(())
}
