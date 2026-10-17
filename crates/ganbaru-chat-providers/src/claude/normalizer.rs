//! Claude SDK message normalization into Ganbaru's canonical event model.

mod blocks;
mod messages;

use super::protocol::{ClaudeResumeCursor, protocol_error, resume_cursor};
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const MAX_TEXT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub(super) struct StreamBlock {
    pub item_id: String,
    pub item_kind: CanonicalItemKind,
    pub stream_kind: ContentStreamKind,
    pub content_index: u32,
    pub complete_on_stop: bool,
}

pub(super) struct ClaudeItemEvent {
    pub source: &'static str,
    pub item_id: String,
    pub kind: CanonicalItemKind,
    pub status: ActivityStatus,
    pub title: Option<String>,
    pub safe_metadata: Option<VersionedJson>,
}

#[derive(Clone, Debug)]
pub struct ClaudeRouteState {
    pub provider_thread_id: Option<String>,
    pub active_chat_turn_id: Option<ChatTurnId>,
    pub session_state: ProviderSessionState,
    pub modes: TurnModeSnapshot,
    pub model_id: Option<ModelId>,
    pub resume_cursor: ClaudeResumeCursor,
    pub(super) stream_blocks: HashMap<u32, StreamBlock>,
    pub(super) tool_kinds: HashMap<String, CanonicalItemKind>,
}

impl ClaudeRouteState {
    pub fn new(
        modes: TurnModeSnapshot,
        model_id: Option<ModelId>,
        resume_cursor: ClaudeResumeCursor,
    ) -> Self {
        Self {
            provider_thread_id: None,
            active_chat_turn_id: None,
            session_state: ProviderSessionState::Starting,
            modes,
            model_id,
            resume_cursor,
            stream_blocks: HashMap::new(),
            tool_kinds: HashMap::new(),
        }
    }
}

pub struct ClaudeEventNormalizer {
    provider_instance_id: ProviderInstanceId,
    thread_id: ChatThreadId,
    session_id: ProviderSessionId,
    next_event_id: AtomicU64,
    next_item_id: AtomicU64,
}

impl ClaudeEventNormalizer {
    pub fn new(
        provider_instance_id: ProviderInstanceId,
        thread_id: ChatThreadId,
        session_id: ProviderSessionId,
    ) -> Self {
        Self {
            provider_instance_id,
            thread_id,
            session_id,
            next_event_id: AtomicU64::new(1),
            next_item_id: AtomicU64::new(1),
        }
    }

    pub fn session_id(&self) -> ProviderSessionId {
        self.session_id.clone()
    }

    pub fn normalize_message(
        &self,
        state: &mut ClaudeRouteState,
        value: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = value
            .as_object()
            .ok_or_else(|| protocol_error("message envelope"))?;
        let message_type = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error("message type"))?;
        match message_type {
            "system" => self.system_message(state, object, &value),
            "stream_event" => self.stream_event(state, object, &value),
            "assistant" => self.assistant_message(state, object, &value),
            "user" => self.user_message(state, object, &value),
            "result" => self.result_message(state, object, &value),
            "control_request" => Ok(Vec::new()),
            "keep_alive" | "control_cancel_request" => Ok(Vec::new()),
            _ => Ok(vec![self.event(
                state,
                message_type,
                None,
                None,
                None,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: format!("claude/{message_type}"),
                    summary: "Claude emitted an unsupported message".to_string(),
                    safe_payload: bounded_shape(&value),
                }),
            )?]),
        }
    }

    pub fn external_event(
        &self,
        state: &ClaudeRouteState,
        source: &str,
        provider_request_id: Option<ProviderRequestId>,
        event: CanonicalEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event(state, source, None, provider_request_id, None, event)
    }

    pub fn interrupted_event(
        &self,
        state: &mut ClaudeRouteState,
        reason: &str,
    ) -> ChatResult<Option<CanonicalRuntimeEvent>> {
        let Some(turn_id) = state.active_chat_turn_id.take() else {
            return Ok(None);
        };
        state.session_state = ProviderSessionState::Ready;
        self.event(
            state,
            "stream/closed",
            Some(turn_id),
            None,
            None,
            CanonicalEvent::TurnAborted(TurnAbortedEvent {
                state: ChatTurnState::Interrupted,
                reason: bounded_text(reason, MAX_TEXT_BYTES),
                recoverable: true,
            }),
        )
        .map(Some)
    }

    fn stream_event(
        &self,
        state: &mut ClaudeRouteState,
        object: &Map<String, Value>,
        raw: &Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let event = object
            .get("event")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("stream event"))?;
        match text(event, "type").unwrap_or("unknown") {
            "content_block_start" => self.content_block_start(state, event),
            "content_block_delta" => self.content_block_delta(state, event),
            "content_block_stop" => self.content_block_stop(state, event),
            "message_delta" => Ok(Vec::new()),
            "message_start" | "message_stop" | "ping" => Ok(Vec::new()),
            event_type => Ok(vec![self.event(
                state,
                event_type,
                None,
                None,
                None,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: format!("claude/stream/{event_type}"),
                    summary: "Claude emitted an unsupported stream event".to_string(),
                    safe_payload: bounded_shape(raw),
                }),
            )?]),
        }
    }

    fn assistant_message(
        &self,
        state: &mut ClaudeRouteState,
        object: &Map<String, Value>,
        raw: &Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let mut events = Vec::new();
        if let Some(uuid) = text(object, "uuid") {
            state.resume_cursor.last_assistant_uuid = Some(uuid.to_string());
            events.push(
                self.event(
                    state,
                    "assistant/cursor",
                    None,
                    None,
                    None,
                    CanonicalEvent::ThreadMetadataUpdated(ThreadMetadataUpdatedEvent {
                        title: None,
                        provider_thread_id: state
                            .provider_thread_id
                            .as_deref()
                            .and_then(|value| ProviderThreadId::new(value.to_string()).ok()),
                        resume_cursor: Some(resume_cursor(&state.resume_cursor)),
                        metadata: None,
                    }),
                )?,
            );
        }
        let content = object
            .get("message")
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for block in &content {
            let Some(block) = block.as_object() else {
                continue;
            };
            let block_type = text(block, "type").unwrap_or("unknown");
            if block_type == "tool_use" && text(block, "name") == Some("ExitPlanMode") {
                let markdown = block
                    .get("input")
                    .and_then(Value::as_object)
                    .and_then(|input| {
                        ["plan", "planMarkdown", "markdown"]
                            .into_iter()
                            .find_map(|key| input.get(key).and_then(Value::as_str))
                    })
                    .unwrap_or_default();
                events.push(self.event(
                    state,
                    "assistant/ExitPlanMode",
                    None,
                    None,
                    text(block, "id").and_then(|id| ProviderItemId::new(id.to_string()).ok()),
                    CanonicalEvent::ProposedPlanCompleted(ProposedPlanCompletedEvent {
                        plan_id: text(block, "id").unwrap_or("claude-plan").to_string(),
                        markdown: bounded_text(markdown, MAX_TEXT_BYTES),
                    }),
                )?);
            }
            if block_type == "tool_use" && text(block, "name") == Some("TodoWrite") {
                if let Some(plan) = todo_plan(block.get("input")) {
                    events.push(self.event(
                        state,
                        "assistant/TodoWrite",
                        None,
                        None,
                        text(block, "id").and_then(|id| ProviderItemId::new(id.to_string()).ok()),
                        CanonicalEvent::PlanUpdated(plan),
                    )?);
                }
            }
        }
        if events.is_empty() && content.is_empty() {
            events.push(self.event(
                state,
                "assistant/empty",
                None,
                None,
                None,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: "claude/assistant".to_string(),
                    summary: "Claude emitted an empty assistant message".to_string(),
                    safe_payload: bounded_shape(raw),
                }),
            )?);
        }
        Ok(events)
    }

    fn user_message(
        &self,
        state: &mut ClaudeRouteState,
        object: &Map<String, Value>,
        _raw: &Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let content = object
            .get("message")
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut events = Vec::new();
        for block in content {
            let Some(block) = block.as_object() else {
                continue;
            };
            if text(block, "type") != Some("tool_result") {
                continue;
            }
            let item_id = text(block, "tool_use_id")
                .map(str::to_string)
                .unwrap_or_else(|| self.fallback_item_id("tool-result"));
            let item_kind = state
                .tool_kinds
                .remove(&item_id)
                .unwrap_or(CanonicalItemKind::DynamicToolCall);
            if let Some(output) = tool_result_text(block.get("content")) {
                events.push(self.event(
                    state,
                    "user/tool_result/output",
                    None,
                    None,
                    ProviderItemId::new(item_id.clone()).ok(),
                    CanonicalEvent::ContentDelta(ContentDeltaEvent {
                        item_id: item_id.clone(),
                        stream_kind: ContentStreamKind::CommandOutput,
                        content_index: 0,
                        delta: bounded_text(&output, MAX_TEXT_BYTES),
                    }),
                )?);
            }
            events.push(self.item_event(
                state,
                ClaudeItemEvent {
                    source: "user/tool_result",
                    item_id,
                    kind: item_kind,
                    status: if block.get("is_error").and_then(Value::as_bool) == Some(true) {
                        ActivityStatus::Failed
                    } else {
                        ActivityStatus::Completed
                    },
                    title: None,
                    safe_metadata: None,
                },
            )?);
        }
        Ok(events)
    }

    pub(super) fn item_event(
        &self,
        state: &ClaudeRouteState,
        input: ClaudeItemEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        let ClaudeItemEvent {
            source,
            item_id,
            kind,
            status,
            title,
            safe_metadata,
        } = input;
        let provider_item_id = ProviderItemId::new(item_id.clone()).ok();
        let item = ItemLifecycleEvent {
            item_id,
            kind,
            status,
            title,
            detail: None,
            safe_metadata,
        };
        let event = match status {
            ActivityStatus::Pending | ActivityStatus::Active => CanonicalEvent::ItemStarted(item),
            ActivityStatus::Completed | ActivityStatus::Interrupted | ActivityStatus::Failed => {
                CanonicalEvent::ItemCompleted(item)
            }
            _ => CanonicalEvent::ItemUpdated(item),
        };
        self.event(state, source, None, None, provider_item_id, event)
    }

    pub(super) fn event(
        &self,
        state: &ClaudeRouteState,
        source: &str,
        turn_id: Option<ChatTurnId>,
        provider_request_id: Option<ProviderRequestId>,
        provider_item_id: Option<ProviderItemId>,
        event: CanonicalEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        let sequence = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        Ok(CanonicalRuntimeEvent {
            schema_version: CANONICAL_EVENT_SCHEMA_VERSION,
            event_id: ChatEventId::new(format!("claude-event-{sequence}"))
                .map_err(|_| protocol_error("event ID"))?,
            provider_family_id: ProviderFamilyId::new("claude")
                .map_err(|_| protocol_error("provider family"))?,
            provider_instance_id: self.provider_instance_id.clone(),
            thread_id: self.thread_id.clone(),
            created_at: now_utc()?,
            turn_id: turn_id.or_else(|| state.active_chat_turn_id.clone()),
            provider_turn_id: state
                .active_chat_turn_id
                .as_ref()
                .and_then(|value| ProviderTurnId::new(format!("claude-{}", value.as_str())).ok()),
            provider_item_id,
            provider_request_id,
            provider_task_id: None,
            provider_reference: Some(VersionedJson {
                schema_version: 1,
                value: json!({ "source": source, "sessionId": self.session_id.as_str() }),
            }),
            event,
            redacted_diagnostic: None,
        })
    }

    pub(super) fn fallback_item_id(&self, prefix: &str) -> String {
        format!(
            "claude-{prefix}-{}",
            self.next_item_id.fetch_add(1, Ordering::Relaxed)
        )
    }
}

fn todo_plan(input: Option<&Value>) -> Option<PlanUpdatedEvent> {
    let todos = input?.get("todos")?.as_array()?;
    let mut steps = Vec::new();
    for (index, todo) in todos.iter().enumerate() {
        let object = todo.as_object()?;
        let step_text = text(object, "content")
            .or_else(|| text(object, "subject"))?
            .to_string();
        let status = match text(object, "status").unwrap_or("pending") {
            "in_progress" | "active" => ActivityStatus::Active,
            "completed" => ActivityStatus::Completed,
            "failed" => ActivityStatus::Failed,
            _ => ActivityStatus::Pending,
        };
        steps.push(PlanStep {
            id: text(object, "id")
                .map(str::to_string)
                .unwrap_or_else(|| format!("claude-todo-{index}")),
            text: bounded_text(&step_text, MAX_TEXT_BYTES),
            status,
        });
    }
    Some(PlanUpdatedEvent {
        markdown: steps
            .iter()
            .map(|step| {
                format!(
                    "- [{}] {}",
                    if step.status == ActivityStatus::Completed {
                        "x"
                    } else {
                        " "
                    },
                    step.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        steps,
    })
}

pub(super) fn normalize_usage(object: &Map<String, Value>) -> Option<ThreadUsageUpdatedEvent> {
    let usage = object.get("usage")?.as_object()?;
    let cost = object
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .map(|amount| ProviderAttributedCost {
            amount,
            currency: "USD".to_string(),
            provider_reported: true,
        });
    Some(ThreadUsageUpdatedEvent {
        input_tokens: unsigned(usage, "input_tokens"),
        output_tokens: unsigned(usage, "output_tokens"),
        cached_input_tokens: unsigned(usage, "cache_read_input_tokens"),
        context_tokens: unsigned(usage, "total_tokens"),
        context_limit: object
            .get("modelUsage")
            .and_then(Value::as_object)
            .and_then(|models| {
                models
                    .values()
                    .filter_map(|value| value.get("contextWindow").and_then(Value::as_u64))
                    .max()
            }),
        cost,
    })
}

fn tool_result_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => Some(
            blocks
                .iter()
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => None,
    }
}

pub(super) fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

pub(super) fn unsigned(object: &Map<String, Value>, key: &str) -> Option<u64> {
    object.get(key).and_then(Value::as_u64)
}

pub(super) fn bounded_shape(value: &Value) -> Option<VersionedJson> {
    let object = value.as_object()?;
    Some(VersionedJson {
        schema_version: 1,
        value: json!({ "keys": object.keys().take(32).collect::<Vec<_>>() }),
    })
}

pub(super) fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn now_utc() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .map_err(|_| protocol_error("timestamp"))
}
