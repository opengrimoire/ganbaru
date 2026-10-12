//! Claude inbound message routing, permission callbacks, structured questions, and pending request state.

use super::normalizer::{ClaudeEventNormalizer, ClaudeRouteState};
use super::protocol::{control_success, protocol_error};
use super::transport::{ClaudeInboundMessage, ClaudeJsonlClient};
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const MAX_QUESTIONS: usize = 16;
const MAX_OPTIONS: usize = 32;
const MAX_TEXT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub enum PendingClaudeRequestKind {
    Approval {
        tool_input: Value,
        permission_suggestions: Option<Value>,
    },
    UserInput {
        questions: Value,
        option_labels: HashMap<String, HashMap<String, String>>,
        provider_question_labels: HashMap<String, String>,
    },
}

#[derive(Clone, Debug)]
pub struct PendingClaudeRequest {
    pub control_request_id: String,
    pub provider_request_id: ProviderRequestId,
    pub kind: PendingClaudeRequestKind,
}

pub type PendingClaudeRequests = Arc<Mutex<HashMap<ProviderRequestId, PendingClaudeRequest>>>;

pub struct RoutedClaudeRequest {
    pub pending: Option<PendingClaudeRequest>,
    pub event: CanonicalEvent,
    pub immediate_response: Option<Value>,
}

pub struct ClaudeRouterResources {
    pub client: ClaudeJsonlClient,
    pub inbound: mpsc::Receiver<ClaudeInboundMessage>,
    pub normalizer: Arc<ClaudeEventNormalizer>,
    pub route: Arc<Mutex<ClaudeRouteState>>,
    pub pending_requests: PendingClaudeRequests,
    pub sink: Arc<dyn crate::ProviderEventSink>,
    pub expected_shutdown: Arc<AtomicBool>,
    pub terminal_error: Arc<Mutex<Option<ChatError>>>,
}

pub fn spawn_claude_router(resources: ClaudeRouterResources) -> JoinHandle<()> {
    tokio::spawn(run_claude_router(resources))
}

async fn run_claude_router(mut resources: ClaudeRouterResources) {
    while let Some(message) = resources.inbound.recv().await {
        let result = match message {
            ClaudeInboundMessage::Message(value) => route_message(&resources, value).await,
            ClaudeInboundMessage::Malformed {
                reason,
                byte_length,
            } => route_malformed(&resources, &reason, byte_length).await,
            ClaudeInboundMessage::Closed => {
                route_closed(&resources).await;
                return;
            }
        };
        if let Err(error) = result {
            if let Ok(mut terminal) = resources.terminal_error.lock() {
                *terminal = Some(error);
            }
            return;
        }
    }
    route_closed(&resources).await;
}

async fn route_message(resources: &ClaudeRouterResources, value: Value) -> ChatResult<()> {
    if value.get("type").and_then(Value::as_str) == Some("control_request") {
        return route_provider_request(resources, &value).await;
    }
    let events = {
        let mut state = resources.route.lock().map_err(|_| state_error())?;
        resources.normalizer.normalize_message(&mut state, value)?
    };
    emit_events(&resources.sink, events).await
}

async fn route_provider_request(
    resources: &ClaudeRouterResources,
    value: &Value,
) -> ChatResult<()> {
    let object = value
        .as_object()
        .ok_or_else(|| protocol_error("control request"))?;
    let control_request_id = required_text(object, "request_id")?;
    let request = object
        .get("request")
        .and_then(Value::as_object)
        .ok_or_else(|| protocol_error("control request body"))?;
    let routed = route_control_request(control_request_id, request)?;
    if let Some(response) = routed.immediate_response {
        resources
            .client
            .send(response)
            .await
            .map_err(|error| error.to_chat_error("control response"))?;
    }
    let provider_request_id = routed
        .pending
        .as_ref()
        .map(|pending| pending.provider_request_id.clone());
    if let Some(pending) = routed.pending {
        let mut requests = resources
            .pending_requests
            .lock()
            .map_err(|_| state_error())?;
        if requests.contains_key(&pending.provider_request_id) {
            return Err(protocol_error("duplicate permission request ID"));
        }
        requests.insert(pending.provider_request_id.clone(), pending);
    }
    let event = {
        let mut state = resources.route.lock().map_err(|_| state_error())?;
        if provider_request_id.is_some() {
            state.session_state = if matches!(&routed.event, CanonicalEvent::UserInputRequested(_))
            {
                ProviderSessionState::WaitingForUserInput
            } else {
                ProviderSessionState::WaitingForApproval
            };
        }
        resources.normalizer.external_event(
            &state,
            "control/can_use_tool",
            provider_request_id,
            routed.event,
        )?
    };
    resources.sink.emit(event).await
}

async fn route_malformed(
    resources: &ClaudeRouterResources,
    reason: &str,
    byte_length: usize,
) -> ChatResult<()> {
    let event = {
        let state = resources.route.lock().map_err(|_| state_error())?;
        resources.normalizer.external_event(
            &state,
            "transport/malformed",
            None,
            CanonicalEvent::RuntimeWarning(NotificationEvent {
                code: "claude_malformed_message".to_string(),
                title: "Claude emitted an invalid message".to_string(),
                detail: Some(format!("{reason}; {byte_length} bytes")),
            }),
        )?
    };
    resources.sink.emit(event).await
}

async fn route_closed(resources: &ClaudeRouterResources) {
    let expected = resources.expected_shutdown.load(Ordering::Acquire);
    let event = resources
        .route
        .lock()
        .map_err(|_| state_error())
        .and_then(|mut state| {
            let interrupted = resources
                .normalizer
                .interrupted_event(&mut state, "Claude stream closed")?;
            state.session_state = if expected {
                ProviderSessionState::Stopped
            } else {
                ProviderSessionState::Failed
            };
            let changed = resources.normalizer.external_event(
                &state,
                "transport/closed",
                None,
                CanonicalEvent::SessionStateChanged(SessionStateChangedEvent {
                    session_id: resources.normalizer.session_id(),
                    previous_state: if interrupted.is_some() {
                        ProviderSessionState::Active
                    } else {
                        ProviderSessionState::Ready
                    },
                    state: state.session_state,
                    reason: Some(if expected {
                        "Claude session stopped".to_string()
                    } else {
                        "Claude stream closed unexpectedly".to_string()
                    }),
                }),
            )?;
            Ok(interrupted.into_iter().chain([changed]).collect::<Vec<_>>())
        });
    if let Ok(events) = event {
        let _ = emit_events(&resources.sink, events).await;
    }
    if !expected {
        if let Ok(mut terminal) = resources.terminal_error.lock() {
            *terminal = Some(ChatError::new(
                ChatErrorCode::TransportUnavailable,
                "Claude stream closed unexpectedly",
                true,
            ));
        }
    }
}

async fn emit_events(
    sink: &Arc<dyn crate::ProviderEventSink>,
    events: Vec<CanonicalRuntimeEvent>,
) -> ChatResult<()> {
    for event in events {
        sink.emit(event).await?;
    }
    Ok(())
}

pub fn route_control_request(
    control_request_id: &str,
    request: &Map<String, Value>,
) -> ChatResult<RoutedClaudeRequest> {
    let subtype = required_text(request, "subtype")?;
    if subtype != "can_use_tool" {
        return Ok(RoutedClaudeRequest {
            pending: None,
            event: CanonicalEvent::Unknown(UnknownEvent {
                source_type: format!("claude/control/{subtype}"),
                summary: "Claude requested an unsupported client operation".to_string(),
                safe_payload: Some(VersionedJson {
                    schema_version: 1,
                    value: json!({ "keys": bounded_keys(request) }),
                }),
            }),
            immediate_response: Some(control_success(
                control_request_id,
                json!({ "behavior": "deny", "message": "Unsupported client operation" }),
            )?),
        });
    }
    let tool_name = required_text(request, "tool_name")?;
    let tool_input = request
        .get("input")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| protocol_error("permission tool input"))?;
    let provider_request_id = ProviderRequestId::new(control_request_id.to_string())
        .map_err(|_| protocol_error("permission request ID"))?;
    if tool_name == "AskUserQuestion" {
        return structured_question(control_request_id, provider_request_id, tool_input);
    }
    if tool_name == "ExitPlanMode" {
        let markdown = extract_plan_markdown(&tool_input).unwrap_or_default();
        return Ok(RoutedClaudeRequest {
            pending: None,
            event: CanonicalEvent::ProposedPlanCompleted(ProposedPlanCompletedEvent {
                plan_id: control_request_id.to_string(),
                markdown,
            }),
            immediate_response: Some(control_success(
                control_request_id,
                json!({
                    "behavior": "deny",
                    "message": "Ganbaru captured the plan. Wait for a later implementation request.",
                }),
            )?),
        });
    }
    let kind = classify_request(tool_name);
    let title = request
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| request.get("display_name").and_then(Value::as_str))
        .unwrap_or(tool_name);
    let detail = request
        .get("description")
        .and_then(Value::as_str)
        .or_else(|| request.get("decision_reason").and_then(Value::as_str))
        .map(|value| bounded_text(value, MAX_TEXT_BYTES));
    let mut allowed_decisions = vec![approval_option(
        "allow_once",
        "Allow once",
        ApprovalDecisionKind::AllowOnce,
    )];
    if request
        .get("permission_suggestions")
        .and_then(Value::as_array)
        .is_some_and(|suggestions| !suggestions.is_empty())
    {
        allowed_decisions.push(approval_option(
            "allow_session",
            "Allow for session",
            ApprovalDecisionKind::AllowSession,
        ));
    }
    allowed_decisions.extend([
        approval_option("deny", "Deny", ApprovalDecisionKind::Deny),
        approval_option("cancel", "Cancel turn", ApprovalDecisionKind::Cancel),
    ]);
    let pending = PendingClaudeRequest {
        control_request_id: control_request_id.to_string(),
        provider_request_id: provider_request_id.clone(),
        kind: PendingClaudeRequestKind::Approval {
            tool_input: tool_input.clone(),
            permission_suggestions: request.get("permission_suggestions").cloned(),
        },
    };
    Ok(RoutedClaudeRequest {
        pending: Some(pending),
        event: CanonicalEvent::RequestOpened(RequestOpenedEvent {
            request_id: provider_request_id,
            kind,
            title: bounded_text(title, MAX_TEXT_BYTES),
            detail,
            allowed_decisions,
            safe_payload: VersionedJson {
                schema_version: 1,
                value: json!({
                    "toolName": tool_name,
                    "toolUseId": request.get("tool_use_id"),
                    "hasBlockedPath": request.get("blocked_path").is_some(),
                    "input": tool_input,
                }),
            },
        }),
        immediate_response: None,
    })
}

fn structured_question(
    control_request_id: &str,
    provider_request_id: ProviderRequestId,
    tool_input: Value,
) -> ChatResult<RoutedClaudeRequest> {
    let questions_value = tool_input
        .get("questions")
        .filter(|value| value.is_array())
        .cloned()
        .ok_or_else(|| protocol_error("AskUserQuestion questions"))?;
    let questions = questions_value.as_array().cloned().unwrap_or_default();
    if questions.is_empty() || questions.len() > MAX_QUESTIONS {
        return Err(protocol_error("AskUserQuestion question count"));
    }
    let mut canonical = Vec::with_capacity(questions.len());
    let mut option_labels = HashMap::new();
    let mut provider_question_labels = HashMap::new();
    for (index, question) in questions.iter().enumerate() {
        let object = question
            .as_object()
            .ok_or_else(|| protocol_error("AskUserQuestion question"))?;
        let text = required_text(object, "question")?;
        let id = format!("question-{index}");
        let raw_options = object
            .get("options")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if raw_options.len() > MAX_OPTIONS {
            return Err(protocol_error("AskUserQuestion option count"));
        }
        let mut labels = HashMap::new();
        let mut options = Vec::new();
        for (option_index, option) in raw_options.iter().enumerate() {
            let option = option
                .as_object()
                .ok_or_else(|| protocol_error("AskUserQuestion option"))?;
            let label = required_text(option, "label")?;
            let option_id = format!("option-{index}-{option_index}");
            labels.insert(option_id.clone(), label.to_string());
            options.push(UserInputOption {
                id: option_id,
                label: bounded_text(label, MAX_TEXT_BYTES),
                description: option
                    .get("description")
                    .and_then(Value::as_str)
                    .map(|value| bounded_text(value, MAX_TEXT_BYTES)),
            });
        }
        option_labels.insert(id.clone(), labels);
        provider_question_labels.insert(id.clone(), text.to_string());
        canonical.push(UserInputQuestion {
            id,
            header: object
                .get("header")
                .and_then(Value::as_str)
                .map(|value| bounded_text(value, 160)),
            question: bounded_text(text, MAX_TEXT_BYTES),
            options,
            multiple: object
                .get("multiSelect")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            free_form_allowed: true,
            required: true,
        });
    }
    let pending = PendingClaudeRequest {
        control_request_id: control_request_id.to_string(),
        provider_request_id: provider_request_id.clone(),
        kind: PendingClaudeRequestKind::UserInput {
            questions: questions_value,
            option_labels,
            provider_question_labels,
        },
    };
    Ok(RoutedClaudeRequest {
        pending: Some(pending),
        event: CanonicalEvent::UserInputRequested(UserInputRequestedEvent {
            request_id: provider_request_id,
            questions: canonical,
        }),
        immediate_response: None,
    })
}

pub async fn resolve_approval(
    client: &ClaudeJsonlClient,
    pending: &PendingClaudeRequests,
    request: &ResolveApprovalRequest,
) -> ChatResult<()> {
    let pending_request = remove_exact(pending, &request.provider_request_id)?;
    let response = match approval_response(&pending_request, request) {
        Ok(response) => response,
        Err(error) => {
            restore_pending(pending, pending_request)?;
            return Err(error);
        }
    };
    if let Err(error) = client
        .send(control_success(
            &pending_request.control_request_id,
            response,
        )?)
        .await
    {
        restore_pending(pending, pending_request)?;
        return Err(error.to_chat_error("approval response"));
    }
    Ok(())
}

fn approval_response(
    pending: &PendingClaudeRequest,
    request: &ResolveApprovalRequest,
) -> ChatResult<Value> {
    let PendingClaudeRequestKind::Approval {
        tool_input,
        permission_suggestions,
    } = &pending.kind
    else {
        return Err(ChatError::invalid_transition(
            "Claude request expects structured input",
        ));
    };
    let updated_input = request
        .decision
        .updated_tool_input
        .as_ref()
        .map(|value| value.value.clone())
        .unwrap_or_else(|| tool_input.clone());
    if !updated_input.is_object() {
        return Err(ChatError::validation(
            "updatedToolInput",
            "Claude updated tool input must be an object",
        ));
    }
    Ok(match request.decision.kind {
        ApprovalDecisionKind::AllowOnce => {
            json!({ "behavior": "allow", "updatedInput": updated_input })
        }
        ApprovalDecisionKind::AllowSession => {
            let mut response = Map::from_iter([
                ("behavior".to_string(), Value::String("allow".to_string())),
                ("updatedInput".to_string(), updated_input),
            ]);
            let Some(suggestions) = permission_suggestions
                .as_ref()
                .and_then(Value::as_array)
                .filter(|suggestions| !suggestions.is_empty())
            else {
                return Err(ChatError::validation(
                    "decision",
                    "Claude did not offer a session permission update",
                ));
            };
            response.insert(
                "updatedPermissions".to_string(),
                Value::Array(suggestions.clone()),
            );
            Value::Object(response)
        }
        ApprovalDecisionKind::Deny => {
            json!({ "behavior": "deny", "message": "User declined tool execution." })
        }
        ApprovalDecisionKind::Cancel => json!({
            "behavior": "deny",
            "message": "User cancelled tool execution.",
            "interrupt": true,
        }),
    })
}

pub async fn resolve_user_input(
    client: &ClaudeJsonlClient,
    pending: &PendingClaudeRequests,
    request: &ResolveUserInputRequest,
) -> ChatResult<()> {
    let pending_request = remove_exact(pending, &request.provider_request_id)?;
    let response = match user_input_response(&pending_request, request) {
        Ok(response) => response,
        Err(error) => {
            restore_pending(pending, pending_request)?;
            return Err(error);
        }
    };
    if let Err(error) = client
        .send(control_success(
            &pending_request.control_request_id,
            response,
        )?)
        .await
    {
        restore_pending(pending, pending_request)?;
        return Err(error.to_chat_error("structured input response"));
    }
    Ok(())
}

fn user_input_response(
    pending: &PendingClaudeRequest,
    request: &ResolveUserInputRequest,
) -> ChatResult<Value> {
    let PendingClaudeRequestKind::UserInput {
        questions,
        option_labels,
        provider_question_labels,
    } = &pending.kind
    else {
        return Err(ChatError::invalid_transition(
            "Claude request expects an approval decision",
        ));
    };
    let mut answers = Map::new();
    for answer in &request.answers {
        let labels = option_labels.get(&answer.question_id).ok_or_else(|| {
            ChatError::validation("answers", "Claude answer references an unknown question")
        })?;
        let mut values = Vec::new();
        for option_id in &answer.selected_option_ids {
            values.push(labels.get(option_id).cloned().ok_or_else(|| {
                ChatError::validation("answers", "Claude answer references an unknown option")
            })?);
        }
        if let Some(text) = answer.free_form_text.as_deref() {
            if text.len() > MAX_TEXT_BYTES || text.contains('\0') {
                return Err(ChatError::validation(
                    "answers",
                    "Claude free-form answer exceeds the supported limit",
                ));
            }
            if !text.trim().is_empty() {
                values.push(text.to_string());
            }
        }
        if values.is_empty() {
            return Err(ChatError::validation(
                "answers",
                "Claude requires an answer for every question",
            ));
        }
        let provider_question = provider_question_labels
            .get(&answer.question_id)
            .ok_or_else(|| {
                ChatError::validation("answers", "Claude question ID is no longer pending")
            })?;
        answers.insert(provider_question.clone(), Value::String(values.join(", ")));
    }
    if answers.len() != option_labels.len() {
        return Err(ChatError::validation(
            "answers",
            "Claude requires an answer for every question",
        ));
    }
    Ok(json!({
        "behavior": "allow",
        "updatedInput": {
            "questions": questions,
            "answers": answers,
        },
    }))
}

fn remove_exact(
    pending: &PendingClaudeRequests,
    provider_request_id: &ProviderRequestId,
) -> ChatResult<PendingClaudeRequest> {
    pending
        .lock()
        .map_err(|_| state_error())?
        .remove(provider_request_id)
        .ok_or_else(|| ChatError::invalid_transition("Claude request is stale or unknown"))
}

fn restore_pending(
    pending: &PendingClaudeRequests,
    request: PendingClaudeRequest,
) -> ChatResult<()> {
    pending
        .lock()
        .map_err(|_| state_error())?
        .insert(request.provider_request_id.clone(), request);
    Ok(())
}

fn approval_option(
    id: &str,
    label: &str,
    decision_kind: ApprovalDecisionKind,
) -> ApprovalDecisionOption {
    ApprovalDecisionOption {
        id: id.to_string(),
        label: label.to_string(),
        decision_kind,
        description: None,
    }
}

fn classify_request(tool_name: &str) -> CanonicalRequestKind {
    match tool_name {
        "Bash" => CanonicalRequestKind::CommandExecution,
        "Read" | "Glob" | "Grep" => CanonicalRequestKind::FileRead,
        "Edit" | "Write" | "NotebookEdit" => CanonicalRequestKind::FileChange,
        "WebFetch" | "WebSearch" => CanonicalRequestKind::DynamicTool,
        name if name.starts_with("mcp__") => CanonicalRequestKind::DynamicTool,
        _ => CanonicalRequestKind::ToolInput,
    }
}

fn extract_plan_markdown(input: &Value) -> Option<String> {
    let object = input.as_object()?;
    ["plan", "planMarkdown", "markdown"]
        .into_iter()
        .find_map(|key| object.get(key).and_then(Value::as_str))
        .map(|value| bounded_text(value, MAX_TEXT_BYTES))
}

fn required_text<'a>(object: &'a Map<String, Value>, key: &str) -> ChatResult<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| protocol_error(key))
}

fn bounded_keys(object: &Map<String, Value>) -> Vec<String> {
    object.keys().take(32).cloned().collect()
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Claude pending request state is unavailable",
        false,
    )
}
