use super::support::*;
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CodexNativeCommand {
    Compact,
    Goal(String),
    Mcp,
    Review(String),
}

pub(crate) fn codex_native_command(request: &SendTurnRequest) -> Option<CodexNativeCommand> {
    if !request.attachments.is_empty()
        || !request.mentions.is_empty()
        || request.developer_instructions.is_some()
    {
        return None;
    }
    let prompt = request.prompt.trim();
    let (name, arguments) = prompt
        .strip_prefix('/')?
        .split_once(char::is_whitespace)
        .map_or((prompt.trim_start_matches('/'), ""), |(name, arguments)| {
            (name, arguments.trim())
        });
    match name.to_ascii_lowercase().as_str() {
        "compact" if arguments.is_empty() => Some(CodexNativeCommand::Compact),
        "goal" => Some(CodexNativeCommand::Goal(arguments.to_string())),
        "mcp" if arguments.is_empty() => Some(CodexNativeCommand::Mcp),
        "review" => Some(CodexNativeCommand::Review(arguments.to_string())),
        _ => None,
    }
}

pub(super) async fn dispatch_codex_command(
    live: &mut CodexLiveSession,
    request: SendTurnRequest,
    command: CodexNativeCommand,
    context: &DriverOperationContext,
) -> ChatResult<TurnDispatchReceipt> {
    let provider_thread_id = live
        .route
        .lock()
        .map_err(|_| driver_state_error())?
        .provider_thread_id
        .clone()
        .ok_or_else(|| protocol_identifier_error("provider thread"))?;
    match command {
        CodexNativeCommand::Compact => {
            prepare_codex_command_route(live, &request)?;
            if let Err(error) = live
                .connection
                .client()
                .request(
                    "thread/compact/start",
                    json!({ "threadId": provider_thread_id }),
                    context,
                )
                .await
            {
                clear_codex_command_route(live);
                return Err(error.to_chat_error("context compaction"));
            }
            Ok(TurnDispatchReceipt {
                turn_id: request.turn_id,
                state: ChatTurnState::Active,
                provider_turn_id: None,
                accepted_at: now_utc()?,
            })
        }
        CodexNativeCommand::Review(instructions) => {
            prepare_codex_command_route(live, &request)?;
            let target = if instructions.is_empty() {
                json!({ "type": "uncommittedChanges" })
            } else {
                json!({ "type": "custom", "instructions": instructions })
            };
            let response = match live
                .connection
                .client()
                .request(
                    "review/start",
                    json!({
                        "threadId": provider_thread_id,
                        "delivery": "inline",
                        "target": target,
                    }),
                    context,
                )
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    clear_codex_command_route(live);
                    return Err(error.to_chat_error("review start"));
                }
            };
            let response: TurnStartResponse =
                match decode_response(response, "review start response") {
                    Ok(response) => response,
                    Err(error) => {
                        clear_codex_command_route(live);
                        return Err(error.to_chat_error("review start"));
                    }
                };
            let provider_turn_id = match ProviderTurnId::new(response.turn.id) {
                Ok(provider_turn_id) => provider_turn_id,
                Err(_) => {
                    clear_codex_command_route(live);
                    return Err(protocol_identifier_error("review turn"));
                }
            };
            if response.turn.status != "inProgress" {
                clear_codex_command_route(live);
                return Err(ChatError::new(
                    ChatErrorCode::Protocol,
                    "Codex did not accept the review as in progress",
                    true,
                ));
            }
            let mut state = live.route.lock().map_err(|_| driver_state_error())?;
            state.active_provider_turn_id = Some(provider_turn_id.as_str().to_string());
            state.session_state = ProviderSessionState::Active;
            drop(state);
            Ok(TurnDispatchReceipt {
                turn_id: request.turn_id,
                state: ChatTurnState::Active,
                provider_turn_id: Some(provider_turn_id),
                accepted_at: now_utc()?,
            })
        }
        CodexNativeCommand::Goal(arguments) => {
            let (method, params, response_text) = goal_request(&provider_thread_id, &arguments)?;
            let response = live
                .connection
                .client()
                .request(method, params, context)
                .await
                .map_err(|error| error.to_chat_error("goal command"))?;
            let text = response_text.unwrap_or_else(|| format_goal_response(&response));
            emit_codex_command_result(live, &request, "Goal", text).await
        }
        CodexNativeCommand::Mcp => {
            let response = live
                .connection
                .client()
                .request(
                    "mcpServerStatus/list",
                    json!({
                        "cursor": null,
                        "limit": 100,
                        "detail": "toolsAndAuthOnly",
                    }),
                    context,
                )
                .await
                .map_err(|error| error.to_chat_error("MCP status"))?;
            emit_codex_command_result(live, &request, "MCP status", format_mcp_response(&response))
                .await
        }
    }
}

fn prepare_codex_command_route(
    live: &CodexLiveSession,
    request: &SendTurnRequest,
) -> ChatResult<()> {
    let mut state = live.route.lock().map_err(|_| driver_state_error())?;
    if state.active_chat_turn_id.is_some() {
        return Err(ChatError::new(
            ChatErrorCode::Busy,
            "Codex already has an active turn",
            true,
        ));
    }
    state.active_chat_turn_id = Some(request.turn_id.clone());
    state.active_provider_turn_id = None;
    state.modes = request.modes;
    state.session_state = ProviderSessionState::Active;
    Ok(())
}

pub(super) fn clear_codex_command_route(live: &CodexLiveSession) {
    if let Ok(mut state) = live.route.lock() {
        state.active_chat_turn_id = None;
        state.active_provider_turn_id = None;
        state.session_state = ProviderSessionState::Ready;
    }
}

pub(crate) fn goal_request(
    thread_id: &str,
    arguments: &str,
) -> ChatResult<(&'static str, Value, Option<String>)> {
    let normalized = arguments.trim();
    if normalized.is_empty() {
        return Ok(("thread/goal/get", json!({ "threadId": thread_id }), None));
    }
    if normalized.eq_ignore_ascii_case("clear") {
        return Ok((
            "thread/goal/clear",
            json!({ "threadId": thread_id }),
            Some("The thread goal was cleared.".to_string()),
        ));
    }
    let status = if normalized.eq_ignore_ascii_case("pause") {
        Some("paused")
    } else if normalized.eq_ignore_ascii_case("resume") {
        Some("active")
    } else {
        None
    };
    if let Some(status) = status {
        return Ok((
            "thread/goal/set",
            json!({ "threadId": thread_id, "status": status }),
            None,
        ));
    }
    let objective = normalized.strip_prefix("set ").unwrap_or(normalized).trim();
    if objective.is_empty() || objective.len() > 4_000 || objective.contains('\0') {
        return Err(ChatError::validation(
            "prompt",
            "Codex goal objectives must contain 1 to 4,000 characters",
        ));
    }
    Ok((
        "thread/goal/set",
        json!({
            "threadId": thread_id,
            "objective": objective,
            "status": "active",
        }),
        None,
    ))
}

fn format_goal_response(response: &Value) -> String {
    let Some(goal) = response.get("goal").and_then(Value::as_object) else {
        return "There is no active thread goal.".to_string();
    };
    let objective = goal
        .get("objective")
        .and_then(Value::as_str)
        .unwrap_or("Untitled goal");
    let status = goal
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let tokens_used = goal.get("tokensUsed").and_then(Value::as_u64);
    let token_budget = goal.get("tokenBudget").and_then(Value::as_u64);
    let mut lines = vec![format!("Goal: {objective}"), format!("Status: {status}")];
    if let Some(tokens_used) = tokens_used {
        lines.push(match token_budget {
            Some(token_budget) => format!("Tokens: {tokens_used} of {token_budget}"),
            None => format!("Tokens used: {tokens_used}"),
        });
    }
    lines.join("\n")
}

fn format_mcp_response(response: &Value) -> String {
    let Some(servers) = response.get("data").and_then(Value::as_array) else {
        return "Codex returned no MCP server status entries.".to_string();
    };
    if servers.is_empty() {
        return "No MCP servers are configured.".to_string();
    }
    let mut lines = vec![format!("MCP servers: {}", servers.len())];
    for server in servers.iter().take(100) {
        let name = server
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Unknown server");
        let status = server
            .get("status")
            .and_then(Value::as_str)
            .or_else(|| server.get("authStatus").and_then(Value::as_str))
            .unwrap_or("configured");
        lines.push(format!("• {name}: {status}"));
    }
    lines.join("\n")
}

pub(crate) struct McpStatusPage {
    pub(crate) servers: Vec<McpServerStatusRead>,
    pub(crate) next_cursor: Option<String>,
}

pub(crate) fn parse_mcp_status_page(response: &Value) -> ChatResult<McpStatusPage> {
    let data = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| mcp_status_protocol_error("page"))?;
    if data.len() > 100 {
        return Err(mcp_status_protocol_error("page size"));
    }
    let servers = data
        .iter()
        .map(|value| {
            let server = value
                .as_object()
                .ok_or_else(|| mcp_status_protocol_error("entry"))?;
            let name = server
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| {
                    !value.is_empty()
                        && value.len() <= 1_000
                        && !value.chars().any(char::is_control)
                })
                .ok_or_else(|| mcp_status_protocol_error("server name"))?;
            let bounded_status = |field: &str| {
                server
                    .get(field)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| {
                        !value.is_empty()
                            && value.len() <= 200
                            && !value.chars().any(char::is_control)
                    })
                    .map(str::to_string)
            };
            Ok(McpServerStatusRead {
                name: name.to_string(),
                auth_status: bounded_status("authStatus"),
                enabled: server
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                runtime_status: bounded_status("status"),
            })
        })
        .collect::<ChatResult<Vec<_>>>()?;
    let next_cursor = response
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 4_000 && !value.chars().any(char::is_control)
        })
        .map(str::to_string);
    Ok(McpStatusPage {
        servers,
        next_cursor,
    })
}

fn mcp_status_protocol_error(detail: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        format!("Codex returned an invalid MCP server status {detail}"),
        false,
    )
}

async fn emit_codex_command_result(
    live: &CodexLiveSession,
    request: &SendTurnRequest,
    title: &str,
    text: String,
) -> ChatResult<TurnDispatchReceipt> {
    prepare_codex_command_route(live, request)?;
    let provider_turn_id =
        ProviderTurnId::new(format!("codex-command-{}", request.turn_id.as_str()))
            .map_err(|_| protocol_identifier_error("command turn"))?;
    let item_id = format!("codex-command-item-{}", request.turn_id.as_str());
    let provider_item_id = ProviderItemId::new(item_id.clone())
        .map_err(|_| protocol_identifier_error("command item"))?;
    let events = {
        let mut state = live.route.lock().map_err(|_| driver_state_error())?;
        state.active_provider_turn_id = Some(provider_turn_id.as_str().to_string());
        let turn_id = Some(request.turn_id.clone());
        let provider_turn = Some(provider_turn_id.clone());
        let provider_item = Some(provider_item_id.clone());
        let mut events = vec![live.normalizer.event(
            &state,
            "command/started",
            turn_id.clone(),
            provider_turn.clone(),
            None,
            CanonicalEvent::TurnStarted(TurnStartedEvent {
                provider_turn_id: provider_turn.clone(),
                state: ChatTurnState::Active,
                modes: request.modes,
                model_id: request.model_id.clone(),
                model_options: request.model_options.clone(),
            }),
        )?];
        events.push(live.normalizer.event(
            &state,
            "command/item/started",
            turn_id.clone(),
            provider_turn.clone(),
            provider_item.clone(),
            CanonicalEvent::ItemStarted(ItemLifecycleEvent {
                item_id: item_id.clone(),
                kind: CanonicalItemKind::AssistantMessage,
                status: ActivityStatus::Active,
                title: Some(title.to_string()),
                detail: None,
                safe_metadata: None,
            }),
        )?);
        events.push(live.normalizer.event(
            &state,
            "command/item/delta",
            turn_id.clone(),
            provider_turn.clone(),
            provider_item.clone(),
            CanonicalEvent::ContentDelta(ContentDeltaEvent {
                item_id: item_id.clone(),
                stream_kind: ContentStreamKind::AssistantText,
                content_index: 0,
                delta: text,
            }),
        )?);
        events.push(live.normalizer.event(
            &state,
            "command/item/completed",
            turn_id.clone(),
            provider_turn.clone(),
            provider_item,
            CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
                item_id,
                kind: CanonicalItemKind::AssistantMessage,
                status: ActivityStatus::Completed,
                title: Some(title.to_string()),
                detail: None,
                safe_metadata: None,
            }),
        )?);
        state.active_chat_turn_id = None;
        state.active_provider_turn_id = None;
        state.session_state = ProviderSessionState::Ready;
        events.push(live.normalizer.event(
            &state,
            "command/completed",
            turn_id,
            provider_turn,
            None,
            CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                state: ChatTurnState::Completed,
                stop_reason: Some("command".to_string()),
                usage: None,
                changed_files: Vec::new(),
            }),
        )?);
        events
    };
    for event in events {
        live.sink.emit(event).await?;
    }
    Ok(TurnDispatchReceipt {
        turn_id: request.turn_id.clone(),
        state: ChatTurnState::Completed,
        provider_turn_id: Some(provider_turn_id),
        accepted_at: now_utc()?,
    })
}
