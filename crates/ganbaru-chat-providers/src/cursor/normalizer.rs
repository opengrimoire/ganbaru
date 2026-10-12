//! ACP core and Cursor extension normalization into canonical Chat events.

mod tools;

use super::protocol::{
    AcpAvailableCommand, AcpConfigOption, MAX_PROTOCOL_TEXT_BYTES, bounded_text,
    is_valid_identifier, object, parse_available_commands_update, parse_config_options_update,
    protocol_error, safe_shape,
};
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug)]
struct ToolState {
    kind: CanonicalItemKind,
    title: Option<String>,
}

struct NormalizedItemInput {
    item_id: String,
    kind: CanonicalItemKind,
    status: ActivityStatus,
    title: Option<String>,
    detail_and_metadata: Option<(Option<String>, VersionedJson)>,
}

#[derive(Clone, Debug)]
pub struct CursorRouteState {
    pub provider_thread_id: String,
    pub active_turn_id: Option<ChatTurnId>,
    pub session_state: ProviderSessionState,
    pub modes: TurnModeSnapshot,
    pub model_id: Option<ModelId>,
    pub config_options: Vec<AcpConfigOption>,
    pub available_commands: Vec<AcpAvailableCommand>,
    pub workspace: PathBuf,
    assistant_item_id: Option<String>,
    reasoning_item_id: Option<String>,
    tools: HashMap<String, ToolState>,
}

impl CursorRouteState {
    pub fn new(
        provider_thread_id: String,
        modes: TurnModeSnapshot,
        model_id: Option<ModelId>,
        config_options: Vec<AcpConfigOption>,
        workspace: PathBuf,
    ) -> Self {
        Self {
            provider_thread_id,
            active_turn_id: None,
            session_state: ProviderSessionState::Ready,
            modes,
            model_id,
            config_options,
            available_commands: Vec::new(),
            workspace,
            assistant_item_id: None,
            reasoning_item_id: None,
            tools: HashMap::new(),
        }
    }
}

pub struct CursorEventNormalizer {
    provider_instance_id: ProviderInstanceId,
    thread_id: ChatThreadId,
    session_id: ProviderSessionId,
    pub(super) provider_family_id: &'static str,
    pub(super) provider_display_name: &'static str,
    next_event_id: AtomicU64,
    next_item_id: AtomicU64,
}

impl CursorEventNormalizer {
    pub fn new_for_provider(
        provider_instance_id: ProviderInstanceId,
        thread_id: ChatThreadId,
        session_id: ProviderSessionId,
        provider_family_id: &'static str,
        provider_display_name: &'static str,
    ) -> Self {
        Self {
            provider_instance_id,
            thread_id,
            session_id,
            provider_family_id,
            provider_display_name,
            next_event_id: AtomicU64::new(1),
            next_item_id: AtomicU64::new(1),
        }
    }

    pub fn normalize_session_update(
        &self,
        state: &mut CursorRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let params_object = object(&params)?;
        text(params_object, "sessionId")
            .filter(|value| *value == state.provider_thread_id)
            .ok_or_else(|| protocol_error("session/update session ID"))?;
        let update = params_object
            .get("update")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("session/update payload"))?;
        let update_type =
            text(update, "sessionUpdate").ok_or_else(|| protocol_error("session update type"))?;
        match update_type {
            "agent_message_chunk" => self.content_chunk(state, update, false),
            "agent_thought_chunk" => self.content_chunk(state, update, true),
            "tool_call" | "tool_call_update" => self.tool_update(state, update, update_type),
            "plan" => self.plan_update(state, update, "session/update"),
            "current_mode_update" => self.mode_update(state, update),
            "config_options_update" => self.config_update(state, update),
            "available_commands_update" => {
                state.available_commands = parse_available_commands_update(update)?;
                Ok(Vec::new())
            }
            "user_message_chunk" | "session_info_update" => Ok(Vec::new()),
            unknown => Ok(vec![self.event(
                state,
                "session/update",
                None,
                None,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: format!("acp/session_update/{unknown}"),
                    summary: format!(
                        "{} emitted an unsupported ACP session update",
                        self.provider_display_name
                    ),
                    safe_payload: Some(safe_shape(&params)),
                }),
            )?]),
        }
    }

    pub fn normalize_create_plan(
        &self,
        state: &CursorRouteState,
        params: &Value,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        let object = object(params)?;
        let tool_call_id = text(object, "toolCallId")
            .filter(|value| is_valid_identifier(value, 512))
            .ok_or_else(|| protocol_error("Cursor plan tool call ID"))?;
        let markdown = text(object, "plan").ok_or_else(|| protocol_error("Cursor plan text"))?;
        let todos = object
            .get("todos")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("Cursor plan todos"))?;
        validate_todos(todos)?;
        self.event(
            state,
            "cursor/create_plan",
            ProviderItemId::new(tool_call_id.to_string()).ok(),
            None,
            CanonicalEvent::ProposedPlanCompleted(ProposedPlanCompletedEvent {
                plan_id: tool_call_id.to_string(),
                markdown: bounded_text(markdown, MAX_PROTOCOL_TEXT_BYTES),
            }),
        )
    }

    pub fn normalize_todos(
        &self,
        state: &CursorRouteState,
        params: &Value,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        let object = object(params)?;
        text(object, "toolCallId")
            .filter(|value| is_valid_identifier(value, 512))
            .ok_or_else(|| protocol_error("Cursor todo tool call ID"))?;
        if !object.get("merge").is_some_and(Value::is_boolean) {
            return Err(protocol_error("Cursor todo merge flag"));
        }
        let todos = object
            .get("todos")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("Cursor todo list"))?;
        validate_todos(todos)?;
        let steps = todos
            .iter()
            .enumerate()
            .filter_map(|(index, todo)| {
                let todo = todo.as_object().expect("todos were validated");
                let content = text(todo, "content").or_else(|| text(todo, "title"))?;
                (!content.trim().is_empty()).then(|| PlanStep {
                    id: text(todo, "id")
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("cursor-todo-{index}")),
                    text: bounded_text(content.trim(), MAX_PROTOCOL_TEXT_BYTES),
                    status: activity_status(text(todo, "status")),
                })
            })
            .collect::<Vec<_>>();
        self.event(
            state,
            "cursor/update_todos",
            None,
            None,
            CanonicalEvent::PlanUpdated(plan_event(steps)),
        )
    }

    pub fn finish_turn(
        &self,
        state: &mut CursorRouteState,
        stop_reason: &str,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let mut events = self.close_text_items(state)?;
        let turn_id = state.active_turn_id.take().ok_or_else(|| {
            ChatError::invalid_transition(format!(
                "{} has no active turn",
                self.provider_display_name
            ))
        })?;
        state.session_state = ProviderSessionState::Ready;
        let terminal = match stop_reason {
            "cancelled" => CanonicalEvent::TurnAborted(TurnAbortedEvent {
                state: ChatTurnState::Interrupted,
                reason: format!("{} turn was cancelled", self.provider_display_name),
                recoverable: true,
            }),
            "end_turn" | "max_tokens" | "refusal" | "unknown" => {
                CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                    state: ChatTurnState::Completed,
                    stop_reason: Some(bounded_text(stop_reason, 256)),
                    usage: None,
                    changed_files: Vec::new(),
                })
            }
            other => CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                state: ChatTurnState::Completed,
                stop_reason: Some(bounded_text(other, 256)),
                usage: None,
                changed_files: Vec::new(),
            }),
        };
        events.push(self.event(state, "session/prompt", None, Some(turn_id), terminal)?);
        Ok(events)
    }

    pub fn interrupted(
        &self,
        state: &mut CursorRouteState,
        reason: &str,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        if state.active_turn_id.is_none() {
            return Ok(Vec::new());
        }
        let mut events = self.close_text_items(state)?;
        let turn_id = state.active_turn_id.take().expect("turn checked above");
        state.session_state = ProviderSessionState::Ready;
        events.push(self.event(
            state,
            "transport/interrupted",
            None,
            Some(turn_id),
            CanonicalEvent::TurnAborted(TurnAbortedEvent {
                state: ChatTurnState::Interrupted,
                reason: bounded_text(reason, MAX_PROTOCOL_TEXT_BYTES),
                recoverable: true,
            }),
        )?);
        Ok(events)
    }

    pub fn external_event(
        &self,
        state: &CursorRouteState,
        source: &str,
        request_id: Option<ProviderRequestId>,
        event: CanonicalEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event_with_request(state, source, None, None, request_id, event)
    }

    fn content_chunk(
        &self,
        state: &mut CursorRouteState,
        update: &Map<String, Value>,
        reasoning: bool,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let content = update
            .get("content")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("ACP content block"))?;
        let content_type = text(content, "type").unwrap_or("unknown");
        if content_type != "text" {
            return Ok(vec![self.event(
                state,
                "session/update/content",
                None,
                None,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: format!("acp/content/{content_type}"),
                    summary: format!(
                        "{} emitted an unsupported ACP content block",
                        self.provider_display_name
                    ),
                    safe_payload: Some(safe_shape(&Value::Object(content.clone()))),
                }),
            )?]);
        }
        let delta = text(content, "text").unwrap_or_default();
        let (item_id, started) = self.ensure_text_item(state, reasoning)?;
        let mut events = Vec::new();
        if let Some(started) = started {
            events.push(started);
        }
        if !delta.is_empty() {
            events.push(self.event(
                state,
                "session/update/content",
                ProviderItemId::new(item_id.clone()).ok(),
                None,
                CanonicalEvent::ContentDelta(ContentDeltaEvent {
                    item_id,
                    stream_kind: if reasoning {
                        ContentStreamKind::ReasoningSummary
                    } else {
                        ContentStreamKind::AssistantText
                    },
                    content_index: 0,
                    delta: bounded_text(delta, MAX_PROTOCOL_TEXT_BYTES),
                }),
            )?);
        }
        Ok(events)
    }

    fn ensure_text_item(
        &self,
        state: &mut CursorRouteState,
        reasoning: bool,
    ) -> ChatResult<(String, Option<CanonicalRuntimeEvent>)> {
        let current = if reasoning {
            &mut state.reasoning_item_id
        } else {
            &mut state.assistant_item_id
        };
        if let Some(item_id) = current.as_ref() {
            return Ok((item_id.clone(), None));
        }
        let item_id = self.fallback_item_id(if reasoning { "reasoning" } else { "assistant" });
        *current = Some(item_id.clone());
        let event = self.item_event(
            state,
            "session/update/content",
            NormalizedItemInput {
                item_id: item_id.clone(),
                kind: if reasoning {
                    CanonicalItemKind::Reasoning
                } else {
                    CanonicalItemKind::AssistantMessage
                },
                status: ActivityStatus::Active,
                title: None,
                detail_and_metadata: None,
            },
        )?;
        Ok((item_id, Some(event)))
    }

    fn close_text_items(
        &self,
        state: &mut CursorRouteState,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let mut closed = Vec::new();
        if let Some(item_id) = state.assistant_item_id.take() {
            closed.push(self.item_event(
                state,
                "session/update/content_end",
                NormalizedItemInput {
                    item_id,
                    kind: CanonicalItemKind::AssistantMessage,
                    status: ActivityStatus::Completed,
                    title: None,
                    detail_and_metadata: None,
                },
            )?);
        }
        if let Some(item_id) = state.reasoning_item_id.take() {
            closed.push(self.item_event(
                state,
                "session/update/content_end",
                NormalizedItemInput {
                    item_id,
                    kind: CanonicalItemKind::Reasoning,
                    status: ActivityStatus::Completed,
                    title: None,
                    detail_and_metadata: None,
                },
            )?);
        }
        Ok(closed)
    }

    fn plan_update(
        &self,
        state: &CursorRouteState,
        update: &Map<String, Value>,
        source: &str,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let entries = update
            .get("entries")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("ACP plan entries"))?;
        if entries.len() > 256 {
            return Err(protocol_error("ACP plan entries"));
        }
        let mut steps = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let entry = entry
                .as_object()
                .ok_or_else(|| protocol_error("ACP plan entry"))?;
            let content = text(entry, "content")
                .map(str::trim)
                .filter(|content| !content.is_empty())
                .ok_or_else(|| protocol_error("ACP plan entry content"))?;
            if entry
                .get("id")
                .is_some_and(|id| id.as_str().is_none_or(|id| !is_valid_identifier(id, 256)))
            {
                return Err(protocol_error("ACP plan entry ID"));
            }
            steps.push(PlanStep {
                id: text(entry, "id")
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("cursor-plan-{index}")),
                text: bounded_text(content, MAX_PROTOCOL_TEXT_BYTES),
                status: activity_status(text(entry, "status")),
            });
        }
        Ok(vec![self.event(
            state,
            source,
            None,
            None,
            CanonicalEvent::PlanUpdated(plan_event(steps)),
        )?])
    }

    fn mode_update(
        &self,
        state: &mut CursorRouteState,
        update: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let mode = text(update, "currentModeId")
            .or_else(|| text(update, "modeId"))
            .filter(|value| is_valid_identifier(value, 256))
            .ok_or_else(|| protocol_error("current mode update"))?;
        state.modes.interaction_mode = if ["plan", "architect"]
            .iter()
            .any(|candidate| mode.eq_ignore_ascii_case(candidate))
        {
            InteractionMode::Plan
        } else {
            InteractionMode::Build
        };
        Ok(vec![self.event(
            state,
            "session/update/mode",
            None,
            None,
            CanonicalEvent::SessionConfigured(SessionConfiguredEvent {
                session_id: self.session_id.clone(),
                effective_modes: state.modes,
                effective_model_id: state.model_id.clone(),
                effective_model_options: Vec::new(),
            }),
        )?])
    }

    fn config_update(
        &self,
        state: &mut CursorRouteState,
        update: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let options = update
            .get("configOptions")
            .cloned()
            .ok_or_else(|| protocol_error("config options update"))?;
        state.config_options = parse_config_options_update(options)?;
        Ok(Vec::new())
    }

    fn item_event(
        &self,
        state: &CursorRouteState,
        source: &str,
        input: NormalizedItemInput,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        let (detail, safe_metadata) = input
            .detail_and_metadata
            .map(|(detail, metadata)| (detail, Some(metadata)))
            .unwrap_or((None, None));
        let item = ItemLifecycleEvent {
            item_id: input.item_id.clone(),
            kind: input.kind,
            status: input.status,
            title: input.title,
            detail,
            safe_metadata,
        };
        let event = match input.status {
            ActivityStatus::Pending | ActivityStatus::Active => CanonicalEvent::ItemStarted(item),
            ActivityStatus::Completed | ActivityStatus::Interrupted | ActivityStatus::Failed => {
                CanonicalEvent::ItemCompleted(item)
            }
            _ => CanonicalEvent::ItemUpdated(item),
        };
        self.event(
            state,
            source,
            ProviderItemId::new(input.item_id).ok(),
            None,
            event,
        )
    }

    fn event(
        &self,
        state: &CursorRouteState,
        source: &str,
        provider_item_id: Option<ProviderItemId>,
        turn_id: Option<ChatTurnId>,
        event: CanonicalEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event_with_request(state, source, provider_item_id, turn_id, None, event)
    }

    fn event_with_request(
        &self,
        state: &CursorRouteState,
        source: &str,
        provider_item_id: Option<ProviderItemId>,
        turn_id: Option<ChatTurnId>,
        provider_request_id: Option<ProviderRequestId>,
        event: CanonicalEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        let sequence = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        Ok(CanonicalRuntimeEvent {
            schema_version: CANONICAL_EVENT_SCHEMA_VERSION,
            event_id: ChatEventId::new(format!("{}:event:{sequence}", self.session_id.as_str()))
                .map_err(|_| protocol_error("event ID"))?,
            provider_family_id: ProviderFamilyId::new(self.provider_family_id)
                .map_err(|_| protocol_error("provider family"))?,
            provider_instance_id: self.provider_instance_id.clone(),
            thread_id: self.thread_id.clone(),
            created_at: now_utc()?,
            turn_id: turn_id.or_else(|| state.active_turn_id.clone()),
            provider_turn_id: state.active_turn_id.as_ref().and_then(|turn| {
                ProviderTurnId::new(format!("{}-{}", self.provider_family_id, turn.as_str())).ok()
            }),
            provider_item_id,
            provider_request_id,
            provider_task_id: None,
            provider_reference: Some(VersionedJson {
                schema_version: 1,
                value: json!({ "source": source, "sessionId": state.provider_thread_id }),
            }),
            event,
            redacted_diagnostic: None,
        })
    }

    fn fallback_item_id(&self, prefix: &str) -> String {
        format!(
            "{}:{prefix}:{}",
            self.session_id.as_str(),
            self.next_item_id.fetch_add(1, Ordering::Relaxed)
        )
    }
}

fn activity_status(status: Option<&str>) -> ActivityStatus {
    match status {
        Some("completed") => ActivityStatus::Completed,
        Some("in_progress" | "inProgress") => ActivityStatus::Active,
        Some("failed") => ActivityStatus::Failed,
        _ => ActivityStatus::Pending,
    }
}

fn validate_todos(todos: &[Value]) -> ChatResult<()> {
    if todos.len() > 256 {
        return Err(protocol_error("Cursor todo list"));
    }
    for todo in todos {
        let todo = todo
            .as_object()
            .ok_or_else(|| protocol_error("Cursor todo"))?;
        if todo
            .get("id")
            .is_some_and(|id| id.as_str().is_none_or(|id| !is_valid_identifier(id, 256)))
            || ["content", "title", "status"].into_iter().any(|key| {
                todo.get(key).is_some_and(|value| {
                    value
                        .as_str()
                        .is_none_or(|value| value.len() > MAX_PROTOCOL_TEXT_BYTES)
                })
            })
        {
            return Err(protocol_error("Cursor todo"));
        }
    }
    Ok(())
}

fn plan_event(steps: Vec<PlanStep>) -> PlanUpdatedEvent {
    let markdown = steps
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
        .join("\n");
    PlanUpdatedEvent { markdown, steps }
}

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn now_utc() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .map_err(|_| protocol_error("timestamp"))
}
