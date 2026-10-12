use serde::Deserialize;
use sqlx::{Row, Sqlite, Transaction};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;
use ganbaru_music_library::fixtures::{DenseMusicFixtureSummary, seed_dense_music_fixture};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkPomodoroHistoryPayload {
    configs: Vec<BenchmarkPomodoroConfigSeed>,
    segments: Vec<BenchmarkPomodoroSegmentSeed>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkPomodoroConfigSeed {
    event_id: String,
    config: BenchmarkPomodoroConfig,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkPomodoroConfig {
    rhythm: BenchmarkPomodoroRhythm,
    rhythm_source: String,
    preset_key: Option<String>,
    idle_timeout_minutes: Option<i64>,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum BenchmarkPomodoroRhythm {
    Count {
        focus_duration_minutes: i64,
        short_break_minutes: i64,
        long_break_minutes: i64,
        long_break_after_focus_count: i64,
    },
    Sequence {
        steps: Vec<BenchmarkPomodoroSequenceStep>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkPomodoroSequenceStep {
    focus_duration_minutes: i64,
    break_phase: String,
    break_duration_minutes: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkPomodoroSegmentSeed {
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
    pauses: Vec<BenchmarkPomodoroPauseSeed>,
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkPomodoroPauseSeed {
    started_at: String,
    ended_at: Option<String>,
    reason: String,
}

#[tauri::command]
pub async fn benchmark_seed_pomodoro_history<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    payload: BenchmarkPomodoroHistoryPayload,
) -> Result<(), String> {
    validate_payload(&payload)?;

    let pool = connect_sqlite(app, db_url).await?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;

    for config in &payload.configs {
        insert_config(&mut tx, config).await?;
    }
    for segment in &payload.segments {
        insert_segment(&mut tx, segment).await?;
    }

    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn benchmark_seed_dense_music_library<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<DenseMusicFixtureSummary, String> {
    let pool = connect_sqlite(app, db_url).await?;
    seed_dense_music_fixture(&pool)
        .await
        .map_err(|error| error.to_string())
}

async fn insert_config(
    tx: &mut Transaction<'_, Sqlite>,
    config: &BenchmarkPomodoroConfigSeed,
) -> Result<(), String> {
    sqlx::query("DELETE FROM calendar_event_pomodoro_configs WHERE event_id = ?")
        .bind(&config.event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear benchmark pomodoro config: {e}"))?;

    sqlx::query(
        "INSERT INTO calendar_event_pomodoro_configs
            (event_id, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&config.event_id)
    .bind(benchmark_rhythm_kind(&config.config.rhythm))
    .bind(&config.config.rhythm_source)
    .bind(&config.config.preset_key)
    .bind(config.config.idle_timeout_minutes)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("seed benchmark pomodoro config: {e}"))?;

    match &config.config.rhythm {
        BenchmarkPomodoroRhythm::Count {
            focus_duration_minutes,
            short_break_minutes,
            long_break_minutes,
            long_break_after_focus_count,
        } => {
            sqlx::query(
                "INSERT INTO calendar_event_pomodoro_config_count_rhythms
                    (event_id, focus_duration_minutes, short_break_minutes, long_break_minutes,
                     long_break_after_focus_count)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&config.event_id)
            .bind(focus_duration_minutes)
            .bind(short_break_minutes)
            .bind(long_break_minutes)
            .bind(long_break_after_focus_count)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("seed benchmark count rhythm: {e}"))?;
        }
        BenchmarkPomodoroRhythm::Sequence { steps } => {
            for (step_index, step) in steps.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO calendar_event_pomodoro_config_sequence_steps
                        (event_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(&config.event_id)
                .bind(step_index as i64)
                .bind(step.focus_duration_minutes)
                .bind(&step.break_phase)
                .bind(step.break_duration_minutes)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("seed benchmark sequence rhythm: {e}"))?;
            }
        }
    }
    Ok(())
}

fn benchmark_rhythm_kind(rhythm: &BenchmarkPomodoroRhythm) -> &'static str {
    match rhythm {
        BenchmarkPomodoroRhythm::Count { .. } => "count",
        BenchmarkPomodoroRhythm::Sequence { .. } => "sequence",
    }
}

async fn insert_segment(
    tx: &mut Transaction<'_, Sqlite>,
    segment: &BenchmarkPomodoroSegmentSeed,
) -> Result<(), String> {
    let config_row = sqlx::query(
        "SELECT rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes
         FROM calendar_event_pomodoro_configs
         WHERE event_id = ?",
    )
    .bind(&segment.event_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load benchmark pomodoro config for run: {e}"))?;
    let rhythm_kind: String = config_row
        .try_get("rhythm_kind")
        .map_err(|e| format!("read rhythm_kind: {e}"))?;
    let rhythm_source: String = config_row
        .try_get("rhythm_source")
        .map_err(|e| format!("read rhythm_source: {e}"))?;
    let preset_key: Option<String> = config_row
        .try_get("preset_key")
        .map_err(|e| format!("read preset_key: {e}"))?;
    let idle_timeout_minutes: Option<i64> = config_row
        .try_get("idle_timeout_minutes")
        .map_err(|e| format!("read idle_timeout_minutes: {e}"))?;

    sqlx::query(
        "INSERT OR IGNORE INTO pomodoro_runs
            (id, event_id, original_event_id, event_date, planned_start, planned_end,
             started_at, ended_at, end_reason, rhythm_kind, rhythm_source, preset_key,
             idle_timeout_minutes, last_heartbeat, start_trigger)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'completed', ?, ?, ?, ?, ?, 'manual')",
    )
    .bind(&segment.run_id)
    .bind(&segment.event_id)
    .bind(&segment.event_id)
    .bind(&segment.event_date)
    .bind(&segment.planned_start)
    .bind(&segment.planned_end)
    .bind(
        segment
            .actual_start
            .as_deref()
            .unwrap_or(&segment.planned_start),
    )
    .bind(&segment.actual_end)
    .bind(&rhythm_kind)
    .bind(&rhythm_source)
    .bind(&preset_key)
    .bind(idle_timeout_minutes)
    .bind(
        segment
            .actual_end
            .as_deref()
            .unwrap_or(&segment.planned_end),
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("seed benchmark pomodoro run: {e}"))?;

    match rhythm_kind.as_str() {
        "count" => {
            sqlx::query(
                "INSERT OR IGNORE INTO pomodoro_run_count_rhythms
                    (run_id, focus_duration_minutes, short_break_minutes, long_break_minutes,
                     long_break_after_focus_count)
                 SELECT ?, focus_duration_minutes, short_break_minutes, long_break_minutes,
                        long_break_after_focus_count
                 FROM calendar_event_pomodoro_config_count_rhythms
                 WHERE event_id = ?",
            )
            .bind(&segment.run_id)
            .bind(&segment.event_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("snapshot benchmark count rhythm: {e}"))?;
        }
        "sequence" => {
            sqlx::query(
                "INSERT OR IGNORE INTO pomodoro_run_sequence_steps
                    (run_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes)
                 SELECT ?, step_index, focus_duration_minutes, break_phase, break_duration_minutes
                 FROM calendar_event_pomodoro_config_sequence_steps
                 WHERE event_id = ?",
            )
            .bind(&segment.run_id)
            .bind(&segment.event_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("snapshot benchmark sequence rhythm: {e}"))?;
        }
        _ => return Err(format!("invalid benchmark rhythm kind: {rhythm_kind}")),
    }

    if let Some(actual_end) = &segment.actual_end {
        sqlx::query(
            "UPDATE pomodoro_runs
             SET planned_end = CASE WHEN planned_end < ? THEN ? ELSE planned_end END,
                 ended_at = CASE WHEN ended_at IS NULL OR ended_at < ? THEN ? ELSE ended_at END,
                 last_heartbeat = CASE WHEN last_heartbeat < ? THEN ? ELSE last_heartbeat END
             WHERE id = ?",
        )
        .bind(&segment.planned_end)
        .bind(&segment.planned_end)
        .bind(actual_end)
        .bind(actual_end)
        .bind(actual_end)
        .bind(actual_end)
        .bind(&segment.run_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("update benchmark pomodoro run bounds: {e}"))?;
    }

    sqlx::query(
        "INSERT OR REPLACE INTO pomodoro_segments
            (id, event_id, event_date, run_id, rhythm_position, phase,
             planned_start, planned_end, actual_start, actual_end, status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&segment.id)
    .bind(&segment.event_id)
    .bind(&segment.event_date)
    .bind(&segment.run_id)
    .bind(segment.rhythm_position)
    .bind(&segment.phase)
    .bind(&segment.planned_start)
    .bind(&segment.planned_end)
    .bind(&segment.actual_start)
    .bind(&segment.actual_end)
    .bind(&segment.status)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("seed benchmark pomodoro segment: {e}"))?;
    for (index, pause) in segment.pauses.iter().enumerate() {
        sqlx::query(
            "INSERT INTO pomodoro_pauses
                (id, segment_id, started_at, ended_at, reason)
             VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?)",
        )
        .bind(&segment.id)
        .bind(&pause.started_at)
        .bind(&pause.ended_at)
        .bind(&pause.reason)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("seed benchmark pomodoro pause {index}: {e}"))?;
    }
    Ok(())
}

fn validate_payload(payload: &BenchmarkPomodoroHistoryPayload) -> Result<(), String> {
    for config in &payload.configs {
        require_non_empty(&config.event_id, "config.event_id")?;
        validate_config(&config.config)?;
    }
    for segment in &payload.segments {
        require_non_empty(&segment.id, "segment.id")?;
        require_non_empty(&segment.event_id, "segment.event_id")?;
        require_non_empty(&segment.event_date, "segment.event_date")?;
        require_non_empty(&segment.run_id, "segment.run_id")?;
        require_positive(segment.rhythm_position, "rhythm_position")?;
        validate_phase(&segment.phase)?;
        require_non_empty(&segment.planned_start, "planned_start")?;
        require_non_empty(&segment.planned_end, "planned_end")?;
        validate_status(&segment.status)?;
        for pause in &segment.pauses {
            require_non_empty(&pause.started_at, "pause.started_at")?;
            if !matches!(pause.reason.as_str(), "idle" | "manual" | "suspend") {
                return Err(format!("invalid pomodoro pause reason: {}", pause.reason));
            }
        }
    }
    Ok(())
}

fn validate_config(config: &BenchmarkPomodoroConfig) -> Result<(), String> {
    validate_source_and_preset(&config.rhythm_source, config.preset_key.as_deref())?;
    if let Some(idle_timeout_minutes) = config.idle_timeout_minutes {
        require_positive(idle_timeout_minutes, "idle_timeout_minutes")?;
    }
    match &config.rhythm {
        BenchmarkPomodoroRhythm::Count {
            focus_duration_minutes,
            short_break_minutes,
            long_break_minutes,
            long_break_after_focus_count,
        } => {
            require_positive(*focus_duration_minutes, "focus_duration_minutes")?;
            require_positive(*short_break_minutes, "short_break_minutes")?;
            require_positive(*long_break_minutes, "long_break_minutes")?;
            validate_rhythm_position_count(*long_break_after_focus_count)?;
        }
        BenchmarkPomodoroRhythm::Sequence { steps } => {
            if steps.is_empty() {
                return Err("sequence rhythm requires at least one step".into());
            }
            validate_rhythm_position_count(steps.len() as i64)?;
            for (index, step) in steps.iter().enumerate() {
                require_positive(
                    step.focus_duration_minutes,
                    &format!("sequence step {index} focus_duration_minutes"),
                )?;
                validate_break_phase(&step.break_phase)?;
                require_positive(
                    step.break_duration_minutes,
                    &format!("sequence step {index} break_duration_minutes"),
                )?;
            }
        }
    }
    Ok(())
}

fn validate_source_and_preset(source: &str, preset_key: Option<&str>) -> Result<(), String> {
    match (source, preset_key) {
        ("preset", Some("adaptive" | "creative" | "balanced" | "deep" | "extended")) => Ok(()),
        ("preset", Some(other)) => Err(format!("invalid pomodoro preset key: {other}")),
        ("preset", None) => Err("preset pomodoro config requires a preset key".into()),
        ("custom", None) => Ok(()),
        ("custom", Some(_)) => Err("custom pomodoro config cannot include a preset key".into()),
        (other, _) => Err(format!("invalid pomodoro rhythm source: {other}")),
    }
}

fn validate_rhythm_position_count(value: i64) -> Result<(), String> {
    if !(1..=12).contains(&value) {
        return Err("rhythm position count must be between 1 and 12".into());
    }
    Ok(())
}

fn require_non_empty(value: &str, name: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{name} is required"));
    }
    Ok(())
}

fn require_positive(value: i64, name: &str) -> Result<(), String> {
    if value <= 0 {
        return Err(format!("{name} must be positive"));
    }
    Ok(())
}

fn validate_phase(phase: &str) -> Result<(), String> {
    match phase {
        "focus" | "short_break" | "long_break" => Ok(()),
        _ => Err(format!("invalid pomodoro segment phase: {phase}")),
    }
}

fn validate_break_phase(phase: &str) -> Result<(), String> {
    match phase {
        "short_break" | "long_break" => Ok(()),
        _ => Err(format!("invalid pomodoro break phase: {phase}")),
    }
}

fn validate_status(status: &str) -> Result<(), String> {
    match status {
        "completed" | "interrupted" | "active" => Ok(()),
        _ => Err(format!("invalid pomodoro segment status: {status}")),
    }
}
