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

fn data<'a>(status: &'a RateLimitStatusEvent, key: &str) -> &'a Value {
    &status.provider_data.as_ref().unwrap().value[key]
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
        .map(|status| {
            (
                data(status, "rateLimitType").as_str().unwrap(),
                data(status, "status").as_str().unwrap(),
                status.limited,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        [
            ("five_hour", "allowed_warning", false),
            ("five_hour", "rejected", true),
            ("seven_day", "allowed", false),
            ("five_hour", "allowed", false),
        ],
        "an unchanged frame and the blocked response repeating the rejection report nothing"
    );
    assert_eq!(
        reported[1].resets_at.as_ref().map(UtcTimestamp::as_str),
        Some("2030-01-01T00:00:00Z")
    );
    assert_eq!(data(reported[0], "utilization"), &json!(0.85));
    assert_eq!(data(reported[1], "overageStatus"), &json!("rejected"));
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
    assert_eq!(data(reported[0], "rateLimitType"), &json!("seven_day_opus"));
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
        assert_eq!(data(&status, "resetsAt"), &Value::Null);
    }
}

#[test]
fn oversized_limit_strings_are_capped() {
    let status = single_status(json!({
        "status": "s".repeat(500), "rateLimitType": "t".repeat(500)
    }));
    assert_eq!(data(&status, "status").as_str().unwrap().len(), 64);
    assert_eq!(data(&status, "rateLimitType").as_str().unwrap().len(), 64);
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
