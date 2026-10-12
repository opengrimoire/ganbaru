//! OpenCode event normalization into provider-neutral Chat events.

use super::protocol::{MAX_EVENT_DATA_BYTES, protocol_error, validate_identifier};
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

mod parts;
mod projections;
mod requests;
mod session;
mod value;

use value::*;

const MAX_DEDUPLICATION_EVENTS: usize = 4_096;
const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_SAFE_COLLECTION_ITEMS: usize = 256;
const MAX_QUESTIONS: usize = 16;
const MAX_OPTIONS: usize = 64;

#[derive(Clone, Debug)]
pub struct OpenCodeRouteState {
    pub provider_thread_id: String,
    pub active_turn_id: Option<ChatTurnId>,
    pub session_state: ProviderSessionState,
    pub modes: TurnModeSnapshot,
    message_roles: HashMap<String, String>,
    rollback_message_id: Option<String>,
    part_text: HashMap<String, String>,
    completed_parts: HashSet<String>,
    pending_permissions: HashSet<String>,
    pending_questions: HashMap<String, Vec<OpenCodeQuestionMapping>>,
    deduplication_order: VecDeque<String>,
    deduplication_set: HashSet<String>,
}

#[derive(Clone, Debug)]
struct OpenCodeQuestionMapping {
    id: String,
    option_labels: Vec<String>,
    free_form_allowed: bool,
}

impl OpenCodeRouteState {
    pub fn new(provider_thread_id: String, modes: TurnModeSnapshot) -> Self {
        Self {
            provider_thread_id,
            active_turn_id: None,
            session_state: ProviderSessionState::Ready,
            modes,
            message_roles: HashMap::new(),
            rollback_message_id: None,
            part_text: HashMap::new(),
            completed_parts: HashSet::new(),
            pending_permissions: HashSet::new(),
            pending_questions: HashMap::new(),
            deduplication_order: VecDeque::new(),
            deduplication_set: HashSet::new(),
        }
    }

    pub fn has_permission(&self, request_id: &ProviderRequestId) -> bool {
        self.pending_permissions.contains(request_id.as_str())
    }

    pub fn question_answers(
        &self,
        request_id: &ProviderRequestId,
        answers: &[UserInputAnswer],
    ) -> ChatResult<Vec<Vec<String>>> {
        let questions = self
            .pending_questions
            .get(request_id.as_str())
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Conflict,
                    "OpenCode question is no longer pending in this session",
                    false,
                )
            })?;
        if answers.len() != questions.len() {
            return Err(ChatError::validation(
                "answers",
                "OpenCode question answers are incomplete",
            ));
        }
        questions
            .iter()
            .map(|question| {
                let answer = answers
                    .iter()
                    .find(|answer| answer.question_id == question.id)
                    .ok_or_else(|| {
                        ChatError::validation(
                            "answers",
                            "OpenCode question answer does not match the prompt",
                        )
                    })?;
                let mut values = Vec::new();
                for option_id in &answer.selected_option_ids {
                    let index = option_id
                        .strip_prefix("option-")
                        .and_then(|value| value.parse::<usize>().ok())
                        .ok_or_else(|| {
                            ChatError::validation("answers", "OpenCode question option is invalid")
                        })?;
                    let label = question.option_labels.get(index).ok_or_else(|| {
                        ChatError::validation("answers", "OpenCode question option is unavailable")
                    })?;
                    values.push(label.clone());
                }
                if let Some(text) = answer
                    .free_form_text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    if !question.free_form_allowed || text.len() > 4096 || text.contains('\0') {
                        return Err(ChatError::validation(
                            "answers",
                            "OpenCode free-form question answer is invalid",
                        ));
                    }
                    values.push(text.to_string());
                }
                Ok(values)
            })
            .collect()
    }

    fn observe(&mut self, value: &Value) -> ChatResult<bool> {
        let encoded = serde_json::to_vec(value).map_err(|_| protocol_error("event envelope"))?;
        if encoded.len() > MAX_EVENT_DATA_BYTES {
            return Err(protocol_error("event envelope size"));
        }
        let fingerprint = format!("{:x}", Sha256::digest(encoded));
        if self.deduplication_set.contains(&fingerprint) {
            return Ok(false);
        }
        self.deduplication_set.insert(fingerprint.clone());
        self.deduplication_order.push_back(fingerprint);
        if self.deduplication_order.len() > MAX_DEDUPLICATION_EVENTS {
            if let Some(expired) = self.deduplication_order.pop_front() {
                self.deduplication_set.remove(&expired);
            }
        }
        Ok(true)
    }
}

pub struct OpenCodeEventNormalizer {
    provider_instance_id: ProviderInstanceId,
    thread_id: ChatThreadId,
    session_id: ProviderSessionId,
    next_event_id: AtomicU64,
}

impl OpenCodeEventNormalizer {
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
        }
    }

    pub fn normalize(
        &self,
        state: &mut OpenCodeRouteState,
        envelope: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        if !state.observe(&envelope)? {
            return Ok(Vec::new());
        }
        let object = envelope
            .as_object()
            .ok_or_else(|| protocol_error("event envelope"))?;
        let event_type = text(object, "type").ok_or_else(|| protocol_error("event type"))?;
        let properties = object
            .get("properties")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("event properties"))?;
        if event_type != "server.connected"
            && event_type != "server.heartbeat"
            && properties
                .get("sessionID")
                .and_then(Value::as_str)
                .is_some_and(|session_id| session_id != state.provider_thread_id)
        {
            return Ok(Vec::new());
        }
        match event_type {
            "server.connected" | "server.heartbeat" => Ok(Vec::new()),
            "session.updated" => self.session_updated(state, properties),
            "session.status" => self.session_status(state, properties),
            "session.error" => self.session_error(state, properties),
            "session.diff" => self.session_diff(state, properties),
            "message.updated" => self.message_updated(state, properties),
            "message.removed" => {
                if let Some(message_id) = text(properties, "messageID") {
                    state.message_roles.remove(message_id);
                }
                Ok(Vec::new())
            }
            "message.part.delta" => self.part_delta(state, properties),
            "message.part.updated" => self.part_updated(state, properties),
            "permission.asked" => self.permission_asked(state, properties),
            "permission.replied" => self.permission_replied(state, properties),
            "question.asked" => self.question_asked(state, properties),
            "question.replied" => self.question_replied(state, properties, false),
            "question.rejected" => self.question_replied(state, properties, true),
            "todo.updated" => self.todo_updated(state, properties),
            "mcp.status" | "mcp.updated" => self.mcp_status(state, properties),
            unknown => Ok(vec![self.event(
                state,
                unknown,
                None,
                None,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: format!("opencode/{unknown}"),
                    summary: "OpenCode emitted an unsupported event".to_string(),
                    safe_payload: Some(safe_shape(&Value::Object(properties.clone()))),
                }),
            )?]),
        }
    }

    pub fn external_event(
        &self,
        state: &OpenCodeRouteState,
        source: &str,
        event: CanonicalEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event(state, source, None, None, event)
    }

    fn item_event(
        &self,
        state: &OpenCodeRouteState,
        id: &str,
        kind: CanonicalItemKind,
        status: ActivityStatus,
        title: Option<String>,
        detail: Option<String>,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        let item = ItemLifecycleEvent {
            item_id: id.to_string(),
            kind,
            status,
            title,
            detail,
            safe_metadata: None,
        };
        self.item_lifecycle_event(state, id, item)
    }

    fn item_lifecycle_event(
        &self,
        state: &OpenCodeRouteState,
        id: &str,
        item: ItemLifecycleEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        validate_identifier(id, "item ID")?;
        let status = item.status;
        let event = match status {
            ActivityStatus::Pending | ActivityStatus::Active => CanonicalEvent::ItemStarted(item),
            ActivityStatus::Completed | ActivityStatus::Failed | ActivityStatus::Interrupted => {
                CanonicalEvent::ItemCompleted(item)
            }
            _ => CanonicalEvent::ItemUpdated(item),
        };
        self.event(
            state,
            "message.part.updated",
            ProviderItemId::new(id.to_string()).ok(),
            None,
            event,
        )
    }

    fn unknown_event(
        &self,
        state: &OpenCodeRouteState,
        source: &str,
        unknown_type: &str,
        value: &Map<String, Value>,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event(
            state,
            source,
            None,
            None,
            CanonicalEvent::Unknown(UnknownEvent {
                source_type: format!("opencode/{unknown_type}"),
                summary: "OpenCode emitted an unsupported protocol value".to_string(),
                safe_payload: Some(safe_shape(&Value::Object(value.clone()))),
            }),
        )
    }

    fn event(
        &self,
        state: &OpenCodeRouteState,
        source: &str,
        provider_item_id: Option<ProviderItemId>,
        turn_id: Option<ChatTurnId>,
        event: CanonicalEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event_with_request(state, source, provider_item_id, turn_id, None, event)
    }

    fn event_with_request(
        &self,
        state: &OpenCodeRouteState,
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
            provider_family_id: ProviderFamilyId::new("opencode")
                .map_err(|_| protocol_error("provider family"))?,
            provider_instance_id: self.provider_instance_id.clone(),
            thread_id: self.thread_id.clone(),
            created_at: now_utc()?,
            turn_id: turn_id.or_else(|| state.active_turn_id.clone()),
            provider_turn_id: state
                .active_turn_id
                .as_ref()
                .and_then(|turn| ProviderTurnId::new(format!("opencode-{}", turn.as_str())).ok()),
            provider_item_id,
            provider_request_id,
            provider_task_id: None,
            provider_reference: Some(VersionedJson {
                schema_version: 1,
                value: match state.rollback_message_id.as_deref() {
                    Some(message_id) => json!({
                        "messageId": message_id,
                        "partId": null,
                        "source": source,
                        "sessionId": state.provider_thread_id,
                    }),
                    None => json!({ "source": source, "sessionId": state.provider_thread_id }),
                },
            }),
            event,
            redacted_diagnostic: None,
        })
    }
}
