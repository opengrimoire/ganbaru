//! Typed Guardian capture and rule projection derived from persisted native policy.

use super::{MAX_JOURNAL_EVENTS, PendingEvent, normalize_event};
use crate::distractions::limits::{self as limits, BudgetTotal, UsageSourceDay, UsageWindow};
use ganbaru_sync::domains::distractions::is_android_package_name;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

pub(super) const MAX_CAPTURE_BYTES: usize = 512 * 1024;
pub(super) const MAX_CAPTURE_AGE_MS: i64 = 10_000;
const MAX_MOBILE_RULES: usize = 256;
const MAX_RULE_NAME_CHARS: usize = 120;
const MAX_MOBILE_LIMITS: usize = 128;
const MAX_SAFE_SECONDS: i64 = 9_007_199_254_740_991;

/// Localized notification text is the only frontend input to native policy publication.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotificationCopy {
    pub channel_name: String,
    pub channel_description: String,
    pub blocked_message: String,
    pub limit_message: String,
}

impl NotificationCopy {
    pub(super) fn validate(&self) -> Result<(), String> {
        for (text, maximum) in [
            (&self.channel_name, 80),
            (&self.channel_description, 160),
            (&self.blocked_message, 120),
            (&self.limit_message, 120),
        ] {
            if text.trim().is_empty() || text.chars().count() > maximum {
                return Err("Guardian notification text exceeds its limit".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapturedSource {
    source_type: String,
    source_key: String,
    local_date: String,
    elapsed_seconds: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GuardianCapture {
    pub vault_id: String,
    pub journal_vault_id: Option<String>,
    pub observed_at_epoch_ms: i64,
    utc_offset_seconds: i32,
    pub local_date: String,
    pub week_start_local_date: String,
    local_sources: Vec<CapturedSource>,
    pub pending: Vec<PendingEvent>,
    pub copy: Option<NotificationCopy>,
}

impl GuardianCapture {
    /// Validate complete evidence before either accounting or baseline publication.
    pub(super) fn decode(encoded: &str, vault_id: &str, now_ms: i64) -> Result<Self, String> {
        if encoded.len() > MAX_CAPTURE_BYTES {
            return Err("Guardian capture exceeds its byte limit".into());
        }
        let mut capture: Self = serde_json::from_str(encoded)
            .map_err(|error| format!("decode Guardian accounting capture: {error}"))?;
        capture.ensure_current(vault_id, now_ms)?;
        if capture.pending.len() > MAX_JOURNAL_EVENTS
            || capture.local_sources.len() > limits::MAX_SOURCE_GROUPS
            || (capture.journal_vault_id.as_deref() != Some(vault_id)
                && !capture.local_sources.is_empty())
        {
            return Err("Guardian capture has inconsistent scope or oversized evidence".into());
        }
        let mut ids = HashSet::new();
        for event in &mut capture.pending {
            *event = normalize_event(event.clone())?;
            if event.kind != "usage"
                || event.vault_id != vault_id
                || !ids.insert(event.id.clone())
                || event.local_date < capture.week_start_local_date
                || event.local_date > capture.local_date
            {
                return Err(
                    "Guardian pending evidence has invalid ownership, identity, or window".into(),
                );
            }
        }
        let mut source_ids = HashSet::new();
        for source in &mut capture.local_sources {
            source.source_key = source.source_key.to_ascii_lowercase();
            if source.source_type != "mobile-app"
                || !is_android_package_name(&source.source_key)
                || !limits::is_valid_local_date(&source.local_date)
                || source.local_date < capture.week_start_local_date
                || source.local_date > capture.local_date
                || !(0..=MAX_SAFE_SECONDS).contains(&source.elapsed_seconds)
                || !source_ids.insert((&source.source_key, &source.local_date))
            {
                return Err("Guardian local counter baseline is invalid".into());
            }
        }
        if let Some(copy) = &capture.copy {
            copy.validate()?;
        }
        Ok(capture)
    }

    pub(super) fn ensure_current(&self, vault_id: &str, now_ms: i64) -> Result<(), String> {
        let offset = chrono::FixedOffset::east_opt(self.utc_offset_seconds)
            .ok_or("Guardian capture offset is invalid")?;
        let captured_local_date =
            chrono::DateTime::from_timestamp_millis(self.observed_at_epoch_ms)
                .map(|time| time.with_timezone(&offset).format("%Y-%m-%d").to_string())
                .ok_or("Guardian capture timestamp or offset is invalid")?;
        let current_local_date = chrono::DateTime::from_timestamp_millis(now_ms)
            .map(|time| time.with_timezone(&offset).format("%Y-%m-%d").to_string())
            .ok_or("Guardian publication timestamp is invalid")?;
        if self.vault_id != vault_id
            || self.observed_at_epoch_ms <= 0
            || !(0..=MAX_CAPTURE_AGE_MS).contains(&now_ms.saturating_sub(self.observed_at_epoch_ms))
            || self.observed_at_epoch_ms > now_ms
            || limits::week_start(&self.local_date)? != self.week_start_local_date
            || captured_local_date != self.local_date
            || current_local_date != self.local_date
        {
            return Err(
                "Guardian accounting capture is stale or belongs to another vault/window".into(),
            );
        }
        Ok(())
    }

    fn local_baseline(&self, packages: &HashSet<String>, start: &str) -> Result<i64, String> {
        self.local_sources
            .iter()
            .filter(|row| {
                packages.contains(&row.source_key.to_ascii_lowercase())
                    && row.local_date.as_str() >= start
                    && row.local_date <= self.local_date
            })
            .try_fold(0_i64, |total, row| {
                total
                    .checked_add(row.elapsed_seconds)
                    .filter(|value| *value <= MAX_SAFE_SECONDS)
                    .ok_or_else(|| "Guardian local baseline exceeds the integer limit".into())
            })
    }
}

fn enabled() -> bool {
    true
}

/// Merge complete pending evidence once per immutable identity into bounded source/day groups.
/// This device's pending rows come first, so a truncated window keeps them and then `sources` in
/// their given order.
pub(super) fn include_pending(
    sources: Vec<UsageSourceDay>,
    capture: &GuardianCapture,
    excluded: &HashSet<String>,
) -> Result<UsageWindow, String> {
    let pending = capture
        .pending
        .iter()
        .filter(|event| !excluded.contains(&event.id))
        .map(|event| UsageSourceDay {
            source_type: "mobile-app".into(),
            source_key: event.package_name.clone(),
            local_date: event.local_date.clone(),
            elapsed_seconds: event.elapsed_seconds,
        });
    limits::merge_source_groups(pending.chain(sources))
}

/// What the last accepted usage window was computed for, and its sources with pending rows merged.
#[derive(Clone, Debug)]
pub(super) struct LastGoodUsage {
    pub generation: u64,
    pub vault_id: String,
    pub rules_revision: u64,
    pub local_date: String,
    pub sources: Vec<UsageSourceDay>,
}

/// Sources Guardian keeps enforcing with after a failed read: the last accepted window of the same
/// generation, vault, rules, and date, or else this device's own captured usage alone. A reused
/// window misses usage since it was read, which can only delay a limit.
pub(super) fn fallback_sources(
    last_good: Option<&LastGoodUsage>,
    capture: &GuardianCapture,
    generation: u64,
    rules_revision: u64,
) -> Vec<UsageSourceDay> {
    match last_good.filter(|usage| {
        usage.generation == generation
            && usage.vault_id == capture.vault_id
            && usage.rules_revision == rules_revision
            && usage.local_date == capture.local_date
    }) {
        Some(usage) => usage.sources.clone(),
        None => capture
            .local_sources
            .iter()
            .map(|source| UsageSourceDay {
                source_type: source.source_type.clone(),
                source_key: source.source_key.clone(),
                local_date: source.local_date.clone(),
                elapsed_seconds: source.elapsed_seconds,
            })
            .collect(),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct MobileRule {
    name: String,
    #[serde(default)]
    package_name: Option<String>,
    #[serde(default = "enabled")]
    enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct MobileSchedule {
    enabled: bool,
    block_during_focus: bool,
    block_during_short_breaks: bool,
    block_during_long_breaks: bool,
    pause_during_focus_pause: bool,
    blocked_apps: Vec<MobileRule>,
}

impl Default for MobileSchedule {
    fn default() -> Self {
        Self {
            enabled: true,
            block_during_focus: true,
            block_during_short_breaks: true,
            block_during_long_breaks: true,
            pause_during_focus_pause: true,
            blocked_apps: Vec::new(),
        }
    }
}

/// Whether Guardian accepts `rule`. The display name comes from a separate replicated group, so
/// sync admission cannot check it together with the identity.
fn is_publishable_rule(rule: &MobileRule, package: &str) -> bool {
    is_android_package_name(package)
        && !rule.name.trim().is_empty()
        && rule.name.chars().count() <= MAX_RULE_NAME_CHARS
}

/// The rules Guardian enforces: rows with a package, one per lowercase package, up to
/// [`MAX_MOBILE_RULES`]. An invalid or surplus row is dropped with a diagnostic, because failing
/// publication would revoke every rule Guardian holds.
fn publishable_rules(rules: Vec<MobileRule>) -> Vec<MobileRule> {
    let mut skipped = 0_usize;
    let mut packages_seen = HashSet::new();
    let mut publishable = Vec::new();
    for mut rule in rules {
        let Some(package) = rule.package_name.take() else {
            continue;
        };
        if !is_publishable_rule(&rule, &package) {
            skipped += 1;
            eprintln!("Guardian skipped an invalid application rule: {package:?}");
            continue;
        }
        let package = package.to_ascii_lowercase();
        if packages_seen.insert(package.clone()) {
            rule.package_name = Some(package);
            publishable.push(rule);
        }
    }
    if skipped > 0 {
        eprintln!("Guardian skipped {skipped} invalid application rules");
    }
    if publishable.len() > MAX_MOBILE_RULES {
        eprintln!(
            "Guardian publishes the first {MAX_MOBILE_RULES} of {} application rules",
            publishable.len()
        );
        publishable.truncate(MAX_MOBILE_RULES);
    }
    publishable
}

/// Build enforcement and display views from one accepted source window and one captured baseline.
pub(super) fn build(
    root: &Value,
    capture: &GuardianCapture,
    sources: &[UsageSourceDay],
    copy: &NotificationCopy,
    revision: &str,
) -> Result<(Value, Value), String> {
    copy.validate()?;
    let config = crate::distractions::limits_or_phase_only(root);
    let budgets = limits::totals(&config, sources, &capture.local_date)?.budgets;
    let mut mobile: MobileSchedule = serde_json::from_value(
        root.pointer("/distractions/mobile")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )
    .map_err(|error| format!("invalid persisted Guardian schedule: {error}"))?;
    mobile.blocked_apps = publishable_rules(mobile.blocked_apps);
    let mut items = Vec::new();
    for limit in &config.items {
        let packages: HashSet<String> = limit
            .entries
            .iter()
            .filter_map(|entry| entry.mobile_app_package.as_ref())
            .map(|package| package.to_ascii_lowercase())
            .collect();
        if packages.is_empty() {
            continue;
        }
        if packages.len() > MAX_MOBILE_RULES
            || packages
                .iter()
                .any(|package| !is_android_package_name(package))
        {
            return Err("persisted Guardian limit packages are invalid".into());
        }
        let accepted = |period: &str| -> Result<Value, String> {
            let Some(budget) = budgets
                .iter()
                .find(|budget| budget.limit_id == limit.id && budget.period == period)
            else {
                return Ok(Value::Null);
            };
            Ok(
                json!({"windowStartLocalDate": budget.window_start_local_date,
                "windowEndLocalDate": budget.window_end_local_date, "usedSeconds": budget.used_seconds,
                "localUsedSecondsAtCapture": capture.local_baseline(&packages, &budget.window_start_local_date)?}),
            )
        };
        let mut sorted: Vec<_> = packages.iter().cloned().collect();
        sorted.sort();
        let name = if limit.name.trim().is_empty() {
            &limit.id
        } else {
            &limit.name
        };
        items.push(
            json!({"id": limit.id, "name": name.chars().take(80).collect::<String>(),
            "enabled": limit.enabled, "minutesPerDay": limit.minutes_per_day,
            "minutesPerWeek": limit.minutes_per_week, "packages": sorted,
            "acceptedUsage": {"day": accepted("day")?, "week": accepted("week")?}}),
        );
    }
    if items.len() > MAX_MOBILE_LIMITS {
        return Err("too many Guardian package limits".into());
    }
    let rules = json!({"schemaVersion": 1, "vaultId": capture.vault_id, "revision": revision,
        "generatedAtEpochMs": capture.observed_at_epoch_ms, "mobile": mobile,
        "limits": {"enabled": config.enabled, "items": items}, "copy": copy});
    if serde_json::to_vec(&rules)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_CAPTURE_BYTES
    {
        return Err("native Guardian rules exceed their byte limit".into());
    }
    let view = usage_view(capture, &budgets);
    Ok((rules, view))
}

fn usage_view(capture: &GuardianCapture, budgets: &[BudgetTotal]) -> Value {
    json!({"vaultId": capture.vault_id, "localDate": capture.local_date,
        "weekStartLocalDate": capture.week_start_local_date,
        "updatedAt": chrono::DateTime::from_timestamp_millis(capture.observed_at_epoch_ms)
            .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        "totals": budgets, "foregroundStatus": {"available": false, "appName": null,
            "processName": null, "processId": null, "matchNames": [], "reason": null}})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn now() -> i64 {
        "2026-10-02T12:00:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond()
    }
    fn capture_value() -> Value {
        json!({"vaultId": "vault", "journalVaultId": "vault",
            "observedAtEpochMs": now(), "utcOffsetSeconds": 0, "localDate": "2026-10-02", "weekStartLocalDate": "2026-09-28",
            "localSources": [{"sourceType": "mobile-app", "sourceKey": "com.example.video",
                "localDate": "2026-10-02", "elapsedSeconds": 20}], "pending": []})
    }
    fn capture() -> GuardianCapture {
        GuardianCapture::decode(&capture_value().to_string(), "vault", now()).unwrap()
    }
    fn pending_value() -> Value {
        json!({"id": "pending", "kind": "usage", "packageName": "com.example.video",
            "displayName": "Video", "startedAtMs": now() - 2_000, "elapsedSeconds": 2,
            "localDate": "2026-10-02", "occurredAtMs": now(), "vaultId": "vault"})
    }
    fn copy() -> NotificationCopy {
        NotificationCopy {
            channel_name: "Distractions".into(),
            channel_description: "Blocks".into(),
            blocked_message: "Blocked".into(),
            limit_message: "Limit reached".into(),
        }
    }
    fn root() -> Value {
        json!({"distractions": {"mobile": {"blockDuringShortBreaks": false,
        "blockedApps": [{"name": "Video", "packageName": "com.example.video", "enabled": true}]},
        "limits": {"enabled": true, "items": [{"id": "video", "name": "Video", "enabled": true,
            "minutesPerDay": 20, "minutesPerWeek": 120, "entries": [
                {"id": "one", "mobileAppPackage": "com.example.video"},
                {"id": "duplicate", "mobileAppPackage": "com.example.video"},
                {"id": "browser", "websiteHost": "video.example"}]},
            {"id": "manual-name", "minutesPerDay": 10, "entries": [{"id": "name", "mobileAppName": "Video"}]}]}}})
    }

    #[test]
    fn native_rules_preserve_package_selection_and_capture_baseline_with_cross_platform_totals() {
        let sources = vec![
            UsageSourceDay {
                source_type: "website".into(),
                source_key: "video.example".into(),
                local_date: "2026-10-02".into(),
                elapsed_seconds: 880,
            },
            UsageSourceDay {
                source_type: "mobile-app".into(),
                source_key: "com.example.video".into(),
                local_date: "2026-10-02".into(),
                elapsed_seconds: 20,
            },
        ];
        let (rules, view) = build(&root(), &capture(), &sources, &copy(), "native-1").unwrap();
        assert_eq!(rules["limits"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            rules["limits"]["items"][0]["packages"],
            json!(["com.example.video"])
        );
        assert_eq!(
            rules["limits"]["items"][0]["acceptedUsage"]["day"]["usedSeconds"],
            900
        );
        assert_eq!(
            rules["limits"]["items"][0]["acceptedUsage"]["day"]["localUsedSecondsAtCapture"],
            20
        );
        assert_eq!(rules["mobile"]["blockDuringShortBreaks"], false);
        assert_eq!(view["totals"][0]["entries"][1]["usedSeconds"], 0);
    }

    fn published_packages(rules: &Value) -> Vec<&str> {
        rules["mobile"]["blockedApps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule["packageName"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn projection_skips_invalid_rule_and_keeps_valid_rules() {
        let mut config = root();
        config["distractions"]["mobile"]["blockedApps"] = json!([
            {"name": "Video", "packageName": "com.example.video", "enabled": true},
            {"name": "Hostile", "packageName": "not a package", "enabled": true},
            {"name": "   ", "packageName": "com.example.blank", "enabled": true},
            {"name": "n".repeat(MAX_RULE_NAME_CHARS + 1), "packageName": "com.example.long", "enabled": true},
            {"name": "Unpicked", "enabled": true},
            {"name": "Feed", "packageName": "Com.Example.Feed", "enabled": true},
        ]);
        let built = build(&config, &capture(), &[], &copy(), "native-skip");
        assert!(built.is_ok(), "one row stopped publication: {built:?}");
        assert_eq!(
            published_packages(&built.unwrap().0),
            ["com.example.video", "com.example.feed"]
        );
    }

    #[test]
    fn limit_parse_failure_keeps_guardian_rules() {
        let mut config = root();
        config["distractions"]["limits"]["items"][0]["entries"][2]["websiteHost"] = json!(" ");
        let built = build(&config, &capture(), &[], &copy(), "native-parse");
        assert!(built.is_ok(), "one limit stopped publication: {built:?}");
        let (rules, _) = built.unwrap();
        assert_eq!(published_packages(&rules), ["com.example.video"]);
        assert_eq!(rules["limits"], json!({"enabled": false, "items": []}));
    }

    #[test]
    fn projection_keeps_publishing_past_the_rule_cap() {
        let mut config = root();
        config["distractions"]["mobile"]["blockedApps"] = (0..=MAX_MOBILE_RULES)
            .map(|index| json!({"name": "App", "packageName": format!("com.flood.app{index}")}))
            .collect();
        let built = build(&config, &capture(), &[], &copy(), "native-cap");
        assert!(built.is_ok(), "the flood stopped publication: {built:?}");
        let (rules, _) = built.unwrap();
        let packages = published_packages(&rules);
        assert_eq!(packages.len(), MAX_MOBILE_RULES);
        assert_eq!(packages[0], "com.flood.app0");
    }

    #[test]
    fn capture_rejects_future_stale_mixed_vault_and_inconsistent_week_evidence() {
        let original = serde_json::to_value(json!({"vaultId": "vault", "journalVaultId": "vault",
            "observedAtEpochMs": now(), "utcOffsetSeconds": 0, "localDate": "2026-10-02", "weekStartLocalDate": "2026-09-28", "localSources": [], "pending": []})).unwrap();
        assert!(GuardianCapture::decode(&original.to_string(), "vault", now() - 1).is_err());
        assert!(GuardianCapture::decode(&original.to_string(), "vault", now() + 10_001).is_err());
        assert!(GuardianCapture::decode(&original.to_string(), "other", now()).is_err());
        let mut invalid = original;
        invalid["weekStartLocalDate"] = json!("2026-09-29");
        assert!(GuardianCapture::decode(&invalid.to_string(), "vault", now()).is_err());
    }

    #[test]
    fn disabled_policy_remains_visible_without_enabling_guardian_enforcement() {
        let mut config = root();
        config["distractions"]["limits"]["enabled"] = json!(false);
        config["distractions"]["limits"]["items"][0]["enabled"] = json!(false);
        let (rules, view) = build(&config, &capture(), &[], &copy(), "native-2").unwrap();
        assert_eq!(rules["limits"]["enabled"], false);
        assert_eq!(rules["limits"]["items"][0]["enabled"], false);
        assert_eq!(view["totals"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn accounting_includes_the_complete_journal_and_excludes_accepted_identities_once() {
        let mut capture = capture();
        capture.pending = (0..500)
            .map(|index| PendingEvent {
                id: format!("event-{index}"),
                kind: "usage".into(),
                package_name: "com.example.video".into(),
                display_name: "Video".into(),
                started_at_ms: now() - 2_000,
                elapsed_seconds: 2,
                local_date: "2026-10-02".into(),
                occurred_at_ms: now(),
                reason: None,
                rule_id: None,
                run_id: None,
                phase: None,
                vault_id: "vault".into(),
            })
            .collect();
        let excluded = (0..100).map(|index| format!("event-{index}")).collect();
        let sources = include_pending(
            vec![UsageSourceDay {
                source_type: "mobile-app".into(),
                source_key: "com.example.video".into(),
                local_date: "2026-10-02".into(),
                elapsed_seconds: 1_000,
            }],
            &capture,
            &excluded,
        )
        .unwrap()
        .sources;
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].elapsed_seconds, 1_800);
        let (_, view) = build(&root(), &capture, &sources, &copy(), "native-full-window").unwrap();
        assert_eq!(view["totals"][0]["usedSeconds"], 1_800);
    }

    #[test]
    fn captured_date_must_match_the_native_instant_and_offset() {
        let mut captured = capture();
        captured.local_date = "2026-10-01".into();
        assert!(captured.ensure_current("vault", now()).is_err());
        let mut captured = capture();
        captured.utc_offset_seconds = 86_400;
        assert!(captured.ensure_current("vault", now()).is_err());
    }

    #[test]
    fn midnight_rollover_rejects_an_otherwise_fresh_capture_before_publication() {
        let mut captured = capture();
        captured.observed_at_epoch_ms = "2026-10-02T23:59:59Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        assert!(
            captured
                .ensure_current("vault", captured.observed_at_epoch_ms)
                .is_ok()
        );
        assert!(
            captured
                .ensure_current("vault", captured.observed_at_epoch_ms + 1_000)
                .is_err()
        );
        captured.utc_offset_seconds = -6 * 60 * 60;
        assert!(
            captured
                .ensure_current("vault", captured.observed_at_epoch_ms + 1_000)
                .is_ok()
        );
    }

    #[test]
    fn capture_rejects_duplicate_cross_vault_and_out_of_window_pending_evidence() {
        let mut value = capture_value();
        let pending = pending_value();
        value["pending"] = json!([pending.clone(), pending]);
        assert!(GuardianCapture::decode(&value.to_string(), "vault", now()).is_err());
        for (key, invalid) in [
            ("kind", json!("block")),
            ("vaultId", json!("another-vault")),
            ("localDate", json!("2026-09-27")),
            ("elapsedSeconds", json!(-1)),
            ("id", json!(" changed-identity ")),
        ] {
            let mut pending = pending_value();
            pending[key] = invalid;
            value["pending"] = json!([pending]);
            assert!(
                GuardianCapture::decode(&value.to_string(), "vault", now()).is_err(),
                "{key}"
            );
        }
        value["pending"] = json!(vec![pending_value(); MAX_JOURNAL_EVENTS + 1]);
        assert!(GuardianCapture::decode(&value.to_string(), "vault", now()).is_err());
        assert!(
            GuardianCapture::decode(&" ".repeat(MAX_CAPTURE_BYTES + 1), "vault", now()).is_err()
        );
    }

    #[test]
    fn local_baseline_rejects_case_alias_duplicates_and_counter_scope_corruption() {
        let mut value = capture_value();
        let mut duplicate = value["localSources"][0].clone();
        duplicate["sourceKey"] = json!("COM.EXAMPLE.VIDEO");
        value["localSources"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(GuardianCapture::decode(&value.to_string(), "vault", now()).is_err());
        let mut value = capture_value();
        value["journalVaultId"] = json!("old-vault");
        assert!(GuardianCapture::decode(&value.to_string(), "vault", now()).is_err());
        value["localSources"] = json!([]);
        assert!(GuardianCapture::decode(&value.to_string(), "vault", now()).is_ok());
        for (key, invalid) in [
            ("sourceType", json!("website")),
            ("sourceKey", json!("missing-package")),
            ("localDate", json!("2026-02-30")),
            ("elapsedSeconds", json!(-1)),
            ("elapsedSeconds", json!(MAX_SAFE_SECONDS + 1)),
        ] {
            let mut value = capture_value();
            value["localSources"][0][key] = invalid;
            assert!(
                GuardianCapture::decode(&value.to_string(), "vault", now()).is_err(),
                "{key}"
            );
        }
    }

    #[test]
    fn complete_pending_aggregation_rejects_overflow_without_clamping() {
        let mut capture = capture();
        capture.pending = vec![serde_json::from_value(pending_value()).unwrap()];
        let source = UsageSourceDay {
            source_type: "mobile-app".into(),
            source_key: "com.example.video".into(),
            local_date: "2026-10-02".into(),
            elapsed_seconds: MAX_SAFE_SECONDS,
        };
        assert!(include_pending(vec![source.clone()], &capture, &HashSet::new()).is_err());
        let excluded = HashSet::from(["pending".into()]);
        assert_eq!(
            include_pending(vec![source], &capture, &excluded)
                .unwrap()
                .sources[0]
                .elapsed_seconds,
            MAX_SAFE_SECONDS
        );
    }

    #[test]
    fn failed_read_reuses_only_a_matching_window_and_else_local_usage() {
        let mut capture = capture();
        capture.local_sources = vec![CapturedSource {
            source_type: "mobile-app".into(),
            source_key: "com.example.local".into(),
            local_date: capture.local_date.clone(),
            elapsed_seconds: 45,
        }];
        let usage = LastGoodUsage {
            generation: 3,
            vault_id: capture.vault_id.clone(),
            rules_revision: 7,
            local_date: capture.local_date.clone(),
            sources: vec![UsageSourceDay {
                source_type: "website".into(),
                source_key: "example.com".into(),
                local_date: capture.local_date.clone(),
                elapsed_seconds: 600,
            }],
        };
        let keys = |sources: Vec<UsageSourceDay>| {
            sources
                .into_iter()
                .map(|source| source.source_key)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            keys(fallback_sources(Some(&usage), &capture, 3, 7)),
            ["example.com"]
        );
        assert_eq!(
            keys(fallback_sources(None, &capture, 3, 7)),
            ["com.example.local"]
        );
        assert_eq!(
            keys(fallback_sources(Some(&usage), &capture, 4, 7)),
            ["com.example.local"]
        );
        assert_eq!(
            keys(fallback_sources(Some(&usage), &capture, 3, 8)),
            ["com.example.local"]
        );
        for mismatch in [
            LastGoodUsage {
                vault_id: "other".into(),
                ..usage.clone()
            },
            LastGoodUsage {
                local_date: "2026-01-01".into(),
                ..usage.clone()
            },
        ] {
            assert_eq!(
                keys(fallback_sources(Some(&mismatch), &capture, 3, 7)),
                ["com.example.local"]
            );
        }
    }

    #[test]
    fn include_pending_stops_at_cap_keeping_local() {
        let mut capture = capture();
        capture.pending = vec![serde_json::from_value(pending_value()).unwrap()];
        let own = UsageSourceDay {
            source_type: "mobile-app".into(),
            source_key: "com.example.own".into(),
            local_date: "2026-09-28".into(),
            elapsed_seconds: 30,
        };
        let flood = (0..limits::MAX_SOURCE_GROUPS).map(|index| UsageSourceDay {
            source_type: "mobile-app".into(),
            source_key: format!("com.flood.app{index}"),
            local_date: "2026-10-02".into(),
            elapsed_seconds: 60,
        });
        let sources = std::iter::once(own.clone()).chain(flood).collect();
        let window = include_pending(sources, &capture, &HashSet::new());
        assert!(window.is_ok(), "the flood failed the merge: {window:?}");
        let window = window.unwrap();
        assert!(window.truncated);
        assert_eq!(window.sources.len(), limits::MAX_SOURCE_GROUPS);
        assert_eq!(window.sources[1], own);
        assert!(window.sources.iter().any(|row| {
            row.source_key == "com.example.video" && row.local_date == "2026-10-02"
        }));
    }
}
