//! Claude rate limit and API retry notice tests.

use super::streams::{active_state, normalize_lines, normalizer};
use crate::events::*;
use crate::models::*;
use serde_json::{Value, json};

const RATE_LIMITS: &str = include_str!("../fixtures/rate-limits.jsonl");

fn statuses(events: &[CanonicalRuntimeEvent]) -> Vec<&RateLimitStatusEvent> {
    events
        .iter()
        .filter_map(|event| match &event.event {
            CanonicalEvent::RateLimitStatus(status) => Some(status),
            _ => None,
        })
        .collect()
}

fn types(status: &RateLimitStatusEvent) -> &serde_json::Map<String, Value> {
    status.provider_data.as_ref().unwrap().value["rateLimitsByType"]
        .as_object()
        .expect("a merged map of limit types")
}

fn limit<'a>(status: &'a RateLimitStatusEvent, limit_type: &str, key: &str) -> &'a Value {
    &types(status)[limit_type][key]
}

fn limit_statuses(status: &RateLimitStatusEvent) -> Vec<(&str, &str)> {
    types(status)
        .iter()
        .map(|(limit_type, entry)| (limit_type.as_str(), entry["status"].as_str().unwrap()))
        .collect()
}

fn single_status(info: Value) -> RateLimitStatusEvent {
    let normalizer = normalizer();
    let mut state = active_state();
    let events = normalizer
        .normalize_message(
            &mut state,
            json!({ "type": "rate_limit_event", "rate_limit_info": info }),
        )
        .unwrap();
    statuses(&events)
        .into_iter()
        .next()
        .cloned()
        .expect("one rate limit status")
}

fn retry_warning(retry: Value) -> NotificationEvent {
    let normalizer = normalizer();
    let mut state = active_state();
    let mut message = json!({ "type": "system", "subtype": "api_retry" });
    for (key, value) in retry.as_object().unwrap() {
        message[key] = value.clone();
    }
    let events = normalizer.normalize_message(&mut state, message).unwrap();
    match &events[..] {
        [event] => match &event.event {
            CanonicalEvent::RuntimeWarning(warning) => warning.clone(),
            other => panic!("expected a runtime warning, got {other:?}"),
        },
        _ => panic!("expected one event"),
    }
}

#[test]
fn rate_limit_statuses_are_reported_once_per_change_of_each_limit() {
    let normalizer = normalizer();
    let mut state = active_state();
    let events = normalize_lines(&normalizer, &mut state, RATE_LIMITS);
    let reported = statuses(&events);
    let summary = reported
        .iter()
        .map(|status| (limit_statuses(status), status.limited))
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        [
            (vec![("five_hour", "allowed_warning")], false),
            (vec![("five_hour", "rejected")], true),
            (
                vec![("five_hour", "rejected"), ("seven_day", "allowed")],
                true
            ),
            (
                vec![("five_hour", "allowed"), ("seven_day", "allowed")],
                false
            ),
        ],
        "an unchanged frame and the blocked response repeating the rejection report nothing, \
         and another type's report keeps the rejection"
    );
    for blocked in &reported[1..3] {
        assert_eq!(
            blocked.resets_at.as_ref().map(UtcTimestamp::as_str),
            Some("2030-01-01T00:00:00Z")
        );
    }
    assert_eq!(reported[3].resets_at, None);
    assert_eq!(limit(reported[0], "five_hour", "utilization"), &json!(0.85));
    assert_eq!(
        limit(reported[1], "five_hour", "utilization"),
        &json!(0.9),
        "an unreported change still merges into the next report"
    );
    assert_eq!(
        limit(reported[1], "five_hour", "overageStatus"),
        &json!("rejected")
    );
}

#[test]
fn a_blocked_assistant_response_reports_its_rate_limit_and_text() {
    let normalizer = normalizer();
    let mut state = active_state();
    let events = normalizer
        .normalize_message(
            &mut state,
            json!({
                "type": "assistant", "parent_tool_use_id": null, "error": "rate_limit",
                "api_error_params": { "rate_limit_info": {
                    "status": "rejected", "rateLimitType": "seven_day_opus", "resetsAt": 1893456000
                }},
                "message": { "role": "assistant", "model": "<synthetic>",
                    "content": [{ "type": "text", "text": "Opus weekly limit reached" }] }
            }),
        )
        .unwrap();
    let reported = statuses(&events);
    assert_eq!(reported.len(), 1);
    assert!(reported[0].limited);
    assert_eq!(
        limit_statuses(reported[0]),
        [("seven_day_opus", "rejected")]
    );
    assert!(events.iter().any(|event| matches!(&event.event,
        CanonicalEvent::ContentDelta(delta) if delta.delta == "Opus weekly limit reached")));
}

#[test]
fn subagent_and_unrejected_assistant_limits_are_ignored() {
    let normalizer = normalizer();
    let mut state = active_state();
    for (parent, status) in [(json!("toolu_task"), "rejected"), (Value::Null, "allowed")] {
        let events = normalizer
            .normalize_message(
                &mut state,
                json!({
                    "type": "assistant", "parent_tool_use_id": parent,
                    "api_error_params": { "rate_limit_info": {
                        "status": status, "rateLimitType": "five_hour"
                    }},
                    "message": { "role": "assistant", "model": "<synthetic>", "content": [] }
                }),
            )
            .unwrap();
        assert!(statuses(&events).is_empty(), "{parent} {status}");
    }
}

#[test]
fn overage_keeps_a_rejected_limit_usable() {
    for overage in [
        json!({ "overageStatus": "allowed" }),
        json!({ "overageStatus": "allowed_warning" }),
        json!({ "isUsingOverage": true }),
        json!({ "overageInUse": true }),
    ] {
        let mut info = json!({ "status": "rejected", "rateLimitType": "five_hour" });
        for (key, value) in overage.as_object().unwrap() {
            info[key] = value.clone();
        }
        assert!(!single_status(info).limited, "{overage}");
    }
}

#[test]
fn reset_instants_outside_the_writable_range_are_dropped() {
    for resets_at in [
        json!(0),
        json!(-5),
        json!(1_893_456_000_000_i64),
        json!("1893456000"),
    ] {
        let status = single_status(json!({
            "status": "rejected", "rateLimitType": "five_hour", "resetsAt": resets_at
        }));
        assert_eq!(status.resets_at, None, "{resets_at}");
        assert_eq!(limit(&status, "five_hour", "resetsAt"), &Value::Null);
    }
}

#[test]
fn oversized_limit_strings_are_capped() {
    let status = single_status(json!({
        "status": "s".repeat(500), "rateLimitType": "t".repeat(500)
    }));
    let [(limit_type, limit_status)] = limit_statuses(&status)[..] else {
        panic!("one limit type");
    };
    assert_eq!(limit_status.len(), 64);
    assert_eq!(limit_type.len(), 64);
    assert_eq!(
        limit(&status, limit_type, "rateLimitType"),
        &json!(limit_type)
    );
    assert!(!status.limited);
}

#[test]
fn api_retries_become_warnings_with_attempt_maximum_and_delay() {
    let normalizer = normalizer();
    let mut state = active_state();
    let events = normalize_lines(&normalizer, &mut state, RATE_LIMITS);
    let warnings = events
        .iter()
        .filter_map(|event| match &event.event {
            CanonicalEvent::RuntimeWarning(warning) => Some(warning),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, "claude_api_retry");
    assert_eq!(
        warnings[0].detail.as_deref(),
        Some("Attempt 2 of 10, next try in 5 s (HTTP 529)")
    );
}

#[test]
fn api_retry_details_clamp_untrusted_numbers() {
    let huge = retry_warning(json!({
        "attempt": u64::MAX, "max_retries": 3, "retry_delay_ms": u64::MAX, "error_status": 9999
    }));
    assert_eq!(
        huge.detail.as_deref(),
        Some("Attempt 1000 of 1000, next try in 3600 s")
    );
    let sparse = retry_warning(json!({ "attempt": 0, "retry_delay_ms": "soon" }));
    assert_eq!(sparse.detail.as_deref(), Some("Attempt 1"));
}

#[test]
fn subagent_api_retries_stay_out_of_the_root_stream() {
    let normalizer = normalizer();
    let mut state = active_state();
    let retry = |parent: Value| {
        json!({
            "type": "system", "subtype": "api_retry", "parent_tool_use_id": parent,
            "attempt": 2, "max_retries": 10, "retry_delay_ms": 5000, "error_status": 529
        })
    };
    let subagent = normalizer
        .normalize_message(&mut state, retry(json!("toolu_task")))
        .unwrap();
    assert!(subagent.is_empty(), "a subagent retry projects nothing");
    for parent in [Value::Null, json!("")] {
        let root = normalizer
            .normalize_message(&mut state, retry(parent.clone()))
            .unwrap();
        assert!(
            matches!(&root[..], [event] if matches!(&event.event, CanonicalEvent::RuntimeWarning(_))),
            "{parent}"
        );
    }
}

fn rate_limit_event(info: Value) -> Value {
    json!({ "type": "rate_limit_event", "rate_limit_info": info })
}

#[test]
fn every_blocking_type_must_reset_before_requests_run() {
    let normalizer = normalizer();
    let mut state = active_state();
    let mut report = |info: Value| {
        statuses(
            &normalizer
                .normalize_message(&mut state, rate_limit_event(info))
                .unwrap(),
        )
        .into_iter()
        .next()
        .cloned()
        .expect("a changed limit is reported")
    };
    let first = report(json!({
        "status": "rejected", "rateLimitType": "five_hour", "resetsAt": 1893456000
    }));
    assert_eq!(
        first.resets_at.as_ref().map(UtcTimestamp::as_str),
        Some("2030-01-01T00:00:00Z")
    );
    let both = report(json!({
        "status": "rejected", "rateLimitType": "seven_day", "resetsAt": 1894000000
    }));
    assert!(both.limited);
    assert_eq!(
        both.resets_at.as_ref().map(UtcTimestamp::as_str),
        Some("2030-01-07T07:06:40Z"),
        "the latest reset among blocking types"
    );
    let one_left = report(json!({
        "status": "allowed", "rateLimitType": "five_hour", "resetsAt": 1893474000
    }));
    assert!(one_left.limited, "the weekly rejection still blocks");
    assert_eq!(
        one_left.resets_at.as_ref().map(UtcTimestamp::as_str),
        Some("2030-01-07T07:06:40Z")
    );
    let unknown_reset = report(json!({ "status": "rejected", "rateLimitType": "opus" }));
    assert!(unknown_reset.limited);
    assert_eq!(
        unknown_reset.resets_at, None,
        "a blocking type without a reset leaves the reset unknown"
    );
}

#[test]
fn limit_types_beyond_the_tracked_bound_are_ignored() {
    let normalizer = normalizer();
    let mut state = active_state();
    for index in 0..16 {
        let events = normalizer
            .normalize_message(
                &mut state,
                rate_limit_event(
                    json!({ "status": "allowed", "rateLimitType": format!("t{index}") }),
                ),
            )
            .unwrap();
        assert_eq!(statuses(&events).len(), 1);
    }
    let overflow = normalizer
        .normalize_message(
            &mut state,
            rate_limit_event(json!({ "status": "rejected", "rateLimitType": "extra" })),
        )
        .unwrap();
    assert!(statuses(&overflow).is_empty());
    let known = normalizer
        .normalize_message(
            &mut state,
            rate_limit_event(json!({ "status": "rejected", "rateLimitType": "t0" })),
        )
        .unwrap();
    let reported = statuses(&known);
    assert!(reported[0].limited);
    assert_eq!(types(reported[0]).len(), 16);
}

/// A trimmed `get_usage` reply with the shape the CLI sends for a Claude plan.
fn usage_reply(five_hour: f64, extra_usage: bool) -> Value {
    json!({
        "session": { "total_cost_usd": 0 },
        "subscription_type": "max",
        "rate_limits_available": true,
        "rate_limits": {
            "five_hour": { "utilization": five_hour, "resets_at": "2026-11-17T01:59:59.656129+00:00" },
            "seven_day": { "utilization": 2, "resets_at": "2026-11-23T20:59:59.656150+00:00" },
            "seven_day_opus": null,
            "promo_credits": { "utilization": 100, "resets_at": "2026-11-18T00:00:00+00:00" },
            "extra_usage": { "is_enabled": extra_usage, "utilization": 0 }
        }
    })
}

#[test]
fn usage_reply_reads_plan_windows_in_the_rate_limit_event_shape() {
    let status = super::super::normalizer::usage_reply_status(&usage_reply(12.0, false))
        .expect("plan limits");
    assert!(!status.limited);
    assert_eq!(status.resets_at, None);
    assert_eq!(
        limit_statuses(&status),
        vec![("five_hour", "allowed"), ("seven_day", "allowed")]
    );
    assert_eq!(limit(&status, "five_hour", "utilization"), &json!(0.12));
    assert_eq!(
        limit(&status, "five_hour", "resetsAt"),
        &json!(1_794_880_799)
    );
    assert_eq!(
        limit(&status, "seven_day", "resetsAt"),
        &json!(1_795_467_599)
    );
}

#[test]
fn usage_reply_marks_a_full_window_limited_unless_extra_usage_runs_requests() {
    let usage_reply_status = super::super::normalizer::usage_reply_status;
    let full = usage_reply_status(&usage_reply(100.0, false)).expect("plan limits");
    assert!(full.limited);
    assert_eq!(
        full.resets_at.as_ref().map(UtcTimestamp::as_str),
        Some("2026-11-17T01:59:59Z")
    );
    assert_eq!(limit(&full, "five_hour", "status"), &json!("rejected"));

    let covered = usage_reply_status(&usage_reply(100.0, true)).expect("plan limits");
    assert!(!covered.limited);

    let over = usage_reply_status(&usage_reply(250.0, false)).expect("plan limits");
    assert_eq!(limit(&over, "five_hour", "utilization"), &json!(1.0));
}

#[test]
fn usage_reply_without_plan_limits_reads_as_no_observation() {
    let usage_reply_status = super::super::normalizer::usage_reply_status;
    assert!(
        usage_reply_status(&json!({ "rate_limits_available": false, "rate_limits": null }))
            .is_none()
    );
    assert!(usage_reply_status(&json!({})).is_none());
    assert!(
        usage_reply_status(&json!({
            "rate_limits_available": true,
            "rate_limits": { "five_hour": null, "promo_credits": { "utilization": 5 } }
        }))
        .is_none()
    );
}
