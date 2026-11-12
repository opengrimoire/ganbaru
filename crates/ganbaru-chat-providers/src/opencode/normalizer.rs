//! OpenCode event normalization into provider-neutral Chat events.

use super::permissions::{OpenCodeSessionRule, OpenCodeSessionRules};
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
/// Patterns one "always" permission reply may approve while the prompt still lists each of them.
const MAX_SESSION_GRANT_PATTERNS: usize = 32;
/// Longest "always" pattern the prompt shows as a session grant.
const MAX_SESSION_GRANT_PATTERN_BYTES: usize = 4096;
/// Provider option recorded on approvals the driver answered from a session rule.
pub const SESSION_RULE_OPTION_ID: &str = "session_rule";

#[derive(Clone, Debug)]
pub struct OpenCodeRouteState {
    pub provider_thread_id: String,
    pub active_turn_id: Option<ChatTurnId>,
    /// The turn the user interrupted or stopped; when OpenCode reports idle for it before the abort
    /// returns, the turn settles as interrupted rather than completed.
    interrupt_requested: Option<ChatTurnId>,
    pub session_state: ProviderSessionState,
    pub modes: TurnModeSnapshot,
    message_roles: HashMap<String, String>,
    rollback_message_id: Option<String>,
    part_text: HashMap<String, String>,
    completed_parts: HashSet<String>,
    pending_permissions: HashMap<String, OpenCodePendingPermission>,
    session_rules: OpenCodeSessionRules,
    /// Requests the driver answered from a session rule; their `permission.replied` echo is dropped.
    auto_answered_permissions: HashSet<String>,
    /// Requests a session rule covers that still need their `once` reply sent.
    auto_answer_queue: Vec<ProviderRequestId>,
    /// Requests the user allowed for the session; OpenCode echoes their reply as `once`.
    session_approved_permissions: HashSet<String>,
    pending_questions: HashMap<String, Vec<OpenCodeQuestionMapping>>,
    deduplication_order: VecDeque<String>,
    deduplication_set: HashSet<String>,
}

/// A permission request still waiting for an answer.
#[derive(Clone, Debug)]
struct OpenCodePendingPermission {
    permission: String,
    patterns: Vec<String>,
    always: Vec<String>,
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
            interrupt_requested: None,
            session_state: ProviderSessionState::Ready,
            modes,
            message_roles: HashMap::new(),
            rollback_message_id: None,
            part_text: HashMap::new(),
            completed_parts: HashSet::new(),
            pending_permissions: HashMap::new(),
            session_rules: OpenCodeSessionRules::default(),
            auto_answered_permissions: HashSet::new(),
            auto_answer_queue: Vec::new(),
            session_approved_permissions: HashSet::new(),
            pending_questions: HashMap::new(),
            deduplication_order: VecDeque::new(),
            deduplication_set: HashSet::new(),
        }
    }

    pub fn has_permission(&self, request_id: &ProviderRequestId) -> bool {
        self.pending_permissions.contains_key(request_id.as_str())
    }

    /// Whether a permission or question is waiting for the user, so the session may stay silent
    /// for as long as the user takes to answer.
    pub fn awaiting_user(&self) -> bool {
        !self.pending_permissions.is_empty() || !self.pending_questions.is_empty()
    }

    /// Marks the active turn as interrupted by the user and returns it, or returns `None` when
    /// no turn is active.
    pub fn request_interrupt(&mut self) -> Option<ChatTurnId> {
        self.interrupt_requested = self.active_turn_id.clone();
        self.interrupt_requested.clone()
    }

    /// Clears an interrupt mark after the abort request failed, so a later idle completes the turn.
    pub fn cancel_interrupt(&mut self) {
        self.interrupt_requested = None;
    }

    /// Takes the active turn when it is still the interrupted one, so the caller settles it.
    pub fn take_interrupted_turn(&mut self, turn_id: &ChatTurnId) -> Option<ChatTurnId> {
        if self.active_turn_id.as_ref() != Some(turn_id) {
            return None;
        }
        self.interrupt_requested = None;
        self.active_turn_id.take()
    }

    /// Session rules an allow-for-session answer to a pending request would grant.
    ///
    /// The rules follow the patterns OpenCode offered for `always`, then the requested patterns,
    /// then the bare permission.
    pub fn session_rules_for(&self, request_id: &ProviderRequestId) -> Vec<OpenCodeSessionRule> {
        let Some(pending) = self.pending_permissions.get(request_id.as_str()) else {
            return Vec::new();
        };
        let patterns = if pending.always.is_empty() {
            &pending.patterns
        } else {
            &pending.always
        };
        if patterns.is_empty() {
            return vec![OpenCodeSessionRule {
                permission: pending.permission.clone(),
                pattern: None,
            }];
        }
        patterns
            .iter()
            .map(|pattern| OpenCodeSessionRule {
                permission: pending.permission.clone(),
                pattern: Some(pattern.clone()),
            })
            .collect()
    }

    /// Marks a pending request as allowed for the session, or clears the mark when the reply
    /// failed, so the reply echo keeps the session decision.
    pub fn set_session_approval(&mut self, request_id: &ProviderRequestId, approved: bool) {
        if approved {
            self.session_approved_permissions
                .insert(request_id.as_str().to_string());
        } else {
            self.session_approved_permissions
                .remove(request_id.as_str());
        }
    }

    /// Remembers session approvals and returns how many older rules were dropped.
    pub fn grant_session_rules(&mut self, rules: Vec<OpenCodeSessionRule>) -> usize {
        self.session_rules.grant(rules)
    }

    /// Requests a session rule answered during the last normalization, in arrival order.
    pub fn take_auto_answers(&mut self) -> Vec<ProviderRequestId> {
        std::mem::take(&mut self.auto_answer_queue)
    }

    /// Returns an automatically answered request to the user after its reply failed.
    pub fn release_auto_answer(&mut self, request_id: &ProviderRequestId) {
        self.auto_answered_permissions.remove(request_id.as_str());
    }

    /// Forgets a request answered from a session rule once OpenCode accepted the reply.
    pub fn complete_auto_answer(&mut self, request_id: &ProviderRequestId) {
        self.pending_permissions.remove(request_id.as_str());
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

    /// Whether `value` is new to this route. A repeated record envelope is a redelivery and is
    /// dropped; a transition or increment repeats legitimately with identical bytes, such as the
    /// busy and idle status of every turn or the same delta twice, so it is never dropped.
    fn observe(&mut self, value: &Value) -> ChatResult<bool> {
        let encoded = serde_json::to_vec(value).map_err(|_| protocol_error("event envelope"))?;
        if encoded.len() > MAX_EVENT_DATA_BYTES {
            return Err(protocol_error("event envelope size"));
        }
        if value
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(repeats_legitimately)
        {
            return Ok(true);
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

/// Whether events of `event_type` are transitions or increments that may repeat byte for byte.
fn repeats_legitimately(event_type: &str) -> bool {
    matches!(
        event_type,
        "session.status"
            | "session.error"
            | "message.part.delta"
            | "server.connected"
            | "server.heartbeat"
    )
}

/// Whether an event names a session other than `session_id`. Events carry the session at the
/// top level and again inside the message, part, or session record they describe; a mismatch
/// in any of them, such as a subagent's child session, keeps the event out of this thread.
fn names_another_session(
    event_type: &str,
    properties: &serde_json::Map<String, Value>,
    session_id: &str,
) -> bool {
    let nested = |record: &str, key: &str| {
        properties
            .get(record)
            .and_then(Value::as_object)
            .and_then(|record| record.get(key))
            .and_then(Value::as_str)
    };
    let session_record_id = if event_type.starts_with("session.") {
        nested("info", "id")
    } else {
        None
    };
    [
        properties.get("sessionID").and_then(Value::as_str),
        nested("info", "sessionID"),
        nested("part", "sessionID"),
        session_record_id,
    ]
    .into_iter()
    .flatten()
    .any(|named| named != session_id)
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
            && names_another_session(event_type, properties, &state.provider_thread_id)
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

    /// Settles a turn the user interrupted.
    pub fn interrupted_turn(
        &self,
        state: &OpenCodeRouteState,
        source: &str,
        turn_id: ChatTurnId,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event(
            state,
            source,
            None,
            Some(turn_id),
            CanonicalEvent::TurnAborted(TurnAbortedEvent {
                state: ChatTurnState::Interrupted,
                reason: "Interrupted by user".to_string(),
                recoverable: true,
            }),
        )
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
