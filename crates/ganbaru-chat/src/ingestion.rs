use crate::events::{CanonicalEvent, CanonicalRuntimeEvent, NotificationEvent};
use crate::models::{
    ChatChangeNotification, ChatError, ChatErrorCode, ChatEventId, ChatResult, ChatTurnId,
    ContentStreamKind,
};
use crate::repository::events::{AppendCanonicalEventRequest, append_canonical_event};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;

const MAX_CANONICAL_EVENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_DELTA_BATCH_BYTES: usize = 64 * 1024;
const MAX_COMMAND_ARTIFACT_BYTES: usize = 2 * 1024 * 1024;
const MAX_OUTPUT_ARTIFACTS_PER_SESSION: usize = 4_096;
const OUTPUT_TRUNCATION_NOTICE: &str = "\n[Output truncated at the 2 MiB Chat artifact limit]\n";
/// Streamed text one message, reasoning, or plan item may persist; model output limits stay far below it.
const MAX_STREAMED_ITEM_TEXT_BYTES: usize = 8 * 1024 * 1024;
/// Streamed text, outside command and file change output, one turn may persist across its items.
const MAX_TURN_STREAMED_TEXT_BYTES: usize = 32 * 1024 * 1024;
/// Accounting charge for each delta that starts a new stored batch, so alternating tiny deltas
/// across items cannot store an unbounded number of event rows inside the byte budget.
const STREAMED_BATCH_OVERHEAD_BYTES: usize = 256;
const ITEM_TEXT_TRUNCATION_NOTICE: &str = "\n\n[Text truncated at the 8 MiB Chat message limit]\n";
const TURN_TEXT_TRUNCATION_NOTICE: &str = "\n\n[Text truncated at the 32 MiB Chat turn limit]\n";
pub const CHAT_CHANGE_EVENT: &str = "chat://change";

pub trait ChatChangeEmitter: Send + Sync {
    fn emit(&self, notification: &ChatChangeNotification) -> ChatResult<()>;
}

pub struct ChatEventIngestor {
    pool: SqlitePool,
    emitter: Arc<dyn ChatChangeEmitter>,
    pending_delta: Option<AppendCanonicalEventRequest>,
    output_bytes: HashMap<String, usize>,
    text_budgets: HashMap<String, TurnTextBudget>,
}

/// Streamed text retained for the latest turn of one thread.
#[derive(Default)]
struct TurnTextBudget {
    turn_id: Option<ChatTurnId>,
    retained: usize,
    items: HashMap<String, usize>,
    last_item: Option<String>,
    exhausted: bool,
}

enum TextAdmission {
    Keep,
    Truncated,
    Drop,
}

impl ChatEventIngestor {
    pub fn new(pool: SqlitePool, emitter: Arc<dyn ChatChangeEmitter>) -> Self {
        Self {
            pool,
            emitter,
            pending_delta: None,
            output_bytes: HashMap::new(),
            text_budgets: HashMap::new(),
        }
    }

    pub async fn ingest(&mut self, mut request: AppendCanonicalEventRequest) -> ChatResult<()> {
        validate_event(&request.runtime)?;
        if !self.bound_output_artifact(&mut request) {
            return Ok(());
        }
        match self.bound_streamed_text(&mut request) {
            TextAdmission::Keep => {}
            TextAdmission::Drop => return Ok(()),
            TextAdmission::Truncated => {
                let warning = text_limit_warning(&request)?;
                self.flush().await?;
                self.append_and_notify(request).await?;
                return self.append_and_notify(warning).await;
            }
        }
        if matches!(
            request.runtime.event,
            CanonicalEvent::ContentDelta(_) | CanonicalEvent::ProposedPlanDelta(_)
        ) {
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

    /// Caps streamed message, reasoning, and plan text, including proposed plan deltas, per item
    /// and per turn.
    ///
    /// The delta that crosses a cap keeps its fitting prefix and a visible notice; later text for
    /// that item, or for the whole turn, is not stored. The turn itself keeps running.
    fn bound_streamed_text(&mut self, request: &mut AppendCanonicalEventRequest) -> TextAdmission {
        let (item_key, text) = match &mut request.runtime.event {
            CanonicalEvent::ContentDelta(delta)
                if !matches!(
                    delta.stream_kind,
                    ContentStreamKind::CommandOutput | ContentStreamKind::FileChangeOutput
                ) =>
            {
                (
                    format!("{}\0{:?}", delta.item_id, delta.stream_kind),
                    &mut delta.delta,
                )
            }
            CanonicalEvent::ProposedPlanDelta(delta) => (
                format!("{}\0{:?}", delta.plan_id, ContentStreamKind::PlanText),
                &mut delta.delta,
            ),
            _ => return TextAdmission::Keep,
        };
        let budget = self
            .text_budgets
            .entry(request.runtime.thread_id.as_str().to_string())
            .or_default();
        if budget.turn_id != request.runtime.turn_id {
            *budget = TurnTextBudget {
                turn_id: request.runtime.turn_id.clone(),
                ..TurnTextBudget::default()
            };
        }
        if budget.exhausted {
            return TextAdmission::Drop;
        }
        let item_retained = budget.items.get(&item_key).copied().unwrap_or(0);
        if item_retained >= MAX_STREAMED_ITEM_TEXT_BYTES {
            return TextAdmission::Drop;
        }
        let overhead = if budget.last_item.as_deref() == Some(item_key.as_str()) {
            0
        } else {
            STREAMED_BATCH_OVERHEAD_BYTES
        };
        budget.last_item = Some(item_key.clone());
        let turn_remaining =
            MAX_TURN_STREAMED_TEXT_BYTES.saturating_sub(budget.retained + overhead);
        let item_remaining = MAX_STREAMED_ITEM_TEXT_BYTES - item_retained;
        let length = text.len();
        if length <= turn_remaining && length <= item_remaining {
            budget.retained += overhead + length;
            budget.items.insert(item_key, item_retained + length);
            return TextAdmission::Keep;
        }
        let (kept, notice) = if turn_remaining < item_remaining {
            budget.exhausted = true;
            (turn_remaining, TURN_TEXT_TRUNCATION_NOTICE)
        } else {
            (item_remaining, ITEM_TEXT_TRUNCATION_NOTICE)
        };
        *text = format!("{}{notice}", truncate_utf8(text, kept));
        budget.retained = budget.retained.saturating_add(overhead + kept);
        budget.items.insert(item_key, MAX_STREAMED_ITEM_TEXT_BYTES);
        TextAdmission::Truncated
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

/// Builds the warning recorded after the delta that reached a streamed text cap.
fn text_limit_warning(
    request: &AppendCanonicalEventRequest,
) -> ChatResult<AppendCanonicalEventRequest> {
    let digest = Sha256::digest(request.runtime.event_id.as_str().as_bytes());
    let event_id = ChatEventId::new(format!("text-limit-{digest:x}")).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Internal,
            "create streamed text limit warning",
            false,
        )
    })?;
    let mut warning = request.clone();
    warning.runtime.event_id = event_id;
    warning.runtime.provider_item_id = None;
    warning.runtime.provider_request_id = None;
    warning.runtime.provider_task_id = None;
    warning.runtime.provider_reference = None;
    warning.runtime.redacted_diagnostic = None;
    warning.diagnostic_expires_at = None;
    warning.runtime.event = CanonicalEvent::RuntimeWarning(NotificationEvent {
        code: "streamed_text_limit".to_string(),
        title: "Provider text truncated".to_string(),
        detail: Some(
            "The provider streamed more text than Chat stores for one message or turn; the rest was not saved."
                .to_string(),
        ),
    });
    Ok(warning)
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
    let (pending_text, next_text) = match (&mut pending.runtime.event, &next.runtime.event) {
        (CanonicalEvent::ContentDelta(pending_delta), CanonicalEvent::ContentDelta(next_delta))
            if pending_delta.item_id == next_delta.item_id
                && pending_delta.stream_kind == next_delta.stream_kind
                && pending_delta.content_index == next_delta.content_index =>
        {
            (&mut pending_delta.delta, &next_delta.delta)
        }
        (
            CanonicalEvent::ProposedPlanDelta(pending_delta),
            CanonicalEvent::ProposedPlanDelta(next_delta),
        ) if pending_delta.plan_id == next_delta.plan_id
            && pending_delta.content_index == next_delta.content_index =>
        {
            (&mut pending_delta.delta, &next_delta.delta)
        }
        _ => return false,
    };
    if pending_text.len() + next_text.len() > MAX_DELTA_BATCH_BYTES {
        return false;
    }
    pending_text.push_str(next_text);
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
