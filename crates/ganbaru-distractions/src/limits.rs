//! Pure budget matching over native, bounded daily source aggregates.

use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub mod store;

pub const MAX_LIMITS: usize = 256;
const MAX_ENTRIES: usize = 2_000;
const MAX_MATCH_NAMES: usize = 64;
const MAX_MATCH_COMPARISONS: usize = 2_000_000;
pub const MAX_SOURCE_GROUPS: usize = 10_000;
pub const MAX_WINDOW_SAMPLES: i64 = 500_000;
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
        if !is_valid_id(&limit.id) || !ids.insert(&limit.id) || limit.name.len() > 512 {
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
                .any(|text| text.trim().is_empty() || text.len() > 512)
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

pub fn is_valid_local_date(value: &str) -> bool {
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

/// Allocate a source to its first matching entry once per limit, preserving overlap semantics.
pub fn totals(
    config: &LimitsConfig,
    sources: &[UsageSourceDay],
    local_date: &str,
) -> Result<Vec<BudgetTotal>, String> {
    let week = week_start(local_date)?;
    if sources.len() > MAX_SOURCE_GROUPS {
        return Err("usage source count exceeds the native limit".into());
    }
    for source in sources {
        if !is_valid_local_date(&source.local_date)
            || source.source_key.len() > 512
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
    let mut result = Vec::new();
    let mut comparisons = 0;
    for limit in &config.items {
        let mut daily = vec![0; limit.entries.len()];
        let mut weekly = daily.clone();
        for source in sources {
            if source.local_date.as_str() < week.as_str() || source.local_date.as_str() > local_date
            {
                continue;
            }
            for (index, entry) in limit.entries.iter().enumerate() {
                comparisons += entry.desktop_app_match_names.len().max(1);
                if comparisons > MAX_MATCH_COMPARISONS {
                    return Err("usage matching exceeds the native work limit".into());
                }
                if matches(entry, source) {
                    weekly[index] = add_seconds(weekly[index], source.elapsed_seconds)?;
                    if source.local_date == local_date {
                        daily[index] = add_seconds(daily[index], source.elapsed_seconds)?;
                    }
                    break;
                }
            }
        }
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
    Ok(result)
}

#[cfg(test)]
mod tests;
