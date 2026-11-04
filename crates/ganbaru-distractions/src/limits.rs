//! Pure budget matching over native, bounded daily source aggregates.

use chrono::{Datelike, Duration, NaiveDate};
use ganbaru_sync::domains::distractions::{MAX_LIMIT_TEXT_BYTES, is_usage_date};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub mod store;

pub const MAX_LIMITS: usize = 256;
const MAX_ENTRIES: usize = 2_000;
const MAX_MATCH_NAMES: usize = 64;
const MAX_MATCH_COMPARISONS: usize = 2_000_000;
pub const MAX_SOURCE_GROUPS: usize = 10_000;
pub const MAX_WINDOW_SAMPLES: i64 = 500_000;
/// Suffix comparisons one window read may spend, so a read of many rows against website rules of
/// many lengths stays well inside the age limit on enforcement actions.
pub const MAX_SUFFIX_PROBES: i64 = 2_000_000;
pub const MAX_SOURCE_KEY_BYTES: usize = 512;
const MAX_SAFE_SECONDS: i64 = 9_007_199_254_740_991;

fn enabled_default() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitEntry {
    pub id: String,
    #[serde(default)]
    pub website_host: Option<String>,
    #[serde(default)]
    pub mobile_app_name: Option<String>,
    #[serde(default)]
    pub mobile_app_package: Option<String>,
    #[serde(default)]
    pub desktop_app_name: Option<String>,
    #[serde(default)]
    pub desktop_app_match_names: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimit {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    #[serde(default)]
    pub minutes_per_day: Option<i64>,
    #[serde(default)]
    pub minutes_per_week: Option<i64>,
    pub entries: Vec<LimitEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LimitsConfig {
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    #[serde(default)]
    pub items: Vec<UsageLimit>,
}

/// Rows retain their recorded date; device timezone changes never relabel history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageSourceDay {
    pub source_type: String,
    pub source_key: String,
    pub local_date: String,
    pub elapsed_seconds: i64,
}

/// Source groups of one usage window, and whether a bound left some of them out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageWindow {
    pub sources: Vec<UsageSourceDay>,
    pub truncated: bool,
}

/// Sources the configured budgets can count, so a read never touches unrelated rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageBudgetScope {
    websites: BTreeSet<String>,
    desktop_apps: BTreeSet<String>,
    mobile_apps: BTreeSet<String>,
}

impl UsageBudgetScope {
    /// Lists the sources of every budget, including disabled ones, which stay visible in Settings.
    /// Names keep both their written and lowered forms because `matches` folds Unicode case and
    /// SQLite's `lower` folds only ASCII.
    pub fn from_rules(config: &LimitsConfig) -> Self {
        let mut scope = Self::default();
        for entry in config.items.iter().flat_map(|limit| &limit.entries) {
            scope
                .websites
                .extend(entry.website_host.as_deref().and_then(normalize_usage_host));
            let mobile_app = entry
                .mobile_app_package
                .as_ref()
                .or(entry.mobile_app_name.as_ref());
            if let Some(name) = mobile_app {
                scope.mobile_apps.insert(name.clone());
                scope.mobile_apps.insert(name.to_lowercase());
            }
            let Some(desktop_app) = &entry.desktop_app_name else {
                continue;
            };
            let desktop_names = if entry.desktop_app_match_names.is_empty() {
                std::slice::from_ref(desktop_app)
            } else {
                entry.desktop_app_match_names.as_slice()
            };
            for name in desktop_names {
                scope.desktop_apps.insert(name.clone());
                scope.desktop_apps.insert(app_name_key(name));
            }
        }
        scope
    }

    /// Whether no stored usage can count toward a budget.
    pub fn is_empty(&self) -> bool {
        self.websites.is_empty() && self.desktop_apps.is_empty() && self.mobile_apps.is_empty()
    }
}

/// Which usage read problems were already logged, so a repeating problem logs once.
#[derive(Clone, Debug, Default)]
pub struct UsageReadDiagnostics {
    last_failure: Option<String>,
    truncated_date: Option<String>,
}

impl UsageReadDiagnostics {
    /// Records a failed read and whether its message is new.
    pub fn record_failure(&mut self, error: &str) -> bool {
        if self.last_failure.as_deref() == Some(error) {
            return false;
        }
        self.last_failure = Some(error.to_owned());
        true
    }

    /// Records a successful read, so the next failure logs again.
    pub fn record_success(&mut self) {
        self.last_failure = None;
    }

    /// Records a truncated window and whether it is the first for `local_date`.
    pub fn record_truncation(&mut self, local_date: &str) -> bool {
        if self.truncated_date.as_deref() == Some(local_date) {
            return false;
        }
        self.truncated_date = Some(local_date.to_owned());
        true
    }
}

/// Sums rows into at most [`MAX_SOURCE_GROUPS`] source/day groups, in the order each group first
/// appears, so the priority of the rows carries into [`totals`]. Earlier rows keep their groups
/// when the cap is reached, and the rows of any later group are left out and reported as
/// truncation.
pub fn merge_source_groups(
    rows: impl IntoIterator<Item = UsageSourceDay>,
) -> Result<UsageWindow, String> {
    let mut positions = BTreeMap::new();
    let mut sources: Vec<UsageSourceDay> = Vec::new();
    let mut truncated = false;
    for row in rows {
        let key = (
            row.source_type.clone(),
            row.source_key.clone(),
            row.local_date.clone(),
        );
        let elapsed_seconds = row.elapsed_seconds;
        let position = match positions.get(&key) {
            Some(&position) => position,
            None if sources.len() == MAX_SOURCE_GROUPS => {
                truncated = true;
                continue;
            }
            None => {
                positions.insert(key, sources.len());
                sources.push(UsageSourceDay {
                    elapsed_seconds: 0,
                    ..row
                });
                sources.len() - 1
            }
        };
        let total = &mut sources[position].elapsed_seconds;
        *total = total
            .checked_add(elapsed_seconds)
            .filter(|value| (0..=MAX_SAFE_SECONDS).contains(value))
            .ok_or("usage source aggregate exceeds the integer limit")?;
    }
    Ok(UsageWindow { sources, truncated })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryTotal {
    pub entry_id: String,
    pub used_seconds: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetTotal {
    pub limit_id: String,
    pub period: &'static str,
    pub window_start_local_date: String,
    pub window_end_local_date: String,
    pub used_seconds: i64,
    pub limit_seconds: i64,
    pub remaining_seconds: i64,
    pub exhausted: bool,
    pub entries: Vec<EntryTotal>,
}

/// Budget totals of one usage window, and whether the matching bound left some sources uncounted.
#[derive(Clone, Debug)]
pub struct UsageTotals {
    pub budgets: Vec<BudgetTotal>,
    pub truncated: bool,
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Read selected package identities and manually entered names with bounded matching work.
pub fn parse_config(root: &Value) -> Result<LimitsConfig, String> {
    let branch = root
        .pointer("/distractions/limits")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "enabled": true, "items": [] }));
    let config: LimitsConfig = serde_json::from_value(branch)
        .map_err(|error| format!("invalid persisted usage limits: {error}"))?;
    if config.items.len() > MAX_LIMITS {
        return Err("usage limit count exceeds the native limit".into());
    }
    let mut ids = HashSet::new();
    let mut entry_count = 0;
    for limit in &config.items {
        if !is_valid_id(&limit.id)
            || !ids.insert(&limit.id)
            || limit.name.len() > MAX_LIMIT_TEXT_BYTES
        {
            return Err("usage limit identity or name is invalid".into());
        }
        if limit.minutes_per_day.is_none() && limit.minutes_per_week.is_none()
            || limit
                .minutes_per_day
                .is_some_and(|minutes| !(1..=1_440).contains(&minutes))
            || limit
                .minutes_per_week
                .is_some_and(|minutes| !(1..=10_080).contains(&minutes))
        {
            return Err("usage limit budget is invalid".into());
        }
        entry_count += limit.entries.len();
        if limit.entries.is_empty() || entry_count > MAX_ENTRIES {
            return Err("usage limit entries are empty or exceed the native limit".into());
        }
        let mut entry_ids = HashSet::new();
        for entry in &limit.entries {
            if !is_valid_id(&entry.id)
                || !entry_ids.insert(&entry.id)
                || entry.desktop_app_match_names.len() > MAX_MATCH_NAMES
            {
                return Err("usage limit entry identity or match names are invalid".into());
            }
            let texts = entry
                .website_host
                .iter()
                .chain(entry.mobile_app_name.iter())
                .chain(entry.mobile_app_package.iter())
                .chain(entry.desktop_app_name.iter())
                .chain(entry.desktop_app_match_names.iter());
            if texts
                .into_iter()
                .any(|text| text.trim().is_empty() || text.len() > MAX_LIMIT_TEXT_BYTES)
            {
                return Err("usage limit source is invalid".into());
            }
            if entry.website_host.is_none()
                && entry.mobile_app_name.is_none()
                && entry.mobile_app_package.is_none()
                && entry.desktop_app_name.is_none()
            {
                return Err("usage limit entry has no source".into());
            }
        }
    }
    Ok(config)
}

/// Fingerprint the persisted limit branch so edits revoke previously derived exhaustion.
pub fn configuration_digest(root: &Value) -> Result<String, String> {
    let bytes = serde_json::to_vec(root.pointer("/distractions/limits").unwrap_or(&Value::Null))
        .map_err(|error| format!("encode usage limit configuration: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Compute the Monday-based week without assuming seven fixed-duration local days.
pub fn week_start(local_date: &str) -> Result<String, String> {
    if !is_valid_local_date(local_date) {
        return Err("usage local date is invalid".into());
    }
    let date =
        NaiveDate::parse_from_str(local_date, "%Y-%m-%d").map_err(|error| error.to_string())?;
    date.checked_sub_signed(Duration::days(i64::from(
        date.weekday().num_days_from_monday(),
    )))
    .map(|value| value.format("%Y-%m-%d").to_string())
    .filter(|value| is_valid_local_date(value))
    .ok_or_else(|| "usage week start is outside the supported range".into())
}

pub fn matches(entry: &LimitEntry, source: &UsageSourceDay) -> bool {
    match source.source_type.as_str() {
        "website" => entry
            .website_host
            .as_deref()
            .and_then(normalize_usage_host)
            .zip(normalize_usage_host(&source.source_key))
            .is_some_and(|(rule, host)| host == rule || host.ends_with(&format!(".{rule}"))),
        "mobile-app" => entry
            .mobile_app_package
            .as_ref()
            .or(entry.mobile_app_name.as_ref())
            .is_some_and(|name| name.to_lowercase() == source.source_key.to_lowercase()),
        "desktop-app" => entry.desktop_app_name.as_ref().is_some_and(|name| {
            if entry.desktop_app_match_names.is_empty() {
                app_name_key(name) == app_name_key(&source.source_key)
            } else {
                entry
                    .desktop_app_match_names
                    .iter()
                    .any(|name| app_name_key(name) == app_name_key(&source.source_key))
            }
        }),
        _ => false,
    }
}

fn add_seconds(left: i64, right: i64) -> Result<i64, String> {
    left.checked_add(right)
        .filter(|value| *value <= MAX_SAFE_SECONDS)
        .ok_or_else(|| "usage total exceeds the supported integer range".into())
}

/// Whether `value` is a real `yyyy-mm-dd` local date, under the check sync admission applies.
pub fn is_valid_local_date(value: &str) -> bool {
    is_usage_date(value)
}

fn app_name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

pub fn normalize_usage_host(input: &str) -> Option<String> {
    let trimmed = input.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = trimmed.strip_prefix("*.").unwrap_or(&trimmed);
    if host.is_empty() || host.contains('*') || host.contains(' ') || host.contains('@') {
        return None;
    }
    Some(host.to_owned())
}

/// Seconds each entry of one limit used today and this week.
struct EntryAllocation {
    daily: Vec<i64>,
    weekly: Vec<i64>,
}

/// Allocates each source of the window to its first matching entry once per limit, preserving
/// overlap semantics. Sources are taken in the given order and each one reaches every limit before
/// the next, so a source ahead of a flood counts in full. Past [`MAX_MATCH_COMPARISONS`] the rest
/// stay uncounted, which the returned flag reports.
fn allocate_sources(
    config: &LimitsConfig,
    sources: &[UsageSourceDay],
    week: &str,
    local_date: &str,
) -> Result<(Vec<EntryAllocation>, bool), String> {
    let mut allocations: Vec<_> = config
        .items
        .iter()
        .map(|limit| EntryAllocation {
            daily: vec![0; limit.entries.len()],
            weekly: vec![0; limit.entries.len()],
        })
        .collect();
    let mut comparisons = 0;
    for source in sources {
        if source.local_date.as_str() < week || source.local_date.as_str() > local_date {
            continue;
        }
        for (limit, allocation) in config.items.iter().zip(&mut allocations) {
            for (index, entry) in limit.entries.iter().enumerate() {
                comparisons += entry.desktop_app_match_names.len().max(1);
                if comparisons > MAX_MATCH_COMPARISONS {
                    return Ok((allocations, true));
                }
                if matches(entry, source) {
                    let weekly = &mut allocation.weekly[index];
                    *weekly = add_seconds(*weekly, source.elapsed_seconds)?;
                    if source.local_date == local_date {
                        let daily = &mut allocation.daily[index];
                        *daily = add_seconds(*daily, source.elapsed_seconds)?;
                    }
                    break;
                }
            }
        }
    }
    Ok((allocations, false))
}

/// Totals every budget over `sources`, which must list the usage to keep first: this device's own
/// usage, then the rest. Replicated rows can make matching reach its work bound, and that truncates
/// the totals rather than failing them, so enforcement keeps running on what was counted.
pub fn totals(
    config: &LimitsConfig,
    sources: &[UsageSourceDay],
    local_date: &str,
) -> Result<UsageTotals, String> {
    let week = week_start(local_date)?;
    if sources.len() > MAX_SOURCE_GROUPS {
        return Err("usage source count exceeds the native limit".into());
    }
    for source in sources {
        if !is_valid_local_date(&source.local_date)
            || source.source_key.len() > MAX_SOURCE_KEY_BYTES
            || !matches!(
                source.source_type.as_str(),
                "website" | "desktop-app" | "mobile-app"
            )
            || source.elapsed_seconds < 0
            || source.elapsed_seconds > MAX_SAFE_SECONDS
        {
            return Err("stored usage aggregate is invalid".into());
        }
    }
    let (allocations, truncated) = allocate_sources(config, sources, &week, local_date)?;
    let mut result = Vec::new();
    for (limit, EntryAllocation { daily, weekly }) in config.items.iter().zip(allocations) {
        for (period, minutes, start, allocation) in [
            ("day", limit.minutes_per_day, local_date, daily),
            ("week", limit.minutes_per_week, week.as_str(), weekly),
        ] {
            let Some(minutes) = minutes else { continue };
            let used_seconds = allocation
                .iter()
                .try_fold(0, |total, &used| add_seconds(total, used))?;
            let limit_seconds = minutes * 60;
            result.push(BudgetTotal {
                limit_id: limit.id.clone(),
                period,
                window_start_local_date: start.to_owned(),
                window_end_local_date: local_date.to_owned(),
                used_seconds,
                limit_seconds,
                remaining_seconds: (limit_seconds - used_seconds).max(0),
                exhausted: used_seconds >= limit_seconds,
                entries: limit
                    .entries
                    .iter()
                    .zip(allocation)
                    .map(|(entry, used_seconds)| EntryTotal {
                        entry_id: entry.id.clone(),
                        used_seconds,
                    })
                    .collect(),
            });
        }
    }
    Ok(UsageTotals {
        budgets: result,
        truncated,
    })
}

#[cfg(test)]
mod tests;
