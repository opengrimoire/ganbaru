mod budget;

use std::collections::{HashMap, HashSet};

use budget::AggregateBudget;
use ganbaru_db::impl_sqlite_from_row;
use sqlx::Row;
use sqlx::SqlitePool;

use super::validation::require_non_empty;
use super::*;

const MAX_ADAPTIVE_EVIDENCE_ROWS: usize = 4096;
const MAX_CALENDAR_EVENT_IDS: usize = 512;
const MAX_CALENDAR_EVENT_ID_BYTES: usize = 1_036;
const MAX_CALENDAR_HISTORY_ROWS: usize = 10_000;

/// Count every indexed outcome scan for eligible assignments before aggregation.
pub(super) const EXPERIMENT_OUTCOME_INPUT_SQL: &str =
    "SELECT 1 FROM pomodoro_adaptive_experiments e CROSS JOIN pomodoro_adaptive_assignments a
     LEFT JOIN pomodoro_adaptive_outcomes o ON o.assignment_id = a.id
     WHERE e.policy_id = ? AND a.experiment_id = e.id AND a.assigned_at < ?";

fn validate_evidence_rows(count: usize, domain: &str) -> Result<(), String> {
    if count > MAX_ADAPTIVE_EVIDENCE_ROWS {
        return Err(format!("Adaptive {domain} evidence exceeds its row limit"));
    }
    Ok(())
}

struct PomodoroSegmentRow {
    id: String,
    event_id: String,
    event_date: String,
    run_id: String,
    rhythm_position: i64,
    phase: String,
    planned_start: String,
    planned_end: String,
    actual_start: Option<String>,
    actual_end: Option<String>,
    status: String,
}
impl_sqlite_from_row!(PomodoroSegmentRow {
    id,
    event_id,
    event_date,
    run_id,
    rhythm_position,
    phase,
    planned_start,
    planned_end,
    actual_start,
    actual_end,
    status,
});

struct PomodoroAdaptiveHistorySegmentRow {
    id: String,
    run_id: String,
    rhythm_position: i64,
    phase: String,
    planned_start: String,
    planned_end: String,
    actual_start: Option<String>,
    actual_end: Option<String>,
    status: String,
    end_reason: Option<String>,
}
impl_sqlite_from_row!(PomodoroAdaptiveHistorySegmentRow {
    id,
    run_id,
    rhythm_position,
    phase,
    planned_start,
    planned_end,
    actual_start,
    actual_end,
    status,
    end_reason,
});

/// Read executed segments and pauses for exact visible Calendar occurrence identities.
///
/// Current run aliases take precedence over original occurrence identities. Device-day
/// dates remain execution evidence and never determine a Calendar occurrence's identity.
pub async fn pomodoro_load_segments_for_events(
    pool: SqlitePool,
    event_ids: Vec<String>,
) -> Result<Vec<PomodoroSegmentRead>, String> {
    if event_ids.is_empty() {
        return Ok(Vec::new());
    }
    if event_ids.len() > MAX_CALENDAR_EVENT_IDS {
        return Err("Focus Calendar query exceeds its event identity budget".into());
    }
    for event_id in &event_ids {
        require_non_empty(event_id, "event_id")?;
        if event_id.len() > MAX_CALENDAR_EVENT_ID_BYTES {
            return Err("Focus Calendar event identity exceeds its byte budget".into());
        }
    }

    let placeholders = std::iter::repeat_n("?", event_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT s.id, r.original_event_id AS event_id, s.event_date, s.run_id, s.rhythm_position, s.phase,
                s.planned_start, s.planned_end, s.actual_start, s.actual_end, s.status
         FROM pomodoro_runs r JOIN pomodoro_segments s ON s.run_id = r.id
         WHERE r.original_event_id IN ({placeholders}) AND r.current_occurrence_id IS NULL
           AND s.status IN ('completed', 'active', 'interrupted')
         UNION ALL
         SELECT s.id, r.current_occurrence_id AS event_id, s.event_date, s.run_id, s.rhythm_position, s.phase,
                s.planned_start, s.planned_end, s.actual_start, s.actual_end, s.status
         FROM pomodoro_runs r JOIN pomodoro_segments s ON s.run_id = r.id
         WHERE r.current_occurrence_id IN ({placeholders})
           AND s.status IN ('completed', 'active', 'interrupted')
         LIMIT {}",
        MAX_CALENDAR_HISTORY_ROWS + 1
    );
    let mut q = sqlx::query_as::<_, PomodoroSegmentRow>(&query);
    for event_id in event_ids.iter().chain(event_ids.iter()) {
        q = q.bind(event_id);
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin Focus Calendar history snapshot: {error}"))?;
    let mut rows = q
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("load pomodoro event segments: {e}"))?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    if rows.len() > MAX_CALENDAR_HISTORY_ROWS {
        return Err("Focus Calendar segments exceed their row budget".into());
    }
    rows.sort_by(|a, b| (&a.planned_start, &a.id).cmp(&(&b.planned_start, &b.id)));

    let segment_ids = rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>();
    let placeholders = std::iter::repeat_n("?", segment_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let pause_query = format!(
        "SELECT segment_id, started_at, ended_at, reason
         FROM pomodoro_pauses
         WHERE segment_id IN ({placeholders})
         LIMIT {}",
        MAX_CALENDAR_HISTORY_ROWS + 1
    );
    let mut pause_q = sqlx::query(&pause_query);
    for id in &segment_ids {
        pause_q = pause_q.bind(id);
    }
    let pause_rows = pause_q
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("load pomodoro pauses: {e}"))?;
    if pause_rows.len() > MAX_CALENDAR_HISTORY_ROWS {
        return Err("Focus Calendar pauses exceed their row budget".into());
    }
    let mut pauses_by_segment: HashMap<String, Vec<PomodoroPauseWrite>> = HashMap::new();
    for row in pause_rows {
        let segment_id: String = row
            .try_get("segment_id")
            .map_err(|e| format!("read pause segment_id: {e}"))?;
        let pause = PomodoroPauseWrite {
            started_at: row
                .try_get("started_at")
                .map_err(|e| format!("read pause started_at: {e}"))?,
            ended_at: row
                .try_get("ended_at")
                .map_err(|e| format!("read pause ended_at: {e}"))?,
            reason: row
                .try_get("reason")
                .map_err(|e| format!("read pause reason: {e}"))?,
        };
        pauses_by_segment.entry(segment_id).or_default().push(pause);
    }
    for pauses in pauses_by_segment.values_mut() {
        pauses.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    }
    tx.commit()
        .await
        .map_err(|error| format!("finish Focus Calendar history snapshot: {error}"))?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let pauses = pauses_by_segment.remove(&row.id).unwrap_or_default();
            PomodoroSegmentRead {
                id: row.id,
                event_id: row.event_id,
                event_date: row.event_date,
                run_id: row.run_id,
                rhythm_position: row.rhythm_position,
                phase: row.phase,
                planned_start: row.planned_start,
                planned_end: row.planned_end,
                actual_start: row.actual_start,
                actual_end: row.actual_end,
                status: row.status,
                pauses,
            }
        })
        .collect())
}

#[cfg(test)]
pub(super) async fn load_adaptive_replay_dataset_from_pool(
    pool: &sqlx::SqlitePool,
    before: &str,
    policy_id: &str,
    limit: i64,
    history_segment_limit: i64,
) -> Result<PomodoroAdaptiveReplayDatasetRead, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("Begin adaptive replay snapshot: {error}"))?;
    let result =
        load_adaptive_replay_dataset_tx(&mut tx, before, policy_id, limit, history_segment_limit)
            .await?;
    tx.commit()
        .await
        .map_err(|error| format!("Finish adaptive replay snapshot: {error}"))?;
    Ok(result)
}

#[cfg(test)]
pub(super) async fn load_adaptive_history_from_pool(
    pool: &sqlx::SqlitePool,
    before: &str,
    policy_id: &str,
    segment_limit: i64,
) -> Result<PomodoroAdaptiveHistoryRead, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("Begin adaptive history snapshot: {error}"))?;
    let result = load_adaptive_history_tx(&mut tx, before, policy_id, segment_limit).await?;
    tx.commit()
        .await
        .map_err(|error| format!("Finish adaptive history snapshot: {error}"))?;
    Ok(result)
}

/// Read replay metadata, outcomes and evidence through the caller's snapshot.
pub(super) async fn load_adaptive_replay_dataset_tx(
    connection: &mut sqlx::SqliteConnection,
    before: &str,
    policy_id: &str,
    limit: i64,
    history_segment_limit: i64,
) -> Result<PomodoroAdaptiveReplayDatasetRead, String> {
    let mut budget = AggregateBudget::replay();
    budget
        .admit(
            connection,
            "SELECT 1 FROM pomodoro_adaptive_decisions WHERE policy_id = ? AND occurred_at < ?",
            policy_id,
            Some(before),
            "replay decision",
        )
        .await?;
    budget.admit(connection,
        "SELECT 1 FROM pomodoro_adaptive_decisions d CROSS JOIN pomodoro_adaptive_decision_values v
         WHERE d.policy_id = ? AND d.occurred_at < ? AND v.decision_id = d.id",
        policy_id, Some(before), "replay decision value").await?;
    let rows = sqlx::query(
        "SELECT d.id AS decision_id,
                d.run_id AS run_id,
                d.occurred_at AS occurred_at,
                d.candidate_id AS candidate_id,
                r.planned_start AS planned_start,
                r.planned_end AS planned_end,
                MAX(CASE
                    WHEN v.value_key = 'focus_duration_minutes'
                    THEN v.previous_numeric_value
                END) AS previous_focus_duration_minutes,
                MAX(CASE
                    WHEN v.value_key = 'short_break_minutes'
                    THEN v.previous_numeric_value
                END) AS previous_short_break_minutes,
                MAX(CASE
                    WHEN v.value_key = 'long_break_minutes'
                    THEN v.previous_numeric_value
                END) AS previous_long_break_minutes,
                MAX(CASE
                    WHEN v.value_key = 'long_break_after_focus_count'
                    THEN v.previous_numeric_value
                END) AS previous_long_break_after_focus_count,
                MAX(CASE
                    WHEN v.value_key = 'focus_duration_minutes'
                    THEN v.selected_numeric_value
                END) AS selected_focus_duration_minutes,
                MAX(CASE
                    WHEN v.value_key = 'short_break_minutes'
                    THEN v.selected_numeric_value
                END) AS selected_short_break_minutes,
                MAX(CASE
                    WHEN v.value_key = 'long_break_minutes'
                    THEN v.selected_numeric_value
                END) AS selected_long_break_minutes,
                MAX(CASE
                    WHEN v.value_key = 'long_break_after_focus_count'
                    THEN v.selected_numeric_value
                END) AS selected_long_break_after_focus_count
         FROM pomodoro_adaptive_decisions d
         CROSS JOIN pomodoro_runs r
         CROSS JOIN pomodoro_adaptive_decision_values v
         WHERE d.policy_id = ?
           AND r.id = d.run_id
           AND v.decision_id = d.id
           AND d.opportunity_kind = 'run_start'
           AND d.occurred_at < ?
         GROUP BY d.id, d.run_id, d.occurred_at, d.candidate_id, r.planned_start, r.planned_end
         HAVING COUNT(DISTINCT v.value_key) = 4
         ORDER BY d.occurred_at DESC
         LIMIT ?",
    )
    .bind(policy_id)
    .bind(before)
    .bind(limit)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load adaptive replay decisions: {e}"))?;

    let opportunities = rows
        .into_iter()
        .map(|row| {
            let decision_id: String = row
                .try_get("decision_id")
                .map_err(|e| format!("read adaptive replay decision_id: {e}"))?;
            Ok(PomodoroAdaptiveReplayRunStartOpportunityRead {
                id: decision_id,
                run_id: row
                    .try_get("run_id")
                    .map_err(|e| format!("read adaptive replay run_id: {e}"))?,
                started_at: row
                    .try_get("occurred_at")
                    .map_err(|e| format!("read adaptive replay occurred_at: {e}"))?,
                planned_start: row
                    .try_get("planned_start")
                    .map_err(|e| format!("read adaptive replay planned_start: {e}"))?,
                planned_end: row
                    .try_get("planned_end")
                    .map_err(|e| format!("read adaptive replay planned_end: {e}"))?,
                candidate_id: row
                    .try_get("candidate_id")
                    .map_err(|e| format!("read adaptive replay candidate_id: {e}"))?,
                current_rhythm: read_replay_count_rhythm(&row, "previous")?,
                selected_rhythm: read_replay_count_rhythm(&row, "selected")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let opportunity_ids = opportunities
        .iter()
        .map(|opportunity| opportunity.id.as_str())
        .collect::<Vec<_>>();
    let outcomes = load_adaptive_replay_outcome_rows(connection, &opportunity_ids, before).await?;
    let histories = load_adaptive_replay_histories(
        connection,
        &opportunities,
        policy_id,
        history_segment_limit,
        &mut budget,
    )
    .await?;
    Ok(PomodoroAdaptiveReplayDatasetRead {
        opportunities,
        outcomes,
        histories,
    })
}

async fn load_adaptive_replay_outcome_rows(
    connection: &mut sqlx::SqliteConnection,
    opportunity_ids: &[&str],
    before: &str,
) -> Result<Vec<PomodoroAdaptiveReplayOutcomeRowRead>, String> {
    if opportunity_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", opportunity_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT decision_id, outcome_window, outcome_key, numeric_value, boolean_value,
                categorical_value
         FROM pomodoro_adaptive_outcomes
         WHERE decision_id IN ({placeholders})
           AND outcome_window = 'run'
           AND julianday(measured_at) <= julianday(?)
         ORDER BY measured_at ASC, outcome_key ASC
         LIMIT ?"
    );
    let mut q = sqlx::query(&query);
    for id in opportunity_ids {
        q = q.bind(id);
    }
    q = q.bind(before).bind(MAX_ADAPTIVE_EVIDENCE_ROWS as i64 + 1);
    let rows = q
        .fetch_all(&mut *connection)
        .await
        .map_err(|e| format!("load adaptive replay outcomes: {e}"))?;
    validate_evidence_rows(rows.len(), "replay outcome")?;
    rows.into_iter()
        .map(|row| {
            let boolean_value: Option<i64> = row
                .try_get("boolean_value")
                .map_err(|e| format!("read adaptive replay boolean_value: {e}"))?;
            Ok(PomodoroAdaptiveReplayOutcomeRowRead {
                opportunity_id: row
                    .try_get("decision_id")
                    .map_err(|e| format!("read adaptive replay outcome decision_id: {e}"))?,
                outcome_window: row
                    .try_get("outcome_window")
                    .map_err(|e| format!("read adaptive replay outcome_window: {e}"))?,
                outcome_key: row
                    .try_get("outcome_key")
                    .map_err(|e| format!("read adaptive replay outcome_key: {e}"))?,
                numeric_value: row
                    .try_get("numeric_value")
                    .map_err(|e| format!("read adaptive replay numeric_value: {e}"))?,
                boolean_value: boolean_value.map(|value| value != 0),
                categorical_value: row
                    .try_get("categorical_value")
                    .map_err(|e| format!("read adaptive replay categorical_value: {e}"))?,
            })
        })
        .collect()
}

async fn load_adaptive_replay_histories(
    connection: &mut sqlx::SqliteConnection,
    opportunities: &[PomodoroAdaptiveReplayRunStartOpportunityRead],
    policy_id: &str,
    history_segment_limit: i64,
    budget: &mut AggregateBudget,
) -> Result<Vec<PomodoroAdaptiveReplayOpportunityHistoryRead>, String> {
    let mut histories = Vec::with_capacity(opportunities.len());
    for opportunity in opportunities {
        histories.push(PomodoroAdaptiveReplayOpportunityHistoryRead {
            opportunity_id: opportunity.id.clone(),
            history: load_adaptive_history_snapshot_tx(
                connection,
                &opportunity.started_at,
                policy_id,
                history_segment_limit,
                AdaptiveHistoryStateSource::Historical,
                budget,
            )
            .await?,
        });
    }
    Ok(histories)
}

fn read_replay_count_rhythm(
    row: &sqlx::sqlite::SqliteRow,
    prefix: &str,
) -> Result<PomodoroRunRhythm, String> {
    let rhythm = PomodoroRunRhythm::Count {
        focus_duration_minutes: read_replay_i64(row, &format!("{prefix}_focus_duration_minutes"))?,
        short_break_minutes: read_replay_i64(row, &format!("{prefix}_short_break_minutes"))?,
        long_break_minutes: read_replay_i64(row, &format!("{prefix}_long_break_minutes"))?,
        long_break_after_focus_count: read_replay_i64(
            row,
            &format!("{prefix}_long_break_after_focus_count"),
        )?,
    };
    super::validation::validate_run_rhythm(&rhythm)?;
    Ok(rhythm)
}

fn read_replay_i64(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<i64, String> {
    let value: Option<f64> = row
        .try_get(column)
        .map_err(|e| format!("read adaptive replay {column}: {e}"))?;
    let Some(value) = value else {
        return Err(format!("adaptive replay decision missing {column}"));
    };
    if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value >= i64::MAX as f64 {
        return Err(format!(
            "adaptive replay decision {column} must be a finite nonnegative integer"
        ));
    }
    Ok(value as i64)
}

pub(super) async fn load_adaptive_history_tx(
    connection: &mut sqlx::SqliteConnection,
    before: &str,
    policy_id: &str,
    segment_limit: i64,
) -> Result<PomodoroAdaptiveHistoryRead, String> {
    let mut budget = AggregateBudget::history();
    load_adaptive_history_snapshot_tx(
        connection,
        before,
        policy_id,
        segment_limit,
        AdaptiveHistoryStateSource::Current,
        &mut budget,
    )
    .await
}

enum AdaptiveHistoryStateSource {
    Current,
    Historical,
}

async fn load_adaptive_history_snapshot_tx(
    connection: &mut sqlx::SqliteConnection,
    before: &str,
    policy_id: &str,
    segment_limit: i64,
    state_source: AdaptiveHistoryStateSource,
    budget: &mut AggregateBudget,
) -> Result<PomodoroAdaptiveHistoryRead, String> {
    admit_policy_experiments(connection, policy_id).await?;
    let mut segment_rows = sqlx::query_as::<_, PomodoroAdaptiveHistorySegmentRow>(
        "SELECT id, run_id, rhythm_position, phase, planned_start, planned_end, actual_start, actual_end,
                status, end_reason
         FROM pomodoro_segments
         WHERE planned_start < ?
           AND status IN ('completed', 'interrupted')
           AND julianday(actual_end) <= julianday(?)
         ORDER BY planned_start DESC
         LIMIT ?",
    )
    .bind(before)
    .bind(before)
    .bind(segment_limit)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load adaptive pomodoro segments: {e}"))?;
    segment_rows.reverse();

    let segment_ids = segment_rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>();
    let pauses_by_segment = load_pauses_for_segment_ids(connection, &segment_ids).await?;

    let run_ids = segment_rows
        .iter()
        .map(|row| row.run_id.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let run_events = load_adaptive_run_events(connection, &run_ids, before).await?;
    let block_events = load_adaptive_block_events(connection, before, segment_limit).await?;
    let previous_states = match state_source {
        AdaptiveHistoryStateSource::Current => {
            load_adaptive_context_states(connection, policy_id).await?
        }
        AdaptiveHistoryStateSource::Historical => {
            load_adaptive_context_states_before(connection, policy_id, before, budget).await?
        }
    };
    let experiment_states = load_adaptive_experiment_states(connection, policy_id, before).await?;
    let experiment_outcomes =
        load_adaptive_experiment_outcomes(connection, policy_id, before, budget).await?;
    let experiment_assignments =
        load_adaptive_experiment_assignments(connection, policy_id, before).await?;

    let segments = segment_rows
        .into_iter()
        .map(|row| PomodoroAdaptiveHistorySegmentRead {
            pause_log: pauses_by_segment.get(&row.id).cloned().unwrap_or_default(),
            run_id: row.run_id,
            rhythm_position: row.rhythm_position,
            phase: row.phase,
            planned_start: row.planned_start,
            planned_end: row.planned_end,
            actual_start: row.actual_start,
            actual_end: row.actual_end,
            status: row.status,
            end_reason: row.end_reason,
        })
        .collect();

    Ok(PomodoroAdaptiveHistoryRead {
        segments,
        run_events,
        block_events,
        previous_states,
        experiment_states,
        experiment_outcomes,
        experiment_assignments,
    })
}

async fn load_pauses_for_segment_ids(
    connection: &mut sqlx::SqliteConnection,
    segment_ids: &[&str],
) -> Result<HashMap<String, Vec<PomodoroPauseWrite>>, String> {
    if segment_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat_n("?", segment_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let pause_query = format!(
        "SELECT segment_id, started_at, ended_at, reason
         FROM pomodoro_pauses
         WHERE segment_id IN ({placeholders})
         ORDER BY started_at ASC
         LIMIT ?"
    );
    let mut pause_q = sqlx::query(&pause_query);
    for id in segment_ids {
        pause_q = pause_q.bind(id);
    }
    pause_q = pause_q.bind(MAX_ADAPTIVE_EVIDENCE_ROWS as i64 + 1);
    let pause_rows = pause_q
        .fetch_all(&mut *connection)
        .await
        .map_err(|e| format!("load adaptive pomodoro pauses: {e}"))?;
    validate_evidence_rows(pause_rows.len(), "pause")?;
    let mut pauses_by_segment: HashMap<String, Vec<PomodoroPauseWrite>> = HashMap::new();
    for row in pause_rows {
        let segment_id: String = row
            .try_get("segment_id")
            .map_err(|e| format!("read adaptive pause segment_id: {e}"))?;
        let pause = PomodoroPauseWrite {
            started_at: row
                .try_get("started_at")
                .map_err(|e| format!("read adaptive pause started_at: {e}"))?,
            ended_at: row
                .try_get("ended_at")
                .map_err(|e| format!("read adaptive pause ended_at: {e}"))?,
            reason: row
                .try_get("reason")
                .map_err(|e| format!("read adaptive pause reason: {e}"))?,
        };
        pauses_by_segment.entry(segment_id).or_default().push(pause);
    }
    Ok(pauses_by_segment)
}

async fn load_adaptive_run_events(
    connection: &mut sqlx::SqliteConnection,
    run_ids: &[&str],
    before: &str,
) -> Result<Vec<PomodoroAdaptiveHistoryRunEventRead>, String> {
    if run_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", run_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT event_type, occurred_at, phase, reason, duration_seconds
         FROM pomodoro_run_events
         WHERE run_id IN ({placeholders})
           AND julianday(occurred_at) <= julianday(?)
         ORDER BY occurred_at ASC
         LIMIT ?"
    );
    let mut q = sqlx::query(&query);
    for id in run_ids {
        q = q.bind(id);
    }
    q = q.bind(before).bind(MAX_ADAPTIVE_EVIDENCE_ROWS as i64 + 1);
    let rows = q
        .fetch_all(&mut *connection)
        .await
        .map_err(|e| format!("load adaptive run events: {e}"))?;
    validate_evidence_rows(rows.len(), "run event")?;
    rows.into_iter()
        .map(|row| {
            Ok(PomodoroAdaptiveHistoryRunEventRead {
                event_type: row
                    .try_get("event_type")
                    .map_err(|e| format!("read adaptive run event event_type: {e}"))?,
                occurred_at: row
                    .try_get("occurred_at")
                    .map_err(|e| format!("read adaptive run event occurred_at: {e}"))?,
                phase: row
                    .try_get("phase")
                    .map_err(|e| format!("read adaptive run event phase: {e}"))?,
                reason: row
                    .try_get("reason")
                    .map_err(|e| format!("read adaptive run event reason: {e}"))?,
                duration_seconds: row
                    .try_get("duration_seconds")
                    .map_err(|e| format!("read adaptive run event duration_seconds: {e}"))?,
            })
        })
        .collect()
}

async fn load_adaptive_block_events(
    connection: &mut sqlx::SqliteConnection,
    before: &str,
    limit: i64,
) -> Result<Vec<PomodoroAdaptiveHistoryBlockEventRead>, String> {
    let mut rows = sqlx::query(
        "SELECT occurred_at, phase, source_type, source_key, decision
         FROM distractions_block_events
         WHERE occurred_at < ?
         ORDER BY occurred_at DESC
         LIMIT ?",
    )
    .bind(before)
    .bind(limit)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load adaptive block events: {e}"))?;
    rows.reverse();
    rows.into_iter()
        .map(|row| {
            Ok(PomodoroAdaptiveHistoryBlockEventRead {
                occurred_at: row
                    .try_get("occurred_at")
                    .map_err(|e| format!("read adaptive block event occurred_at: {e}"))?,
                phase: row
                    .try_get("phase")
                    .map_err(|e| format!("read adaptive block event phase: {e}"))?,
                source_type: row
                    .try_get("source_type")
                    .map_err(|e| format!("read adaptive block event source_type: {e}"))?,
                source_key: row
                    .try_get("source_key")
                    .map_err(|e| format!("read adaptive block event source_key: {e}"))?,
                decision: row
                    .try_get("decision")
                    .map_err(|e| format!("read adaptive block event decision: {e}"))?,
            })
        })
        .collect()
}

async fn load_adaptive_context_states(
    connection: &mut sqlx::SqliteConnection,
    policy_id: &str,
) -> Result<Vec<PomodoroAdaptiveHistoryContextStateRead>, String> {
    let rows = sqlx::query(
        "SELECT context_key, readiness, strain, recovery_debt, avoidance_pressure, momentum,
                confidence
         FROM pomodoro_adaptive_context_states
         WHERE policy_id = ?
         ORDER BY updated_at DESC
         LIMIT 120",
    )
    .bind(policy_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load adaptive context states: {e}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(PomodoroAdaptiveHistoryContextStateRead {
                context_key: row
                    .try_get("context_key")
                    .map_err(|e| format!("read adaptive context state key: {e}"))?,
                readiness: row
                    .try_get("readiness")
                    .map_err(|e| format!("read adaptive state readiness: {e}"))?,
                strain: row
                    .try_get("strain")
                    .map_err(|e| format!("read adaptive state strain: {e}"))?,
                recovery_debt: row
                    .try_get("recovery_debt")
                    .map_err(|e| format!("read adaptive state recovery_debt: {e}"))?,
                avoidance_pressure: row
                    .try_get("avoidance_pressure")
                    .map_err(|e| format!("read adaptive state avoidance_pressure: {e}"))?,
                momentum: row
                    .try_get("momentum")
                    .map_err(|e| format!("read adaptive state momentum: {e}"))?,
                confidence: row
                    .try_get("confidence")
                    .map_err(|e| format!("read adaptive state confidence: {e}"))?,
            })
        })
        .collect()
}

async fn load_adaptive_context_states_before(
    connection: &mut sqlx::SqliteConnection,
    policy_id: &str,
    before: &str,
    budget: &mut AggregateBudget,
) -> Result<Vec<PomodoroAdaptiveHistoryContextStateRead>, String> {
    budget
        .admit(
            connection,
            "SELECT 1 FROM pomodoro_adaptive_context_state_history WHERE policy_id = ?",
            policy_id,
            None,
            "historical context",
        )
        .await?;
    let rows = sqlx::query(
        "SELECT h.context_key, h.readiness, h.strain, h.recovery_debt,
                h.avoidance_pressure, h.momentum, h.confidence
         FROM (
            SELECT context_key, MAX(observed_at) AS observed_at
            FROM pomodoro_adaptive_context_state_history
            WHERE policy_id = ?
              AND observed_at < ?
            GROUP BY context_key
         ) latest
         CROSS JOIN pomodoro_adaptive_context_state_history h
         WHERE h.policy_id = ?
           AND latest.context_key = h.context_key
           AND latest.observed_at = h.observed_at
         ORDER BY h.observed_at DESC
         LIMIT 120",
    )
    .bind(policy_id)
    .bind(before)
    .bind(policy_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load adaptive historical context states: {e}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(PomodoroAdaptiveHistoryContextStateRead {
                context_key: row
                    .try_get("context_key")
                    .map_err(|e| format!("read adaptive historical context state key: {e}"))?,
                readiness: row
                    .try_get("readiness")
                    .map_err(|e| format!("read adaptive historical state readiness: {e}"))?,
                strain: row
                    .try_get("strain")
                    .map_err(|e| format!("read adaptive historical state strain: {e}"))?,
                recovery_debt: row
                    .try_get("recovery_debt")
                    .map_err(|e| format!("read adaptive historical state recovery_debt: {e}"))?,
                avoidance_pressure: row.try_get("avoidance_pressure").map_err(|e| {
                    format!("read adaptive historical state avoidance_pressure: {e}")
                })?,
                momentum: row
                    .try_get("momentum")
                    .map_err(|e| format!("read adaptive historical state momentum: {e}"))?,
                confidence: row
                    .try_get("confidence")
                    .map_err(|e| format!("read adaptive historical state confidence: {e}"))?,
            })
        })
        .collect()
}

async fn load_adaptive_experiment_states(
    connection: &mut sqlx::SqliteConnection,
    policy_id: &str,
    before: &str,
) -> Result<Vec<PomodoroAdaptiveHistoryExperimentStateRead>, String> {
    let rows = sqlx::query(
        "SELECT id, status, started_at, ended_at
         FROM pomodoro_adaptive_experiments
         WHERE policy_id = ?
           AND (started_at IS NULL OR started_at < ?)
           AND (ended_at IS NULL OR ended_at < ?)
         ORDER BY COALESCE(ended_at, started_at, created_at) DESC
         LIMIT 50",
    )
    .bind(policy_id)
    .bind(before)
    .bind(before)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load adaptive experiment states: {e}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(PomodoroAdaptiveHistoryExperimentStateRead {
                experiment_id: row
                    .try_get("id")
                    .map_err(|e| format!("read adaptive experiment state id: {e}"))?,
                status: row
                    .try_get("status")
                    .map_err(|e| format!("read adaptive experiment state status: {e}"))?,
                started_at: row
                    .try_get("started_at")
                    .map_err(|e| format!("read adaptive experiment state started_at: {e}"))?,
                ended_at: row
                    .try_get("ended_at")
                    .map_err(|e| format!("read adaptive experiment state ended_at: {e}"))?,
            })
        })
        .collect()
}

async fn load_adaptive_experiment_assignments(
    connection: &mut sqlx::SqliteConnection,
    policy_id: &str,
    before: &str,
) -> Result<Vec<PomodoroAdaptiveHistoryExperimentAssignmentRead>, String> {
    let rows = sqlx::query(
        "SELECT a.experiment_id AS experiment_id,
                a.variant_key AS variant_key,
                cs.time_of_day || ':' ||
                    cs.session_position || ':' ||
                    cs.event_length || ':' ||
                    cs.workload || ':' ||
                    cs.energy || ':' ||
                    COALESCE(cs.environment_id, 'none') AS context_key,
                a.assigned_at AS assigned_at
         FROM pomodoro_adaptive_experiments e
         CROSS JOIN pomodoro_adaptive_assignments a
         CROSS JOIN pomodoro_adaptive_context_snapshots cs
         WHERE e.policy_id = ?
           AND a.experiment_id = e.id
           AND cs.id = a.context_snapshot_id
           AND a.assigned_at < ?
         ORDER BY a.assigned_at DESC
         LIMIT 100",
    )
    .bind(policy_id)
    .bind(before)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load adaptive experiment assignments: {e}"))?;

    rows.into_iter()
        .map(|row| {
            Ok(PomodoroAdaptiveHistoryExperimentAssignmentRead {
                experiment_id: row
                    .try_get("experiment_id")
                    .map_err(|e| format!("read adaptive assignment experiment_id: {e}"))?,
                variant_key: row
                    .try_get("variant_key")
                    .map_err(|e| format!("read adaptive assignment variant_key: {e}"))?,
                context_key: row
                    .try_get("context_key")
                    .map_err(|e| format!("read adaptive assignment context_key: {e}"))?,
                assigned_at: row
                    .try_get("assigned_at")
                    .map_err(|e| format!("read adaptive assignment assigned_at: {e}"))?,
            })
        })
        .collect()
}

/// Bound the policy scan before state sorting or any assignment traversal.
async fn admit_policy_experiments(
    connection: &mut sqlx::SqliteConnection,
    policy_id: &str,
) -> Result<(), String> {
    let experiment_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT 1 FROM pomodoro_adaptive_experiments WHERE policy_id = ? LIMIT ?)",
    ).bind(policy_id).bind(MAX_ADAPTIVE_EVIDENCE_ROWS as i64 + 1)
        .fetch_one(&mut *connection).await
        .map_err(|error| format!("admit adaptive policy experiments: {error}"))?;
    validate_evidence_rows(experiment_count as usize, "policy experiment")
}

async fn load_adaptive_experiment_outcomes(
    connection: &mut sqlx::SqliteConnection,
    policy_id: &str,
    before: &str,
    budget: &mut AggregateBudget,
) -> Result<Vec<PomodoroAdaptiveHistoryExperimentOutcomeRead>, String> {
    // Later outcomes still cost an indexed scan, although their values never
    // contribute to the historical aggregate. Admit that work as well.
    budget
        .admit(
            connection,
            EXPERIMENT_OUTCOME_INPUT_SQL,
            policy_id,
            Some(before),
            "experiment outcome",
        )
        .await?;
    let rows = sqlx::query(
        "SELECT a.experiment_id AS experiment_id,
                a.variant_key AS variant_key,
                cs.time_of_day || ':' ||
                    cs.session_position || ':' ||
                    cs.event_length || ':' ||
                    cs.workload || ':' ||
                    cs.energy || ':' ||
                    COALESCE(cs.environment_id, 'none') AS context_key,
                COUNT(DISTINCT a.id) AS assignment_count,
                COUNT(DISTINCT CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'run_completed'
                    THEN o.id
                END) AS run_observed_count,
                COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run'
                     AND o.outcome_key = 'run_completed'
                     AND o.boolean_value = 1
                    THEN 1 ELSE 0
                END), 0) AS run_completed_count,
                COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run'
                     AND o.outcome_key = 'run_stopped'
                     AND o.boolean_value = 1
                    THEN 1 ELSE 0
                END), 0) AS run_stopped_count,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'clean_focus_seconds'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS clean_focus_seconds_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'clean_focus_seconds'
                    THEN o.numeric_value * o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS clean_focus_seconds_square_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'blocked_attempt_count'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS blocked_attempt_count_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'blocked_attempt_count'
                    THEN o.numeric_value * o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS blocked_attempt_count_square_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'break_skipped_count'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS break_skipped_count_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'break_skipped_count'
                    THEN o.numeric_value * o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS break_skipped_count_square_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'short_break_overtime_seconds'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS short_break_overtime_seconds_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'short_break_overtime_seconds'
                    THEN o.numeric_value * o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS short_break_overtime_seconds_square_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'long_break_overtime_seconds'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS long_break_overtime_seconds_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'run' AND o.outcome_key = 'long_break_overtime_seconds'
                    THEN o.numeric_value * o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS long_break_overtime_seconds_square_sum,
                COUNT(DISTINCT CASE
                    WHEN o.outcome_window = 'day' AND o.outcome_key = 'day_observed'
                    THEN o.id
                END) AS day_observed_count,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'day'
                     AND o.outcome_key = 'day_started_planned_pomodoro_event_count'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS day_started_planned_pomodoro_count_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'day'
                     AND o.outcome_key = 'day_missed_planned_pomodoro_event_count'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS day_missed_planned_pomodoro_count_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'day'
                     AND o.outcome_key = 'day_missed_planned_pomodoro_event_count'
                    THEN o.numeric_value * o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS day_missed_planned_pomodoro_count_square_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'day'
                     AND o.outcome_key = 'day_clean_focus_seconds'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS day_clean_focus_seconds_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'day'
                     AND o.outcome_key = 'day_blocked_attempt_count'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS day_blocked_attempt_count_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'day'
                     AND o.outcome_key = 'day_blocked_attempt_count'
                    THEN o.numeric_value * o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS day_blocked_attempt_count_square_sum,
                COUNT(DISTINCT CASE
                    WHEN o.outcome_window = 'next_day' AND o.outcome_key = 'next_day_observed'
                    THEN o.id
                END) AS next_day_observed_count,
                COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'next_day'
                     AND o.outcome_key = 'next_day_started_run'
                     AND o.boolean_value = 1
                    THEN 1 ELSE 0
                END), 0) AS next_day_started_run_count,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'next_day'
                     AND o.outcome_key = 'next_day_clean_focus_seconds'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS next_day_clean_focus_seconds_sum,
                CAST(COALESCE(SUM(CASE
                    WHEN o.outcome_window = 'next_day'
                     AND o.outcome_key = 'next_day_blocked_attempt_count'
                    THEN o.numeric_value ELSE 0
                END), 0.0) AS REAL) AS next_day_blocked_attempt_count_sum
         FROM pomodoro_adaptive_experiments e
         CROSS JOIN pomodoro_adaptive_assignments a
         CROSS JOIN pomodoro_adaptive_context_snapshots cs
         LEFT JOIN pomodoro_adaptive_outcomes o ON o.assignment_id = a.id
           AND julianday(o.measured_at) <= julianday(?)
         WHERE e.policy_id = ?
           AND a.experiment_id = e.id
           AND cs.id = a.context_snapshot_id
           AND a.assigned_at < ?
         GROUP BY a.experiment_id, a.variant_key, context_key
         ORDER BY a.experiment_id ASC, a.variant_key ASC, context_key ASC
         LIMIT ?",
    )
    .bind(before)
    .bind(policy_id)
    .bind(before)
    .bind(MAX_ADAPTIVE_EVIDENCE_ROWS as i64 + 1)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load adaptive experiment outcomes: {e}"))?;
    validate_evidence_rows(rows.len(), "experiment outcome")?;

    rows.into_iter()
        .map(|row| {
            Ok(PomodoroAdaptiveHistoryExperimentOutcomeRead {
                experiment_id: row
                    .try_get("experiment_id")
                    .map_err(|e| format!("read adaptive experiment outcome experiment_id: {e}"))?,
                variant_key: row
                    .try_get("variant_key")
                    .map_err(|e| format!("read adaptive experiment outcome variant_key: {e}"))?,
                context_key: row
                    .try_get("context_key")
                    .map_err(|e| format!("read adaptive experiment outcome context_key: {e}"))?,
                assignment_count: row
                    .try_get("assignment_count")
                    .map_err(|e| format!("read adaptive experiment outcome assignment_count: {e}"))?,
                run_observed_count: row.try_get("run_observed_count").map_err(|e| {
                    format!("read adaptive experiment outcome run_observed_count: {e}")
                })?,
                run_completed_count: row.try_get("run_completed_count").map_err(|e| {
                    format!("read adaptive experiment outcome run_completed_count: {e}")
                })?,
                run_stopped_count: row.try_get("run_stopped_count").map_err(|e| {
                    format!("read adaptive experiment outcome run_stopped_count: {e}")
                })?,
                clean_focus_seconds_sum: row.try_get("clean_focus_seconds_sum").map_err(|e| {
                    format!("read adaptive experiment outcome clean_focus_seconds_sum: {e}")
                })?,
                clean_focus_seconds_square_sum: row
                    .try_get("clean_focus_seconds_square_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome clean_focus_seconds_square_sum: {e}"
                        )
                    })?,
                blocked_attempt_count_sum: row.try_get("blocked_attempt_count_sum").map_err(|e| {
                    format!("read adaptive experiment outcome blocked_attempt_count_sum: {e}")
                })?,
                blocked_attempt_count_square_sum: row
                    .try_get("blocked_attempt_count_square_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome blocked_attempt_count_square_sum: {e}"
                        )
                    })?,
                break_skipped_count_sum: row.try_get("break_skipped_count_sum").map_err(|e| {
                    format!("read adaptive experiment outcome break_skipped_count_sum: {e}")
                })?,
                break_skipped_count_square_sum: row
                    .try_get("break_skipped_count_square_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome break_skipped_count_square_sum: {e}"
                        )
                    })?,
                short_break_overtime_seconds_sum: row
                    .try_get("short_break_overtime_seconds_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome short_break_overtime_seconds_sum: {e}"
                        )
                    })?,
                short_break_overtime_seconds_square_sum: row
                    .try_get("short_break_overtime_seconds_square_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome short_break_overtime_seconds_square_sum: {e}"
                        )
                    })?,
                long_break_overtime_seconds_sum: row
                    .try_get("long_break_overtime_seconds_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome long_break_overtime_seconds_sum: {e}"
                        )
                    })?,
                long_break_overtime_seconds_square_sum: row
                    .try_get("long_break_overtime_seconds_square_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome long_break_overtime_seconds_square_sum: {e}"
                        )
                    })?,
                day_observed_count: row.try_get("day_observed_count").map_err(|e| {
                    format!("read adaptive experiment outcome day_observed_count: {e}")
                })?,
                day_started_planned_pomodoro_count_sum: row
                    .try_get("day_started_planned_pomodoro_count_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome day_started_planned_pomodoro_count_sum: {e}"
                        )
                    })?,
                day_missed_planned_pomodoro_count_sum: row
                    .try_get("day_missed_planned_pomodoro_count_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome day_missed_planned_pomodoro_count_sum: {e}"
                        )
                    })?,
                day_missed_planned_pomodoro_count_square_sum: row
                    .try_get("day_missed_planned_pomodoro_count_square_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome day_missed_planned_pomodoro_count_square_sum: {e}"
                        )
                    })?,
                day_clean_focus_seconds_sum: row.try_get("day_clean_focus_seconds_sum").map_err(
                    |e| format!("read adaptive experiment outcome day_clean_focus_seconds_sum: {e}"),
                )?,
                day_blocked_attempt_count_sum: row
                    .try_get("day_blocked_attempt_count_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome day_blocked_attempt_count_sum: {e}"
                        )
                    })?,
                day_blocked_attempt_count_square_sum: row
                    .try_get("day_blocked_attempt_count_square_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome day_blocked_attempt_count_square_sum: {e}"
                        )
                    })?,
                next_day_observed_count: row.try_get("next_day_observed_count").map_err(|e| {
                    format!("read adaptive experiment outcome next_day_observed_count: {e}")
                })?,
                next_day_started_run_count: row.try_get("next_day_started_run_count").map_err(
                    |e| format!("read adaptive experiment outcome next_day_started_run_count: {e}"),
                )?,
                next_day_clean_focus_seconds_sum: row
                    .try_get("next_day_clean_focus_seconds_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome next_day_clean_focus_seconds_sum: {e}"
                        )
                    })?,
                next_day_blocked_attempt_count_sum: row
                    .try_get("next_day_blocked_attempt_count_sum")
                    .map_err(|e| {
                        format!(
                            "read adaptive experiment outcome next_day_blocked_attempt_count_sum: {e}"
                        )
                    })?,
            })
        })
        .collect()
}
