use crate::events::{CanonicalEvent, CanonicalRuntimeEvent};
use crate::models::{
    ChatChangeNotification, ChatError, ChatErrorCode, ChatResult, ContentStreamKind,
};
use crate::repository::events::{AppendCanonicalEventRequest, append_canonical_event};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;

const MAX_CANONICAL_EVENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_DELTA_BATCH_BYTES: usize = 64 * 1024;
const MAX_COMMAND_ARTIFACT_BYTES: usize = 2 * 1024 * 1024;
const MAX_OUTPUT_ARTIFACTS_PER_SESSION: usize = 4_096;
const OUTPUT_TRUNCATION_NOTICE: &str = "\n[Output truncated at the 2 MiB Chat artifact limit]\n";
pub const CHAT_CHANGE_EVENT: &str = "chat://change";

pub trait ChatChangeEmitter: Send + Sync {
    fn emit(&self, notification: &ChatChangeNotification) -> ChatResult<()>;
}

pub struct ChatEventIngestor {
    pool: SqlitePool,
    emitter: Arc<dyn ChatChangeEmitter>,
    pending_delta: Option<AppendCanonicalEventRequest>,
    output_bytes: HashMap<String, usize>,
}

impl ChatEventIngestor {
    pub fn new(pool: SqlitePool, emitter: Arc<dyn ChatChangeEmitter>) -> Self {
        Self {
            pool,
            emitter,
            pending_delta: None,
            output_bytes: HashMap::new(),
        }
    }

    pub async fn ingest(&mut self, mut request: AppendCanonicalEventRequest) -> ChatResult<()> {
        validate_event(&request.runtime)?;
        if !self.bound_output_artifact(&mut request) {
            return Ok(());
        }
        if matches!(request.runtime.event, CanonicalEvent::ContentDelta(_)) {
            if let Some(pending) = self.pending_delta.as_mut() {
                if merge_adjacent_delta(pending, &request) {
                    return Ok(());
                }
            }
            self.flush().await?;
            self.pending_delta = Some(request);
            return Ok(());
        }
        self.flush().await?;
        self.append_and_notify(request).await
    }

    fn bound_output_artifact(&mut self, request: &mut AppendCanonicalEventRequest) -> bool {
        let CanonicalEvent::ContentDelta(delta) = &mut request.runtime.event else {
            return true;
        };
        if !matches!(
            delta.stream_kind,
            ContentStreamKind::CommandOutput | ContentStreamKind::FileChangeOutput
        ) {
            return true;
        }
        let key = format!(
            "{}\0{}\0{}",
            request.runtime.thread_id.as_str(),
            delta.item_id.as_str(),
            match delta.stream_kind {
                ContentStreamKind::CommandOutput => "command",
                _ => "file",
            }
        );
        let artifact_count = self.output_bytes.len();
        let retained = match self.output_bytes.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(_) if artifact_count >= MAX_OUTPUT_ARTIFACTS_PER_SESSION => return false,
            Entry::Vacant(entry) => entry.insert(0),
        };
        let remaining = MAX_COMMAND_ARTIFACT_BYTES.saturating_sub(*retained);
        if remaining == 0 {
            return false;
        }
        if delta.delta.len() > remaining {
            delta.delta = format!(
                "{}{}",
                truncate_utf8(&delta.delta, remaining),
                OUTPUT_TRUNCATION_NOTICE
            );
            *retained = MAX_COMMAND_ARTIFACT_BYTES;
        } else {
            *retained += delta.delta.len();
        }
        true
    }

    pub async fn flush(&mut self) -> ChatResult<()> {
        if let Some(request) = self.pending_delta.take() {
            match append_canonical_event(&self.pool, request.clone()).await {
                Ok(result) => self.emitter.emit(&result.notification)?,
                Err(error) => {
                    self.pending_delta = Some(request);
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    async fn append_and_notify(&self, request: AppendCanonicalEventRequest) -> ChatResult<()> {
        let result = append_canonical_event(&self.pool, request).await?;
        self.emitter.emit(&result.notification)
    }
}

fn merge_adjacent_delta(
    pending: &mut AppendCanonicalEventRequest,
    next: &AppendCanonicalEventRequest,
) -> bool {
    if pending.runtime.thread_id != next.runtime.thread_id
        || pending.runtime.turn_id != next.runtime.turn_id
        || pending.runtime.provider_instance_id != next.runtime.provider_instance_id
    {
        return false;
    }
    let (CanonicalEvent::ContentDelta(pending_delta), CanonicalEvent::ContentDelta(next_delta)) =
        (&mut pending.runtime.event, &next.runtime.event)
    else {
        return false;
    };
    if pending_delta.item_id != next_delta.item_id
        || pending_delta.stream_kind != next_delta.stream_kind
        || pending_delta.content_index != next_delta.content_index
        || pending_delta.delta.len() + next_delta.delta.len() > MAX_DELTA_BATCH_BYTES
    {
        return false;
    }
    pending_delta.delta.push_str(&next_delta.delta);
    pending.runtime.created_at = next.runtime.created_at.clone();
    pending.ingested_at = next.ingested_at.clone();
    true
}

fn validate_event(event: &CanonicalRuntimeEvent) -> ChatResult<()> {
    let encoded = serde_json::to_vec(event).map_err(serialization_error)?;
    if encoded.len() > MAX_CANONICAL_EVENT_BYTES {
        return Err(ChatError::validation(
            "event",
            "Canonical Chat event exceeds the ingestion limit",
        ));
    }
    if let Some(diagnostic) = &event.redacted_diagnostic {
        validate_redacted_value(&diagnostic.value)?;
    }
    Ok(())
}

fn validate_redacted_value(value: &Value) -> ChatResult<()> {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let normalized = key.to_ascii_lowercase();
                if [
                    "authorization",
                    "token",
                    "password",
                    "secret",
                    "api_key",
                    "apiKey",
                ]
                .iter()
                .any(|blocked| normalized.contains(&blocked.to_ascii_lowercase()))
                {
                    return Err(unredacted_diagnostic_error());
                }
                validate_redacted_value(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_redacted_value(value)?;
            }
        }
        Value::String(value) => {
            let normalized = value.to_ascii_lowercase();
            if normalized.contains("/home/")
                || normalized.contains("/root/")
                || normalized.contains("\\users\\")
                || normalized.contains("/users/")
                || normalized.contains("bearer ")
            {
                return Err(unredacted_diagnostic_error());
            }
        }
        _ => {}
    }
    Ok(())
}

fn unredacted_diagnostic_error() -> ChatError {
    ChatError::validation(
        "event.redactedDiagnostic",
        "Chat diagnostic contains prohibited sensitive fields",
    )
}
fn serialization_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        "Canonical Chat event could not be encoded",
        false,
    )
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> &str {
    let mut boundary = maximum_bytes.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &value[..boundary]
}

#[cfg(test)]
mod tests {
    use super::truncate_utf8;

    #[test]
    fn output_truncation_preserves_utf8_boundaries() {
        assert_eq!(truncate_utf8("aé日", 4), "aé");
        assert_eq!(truncate_utf8("aé日", 7), "aé日");
    }
}
