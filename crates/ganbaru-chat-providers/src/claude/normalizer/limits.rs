//! Claude rate limit and API retry normalization.

use super::{ClaudeEventNormalizer, ClaudeRouteState, bounded_text, text};
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

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

/// The last rate limit state reported per limit type.
///
/// The CLI repeats unchanged `rate_limit_event` frames and the blocked notice on every rejected
/// response, so only a change of status or reset instant is reported.
#[derive(Clone, Debug, Default)]
pub(in crate::claude) struct ClaudeRateLimitMemory {
    last: HashMap<String, (String, Option<i64>)>,
}

impl ClaudeRateLimitMemory {
    /// Records the state of `limit_type` and reports whether it differs from the last one.
    fn changed(&mut self, limit_type: &str, status: &str, resets_at: Option<i64>) -> bool {
        let current = (status.to_string(), resets_at);
        match self.last.get_mut(limit_type) {
            Some(previous) if *previous == current => false,
            Some(previous) => {
                *previous = current;
                true
            }
            None => {
                if self.last.len() < MAX_TRACKED_RATE_LIMIT_TYPES {
                    self.last.insert(limit_type.to_string(), current);
                }
                true
            }
        }
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

    /// Maps `system/api_retry` to a warning naming the attempt, its maximum, and the delay.
    pub(super) fn api_retry(
        &self,
        state: &ClaudeRouteState,
        object: &Map<String, Value>,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event(
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
        )
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
        let status = bounded_text(
            text(info, "status").unwrap_or("unknown"),
            MAX_LIMIT_FIELD_BYTES,
        );
        let resets_at = info.get("resetsAt").and_then(reset_epoch_seconds);
        if !state.rate_limits.changed(&limit_type, &status, resets_at) {
            return Ok(Vec::new());
        }
        let limited = status == "rejected" && !overage_allowed(info);
        let mut data = Map::new();
        data.insert("rateLimitType".to_string(), json!(limit_type));
        data.insert("status".to_string(), json!(status));
        data.insert("resetsAt".to_string(), json!(resets_at));
        if let Some(overage) = text(info, "overageStatus") {
            data.insert(
                "overageStatus".to_string(),
                json!(bounded_text(overage, MAX_LIMIT_FIELD_BYTES)),
            );
        }
        for key in ["isUsingOverage", "overageInUse"] {
            if let Some(flag) = info.get(key).and_then(Value::as_bool) {
                data.insert(key.to_string(), json!(flag));
            }
        }
        if let Some(utilization) = info
            .get("utilization")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
        {
            data.insert("utilization".to_string(), json!(utilization));
        }
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
                    value: Value::Object(data),
                }),
            }),
        )?])
    }
}

/// Whether overage billing lets requests continue past a rejected limit.
fn overage_allowed(info: &Map<String, Value>) -> bool {
    matches!(
        text(info, "overageStatus"),
        Some("allowed" | "allowed_warning")
    ) || info.get("isUsingOverage").and_then(Value::as_bool) == Some(true)
        || info.get("overageInUse").and_then(Value::as_bool) == Some(true)
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
