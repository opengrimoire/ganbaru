use super::*;
use crate::events::{RateLimitStatusEvent, ThreadUsageUpdatedEvent, UsageCountScope};

fn normalizer_and_state() -> (CodexEventNormalizer, CodexRouteState) {
    let normalizer = CodexEventNormalizer::new(
        identifier("codex-instance-1", ProviderInstanceId::new),
        identifier("chat-thread-1", ChatThreadId::new),
        identifier("session-1", ProviderSessionId::new),
    );
    let state = CodexRouteState::new(
        modes(SafetyMode::AskForApproval, InteractionMode::Build),
        None,
    );
    (normalizer, state)
}

fn usage_event(
    normalizer: &CodexEventNormalizer,
    state: &mut CodexRouteState,
    token_usage: Value,
) -> ThreadUsageUpdatedEvent {
    let events = normalizer
        .normalize_notification(
            state,
            "thread/tokenUsage/updated",
            json!({ "threadId": "provider-thread-1", "turnId": "turn-1", "tokenUsage": token_usage }),
        )
        .unwrap();
    let [event] = events.as_slice() else {
        panic!("expected one usage event, got {events:?}");
    };
    let CanonicalEvent::ThreadUsageUpdated(usage) = &event.event else {
        panic!("expected usage, got {:?}", event.event);
    };
    usage.clone()
}

fn rate_limit_events(
    normalizer: &CodexEventNormalizer,
    state: &mut CodexRouteState,
    rate_limits: Value,
) -> Vec<RateLimitStatusEvent> {
    normalizer
        .normalize_notification(
            state,
            "account/rateLimits/updated",
            json!({ "rateLimits": rate_limits }),
        )
        .unwrap()
        .into_iter()
        .map(|event| match event.event {
            CanonicalEvent::RateLimitStatus(status) => status,
            other => panic!("expected a rate limit status, got {other:?}"),
        })
        .collect()
}

fn breakdown(input: u64, cached: u64, cache_write: u64, output: u64) -> Value {
    json!({
        "inputTokens": input,
        "cachedInputTokens": cached,
        "cacheWriteInputTokens": cache_write,
        "outputTokens": output,
        "reasoningOutputTokens": 0,
        "totalTokens": input + output,
    })
}

#[test]
fn context_fill_comes_from_the_newest_request_and_counts_from_the_thread_total() {
    let (normalizer, mut state) = normalizer_and_state();
    let usage = usage_event(
        &normalizer,
        &mut state,
        json!({
            "last": breakdown(40_000, 30_000, 2_000, 1_500),
            "total": breakdown(250_000, 180_000, 9_000, 12_000),
            "modelContextWindow": 272_000,
        }),
    );
    assert_eq!(usage.context_tokens, Some(41_500));
    assert_eq!(usage.context_limit, Some(272_000));
    assert_eq!(
        usage.input_tokens,
        Some(61_000),
        "cache reads and writes leave the uncached input"
    );
    assert_eq!(usage.output_tokens, Some(12_000));
    assert_eq!(usage.cached_input_tokens, Some(180_000));
    assert_eq!(usage.cache_write_input_tokens, Some(9_000));
    assert_eq!(usage.cost, None);
    assert_eq!(usage.count_scope, Some(UsageCountScope::ThreadCumulative));
}

#[test]
fn context_fill_is_unknown_without_the_newest_request() {
    let (normalizer, mut state) = normalizer_and_state();
    let mut total = breakdown(250_000, 180_000, 0, 12_000);
    total
        .as_object_mut()
        .unwrap()
        .remove("cacheWriteInputTokens");
    let usage = usage_event(
        &normalizer,
        &mut state,
        json!({ "total": total, "modelContextWindow": null }),
    );
    assert_eq!(usage.context_tokens, None);
    assert_eq!(usage.context_limit, None);
    assert_eq!(usage.input_tokens, Some(70_000));
    assert_eq!(usage.cache_write_input_tokens, None);
}

#[test]
fn usage_input_never_goes_below_zero_when_cache_counts_exceed_it() {
    let (normalizer, mut state) = normalizer_and_state();
    let usage = usage_event(
        &normalizer,
        &mut state,
        json!({ "total": breakdown(100, 80, 40, 5), "modelContextWindow": null }),
    );
    assert_eq!(usage.input_tokens, Some(0));
    assert_eq!(usage.count_scope, Some(UsageCountScope::ThreadCumulative));
}

#[test]
fn sparse_rate_limit_updates_merge_per_limit_and_project_windows() {
    let (normalizer, mut state) = normalizer_and_state();
    let first = rate_limit_events(
        &normalizer,
        &mut state,
        json!({
            "limitId": "codex",
            "limitName": "Codex",
            "primary": { "usedPercent": 31, "windowDurationMins": 300, "resetsAt": 1_784_000_000 },
            "secondary": { "usedPercent": 12, "windowDurationMins": 10_080, "resetsAt": 1_784_500_000 },
            "rateLimitReachedType": null,
            "planType": "plus"
        }),
    );
    assert_eq!(first.len(), 1);
    assert!(!first[0].limited);
    assert_eq!(first[0].resets_at, None);

    let second = rate_limit_events(
        &normalizer,
        &mut state,
        json!({ "limitId": "codex", "limitName": null, "primary": { "usedPercent": 44 } }),
    );
    assert_eq!(second.len(), 1);
    let value = &second[0].provider_data.as_ref().unwrap().value;
    assert_eq!(
        value,
        &json!({
            "rateLimitsByLimitId": {
                "codex": {
                    "limitId": "codex",
                    "limitName": "Codex",
                    "primary": { "usedPercent": 44, "windowDurationMins": null, "resetsAt": null },
                    "secondary": { "usedPercent": 12, "windowDurationMins": 10_080, "resetsAt": 1_784_500_000 },
                    "rateLimitReachedType": null
                }
            }
        })
    );

    let repeated = rate_limit_events(
        &normalizer,
        &mut state,
        json!({ "limitId": "codex", "primary": { "usedPercent": 44 } }),
    );
    assert!(repeated.is_empty(), "an unchanged merge reports nothing");

    let model_limit = rate_limit_events(
        &normalizer,
        &mut state,
        json!({
            "limitId": "codex_mini",
            "primary": { "usedPercent": 100, "windowDurationMins": 300, "resetsAt": 1_784_100_000 },
            "secondary": { "usedPercent": 100, "windowDurationMins": 10_080, "resetsAt": 1_784_900_000 }
        }),
    );
    assert!(model_limit[0].limited);
    assert_eq!(
        model_limit[0].resets_at.as_ref().map(UtcTimestamp::as_str),
        Some("2026-07-24T13:33:20Z"),
        "the latest blocked window decides when requests can run again"
    );
    let buckets = &model_limit[0].provider_data.as_ref().unwrap().value["rateLimitsByLimitId"];
    assert_eq!(buckets["codex"]["primary"]["usedPercent"], 44);
    assert_eq!(buckets["codex_mini"]["limitName"], Value::Null);
}

#[test]
fn reached_type_sets_limited_until_an_explicit_null_clears_it() {
    let (normalizer, mut state) = normalizer_and_state();
    let reached = rate_limit_events(
        &normalizer,
        &mut state,
        json!({
            "primary": { "usedPercent": 87, "windowDurationMins": 300, "resetsAt": 1_784_000_000 },
            "rateLimitReachedType": "workspace_member_usage_limit_reached"
        }),
    );
    assert!(reached[0].limited);
    assert_eq!(reached[0].resets_at, None, "no window is exhausted");
    let buckets = &reached[0].provider_data.as_ref().unwrap().value["rateLimitsByLimitId"];
    assert_eq!(
        buckets["codex"]["rateLimitReachedType"], "workspace_member_usage_limit_reached",
        "an update without a limit id is the main allowance"
    );

    let kept = rate_limit_events(
        &normalizer,
        &mut state,
        json!({ "primary": { "usedPercent": 90, "windowDurationMins": 300 } }),
    );
    assert!(
        kept[0].limited,
        "an absent reached type keeps the previous one"
    );

    let cleared = rate_limit_events(
        &normalizer,
        &mut state,
        json!({ "rateLimitReachedType": null, "primary": null }),
    );
    assert!(!cleared[0].limited);
    let buckets = &cleared[0].provider_data.as_ref().unwrap().value["rateLimitsByLimitId"];
    assert_eq!(buckets["codex"]["primary"], Value::Null);
}

#[test]
fn blocked_window_without_reset_leaves_the_reset_unknown() {
    let (normalizer, mut state) = normalizer_and_state();
    let status = rate_limit_events(
        &normalizer,
        &mut state,
        json!({
            "primary": { "usedPercent": 140, "windowDurationMins": 300, "resetsAt": 1_784_000_000 },
            "secondary": { "usedPercent": 100, "windowDurationMins": -5, "resetsAt": "soon" }
        }),
    );
    assert!(status[0].limited);
    assert_eq!(status[0].resets_at, None);
    let codex = &status[0].provider_data.as_ref().unwrap().value["rateLimitsByLimitId"]["codex"];
    assert_eq!(codex["primary"]["usedPercent"], 100);
    assert_eq!(codex["secondary"]["windowDurationMins"], Value::Null);
}

#[test]
fn malformed_rate_limit_updates_are_ignored_and_limits_stay_bounded() {
    let (normalizer, mut state) = normalizer_and_state();
    assert!(rate_limit_events(&normalizer, &mut state, json!(null)).is_empty());
    assert!(
        rate_limit_events(
            &normalizer,
            &mut state,
            json!({ "primary": { "windowDurationMins": 300 } }),
        )
        .is_empty(),
        "a window without usage is unreadable"
    );
    for index in 0..40 {
        rate_limit_events(
            &normalizer,
            &mut state,
            json!({ "limitId": format!("limit-{index}"), "primary": { "usedPercent": 5 } }),
        );
    }
    let latest = rate_limit_events(
        &normalizer,
        &mut state,
        json!({ "limitId": "limit-0", "primary": { "usedPercent": 6 } }),
    );
    let buckets = latest[0].provider_data.as_ref().unwrap().value["rateLimitsByLimitId"]
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(buckets.len(), 16);
    assert!(!buckets.contains_key("limit-39"));
    assert!(
        !buckets.contains_key("codex"),
        "an update that changes nothing tracks no limit"
    );
}
