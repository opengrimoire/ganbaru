use super::*;

pub(crate) async fn capture_adaptive_planned_blocks_for_day_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    event_date: &str,
    captured_at: &str,
) -> Result<(), String> {
    let blocks = load_calendar_adaptive_planned_blocks_tx(tx, event_date).await?;
    for block in blocks {
        insert_adaptive_planned_block_tx(tx, run_id, event_date, captured_at, &block).await?;
    }
    Ok(())
}

pub(crate) async fn insert_adaptive_planned_blocks_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    event_date: &str,
    captured_at: &str,
    blocks: &[PomodoroAdaptivePlannedBlockWrite],
) -> Result<(), String> {
    for block in blocks {
        let planned_block = AdaptivePlannedPomodoroBlock {
            event_id: block.event_id.clone(),
            original_event_id: block.original_event_id.clone(),
            planned_start: block.planned_start.clone(),
            planned_end: block.planned_end.clone(),
            source_kind: block.source_kind.clone(),
        };
        insert_adaptive_planned_block_tx(tx, run_id, event_date, captured_at, &planned_block)
            .await?;
    }
    Ok(())
}

async fn insert_adaptive_planned_block_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    event_date: &str,
    captured_at: &str,
    block: &AdaptivePlannedPomodoroBlock,
) -> Result<(), String> {
    sqlx::query(
        "INSERT OR IGNORE INTO pomodoro_adaptive_planned_blocks
            (id, capture_run_id, event_date, event_id, original_event_id,
             planned_start, planned_end, source_kind, captured_at)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(run_id)
    .bind(event_date)
    .bind(&block.event_id)
    .bind(&block.original_event_id)
    .bind(&block.planned_start)
    .bind(&block.planned_end)
    .bind(&block.source_kind)
    .bind(captured_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("capture adaptive planned pomodoro block: {e}"))?;
    Ok(())
}

pub(crate) async fn record_matured_adaptive_day_outcomes_tx(
    tx: &mut Transaction<'_, Sqlite>,
    current_run_id: &str,
    measured_at: &str,
) -> Result<(), String> {
    let Some(current_event_date) = load_run_event_date_tx(tx, current_run_id).await? else {
        return Ok(());
    };
    let decisions = load_matured_day_decisions_tx(tx, &current_event_date).await?;
    for decision in decisions {
        if decision.event_date >= current_event_date {
            continue;
        }
        let summary = load_adaptive_day_outcome_summary_tx(tx, &decision.event_date).await?;
        insert_adaptive_day_outcomes_tx(tx, &decision.decision_id, measured_at, &summary).await?;
    }
    Ok(())
}

pub(crate) async fn record_matured_adaptive_next_day_outcomes_tx(
    tx: &mut Transaction<'_, Sqlite>,
    current_run_id: &str,
    measured_at: &str,
) -> Result<(), String> {
    let Some(current_event_date) = load_run_event_date_tx(tx, current_run_id).await? else {
        return Ok(());
    };
    let decisions = load_matured_next_day_decisions_tx(tx, &current_event_date).await?;
    for decision in decisions {
        let Some(next_event_date) = next_event_date(&decision.event_date) else {
            continue;
        };
        if next_event_date >= current_event_date {
            continue;
        }
        let summary = load_adaptive_next_day_outcome_summary_tx(tx, &next_event_date).await?;
        insert_adaptive_next_day_outcomes_tx(tx, &decision.decision_id, measured_at, &summary)
            .await?;
    }
    Ok(())
}

async fn load_run_event_date_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
) -> Result<Option<String>, String> {
    sqlx::query_scalar("SELECT event_date FROM pomodoro_runs WHERE id = ?")
        .bind(run_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load adaptive run event date: {e}"))
}

async fn load_matured_day_decisions_tx(
    tx: &mut Transaction<'_, Sqlite>,
    current_event_date: &str,
) -> Result<Vec<AdaptiveDecisionForDayOutcome>, String> {
    let rows = sqlx::query(
        "SELECT d.id AS decision_id, r.event_date AS event_date
         FROM pomodoro_run_adaptive_snapshots rs
         JOIN pomodoro_adaptive_decisions d ON d.id = rs.decision_id
         JOIN pomodoro_runs r ON r.id = rs.run_id
         WHERE r.ended_at IS NOT NULL
           AND r.event_date < ?
           AND NOT EXISTS (
             SELECT 1
             FROM pomodoro_adaptive_outcomes o
             WHERE o.decision_id = d.id
               AND o.outcome_window = 'day'
               AND o.outcome_key = 'day_observed'
           )
         ORDER BY r.event_date ASC, d.occurred_at ASC
         LIMIT 50",
    )
    .bind(current_event_date)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load matured adaptive day decisions: {e}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(AdaptiveDecisionForDayOutcome {
                decision_id: row
                    .try_get("decision_id")
                    .map_err(|e| format!("read day decision id: {e}"))?,
                event_date: row
                    .try_get("event_date")
                    .map_err(|e| format!("read day event date: {e}"))?,
            })
        })
        .collect()
}

async fn load_matured_next_day_decisions_tx(
    tx: &mut Transaction<'_, Sqlite>,
    current_event_date: &str,
) -> Result<Vec<AdaptiveDecisionForNextDayOutcome>, String> {
    let rows = sqlx::query(
        "SELECT d.id AS decision_id, r.event_date AS event_date
         FROM pomodoro_run_adaptive_snapshots rs
         JOIN pomodoro_adaptive_decisions d ON d.id = rs.decision_id
         JOIN pomodoro_runs r ON r.id = rs.run_id
         WHERE r.ended_at IS NOT NULL
           AND r.event_date < ?
           AND NOT EXISTS (
             SELECT 1
             FROM pomodoro_adaptive_outcomes o
             WHERE o.decision_id = d.id
               AND o.outcome_window = 'next_day'
               AND o.outcome_key = 'next_day_observed'
           )
         ORDER BY r.event_date ASC, d.occurred_at ASC
         LIMIT 50",
    )
    .bind(current_event_date)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load matured adaptive next-day decisions: {e}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(AdaptiveDecisionForNextDayOutcome {
                decision_id: row
                    .try_get("decision_id")
                    .map_err(|e| format!("read next-day decision id: {e}"))?,
                event_date: row
                    .try_get("event_date")
                    .map_err(|e| format!("read next-day event date: {e}"))?,
            })
        })
        .collect()
}

async fn load_adaptive_day_outcome_summary_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event_date: &str,
) -> Result<AdaptiveDayOutcomeSummary, String> {
    let mut summary = AdaptiveDayOutcomeSummary::default();
    add_run_outcomes_for_date_tx(tx, event_date, &mut summary).await?;
    add_planned_pomodoro_day_outcomes_tx(tx, event_date, &mut summary).await?;
    Ok(summary)
}

async fn load_adaptive_next_day_outcome_summary_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event_date: &str,
) -> Result<AdaptiveNextDayOutcomeSummary, String> {
    let mut summary = AdaptiveNextDayOutcomeSummary::default();
    add_run_outcomes_for_date_tx(tx, event_date, &mut summary).await?;
    Ok(summary)
}

trait AdaptiveDailyRunOutcomeAccumulator {
    fn add_run_outcome(
        &mut self,
        end_reason: Option<&str>,
        run_summary: &AdaptiveRunOutcomeSummary,
    );
}

impl AdaptiveDailyRunOutcomeAccumulator for AdaptiveDayOutcomeSummary {
    fn add_run_outcome(
        &mut self,
        end_reason: Option<&str>,
        run_summary: &AdaptiveRunOutcomeSummary,
    ) {
        self.run_count += 1;
        if end_reason == Some("completed") {
            self.completed_run_count += 1;
        }
        if end_reason == Some("stopped") {
            self.stopped_run_count += 1;
        }
        self.clean_focus_seconds += run_summary.clean_focus_seconds;
        self.completed_focus_segments += run_summary.completed_focus_segments;
        self.interrupted_focus_segments += run_summary.interrupted_focus_segments;
        self.focus_failure_count += run_summary.focus_failure_count;
        self.break_skipped_count += run_summary.break_skipped_count;
        self.break_overtime_seconds +=
            run_summary.short_break_overtime_seconds + run_summary.long_break_overtime_seconds;
        self.blocked_attempt_count += run_summary.blocked_attempt_count;
    }
}

impl AdaptiveDailyRunOutcomeAccumulator for AdaptiveNextDayOutcomeSummary {
    fn add_run_outcome(
        &mut self,
        end_reason: Option<&str>,
        run_summary: &AdaptiveRunOutcomeSummary,
    ) {
        self.run_count += 1;
        if end_reason == Some("completed") {
            self.completed_run_count += 1;
        }
        if end_reason == Some("stopped") {
            self.stopped_run_count += 1;
        }
        self.clean_focus_seconds += run_summary.clean_focus_seconds;
        self.completed_focus_segments += run_summary.completed_focus_segments;
        self.interrupted_focus_segments += run_summary.interrupted_focus_segments;
        self.focus_failure_count += run_summary.focus_failure_count;
        self.break_skipped_count += run_summary.break_skipped_count;
        self.break_overtime_seconds +=
            run_summary.short_break_overtime_seconds + run_summary.long_break_overtime_seconds;
        self.blocked_attempt_count += run_summary.blocked_attempt_count;
    }
}

async fn add_run_outcomes_for_date_tx<T>(
    tx: &mut Transaction<'_, Sqlite>,
    event_date: &str,
    summary: &mut T,
) -> Result<(), String>
where
    T: AdaptiveDailyRunOutcomeAccumulator,
{
    let rows = sqlx::query(
        "SELECT id, ended_at, end_reason
         FROM pomodoro_runs
         WHERE event_date = ?
           AND ended_at IS NOT NULL
         ORDER BY started_at ASC",
    )
    .bind(event_date)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load adaptive daily outcome runs: {e}"))?;
    for row in rows {
        let run_id: String = row
            .try_get("id")
            .map_err(|e| format!("read daily outcome run id: {e}"))?;
        let ended_at: String = row
            .try_get("ended_at")
            .map_err(|e| format!("read daily outcome run ended_at: {e}"))?;
        let end_reason: Option<String> = row
            .try_get("end_reason")
            .map_err(|e| format!("read daily outcome run end_reason: {e}"))?;
        let run_summary = load_adaptive_run_outcome_summary(tx, &run_id, &ended_at).await?;

        summary.add_run_outcome(end_reason.as_deref(), &run_summary);
    }
    Ok(())
}

async fn add_planned_pomodoro_day_outcomes_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event_date: &str,
    summary: &mut AdaptiveDayOutcomeSummary,
) -> Result<(), String> {
    let mut planned_blocks = load_captured_adaptive_planned_blocks_tx(tx, event_date).await?;
    if planned_blocks.is_empty() {
        planned_blocks = load_calendar_adaptive_planned_blocks_tx(tx, event_date).await?;
    }

    for block in planned_blocks {
        let planned_seconds = iso_seconds_between(&block.planned_start, &block.planned_end)
            .unwrap_or(0)
            .max(0);
        let synthetic_original_event_id = format!("{}::{event_date}", block.original_event_id);
        let started_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM pomodoro_runs
             WHERE event_date = ?
               AND (
                 event_id = ?
                 OR original_event_id = ?
                 OR original_event_id = ?
               )",
        )
        .bind(event_date)
        .bind(&block.event_id)
        .bind(&block.original_event_id)
        .bind(&synthetic_original_event_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("count adaptive planned day started runs: {e}"))?;
        summary.planned_pomodoro_event_count += 1;
        summary.planned_pomodoro_minutes += planned_seconds / 60;
        if started_count > 0 {
            summary.started_planned_pomodoro_event_count += 1;
        } else {
            summary.missed_planned_pomodoro_event_count += 1;
        }
    }
    Ok(())
}

async fn load_captured_adaptive_planned_blocks_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event_date: &str,
) -> Result<Vec<AdaptivePlannedPomodoroBlock>, String> {
    let rows = sqlx::query(
        "SELECT event_id, original_event_id, planned_start, planned_end, source_kind
         FROM pomodoro_adaptive_planned_blocks
         WHERE event_date = ?
         ORDER BY planned_start ASC",
    )
    .bind(event_date)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load captured adaptive planned pomodoro blocks: {e}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(AdaptivePlannedPomodoroBlock {
                event_id: row
                    .try_get("event_id")
                    .map_err(|e| format!("read captured planned block event_id: {e}"))?,
                original_event_id: row
                    .try_get("original_event_id")
                    .map_err(|e| format!("read captured planned block original_event_id: {e}"))?,
                planned_start: row
                    .try_get("planned_start")
                    .map_err(|e| format!("read captured planned block planned_start: {e}"))?,
                planned_end: row
                    .try_get("planned_end")
                    .map_err(|e| format!("read captured planned block planned_end: {e}"))?,
                source_kind: row
                    .try_get("source_kind")
                    .map_err(|e| format!("read captured planned block source_kind: {e}"))?,
            })
        })
        .collect()
}

async fn load_calendar_adaptive_planned_blocks_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event_date: &str,
) -> Result<Vec<AdaptivePlannedPomodoroBlock>, String> {
    let rows = sqlx::query(
        "SELECT ce.id AS event_id,
                ce.id AS original_event_id,
                ce.start_time AS start_time,
                ce.end_time AS end_time,
                'live_event' AS source_kind
         FROM calendar_events ce
         JOIN calendar_event_pomodoro_configs pc ON pc.event_id = ce.id
         WHERE ce.all_day = 0
           AND ce.status != 'cancelled'
           AND substr(ce.start_time, 1, 10) = ?
         UNION
         SELECT cea.source_event_id AS event_id,
                cea.source_event_id AS original_event_id,
                cea.start_time AS start_time,
                cea.end_time AS end_time,
                'archived_event' AS source_kind
         FROM calendar_event_archives cea
         JOIN calendar_event_archive_pomodoro_configs pc
           ON pc.archive_event_id = cea.id
         WHERE cea.all_day = 0
           AND cea.status != 'cancelled'
           AND substr(cea.start_time, 1, 10) = ?",
    )
    .bind(event_date)
    .bind(event_date)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load adaptive planned pomodoro day events: {e}"))?;

    let mut blocks_by_key = HashMap::<String, AdaptivePlannedPomodoroBlock>::new();
    for row in rows {
        let event_id: String = row
            .try_get("event_id")
            .map_err(|e| format!("read adaptive planned day event_id: {e}"))?;
        let original_event_id: String = row
            .try_get("original_event_id")
            .map_err(|e| format!("read adaptive planned day original_event_id: {e}"))?;
        let start_time: String = row
            .try_get("start_time")
            .map_err(|e| format!("read adaptive planned day start_time: {e}"))?;
        let end_time: String = row
            .try_get("end_time")
            .map_err(|e| format!("read adaptive planned day end_time: {e}"))?;
        let source_kind: String = row
            .try_get("source_kind")
            .map_err(|e| format!("read adaptive planned day source_kind: {e}"))?;
        let key = format!("{original_event_id}|{start_time}");
        blocks_by_key
            .entry(key)
            .or_insert(AdaptivePlannedPomodoroBlock {
                event_id: Some(event_id),
                original_event_id,
                planned_start: start_time,
                planned_end: end_time,
                source_kind,
            });
    }
    let mut blocks = blocks_by_key.into_values().collect::<Vec<_>>();
    blocks.sort_by(|a, b| a.planned_start.cmp(&b.planned_start));
    Ok(blocks)
}

async fn insert_adaptive_day_outcomes_tx(
    tx: &mut Transaction<'_, Sqlite>,
    decision_id: &str,
    measured_at: &str,
    summary: &AdaptiveDayOutcomeSummary,
) -> Result<(), String> {
    let assignment_id = load_adaptive_assignment_id_for_decision_tx(tx, decision_id).await?;
    let assignment_id = assignment_id.as_deref();
    for (key, value) in [
        ("day_run_count", summary.run_count),
        ("day_completed_run_count", summary.completed_run_count),
        ("day_stopped_run_count", summary.stopped_run_count),
        ("day_clean_focus_seconds", summary.clean_focus_seconds),
        (
            "day_completed_focus_segments",
            summary.completed_focus_segments,
        ),
        (
            "day_interrupted_focus_segments",
            summary.interrupted_focus_segments,
        ),
        ("day_focus_failure_count", summary.focus_failure_count),
        ("day_break_skipped_count", summary.break_skipped_count),
        ("day_break_overtime_seconds", summary.break_overtime_seconds),
        ("day_blocked_attempt_count", summary.blocked_attempt_count),
        (
            "day_planned_pomodoro_event_count",
            summary.planned_pomodoro_event_count,
        ),
        (
            "day_started_planned_pomodoro_event_count",
            summary.started_planned_pomodoro_event_count,
        ),
        (
            "day_missed_planned_pomodoro_event_count",
            summary.missed_planned_pomodoro_event_count,
        ),
        (
            "day_planned_pomodoro_minutes",
            summary.planned_pomodoro_minutes,
        ),
    ] {
        insert_adaptive_outcome_numeric_tx(
            tx,
            decision_id,
            assignment_id,
            "day",
            key,
            value as f64,
            measured_at,
        )
        .await?;
    }
    insert_adaptive_outcome_boolean_tx(
        tx,
        decision_id,
        assignment_id,
        "day",
        "day_started_run",
        summary.run_count > 0,
        measured_at,
    )
    .await?;
    insert_adaptive_outcome_boolean_tx(
        tx,
        decision_id,
        assignment_id,
        "day",
        "day_observed",
        true,
        measured_at,
    )
    .await
}

async fn insert_adaptive_next_day_outcomes_tx(
    tx: &mut Transaction<'_, Sqlite>,
    decision_id: &str,
    measured_at: &str,
    summary: &AdaptiveNextDayOutcomeSummary,
) -> Result<(), String> {
    let assignment_id = load_adaptive_assignment_id_for_decision_tx(tx, decision_id).await?;
    let assignment_id = assignment_id.as_deref();
    for (key, value) in [
        ("next_day_run_count", summary.run_count),
        ("next_day_completed_run_count", summary.completed_run_count),
        ("next_day_stopped_run_count", summary.stopped_run_count),
        ("next_day_clean_focus_seconds", summary.clean_focus_seconds),
        (
            "next_day_completed_focus_segments",
            summary.completed_focus_segments,
        ),
        (
            "next_day_interrupted_focus_segments",
            summary.interrupted_focus_segments,
        ),
        ("next_day_focus_failure_count", summary.focus_failure_count),
        ("next_day_break_skipped_count", summary.break_skipped_count),
        (
            "next_day_break_overtime_seconds",
            summary.break_overtime_seconds,
        ),
        (
            "next_day_blocked_attempt_count",
            summary.blocked_attempt_count,
        ),
    ] {
        insert_adaptive_outcome_numeric_tx(
            tx,
            decision_id,
            assignment_id,
            "next_day",
            key,
            value as f64,
            measured_at,
        )
        .await?;
    }
    insert_adaptive_outcome_boolean_tx(
        tx,
        decision_id,
        assignment_id,
        "next_day",
        "next_day_started_run",
        summary.run_count > 0,
        measured_at,
    )
    .await?;
    insert_adaptive_outcome_boolean_tx(
        tx,
        decision_id,
        assignment_id,
        "next_day",
        "next_day_observed",
        true,
        measured_at,
    )
    .await
}

fn next_event_date(event_date: &str) -> Option<String> {
    let date = NaiveDate::parse_from_str(event_date, "%Y-%m-%d").ok()?;
    date.checked_add_signed(Duration::days(1))
        .map(|next| next.format("%Y-%m-%d").to_string())
}

pub(crate) async fn load_adaptive_assignment_id_for_decision_tx(
    tx: &mut Transaction<'_, Sqlite>,
    decision_id: &str,
) -> Result<Option<String>, String> {
    sqlx::query_scalar(
        "SELECT a.id
         FROM pomodoro_adaptive_assignments a
         JOIN pomodoro_adaptive_decisions d
           ON d.context_snapshot_id = a.context_snapshot_id
         WHERE d.id = ?
           AND (d.run_id IS NULL OR a.run_id IS NULL OR d.run_id = a.run_id)
           AND (d.segment_id IS NULL OR a.segment_id IS NULL OR d.segment_id = a.segment_id)
         ORDER BY a.assigned_at ASC
         LIMIT 1",
    )
    .bind(decision_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load adaptive assignment for decision: {e}"))
}
