//! Claude rate limit and API retry normalization.

use super::blocks::ClaudeStreamScope;
use super::{ClaudeEventNormalizer, ClaudeRouteState, bounded_text, text};
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Warning code of a Claude API request the CLI is retrying.
const API_RETRY_WARNING_CODE: &str = "claude_api_retry";
const API_RETRY_TITLE: &str = "Claude is retrying the API request";
/// Latest reset instant accepted, 9999-12-31T23:59:59Z, the last one RFC 3339 can write.
const MAX_RESET_EPOCH_SECONDS: i64 = 253_402_300_799;
/// Byte limit of each string Claude reports about a rate limit.
const MAX_LIMIT_FIELD_BYTES: usize = 64;
/// Rate limit types remembered for change detection; Claude reports a handful.
const MAX_TRACKED_RATE_LIMIT_TYPES: usize = 16;
/// Upper bound of a retry attempt or attempt maximum shown in a notice.
const MAX_RETRY_ATTEMPTS: u64 = 1_000;
/// Upper bound of a retry delay shown in a notice, one hour.
const MAX_RETRY_DELAY_MS: u64 = 3_600_000;
const MILLISECONDS_PER_SECOND: u64 = 1_000;
const HTTP_STATUS_RANGE: std::ops::RangeInclusive<u64> = 100..=599;

/// The merged state of one Claude limit type.
#[derive(Clone, Debug, Default, PartialEq)]
struct ClaudeRateLimitEntry {
    status: String,
    resets_at: Option<i64>,
    overage_status: Option<String>,
    is_using_overage: Option<bool>,
    overage_in_use: Option<bool>,
    utilization: Option<f64>,
}

impl ClaudeRateLimitEntry {
    /// Applies one report. The status and reset instant always come from it; overage and
    /// utilization fields it omits, as the blocked assistant notice does, keep their values.
    fn merge(&mut self, info: &Map<String, Value>) {
        self.status = bounded_text(
            text(info, "status").unwrap_or("unknown"),
            MAX_LIMIT_FIELD_BYTES,
        );
        self.resets_at = info.get("resetsAt").and_then(reset_epoch_seconds);
        if let Some(overage) = text(info, "overageStatus") {
            self.overage_status = Some(bounded_text(overage, MAX_LIMIT_FIELD_BYTES));
        }
        if let Some(flag) = info.get("isUsingOverage").and_then(Value::as_bool) {
            self.is_using_overage = Some(flag);
        }
        if let Some(flag) = info.get("overageInUse").and_then(Value::as_bool) {
            self.overage_in_use = Some(flag);
        }
        if let Some(utilization) = info
            .get("utilization")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
        {
            self.utilization = Some(utilization);
        }
    }

    /// Whether this type blocks requests: rejected, with no overage billing letting them run.
    fn limited(&self) -> bool {
        let overage_allowed = matches!(
            self.overage_status.as_deref(),
            Some("allowed" | "allowed_warning")
        ) || self.is_using_overage == Some(true)
            || self.overage_in_use == Some(true);
        self.status == "rejected" && !overage_allowed
    }

    fn projection(&self, limit_type: &str) -> Value {
        let mut data = Map::new();
        data.insert("rateLimitType".to_string(), json!(limit_type));
        data.insert("status".to_string(), json!(self.status));
        data.insert("resetsAt".to_string(), json!(self.resets_at));
        if let Some(overage) = &self.overage_status {
            data.insert("overageStatus".to_string(), json!(overage));
        }
        if let Some(flag) = self.is_using_overage {
            data.insert("isUsingOverage".to_string(), json!(flag));
        }
        if let Some(flag) = self.overage_in_use {
            data.insert("overageInUse".to_string(), json!(flag));
        }
        if let Some(utilization) = self.utilization {
            data.insert("utilization".to_string(), json!(utilization));
        }
        Value::Object(data)
    }
}

/// The merged state of every Claude limit type seen in a session.
///
/// Claude reports each limit type separately, so one type's report never hides another's
/// rejection. The CLI repeats unchanged `rate_limit_event` frames and the blocked notice on
/// every rejected response, so only a change of a type's status or reset instant is reported;
/// other fields still merge silently and appear in the next report.
#[derive(Clone, Debug, Default)]
pub(in crate::claude) struct ClaudeRateLimitMemory {
    entries: BTreeMap<String, ClaudeRateLimitEntry>,
}

impl ClaudeRateLimitMemory {
    /// Merges one report for `limit_type` and reports whether its status or reset changed.
    ///
    /// A type beyond the tracked bound is ignored so known types keep their state.
    fn merge(&mut self, limit_type: &str, info: &Map<String, Value>) -> bool {
        if !self.entries.contains_key(limit_type)
            && self.entries.len() >= MAX_TRACKED_RATE_LIMIT_TYPES
        {
            return false;
        }
        let entry = self.entries.entry(limit_type.to_string()).or_default();
        let previous = (entry.status.clone(), entry.resets_at);
        let known = !entry.status.is_empty();
        entry.merge(info);
        !known || previous != (entry.status.clone(), entry.resets_at)
    }

    /// Whether any type blocks requests.
    fn limited(&self) -> bool {
        self.entries.values().any(ClaudeRateLimitEntry::limited)
    }

    /// The latest reset among blocking types, since each must reset before a request can run;
    /// None when nothing blocks or a blocking type has no reset instant.
    fn resets_at(&self) -> Option<i64> {
        self.entries
            .values()
            .filter(|entry| entry.limited())
            .map(|entry| entry.resets_at)
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .max()
    }

    fn projection(&self) -> Value {
        let types = self
            .entries
            .iter()
            .map(|(limit_type, entry)| (limit_type.clone(), entry.projection(limit_type)))
            .collect::<Map<String, Value>>();
        json!({ "rateLimitsByType": types })
    }
}

impl ClaudeEventNormalizer {
    /// Maps a top-level `rate_limit_event`.
    pub(super) fn rate_limit_event(
        &self,
        state: &mut ClaudeRouteState,
        object: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        match object.get("rate_limit_info").and_then(Value::as_object) {
            Some(info) => self.rate_limit_status(state, info, "rate_limit_event"),
            None => Ok(Vec::new()),
        }
    }

    /// Maps the rate limit a rejected root assistant response carries in `api_error_params`.
    pub(super) fn blocked_assistant(
        &self,
        state: &mut ClaudeRouteState,
        object: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let info = object
            .get("api_error_params")
            .and_then(|params| params.get("rate_limit_info"))
            .and_then(Value::as_object)
            .filter(|info| text(info, "status") == Some("rejected"));
        match info {
            Some(info) => self.rate_limit_status(state, info, "assistant/rate_limit"),
            None => Ok(Vec::new()),
        }
    }

    /// Maps a root `system/api_retry` to a warning naming the attempt, its maximum, and the
    /// delay. A subagent's retries stay inside its task, like the rest of its stream.
    pub(super) fn api_retry(
        &self,
        state: &ClaudeRouteState,
        object: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        if !ClaudeStreamScope::of(object).is_root() {
            return Ok(Vec::new());
        }
        Ok(vec![self.event(
            state,
            "system/api_retry",
            None,
            None,
            None,
            CanonicalEvent::RuntimeWarning(NotificationEvent {
                code: API_RETRY_WARNING_CODE.to_string(),
                title: API_RETRY_TITLE.to_string(),
                detail: Some(api_retry_detail(object)),
            }),
        )?])
    }

    fn rate_limit_status(
        &self,
        state: &mut ClaudeRouteState,
        info: &Map<String, Value>,
        source: &str,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let limit_type = bounded_text(
            text(info, "rateLimitType").unwrap_or("unknown"),
            MAX_LIMIT_FIELD_BYTES,
        );
        if !state.rate_limits.merge(&limit_type, info) {
            return Ok(Vec::new());
        }
        let limited = state.rate_limits.limited();
        let resets_at = state.rate_limits.resets_at();
        let data = state.rate_limits.projection();
        Ok(vec![self.event(
            state,
            source,
            None,
            None,
            None,
            CanonicalEvent::RateLimitStatus(RateLimitStatusEvent {
                limited,
                resets_at: resets_at.and_then(utc_timestamp),
                detail: None,
                provider_data: Some(VersionedJson {
                    schema_version: 1,
                    value: data,
                }),
            }),
        )?])
    }
}

/// Reads `resetsAt`, which Claude reports in epoch seconds, within the range RFC 3339 can write.
fn reset_epoch_seconds(value: &Value) -> Option<i64> {
    let seconds = value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(|value| value as i64)
    })?;
    (1..=MAX_RESET_EPOCH_SECONDS)
        .contains(&seconds)
        .then_some(seconds)
}

fn utc_timestamp(seconds: i64) -> Option<UtcTimestamp> {
    let instant = chrono::DateTime::from_timestamp(seconds, 0)?;
    UtcTimestamp::new(instant.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).ok()
}

fn api_retry_detail(object: &Map<String, Value>) -> String {
    let clamped = |key: &str, maximum: u64| {
        object
            .get(key)
            .and_then(Value::as_u64)
            .map(|value| value.min(maximum))
    };
    let attempt = clamped("attempt", MAX_RETRY_ATTEMPTS).unwrap_or(1).max(1);
    let mut detail = match clamped("max_retries", MAX_RETRY_ATTEMPTS) {
        Some(maximum) => format!("Attempt {attempt} of {}", maximum.max(attempt)),
        None => format!("Attempt {attempt}"),
    };
    if let Some(delay) = clamped("retry_delay_ms", MAX_RETRY_DELAY_MS) {
        detail.push_str(&format!(
            ", next try in {} s",
            delay.div_ceil(MILLISECONDS_PER_SECOND)
        ));
    }
    if let Some(status) = object
        .get("error_status")
        .and_then(Value::as_u64)
        .filter(|status| HTTP_STATUS_RANGE.contains(status))
    {
        detail.push_str(&format!(" (HTTP {status})"));
    }
    detail
}
