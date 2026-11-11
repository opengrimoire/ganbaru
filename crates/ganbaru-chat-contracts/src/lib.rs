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
}
