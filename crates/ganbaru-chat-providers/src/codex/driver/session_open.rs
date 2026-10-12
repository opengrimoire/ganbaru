use super::support::*;
use super::*;

impl CodexProviderDriver {
    pub(super) fn open_connection(
        &self,
        working_directory: &Path,
    ) -> ChatResult<(CodexRpcConnection, CodexHomeLayout)> {
        let inject_internal_mcp = self
            .configuration
            .internal_mcp
            .as_ref()
            .is_some_and(|server| !server.organizational_authority);
        self.spawn_connection(working_directory, false, &[], inject_internal_mcp)
    }

    pub(super) fn spawn_connection(
        &self,
        working_directory: &Path,
        organizational: bool,
        disabled_mcp_servers: &[String],
        inject_internal_mcp: bool,
    ) -> ChatResult<(CodexRpcConnection, CodexHomeLayout)> {
        #[cfg(test)]
        if let Some(factory) = self.connection_factory.as_ref() {
            return factory(working_directory);
        }
        let mut layout = resolve_codex_home_layout(&self.configuration, &self.settings)?;
        materialize_codex_shadow_home(&mut layout)?;
        verify_codex_shadow_home(&layout)?;
        let mut environment = codex_process_environment(&self.configuration, &layout)?;
        let executable = resolve_codex_executable(&self.configuration.executable, &environment)?;
        let mut arguments = executable.prefix_arguments;
        arguments.push("app-server".to_string());
        arguments.extend(validated_app_server_arguments(
            &self.configuration.launch_arguments,
        )?);
        if organizational {
            append_organizational_base_arguments(&mut arguments);
        }
        if inject_internal_mcp {
            let server = self.configuration.internal_mcp.as_ref().ok_or_else(|| {
                ChatError::validation(
                    "internalMcp",
                    "Organizational Codex runs require the internal MCP endpoint",
                )
            })?;
            if organizational != server.organizational_authority {
                return Err(ChatError::validation(
                    "internalMcp",
                    "Codex organizational authority mode is inconsistent",
                ));
            }
            if organizational {
                append_disabled_mcp_arguments(&mut arguments, disabled_mcp_servers, &server.name)?;
                append_internal_mcp_arguments(&mut arguments, &server.name, &server.url)?;
            } else {
                let name = mcp_server_config_key(&server.name)?;
                let url = serde_json::to_string(&server.url).map_err(|_| {
                    ChatError::validation("internalMcp", "Internal MCP URL is invalid")
                })?;
                let token_environment = serde_json::to_string(INTERNAL_MCP_TOKEN_ENVIRONMENT)
                    .map_err(|_| {
                        ChatError::validation(
                            "internalMcp",
                            "Internal MCP token reference is invalid",
                        )
                    })?;
                arguments.extend([
                    "-c".to_string(),
                    format!("mcp_servers.{name}.url={url}"),
                    "-c".to_string(),
                    format!("mcp_servers.{name}.bearer_token_env_var={token_environment}"),
                ]);
            }
            environment.insert(
                INTERNAL_MCP_TOKEN_ENVIRONMENT.to_string(),
                server.bearer_token.clone(),
            );
        }
        let process = spawn_provider_process(ProviderProcessConfig {
            executable: executable.executable,
            arguments,
            working_directory: working_directory.to_path_buf(),
            environment,
            stderr_limit_bytes: CODEX_STDERR_LIMIT_BYTES,
        })?;
        Ok((CodexRpcConnection::from_process(process)?, layout))
    }

    async fn open_organizational_connection(
        &self,
        working_directory: &Path,
        context: &DriverOperationContext,
    ) -> ChatResult<(CodexRpcConnection, CodexHomeLayout)> {
        let server = self.configuration.internal_mcp.as_ref().ok_or_else(|| {
            ChatError::validation(
                "internalMcp",
                "Organizational Codex runs require the internal MCP endpoint",
            )
        })?;
        if !server.organizational_authority {
            return Err(ChatError::validation(
                "internalMcp",
                "Codex organizational authority was not activated",
            ));
        }
        let disabled_mcp_servers = self
            .organizational_mcp_server_names(working_directory, context)
            .await?;
        self.spawn_connection(working_directory, true, &disabled_mcp_servers, true)
    }

    pub(super) async fn organizational_mcp_server_names(
        &self,
        working_directory: &Path,
        context: &DriverOperationContext,
    ) -> ChatResult<Vec<String>> {
        let (mut inspection, layout) =
            self.spawn_connection(working_directory, true, &[], false)?;
        let inspection_client = inspection.client();
        let inspection_result = async {
            let initialize = inspection_client
                .request("initialize", initialize_params(), context)
                .await
                .map_err(|error| error.to_chat_error("organizational initialize"))?;
            let initialize: InitializeResponse =
                decode_response(initialize, "organizational initialize response")
                    .map_err(|error| error.to_chat_error("organizational initialize"))?;
            verify_reported_home(&layout, &initialize.codex_home)?;
            inspection_client
                .notify("initialized", json!({}))
                .await
                .map_err(|error| error.to_chat_error("organizational initialized notification"))?;
            let response = inspection_client
                .request(
                    "config/read",
                    json!({
                        "cwd": working_directory.to_string_lossy(),
                        "includeLayers": false,
                    }),
                    context,
                )
                .await
                .map_err(|error| error.to_chat_error("organizational config read"))?;
            let response: ConfigReadResponse = decode_response(response, "config response")
                .map_err(|error| error.to_chat_error("organizational config read"))?;
            mcp_server_names(response)
        }
        .await;
        let stop_result = inspection
            .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
            .await;
        match inspection_result {
            Ok(names) => {
                stop_result?;
                Ok(names)
            }
            Err(error) => Err(error),
        }
    }

    pub(super) async fn open_session(
        &mut self,
        input: SessionOpenInput,
        event_sink: Arc<dyn ProviderEventSink>,
        context: &DriverOperationContext,
    ) -> ChatResult<ProviderSessionSnapshot> {
        if self.live.is_some() {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Codex session is already running",
                true,
            ));
        }
        if input.provider_instance_id() != &self.configuration.instance_id {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Codex provider instance does not match the session request",
                false,
            ));
        }
        let workspace = canonical_verified_workspace(input.workspace())?;
        let organizational = self
            .configuration
            .internal_mcp
            .as_ref()
            .is_some_and(|server| server.organizational_authority);
        let (mut connection, layout) = if organizational {
            self.open_organizational_connection(&workspace, context)
                .await?
        } else {
            self.open_connection(&workspace)?
        };
        let continuation_group_id = layout.continuation_group_with_authority(organizational)?;
        input.verify_continuation(&continuation_group_id)?;
        let client = connection.client();
        let initialize = client
            .request("initialize", initialize_params(), context)
            .await
            .map_err(|error| error.to_chat_error("initialize"))?;
        let initialize: InitializeResponse = decode_response(initialize, "initialize response")
            .map_err(|error| error.to_chat_error("initialize"))?;
        verify_reported_home(&layout, &initialize.codex_home)?;
        client
            .notify("initialized", json!({}))
            .await
            .map_err(|error| error.to_chat_error("initialized notification"))?;
        if organizational {
            let config = client
                .request(
                    "config/read",
                    json!({
                        "cwd": workspace.to_string_lossy(),
                        "includeLayers": false,
                    }),
                    context,
                )
                .await
                .map_err(|error| error.to_chat_error("organizational config verification"))?;
            let config: ConfigReadResponse = decode_response(config, "config response")
                .map_err(|error| error.to_chat_error("organizational config verification"))?;
            let internal_mcp = self
                .configuration
                .internal_mcp
                .as_ref()
                .map(|server| (server.name.as_str(), server.url.as_str()));
            verify_organizational_effective_config(config, internal_mcp)?;
            let profiles = client
                .request(
                    "permissionProfile/list",
                    json!({
                        "cwd": workspace.to_string_lossy(),
                        "cursor": null,
                        "limit": 100,
                    }),
                    context,
                )
                .await
                .map_err(|error| error.to_chat_error("organizational permission profiles"))?;
            let profiles: PermissionProfileListResponse =
                decode_response(profiles, "permission profile list response")
                    .map_err(|error| error.to_chat_error("organizational permission profiles"))?;
            verify_permission_profile_list(&profiles)?;
        }

        let session_id = new_session_id(&self.configuration.instance_id)?;
        let requested_model = input.model_id();
        let route = Arc::new(Mutex::new(CodexRouteState::new(
            input.modes(),
            requested_model.cloned(),
        )));
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));
        let normalizer = Arc::new(CodexEventNormalizer::new(
            self.configuration.instance_id.clone(),
            input.thread_id().clone(),
            session_id.clone(),
        ));
        let (provider_thread_sender, provider_thread_receiver) = watch::channel(None);
        let expected_shutdown = Arc::new(AtomicBool::new(false));
        let terminal_error = Arc::new(Mutex::new(None));
        let inbound = connection.take_inbound()?;
        let router_task = spawn_codex_router(CodexRouterResources {
            client: client.clone(),
            inbound,
            normalizer: Arc::clone(&normalizer),
            route: Arc::clone(&route),
            pending_requests: Arc::clone(&pending_requests),
            sink: Arc::clone(&event_sink),
            provider_thread_sender,
            expected_shutdown: Arc::clone(&expected_shutdown),
            terminal_error: Arc::clone(&terminal_error),
            organizational,
        });

        let developer_instructions = input.developer_instructions();
        let start_params = thread_open_params(
            None,
            &workspace,
            input.modes(),
            requested_model,
            developer_instructions,
            organizational,
        )?;
        let requested_provider_thread_id = input.resume_provider_thread_id().map(str::to_owned);
        let (response, resumed) = match requested_provider_thread_id.as_deref() {
            Some(provider_thread_id) => {
                let params = thread_open_params(
                    Some(provider_thread_id),
                    &workspace,
                    input.modes(),
                    requested_model,
                    developer_instructions,
                    organizational,
                )?;
                match client.request("thread/resume", params, context).await {
                    Ok(response) => (response, true),
                    Err(error) if confirmed_resume_not_found(&error) => {
                        let state = route.lock().map_err(|_| driver_state_error())?.clone();
                        let warning = normalizer.event(
                            &state,
                            "thread/resume",
                            None,
                            None,
                            None,
                            CanonicalEvent::RuntimeWarning(NotificationEvent {
                                code: "codex_resume_not_found_fresh_start".to_string(),
                                title: "Codex continuation was not found".to_string(),
                                detail: Some(
                                    "A fresh native Codex thread was started while Ganbaru history was preserved."
                                        .to_string(),
                                ),
                            }),
                        )?;
                        event_sink.emit(warning).await?;
                        (
                            client
                                .request("thread/start", start_params, context)
                                .await
                                .map_err(|error| error.to_chat_error("fresh thread fallback"))?,
                            false,
                        )
                    }
                    Err(error) => return Err(error.to_chat_error("thread resume")),
                }
            }
            None => (
                client
                    .request("thread/start", start_params, context)
                    .await
                    .map_err(|error| error.to_chat_error("thread start"))?,
                false,
            ),
        };
        let response: ThreadOpenResponse = decode_response(response, "thread open response")
            .map_err(|error| error.to_chat_error("thread open"))?;
        if organizational {
            verify_organizational_thread_open(input.modes().safety_mode, &workspace, &response)?;
        } else {
            verify_effective_safety(input.modes().safety_mode, &response)?;
        }
        let provider_thread_id = ProviderThreadId::new(response.thread.id.clone())
            .map_err(|_| protocol_identifier_error("provider thread"))?;
        if resumed && requested_provider_thread_id.as_deref() != Some(provider_thread_id.as_str()) {
            return Err(ChatError::new(
                ChatErrorCode::Protocol,
                "Codex resumed a different provider thread",
                false,
            ));
        }
        if !resumed {
            wait_for_provider_thread(
                provider_thread_receiver.clone(),
                provider_thread_id.as_str(),
                context,
            )
            .await?;
        }
        if organizational {
            let internal_name = self
                .configuration
                .internal_mcp
                .as_ref()
                .map(|server| server.name.as_str())
                .ok_or_else(|| {
                    ChatError::validation(
                        "internalMcp",
                        "Organizational Codex runs require the internal MCP endpoint",
                    )
                })?;
            let status =
                Self::fetch_mcp_status(&client, Some(provider_thread_id.as_str()), context).await?;
            verify_organizational_mcp_status(&status, internal_name)?;
        }
        let effective_model = ModelId::new(response.model.clone())
            .map_err(|_| protocol_identifier_error("effective model"))?;
        {
            let mut state = route.lock().map_err(|_| driver_state_error())?;
            state.provider_thread_id = Some(provider_thread_id.as_str().to_string());
            state.effective_model_id = Some(effective_model.clone());
            state.session_state = ProviderSessionState::Ready;
        }
        let started_at = now_utc()?;
        let resume_cursor = VersionedJson {
            schema_version: 1,
            value: json!({ "threadId": provider_thread_id.as_str() }),
        };
        let capabilities = codex_capabilities();
        let state = route.lock().map_err(|_| driver_state_error())?.clone();
        event_sink
            .emit(normalizer.event(
                &state,
                if resumed {
                    "session/resumed"
                } else {
                    "session/started"
                },
                None,
                None,
                None,
                CanonicalEvent::SessionStarted(SessionStartedEvent {
                    session_id: session_id.clone(),
                    state: ProviderSessionState::Ready,
                    provider_thread_id: Some(provider_thread_id.clone()),
                    resume_cursor: Some(resume_cursor.clone()),
                    effective_modes: input.modes(),
                    capability_overrides: capabilities.clone(),
                }),
            )?)
            .await?;
        event_sink
            .emit(normalizer.event(
                &state,
                "session/configured",
                None,
                None,
                None,
                CanonicalEvent::SessionConfigured(SessionConfiguredEvent {
                    session_id: session_id.clone(),
                    effective_modes: input.modes(),
                    effective_model_id: Some(effective_model.clone()),
                    effective_model_options: Vec::new(),
                }),
            )?)
            .await?;
        self.live = Some(CodexLiveSession {
            connection,
            router_task,
            route,
            pending_requests,
            normalizer,
            sink: event_sink,
            expected_shutdown,
            terminal_error,
            session_id: session_id.clone(),
            workspace,
            effective_model: effective_model.as_str().to_string(),
            refresh_mcp_before_turn: self.settings.refresh_mcp_before_turn
                || self.configuration.internal_mcp.is_some()
                || self
                    .configuration
                    .launch_arguments
                    .iter()
                    .any(|argument| argument.contains("mcp_servers.")),
            organizational,
            internal_mcp_name: self
                .configuration
                .internal_mcp
                .as_ref()
                .map(|server| server.name.clone()),
        });
        Ok(ProviderSessionSnapshot {
            session_id,
            state: ProviderSessionState::Ready,
            provider_thread_id: Some(provider_thread_id),
            continuation_group_id,
            resume_cursor: Some(resume_cursor),
            effective_modes: input.modes(),
            capabilities,
            started_at,
        })
    }
}

pub(super) enum SessionOpenInput {
    Fresh(StartSessionRequest),
    Resume(ResumeSessionRequest),
}

impl SessionOpenInput {
    fn provider_instance_id(&self) -> &ProviderInstanceId {
        match self {
            Self::Fresh(request) => &request.provider_instance_id,
            Self::Resume(request) => &request.provider_instance_id,
        }
    }

    fn thread_id(&self) -> &ChatThreadId {
        match self {
            Self::Fresh(request) => &request.thread_id,
            Self::Resume(request) => &request.thread_id,
        }
    }

    fn workspace(&self) -> &VerifiedWorkspaceContext {
        match self {
            Self::Fresh(request) => &request.workspace,
            Self::Resume(request) => &request.workspace,
        }
    }

    fn modes(&self) -> TurnModeSnapshot {
        match self {
            Self::Fresh(request) => request.modes,
            Self::Resume(request) => request.modes,
        }
    }

    fn model_id(&self) -> Option<&ModelId> {
        match self {
            Self::Fresh(request) => request.model_id.as_ref(),
            Self::Resume(_) => None,
        }
    }

    fn developer_instructions(&self) -> Option<&str> {
        None
    }

    fn resume_provider_thread_id(&self) -> Option<&str> {
        match self {
            Self::Fresh(_) => None,
            Self::Resume(request) => Some(request.provider_thread_id.as_str()),
        }
    }

    fn verify_continuation(&self, actual: &ContinuationGroupId) -> ChatResult<()> {
        let Self::Resume(request) = self else {
            return Ok(());
        };
        if &request.continuation_group_id != actual {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Codex home is incompatible with this thread. Fork the thread to continue.",
                true,
            ));
        }
        if request.resume_cursor.schema_version != 1
            || request
                .resume_cursor
                .value
                .get("threadId")
                .and_then(Value::as_str)
                != Some(request.provider_thread_id.as_str())
        {
            return Err(ChatError::validation(
                "resumeCursor",
                "Codex resume cursor is invalid",
            ));
        }
        Ok(())
    }
}
