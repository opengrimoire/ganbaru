use super::diff::file_change_metadata;
use super::*;

#[derive(Clone, Copy)]
pub(super) enum CanonicalNotificationKind {
    Configuration,
    Deprecation,
    Runtime,
}

pub(super) trait ProviderIdentifier: Sized {
    fn parse(value: String) -> Result<Self, String>;
}

impl ProviderIdentifier for ProviderTurnId {
    fn parse(value: String) -> Result<Self, String> {
        Self::new(value)
    }
}

impl ProviderIdentifier for ProviderItemId {
    fn parse(value: String) -> Result<Self, String> {
        Self::new(value)
    }
}

pub(super) fn provider_identifier<T: ProviderIdentifier>(value: &str) -> Option<T> {
    T::parse(value.to_string()).ok()
}

pub(super) fn required_object<'a>(
    value: &'a Value,
    label: &str,
) -> ChatResult<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| protocol_error(label))
}

pub(super) fn required_text<'a>(object: &'a Map<String, Value>, key: &str) -> ChatResult<&'a str> {
    text(object, key).ok_or_else(|| protocol_error(key))
}

pub(super) fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

pub(super) fn unsigned(object: &Map<String, Value>, key: &str) -> Option<u64> {
    object.get(key).and_then(Value::as_u64)
}

pub(super) fn status_text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.as_object().and_then(|value| text(value, "type")))
}

pub(super) fn route_turn(
    state: &CodexRouteState,
    object: Option<&Map<String, Value>>,
) -> Option<ChatTurnId> {
    let provider_turn = object.and_then(|value| text(value, "turnId"));
    match (provider_turn, state.active_provider_turn_id.as_deref()) {
        (Some(actual), Some(expected)) if actual != expected => None,
        _ => state.active_chat_turn_id.clone(),
    }
}

pub(super) fn provider_turn_from(object: &Map<String, Value>) -> Option<ProviderTurnId> {
    text(object, "turnId").and_then(provider_identifier)
}

pub(super) fn provider_item_from(object: &Map<String, Value>) -> Option<ProviderItemId> {
    text(object, "itemId").and_then(provider_identifier)
}

pub(super) fn item_kind(value: &str) -> CanonicalItemKind {
    match value {
        "userMessage" => CanonicalItemKind::UserMessage,
        "agentMessage" => CanonicalItemKind::AssistantMessage,
        "reasoning" => CanonicalItemKind::Reasoning,
        "plan" => CanonicalItemKind::Plan,
        "commandExecution" => CanonicalItemKind::CommandExecution,
        "fileChange" => CanonicalItemKind::FileChange,
        "mcpToolCall" => CanonicalItemKind::McpToolCall,
        "dynamicToolCall" => CanonicalItemKind::DynamicToolCall,
        "collabAgentToolCall" | "subAgentActivity" => CanonicalItemKind::CollaborationTask,
        "webSearch" => CanonicalItemKind::WebSearch,
        "imageView" | "imageGeneration" => CanonicalItemKind::ImageView,
        "enteredReviewMode" | "exitedReviewMode" => CanonicalItemKind::ReviewTransition,
        "contextCompaction" => CanonicalItemKind::ContextCompaction,
        _ => CanonicalItemKind::Unknown,
    }
}

pub(super) fn item_status(item: &Map<String, Value>, completed: bool) -> ActivityStatus {
    if let Some(status) = item.get("status").and_then(status_text) {
        return activity_status(status);
    }
    if completed {
        ActivityStatus::Completed
    } else {
        ActivityStatus::Active
    }
}

pub(super) fn activity_status(value: &str) -> ActivityStatus {
    match value {
        "pending" | "notStarted" => ActivityStatus::Pending,
        "inProgress" | "running" | "active" => ActivityStatus::Active,
        "waiting" => ActivityStatus::Waiting,
        "completed" | "success" | "succeeded" => ActivityStatus::Completed,
        "interrupted" | "cancelled" | "canceled" | "declined" => ActivityStatus::Interrupted,
        "failed" | "error" => ActivityStatus::Failed,
        _ => ActivityStatus::Unknown,
    }
}

pub(super) fn item_title(item: &Map<String, Value>, kind: &str) -> Option<String> {
    let title = match kind {
        "commandExecution" => text(item, "command"),
        "mcpToolCall" => text(item, "tool"),
        "dynamicToolCall" => text(item, "tool"),
        "collabAgentToolCall" => text(item, "tool"),
        "webSearch" => text(item, "query"),
        "imageView" => Some("Viewed image"),
        "fileChange" => Some("File changes"),
        "reasoning" => Some("Reasoning"),
        "plan" => Some("Proposed plan"),
        "agentMessage" => Some("Assistant response"),
        "userMessage" => Some("User message"),
        "enteredReviewMode" => Some("Entered review"),
        "exitedReviewMode" => Some("Exited review"),
        _ => Some("Codex activity"),
    }?;
    Some(bounded_text(title, 512))
}

pub(super) fn item_detail(item: &Map<String, Value>, kind: &str) -> Option<String> {
    let value = match kind {
        "commandExecution" => text(item, "aggregatedOutput").map(str::to_string),
        "agentMessage" | "plan" => text(item, "text").map(str::to_string),
        "webSearch" => item.get("action").map(provider_reason),
        "collabAgentToolCall" => text(item, "prompt").map(str::to_string),
        "imageView" => text(item, "path").map(str::to_string),
        _ => None,
    }?;
    Some(bounded_text(&value, MAX_PROVIDER_TEXT_BYTES))
}

pub(super) fn item_safe_metadata(item: &Map<String, Value>, kind: &str) -> Option<VersionedJson> {
    let value = match kind {
        "commandExecution" => json!({
            "cwd": text(item, "cwd"),
            "exitCode": item.get("exitCode"),
            "durationMs": item.get("durationMs"),
        }),
        "fileChange" => json!({
            "changes": file_change_metadata(item),
        }),
        "agentMessage" => json!({
            "phase": text(item, "phase"),
        }),
        "mcpToolCall" => json!({
            "server": text(item, "server"),
            "tool": text(item, "tool"),
            "durationMs": item.get("durationMs"),
        }),
        "dynamicToolCall" => json!({
            "tool": text(item, "tool"),
            "namespace": text(item, "namespace"),
            "durationMs": item.get("durationMs"),
        }),
        _ => return None,
    };
    Some(VersionedJson {
        schema_version: 1,
        value,
    })
}

pub(super) fn provider_reason(value: &Value) -> String {
    match value {
        Value::String(value) => bounded_text(value, MAX_PROVIDER_TEXT_BYTES),
        Value::Object(value) => text(value, "message")
            .or_else(|| text(value, "type"))
            .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES))
            .unwrap_or_else(|| "Provider supplied structured detail".to_string()),
        _ => "Provider supplied structured detail".to_string(),
    }
}

pub(super) fn is_child_thread_lifecycle(method: &str) -> bool {
    matches!(
        method,
        "thread/started"
            | "thread/status/changed"
            | "thread/archived"
            | "thread/unarchived"
            | "thread/closed"
            | "thread/name/updated"
            | "thread/tokenUsage/updated"
            | "turn/started"
            | "turn/completed"
            | "turn/plan/updated"
    )
}

pub(super) fn bounded_shape(value: &Value) -> Option<VersionedJson> {
    let object = value.as_object()?;
    Some(VersionedJson {
        schema_version: 1,
        value: json!({ "keys": bounded_keys(object) }),
    })
}

pub(super) fn bounded_keys(object: &Map<String, Value>) -> Vec<String> {
    object
        .keys()
        .take(MAX_UNKNOWN_KEYS)
        .map(|key| bounded_text(key, 256))
        .collect()
}

pub(super) fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_string()
}

pub(super) fn now_utc() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .map_err(identifier_error)
}

pub(super) fn protocol_error(field: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        format!("Codex protocol field '{field}' is invalid"),
        true,
    )
}

pub(super) fn identifier_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Codex canonical event identity is invalid",
        false,
    )
}
