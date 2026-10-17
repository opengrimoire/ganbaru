//! Usage sample and desktop block event normalization, and their bounded SQLite writes.

use crate::contracts::{
    DistractionsDesktopBlockEventInput, DistractionsRuntimeState, DistractionsUsageSampleInput,
    DistractionsUsageSampleRow, NormalizedDesktopBlockEvent,
};
use crate::limits::{is_valid_local_date, normalize_usage_host};
use crate::rules::{is_protected_desktop_app_name, normalize_app_candidate_name};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// Map the active runtime state to the phase recorded on a block event, or none when inactive.
pub fn block_event_phase_from_runtime(
    runtime: Option<&DistractionsRuntimeState>,
) -> Option<String> {
    let runtime = runtime?;
    if !runtime.active {
        return None;
    }
    if runtime.paused {
        return match runtime.pause_reason.as_deref() {
            Some("idle") => Some("idle_pause".to_string()),
            Some("suspend") => Some("suspend_pause".to_string()),
            Some("manual") | None => Some("manual_pause".to_string()),
            Some(_) => None,
        };
    }
    match runtime.phase.as_str() {
        "focus" | "short_break" | "long_break" => Some(runtime.phase.clone()),
        _ => None,
    }
}

fn normalize_usage_source_key(source_type: &str, source_key: &str) -> Option<String> {
    match source_type {
        "website" => normalize_usage_host(source_key),
        "desktop-app" | "mobile-app" => {
            normalize_app_candidate_name(source_key).map(|name| name.to_lowercase())
        }
        _ => None,
    }
}

fn normalize_usage_display_name(value: Option<String>) -> Option<String> {
    value.and_then(|name| {
        let normalized = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.chars().take(120).collect())
        }
    })
}

pub fn normalize_desktop_block_event(
    input: DistractionsDesktopBlockEventInput,
) -> Result<NormalizedDesktopBlockEvent, String> {
    let source_name = input.process_name.as_deref().unwrap_or(&input.app_name);
    let source_key = normalize_usage_source_key("desktop-app", source_name)
        .ok_or_else(|| "desktop block event source key is invalid".to_string())?;
    if is_protected_desktop_app_name(&source_key) {
        return Err("protected desktop apps cannot be tracked".to_string());
    }
    Ok(NormalizedDesktopBlockEvent {
        source_key,
        display_name: normalize_usage_display_name(Some(input.app_name)),
        process_id: input.process_id,
    })
}

pub fn normalize_usage_sample(
    sample: DistractionsUsageSampleInput,
    fallback_id_prefix: &str,
) -> Result<DistractionsUsageSampleRow, String> {
    let source_type = match sample.source_type.as_str() {
        "website" | "desktop-app" | "mobile-app" => sample.source_type,
        other => return Err(format!("unsupported usage source type '{other}'")),
    };
    let source_key = normalize_usage_source_key(&source_type, &sample.source_key)
        .ok_or_else(|| "usage sample source key is invalid".to_string())?;
    if source_type == "desktop-app" && is_protected_desktop_app_name(&source_key) {
        return Err("protected desktop apps cannot be tracked".to_string());
    }
    if sample.elapsed_seconds <= 0 || sample.elapsed_seconds > 86_400 {
        return Err("elapsed_seconds must be between 1 and 86400".to_string());
    }
    if sample.started_at_ms < 0 {
        return Err("started_at_ms must be non-negative".to_string());
    }
    if !is_valid_local_date(&sample.local_date) {
        return Err("local_date must use yyyy-mm-dd".to_string());
    }
    let id = sample.id.unwrap_or_else(|| {
        let identity = format!(
            "{source_type}|{source_key}|{}|{}",
            sample.started_at_ms, sample.local_date
        );
        format!(
            "{fallback_id_prefix}-{:x}",
            Sha256::digest(identity.as_bytes())
        )
    });
    if id.trim().is_empty() {
        return Err("usage sample id is required".to_string());
    }

    Ok(DistractionsUsageSampleRow {
        id: id.chars().take(120).collect(),
        source_type,
        source_key,
        display_name: normalize_usage_display_name(sample.display_name),
        started_at_ms: sample.started_at_ms,
        elapsed_seconds: sample.elapsed_seconds,
        local_date: sample.local_date,
        created_at_ms: now_epoch_ms(),
    })
}

#[cfg(test)]
pub async fn insert_usage_samples(
    pool: &SqlitePool,
    samples: Vec<DistractionsUsageSampleRow>,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    for sample in samples {
        sqlx::query(
            "INSERT OR IGNORE INTO distractions_usage_samples
                (id, source_type, source_key, display_name, started_at_ms, elapsed_seconds, local_date, created_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(sample.id)
        .bind(sample.source_type)
        .bind(sample.source_key)
        .bind(sample.display_name)
        .bind(sample.started_at_ms)
        .bind(sample.elapsed_seconds)
        .bind(sample.local_date)
        .bind(sample.created_at_ms)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("record distraction usage sample: {e}"))?;
    }
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

pub async fn insert_desktop_block_event(
    pool: &SqlitePool,
    event: NormalizedDesktopBlockEvent,
    runtime: Option<&DistractionsRuntimeState>,
    occurred_at: &str,
) -> Result<(), String> {
    let phase = block_event_phase_from_runtime(runtime);
    let run_id = phase
        .as_deref()
        .and(runtime)
        .and_then(|state| state.active_run_id.clone());
    let event_id = format!(
        "desktop-block-{}-{}-{}",
        now_epoch_ms(),
        std::process::id(),
        event.process_id.unwrap_or(0),
    );

    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin desktop block event: {error}"))?;
    sqlx::query(
        "INSERT OR IGNORE INTO distractions_block_events
            (id, run_id, segment_id, occurred_at, source_type, source_key,
             display_name, phase, decision, rule_id, category_id)
         VALUES (?, ?, NULL, ?, 'desktop_app', ?, ?, ?, 'blocked', NULL, NULL)",
    )
    .bind(&event_id)
    .bind(&run_id)
    .bind(occurred_at)
    .bind(&event.source_key)
    .bind(&event.display_name)
    .bind(&phase)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("record desktop block event: {e}"))?;

    sqlx::query(
        "INSERT OR REPLACE INTO distractions_block_event_rule_snapshots
            (block_event_id, rule_id, rule_kind, rule_label, environment_id, blocker_mode)
         VALUES (?, NULL, 'desktop_app', ?, NULL, 'blacklist')",
    )
    .bind(&event_id)
    .bind(&event.display_name)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("record desktop block event rule snapshot: {e}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit desktop block event: {error}"))?;

    Ok(())
}

#[cfg(test)]
mod tests;
