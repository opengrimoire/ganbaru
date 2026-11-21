//! Merging a provider's usage limit reports so a report without usage numbers keeps the
//! numbers an earlier report read for the same windows.

use ganbaru_chat_contracts::events::RateLimitStatusEvent;
use serde_json::{Map, Value};

/// Seconds two reset instants of one window may differ by: a probe and a turn round the same
/// reset to whole seconds from different sources.
const SAME_WINDOW_TOLERANCE_SECONDS: i64 = 60;
/// Limit types a merged report holds at most, the bound the Claude normalizer tracks.
const MAX_LIMIT_TYPES: usize = 16;

/// What a report said about usage numbers once merged with the report before it.
#[derive(Debug, Default, Eq, PartialEq)]
pub(super) struct UsageNumbers {
    /// The report read usage numbers itself.
    pub read: bool,
    /// The report keeps numbers an earlier report read.
    pub carried: bool,
}

/// Carries the usage numbers of `previous` into `next` where `next` reports a window without
/// them, as a Claude turn does: its `rate_limit_event` frames name each window's status and
/// reset but usually not its utilization, which only the plan usage read returns.
///
/// A window of `previous` that `next` names keeps its utilization while its reset instant is
/// unchanged, and a window `next` does not name stays while it has not reset at
/// `observed_at_seconds` and was not rejected, since a turn reports only the windows its
/// session saw. A report without provider data keeps all of the earlier data, and reports in
/// another shape read their numbers themselves and are left as they are.
pub(super) fn carry_usage_numbers(
    previous: Option<&RateLimitStatusEvent>,
    next: &mut RateLimitStatusEvent,
    observed_at_seconds: i64,
) -> UsageNumbers {
    if next.provider_data.is_none() {
        next.provider_data = previous.and_then(|status| status.provider_data.clone());
        return UsageNumbers {
            read: false,
            carried: next.provider_data.is_some(),
        };
    }
    let Some(next_types) = limit_types_mut(next) else {
        return UsageNumbers {
            read: true,
            carried: false,
        };
    };
    let read = next_types
        .values()
        .any(|entry| utilization(entry).is_some());
    let Some(previous_types) = previous.and_then(limit_types) else {
        return UsageNumbers {
            read,
            carried: false,
        };
    };
    let mut carried = false;
    for (limit_type, previous_entry) in previous_types {
        let Some(previous_utilization) = utilization(previous_entry) else {
            continue;
        };
        let previous_reset = resets_at(previous_entry);
        match next_types.get_mut(limit_type) {
            Some(Value::Object(entry)) => {
                if entry.get("utilization").and_then(Value::as_f64).is_none()
                    && same_window(previous_reset, resets_at_of(entry), observed_at_seconds)
                {
                    entry.insert("utilization".to_string(), Value::from(previous_utilization));
                    carried = true;
                }
            }
            Some(_) => {}
            None => {
                let open = previous_reset.is_some_and(|reset| reset > observed_at_seconds);
                let rejected =
                    previous_entry.get("status").and_then(Value::as_str) == Some("rejected");
                if open && !rejected && next_types.len() < MAX_LIMIT_TYPES {
                    next_types.insert(limit_type.clone(), previous_entry.clone());
                    carried = true;
                }
            }
        }
    }
    UsageNumbers { read, carried }
}

/// Whether a window that reset at `previous` is the window that resets at `next`. A report
/// without a reset instant names the same window while the earlier one has not reset.
fn same_window(previous: Option<i64>, next: Option<i64>, observed_at_seconds: i64) -> bool {
    match (previous, next) {
        (Some(previous), Some(next)) => {
            (previous - next).abs() <= SAME_WINDOW_TOLERANCE_SECONDS
                && previous > observed_at_seconds
        }
        (Some(previous), None) => previous > observed_at_seconds,
        (None, _) => false,
    }
}

fn limit_types(status: &RateLimitStatusEvent) -> Option<&Map<String, Value>> {
    status
        .provider_data
        .as_ref()?
        .value
        .get("rateLimitsByType")?
        .as_object()
}

fn limit_types_mut(status: &mut RateLimitStatusEvent) -> Option<&mut Map<String, Value>> {
    status
        .provider_data
        .as_mut()?
        .value
        .get_mut("rateLimitsByType")?
        .as_object_mut()
}

fn utilization(entry: &Value) -> Option<f64> {
    entry
        .get("utilization")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn resets_at(entry: &Value) -> Option<i64> {
    entry.as_object().and_then(resets_at_of)
}

fn resets_at_of(entry: &Map<String, Value>) -> Option<i64> {
    entry.get("resetsAt").and_then(Value::as_i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_chat_contracts::models::VersionedJson;
    use serde_json::json;

    const NOW: i64 = 1_800_000_000;

    fn claude(types: Value) -> RateLimitStatusEvent {
        RateLimitStatusEvent {
            limited: false,
            resets_at: None,
            detail: None,
            provider_data: Some(VersionedJson {
                schema_version: 1,
                value: json!({ "rateLimitsByType": types }),
            }),
        }
    }

    fn types(status: &RateLimitStatusEvent) -> &Map<String, Value> {
        limit_types(status).unwrap()
    }

    fn probe() -> RateLimitStatusEvent {
        claude(json!({
            "five_hour": { "rateLimitType": "five_hour", "status": "allowed", "resetsAt": NOW + 3_600, "utilization": 0.4 },
            "seven_day": { "rateLimitType": "seven_day", "status": "allowed", "resetsAt": NOW + 86_400, "utilization": 0.1 },
        }))
    }

    #[test]
    fn a_turn_report_keeps_the_numbers_of_the_same_windows() {
        let mut turn = claude(json!({
            "five_hour": { "rateLimitType": "five_hour", "status": "allowed", "resetsAt": NOW + 3_600 },
        }));
        let numbers = carry_usage_numbers(Some(&probe()), &mut turn, NOW);
        assert_eq!(
            numbers,
            UsageNumbers {
                read: false,
                carried: true
            }
        );
        assert_eq!(types(&turn)["five_hour"]["utilization"], json!(0.4));
        assert_eq!(
            types(&turn)["seven_day"]["utilization"],
            json!(0.1),
            "a window the turn's session did not see stays while it is open"
        );
    }

    #[test]
    fn a_window_that_reset_does_not_keep_old_numbers() {
        let mut turn = claude(json!({
            "five_hour": { "rateLimitType": "five_hour", "status": "allowed", "resetsAt": NOW + 5 * 3_600 },
        }));
        let numbers = carry_usage_numbers(Some(&probe()), &mut turn, NOW + 4_000);
        assert!(!numbers.read);
        assert!(types(&turn)["five_hour"].get("utilization").is_none());
        assert!(
            types(&turn)["seven_day"].get("utilization").is_some(),
            "the week window is still open"
        );
    }

    #[test]
    fn a_report_with_its_own_numbers_keeps_them() {
        let mut later = claude(json!({
            "five_hour": { "rateLimitType": "five_hour", "status": "allowed", "resetsAt": NOW + 3_600, "utilization": 0.7 },
        }));
        let numbers = carry_usage_numbers(Some(&probe()), &mut later, NOW);
        assert!(numbers.read);
        assert_eq!(types(&later)["five_hour"]["utilization"], json!(0.7));
    }

    #[test]
    fn a_rejected_window_the_report_leaves_out_is_not_carried() {
        let previous = claude(json!({
            "seven_day": { "rateLimitType": "seven_day", "status": "rejected", "resetsAt": NOW + 86_400, "utilization": 1.0 },
        }));
        let mut turn = claude(json!({
            "five_hour": { "rateLimitType": "five_hour", "status": "allowed", "resetsAt": NOW + 3_600 },
        }));
        let numbers = carry_usage_numbers(Some(&previous), &mut turn, NOW);
        assert_eq!(numbers, UsageNumbers::default());
        assert!(!types(&turn).contains_key("seven_day"));
    }

    #[test]
    fn a_report_without_data_keeps_the_earlier_data() {
        let mut bare = RateLimitStatusEvent {
            limited: true,
            resets_at: None,
            detail: None,
            provider_data: None,
        };
        let numbers = carry_usage_numbers(Some(&probe()), &mut bare, NOW);
        assert_eq!(
            numbers,
            UsageNumbers {
                read: false,
                carried: true
            }
        );
        assert_eq!(types(&bare)["five_hour"]["utilization"], json!(0.4));
    }

    #[test]
    fn another_shape_reads_its_own_numbers() {
        let mut codex = RateLimitStatusEvent {
            limited: false,
            resets_at: None,
            detail: None,
            provider_data: Some(VersionedJson {
                schema_version: 1,
                value: json!({ "rateLimitsByLimitId": {} }),
            }),
        };
        let numbers = carry_usage_numbers(Some(&probe()), &mut codex, NOW);
        assert_eq!(
            numbers,
            UsageNumbers {
                read: true,
                carried: false
            }
        );
    }
}
