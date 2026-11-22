//! Codex app-server notification normalization.

use super::turn_route::{TurnRoute, TurnRouteEvent, TurnRouteOutcome};
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

mod diff;
mod items;
mod lifecycle;
mod rate_limits;
mod status;
mod stop_reason;
mod value;

pub use rate_limits::CodexRateLimits;
pub use stop_reason::CodexStopReason;
use value::*;

const MAX_PROVIDER_TEXT_BYTES: usize = 64 * 1024;
const MAX_UNKNOWN_KEYS: usize = 32;
const MAX_FILE_CHANGES: usize = 512;

/// Every Codex server notification the normalizer dispatches. Any other method becomes an
/// unknown event, and the protocol coverage test checks each entry against the vendored schema.
pub(super) const CODEX_NOTIFICATION_METHODS: &[&str] = &[
    "thread/started",
    "thread/status/changed",
    "thread/name/updated",
    "thread/tokenUsage/updated",
    "turn/started",
    "turn/completed",
    "turn/plan/updated",
    "turn/diff/updated",
    "item/fileChange/patchUpdated",
    "item/started",
    "item/completed",
    "item/agentMessage/delta",
    "item/reasoning/summaryTextDelta",
    "item/reasoning/textDelta",
    "item/plan/delta",
    "item/commandExecution/outputDelta",
    "item/fileChange/outputDelta",
    "item/mcpToolCall/progress",
    "hook/started",
    "hook/completed",
    "thread/compacted",
    "account/updated",
    "account/login/completed",
    "account/rateLimits/updated",
    "mcpServer/startupStatus/updated",
    "mcpServer/oauthLogin/completed",
    "model/rerouted",
    "configWarning",
    "deprecationNotice",
    "warning",
    "guardianWarning",
    "windows/worldWritableWarning",
    "windowsSandbox/setupCompleted",
    "model/verification",
    "model/safetyBuffering/updated",
    "error",
    "thread/closed",
    "serverRequest/resolved",
    "thread/archived",
    "thread/unarchived",
    "thread/deleted",
    "thread/reverted",
    "item/reasoning/summaryPartAdded",
    "item/commandExecution/terminalInteraction",
    "item/autoApprovalReview/started",
    "item/autoApprovalReview/completed",
];

#[derive(Clone, Debug)]
pub struct CodexRouteState {
    pub provider_thread_id: Option<String>,
    /// The running turn, changed only through [`Self::transition_turn`].
    turn: TurnRoute,
    pub thread_state: ChatThreadState,
    pub modes: TurnModeSnapshot,
    pub effective_model_id: Option<ModelId>,
    pub stream_indexes: HashMap<(String, ContentStreamKind), u32>,
    pub changed_files: Vec<ChangedFileSummary>,
    pub rate_limits: CodexRateLimits,
    /// A retry `turn/start` is awaiting its response. The chat turn may already have settled, so
    /// a new turn must wait until the retried provider turn is known and can be ignored.
    pub retry_request_in_flight: bool,
    /// Recent provider turns started by a retry after their chat turn had settled. Their late
    /// notifications must not touch the active chat turn.
    abandoned_provider_turn_ids: VecDeque<String>,
}

/// Abandoned retry turns remembered per session; one retry request is in flight at a time, so a
/// few cover every late notification.
const MAX_ABANDONED_PROVIDER_TURNS: usize = 8;

impl CodexRouteState {
    /// The state a session change reports as its previous state: `Active` while a chat turn
    /// runs, `Ready` otherwise. The runtime owns the session state; this only labels events.
    pub fn running_session_state(&self) -> ProviderSessionState {
        if self.turn.chat_turn().is_some() {
            ProviderSessionState::Active
        } else {
            ProviderSessionState::Ready
        }
    }

    pub fn new(modes: TurnModeSnapshot, model_id: Option<ModelId>) -> Self {
        Self {
            provider_thread_id: None,
            turn: TurnRoute::Idle,
            thread_state: ChatThreadState::Active,
            modes,
            effective_model_id: model_id,
            stream_indexes: HashMap::new(),
            changed_files: Vec::new(),
            rate_limits: CodexRateLimits::default(),
            retry_request_in_flight: false,
            abandoned_provider_turn_ids: VecDeque::new(),
        }
    }

    /// The running turn.
    pub fn turn(&self) -> &TurnRoute {
        &self.turn
    }

    /// Applies `event` to the running turn; see [`TurnRoute::transition`].
    pub fn transition_turn(&mut self, event: TurnRouteEvent) -> ChatResult<TurnRouteOutcome> {
        self.turn.transition(event)
    }

    /// Takes the armed capacity retry out and cancels it, leaving the turn running.
    pub fn cancel_capacity_retry(&mut self) -> Option<super::capacity_retry::CodexCapacityRetry> {
        self.turn.cancel_retry()
    }

    /// The capacity retry armed for the running chat turn.
    pub fn capacity_retry_mut(&mut self) -> Option<&mut super::capacity_retry::CodexCapacityRetry> {
        self.turn.retry_mut()
    }

    /// Remembers a provider turn no chat turn owns, dropping the oldest past the bound.
    pub fn abandon_provider_turn(&mut self, provider_turn_id: &str) {
        if self.is_abandoned(provider_turn_id) {
            return;
        }
        self.abandoned_provider_turn_ids
            .push_back(provider_turn_id.to_string());
        while self.abandoned_provider_turn_ids.len() > MAX_ABANDONED_PROVIDER_TURNS {
            self.abandoned_provider_turn_ids.pop_front();
        }
    }

    /// Whether `provider_turn_id` is a retried turn abandoned after its chat turn settled.
    pub fn is_abandoned(&self, provider_turn_id: &str) -> bool {
        self.abandoned_provider_turn_ids
            .iter()
            .any(|abandoned| abandoned == provider_turn_id)
    }

    /// Whether a provider turn notification belongs to the active chat turn: it is not an
    /// abandoned retry turn, and it matches the provider turn the session tracks when one is known.
    pub fn owns_provider_turn(&self, provider_turn_id: &str) -> bool {
        !self.is_abandoned(provider_turn_id)
            && self
                .turn
                .provider_turn()
                .is_none_or(|active| active == provider_turn_id)
    }
}

pub struct CodexEventNormalizer {
    provider_instance_id: ProviderInstanceId,
    thread_id: ChatThreadId,
    session_id: ProviderSessionId,
    next_event_id: AtomicU64,
    next_fallback_id: AtomicU64,
}

impl CodexEventNormalizer {
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
            next_fallback_id: AtomicU64::new(1),
        }
    }

    pub fn normalize_notification(
        &self,
        state: &mut CodexRouteState,
        method: &str,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = params.as_object();
        let provider_thread = object.and_then(|value| text(value, "threadId"));
        if let (Some(expected), Some(actual)) =
            (state.provider_thread_id.as_deref(), provider_thread)
        {
            if expected != actual && is_child_thread_lifecycle(method) {
                return Ok(Vec::new());
            }
        }
        if notification_turn_id(&params).is_some_and(|turn_id| state.is_abandoned(turn_id)) {
            return Ok(Vec::new());
        }
        if !CODEX_NOTIFICATION_METHODS.contains(&method) {
            return self.unknown_notification(state, method, &params);
        }
        match method {
            "thread/started" => self.thread_started(state, params),
            "thread/status/changed" => self.thread_status_changed(state, params),
            "thread/name/updated" => self.thread_metadata(state, params),
            "thread/tokenUsage/updated" => self.thread_usage(state, params),
            "turn/started" => self.turn_started(state, params),
            "turn/completed" => self.turn_completed(state, params),
            "turn/plan/updated" => self.plan_updated(state, params),
            "turn/diff/updated" | "item/fileChange/patchUpdated" => {
                self.diff_updated(state, method, params)
            }
            "item/started" => self.item_lifecycle(state, params, false),
            "item/completed" => self.item_lifecycle(state, params, true),
            "item/agentMessage/delta" => self.content_delta(
                state,
                params,
                ContentStreamKind::AssistantText,
                "delta",
                None,
            ),
            "item/reasoning/summaryTextDelta" => self.content_delta(
                state,
                params,
                ContentStreamKind::ReasoningSummary,
                "delta",
                Some("summaryIndex"),
            ),
            "item/reasoning/textDelta" => self.content_delta(
                state,
                params,
                ContentStreamKind::ReasoningText,
                "delta",
                Some("contentIndex"),
            ),
            "item/plan/delta" => self.proposed_plan_delta(state, params),
            "item/commandExecution/outputDelta" => self.content_delta(
                state,
                params,
                ContentStreamKind::CommandOutput,
                "delta",
                None,
            ),
            "item/fileChange/outputDelta" => self.content_delta(
                state,
                params,
                ContentStreamKind::FileChangeOutput,
                "delta",
                None,
            ),
            "item/mcpToolCall/progress" => self.tool_progress(state, params),
            "hook/started" => self.hook_lifecycle(state, params, false),
            "hook/completed" => self.hook_lifecycle(state, params, true),
            "thread/compacted" => self.context_compacted(state, params),
            "account/updated" | "account/login/completed" => {
                self.authentication_status(state, method, params)
            }
            "account/rateLimits/updated" => self.rate_limit_status(state, params),
            "mcpServer/startupStatus/updated" => self.mcp_status(state, params),
            "mcpServer/oauthLogin/completed" => self.mcp_oauth(state, params),
            "model/rerouted" => self.model_rerouted(state, params),
            "configWarning" => self.notification(
                state,
                params,
                "codex_configuration_warning",
                CanonicalNotificationKind::Configuration,
            ),
            "deprecationNotice" => self.notification(
                state,
                params,
                "codex_deprecation",
                CanonicalNotificationKind::Deprecation,
            ),
            "warning"
            | "guardianWarning"
            | "windows/worldWritableWarning"
            | "windowsSandbox/setupCompleted"
            | "model/verification"
            | "model/safetyBuffering/updated" => self.notification(
                state,
                params,
                "codex_runtime_warning",
                CanonicalNotificationKind::Runtime,
            ),
            "error" => self.runtime_error(state, params),
            "thread/closed" => self.session_closed(state, params),
            "serverRequest/resolved"
            | "thread/archived"
            | "thread/unarchived"
            | "thread/deleted"
            | "thread/reverted"
            | "item/reasoning/summaryPartAdded"
            | "item/commandExecution/terminalInteraction"
            | "item/autoApprovalReview/started"
            | "item/autoApprovalReview/completed" => Ok(Vec::new()),
            _ => self.unknown_notification(state, method, &params),
        }
    }

    /// An unknown event that keeps the bounded shape of a notification the normalizer does not
    /// handle.
    fn unknown_notification(
        &self,
        state: &CodexRouteState,
        method: &str,
        params: &Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        Ok(vec![self.event(
            state,
            method,
            route_turn(state, params.as_object()),
            None,
            None,
            CanonicalEvent::Unknown(UnknownEvent {
                source_type: method.to_string(),
                summary: "Codex emitted an unsupported notification".to_string(),
                safe_payload: bounded_shape(params),
            }),
        )?])
    }

    pub fn malformed_event(
        &self,
        state: &CodexRouteState,
        reason: &str,
        byte_length: usize,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event(
            state,
            "malformed",
            None,
            None,
            None,
            CanonicalEvent::RuntimeError(RuntimeErrorEvent {
                code: "codex_malformed_protocol".to_string(),
                message: bounded_text(reason, MAX_PROVIDER_TEXT_BYTES),
                recoverable: true,
                safe_details: Some(VersionedJson {
                    schema_version: 1,
                    value: json!({ "byteLength": byte_length }),
                }),
            }),
        )
    }

    /// The runtime error for a line over the JSON Lines bound, which ends the session.
    pub fn oversized_event(
        &self,
        state: &CodexRouteState,
        byte_length: usize,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event(
            state,
            "malformed",
            None,
            None,
            None,
            CanonicalEvent::RuntimeError(RuntimeErrorEvent {
                code: "codex_oversized_message".to_string(),
                message: super::transport::OVERSIZED_MESSAGE_ERROR.to_string(),
                recoverable: false,
                safe_details: Some(VersionedJson {
                    schema_version: 1,
                    value: json!({ "byteLength": byte_length }),
                }),
            }),
        )
    }

    /// The runtime error for a message the router could not route, which ends the session.
    pub fn route_failed_event(
        &self,
        state: &CodexRouteState,
        error: &ChatError,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        self.event(
            state,
            "transport/route",
            None,
            None,
            None,
            CanonicalEvent::RuntimeError(RuntimeErrorEvent {
                code: "codex_unroutable_message".to_string(),
                message: error.message.clone(),
                recoverable: false,
                safe_details: None,
            }),
        )
    }

    pub fn request_event(
        &self,
        state: &CodexRouteState,
        provider_request_id: ProviderRequestId,
        provider_turn_id: Option<ProviderTurnId>,
        provider_item_id: Option<ProviderItemId>,
        turn_id: Option<ChatTurnId>,
        event: CanonicalEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        let mut runtime = self.event(
            state,
            "server/request",
            turn_id,
            provider_turn_id,
            provider_item_id,
            event,
        )?;
        runtime.provider_request_id = Some(provider_request_id);
        Ok(runtime)
    }

    pub(super) fn session_id(&self) -> ProviderSessionId {
        self.session_id.clone()
    }

    pub(super) fn event(
        &self,
        _state: &CodexRouteState,
        method: &str,
        turn_id: Option<ChatTurnId>,
        provider_turn_id: Option<ProviderTurnId>,
        provider_item_id: Option<ProviderItemId>,
        event: CanonicalEvent,
    ) -> ChatResult<CanonicalRuntimeEvent> {
        let sequence = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        let provider_reference = self.provider_reference(method, provider_turn_id.as_ref());
        Ok(CanonicalRuntimeEvent {
            schema_version: CANONICAL_EVENT_SCHEMA_VERSION,
            event_id: ChatEventId::new(format!("{}:{sequence}", self.session_id.as_str()))
                .map_err(identifier_error)?,
            provider_family_id: ProviderFamilyId::new("codex").map_err(identifier_error)?,
            provider_instance_id: self.provider_instance_id.clone(),
            thread_id: self.thread_id.clone(),
            created_at: now_utc()?,
            turn_id,
            provider_turn_id,
            provider_item_id,
            provider_request_id: None,
            provider_task_id: None,
            provider_reference: Some(VersionedJson {
                schema_version: 1,
                value: provider_reference,
            }),
            event,
            redacted_diagnostic: None,
        })
    }

    /// The notification method and, for a turn id Codex assigned, the `turnId` a checkpoint
    /// restore later rewinds the native thread to. A fallback turn id is never a rewind point.
    fn provider_reference(&self, method: &str, provider_turn_id: Option<&ProviderTurnId>) -> Value {
        let method = bounded_text(method, 256);
        match provider_turn_id.filter(|turn| !self.is_fallback_id(turn.as_str())) {
            Some(turn) => json!({ "method": method, "turnId": turn.as_str() }),
            None => json!({ "method": method }),
        }
    }

    fn is_fallback_id(&self, id: &str) -> bool {
        id.strip_prefix(self.session_id.as_str())
            .is_some_and(|rest| rest.starts_with(':'))
    }

    fn fallback_provider_thread_id(&self) -> ProviderThreadId {
        ProviderThreadId::new(self.fallback_id("thread"))
            .expect("bounded fallback provider thread ID must be valid")
    }

    fn fallback_provider_turn_id(&self) -> ProviderTurnId {
        ProviderTurnId::new(self.fallback_id("turn"))
            .expect("bounded fallback provider turn ID must be valid")
    }

    fn fallback_provider_item_id(&self) -> ProviderItemId {
        ProviderItemId::new(self.fallback_id("item"))
            .expect("bounded fallback provider item ID must be valid")
    }

    fn fallback_id(&self, kind: &str) -> String {
        format!(
            "{}:{kind}:{}",
            self.session_id.as_str(),
            self.next_fallback_id.fetch_add(1, Ordering::Relaxed)
        )
    }
}

/// The provider turn a notification names, from `turnId` or the `turn.id` of lifecycle events.
fn notification_turn_id(params: &Value) -> Option<&str> {
    params
        .get("turnId")
        .or_else(|| params.get("turn").and_then(|turn| turn.get("id")))
        .and_then(Value::as_str)
}
