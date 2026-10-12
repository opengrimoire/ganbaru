use super::callbacks::*;
use super::*;

pub fn spawn_cursor_router(resources: CursorRouterResources) -> JoinHandle<()> {
    tokio::spawn(async move {
        route_inbound(resources).await;
    })
}

async fn route_inbound(mut resources: CursorRouterResources) {
    while let Some(message) = resources.inbound.recv().await {
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
    if !resources.expected_shutdown.load(Ordering::Acquire) {
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
    let events = {
        let mut state = resources.route.lock().map_err(|_| driver_state_error())?;
        match method {
            "session/update" => resources
                .normalizer
                .normalize_session_update(&mut state, params)?,
            "cursor/update_todos" => vec![resources.normalizer.normalize_todos(&state, &params)?],
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
        "terminal/create" => handle_terminal_create(resources, rpc_id, params).await,
        "terminal/output" => handle_terminal_output(resources, rpc_id, params).await,
        "terminal/wait_for_exit" => handle_terminal_wait(resources, rpc_id, params).await,
        "terminal/kill" => handle_terminal_kill(resources, rpc_id, params).await,
        "terminal/release" => handle_terminal_release(resources, rpc_id, params).await,
        "cursor/ask_question" => handle_question(resources, rpc_id, params).await,
        "x.ai/ask_user_question" | "_x.ai/ask_user_question"
            if resources.flavor == AcpProviderFlavor::Grok =>
        {
            handle_xai_question(resources, rpc_id, params).await
        }
        "cursor/create_plan" => {
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
                .respond_error(rpc_id, -32601, "Method not supported by Ganbaru AI")
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
