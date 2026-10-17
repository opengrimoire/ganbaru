//! Live Codex session routing, approvals, and structured input.

use super::normalizer::{CodexEventNormalizer, CodexRouteState};
use super::transport::{CodexInboundMessage, CodexRpcClient};
use crate::events::*;
use crate::models::*;
use crate::{DriverOperationContext, ProviderEventSink};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

const MAX_REQUEST_TEXT_BYTES: usize = 64 * 1024;
const MAX_QUESTIONS: usize = 16;
const MAX_OPTIONS_PER_QUESTION: usize = 32;

#[derive(Clone, Debug)]
pub enum PendingCodexRequestKind {
    Approval {
        allowed_provider_decisions: Vec<String>,
        response: CodexApprovalResponse,
    },
    UserInput {
        option_labels: HashMap<String, HashMap<String, String>>,
        secret_question_ids: BTreeSet<String>,
    },
}

#[derive(Clone, Debug)]
pub enum CodexApprovalResponse {
    Decision,
    Permissions { requested: Value },
}

#[derive(Clone, Debug)]
pub struct PendingCodexRequest {
    pub rpc_id: Value,
    pub provider_request_id: ProviderRequestId,
    pub chat_turn_id: Option<ChatTurnId>,
    pub provider_turn_id: Option<ProviderTurnId>,
    pub provider_item_id: Option<ProviderItemId>,
    pub kind: PendingCodexRequestKind,
}

#[derive(Clone, Debug)]
struct CodexRequestProvenance {
    chat_turn_id: Option<ChatTurnId>,
    provider_turn_id: Option<ProviderTurnId>,
    provider_item_id: Option<ProviderItemId>,
}

pub type PendingCodexRequests = Arc<Mutex<HashMap<ProviderRequestId, PendingCodexRequest>>>;

pub struct CodexRouterResources {
    pub client: CodexRpcClient,
    pub inbound: mpsc::Receiver<CodexInboundMessage>,
    pub normalizer: Arc<CodexEventNormalizer>,
    pub route: Arc<Mutex<CodexRouteState>>,
    pub pending_requests: PendingCodexRequests,
    pub sink: Arc<dyn ProviderEventSink>,
    pub provider_thread_sender: watch::Sender<Option<String>>,
    pub expected_shutdown: Arc<AtomicBool>,
    pub terminal_error: Arc<Mutex<Option<ChatError>>>,
    pub organizational: bool,
}

pub fn spawn_codex_router(resources: CodexRouterResources) -> JoinHandle<()> {
    tokio::spawn(run_codex_router(resources))
}

async fn run_codex_router(mut resources: CodexRouterResources) {
    while let Some(message) = resources.inbound.recv().await {
        let result = match message {
            CodexInboundMessage::Notification { method, params } => {
                route_notification(&resources, &method, params).await
            }
            CodexInboundMessage::Request { id, method, params } => {
                route_server_request(&resources, id, &method, params).await
            }
            CodexInboundMessage::Malformed {
                reason,
                byte_length,
            } => {
                let event = resources
                    .route
                    .lock()
                    .map_err(|_| router_state_error())
                    .and_then(|state| {
                        resources
                            .normalizer
                            .malformed_event(&state, &reason, byte_length)
                    });
                emit_events(&resources.sink, event.map(|event| vec![event])).await
            }
            CodexInboundMessage::Closed => {
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

async fn route_notification(
    resources: &CodexRouterResources,
    method: &str,
    params: Value,
) -> ChatResult<()> {
    if method == "serverRequest/resolved" {
        resolve_provider_cleared_request(resources, &params).await?;
    }
    let (events, provider_thread) = {
        let mut state = resources.route.lock().map_err(|_| router_state_error())?;
        let events = resources
            .normalizer
            .normalize_notification(&mut state, method, params)?;
        (events, state.provider_thread_id.clone())
    };
    if provider_thread.is_some() {
        let _ = resources.provider_thread_sender.send(provider_thread);
    }
    emit_events(&resources.sink, Ok(events)).await
}

async fn route_server_request(
    resources: &CodexRouterResources,
    rpc_id: Value,
    method: &str,
    params: Value,
) -> ChatResult<()> {
    let provider_request_id = provider_request_id(&rpc_id)?;
    let object = params
        .as_object()
        .ok_or_else(|| protocol_error("server request params"))?;
    let route = resources
        .route
        .lock()
        .map_err(|_| router_state_error())?
        .clone();
    let provenance = CodexRequestProvenance {
        chat_turn_id: route.active_chat_turn_id.clone(),
        provider_turn_id: text(object, "turnId")
            .and_then(|value| ProviderTurnId::new(value.to_string()).ok()),
        provider_item_id: text(object, "itemId")
            .and_then(|value| ProviderItemId::new(value.to_string()).ok()),
    };
    if resources.organizational
        && deny_organizational_authority_escalation(
            resources,
            rpc_id.clone(),
            method,
            object,
            &route,
            &provenance,
        )
        .await?
    {
        return Ok(());
    }
    let (pending_kind, event) = match method {
        "item/commandExecution/requestApproval" => {
            let title = text(object, "command")
                .map(|value| bounded_text(value, MAX_REQUEST_TEXT_BYTES))
                .unwrap_or_else(|| "Run command".to_string());
            let detail =
                text(object, "reason").map(|value| bounded_text(value, MAX_REQUEST_TEXT_BYTES));
            approval_request(
                &provider_request_id,
                CanonicalRequestKind::CommandExecution,
                title,
                detail,
                CodexApprovalResponse::Decision,
                json!({
                    "hasCommandActions": object.get("commandActions").is_some(),
                    "hasNetworkContext": object.get("networkApprovalContext").is_some(),
                }),
            )
        }
        "item/fileChange/requestApproval" => approval_request(
            &provider_request_id,
            CanonicalRequestKind::FileChange,
            "Apply file changes".to_string(),
            text(object, "reason").map(|value| bounded_text(value, MAX_REQUEST_TEXT_BYTES)),
            CodexApprovalResponse::Decision,
            json!({ "requestsAdditionalRoot": object.get("grantRoot").is_some() }),
        ),
        "item/permissions/requestApproval" => {
            let requested = object
                .get("permissions")
                .filter(|value| value.is_object())
                .cloned()
                .ok_or_else(|| protocol_error("permissions"))?;
            approval_request(
                &provider_request_id,
                CanonicalRequestKind::ToolInput,
                "Grant requested permissions".to_string(),
                text(object, "reason").map(|value| bounded_text(value, MAX_REQUEST_TEXT_BYTES)),
                CodexApprovalResponse::Permissions {
                    requested: requested.clone(),
                },
                permission_summary(&requested),
            )
        }
        "item/tool/requestUserInput" => structured_input_request(&provider_request_id, object)?,
        _ => {
            resources
                .client
                .respond_error(rpc_id, -32601, "Unsupported Codex server request")
                .await
                .map_err(|error| error.to_chat_error("server request response"))?;
            let unknown = resources.normalizer.request_event(
                &route,
                provider_request_id,
                provenance.provider_turn_id,
                provenance.provider_item_id,
                provenance.chat_turn_id,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: method.to_string(),
                    summary: "Codex requested an unsupported client operation".to_string(),
                    safe_payload: Some(VersionedJson {
                        schema_version: 1,
                        value: json!({ "keys": bounded_keys(object) }),
                    }),
                }),
            )?;
            return resources.sink.emit(unknown).await;
        }
    };
    let pending = PendingCodexRequest {
        rpc_id,
        provider_request_id: provider_request_id.clone(),
        chat_turn_id: provenance.chat_turn_id.clone(),
        provider_turn_id: provenance.provider_turn_id.clone(),
        provider_item_id: provenance.provider_item_id.clone(),
        kind: pending_kind,
    };
    {
        let mut requests = resources
            .pending_requests
            .lock()
            .map_err(|_| router_state_error())?;
        if requests.contains_key(&provider_request_id) {
            return Err(protocol_error("duplicate provider request ID"));
        }
        requests.insert(provider_request_id.clone(), pending);
    }
    let canonical = resources.normalizer.request_event(
        &route,
        provider_request_id.clone(),
        provenance.provider_turn_id,
        provenance.provider_item_id,
        provenance.chat_turn_id,
        event,
    )?;
    if let Err(error) = resources.sink.emit(canonical).await {
        if let Ok(mut requests) = resources.pending_requests.lock() {
            requests.remove(&provider_request_id);
        }
        return Err(error);
    }
    if let Ok(mut state) = resources.route.lock() {
        state.session_state = match method {
            "item/tool/requestUserInput" => ProviderSessionState::WaitingForUserInput,
            _ => ProviderSessionState::WaitingForApproval,
        };
    }
    Ok(())
}

async fn deny_organizational_authority_escalation(
    resources: &CodexRouterResources,
    rpc_id: Value,
    method: &str,
    object: &Map<String, Value>,
    route: &CodexRouteState,
    provenance: &CodexRequestProvenance,
) -> ChatResult<bool> {
    let response = match method {
        "item/permissions/requestApproval" => Some(json!({
            "permissions": {},
            "scope": "turn",
        })),
        "item/fileChange/requestApproval"
            if object
                .get("grantRoot")
                .is_some_and(|value| !value.is_null()) =>
        {
            Some(json!({ "decision": "decline" }))
        }
        "item/commandExecution/requestApproval"
            if object
                .get("additionalPermissions")
                .is_some_and(|value| !value.is_null())
                || object
                    .get("networkApprovalContext")
                    .is_some_and(|value| !value.is_null())
                || object
                    .get("proposedExecpolicyAmendment")
                    .is_some_and(|value| !value.is_null())
                || object
                    .get("proposedNetworkPolicyAmendments")
                    .is_some_and(|value| !value.is_null()) =>
        {
            Some(json!({ "decision": "decline" }))
        }
        _ => None,
    };
    let Some(response) = response else {
        return Ok(false);
    };
    resources
        .client
        .respond(rpc_id, response)
        .await
        .map_err(|error| error.to_chat_error("organizational authority denial"))?;
    resources
        .sink
        .emit(resources.normalizer.event(
            route,
            "organizational/authority-denied",
            provenance.chat_turn_id.clone(),
            provenance.provider_turn_id.clone(),
            provenance.provider_item_id.clone(),
            CanonicalEvent::RuntimeWarning(NotificationEvent {
                code: "codex_organizational_authority_escalation_denied".to_string(),
                title: "Codex authority expansion was denied".to_string(),
                detail: Some(
                    "The organizational assignment keeps its original channel, folder, network, and runtime boundaries."
                        .to_string(),
                ),
            }),
        )?)
        .await?;
    Ok(true)
}

fn approval_request(
    request_id: &ProviderRequestId,
    kind: CanonicalRequestKind,
    title: String,
    detail: Option<String>,
    response: CodexApprovalResponse,
    safe_payload: Value,
) -> (PendingCodexRequestKind, CanonicalEvent) {
    let decisions = vec![
        ("accept", "Allow once", ApprovalDecisionKind::AllowOnce),
        (
            "acceptForSession",
            "Allow for session",
            ApprovalDecisionKind::AllowSession,
        ),
        ("decline", "Deny", ApprovalDecisionKind::Deny),
        ("cancel", "Cancel turn", ApprovalDecisionKind::Cancel),
    ];
    let allowed_provider_decisions = decisions
        .iter()
        .map(|(id, _, _)| (*id).to_string())
        .collect();
    let allowed_decisions = decisions
        .into_iter()
        .map(|(id, label, decision_kind)| ApprovalDecisionOption {
            id: id.to_string(),
            label: label.to_string(),
            decision_kind,
            description: None,
        })
        .collect();
    (
        PendingCodexRequestKind::Approval {
            allowed_provider_decisions,
            response,
        },
        CanonicalEvent::RequestOpened(RequestOpenedEvent {
            request_id: request_id.clone(),
            kind,
            title,
            detail,
            allowed_decisions,
            safe_payload: VersionedJson {
                schema_version: 1,
                value: safe_payload,
            },
        }),
    )
}

fn structured_input_request(
    request_id: &ProviderRequestId,
    object: &Map<String, Value>,
) -> ChatResult<(PendingCodexRequestKind, CanonicalEvent)> {
    let questions = object
        .get("questions")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_error("request_user_input questions"))?;
    if questions.is_empty() || questions.len() > MAX_QUESTIONS {
        return Err(protocol_error("request_user_input question count"));
    }
    let mut option_labels = HashMap::new();
    let mut secret_question_ids = BTreeSet::new();
    let mut canonical_questions = Vec::new();
    for (question_index, question) in questions.iter().enumerate() {
        let question = question
            .as_object()
            .ok_or_else(|| protocol_error("request_user_input question"))?;
        let id = required_text(question, "id")?.to_string();
        let options = question
            .get("options")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if options.len() > MAX_OPTIONS_PER_QUESTION {
            return Err(protocol_error("request_user_input option count"));
        }
        let mut labels = HashMap::new();
        let mut canonical_options = Vec::new();
        for (option_index, option) in options.iter().enumerate() {
            let option = option
                .as_object()
                .ok_or_else(|| protocol_error("request_user_input option"))?;
            let label = bounded_text(required_text(option, "label")?, MAX_REQUEST_TEXT_BYTES);
            let option_id = format!("q{question_index}-option-{option_index}");
            labels.insert(option_id.clone(), label.clone());
            canonical_options.push(UserInputOption {
                id: option_id,
                label,
                description: text(option, "description")
                    .map(|value| bounded_text(value, MAX_REQUEST_TEXT_BYTES)),
            });
        }
        option_labels.insert(id.clone(), labels);
        if question
            .get("isSecret")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            secret_question_ids.insert(id.clone());
        }
        let free_form_allowed = options.is_empty()
            || question
                .get("isOther")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        canonical_questions.push(UserInputQuestion {
            id,
            header: text(question, "header")
                .map(|value| bounded_text(value, MAX_REQUEST_TEXT_BYTES)),
            question: bounded_text(required_text(question, "question")?, MAX_REQUEST_TEXT_BYTES),
            options: canonical_options,
            multiple: false,
            free_form_allowed,
            required: true,
        });
    }
    Ok((
        PendingCodexRequestKind::UserInput {
            option_labels,
            secret_question_ids,
        },
        CanonicalEvent::UserInputRequested(UserInputRequestedEvent {
            request_id: request_id.clone(),
            questions: canonical_questions,
        }),
    ))
}

pub async fn resolve_codex_approval(
    client: &CodexRpcClient,
    pending_requests: &PendingCodexRequests,
    normalizer: &CodexEventNormalizer,
    route: &Arc<Mutex<CodexRouteState>>,
    sink: &Arc<dyn ProviderEventSink>,
    request: &ResolveApprovalRequest,
    context: &DriverOperationContext,
) -> ChatResult<()> {
    let pending = take_pending(pending_requests, &request.provider_request_id)?;
    let PendingCodexRequestKind::Approval {
        allowed_provider_decisions,
        response,
    } = &pending.kind
    else {
        restore_pending(pending_requests, pending)?;
        return Err(ChatError::invalid_transition(
            "Codex request expects structured user input",
        ));
    };
    let provider_decision = approval_decision(&request.decision);
    if !allowed_provider_decisions
        .iter()
        .any(|allowed| allowed == provider_decision)
    {
        restore_pending(pending_requests, pending)?;
        return Err(ChatError::validation(
            "decision",
            "Codex did not offer this approval decision",
        ));
    }
    let response_payload = match response {
        CodexApprovalResponse::Decision => json!({ "decision": provider_decision }),
        CodexApprovalResponse::Permissions { requested } => {
            let permissions = match request.decision.kind {
                ApprovalDecisionKind::AllowOnce | ApprovalDecisionKind::AllowSession => {
                    requested.clone()
                }
                ApprovalDecisionKind::Deny | ApprovalDecisionKind::Cancel => json!({}),
            };
            json!({
                "permissions": permissions,
                "scope": if request.decision.kind == ApprovalDecisionKind::AllowSession {
                    "session"
                } else {
                    "turn"
                },
            })
        }
    };
    if let Err(error) = client
        .respond(pending.rpc_id.clone(), response_payload)
        .await
    {
        restore_pending(pending_requests, pending)?;
        return Err(error.to_chat_error("approval response"));
    }
    if request.decision.kind == ApprovalDecisionKind::Cancel {
        let state = route.lock().map_err(|_| router_state_error())?.clone();
        if let (Some(thread_id), Some(turn_id)) =
            (state.provider_thread_id, state.active_provider_turn_id)
        {
            client
                .request(
                    "turn/interrupt",
                    json!({ "threadId": thread_id, "turnId": turn_id }),
                    context,
                )
                .await
                .map_err(|error| error.to_chat_error("approval cancel"))?;
        }
    }
    let state = route.lock().map_err(|_| router_state_error())?.clone();
    let event = normalizer.request_event(
        &state,
        pending.provider_request_id,
        pending.provider_turn_id,
        pending.provider_item_id,
        pending.chat_turn_id,
        CanonicalEvent::RequestResolved(RequestResolvedEvent {
            request_id: request.provider_request_id.clone(),
            state: RequestResolutionState::Resolved,
            decision: Some(request.decision.clone()),
        }),
    )?;
    sink.emit(event).await?;
    if let Ok(mut state) = route.lock() {
        state.session_state = ProviderSessionState::Active;
    }
    Ok(())
}

pub async fn resolve_codex_user_input(
    client: &CodexRpcClient,
    pending_requests: &PendingCodexRequests,
    normalizer: &CodexEventNormalizer,
    route: &Arc<Mutex<CodexRouteState>>,
    sink: &Arc<dyn ProviderEventSink>,
    request: &ResolveUserInputRequest,
) -> ChatResult<()> {
    let pending = take_pending(pending_requests, &request.provider_request_id)?;
    let PendingCodexRequestKind::UserInput {
        option_labels,
        secret_question_ids,
    } = &pending.kind
    else {
        restore_pending(pending_requests, pending)?;
        return Err(ChatError::invalid_transition(
            "Codex request expects an approval decision",
        ));
    };
    let answers = match build_user_input_answers(&request.answers, option_labels) {
        Ok(answers) => answers,
        Err(error) => {
            restore_pending(pending_requests, pending)?;
            return Err(error);
        }
    };
    if let Err(error) = client
        .respond(pending.rpc_id.clone(), json!({ "answers": answers }))
        .await
    {
        restore_pending(pending_requests, pending)?;
        return Err(error.to_chat_error("user input response"));
    }
    let state = route.lock().map_err(|_| router_state_error())?.clone();
    let event = normalizer.request_event(
        &state,
        pending.provider_request_id,
        pending.provider_turn_id,
        pending.provider_item_id,
        pending.chat_turn_id,
        CanonicalEvent::UserInputResolved(UserInputResolvedEvent {
            request_id: request.provider_request_id.clone(),
            state: RequestResolutionState::Resolved,
            answers: redacted_answers(&request.answers, secret_question_ids),
        }),
    )?;
    sink.emit(event).await?;
    if let Ok(mut state) = route.lock() {
        state.session_state = ProviderSessionState::Active;
    }
    Ok(())
}

async fn resolve_provider_cleared_request(
    resources: &CodexRouterResources,
    params: &Value,
) -> ChatResult<()> {
    let object = params
        .as_object()
        .ok_or_else(|| protocol_error("serverRequest/resolved"))?;
    let Some(raw_request_id) = object.get("requestId") else {
        return Ok(());
    };
    let provider_request_id = provider_request_id(raw_request_id)?;
    let pending = resources
        .pending_requests
        .lock()
        .map_err(|_| router_state_error())?
        .remove(&provider_request_id);
    let Some(pending) = pending else {
        return Ok(());
    };
    let state = resources
        .route
        .lock()
        .map_err(|_| router_state_error())?
        .clone();
    let event = match pending.kind {
        PendingCodexRequestKind::Approval { .. } => {
            CanonicalEvent::RequestResolved(RequestResolvedEvent {
                request_id: provider_request_id.clone(),
                state: RequestResolutionState::Interrupted,
                decision: None,
            })
        }
        PendingCodexRequestKind::UserInput { .. } => {
            CanonicalEvent::UserInputResolved(UserInputResolvedEvent {
                request_id: provider_request_id.clone(),
                state: RequestResolutionState::Interrupted,
                answers: Vec::new(),
            })
        }
    };
    let event = resources.normalizer.request_event(
        &state,
        provider_request_id,
        pending.provider_turn_id,
        pending.provider_item_id,
        pending.chat_turn_id,
        event,
    )?;
    resources.sink.emit(event).await
}

async fn route_closed(resources: &CodexRouterResources) {
    let expected = resources.expected_shutdown.load(Ordering::Acquire);
    let event = resources
        .route
        .lock()
        .map_err(|_| router_state_error())
        .and_then(|mut state| {
            let previous = state.session_state;
            state.session_state = if expected {
                ProviderSessionState::Stopped
            } else {
                ProviderSessionState::Failed
            };
            resources.normalizer.event(
                &state,
                "transport/closed",
                None,
                None,
                None,
                CanonicalEvent::SessionStateChanged(SessionStateChangedEvent {
                    session_id: resources.normalizer.session_id(),
                    previous_state: previous,
                    state: state.session_state,
                    reason: Some(if expected {
                        "Codex session stopped".to_string()
                    } else {
                        "Codex app-server stream closed".to_string()
                    }),
                }),
            )
        });
    let result = emit_events(&resources.sink, event.map(|event| vec![event])).await;
    if !expected {
        let error = result.err().unwrap_or_else(|| {
            ChatError::new(
                ChatErrorCode::TransportUnavailable,
                "Codex app-server stream closed unexpectedly",
                true,
            )
        });
        if let Ok(mut terminal) = resources.terminal_error.lock() {
            *terminal = Some(error);
        }
    }
}

fn take_pending(
    requests: &PendingCodexRequests,
    request_id: &ProviderRequestId,
) -> ChatResult<PendingCodexRequest> {
    requests
        .lock()
        .map_err(|_| router_state_error())?
        .remove(request_id)
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::NotFound,
                "Codex provider request is no longer pending",
                true,
            )
        })
}

fn restore_pending(
    requests: &PendingCodexRequests,
    pending: PendingCodexRequest,
) -> ChatResult<()> {
    requests
        .lock()
        .map_err(|_| router_state_error())?
        .insert(pending.provider_request_id.clone(), pending);
    Ok(())
}

fn approval_decision(decision: &ApprovalDecision) -> &'static str {
    match decision.kind {
        ApprovalDecisionKind::AllowOnce => "accept",
        ApprovalDecisionKind::AllowSession => "acceptForSession",
        ApprovalDecisionKind::Deny => "decline",
        ApprovalDecisionKind::Cancel => "cancel",
    }
}

fn build_user_input_answers(
    answers: &[UserInputAnswer],
    option_labels: &HashMap<String, HashMap<String, String>>,
) -> ChatResult<Map<String, Value>> {
    let mut output = Map::new();
    for answer in answers {
        let labels = option_labels
            .get(&answer.question_id)
            .ok_or_else(|| ChatError::validation("answers", "Codex question ID is not pending"))?;
        let mut values = Vec::new();
        for option_id in &answer.selected_option_ids {
            let label = labels
                .get(option_id)
                .ok_or_else(|| ChatError::validation("answers", "Codex option was not offered"))?;
            values.push(Value::String(label.clone()));
        }
        if let Some(free_form) = answer.free_form_text.as_deref() {
            if free_form.len() > MAX_REQUEST_TEXT_BYTES || free_form.contains('\0') {
                return Err(ChatError::validation(
                    "answers",
                    "Codex free-form answer exceeds the supported limit",
                ));
            }
            if !free_form.is_empty() {
                values.push(Value::String(free_form.to_string()));
            }
        }
        if values.is_empty() {
            return Err(ChatError::validation(
                "answers",
                "Codex question requires an answer",
            ));
        }
        output.insert(answer.question_id.clone(), json!({ "answers": values }));
    }
    if output.len() != option_labels.len() {
        return Err(ChatError::validation(
            "answers",
            "Every Codex question requires an answer",
        ));
    }
    Ok(output)
}

fn redacted_answers(
    answers: &[UserInputAnswer],
    secret_question_ids: &BTreeSet<String>,
) -> Vec<UserInputAnswer> {
    answers
        .iter()
        .map(|answer| UserInputAnswer {
            question_id: answer.question_id.clone(),
            selected_option_ids: answer.selected_option_ids.clone(),
            free_form_text: if secret_question_ids.contains(&answer.question_id) {
                None
            } else {
                answer.free_form_text.clone()
            },
        })
        .collect()
}

fn permission_summary(requested: &Value) -> Value {
    let file_system = requested.get("fileSystem").and_then(Value::as_object);
    let count = |key: &str| {
        file_system
            .and_then(|value| value.get(key))
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    };
    json!({
        "permissionRequest": true,
        "fileSystemEntries": count("entries"),
        "legacyReadPaths": count("read"),
        "legacyWritePaths": count("write"),
        "networkRequested": requested
            .pointer("/network/enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn provider_request_id(id: &Value) -> ChatResult<ProviderRequestId> {
    let serialized = serde_json::to_vec(id).map_err(|_| protocol_error("request ID"))?;
    let mut digest = Sha256::new();
    digest.update(b"ganbaru-codex-rpc-request-v1\0");
    digest.update(serialized);
    ProviderRequestId::new(format!("codex-request-{:x}", digest.finalize()))
        .map_err(|_| protocol_error("request ID"))
}

async fn emit_events(
    sink: &Arc<dyn ProviderEventSink>,
    events: ChatResult<Vec<CanonicalRuntimeEvent>>,
) -> ChatResult<()> {
    for event in events? {
        sink.emit(event).await?;
    }
    Ok(())
}

fn required_text<'a>(object: &'a Map<String, Value>, key: &str) -> ChatResult<&'a str> {
    text(object, key).ok_or_else(|| protocol_error(key))
}

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn bounded_keys(object: &Map<String, Value>) -> Vec<String> {
    object.keys().take(32).cloned().collect()
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_string()
}

fn router_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Codex session router state is unavailable",
        false,
    )
}

fn protocol_error(field: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        format!("Codex server request field '{field}' is invalid"),
        true,
    )
}
