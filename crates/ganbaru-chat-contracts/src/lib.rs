//! Provider-neutral Chat commands, events, configuration, and wire models.

pub mod config;
pub mod events;
pub mod models;
pub mod protected_paths;
pub mod secret_names;

#[cfg(test)]
mod tests {
    use crate::events::ThreadUsageUpdatedEvent;
    use crate::models::{ChatError, ChatErrorCode};
    use serde_json::json;

    #[test]
    fn chat_errors_keep_the_stable_wire_shape() {
        let error = ChatError::new(
            ChatErrorCode::DriverUnavailable,
            "Provider unavailable",
            true,
        );

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "code": "driver_unavailable",
                "message": "Provider unavailable",
                "field": null,
                "recoverable": true,
                "details": null
            })
        );
    }

    #[test]
    fn thread_usage_round_trips_with_and_without_cache_writes() {
        let usage = ThreadUsageUpdatedEvent {
            input_tokens: Some(10),
            output_tokens: Some(4),
            cached_input_tokens: Some(2),
            cache_write_input_tokens: Some(7),
            context_tokens: Some(23),
            context_limit: Some(200_000),
            cost: None,
        };
        let value = serde_json::to_value(&usage).unwrap();
        assert_eq!(value["cacheWriteInputTokens"], json!(7));
        assert_eq!(
            serde_json::from_value::<ThreadUsageUpdatedEvent>(value).unwrap(),
            usage
        );

        let stored = json!({
            "inputTokens": 10, "outputTokens": 4, "cachedInputTokens": 2,
            "contextTokens": 23, "contextLimit": 200000, "cost": null
        });
        let read = serde_json::from_value::<ThreadUsageUpdatedEvent>(stored).unwrap();
        assert_eq!(read.cache_write_input_tokens, None);
        assert_eq!(read.context_tokens, Some(23));
    }

    #[test]
    fn provider_version_admission_round_trips_in_snake_case() {
        use crate::models::ProviderVersionAdmission;
        let cases = [
            (ProviderVersionAdmission::Supported, "supported"),
            (ProviderVersionAdmission::UntestedNewer, "untested_newer"),
            (ProviderVersionAdmission::BelowMinimum, "below_minimum"),
            (
                ProviderVersionAdmission::AtOrAboveCeiling,
                "at_or_above_ceiling",
            ),
            (ProviderVersionAdmission::Unreadable, "unreadable"),
        ];
        for (admission, wire) in cases {
            assert_eq!(serde_json::to_value(admission).unwrap(), json!(wire));
            assert_eq!(
                serde_json::from_value::<ProviderVersionAdmission>(json!(wire)).unwrap(),
                admission
            );
        }
        assert!(serde_json::from_value::<ProviderVersionAdmission>(json!("newer")).is_err());
    }

    #[test]
    fn provider_probe_without_an_admission_reads_as_none() {
        use crate::models::{ProviderProbeResult, ProviderVersionAdmission};
        let stored = json!({
            "instanceId": "claude",
            "state": "healthy",
            "version": "2.1.322",
            "negotiatedProtocolVersion": "stream-json",
            "accountLabel": null,
            "capabilities": { "entries": [] },
            "authoritySupport": crate::models::ProviderAuthoritySupport::default(),
            "checkedAt": "2026-11-11T03:18:50.240Z",
            "detail": null
        });
        let probe = serde_json::from_value::<ProviderProbeResult>(stored).unwrap();
        assert_eq!(probe.version_admission, None);

        let admitted = ProviderProbeResult {
            version_admission: Some(ProviderVersionAdmission::UntestedNewer),
            ..probe
        };
        let value = serde_json::to_value(&admitted).unwrap();
        assert_eq!(value["versionAdmission"], json!("untested_newer"));
        assert_eq!(
            serde_json::from_value::<ProviderProbeResult>(value).unwrap(),
            admitted
        );
    }

    #[test]
    fn chat_behavior_timeline_density_reads_known_values_and_refuses_others() {
        use crate::config::{
            CHAT_TIMELINE_DENSITY_VALUES, ChatBehaviorPreferences, ChatTimelineDensity,
        };

        let compact: ChatBehaviorPreferences =
            serde_json::from_value(json!({ "timelineDensity": "compact" })).unwrap();
        assert_eq!(compact.timeline_density, ChatTimelineDensity::Compact);
        let missing: ChatBehaviorPreferences = serde_json::from_value(json!({})).unwrap();
        assert_eq!(missing.timeline_density, ChatTimelineDensity::Comfortable);
        for value in CHAT_TIMELINE_DENSITY_VALUES {
            assert!(serde_json::from_value::<ChatTimelineDensity>(json!(value)).is_ok());
        }
        for refused in [json!("dense"), json!("Compact"), json!(1)] {
            let behavior = json!({ "timelineDensity": refused });
            assert!(serde_json::from_value::<ChatBehaviorPreferences>(behavior).is_err());
        }
    }
}
