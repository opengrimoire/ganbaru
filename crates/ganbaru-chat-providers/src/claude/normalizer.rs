//! Claude SDK message normalization into Ganbaru's canonical event model.

mod blocks;
mod limits;
mod messages;
mod seen;
mod usage;

use blocks::{ClaudeStreamScope, ClaudeStreamState, usable_message_id};
use limits::ClaudeRateLimitMemory;
use seen::{BoundedIdSet, ClaudeTurnText, plan_id_set};
use usage::ClaudeUsageTracker;

use super::protocol::{ClaudeResumeCursor, protocol_error, resume_cursor};
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const MAX_TEXT_BYTES: usize = 64 * 1024;
/// Model name Claude gives to assistant messages it builds locally (CLI notices, API errors).
pub(super) const SYNTHETIC_MODEL: &str = "<synthetic>";

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
    pub(super) stream: ClaudeStreamState,
    pub(super) turn_text: ClaudeTurnText,
    emitted_plan_ids: BoundedIdSet,
    rate_limits: ClaudeRateLimitMemory,
    pub(super) usage: ClaudeUsageTracker,
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
            stream: ClaudeStreamState::default(),
            turn_text: ClaudeTurnText::default(),
            emitted_plan_ids: plan_id_set(),
            rate_limits: ClaudeRateLimitMemory::default(),
            usage: ClaudeUsageTracker::default(),
            tool_kinds: HashMap::new(),
        }
    }

    /// Claims the right to project the ExitPlanMode plan `plan_id`, which both the assistant
    /// message and the permission request carry; only the first claim succeeds.
    pub(super) fn claim_plan(&mut self, plan_id: &str) -> bool {
        self.emitted_plan_ids.insert(plan_id)
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
            "rate_limit_event" => self.rate_limit_event(state, object),
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
        let scope = ClaudeStreamScope::of(object);
        if !scope.is_root() {
            // Subagent frames belong to the sidechain transcript; the subagent's result reaches
            // the root conversation through its task lifecycle and tool result.
            return Ok(Vec::new());
        }
        match text(event, "type").unwrap_or("unknown") {
            "message_start" => self.message_start(state, &scope, event),
            "content_block_start" => self.content_block_start(state, &scope, event),
            "content_block_delta" => self.content_block_delta(state, &scope, event),
            "content_block_stop" => self.content_block_stop(state, &scope, event),
            "message_delta" => {
                state.usage.message_delta(event);
                Ok(Vec::new())
            }
            "message_stop" | "ping" => Ok(Vec::new()),
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
        let scope = ClaudeStreamScope::of(object);
        let model = object
            .get("message")
            .and_then(Value::as_object)
            .and_then(|message| text(message, "model"));
        // Subagent uuids exist only in the sidechain transcript and synthetic notices are not
        // model turns, so resuming at either would fail or fork the conversation.
        let advances_cursor = scope.is_root() && model != Some(SYNTHETIC_MODEL);
        if let Some(uuid) = text(object, "uuid").filter(|_| advances_cursor) {
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
        if !scope.is_root() {
            // A subagent's plans and checklists are its own; they must not replace the root's.
            return Ok(events);
        }
        events.extend(self.blocked_assistant(state, object)?);
        let message = object.get("message").and_then(Value::as_object);
        let message_id = message
            .and_then(|message| text(message, "id"))
            .filter(|id| usable_message_id(id))
            .map(str::to_string);
        if let Some(message) = message {
            state.usage.assistant(message, message_id.as_deref());
        }
        let content = message
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for block in &content {
            let Some(block) = block.as_object() else {
                continue;
            };
            let block_type = text(block, "type").unwrap_or("unknown");
            if block_type == "text" {
                if let Some(body) = text(block, "text").filter(|body| !body.is_empty()) {
                    events.extend(self.unstreamed_text(state, message_id.as_deref(), body)?);
                }
                continue;
            }
            let tool_use_id = text(block, "id");
            if block_type == "tool_use"
                && text(block, "name") == Some("ExitPlanMode")
                && tool_use_id.is_none_or(|id| state.claim_plan(id))
            {
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
        if !ClaudeStreamScope::of(object).is_root() {
            // Subagent tool calls never opened root items, so their results have nothing to close.
            return Ok(Vec::new());
        }
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
            event_id: ChatEventId::new(format!("{}:event:{sequence}", self.session_id.as_str()))
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

    /// One completed assistant item for a root text block that did not stream, such as a
    /// `<synthetic>` notice, or nothing when the message's text already streamed.
    ///
    /// Positions count text blocks per message, because the CLI sends each block in its own frame
    /// at array position zero.
    fn unstreamed_text(
        &self,
        state: &mut ClaudeRouteState,
        message_id: Option<&str>,
        body: &str,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        if message_id.is_some_and(|id| state.turn_text.was_streamed(id)) {
            return Ok(Vec::new());
        }
        let item_id = message_id
            .and_then(|id| {
                state
                    .turn_text
                    .next_unstreamed_block(id)
                    .map(|position| self.message_item_id(id, position))
            })
            .unwrap_or_else(|| self.fallback_item_id("assistant"));
        let lifecycle = |status| ClaudeItemEvent {
            source: "assistant/text",
            item_id: item_id.clone(),
            kind: CanonicalItemKind::AssistantMessage,
            status,
            title: None,
            safe_metadata: None,
        };
        Ok(vec![
            self.item_event(state, lifecycle(ActivityStatus::Active))?,
            self.event(
                state,
                "assistant/text",
                None,
                None,
                ProviderItemId::new(item_id.clone()).ok(),
                CanonicalEvent::ContentDelta(ContentDeltaEvent {
                    item_id: item_id.clone(),
                    stream_kind: ContentStreamKind::AssistantText,
                    content_index: 0,
                    delta: bounded_text(body, MAX_TEXT_BYTES),
                }),
            )?,
            self.item_event(state, lifecycle(ActivityStatus::Completed))?,
        ])
    }

    /// Item id of one content block of an API message, the same for every frame that carries it.
    pub(super) fn message_item_id(&self, message_id: &str, index: u32) -> String {
        format!("{}:{message_id}:{index}", self.session_id.as_str())
    }

    pub(super) fn fallback_item_id(&self, prefix: &str) -> String {
        format!(
            "{}:{prefix}:{}",
            self.session_id.as_str(),
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
