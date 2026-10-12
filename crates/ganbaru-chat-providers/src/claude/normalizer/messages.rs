//! Claude system and terminal result normalization.

use super::{
    ClaudeEventNormalizer, ClaudeItemEvent, ClaudeRouteState, MAX_TEXT_BYTES, bounded_shape,
    bounded_text, normalize_usage, text,
};
use crate::claude::protocol::{protocol_error, resume_cursor};
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};

impl ClaudeEventNormalizer {
    pub(super) fn system_message(
        &self,
        state: &mut ClaudeRouteState,
        object: &Map<String, Value>,
        raw: &Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let subtype = text(object, "subtype").unwrap_or("unknown");
        if let Some(session_id) = text(object, "session_id") {
            if state.provider_thread_id.is_none() {
                state.provider_thread_id = Some(session_id.to_string());
                state.resume_cursor.session_uuid = session_id.to_string();
            }
        }
        match subtype {
            "init" => {
                let provider_thread = state
                    .provider_thread_id
                    .clone()
                    .unwrap_or_else(|| state.resume_cursor.session_uuid.clone());
                let provider_id = ProviderThreadId::new(provider_thread.clone())
                    .map_err(|_| protocol_error("session UUID"))?;
                state.session_state = ProviderSessionState::Ready;
                let mut events = vec![
                    self.event(
                        state,
                        "system/init",
                        None,
                        None,
                        None,
                        CanonicalEvent::ThreadStarted(ThreadStartedEvent {
                            provider_thread_id: provider_id.clone(),
                            title: None,
                        }),
                    )?,
                    self.event(
                        state,
                        "system/init",
                        None,
                        None,
                        None,
                        CanonicalEvent::ThreadMetadataUpdated(ThreadMetadataUpdatedEvent {
                            title: None,
                            provider_thread_id: Some(provider_id),
                            resume_cursor: Some(resume_cursor(&state.resume_cursor)),
                            metadata: Some(VersionedJson {
                                schema_version: 1,
                                value: json!({
                                    "model": object.get("model"),
                                    "permissionMode": object.get("permissionMode"),
                                    "tools": object.get("tools").and_then(Value::as_array).map(Vec::len),
                                }),
                            }),
                        }),
                    )?,
                ];
                if let Some(servers) = object.get("mcp_servers").and_then(Value::as_array) {
                    for server in servers.iter().take(128) {
                        let Some(server) = server.as_object() else {
                            continue;
                        };
                        let server_id = text(server, "name")
                            .or_else(|| text(server, "id"))
                            .unwrap_or("unknown-mcp-server");
                        let raw_status = text(server, "status").unwrap_or("unknown");
                        let status = match raw_status {
                            "connected" | "ready" => ActivityStatus::Completed,
                            "connecting" | "pending" => ActivityStatus::Active,
                            "failed" | "error" => ActivityStatus::Failed,
                            _ => ActivityStatus::Unknown,
                        };
                        events.push(
                            self.event(
                                state,
                                "system/init/mcp",
                                None,
                                None,
                                None,
                                CanonicalEvent::McpStatus(McpStatusEvent {
                                    server_id: bounded_text(server_id, 512),
                                    status,
                                    detail: (status == ActivityStatus::Unknown)
                                        .then(|| bounded_text(raw_status, 512)),
                                }),
                            )?,
                        );
                    }
                }
                Ok(events)
            }
            "task_started" | "task_progress" | "task_notification" => {
                let task_id = text(object, "task_id")
                    .or_else(|| text(object, "taskId"))
                    .unwrap_or("claude-task");
                let status = match subtype {
                    "task_started" => ActivityStatus::Active,
                    "task_progress" => ActivityStatus::Active,
                    _ => ActivityStatus::Completed,
                };
                Ok(vec![
                    self.event(
                        state,
                        subtype,
                        None,
                        None,
                        None,
                        CanonicalEvent::TaskLifecycle(TaskLifecycleEvent {
                            task_id: task_id.to_string(),
                            parent_task_id: text(object, "parent_task_id").map(str::to_string),
                            status,
                            title: text(object, "subject")
                                .or_else(|| text(object, "description"))
                                .unwrap_or("Claude task")
                                .to_string(),
                            detail: text(object, "summary")
                                .map(|value| bounded_text(value, MAX_TEXT_BYTES)),
                            safe_metadata: bounded_shape(raw),
                        }),
                    )?,
                ])
            }
            "compact_boundary" => {
                let mut event = self.item_event(
                    state,
                    ClaudeItemEvent {
                        source: "system/compact_boundary",
                        item_id: self.fallback_item_id("compact"),
                        kind: CanonicalItemKind::ContextCompaction,
                        status: ActivityStatus::Completed,
                        title: Some("Context compacted".to_string()),
                        safe_metadata: None,
                    },
                )?;
                event.turn_id = None;
                Ok(vec![event])
            }
            "status" => Ok(vec![
                self.event(
                    state,
                    "system/status",
                    None,
                    None,
                    None,
                    CanonicalEvent::RuntimeWarning(NotificationEvent {
                        code: "claude_status".to_string(),
                        title: text(object, "status")
                            .unwrap_or("Claude status changed")
                            .to_string(),
                        detail: text(object, "message")
                            .map(|value| bounded_text(value, MAX_TEXT_BYTES)),
                    }),
                )?,
            ]),
            "permission_denied" => {
                Ok(vec![
                    self.event(
                        state,
                        "system/permission_denied",
                        None,
                        None,
                        None,
                        CanonicalEvent::RuntimeWarning(NotificationEvent {
                            code: "claude_permission_denied".to_string(),
                            title: "Claude denied a tool request".to_string(),
                            detail: text(object, "message")
                                .map(|value| bounded_text(value, MAX_TEXT_BYTES)),
                        }),
                    )?,
                ])
            }
            "hook_started" | "hook_progress" | "hook_response" => {
                let status = if subtype == "hook_response" {
                    ActivityStatus::Completed
                } else {
                    ActivityStatus::Active
                };
                Ok(vec![
                    self.event(
                        state,
                        subtype,
                        None,
                        None,
                        None,
                        CanonicalEvent::HookLifecycle(HookLifecycleEvent {
                            hook_id: text(object, "hook_id")
                                .or_else(|| text(object, "hook_name"))
                                .unwrap_or("claude-hook")
                                .to_string(),
                            status,
                            title: text(object, "hook_name")
                                .unwrap_or("Claude hook")
                                .to_string(),
                            detail: text(object, "message")
                                .map(|value| bounded_text(value, MAX_TEXT_BYTES)),
                        }),
                    )?,
                ])
            }
            _ => Ok(vec![self.event(
                state,
                subtype,
                None,
                None,
                None,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: format!("claude/system/{subtype}"),
                    summary: "Claude emitted an unsupported system message".to_string(),
                    safe_payload: bounded_shape(raw),
                }),
            )?]),
        }
    }

    pub(super) fn result_message(
        &self,
        state: &mut ClaudeRouteState,
        object: &Map<String, Value>,
        _raw: &Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let usage = normalize_usage(object);
        let subtype = text(object, "subtype").unwrap_or("unknown");
        let error_text = object
            .get("errors")
            .and_then(Value::as_array)
            .and_then(|errors| errors.first())
            .and_then(Value::as_str)
            .map(|value| bounded_text(value, MAX_TEXT_BYTES));
        let interrupted = subtype.to_ascii_lowercase().contains("interrupt")
            || text(object, "stop_reason")
                .is_some_and(|reason| reason.to_ascii_lowercase().contains("interrupt"))
            || error_text
                .as_deref()
                .is_some_and(|message| message.to_ascii_lowercase().contains("interrupt"));
        let is_success =
            subtype == "success" && object.get("is_error").and_then(Value::as_bool) != Some(true);
        state.resume_cursor.turn_count = state.resume_cursor.turn_count.saturating_add(1);
        state.session_state = ProviderSessionState::Ready;
        let turn_id = state.active_chat_turn_id.take();
        let event = if is_success {
            CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                state: ChatTurnState::Completed,
                stop_reason: text(object, "stop_reason").map(str::to_string),
                usage: usage.clone(),
                changed_files: Vec::new(),
            })
        } else {
            CanonicalEvent::TurnAborted(TurnAbortedEvent {
                state: if interrupted {
                    ChatTurnState::Interrupted
                } else {
                    ChatTurnState::Failed
                },
                reason: error_text.unwrap_or_else(|| {
                    if interrupted {
                        "Claude turn was interrupted".to_string()
                    } else {
                        "Claude turn failed".to_string()
                    }
                }),
                recoverable: true,
            })
        };
        let mut events = vec![self.event(state, "result", turn_id, None, None, event)?];
        if let Some(usage) = usage {
            events.push(self.event(
                state,
                "result/usage",
                None,
                None,
                None,
                CanonicalEvent::ThreadUsageUpdated(usage),
            )?);
        }
        events.push(
            self.event(
                state,
                "result/cursor",
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
        Ok(events)
    }
}
