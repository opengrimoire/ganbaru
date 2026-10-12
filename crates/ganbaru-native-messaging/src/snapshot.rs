//! Device snapshot discovery, vault path checks, and runtime freshness.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Deserializer};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::{DistractionsConfig, default_config, read_config};
use crate::{NativeResponse, now_utc};

const STATE_FILE: &str = "distractions-state.json";
const LIMIT_STATE_FILE: &str = "distractions-limit-state.json";
const APP_STATE_FILE: &str = "app-state.json";
const CONFIG_FILE: &str = "config.json";
const APP_SQLITE_FILE: &str = "ganbaru-ai.sqlite";
const STALE_STATE_SECONDS: i64 = 75;
const ACTIVE_STATE_STALE_SECONDS: i64 = 45;
const LIMIT_STATE_STALE_SECONDS: i64 = 20;
const MAX_DEVICE_STATE_BYTES: u64 = 1024 * 1024;

fn read_bounded_state(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_DEVICE_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_DEVICE_STATE_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RuntimeState {
    pub(super) active: bool,
    pub(super) paused: bool,
    #[serde(deserialize_with = "required_nullable")]
    pub(super) pause_reason: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub(super) active_run_id: Option<String>,
    pub(super) phase: String,
    #[serde(deserialize_with = "required_nullable")]
    pub(super) remaining_seconds: Option<i64>,
    pub(super) updated_at: String,
    #[serde(default)]
    pub(super) valid_until_ms: Option<i64>,
}

#[derive(Debug)]
pub(super) struct StateSnapshot {
    pub(super) config_dir: Option<PathBuf>,
    pub(super) vault_path: Option<PathBuf>,
    pub(super) config: DistractionsConfig,
    pub(super) runtime: Option<RuntimeState>,
    pub(super) limit_state: Option<LimitState>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LimitState {
    pub(super) local_date: String,
    pub(super) week_start_local_date: String,
    pub(super) updated_at: String,
    pub(super) database_path: String,
    #[serde(default)]
    pub(super) configuration_digest: Option<String>,
    pub(super) limits: Vec<LimitStateItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppState {
    #[serde(deserialize_with = "required_nullable")]
    active_vault_path: Option<String>,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LimitStateItem {
    pub(super) id: String,
    pub(super) period: String,
    pub(super) window_start_local_date: String,
    pub(super) window_end_local_date: String,
    pub(super) used_seconds: i64,
    pub(super) limit_seconds: i64,
    pub(super) remaining_seconds: i64,
    pub(super) exhausted: bool,
}

pub(super) fn load_snapshot() -> StateSnapshot {
    let config_dir = config_dir_candidates().into_iter().find(|dir| {
        dir.join(STATE_FILE).exists()
            || dir.join(LIMIT_STATE_FILE).exists()
            || dir.join(APP_STATE_FILE).exists()
    });
    let vault_path = config_dir
        .as_ref()
        .and_then(|dir| read_app_state(&dir.join(APP_STATE_FILE)))
        .and_then(active_vault_path_from_state);
    let config = vault_path
        .as_ref()
        .and_then(|dir| read_config(&dir.join(CONFIG_FILE)))
        .unwrap_or_else(default_config);
    let runtime = config_dir
        .as_ref()
        .and_then(|dir| read_runtime_state(&dir.join(STATE_FILE)));
    let limit_state = config_dir
        .as_ref()
        .and_then(|dir| read_limit_state(&dir.join(LIMIT_STATE_FILE), vault_path.as_deref()))
        .filter(|state| {
            state.configuration_digest.is_some()
                && state.configuration_digest == config.limit_configuration_digest
        });
    StateSnapshot {
        config_dir,
        vault_path,
        config,
        runtime,
        limit_state,
    }
}

pub(super) fn config_dir_candidates() -> Vec<PathBuf> {
    if let Ok(dir) = std::env::var("GANBARU_AI_CONFIG_DIR") {
        return vec![PathBuf::from(dir)];
    }

    let ids = [
        "org.opengrimoire.ganbaruai",
        "org.opengrimoire.ganbaruai.dev",
    ];
    let mut candidates = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
        if let Some(base) = base {
            for id in ids {
                candidates.push(base.join(id));
            }
        }
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let base = home.join("Library").join("Application Support");
        for id in ids {
            candidates.push(base.join(id));
        }
    }

    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        for id in ids {
            candidates.push(appdata.join(id));
        }
    }

    candidates
}

fn read_runtime_state(path: &std::path::Path) -> Option<RuntimeState> {
    let contents = read_bounded_state(path)?;
    serde_json::from_str(&contents).ok()
}

fn read_app_state(path: &std::path::Path) -> Option<AppState> {
    let contents = read_bounded_state(path)?;
    serde_json::from_str(&contents).ok()
}

fn active_vault_path_from_state(state: AppState) -> Option<PathBuf> {
    let path = PathBuf::from(state.active_vault_path?);
    if !path.is_absolute() || !path.is_dir() {
        return None;
    }
    path.join("vault.json").exists().then_some(path)
}

fn read_limit_state(path: &std::path::Path, vault_path: Option<&Path>) -> Option<LimitState> {
    let contents = read_bounded_state(path)?;
    let state: LimitState = serde_json::from_str(&contents).ok()?;
    limit_state_is_fresh(&state, vault_path).then_some(state)
}

pub(super) fn runtime_status(
    snapshot: &StateSnapshot,
) -> (bool, String, Option<i64>, Option<String>) {
    runtime_status_at(snapshot, now_utc())
}

pub(super) fn runtime_status_at(
    snapshot: &StateSnapshot,
    checked_at: DateTime<Utc>,
) -> (bool, String, Option<i64>, Option<String>) {
    if !snapshot.config.enabled {
        return (
            false,
            "inactive".to_string(),
            None,
            Some("distraction rules are disabled".to_string()),
        );
    }

    let Some(runtime) = &snapshot.runtime else {
        return (
            false,
            "inactive".to_string(),
            None,
            Some("no runtime state".to_string()),
        );
    };

    let Ok(updated_at) = DateTime::parse_from_rfc3339(&runtime.updated_at) else {
        return (
            false,
            "inactive".to_string(),
            None,
            Some("runtime state has invalid timestamp".to_string()),
        );
    };
    let age = checked_at - updated_at.with_timezone(&Utc);
    let age_seconds = age.num_seconds();
    let stale_state_seconds = if runtime.active {
        ACTIVE_STATE_STALE_SECONDS
    } else {
        STALE_STATE_SECONDS
    };
    if age < chrono::Duration::zero() || age > chrono::Duration::seconds(stale_state_seconds) {
        return (
            false,
            "inactive".to_string(),
            None,
            Some("runtime state is stale".to_string()),
        );
    }

    if runtime
        .valid_until_ms
        .is_some_and(|deadline| deadline < 0 || checked_at.timestamp_millis() >= deadline)
        || (runtime.active
            && !runtime.paused
            && runtime.remaining_seconds.is_some_and(|remaining| {
                remaining < 0
                    || checked_at.timestamp_millis()
                        >= updated_at
                            .timestamp_millis()
                            .saturating_add(remaining.saturating_mul(1000))
            }))
    {
        return (
            false,
            "inactive".into(),
            None,
            Some("accepted phase validity expired".into()),
        );
    }

    let remaining_seconds = runtime.remaining_seconds.map(|remaining| {
        if runtime.paused {
            remaining
        } else {
            (remaining - age_seconds).max(0)
        }
    });
    (
        runtime.active,
        runtime.phase.clone(),
        remaining_seconds,
        None,
    )
}

pub(super) fn should_enforce(snapshot: &StateSnapshot, response: &NativeResponse) -> bool {
    if !response.active {
        return false;
    }

    if snapshot
        .runtime
        .as_ref()
        .is_some_and(pause_should_suspend_enforcement)
        && snapshot.config.pause_during_focus_pause
    {
        return false;
    }

    match response.phase.as_str() {
        "focus" => snapshot.config.block_during_focus,
        "short_break" => snapshot.config.block_during_short_breaks,
        "long_break" => snapshot.config.block_during_long_breaks,
        _ => false,
    }
}

fn pause_should_suspend_enforcement(runtime: &RuntimeState) -> bool {
    runtime.paused && !matches!(runtime.pause_reason.as_deref(), Some("idle" | "suspend"))
}

pub(super) fn is_valid_local_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
        && !value.starts_with("0000")
        && NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
}

fn database_path_is_allowed(path: &Path, vault_path: Option<&Path>) -> bool {
    if !path.is_absolute()
        || path.file_name().and_then(|name| name.to_str()) != Some(APP_SQLITE_FILE)
    {
        return false;
    }
    match vault_path {
        Some(vault_path) => path == vault_path.join(APP_SQLITE_FILE),
        None => true,
    }
}

pub(super) fn usage_db_path(vault_path: &Path, limit_state: Option<&LimitState>) -> PathBuf {
    limit_state
        .map(|state| PathBuf::from(&state.database_path))
        .filter(|path| database_path_is_allowed(path, Some(vault_path)))
        .unwrap_or_else(|| vault_path.join(APP_SQLITE_FILE))
}

fn limit_state_is_fresh(state: &LimitState, vault_path: Option<&Path>) -> bool {
    let now = now_utc();
    let Ok(zone) = jiff::tz::TimeZone::try_system() else {
        return false;
    };
    let Ok(timestamp) = jiff::Timestamp::from_millisecond(now.timestamp_millis()) else {
        return false;
    };
    limit_state_is_fresh_at(
        state,
        vault_path,
        now,
        &zone.to_datetime(timestamp).date().to_string(),
    )
}

fn limit_state_is_fresh_at(
    state: &LimitState,
    vault_path: Option<&Path>,
    checked_at: DateTime<Utc>,
    local_date: &str,
) -> bool {
    if !is_valid_local_date(&state.local_date) {
        return false;
    }
    if !is_valid_local_date(&state.week_start_local_date) {
        return false;
    }
    let Ok(date) = NaiveDate::parse_from_str(&state.local_date, "%Y-%m-%d") else {
        return false;
    };
    let week = date.checked_sub_signed(chrono::Duration::days(i64::from(
        date.weekday().num_days_from_monday(),
    )));
    if state.local_date != local_date
        || state.limits.len() > 512
        || week
            .is_none_or(|week| week.format("%Y-%m-%d").to_string() != state.week_start_local_date)
    {
        return false;
    }
    let mut identities = std::collections::HashSet::new();
    if state.limits.iter().any(|limit| {
        !matches!(limit.period.as_str(), "day" | "week")
            || !is_valid_local_date(&limit.window_start_local_date)
            || !is_valid_local_date(&limit.window_end_local_date)
            || limit.window_start_local_date > limit.window_end_local_date
            || limit.window_end_local_date != state.local_date
            || limit.window_start_local_date
                != if limit.period == "day" {
                    &state.local_date
                } else {
                    &state.week_start_local_date
                }
                .as_str()
            || limit.used_seconds < 0
            || limit.limit_seconds <= 0
            || limit.remaining_seconds
                != limit
                    .limit_seconds
                    .saturating_sub(limit.used_seconds)
                    .max(0)
            || limit.exhausted != (limit.used_seconds >= limit.limit_seconds)
            || !identities.insert((&limit.id, &limit.period))
    }) {
        return false;
    }
    if !database_path_is_allowed(Path::new(&state.database_path), vault_path) {
        return false;
    }
    let Ok(updated_at) = DateTime::parse_from_rfc3339(&state.updated_at) else {
        return false;
    };
    let age = checked_at - updated_at.with_timezone(&Utc);
    age >= chrono::Duration::zero() && age <= chrono::Duration::seconds(LIMIT_STATE_STALE_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_require_the_current_day_consistent_arithmetic_and_nonfuture_freshness() {
        let checked_at = "2026-10-02T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let vault = Path::new("/tmp/vault");
        let mut state = LimitState {
            local_date: "2026-10-02".into(),
            week_start_local_date: "2026-09-28".into(),
            updated_at: checked_at.to_rfc3339(),
            database_path: "/tmp/vault/ganbaru-ai.sqlite".into(),
            configuration_digest: None,
            limits: vec![LimitStateItem {
                id: "habit".into(),
                period: "day".into(),
                window_start_local_date: "2026-10-02".into(),
                window_end_local_date: "2026-10-02".into(),
                used_seconds: 60,
                limit_seconds: 60,
                remaining_seconds: 0,
                exhausted: true,
            }],
        };
        assert!(limit_state_is_fresh_at(
            &state,
            Some(vault),
            checked_at,
            "2026-10-02"
        ));
        assert!(!limit_state_is_fresh_at(
            &state,
            Some(vault),
            checked_at,
            "2026-10-03"
        ));
        assert!(!limit_state_is_fresh_at(
            &state,
            Some(vault),
            checked_at - chrono::Duration::milliseconds(1),
            "2026-10-02"
        ));
        assert!(!limit_state_is_fresh_at(
            &state,
            Some(vault),
            checked_at + chrono::Duration::milliseconds(20_001),
            "2026-10-02"
        ));
        state.limits[0].exhausted = false;
        assert!(!limit_state_is_fresh_at(
            &state,
            Some(vault),
            checked_at,
            "2026-10-02"
        ));
        assert!(!is_valid_local_date("2026-02-29"));
    }
}
