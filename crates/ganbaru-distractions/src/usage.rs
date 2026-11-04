//! Usage sample and block event normalization, and their bounded SQLite writes.

pub mod totals;

use crate::contracts::{
    DistractionsDesktopBlockEventInput, DistractionsRuntimeState, DistractionsUsageSampleInput,
    DistractionsUsageSampleRow, NormalizedDesktopBlockEvent,
};
use crate::limits::{is_valid_local_date, normalize_usage_host};
use crate::rules::{is_protected_desktop_app_name, normalize_app_candidate_name};
use ganbaru_sync::domains::distractions::{
    BLOCK_DECISIONS, BLOCK_PHASES, BLOCK_SOURCE_TYPES, MAX_BLOCK_TEXT_CHARS,
    SNAPSHOT_BLOCKER_MODES, SNAPSHOT_RULE_KINDS,
};
use ganbaru_sync::manifest::MAX_TIMESTAMP_BYTES;
use ganbaru_sync_contracts::RowKey;
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};

/// Days of usage history kept, far beyond the weekly budget window so yearly trends survive.
pub const USAGE_RETENTION_DAYS: u64 = 400;

/// Days a sample's local date may run ahead of the receiving device's UTC date. Local dates
/// range from UTC-12 to UTC+14, so a correct clock is at most one calendar day ahead of UTC.
pub const USAGE_FUTURE_TOLERANCE_DAYS: u64 = 1;

/// Inclusive range of `yyyy-mm-dd` local dates accepted into usage history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageDateWindow {
    pub earliest: String,
    pub latest: String,
}

impl UsageDateWindow {
    /// Whether `local_date` is a valid local date inside the window.
    pub fn contains(&self, local_date: &str) -> bool {
        is_valid_local_date(local_date)
            && local_date >= self.earliest.as_str()
            && local_date <= self.latest.as_str()
    }
}

/// Usage history window for a device whose current UTC date is `today_utc`.
pub fn usage_date_window(today_utc: chrono::NaiveDate) -> UsageDateWindow {
    let format = |date: chrono::NaiveDate| date.format("%Y-%m-%d").to_string();
    let earliest = today_utc
        .checked_sub_days(chrono::Days::new(USAGE_RETENTION_DAYS))
        .unwrap_or(chrono::NaiveDate::MIN);
    let latest = today_utc
        .checked_add_days(chrono::Days::new(USAGE_FUTURE_TOLERANCE_DAYS))
        .unwrap_or(chrono::NaiveDate::MAX);
    UsageDateWindow {
        earliest: format(earliest),
        latest: format(latest),
    }
}

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
        "INSERT OR IGNORE INTO distractions_block_event_rule_snapshots
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

/// A block event another process left in the device spool for the app to record in the vault,
/// with the single rule snapshot it carries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpooledBlockEvent {
    pub id: String,
    pub run_id: Option<String>,
    pub occurred_at: String,
    pub source_type: String,
    pub source_key: String,
    pub display_name: Option<String>,
    pub phase: Option<String>,
    pub decision: String,
    /// The rule that blocked the source, recorded on the event and its snapshot.
    pub rule_id: Option<String>,
    pub rule_kind: Option<String>,
    pub rule_label: Option<String>,
    pub blocker_mode: Option<String>,
}

fn bounded_text(value: &str, max_chars: usize, what: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.contains('\0') || value.chars().count() > max_chars {
        return Err(format!("spooled block event {what} is invalid"));
    }
    Ok(())
}

/// Whether `key` is a row key the vault's value triggers and the sync engine both accept.
fn is_row_key(key: &str) -> bool {
    !key.trim().is_empty() && RowKey::new(key).is_ok()
}

fn optional_choice(value: Option<&str>, allowed: &[&str], what: &str) -> Result<(), String> {
    match value {
        Some(value) if !allowed.contains(&value) => {
            Err(format!("spooled block event {what} is invalid"))
        }
        _ => Ok(()),
    }
}

/// Checks a spooled block event against the bounds the vault's capture triggers enforce and the
/// values the replicated table accepts, so a malformed spool row never aborts a vault write.
pub fn validate_spooled_block_event(event: &SpooledBlockEvent) -> Result<(), String> {
    if !is_row_key(&event.id) {
        return Err("spooled block event id is invalid".into());
    }
    if let Some(run_id) = &event.run_id
        && !is_row_key(run_id)
    {
        return Err("spooled block event run is invalid".into());
    }
    if event.occurred_at.len() > MAX_TIMESTAMP_BYTES {
        return Err("spooled block event time is invalid".into());
    }
    chrono::DateTime::parse_from_rfc3339(&event.occurred_at)
        .map_err(|_| "spooled block event time is invalid".to_string())?;
    optional_choice(Some(&event.source_type), BLOCK_SOURCE_TYPES, "source type")?;
    bounded_text(&event.source_key, MAX_BLOCK_TEXT_CHARS, "source key")?;
    if event.source_key.trim() != event.source_key
        || event.source_key.contains("://")
        || event.source_key.contains('/')
        || (event.source_type == "browser"
            && normalize_usage_host(&event.source_key).as_deref() != Some(&event.source_key))
    {
        return Err("spooled block event source key is invalid".into());
    }
    for (value, what) in [
        (&event.display_name, "display name"),
        (&event.rule_id, "rule"),
        (&event.rule_label, "rule label"),
    ] {
        if let Some(value) = value {
            bounded_text(value, MAX_BLOCK_TEXT_CHARS, what)?;
        }
    }
    optional_choice(event.phase.as_deref(), BLOCK_PHASES, "phase")?;
    optional_choice(Some(&event.decision), BLOCK_DECISIONS, "decision")?;
    optional_choice(event.rule_kind.as_deref(), SNAPSHOT_RULE_KINDS, "rule kind")?;
    optional_choice(
        event.blocker_mode.as_deref(),
        SNAPSHOT_BLOCKER_MODES,
        "blocker mode",
    )
}

/// Why a spooled block event was not recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpooledBlockEventError {
    /// The event breaks a rule of the vault, so every later attempt would fail the same way.
    Refused(String),
    /// The vault could not be written now, for example while busy; a later attempt may succeed.
    Failed(String),
}

impl std::fmt::Display for SpooledBlockEventError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(message) | Self::Failed(message) => formatter.write_str(message),
        }
    }
}

/// SQLite primary result code of a constraint, CHECK, foreign key, or `RAISE(ABORT)` failure.
const SQLITE_CONSTRAINT: i32 = 19;

/// Classifies a vault error: a constraint or trigger refusal is permanent, anything else is not.
fn spooled_write_error(error: sqlx::Error, what: &str) -> SpooledBlockEventError {
    let refused = error
        .as_database_error()
        .and_then(|database| database.code())
        .and_then(|code| code.parse::<i32>().ok())
        .is_some_and(|code| code & 0xff == SQLITE_CONSTRAINT);
    let message = format!("{what}: {error}");
    if refused {
        SpooledBlockEventError::Refused(message)
    } else {
        SpooledBlockEventError::Failed(message)
    }
}

/// Records one spooled block event and its rule snapshot inside a savepoint, so a refused event
/// leaves nothing behind and the caller's transaction stays usable. Recording an event the vault
/// already holds changes nothing, so a spool batch that committed but was not removed records
/// again harmlessly. A run the vault no longer holds leaves the event without a run. A spooled
/// event never names a segment, so it can never reference a segment without its run.
pub async fn record_spooled_block_event(
    connection: &mut SqliteConnection,
    event: &SpooledBlockEvent,
) -> Result<(), SpooledBlockEventError> {
    validate_spooled_block_event(event).map_err(SpooledBlockEventError::Refused)?;
    let mut savepoint = sqlx::Connection::begin(&mut *connection)
        .await
        .map_err(|error| spooled_write_error(error, "begin spooled block event"))?;
    match write_spooled_block_event(&mut savepoint, event).await {
        Ok(()) => savepoint
            .commit()
            .await
            .map_err(|error| spooled_write_error(error, "commit spooled block event")),
        Err(error) => {
            savepoint.rollback().await.map_err(|rollback| {
                spooled_write_error(rollback, "roll back spooled block event")
            })?;
            Err(error)
        }
    }
}

async fn write_spooled_block_event(
    connection: &mut SqliteConnection,
    event: &SpooledBlockEvent,
) -> Result<(), SpooledBlockEventError> {
    let run_id = match &event.run_id {
        Some(run_id) => {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pomodoro_runs WHERE id = ?)")
                    .bind(run_id)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(|error| spooled_write_error(error, "find spooled block event run"))?;
            exists.then_some(run_id.as_str())
        }
        None => None,
    };
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO distractions_block_events
            (id, run_id, segment_id, occurred_at, source_type, source_key,
             display_name, phase, decision, rule_id, category_id, created_at)
         VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, NULL, ?)",
    )
    .bind(&event.id)
    .bind(run_id)
    .bind(&event.occurred_at)
    .bind(&event.source_type)
    .bind(&event.source_key)
    .bind(&event.display_name)
    .bind(&event.phase)
    .bind(&event.decision)
    .bind(&event.rule_id)
    .bind(&event.occurred_at)
    .execute(&mut *connection)
    .await
    .map_err(|error| spooled_write_error(error, "record spooled block event"))?
    .rows_affected();
    if inserted == 0 {
        return Ok(());
    }
    sqlx::query(
        "INSERT OR IGNORE INTO distractions_block_event_rule_snapshots
            (block_event_id, rule_id, rule_kind, rule_label, environment_id, blocker_mode)
         VALUES (?, ?, ?, ?, NULL, ?)",
    )
    .bind(&event.id)
    .bind(&event.rule_id)
    .bind(&event.rule_kind)
    .bind(&event.rule_label)
    .bind(&event.blocker_mode)
    .execute(&mut *connection)
    .await
    .map_err(|error| spooled_write_error(error, "record spooled block event rule snapshot"))?;
    Ok(())
}

#[cfg(test)]
mod tests;
