//! The vault's distraction rules as one configuration: the shape Settings edits and every
//! blocker reads, its defaults, and the normalization rule writers apply before storing a rule.
//!
//! The rules live in replicated SQLite tables; [`store`] assembles them into
//! [`DistractionsConfig`] and applies one edit at a time. Bounds mirror the sync manifest of the
//! rules domain, whose rendered triggers enforce them again at write time.

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::rules::{app_name_key, is_protected_desktop_app_name, normalize_app_candidate_name};

pub mod store;

/// Built-in category definitions shared with the frontend and the native messaging host.
const BUILT_IN_CATEGORIES_JSON: &str =
    include_str!("../../../apps/client/src/lib/distractions/categories.json");

/// Most characters of a host name.
pub const MAX_HOST_CHARS: usize = 253;

/// Most characters of one host label.
const MAX_HOST_LABEL_CHARS: usize = 63;

/// Most characters of a custom category name.
pub const MAX_CATEGORY_NAME_CHARS: usize = 60;

/// Most characters of an app name, usage limit name, or limit entry name.
pub const MAX_NAME_CHARS: usize = 80;

/// Most characters of an Android package name.
pub const MAX_PACKAGE_CHARS: usize = 255;

/// Most characters of a custom category, usage limit, or limit entry id.
pub const MAX_ID_CHARS: usize = 80;

/// Most desktop process names one app rule or limit entry matches.
pub const MAX_MATCH_NAMES: usize = 64;

/// Largest event palette index of a limit entry color.
pub const MAX_COLOR: i64 = 31;

/// Largest daily budget in minutes.
pub const MAX_MINUTES_PER_DAY: i64 = 1_440;

/// Largest weekly budget in minutes.
pub const MAX_MINUTES_PER_WEEK: i64 = 10_080;

/// Most usage limits of a vault.
pub const MAX_USAGE_LIMITS: usize = crate::limits::MAX_LIMITS;

/// Most limit entries of every usage limit together.
pub const MAX_LIMIT_ENTRIES: usize = 2_000;

fn enabled_default() -> bool {
    true
}

/// Whether sites are blocked by list (`blacklist`) or everything outside the allowed list is
/// blocked (`whitelist`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DistractionsMode {
    #[default]
    Blacklist,
    Whitelist,
}

impl DistractionsMode {
    /// The stored value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blacklist => "blacklist",
            Self::Whitelist => "whitelist",
        }
    }

    fn from_stored(value: &str) -> Self {
        if value == "whitelist" {
            Self::Whitelist
        } else {
            Self::Blacklist
        }
    }
}

/// A host rule of a list or custom category.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostRule {
    pub host: String,
    pub enabled: bool,
}

/// A built-in category toggle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryRule {
    pub id: String,
    pub enabled: bool,
}

/// A named group of hosts the user blocks together.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomCategory {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub hosts: Vec<HostRule>,
}

/// A blocked desktop app and the process names it matches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAppRule {
    pub name: String,
    pub enabled: bool,
    pub match_names: Vec<String>,
}

/// A blocked Android app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileAppRule {
    pub name: String,
    pub package_name: String,
    pub enabled: bool,
}

/// Desktop blocking switches and blocked apps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopConfig {
    pub enabled: bool,
    pub block_during_focus: bool,
    pub block_during_short_breaks: bool,
    pub block_during_long_breaks: bool,
    pub pause_during_focus_pause: bool,
    pub blocked_apps: Vec<DesktopAppRule>,
}

/// Android blocking switches and blocked apps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileConfig {
    pub enabled: bool,
    pub block_during_focus: bool,
    pub block_during_short_breaks: bool,
    pub block_during_long_breaks: bool,
    pub pause_during_focus_pause: bool,
    pub blocked_apps: Vec<MobileAppRule>,
}

/// One source a usage limit counts: a website, an Android app, a desktop app, or several.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitEntryRule {
    pub id: String,
    pub name: Option<String>,
    pub color: Option<i64>,
    pub website_host: Option<String>,
    pub mobile_app_name: Option<String>,
    pub mobile_app_package: Option<String>,
    pub desktop_app_name: Option<String>,
    pub desktop_app_match_names: Vec<String>,
}

/// A daily or weekly budget shared by its entries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub minutes_per_day: Option<i64>,
    pub minutes_per_week: Option<i64>,
    pub entries: Vec<LimitEntryRule>,
}

/// The usage limits switch and limits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageLimitsConfig {
    pub enabled: bool,
    pub items: Vec<LimitRule>,
}

/// Every distraction rule of the vault, in the shape Settings shows and the blockers read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsConfig {
    pub mode: DistractionsMode,
    pub enabled: bool,
    pub block_during_focus: bool,
    pub block_during_short_breaks: bool,
    pub block_during_long_breaks: bool,
    pub pause_during_focus_pause: bool,
    pub blocked_categories: Vec<CategoryRule>,
    pub custom_category_stacks: Vec<CustomCategory>,
    pub blocked_hosts: Vec<HostRule>,
    pub exception_hosts: Vec<HostRule>,
    pub allowed_hosts: Vec<HostRule>,
    pub mobile: MobileConfig,
    pub desktop: DesktopConfig,
    pub limits: UsageLimitsConfig,
}

impl Default for DistractionsConfig {
    fn default() -> Self {
        Self {
            mode: DistractionsMode::Blacklist,
            enabled: true,
            block_during_focus: true,
            block_during_short_breaks: true,
            block_during_long_breaks: true,
            pause_during_focus_pause: true,
            blocked_categories: built_in_categories()
                .iter()
                .map(|category| CategoryRule {
                    id: category.id.clone(),
                    enabled: category.default_enabled,
                })
                .collect(),
            custom_category_stacks: Vec::new(),
            blocked_hosts: Vec::new(),
            exception_hosts: Vec::new(),
            allowed_hosts: Vec::new(),
            mobile: MobileConfig {
                enabled: true,
                block_during_focus: true,
                block_during_short_breaks: true,
                block_during_long_breaks: true,
                pause_during_focus_pause: true,
                blocked_apps: Vec::new(),
            },
            desktop: DesktopConfig {
                enabled: true,
                block_during_focus: true,
                block_during_short_breaks: true,
                block_during_long_breaks: true,
                pause_during_focus_pause: true,
                blocked_apps: Vec::new(),
            },
            limits: UsageLimitsConfig {
                enabled: true,
                items: Vec::new(),
            },
        }
    }
}

impl DistractionsConfig {
    /// The configuration under a `distractions` key, the root that
    /// [`crate::limits::parse_config`] and [`crate::limits::configuration_digest`] read.
    pub fn to_root_value(&self) -> Result<Value, String> {
        let config = serde_json::to_value(self)
            .map_err(|error| format!("encode distraction rules: {error}"))?;
        Ok(serde_json::json!({ "distractions": config }))
    }
}

/// A built-in category id and whether it blocks before the vault records a choice.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltInCategory {
    pub id: String,
    #[serde(default = "enabled_default")]
    pub default_enabled: bool,
}

/// The built-in categories in display order.
pub fn built_in_categories() -> &'static [BuiltInCategory] {
    static CATEGORIES: OnceLock<Vec<BuiltInCategory>> = OnceLock::new();
    CATEGORIES.get_or_init(|| {
        serde_json::from_str(BUILT_IN_CATEGORIES_JSON)
            .expect("embedded distraction categories must be valid JSON")
    })
}

/// Whether `id` names a built-in category.
pub fn is_built_in_category(id: &str) -> bool {
    built_in_categories()
        .iter()
        .any(|category| category.id == id)
}

/// Whether `id` is a custom category, usage limit, or limit entry id.
pub fn is_rule_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Whether `host` is a normalized host rule: lowercase ASCII labels of letters, digits, and
/// inner hyphens joined by dots.
pub fn is_rule_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= MAX_HOST_CHARS
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= MAX_HOST_LABEL_CHARS
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

/// A host as Settings normalizes it: lowercase, without a leading `*.` or trailing dots.
/// Callers pass hosts already extracted from URLs; anything else that is not a valid host rule
/// is refused.
pub fn normalize_host(input: &str) -> Option<String> {
    let lowered = input.trim().to_ascii_lowercase();
    let host = lowered.strip_prefix("*.").unwrap_or(&lowered);
    let host = host.trim_end_matches('.');
    is_rule_host(host).then(|| host.to_string())
}

/// Normalized distinct hosts in first-seen order.
pub fn normalize_hosts(inputs: &[String]) -> Option<Vec<String>> {
    let mut seen = HashSet::new();
    let mut hosts = Vec::new();
    for input in inputs {
        let host = normalize_host(input)?;
        if seen.insert(host.clone()) {
            hosts.push(host);
        }
    }
    Some(hosts)
}

/// A name with whitespace runs collapsed, cut to `max_chars`, or `None` when blank or holding
/// control characters.
pub fn normalize_name(input: &str, max_chars: usize) -> Option<String> {
    let name = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().any(char::is_control) {
        return None;
    }
    Some(name.chars().take(max_chars).collect())
}

/// A desktop app name as Settings normalizes it.
pub fn normalize_app_name(input: &str) -> Option<String> {
    normalize_app_candidate_name(input)
}

/// An Android package name: dotted segments that start with a letter.
pub fn normalize_package(input: &str) -> Option<String> {
    let package = input.trim();
    let valid = !package.is_empty()
        && package.len() <= MAX_PACKAGE_CHARS
        && package.split('.').count() >= 2
        && package.split('.').all(|segment| {
            segment
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        });
    valid.then(|| package.to_string())
}

/// The display name followed by the distinct unprotected match names, by case-insensitive key;
/// the display name alone when nothing else remains.
pub fn normalize_match_names(name: &str, match_names: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for candidate in std::iter::once(name).chain(match_names.iter().map(String::as_str)) {
        let Some(candidate) = normalize_app_name(candidate) else {
            continue;
        };
        if is_protected_desktop_app_name(&candidate) || !seen.insert(app_name_key(&candidate)) {
            continue;
        }
        normalized.push(candidate);
    }
    if normalized.is_empty() {
        normalized.push(name.to_string());
    }
    normalized
}

#[cfg(test)]
mod tests;
