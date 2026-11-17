//! Codex rate limit snapshot merging and projection.
//!
//! `account/rateLimits/updated` is a sparse rolling update: a field that is absent keeps the
//! value last observed for that limit, an explicit null clears a window or the reached state,
//! and nullable metadata such as the limit name never clears a known value.

use super::value::{bounded_text, text};
use crate::events::RateLimitStatusEvent;
use crate::models::{UtcTimestamp, VersionedJson};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Limit id of the main Codex allowance, which Codex versions without `limitId` report.
const DEFAULT_LIMIT_ID: &str = "codex";
/// Metered limits remembered per session; Codex reports the main allowance and a few models.
const MAX_TRACKED_RATE_LIMITS: usize = 16;
/// Byte limit of a limit id, name, or reached type.
const MAX_LIMIT_FIELD_BYTES: usize = 128;
/// Usage percentage at which a window blocks further requests.
const FULL_USAGE_PERCENT: i64 = 100;
/// Longest window duration accepted, one leap year in minutes.
const MAX_WINDOW_DURATION_MINS: i64 = 366 * 24 * 60;
/// Latest reset instant accepted, 9999-12-31T23:59:59Z, the last one RFC 3339 can write.
const MAX_RESET_EPOCH_SECONDS: i64 = 253_402_300_799;

/// One usage window of a Codex limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CodexRateLimitWindow {
    used_percent: i64,
    window_duration_mins: Option<i64>,
    resets_at: Option<i64>,
}

impl CodexRateLimitWindow {
    fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let used_percent = object
            .get("usedPercent")
            .and_then(whole_number)?
            .clamp(0, FULL_USAGE_PERCENT);
        Some(Self {
            used_percent,
            window_duration_mins: object
                .get("windowDurationMins")
                .and_then(whole_number)
                .filter(|minutes| (1..=MAX_WINDOW_DURATION_MINS).contains(minutes)),
            resets_at: object
                .get("resetsAt")
                .and_then(whole_number)
                .filter(|seconds| (1..=MAX_RESET_EPOCH_SECONDS).contains(seconds)),
        })
    }

    fn blocked(&self) -> bool {
        self.used_percent >= FULL_USAGE_PERCENT
    }

    fn projection(&self) -> Value {
        json!({
            "usedPercent": self.used_percent,
            "windowDurationMins": self.window_duration_mins,
            "resetsAt": self.resets_at,
        })
    }
}

/// The merged state of one metered Codex limit.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CodexRateLimitSnapshot {
    limit_name: Option<String>,
    primary: Option<CodexRateLimitWindow>,
    secondary: Option<CodexRateLimitWindow>,
    reached_type: Option<String>,
}

impl CodexRateLimitSnapshot {
    fn merge(&mut self, update: &Map<String, Value>) {
        if let Some(name) = text(update, "limitName") {
            self.limit_name = Some(bounded_text(name, MAX_LIMIT_FIELD_BYTES));
        }
        merge_window(&mut self.primary, update.get("primary"));
        merge_window(&mut self.secondary, update.get("secondary"));
        match update.get("rateLimitReachedType") {
            None => {}
            Some(Value::String(reached)) => {
                self.reached_type = Some(bounded_text(reached, MAX_LIMIT_FIELD_BYTES));
            }
            Some(_) => self.reached_type = None,
        }
    }

    fn windows(&self) -> impl Iterator<Item = &CodexRateLimitWindow> {
        self.primary.iter().chain(self.secondary.iter())
    }

    fn limited(&self) -> bool {
        self.reached_type.is_some() || self.windows().any(CodexRateLimitWindow::blocked)
    }

    fn projection(&self, limit_id: &str) -> Value {
        json!({
            "limitId": limit_id,
            "limitName": self.limit_name,
            "primary": self.primary.as_ref().map(CodexRateLimitWindow::projection),
            "secondary": self.secondary.as_ref().map(CodexRateLimitWindow::projection),
            "rateLimitReachedType": self.reached_type,
        })
    }
}

/// An absent window keeps the previous one, null clears it, and an unreadable one is ignored.
fn merge_window(slot: &mut Option<CodexRateLimitWindow>, update: Option<&Value>) {
    match update {
        None => {}
        Some(Value::Null) => *slot = None,
        Some(value) => {
            if let Some(window) = CodexRateLimitWindow::parse(value) {
                *slot = Some(window);
            }
        }
    }
}

/// Reads an integer, accepting a finite float the way JSON encoders sometimes write one.
fn whole_number(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|number| number.is_finite())
            .map(|number| number.round() as i64)
    })
}

/// The latest merged snapshot of every Codex limit seen in a session.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CodexRateLimits {
    snapshots: BTreeMap<String, CodexRateLimitSnapshot>,
}

/// The rate limit status a merged update projects.
pub(super) struct CodexRateLimitProjection {
    pub limited: bool,
    pub resets_at: Option<UtcTimestamp>,
    pub provider_data: VersionedJson,
}

impl CodexRateLimits {
    /// Merges one sparse snapshot and reports whether the merged state changed.
    ///
    /// A limit id beyond the tracked bound is ignored so known limits keep their state.
    pub(super) fn merge(&mut self, update: &Map<String, Value>) -> bool {
        let limit_id = text(update, "limitId")
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| bounded_text(id, MAX_LIMIT_FIELD_BYTES))
            .unwrap_or_else(|| DEFAULT_LIMIT_ID.to_string());
        let previous = self.snapshots.get(&limit_id).cloned();
        if previous.is_none() && self.snapshots.len() >= MAX_TRACKED_RATE_LIMITS {
            return false;
        }
        let mut snapshot = previous.clone().unwrap_or_default();
        snapshot.merge(update);
        if previous.unwrap_or_default() == snapshot {
            return false;
        }
        self.snapshots.insert(limit_id, snapshot);
        true
    }

    /// Projects every merged limit in the shape the composer reads.
    ///
    /// `resets_at` is the latest reset among blocked windows, since every exhausted window
    /// must reset before a request can run; it is None when a blocked window has no reset.
    pub(super) fn projection(&self) -> CodexRateLimitProjection {
        let limited = self.snapshots.values().any(CodexRateLimitSnapshot::limited);
        let blocked = self
            .snapshots
            .values()
            .flat_map(CodexRateLimitSnapshot::windows)
            .filter(|window| window.blocked())
            .map(|window| window.resets_at)
            .collect::<Vec<_>>();
        let resets_at = if blocked.is_empty() {
            None
        } else {
            blocked
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .and_then(|resets| resets.into_iter().max())
                .and_then(utc_timestamp)
        };
        let buckets = self
            .snapshots
            .iter()
            .map(|(id, snapshot)| (id.clone(), snapshot.projection(id)))
            .collect::<Map<String, Value>>();
        CodexRateLimitProjection {
            limited,
            resets_at,
            provider_data: VersionedJson {
                schema_version: 1,
                value: json!({ "rateLimitsByLimitId": buckets }),
            },
        }
    }

    /// Reads an `account/rateLimits/read` response into the status a turn would report.
    ///
    /// The response carries the main allowance in `rateLimits` and every metered limit in
    /// `rateLimitsByLimitId`, each in the same snapshot shape as the rolling update. Returns
    /// `None` when the response names no readable limit, as for API key accounts.
    pub fn status_from_read_response(response: &Value) -> Option<RateLimitStatusEvent> {
        let object = response.as_object()?;
        let mut limits = Self::default();
        if let Some(main) = object.get("rateLimits").and_then(Value::as_object) {
            limits.merge(main);
        }
        if let Some(by_id) = object.get("rateLimitsByLimitId").and_then(Value::as_object) {
            for (limit_id, snapshot) in by_id {
                let Some(snapshot) = snapshot.as_object() else {
                    continue;
                };
                let mut snapshot = snapshot.clone();
                snapshot
                    .entry("limitId")
                    .or_insert_with(|| Value::String(limit_id.clone()));
                limits.merge(&snapshot);
            }
        }
        if limits
            .snapshots
            .values()
            .all(|snapshot| snapshot.windows().next().is_none())
        {
            return None;
        }
        let projection = limits.projection();
        Some(RateLimitStatusEvent {
            limited: projection.limited,
            resets_at: projection.resets_at,
            detail: None,
            provider_data: Some(projection.provider_data),
        })
    }
}

fn utc_timestamp(seconds: i64) -> Option<UtcTimestamp> {
    let instant = chrono::DateTime::from_timestamp(seconds, 0)?;
    UtcTimestamp::new(instant.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).ok()
}
