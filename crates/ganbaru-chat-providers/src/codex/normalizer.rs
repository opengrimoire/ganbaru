//! Codex app-server notification normalization.

use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

mod diff;
mod items;
mod lifecycle;
mod status;
mod value;

use value::*;

const MAX_PROVIDER_TEXT_BYTES: usize = 64 * 1024;
const MAX_UNKNOWN_KEYS: usize = 32;
const MAX_FILE_CHANGES: usize = 512;

#[derive(Clone, Debug)]
pub struct CodexRouteState {
    pub provider_thread_id: Option<String>,
    pub active_chat_turn_id: Option<ChatTurnId>,
    pub active_provider_turn_id: Option<String>,
    pub session_state: ProviderSessionState,
    pub thread_state: ChatThreadState,
    pub modes: TurnModeSnapshot,
    pub effective_model_id: Option<ModelId>,
    pub stream_indexes: HashMap<(String, ContentStreamKind), u32>,
    pub changed_files: Vec<ChangedFileSummary>,
}

impl CodexRouteState {
    pub fn new(modes: TurnModeSnapshot, model_id: Option<ModelId>) -> Self {
        Self {
            provider_thread_id: None,
            active_chat_turn_id: None,
            active_provider_turn_id: None,
            session_state: ProviderSessionState::Starting,
            thread_state: ChatThreadState::Active,
            modes,
            effective_model_id: model_id,
            stream_indexes: HashMap::new(),
            changed_files: Vec::new(),
        }
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
            | "item/reasoning/summaryPartAdded"
            | "item/commandExecution/terminalInteraction"
            | "item/autoApprovalReview/started"
            | "item/autoApprovalReview/completed" => Ok(Vec::new()),
            _ => Ok(vec![self.event(
                state,
                method,
                route_turn(state, object),
                None,
                None,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: method.to_string(),
                    summary: "Codex emitted an unsupported notification".to_string(),
                    safe_payload: bounded_shape(&params),
                }),
            )?]),
        }
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
                value: json!({ "method": bounded_text(method, 256) }),
            }),
            event,
            redacted_diagnostic: None,
        })
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
            "codex-{kind}-{}",
            self.next_fallback_id.fetch_add(1, Ordering::Relaxed)
        )
    }
}
