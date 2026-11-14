use super::*;

#[derive(Clone, Copy)]
pub struct AcpRequestedConfiguration<'a> {
    pub modes: TurnModeSnapshot,
    pub model_id: Option<&'a ModelId>,
    pub model_options: &'a [ModelOptionSelection],
}

pub struct AcpSessionInitialization<'a> {
    pub flavor: AcpProviderFlavor,
    pub auth: AcpAuthPreference<'a>,
    pub connection: &'a AcpRpcConnection,
    /// Inbound messages of `connection`, which drain the replay of a loaded session.
    pub inbound: &'a mut AcpInboundQueue,
    pub workspace: &'a str,
    pub resume_session_id: Option<&'a str>,
    pub requested: AcpRequestedConfiguration<'a>,
    pub internal_mcp: Option<&'a ProviderInternalMcpConfig>,
    /// Whether the client offers `terminal/create`; a session whose folder access runs no
    /// commands leaves it out.
    pub offers_terminal: bool,
    pub context: &'a DriverOperationContext,
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub async fn initialize_session(
    connection: &AcpRpcConnection,
    inbound: &mut AcpInboundQueue,
    workspace: &str,
    resume_session_id: Option<&str>,
    modes: TurnModeSnapshot,
    model_id: Option<&ModelId>,
    model_options: &[ModelOptionSelection],
    context: &DriverOperationContext,
) -> ChatResult<AcpStartedSession> {
    initialize_provider_session(AcpSessionInitialization {
        flavor: AcpProviderFlavor::Cursor,
        auth: AcpAuthPreference::Flavor(CURSOR_AUTH_METHOD),
        connection,
        inbound,
        workspace,
        resume_session_id,
        requested: AcpRequestedConfiguration {
            modes,
            model_id,
            model_options,
        },
        internal_mcp: None,
        offers_terminal: true,
        context,
    })
    .await
}

pub async fn initialize_provider_session(
    initialization: AcpSessionInitialization<'_>,
) -> ChatResult<AcpStartedSession> {
    let AcpSessionInitialization {
        flavor,
        auth,
        connection,
        inbound,
        workspace,
        resume_session_id,
        requested,
        internal_mcp,
        offers_terminal,
        context,
    } = initialization;
    let client = connection.client();
    let provider_name = flavor.display_name();
    let initialize = negotiate_initialize(flavor, &client, offers_terminal, context).await?;
    if let Some(method) = select_auth_method(provider_name, auth, &initialize)? {
        client
            .request("authenticate", json!({ "methodId": method }), context)
            .await
            .map_err(|error| authentication_error(error, "authentication"))?;
    }
    let accepts_http_mcp = initialize.agent_capabilities.accepts_http_mcp(flavor);
    let host_tools_omitted = internal_mcp.is_some() && !accepts_http_mcp;
    let mcp_servers = acp_mcp_servers(provider_name, accepts_http_mcp, internal_mcp)?;
    let (mut setup, session_id) = if let Some(session_id) = resume_session_id {
        require_session_loading(&initialize, provider_name)?;
        let result = inbound
            .load_session(
                &client,
                session_id,
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
        // A generic agent may offer no model catalog at all.
        AcpProviderFlavor::Generic
            if setup
                .models
                .as_ref()
                .is_none_or(|models| models.available_models.is_empty()) =>
        {
            Vec::new()
        }
        AcpProviderFlavor::Generic => parse_acp_models(&setup)?,
    };
    Ok(AcpStartedSession {
        initialize,
        setup,
        session_id,
        models,
        host_tools_omitted,
    })
}

/// Sends `initialize`, offering `terminal/create` when `terminal` is set, and checks the
/// negotiated ACP protocol version.
///
/// This is the whole exchange an `initialize`-only probe makes: no authentication, session, or
/// extension request follows it.
pub async fn negotiate_initialize(
    flavor: AcpProviderFlavor,
    client: &AcpRpcClient,
    terminal: bool,
    context: &DriverOperationContext,
) -> ChatResult<AcpInitializeResponse> {
    let provider_name = flavor.display_name();
    let filesystem = FileSystemCapabilities::new()
        .read_text_file(true)
        .write_text_file(true);
    let capabilities = ClientCapabilities::new().fs(filesystem).terminal(terminal);
    // The parameterized model picker is a vendor extension; a generic agent gets the base
    // capabilities only.
    let capabilities = if flavor.offers_structured_extensions() {
        capabilities.meta(serde_json::Map::from_iter([(
            "parameterizedModelPicker".to_string(),
            Value::Bool(true),
        )]))
    } else {
        capabilities
    };
    let initialize_request = InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(capabilities)
        .client_info(Implementation::new("ganbaru", env!("CARGO_PKG_VERSION")));
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
    parse_initialize(initialize_value)
}

/// How Ganbaru picks one of the agent's `authMethods`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpAuthPreference<'a> {
    /// The method a dedicated flavor always uses.
    Flavor(&'static str),
    /// The method the user configured for a generic agent, if any.
    Configured(Option<&'a str>),
}

/// The variable Grok reads its xAI API key from, ahead of the account login in its home.
pub const GROK_API_KEY_VARIABLE: &str = "XAI_API_KEY";

/// Whether the environment a Grok process starts with carries a non-blank xAI API key.
///
/// Only the launched environment counts: the app's own environment reaches the process only
/// through a configured entry, so a key that is not configured never selects key
/// authentication. Names match case-insensitively only where the platform does.
pub fn grok_api_key_configured(environment: &std::collections::BTreeMap<String, String>) -> bool {
    environment.iter().any(|(name, value)| {
        let matches = if cfg!(windows) {
            name.eq_ignore_ascii_case(GROK_API_KEY_VARIABLE)
        } else {
            name == GROK_API_KEY_VARIABLE
        };
        matches && !value.trim().is_empty()
    })
}

/// The authentication preference of a flavor and its instance settings.
pub fn auth_preference(
    flavor: AcpProviderFlavor,
    grok_uses_api_key: bool,
    configured: Option<&str>,
) -> AcpAuthPreference<'_> {
    match flavor {
        AcpProviderFlavor::Cursor => AcpAuthPreference::Flavor(CURSOR_AUTH_METHOD),
        AcpProviderFlavor::Grok if grok_uses_api_key => AcpAuthPreference::Flavor("xai.api_key"),
        AcpProviderFlavor::Grok => AcpAuthPreference::Flavor("cached_token"),
        AcpProviderFlavor::Generic => AcpAuthPreference::Configured(configured),
    }
}

/// The method to send in `authenticate`, or `None` when the agent advertises none.
///
/// The preferred method must be among the advertised ones. A generic agent without a configured
/// method fails with the advertised names, because the driver never picks a method itself.
pub fn select_auth_method(
    provider_name: &str,
    preference: AcpAuthPreference<'_>,
    initialize: &AcpInitializeResponse,
) -> ChatResult<Option<String>> {
    if initialize.auth_methods.is_empty() {
        return Ok(None);
    }
    let preferred = match preference {
        AcpAuthPreference::Flavor(method) => Some(method),
        AcpAuthPreference::Configured(method) => method,
    };
    if let Some(method) = preferred.filter(|method| {
        initialize
            .auth_methods
            .iter()
            .any(|advertised| advertised.id == *method)
    }) {
        return Ok(Some(method.to_string()));
    }
    Err(match preference {
        AcpAuthPreference::Flavor(_) => unsupported_authentication(provider_name),
        AcpAuthPreference::Configured(_) => ChatError::new(
            ChatErrorCode::AuthenticationRequired,
            format!(
                "{provider_name} requires one of its authentication methods: {}",
                initialize
                    .auth_methods
                    .iter()
                    .map(|method| format!("{} ({})", method.name, method.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            true,
        ),
    })
}

/// The error for an agent that requires authentication Ganbaru cannot perform.
pub fn unsupported_authentication(provider_name: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::AuthenticationRequired,
        format!("{provider_name} requires an authentication method Ganbaru does not support"),
        true,
    )
}

/// Builds the `mcpServers` list for `session/new` or `session/load`.
///
/// The internal server goes only to an agent that takes HTTP MCP servers. Without that support
/// an organizational session fails closed, because its host tools carry the assignment's
/// authority, and a direct thread runs without host tools.
pub(super) fn acp_mcp_servers(
    provider_name: &str,
    accepts_http_mcp: bool,
    config: Option<&ProviderInternalMcpConfig>,
) -> ChatResult<Value> {
    let servers = match config {
        Some(server) if accepts_http_mcp => vec![McpServer::Http(
            McpServerHttp::new(server.name.clone(), server.url.clone()).headers(vec![
                HttpHeader::new("Authorization", format!("Bearer {}", server.bearer_token)),
            ]),
        )],
        Some(server) if server.organizational_authority => {
            return Err(ChatError::unsupported(format!(
                "{provider_name} did not advertise HTTP MCP servers, so it cannot use the assignment's host tools"
            )));
        }
        Some(_) | None => Vec::new(),
    };
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
    if matches!(flavor, AcpProviderFlavor::Grok | AcpProviderFlavor::Generic) {
        if !requested.model_options.is_empty() {
            return Err(ChatError::validation(
                "modelOptions",
                format!("{provider_name} does not advertise model options"),
            ));
        }
        apply_session_model(
            client,
            provider_name,
            session_id,
            setup,
            requested.model_id,
            context,
        )
        .await?;
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

/// Switches the session to `model_id` through `session/set_model` when the agent advertised a
/// model catalog and the requested model differs from the current one.
async fn apply_session_model(
    client: &AcpRpcClient,
    provider_name: &str,
    session_id: &str,
    setup: &mut AcpSessionSetup,
    model_id: Option<&ModelId>,
    context: &DriverOperationContext,
) -> ChatResult<()> {
    let Some(model_state) = setup
        .models
        .as_mut()
        .filter(|models| !models.available_models.is_empty())
    else {
        if model_id.is_some() {
            return Err(ChatError::unsupported(format!(
                "{provider_name} did not advertise dynamic models"
            )));
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
            format!("{provider_name} model is not in the advertised catalog"),
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
                    format!("{provider_name} rejected the requested model: {error}"),
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

/// Refuses to resume with an agent that cannot load an earlier session. The earlier session is
/// unreachable, so the error reports it as not found and callers start a new session.
pub fn require_session_loading(
    initialize: &AcpInitializeResponse,
    provider_name: &str,
) -> ChatResult<()> {
    if initialize.agent_capabilities.load_session {
        return Ok(());
    }
    Err(ChatError::new(
        ChatErrorCode::ResumeNotFound,
        format!("{provider_name} did not advertise ACP session loading"),
        true,
    ))
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
