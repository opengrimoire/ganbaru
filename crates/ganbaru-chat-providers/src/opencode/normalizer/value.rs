use super::*;

pub(super) fn changed_files_from_paths(values: &[Value]) -> ChatResult<Vec<ChangedFileSummary>> {
    if values.len() > MAX_SAFE_COLLECTION_ITEMS {
        return Err(protocol_error("patch file count"));
    }
    values
        .iter()
        .map(|value| {
            let path = value
                .as_str()
                .ok_or_else(|| protocol_error("patch file path"))?;
            Ok(ChangedFileSummary {
                relative_path: bounded_text(path, 4096),
                previous_relative_path: None,
                additions: None,
                deletions: None,
                binary: false,
                status: "modified".to_string(),
            })
        })
        .collect()
}

pub(super) fn part_id(part: &Map<String, Value>) -> Option<ProviderItemId> {
    text(part, "id").and_then(|id| ProviderItemId::new(id.to_string()).ok())
}

pub(super) fn tool_kind(tool: &str) -> CanonicalItemKind {
    let tool = tool.to_ascii_lowercase();
    if tool.contains("bash") || tool.contains("shell") || tool.contains("command") {
        CanonicalItemKind::CommandExecution
    } else if tool.contains("edit") || tool.contains("write") || tool.contains("patch") {
        CanonicalItemKind::FileChange
    } else if tool.contains("web") || tool.contains("search") {
        CanonicalItemKind::WebSearch
    } else if tool.contains("mcp") {
        CanonicalItemKind::McpToolCall
    } else if tool.contains("task") || tool.contains("agent") {
        CanonicalItemKind::CollaborationTask
    } else {
        CanonicalItemKind::DynamicToolCall
    }
}

pub(super) fn request_kind(permission: &str) -> CanonicalRequestKind {
    match permission {
        "bash" => CanonicalRequestKind::CommandExecution,
        "edit" => CanonicalRequestKind::FileChange,
        "read" => CanonicalRequestKind::FileRead,
        "external_directory" => CanonicalRequestKind::FileRead,
        _ => CanonicalRequestKind::DynamicTool,
    }
}

pub(super) fn activity_status(status: Option<&str>) -> ActivityStatus {
    match status {
        Some("pending") => ActivityStatus::Pending,
        Some("running" | "in_progress" | "active" | "busy" | "connected") => ActivityStatus::Active,
        Some("completed" | "complete" | "done" | "idle") => ActivityStatus::Completed,
        Some("failed" | "error" | "disconnected") => ActivityStatus::Failed,
        Some("cancelled" | "canceled") => ActivityStatus::Interrupted,
        _ => ActivityStatus::Unknown,
    }
}

pub(super) fn identifier<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> ChatResult<&'a str> {
    let value = text(object, key).ok_or_else(|| protocol_error(label))?;
    validate_identifier(value, label)?;
    Ok(value)
}

pub(super) fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

pub(super) fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

pub(super) fn safe_shape(value: &Value) -> VersionedJson {
    VersionedJson {
        schema_version: 1,
        value: safe_value(value, 0),
    }
}

pub(super) fn safe_value(value: &Value, depth: usize) -> Value {
    if depth >= 6 {
        return Value::String("[nested]".to_string());
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(value) => Value::String(bounded_text(value, 4_096)),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(MAX_SAFE_COLLECTION_ITEMS)
                .map(|value| safe_value(value, depth + 1))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .take(MAX_SAFE_COLLECTION_ITEMS)
                .filter(|(key, _)| !sensitive_key(key))
                .map(|(key, value)| (key.clone(), safe_value(value, depth + 1)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
    }
}

pub(super) fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "authorization",
        "password",
        "token",
        "secret",
        "credential",
        "api_key",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

pub(super) fn now_utc() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .map_err(|_| protocol_error("timestamp"))
}
