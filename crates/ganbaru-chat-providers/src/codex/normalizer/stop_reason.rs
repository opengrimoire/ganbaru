//! Classification of Codex turn errors from `codexErrorInfo`.
//!
//! `codexErrorInfo` is an open enum: a camelCase string, or an object with one camelCase key
//! for variants that carry an HTTP status. Decoding never fails; a value Ganbaru does not act on
//! becomes `Other` with its code.

use super::value::bounded_text;
use crate::models::UtcTimestamp;
use serde_json::{Map, Value};

/// Byte limit of an unrecognized error code kept for the stop reason text.
const MAX_ERROR_CODE_BYTES: usize = 64;
/// HTTP status that marks a retry exhaustion as a usage or rate limit.
const HTTP_TOO_MANY_REQUESTS: u64 = 429;

/// Why a Codex turn stopped with an error, as `codexErrorInfo` reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexStopReason {
    /// The account allowance or rate limit is exhausted; `resets_at` is the latest reset of the
    /// blocked windows Codex reported, when known.
    UsageLimit {
        resets_at: Option<UtcTimestamp>,
    },
    /// The model is overloaded; the same turn may succeed after a short wait.
    Capacity,
    ModelNotFound,
    ContextWindowExceeded,
    /// Any other code, bounded.
    Other(String),
}

/// The error code `codexErrorInfo` names, without its payload.
fn error_code(error: &Map<String, Value>) -> Option<&str> {
    match error.get("codexErrorInfo")? {
        Value::String(code) => Some(code),
        Value::Object(variant) if variant.len() == 1 => variant.keys().next().map(String::as_str),
        _ => None,
    }
}

/// Whether a retry exhaustion ended on HTTP 429, which Codex reports as a usage or rate limit.
fn exhausted_on_rate_limit(error: &Map<String, Value>) -> bool {
    error
        .get("codexErrorInfo")
        .and_then(|info| info.get("responseTooManyFailedAttempts"))
        .and_then(|attempts| attempts.get("httpStatusCode"))
        .and_then(Value::as_u64)
        == Some(HTTP_TOO_MANY_REQUESTS)
}

impl CodexStopReason {
    /// Classifies a turn error object, or returns None when it names no code.
    ///
    /// `resets_at` is the reset the merged rate limit state projects; only `UsageLimit` keeps it.
    pub fn from_turn_error(
        error: &Map<String, Value>,
        resets_at: Option<UtcTimestamp>,
    ) -> Option<Self> {
        if exhausted_on_rate_limit(error) {
            return Some(Self::UsageLimit { resets_at });
        }
        Some(match error_code(error)? {
            "usageLimitExceeded" | "rateLimitExceeded" => Self::UsageLimit { resets_at },
            "serverOverloaded" => Self::Capacity,
            "modelNotFound" => Self::ModelNotFound,
            "contextWindowExceeded" => Self::ContextWindowExceeded,
            other => Self::Other(bounded_text(other, MAX_ERROR_CODE_BYTES)),
        })
    }

    /// Whether the error names model capacity, without reading the rate limit state.
    pub fn is_capacity(error: &Map<String, Value>) -> bool {
        Self::from_turn_error(error, None) == Some(Self::Capacity)
    }

    /// The stop reason shown when Codex sent a code without a message.
    pub fn fallback_message(&self) -> String {
        match self {
            Self::UsageLimit { .. } => "Codex usage limit reached".to_string(),
            Self::Capacity => "The selected model is at capacity".to_string(),
            Self::ModelNotFound => "The selected model is not available".to_string(),
            Self::ContextWindowExceeded => {
                "The conversation no longer fits the model context window".to_string()
            }
            Self::Other(code) => format!("Codex turn failed ({code})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn classify(info: Value) -> Option<CodexStopReason> {
        let error = json!({ "message": "failed", "codexErrorInfo": info });
        CodexStopReason::from_turn_error(error.as_object().unwrap(), None)
    }

    #[test]
    fn known_codes_map_to_their_variants() {
        assert_eq!(
            classify(json!("usageLimitExceeded")),
            Some(CodexStopReason::UsageLimit { resets_at: None })
        );
        assert_eq!(
            classify(json!("rateLimitExceeded")),
            Some(CodexStopReason::UsageLimit { resets_at: None })
        );
        assert_eq!(
            classify(json!("serverOverloaded")),
            Some(CodexStopReason::Capacity)
        );
        assert_eq!(
            classify(json!("modelNotFound")),
            Some(CodexStopReason::ModelNotFound)
        );
        assert_eq!(
            classify(json!("contextWindowExceeded")),
            Some(CodexStopReason::ContextWindowExceeded)
        );
    }

    #[test]
    fn object_variants_and_unknown_codes_never_fail() {
        assert_eq!(
            classify(json!({ "responseStreamDisconnected": { "httpStatusCode": 502 } })),
            Some(CodexStopReason::Other(
                "responseStreamDisconnected".to_string()
            ))
        );
        assert_eq!(
            classify(json!("aCodeFromTheFuture")),
            Some(CodexStopReason::Other("aCodeFromTheFuture".to_string()))
        );
        assert_eq!(classify(json!({ "a": 1, "b": 2 })), None);
        assert_eq!(classify(json!(7)), None);
        assert_eq!(classify(Value::Null), None);
        let long = "x".repeat(MAX_ERROR_CODE_BYTES * 2);
        let Some(CodexStopReason::Other(code)) = classify(json!(long)) else {
            panic!("a long code is kept as Other");
        };
        assert_eq!(code.len(), MAX_ERROR_CODE_BYTES);
    }

    #[test]
    fn retry_exhaustion_on_too_many_requests_is_a_usage_limit() {
        assert_eq!(
            classify(json!({ "responseTooManyFailedAttempts": { "httpStatusCode": 429 } })),
            Some(CodexStopReason::UsageLimit { resets_at: None })
        );
        assert_eq!(
            classify(json!({ "responseTooManyFailedAttempts": { "httpStatusCode": 500 } })),
            Some(CodexStopReason::Other(
                "responseTooManyFailedAttempts".to_string()
            ))
        );
    }
}
