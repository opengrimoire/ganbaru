use super::*;

#[derive(Clone, Copy)]
pub struct AcpRequestedConfiguration<'a> {
    pub modes: TurnModeSnapshot,
    pub model_id: Option<&'a ModelId>,
    pub model_options: &'a [ModelOptionSelection],
}

pub struct AcpSessionInitialization<'a> {
    pub flavor: AcpProviderFlavor,
    pub grok_uses_api_key: bool,
    pub connection: &'a AcpRpcConnection,
    pub workspace: &'a str,
    pub resume_session_id: Option<&'a str>,
    pub requested: AcpRequestedConfiguration<'a>,
    pub internal_mcp: Option<&'a ProviderInternalMcpConfig>,
    pub context: &'a DriverOperationContext,
}

#[cfg(test)]
pub async fn initialize_session(
    connection: &AcpRpcConnection,
    workspace: &str,
    resume_session_id: Option<&str>,
    modes: TurnModeSnapshot,
    model_id: Option<&ModelId>,
    model_options: &[ModelOptionSelection],
    context: &DriverOperationContext,
) -> ChatResult<AcpStartedSession> {
    initialize_provider_session(AcpSessionInitialization {
        flavor: AcpProviderFlavor::Cursor,
        grok_uses_api_key: false,
        connection,
        workspace,
        resume_session_id,
        requested: AcpRequestedConfiguration {
            modes,
            model_id,
            model_options,
        },
        internal_mcp: None,
        context,
    })
    .await
}

pub async fn initialize_provider_session(
    initialization: AcpSessionInitialization<'_>,
) -> ChatResult<AcpStartedSession> {
    let AcpSessionInitialization {
        flavor,
        grok_uses_api_key,
        connection,
        workspace,
        resume_session_id,
        requested,
        internal_mcp,
        context,
    } = initialization;
    let client = connection.client();
    let provider_name = flavor.display_name();
    let filesystem = FileSystemCapabilities::new()
        .read_text_file(true)
        .write_text_file(true);
    let capabilities = ClientCapabilities::new()
        .fs(filesystem)
        .terminal(true)
        .meta(serde_json::Map::from_iter([(
            "parameterizedModelPicker".to_string(),
            Value::Bool(true),
        )]));
    let initialize_request = InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(capabilities)
        .client_info(Implementation::new("ganbaru-ai", env!("CARGO_PKG_VERSION")));
    let initialize_request = serde_json::to_value(initialize_request)
        .map_err(|_| protocol_error("ACP initialization request"))?;
    let initialize_value = client
        .request("initialize", initialize_request, context)
        .await
        .map_err(|error| error.to_chat_error("initialization"))?;
    let negotiated: OfficialInitializeResponse =
        serde_json::from_value(initialize_value.clone())
            .map_err(|_| protocol_error("ACP initialization response"))?;
    if negotiated.protocol_version != ProtocolVersion::V1 {
        return Err(ChatError::new(
            ChatErrorCode::UnsupportedVersion,
            format!(
                "{provider_name} negotiated unsupported ACP protocol version {}",
                negotiated.protocol_version
            ),
            true,
        ));
    }
    let initialize = parse_initialize(initialize_value)?;
    let preferred_auth_method = match flavor {
        AcpProviderFlavor::Cursor => CURSOR_AUTH_METHOD,
        AcpProviderFlavor::Grok => {
            if grok_uses_api_key {
                "xai.api_key"
            } else {
                "cached_token"
            }
        }
    };
    if initialize
        .auth_methods
        .iter()
        .any(|method| method.id == preferred_auth_method)
    {
        client
            .request(
                "authenticate",
                json!({ "methodId": preferred_auth_method }),
                context,
            )
            .await
            .map_err(|error| authentication_error(error, "authentication"))?;
    } else if !initialize.auth_methods.is_empty() {
        return Err(ChatError::new(
            ChatErrorCode::AuthenticationRequired,
            format!("{provider_name} requires an authentication method Ganbaru does not support"),
            true,
        ));
    }
    let mcp_servers = acp_mcp_servers(internal_mcp)?;
    let (mut setup, session_id) = if let Some(session_id) = resume_session_id {
        if !initialize.agent_capabilities.load_session {
            return Err(ChatError::unsupported(format!(
                "{provider_name} did not advertise ACP session loading"
            )));
        }
        let result = client
            .request(
                "session/load",
                json!({ "sessionId": session_id, "cwd": workspace, "mcpServers": mcp_servers }),
                context,
            )
            .await
            .map_err(|error| session_load_error(error, session_id))?;
        (parse_session_setup(result, false)?, session_id.to_string())
    } else {
        let result = client
            .request(
                "session/new",
                json!({ "cwd": workspace, "mcpServers": mcp_servers }),
                context,
            )
            .await
            .map_err(|error| authentication_error(error, "session creation"))?;
        let setup = parse_session_setup(result, true)?;
        let session_id = setup
            .session_id
            .clone()
            .ok_or_else(|| protocol_error("new session ID"))?;
        (setup, session_id)
    };
    if flavor == AcpProviderFlavor::Grok {
        ensure_grok_model_state(&mut setup);
    }
    apply_provider_configuration(flavor, &client, &session_id, &mut setup, requested, context)
        .await?;
    let models = match flavor {
        AcpProviderFlavor::Cursor => parse_available_models(
            client
                .request("cursor/list_available_models", json!({}), context)
                .await
                .map_err(|error| error.to_chat_error("model discovery"))?,
        )?,
        AcpProviderFlavor::Grok => parse_acp_models(&setup)?,
    };
    Ok(AcpStartedSession {
        initialize,
        setup,
        session_id,
        models,
    })
}

fn acp_mcp_servers(config: Option<&ProviderInternalMcpConfig>) -> ChatResult<Value> {
    let servers = config
        .map(|server| {
            vec![McpServer::Http(
                McpServerHttp::new(server.name.clone(), server.url.clone()).headers(vec![
                    HttpHeader::new("Authorization", format!("Bearer {}", server.bearer_token)),
                ]),
            )]
        })
        .unwrap_or_default();
    serde_json::to_value(servers)
        .map_err(|_| ChatError::validation("internalMcp", "ACP MCP server is invalid"))
}

pub async fn apply_provider_configuration(
    flavor: AcpProviderFlavor,
    client: &AcpRpcClient,
    session_id: &str,
    setup: &mut AcpSessionSetup,
    requested: AcpRequestedConfiguration<'_>,
    context: &DriverOperationContext,
) -> ChatResult<()> {
    let provider_name = flavor.display_name();
    if flavor == AcpProviderFlavor::Grok {
        if !requested.model_options.is_empty() {
            return Err(ChatError::validation(
                "modelOptions",
                "Grok does not advertise model options",
            ));
        }
        apply_grok_model(client, session_id, setup, requested.model_id, context).await?;
    }
    let updates = if flavor == AcpProviderFlavor::Cursor {
        resolve_configuration_updates(
            &setup.config_options,
            requested.model_id,
            requested.model_options,
        )?
    } else {
        Vec::new()
    };
    let requested_mode = setup
        .modes
        .as_ref()
        .and_then(|available| find_mode(available, requested.modes.interaction_mode))
        .map(|mode| mode.id.clone());
    if requested_mode.is_none() {
        if requested.modes.interaction_mode == InteractionMode::Plan {
            return Err(ChatError::unsupported(format!(
                "{provider_name} did not advertise a native Plan mode"
            )));
        }
        if setup.modes.is_some() {
            return Err(ChatError::unsupported(format!(
                "{provider_name} did not advertise a compatible Build mode"
            )));
        }
    }
    let mut applied = Vec::new();
    for update in &updates {
        let result = client
            .request(
                "session/set_config_option",
                config_request(session_id, &update.config_id, &update.value),
                context,
            )
            .await;
        match result {
            Ok(result) => {
                let parsed = parse_config_update_response(result, &update.config_id, &update.value);
                let options = match parsed {
                    Ok(options) => options,
                    Err(error) => {
                        let mut rollback = applied.clone();
                        rollback.push(update.clone());
                        rollback_configuration(client, session_id, &rollback, context).await;
                        return Err(error);
                    }
                };
                setup.config_options = options;
                applied.push(update.clone());
            }
            Err(error) => {
                rollback_configuration(client, session_id, &applied, context).await;
                return Err(ChatError::validation(
                    "modelOptions",
                    format!("{provider_name} rejected a model setting: {error}"),
                ));
            }
        }
    }
    if let (Some(mode_id), Some(mode_state)) = (requested_mode, setup.modes.as_mut()) {
        if mode_state.current_mode_id != mode_id {
            if let Some(mode_update) =
                resolve_mode_configuration_update(&setup.config_options, &mode_id)?
            {
                let result = client
                    .request(
                        "session/set_config_option",
                        config_request(session_id, &mode_update.config_id, &mode_update.value),
                        context,
                    )
                    .await;
                let options = match result {
                    Ok(result) => parse_config_update_response(
                        result,
                        &mode_update.config_id,
                        &mode_update.value,
                    ),
                    Err(error) => Err(ChatError::validation(
                        "interactionMode",
                        format!("{provider_name} rejected the requested mode: {error}"),
                    )),
                };
                match options {
                    Ok(options) => setup.config_options = options,
                    Err(error) => {
                        let mut rollback = applied.clone();
                        rollback.push(mode_update);
                        rollback_configuration(client, session_id, &rollback, context).await;
                        return Err(error);
                    }
                }
            } else if let Err(error) = client
                .request(
                    "session/set_mode",
                    json!({ "sessionId": session_id, "modeId": mode_id }),
                    context,
                )
                .await
            {
                rollback_configuration(client, session_id, &applied, context).await;
                return Err(ChatError::validation(
                    "interactionMode",
                    format!("{provider_name} rejected the requested mode: {error}"),
                ));
            }
            mode_state.current_mode_id = mode_id;
        }
    }
    Ok(())
}

async fn apply_grok_model(
    client: &AcpRpcClient,
    session_id: &str,
    setup: &mut AcpSessionSetup,
    model_id: Option<&ModelId>,
    context: &DriverOperationContext,
) -> ChatResult<()> {
    let Some(model_state) = setup.models.as_mut() else {
        if model_id.is_some() {
            return Err(ChatError::unsupported(
                "Grok did not advertise dynamic models",
            ));
        }
        return Ok(());
    };
    let requested = model_id
        .map(ModelId::as_str)
        .unwrap_or(model_state.current_model_id.as_str());
    if !model_state
        .available_models
        .iter()
        .any(|model| model.model_id == requested)
    {
        return Err(ChatError::validation(
            "modelId",
            "Grok model is not in the advertised catalog",
        ));
    }
    if model_state.current_model_id != requested {
        client
            .request(
                "session/set_model",
                json!({ "sessionId": session_id, "modelId": requested }),
                context,
            )
            .await
            .map_err(|error| {
                ChatError::validation(
                    "modelId",
                    format!("Grok rejected the requested model: {error}"),
                )
            })?;
        model_state.current_model_id = requested.to_string();
    }
    Ok(())
}

async fn rollback_configuration(
    client: &AcpRpcClient,
    session_id: &str,
    applied: &[ResolvedConfigUpdate],
    context: &DriverOperationContext,
) {
    for update in applied.iter().rev() {
        let _ = client
            .request(
                "session/set_config_option",
                config_request(session_id, &update.config_id, &update.previous_value),
                context,
            )
            .await;
    }
}

fn config_request(session_id: &str, config_id: &str, value: &Value) -> Value {
    match value {
        Value::Bool(value) => json!({
            "sessionId": session_id,
            "configId": config_id,
            "type": "boolean",
            "value": value,
        }),
        Value::String(value) => json!({
            "sessionId": session_id,
            "configId": config_id,
            "value": value,
        }),
        _ => json!({
            "sessionId": session_id,
            "configId": config_id,
            "value": value,
        }),
    }
}

fn session_load_error(error: AcpRpcFailure, session_id: &str) -> ChatError {
    if let AcpRpcFailure::Remote { code, message } = &error {
        if confirmed_session_not_found(*code, message) {
            return ChatError::new(
                ChatErrorCode::ResumeNotFound,
                format!("ACP provider session {session_id} was not found"),
                true,
            );
        }
    }
    error.to_chat_error("session load")
}

fn authentication_error(error: AcpRpcFailure, operation: &str) -> ChatError {
    if let AcpRpcFailure::Remote { code, message } = &error {
        let lower = message.to_ascii_lowercase();
        if *code == -32000
            && (lower.contains("auth") || lower.contains("login") || lower.contains("credential"))
        {
            return ChatError::new(
                ChatErrorCode::AuthenticationRequired,
                "ACP provider authentication is required",
                true,
            );
        }
    }
    error.to_chat_error(operation)
}
