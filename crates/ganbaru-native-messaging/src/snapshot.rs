//! Device snapshot discovery, vault path checks, and runtime freshness.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use ganbaru_distractions_authority::{ActivePhase, DistractionsRuntimeState, RuntimeAuthority};
use serde::{Deserialize, Deserializer};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::{DistractionsConfig, default_config, read_rules};
use crate::now_utc;

const STATE_FILE: &str = "distractions-state.json";
const LIMIT_STATE_FILE: &str = "distractions-limit-state.json";
const APP_STATE_FILE: &str = "app-state.json";
/// Device-local rules the app publishes from the active vault's SQLite rules.
const RULES_FILE: &str = "distractions-rules.json";
const APP_SQLITE_FILE: &str = "ganbaru.sqlite";
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

#[derive(Debug)]
pub(super) struct StateSnapshot {
    pub(super) config_dir: Option<PathBuf>,
    pub(super) vault_path: Option<PathBuf>,
    pub(super) config: DistractionsConfig,
    pub(super) runtime: Option<DistractionsRuntimeState>,
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
    let config = config_dir
        .as_ref()
        .zip(vault_path.as_ref())
        .and_then(|(dir, vault)| read_rules(&dir.join(RULES_FILE), vault))
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
    if let Ok(dir) = std::env::var("GANBARU_CONFIG_DIR") {
        return vec![PathBuf::from(dir)];
    }

    let ids = ["org.opengrimoire.ganbaru", "org.opengrimoire.ganbaru.dev"];
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

pub(super) fn read_runtime_state(path: &std::path::Path) -> Option<DistractionsRuntimeState> {
    DistractionsRuntimeState::parse(&read_bounded_state(path)?).ok()
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

/// Runtime authority the host acts on at one instant, after its freshness and validity bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimeStatus {
    /// The file's authority, or inactive when rules are disabled or the file is missing, stale,
    /// or past its validity.
    pub(super) authority: RuntimeAuthority,
    /// Seconds left in the active phase at the check, frozen while paused.
    pub(super) remaining_seconds: Option<i64>,
    /// Why the file's authority was not accepted.
    pub(super) reason: Option<String>,
}

impl RuntimeStatus {
    fn inactive(reason: &str) -> Self {
        Self {
            authority: RuntimeAuthority::Inactive {},
            remaining_seconds: None,
            reason: Some(reason.to_string()),
        }
    }

    /// The accepted phase, if one is current.
    pub(super) fn active_phase(&self) -> Option<&ActivePhase> {
        self.authority.active_phase()
    }
}

pub(super) fn runtime_status(snapshot: &StateSnapshot) -> RuntimeStatus {
    runtime_status_at(snapshot, now_utc())
}

pub(super) fn runtime_status_at(
    snapshot: &StateSnapshot,
    checked_at: DateTime<Utc>,
) -> RuntimeStatus {
    if !snapshot.config.enabled {
        return RuntimeStatus::inactive("distraction rules are disabled");
    }
    let Some(runtime) = &snapshot.runtime else {
        return RuntimeStatus::inactive("no runtime state");
    };
    let Ok(updated_at) = DateTime::parse_from_rfc3339(&runtime.updated_at) else {
        return RuntimeStatus::inactive("runtime state has invalid timestamp");
    };
    let age = checked_at - updated_at.with_timezone(&Utc);
    let active = runtime.authority.active_phase();
    let stale_state_seconds = if active.is_some() {
        ACTIVE_STATE_STALE_SECONDS
    } else {
        STALE_STATE_SECONDS
    };
    if age < chrono::Duration::zero() || age > chrono::Duration::seconds(stale_state_seconds) {
        return RuntimeStatus::inactive("runtime state is stale");
    }
    if runtime.validity_expired(updated_at.timestamp_millis(), checked_at.timestamp_millis()) {
        return RuntimeStatus::inactive("accepted phase validity expired");
    }
    let remaining_seconds = active.map(|active| {
        if active.is_paused() {
            active.remaining_seconds
        } else {
            (active.remaining_seconds - age.num_seconds()).max(0)
        }
    });
    RuntimeStatus {
        authority: runtime.authority.clone(),
        remaining_seconds,
        reason: None,
    }
}

/// Whether regular blocking rules apply, through the rule shared with desktop close authorization.
pub(super) fn should_enforce(config: &DistractionsConfig, status: &RuntimeStatus) -> bool {
    config.enabled && status.authority.enforces_blocking(config.blocking)
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
            database_path: "/tmp/vault/ganbaru.sqlite".into(),
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
