//! Pure browser decisions and stable rule fingerprints.

use crate::config::{
    BUILT_IN_CATEGORIES_JSON, BuiltInCategory, DistractionsConfig, DistractionsMode,
    UsageLimitEntry, built_in_category, normalize_host_rule, usage_limit_entry_source_keys,
};
use crate::snapshot::LimitState;

/// A rule result whose display label never determines its persistence semantics.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum HostDecision {
    Allowed,
    SafetyAllowlist,
    WhitelistedHost(String),
    ExceptionHost(String),
    OutsideWhitelist,
    BlockedHost(String),
    CustomCategory(String),
    BuiltInCategory(String),
    DailyLimit(String),
    WeeklyLimit(String),
}

impl HostDecision {
    pub(super) fn is_blocked(&self) -> bool {
        match self {
            Self::Allowed
            | Self::SafetyAllowlist
            | Self::WhitelistedHost(_)
            | Self::ExceptionHost(_) => false,
            Self::OutsideWhitelist
            | Self::BlockedHost(_)
            | Self::CustomCategory(_)
            | Self::BuiltInCategory(_)
            | Self::DailyLimit(_)
            | Self::WeeklyLimit(_) => true,
        }
    }

    /// Returns the stable labels consumed by the extension and stored rule snapshots.
    pub(super) fn matched_rule_name(&self) -> Option<String> {
        match self {
            Self::Allowed => None,
            Self::SafetyAllowlist => Some("browser safety allowlist".to_string()),
            Self::WhitelistedHost(host) => Some(format!("whitelist: {host}")),
            Self::ExceptionHost(host) => Some(format!("exception: {host}")),
            Self::OutsideWhitelist => Some("not in whitelist".to_string()),
            Self::BlockedHost(host) => Some(format!("blocked host: {host}")),
            Self::CustomCategory(name) => Some(format!("custom stack: {name}")),
            Self::BuiltInCategory(name) => Some(format!("category: {name}")),
            Self::DailyLimit(name) => Some(format!("daily limit: {name}")),
            Self::WeeklyLimit(name) => Some(format!("weekly limit: {name}")),
        }
    }

    pub(super) fn event_decision(&self) -> &'static str {
        match self {
            Self::DailyLimit(_) | Self::WeeklyLimit(_) => "limit_exhausted",
            _ => "blocked",
        }
    }

    pub(super) fn rule_kind(&self) -> Option<&'static str> {
        match self {
            Self::DailyLimit(_) | Self::WeeklyLimit(_) => Some("usage_limit"),
            Self::BuiltInCategory(_) => Some("category"),
            Self::CustomCategory(_) => Some("custom_category"),
            Self::BlockedHost(_) | Self::OutsideWhitelist => Some("domain"),
            Self::Allowed
            | Self::SafetyAllowlist
            | Self::WhitelistedHost(_)
            | Self::ExceptionHost(_) => None,
        }
    }

    pub(super) fn blocker_mode(&self, mode: &DistractionsMode) -> &'static str {
        match self {
            Self::DailyLimit(_) | Self::WeeklyLimit(_) => "limit",
            _ => mode.as_str(),
        }
    }
}

pub(super) fn decide_url_with_limits(
    host: &str,
    url: Option<&str>,
    config: &DistractionsConfig,
    limit_state: Option<&LimitState>,
    regular_rules_active: bool,
) -> HostDecision {
    let regular_decision = if regular_rules_active {
        let decision = decide_url(host, url, config);
        if decision.is_blocked() {
            return decision;
        }
        Some(decision)
    } else {
        None
    };
    let limit_decision = decide_url_limit(host, config, limit_state);
    if limit_decision.is_blocked() {
        return limit_decision;
    }
    regular_decision.unwrap_or(HostDecision::Allowed)
}

pub(super) fn decide_url(
    host: &str,
    url: Option<&str>,
    config: &DistractionsConfig,
) -> HostDecision {
    if is_safety_allowed_host(host) {
        return HostDecision::SafetyAllowlist;
    }

    match config.mode {
        DistractionsMode::Whitelist => {
            for allowed_host in &config.allowed_hosts {
                if host_matches_rule(host, allowed_host) {
                    return HostDecision::WhitelistedHost(allowed_host.clone());
                }
            }
            HostDecision::OutsideWhitelist
        }
        DistractionsMode::Blacklist => {
            for exception_host in &config.exception_hosts {
                if host_matches_rule(host, exception_host) {
                    return HostDecision::ExceptionHost(exception_host.clone());
                }
            }
            for blocked_host in &config.blocked_hosts {
                if host_matches_rule(host, blocked_host) {
                    return HostDecision::BlockedHost(blocked_host.clone());
                }
            }
            for stack in &config.custom_category_stacks {
                for stack_host in &stack.hosts {
                    if host_matches_rule(host, stack_host) {
                        return HostDecision::CustomCategory(stack.name.clone());
                    }
                }
            }
            for category_id in &config.blocked_category_ids {
                let Some(category) = built_in_category(category_id) else {
                    continue;
                };
                if category_matches_url(host, url, category) {
                    return HostDecision::BuiltInCategory(category.label.clone());
                }
            }
            HostDecision::Allowed
        }
    }
}

fn decide_url_limit(
    host: &str,
    config: &DistractionsConfig,
    limit_state: Option<&LimitState>,
) -> HostDecision {
    if is_safety_allowed_host(host) || !config.limits.enabled {
        return HostDecision::Allowed;
    }
    let Some(limit_state) = limit_state else {
        return HostDecision::Allowed;
    };
    if limit_state.configuration_digest.is_none()
        || limit_state.configuration_digest != config.limit_configuration_digest
    {
        return HostDecision::Allowed;
    }
    for limit in &config.limits.items {
        if !limit.enabled {
            continue;
        }
        let Some(total) = limit_state
            .limits
            .iter()
            .find(|total| total.id == limit.id && total.exhausted)
        else {
            continue;
        };
        let minutes = if total.period == "day" {
            limit.minutes_per_day
        } else if total.period == "week" {
            limit.minutes_per_week
        } else {
            None
        };
        if total.used_seconds < total.limit_seconds
            || total.remaining_seconds != 0
            || !minutes.is_some_and(|minutes| minutes.checked_mul(60) == Some(total.limit_seconds))
        {
            continue;
        }
        if limit
            .entries
            .iter()
            .any(|entry| limit_entry_matches_host(entry, host))
        {
            return if total.period == "week" {
                HostDecision::WeeklyLimit(limit.name.clone())
            } else {
                HostDecision::DailyLimit(limit.name.clone())
            };
        }
    }
    HostDecision::Allowed
}

fn limit_entry_matches_host(entry: &UsageLimitEntry, host: &str) -> bool {
    entry
        .website_host
        .as_deref()
        .is_some_and(|source_host| host_matches_rule(host, source_host))
}

fn category_matches_url(host: &str, url: Option<&str>, category: &BuiltInCategory) -> bool {
    if category
        .hosts
        .iter()
        .any(|category_host| host_matches_rule(host, category_host))
    {
        return true;
    }
    if category
        .domain_keywords
        .iter()
        .any(|keyword| host.contains(keyword))
    {
        return true;
    }
    if !host_matches_rule(host, "reddit.com") {
        return false;
    }
    let Some(subreddit) = url.and_then(reddit_subreddit_from_url) else {
        return false;
    };
    category
        .reddit_subreddit_keywords
        .iter()
        .any(|keyword| subreddit.contains(keyword))
}

pub(super) fn feed_fingerprint(hash: &mut u64, value: &str) {
    for byte in value.as_bytes() {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(1_099_511_628_211);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(1_099_511_628_211);
}

fn feed_fingerprint_bool(hash: &mut u64, value: bool) {
    feed_fingerprint(hash, if value { "1" } else { "0" });
}

fn feed_fingerprint_list(hash: &mut u64, label: &str, values: &[String]) {
    feed_fingerprint(hash, label);
    for value in values {
        feed_fingerprint(hash, value);
    }
}

pub(super) fn rules_fingerprint(
    config: &DistractionsConfig,
    limit_state: Option<&LimitState>,
) -> String {
    let mut hash = 14_695_981_039_346_656_037_u64;
    feed_fingerprint(&mut hash, "built_in_categories");
    feed_fingerprint(&mut hash, BUILT_IN_CATEGORIES_JSON);
    feed_fingerprint(&mut hash, config.mode.as_str());
    feed_fingerprint_bool(&mut hash, config.enabled);
    feed_fingerprint_bool(&mut hash, config.block_during_focus);
    feed_fingerprint_bool(&mut hash, config.block_during_short_breaks);
    feed_fingerprint_bool(&mut hash, config.block_during_long_breaks);
    feed_fingerprint_bool(&mut hash, config.pause_during_focus_pause);
    feed_fingerprint_list(&mut hash, "category", &config.blocked_category_ids);
    feed_fingerprint(&mut hash, "custom_stack");
    for stack in &config.custom_category_stacks {
        feed_fingerprint(&mut hash, &stack.id);
        feed_fingerprint(&mut hash, &stack.name);
        for host in &stack.hosts {
            feed_fingerprint(&mut hash, host);
        }
    }
    feed_fingerprint_list(&mut hash, "blocked", &config.blocked_hosts);
    feed_fingerprint_list(&mut hash, "exception", &config.exception_hosts);
    feed_fingerprint_list(&mut hash, "allowed", &config.allowed_hosts);
    feed_fingerprint_bool(&mut hash, config.limits.enabled);
    feed_fingerprint(&mut hash, "limits");
    for limit in &config.limits.items {
        feed_fingerprint(&mut hash, &limit.id);
        feed_fingerprint(&mut hash, &limit.name);
        feed_fingerprint_bool(&mut hash, limit.enabled);
        feed_fingerprint(&mut hash, "day");
        if let Some(minutes_per_day) = limit.minutes_per_day {
            feed_fingerprint(&mut hash, &minutes_per_day.to_string());
        } else {
            feed_fingerprint(&mut hash, "none");
        }
        feed_fingerprint(&mut hash, "week");
        if let Some(minutes_per_week) = limit.minutes_per_week {
            feed_fingerprint(&mut hash, &minutes_per_week.to_string());
        } else {
            feed_fingerprint(&mut hash, "none");
        }
        for entry in &limit.entries {
            feed_fingerprint(&mut hash, &entry.id);
            if let Some(name) = &entry.name {
                feed_fingerprint(&mut hash, name);
            }
            for key in usage_limit_entry_source_keys(entry) {
                feed_fingerprint(&mut hash, &key);
            }
        }
    }
    if let Some(limit_state) = limit_state {
        feed_fingerprint(&mut hash, &limit_state.local_date);
        feed_fingerprint(&mut hash, &limit_state.week_start_local_date);
        feed_fingerprint(&mut hash, &limit_state.database_path);
        for limit in &limit_state.limits {
            feed_fingerprint(&mut hash, &limit.id);
            feed_fingerprint(&mut hash, &limit.period);
            feed_fingerprint(&mut hash, &limit.window_start_local_date);
            feed_fingerprint(&mut hash, &limit.window_end_local_date);
            feed_fingerprint(&mut hash, &limit.used_seconds.to_string());
            feed_fingerprint(&mut hash, &limit.limit_seconds.to_string());
            feed_fingerprint(&mut hash, &limit.remaining_seconds.to_string());
            feed_fingerprint_bool(&mut hash, limit.exhausted);
        }
    }
    format!("{hash:016x}")
}

pub(super) fn host_from_url(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let after_user = authority.rsplit('@').next().unwrap_or(authority);
    let host = after_user
        .strip_prefix('[')
        .and_then(|value| value.split_once(']').map(|(host, _)| host))
        .unwrap_or_else(|| after_user.split(':').next().unwrap_or_default());
    normalize_host_rule(host)
}

pub(super) fn host_matches_rule(host: &str, rule_host: &str) -> bool {
    host == rule_host
        || host
            .strip_suffix(rule_host)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn reddit_subreddit_from_url(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let path_start = rest.find('/')?;
    let path = rest[path_start..]
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let subreddit = path.strip_prefix("/r/")?.split('/').next()?;
    if subreddit.is_empty() {
        return None;
    }
    Some(subreddit.to_string())
}

fn is_safety_allowed_host(host: &str) -> bool {
    host == "localhost" || host == "127.0.0.1" || host == "::1" || host.ends_with(".localhost")
}
