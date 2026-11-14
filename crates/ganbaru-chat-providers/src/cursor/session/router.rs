use super::callbacks::*;
use super::*;

pub fn spawn_cursor_router(resources: CursorRouterResources) -> JoinHandle<()> {
    tokio::spawn(async move {
        route_inbound(resources).await;
    })
}

async fn route_inbound(mut resources: CursorRouterResources) {
    let git_metadata_tripped = resources.terminal_callbacks.git_metadata_tripped();
    let mut ended_by_guard = false;
    loop {
        let message = tokio::select! {
            message = resources.inbound.recv() => message,
            () = wait_for_trip(git_metadata_tripped.as_deref()) => {
                end_after_git_metadata_change(&resources).await;
                ended_by_guard = true;
                break;
            }
        };
        let Some(message) = message else { break };
        let result = match message {
            AcpInboundMessage::Notification { method, params } => {
                handle_notification(&resources, &method, params).await
            }
            AcpInboundMessage::Request { id, method, params } => {
                handle_request(&resources, id, &method, params).await
            }
            AcpInboundMessage::Malformed { reason, .. } => {
                Err(protocol_error(&format!("JSON-RPC message: {reason}")))
            }
            AcpInboundMessage::Closed => break,
        };
        if let Err(error) = result {
            set_terminal_error(&resources.terminal_error, error);
            break;
        }
    }
    resources.terminal_callbacks.shutdown();
    if !ended_by_guard && !resources.expected_shutdown.load(Ordering::Acquire) {
        if reached_memory_limit(resources.memory_limit.as_ref()).await {
            let warning = resources
                .route
                .lock()
                .map_err(|_| driver_state_error())
                .and_then(|state| {
                    resources.normalizer.external_event(
                        &state,
                        "transport/memory_limit",
                        None,
                        CanonicalEvent::RuntimeWarning(memory_limit_warning()),
                    )
                });
            if let Ok(warning) = warning {
                emit_all(&resources.sink, vec![warning]).await;
            }
        }
        let provider_name = resources.flavor.display_name();
        let events = resources
            .route
            .lock()
            .map_err(|_| driver_state_error())
            .and_then(|mut state| {
                resources
                    .normalizer
                    .interrupted(&mut state, &format!("{provider_name} ACP transport closed"))
            })
            .unwrap_or_default();
        emit_all(&resources.sink, events).await;
        set_terminal_error(
            &resources.terminal_error,
            ChatError::new(
                ChatErrorCode::TransportUnavailable,
                format!("{provider_name} ACP transport closed unexpectedly"),
                true,
            ),
        );
    }
}

/// Waits until a confined command changes the Git metadata of the folder; never returns for a
/// session without the guard.
async fn wait_for_trip(tripped: Option<&Notify>) {
    match tripped {
        Some(tripped) => tripped.notified().await,
        None => std::future::pending().await,
    }
}

/// Ends the session after a confined command changed Git metadata: the turn is interrupted with
/// the reason, a warning records the change, and later operations fail with the same error.
async fn end_after_git_metadata_change(resources: &CursorRouterResources) {
    let error = crate::confinement::git_metadata_changed_error();
    set_terminal_error(&resources.terminal_error, error.clone());
    let events = resources
        .route
        .lock()
        .map_err(|_| driver_state_error())
        .and_then(|mut state| {
            let warning = resources.normalizer.external_event(
                &state,
                "session/git_metadata_changed",
                None,
                CanonicalEvent::RuntimeWarning(NotificationEvent {
                    code: crate::confinement::GIT_METADATA_CHANGED_CODE.to_string(),
                    title: "A teammate changed this folder's Git or Cursor configuration"
                        .to_string(),
                    detail: Some(error.message.clone()),
                }),
            )?;
            let mut events = vec![warning];
            events.extend(
                resources
                    .normalizer
                    .interrupted(&mut state, &error.message)?,
            );
            Ok(events)
        })
        .unwrap_or_default();
    emit_all(&resources.sink, events).await;
}

async fn handle_notification(
    resources: &CursorRouterResources,
    method: &str,
    params: Value,
) -> ChatResult<()> {
    if resources.flavor == AcpProviderFlavor::Grok
        && matches!(
            method,
            "x.ai/session/prompt_complete" | "_x.ai/session/prompt_complete"
        )
    {
        return handle_grok_prompt_completion(resources, params);
    }
    if method == "session/update" && finishes_tool_call(&params) {
        // A change trips the guard, whose signal ends the session in `route_inbound`.
        let _ = resources.terminal_callbacks.verify_direct_writes();
    }
    let events = {
        let mut state = resources.route.lock().map_err(|_| driver_state_error())?;
        match method {
            "session/update" => resources
                .normalizer
                .normalize_session_update(&mut state, params)?,
            "cursor/update_todos" if resources.flavor.offers_structured_extensions() => {
                vec![resources.normalizer.normalize_todos(&state, &params)?]
            }
            _ => vec![resources.normalizer.external_event(
                &state,
                method,
                None,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: format!("acp/notification/{method}"),
                    summary: format!(
                        "{} emitted an unsupported ACP notification",
                        resources.flavor.display_name()
                    ),
                    safe_payload: Some(safe_shape(&params)),
                }),
            )?],
        }
    };
    for event in events {
        resources.sink.emit(event).await?;
    }
    Ok(())
}

/// Whether a `session/update` reports a tool call that completed or failed, after which an
/// agent that writes the folder itself may have changed it.
fn finishes_tool_call(params: &Value) -> bool {
    let Some(update) = params.get("update") else {
        return false;
    };
    matches!(
        update.get("sessionUpdate").and_then(Value::as_str),
        Some("tool_call" | "tool_call_update")
    ) && matches!(
        update.get("status").and_then(Value::as_str),
        Some("completed" | "failed")
    )
}

fn handle_grok_prompt_completion(
    resources: &CursorRouterResources,
    params: Value,
) -> ChatResult<()> {
    let object = params
        .as_object()
        .ok_or_else(|| protocol_error("Grok prompt completion"))?;
    let session_id = object
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| is_valid_identifier(value, 512))
        .ok_or_else(|| protocol_error("Grok prompt completion session ID"))?;
    let expected_session = resources
        .route
        .lock()
        .map_err(|_| driver_state_error())?
        .provider_thread_id
        .clone();
    if session_id != expected_session {
        return Err(protocol_error("Grok prompt completion session ID"));
    }
    let prompt_id = object
        .get("promptId")
        .and_then(Value::as_str)
        .filter(|value| is_valid_identifier(value, 512));
    let stop_reason = object
        .get("stopReason")
        .and_then(Value::as_str)
        .filter(|value| is_valid_identifier(value, 128))
        .unwrap_or("unknown");
    let sender = {
        let mut pending = resources
            .prompt_completions
            .lock()
            .map_err(|_| driver_state_error())?;
        if let Some(prompt_id) = prompt_id {
            pending.remove(prompt_id)
        } else if pending.len() == 1 {
            let only_id = pending.keys().next().cloned();
            only_id.and_then(|id| pending.remove(&id))
        } else if pending.is_empty() {
            None
        } else {
            return Err(protocol_error("ambiguous Grok prompt completion"));
        }
    };
    if let Some(sender) = sender {
        let _ = sender.send(json!({ "stopReason": stop_reason }));
    }
    Ok(())
}

async fn handle_request(
    resources: &CursorRouterResources,
    rpc_id: Value,
    method: &str,
    params: Value,
) -> ChatResult<()> {
    match method {
        "session/request_permission" => handle_permission(resources, rpc_id, params).await,
        "fs/read_text_file" => handle_read_text_file(resources, rpc_id, params).await,
        "fs/write_text_file" => handle_write_text_file(resources, rpc_id, params).await,
        "terminal/create" if holds_terminals_for_the_person(resources)? => {
            hold_terminal_create(resources, rpc_id, params).await
        }
        "terminal/create" => handle_terminal_create(resources, rpc_id, params).await,
        "terminal/output" => handle_terminal_output(resources, rpc_id, params).await,
        "terminal/wait_for_exit" => handle_terminal_wait(resources, rpc_id, params).await,
        "terminal/kill" => handle_terminal_kill(resources, rpc_id, params).await,
        "terminal/release" => handle_terminal_release(resources, rpc_id, params).await,
        "cursor/ask_question" if resources.flavor.offers_structured_extensions() => {
            handle_question(resources, rpc_id, params).await
        }
        "x.ai/ask_user_question" | "_x.ai/ask_user_question"
            if resources.flavor == AcpProviderFlavor::Grok =>
        {
            handle_xai_question(resources, rpc_id, params).await
        }
        "cursor/create_plan" if resources.flavor.offers_structured_extensions() => {
            let event = {
                let state = resources.route.lock().map_err(|_| driver_state_error())?;
                resources
                    .normalizer
                    .normalize_create_plan(&state, &params)?
            };
            resources.sink.emit(event).await?;
            resources
                .client
                .respond(rpc_id, json!({ "accepted": true }))
                .await
                .map_err(|error| error.to_chat_error("plan response"))
        }
        _ => {
            let event = {
                let state = resources.route.lock().map_err(|_| driver_state_error())?;
                resources.normalizer.external_event(
                    &state,
                    method,
                    None,
                    CanonicalEvent::Unknown(UnknownEvent {
                        source_type: format!("acp/request/{method}"),
                        summary: format!(
                            "{} requested an unsupported ACP extension",
                            resources.flavor.display_name()
                        ),
                        safe_payload: Some(safe_shape(&params)),
                    }),
                )?
            };
            resources.sink.emit(event).await?;
            resources
                .client
                .respond_error(rpc_id, -32601, "Method not supported by Ganbaru")
                .await
                .map_err(|error| error.to_chat_error("extension rejection"))
        }
    }
}

async fn handle_xai_question(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let params_object = params
        .as_object()
        .ok_or_else(|| protocol_error("Grok question parameters"))?;
    let question_object = params_object
        .get("params")
        .and_then(Value::as_object)
        .unwrap_or(params_object);
    let question_session_id = question_object
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| protocol_error("Grok question session ID"))?;
    let expected_session_id = resources
        .route
        .lock()
        .map_err(|_| driver_state_error())?
        .provider_thread_id
        .clone();
    if question_session_id != expected_session_id {
        return Err(protocol_error("Grok question session ID"));
    }
    let (parsed, questions) = parse_xai_question(&params)?;
    let event = {
        let state = resources.route.lock().map_err(|_| driver_state_error())?;
        resources.normalizer.external_event(
            &state,
            "x.ai/ask_user_question",
            Some(parsed.provider_request_id.clone()),
            CanonicalEvent::UserInputRequested(parsed.request),
        )?
    };
    resources.sink.emit(event).await?;
    insert_pending(
        &resources.pending,
        parsed.provider_request_id,
        PendingCursorRequest {
            rpc_id,
            kind: PendingCursorRequestKind::XaiUserInput { questions },
        },
    )
}

async fn handle_permission(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let (parsed, safety_mode) = {
        let state = resources.route.lock().map_err(|_| driver_state_error())?;
        (
            parse_permission(&params, &state.workspace)?,
            state.modes.safety_mode,
        )
    };
    let opened = {
        let state = resources.route.lock().map_err(|_| driver_state_error())?;
        resources.normalizer.external_event(
            &state,
            "session/request_permission",
            Some(parsed.provider_request_id.clone()),
            CanonicalEvent::RequestOpened(parsed.request.clone()),
        )?
    };
    resources.sink.emit(opened).await?;
    if let Some(option) = auto_permission_option(&parsed, safety_mode) {
        let decision = ApprovalDecision {
            kind: if option.kind == "allow_always" {
                ApprovalDecisionKind::AllowSession
            } else {
                ApprovalDecisionKind::AllowOnce
            },
            provider_option_id: Some(option.option_id.clone()),
            updated_tool_input: None,
        };
        resources
            .client
            .respond(
                rpc_id,
                json!({
                    "outcome": { "outcome": "selected", "optionId": option.option_id }
                }),
            )
            .await
            .map_err(|error| error.to_chat_error("automatic permission response"))?;
        let resolved = {
            let state = resources.route.lock().map_err(|_| driver_state_error())?;
            resources.normalizer.external_event(
                &state,
                "session/request_permission/resolved",
                Some(parsed.provider_request_id.clone()),
                CanonicalEvent::RequestResolved(RequestResolvedEvent {
                    request_id: parsed.provider_request_id,
                    state: RequestResolutionState::Resolved,
                    decision: Some(decision),
                }),
            )?
        };
        resources.sink.emit(resolved).await?;
        return Ok(());
    }
    insert_pending(
        &resources.pending,
        parsed.provider_request_id,
        PendingCursorRequest {
            rpc_id,
            kind: PendingCursorRequestKind::Approval {
                options: parsed.options,
            },
        },
    )
}

/// Whether a confined terminal command waits for the person: in every safety mode but full
/// access, so the approval does not depend on the agent asking first.
fn holds_terminals_for_the_person(resources: &CursorRouterResources) -> ChatResult<bool> {
    if resources.terminal_callbacks.confinement.is_none() {
        return Ok(false);
    }
    let safety_mode = resources
        .route
        .lock()
        .map_err(|_| driver_state_error())?
        .modes
        .safety_mode;
    Ok(safety_mode != SafetyMode::FullAccess)
}

/// Opens an approval for a confined terminal command and holds the request until the person
/// decides; a command the session may not run is refused without asking.
async fn hold_terminal_create(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let safe_payload = safe_shape(&params);
    let request: CreateTerminalRequest = match serde_json::from_value(params) {
        Ok(request) => request,
        Err(_) => {
            return reject_callback(resources, rpc_id, "Invalid terminal create request").await;
        }
    };
    let admitted = resources
        .terminal_callbacks
        .verify_session(&request.session_id.to_string())
        .and_then(|()| validate_terminal_command(&request))
        .and_then(|()| resources.terminal_callbacks.verify_runs_commands());
    if let Err(error) = admitted {
        return reject_callback(resources, rpc_id, &error.message).await;
    }
    let request_id = ProviderRequestId::new(format!(
        "ganbaru-terminal-{}",
        NEXT_ACP_TERMINAL_APPROVAL.fetch_add(1, Ordering::Relaxed)
    ))
    .map_err(|_| protocol_error("terminal approval request ID"))?;
    let command_line = std::iter::once(request.command.as_str())
        .chain(request.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    let opened = {
        let state = resources.route.lock().map_err(|_| driver_state_error())?;
        resources.normalizer.external_event(
            &state,
            "terminal/create",
            Some(request_id.clone()),
            CanonicalEvent::RequestOpened(RequestOpenedEvent {
                request_id: request_id.clone(),
                kind: CanonicalRequestKind::CommandExecution,
                title: "Run a command in the working folder".to_string(),
                detail: Some(bounded_text(&command_line, MAX_PROTOCOL_TEXT_BYTES)),
                allowed_decisions: terminal_approval_decisions(),
                safe_payload,
                subject: None,
                session_grants: Vec::new(),
            }),
        )?
    };
    resources.sink.emit(opened).await?;
    insert_pending(
        &resources.pending,
        request_id,
        PendingCursorRequest {
            rpc_id,
            kind: PendingCursorRequestKind::Terminal {
                request: Box::new(request),
            },
        },
    )
}

async fn handle_question(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let parsed = parse_question(&params)?;
    let event = {
        let state = resources.route.lock().map_err(|_| driver_state_error())?;
        resources.normalizer.external_event(
            &state,
            "cursor/ask_question",
            Some(parsed.provider_request_id.clone()),
            CanonicalEvent::UserInputRequested(parsed.request),
        )?
    };
    resources.sink.emit(event).await?;
    insert_pending(
        &resources.pending,
        parsed.provider_request_id,
        PendingCursorRequest {
            rpc_id,
            kind: PendingCursorRequestKind::UserInput {
                questions: parsed.questions,
            },
        },
    )
}

pub fn take_pending(
    pending: &PendingCursorRequests,
    request_id: &ProviderRequestId,
) -> ChatResult<PendingCursorRequest> {
    pending
        .lock()
        .map_err(|_| driver_state_error())?
        .remove(request_id)
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Conflict,
                "ACP provider request is no longer pending",
                false,
            )
        })
}

pub fn restore_pending(
    pending: &PendingCursorRequests,
    request_id: ProviderRequestId,
    request: PendingCursorRequest,
) {
    if let Ok(mut pending) = pending.lock() {
        pending.insert(request_id, request);
    }
}

fn insert_pending(
    pending: &PendingCursorRequests,
    request_id: ProviderRequestId,
    request: PendingCursorRequest,
) -> ChatResult<()> {
    let mut pending = pending.lock().map_err(|_| driver_state_error())?;
    if pending.len() >= 64 || pending.contains_key(&request_id) {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "ACP provider emitted a duplicate or excessive pending request",
            false,
        ));
    }
    pending.insert(request_id, request);
    Ok(())
}

async fn emit_all(sink: &Arc<dyn ProviderEventSink>, events: Vec<CanonicalRuntimeEvent>) {
    for event in events {
        let _ = sink.emit(event).await;
    }
    let _ = sink.flush().await;
}

fn set_terminal_error(target: &Arc<Mutex<Option<ChatError>>>, error: ChatError) {
    if let Ok(mut target) = target.lock() {
        if target.is_none() {
            *target = Some(error);
        }
    }
}
