use super::commands::parse_mcp_status_page;
use super::support::*;
use super::*;

impl CodexProviderDriver {
    pub(super) async fn probe_snapshot(
        &self,
        context: &DriverOperationContext,
    ) -> ChatResult<ProbeSnapshot> {
        let working_directory = canonical_current_directory()?;
        let (mut connection, layout) = self.open_connection(&working_directory)?;
        let client = connection.client();
        let result = async {
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
            let account = client
                .request("account/read", json!({}), context)
                .await
                .map_err(|error| error.to_chat_error("account probe"))?;
            let account: AccountReadResponse = decode_response(account, "account response")
                .map_err(|error| error.to_chat_error("account probe"))?;
            let models = if account.account.is_some() || !account.requires_openai_auth {
                self.fetch_models(&client, context).await?
            } else {
                Vec::new()
            };
            let authority_support = self
                .probe_organizational_authority(&working_directory, context)
                .await
                .unwrap_or_default();
            Ok(ProbeSnapshot {
                initialize,
                account,
                models,
                authority_support,
            })
        }
        .await;
        let stop_result = connection
            .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
            .await;
        match result {
            Ok(snapshot) => {
                stop_result?;
                Ok(snapshot)
            }
            Err(error) => Err(error),
        }
    }

    async fn probe_organizational_authority(
        &self,
        working_directory: &Path,
        context: &DriverOperationContext,
    ) -> ChatResult<ProviderAuthoritySupport> {
        let disabled_mcp_servers = self
            .organizational_mcp_server_names(working_directory, context)
            .await?;
        let (mut connection, layout) =
            self.spawn_connection(working_directory, true, &disabled_mcp_servers, false)?;
        let client = connection.client();
        let result = async {
            let initialize = client
                .request("initialize", initialize_params(), context)
                .await
                .map_err(|error| error.to_chat_error("organizational authority initialize"))?;
            let initialize: InitializeResponse =
                decode_response(initialize, "organizational authority initialize response")
                    .map_err(|error| error.to_chat_error("organizational authority initialize"))?;
            verify_reported_home(&layout, &initialize.codex_home)?;
            client
                .notify("initialized", json!({}))
                .await
                .map_err(|error| {
                    error.to_chat_error("organizational authority initialized notification")
                })?;
            let config = client
                .request(
                    "config/read",
                    json!({
                        "cwd": working_directory.to_string_lossy(),
                        "includeLayers": false,
                    }),
                    context,
                )
                .await
                .map_err(|error| error.to_chat_error("organizational authority config"))?;
            let config: ConfigReadResponse = decode_response(config, "config response")
                .map_err(|error| error.to_chat_error("organizational authority config"))?;
            verify_organizational_effective_config(config, None)?;
            let profiles = client
                .request(
                    "permissionProfile/list",
                    json!({
                        "cwd": working_directory.to_string_lossy(),
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
            let modes = TurnModeSnapshot {
                safety_mode: SafetyMode::AskForApproval,
                interaction_mode: InteractionMode::Build,
            };
            let mut params = thread_open_params(None, working_directory, modes, None, None, true)?;
            params
                .as_object_mut()
                .ok_or_else(|| {
                    ChatError::new(
                        ChatErrorCode::Protocol,
                        "Codex organizational thread request is invalid",
                        false,
                    )
                })?
                .insert("ephemeral".to_string(), Value::Bool(true));
            let response = client
                .request("thread/start", params, context)
                .await
                .map_err(|error| error.to_chat_error("organizational authority thread"))?;
            let response: ThreadOpenResponse = decode_response(response, "thread open response")
                .map_err(|error| error.to_chat_error("organizational authority thread"))?;
            verify_organizational_thread_open(modes.safety_mode, working_directory, &response)?;
            Ok(organizational_authority_support())
        }
        .await;
        let stop_result = connection
            .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
            .await;
        match result {
            Ok(support) => {
                stop_result?;
                Ok(support)
            }
            Err(error) => Err(error),
        }
    }

    pub(super) async fn fetch_models(
        &self,
        client: &super::super::transport::CodexRpcClient,
        context: &DriverOperationContext,
    ) -> ChatResult<Vec<ProviderModel>> {
        let mut models = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0;
        loop {
            if pages >= MAX_MODEL_PAGES || models.len() >= MAX_MODELS {
                return Err(ChatError::new(
                    ChatErrorCode::Protocol,
                    "Codex model catalog exceeds the supported bounds",
                    true,
                ));
            }
            let params = cursor
                .as_ref()
                .map(|cursor| json!({ "cursor": cursor }))
                .unwrap_or_else(|| json!({}));
            let response = client
                .request("model/list", params, context)
                .await
                .map_err(|error| error.to_chat_error("model discovery"))?;
            let response: ModelListResponse = decode_response(response, "model list response")
                .map_err(|error| error.to_chat_error("model discovery"))?;
            for model in response.data {
                if models.len() >= MAX_MODELS {
                    return Err(ChatError::new(
                        ChatErrorCode::Protocol,
                        "Codex model catalog exceeds the supported bounds",
                        true,
                    ));
                }
                models.push(provider_model(model)?);
            }
            pages += 1;
            cursor = response.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        if self.settings.allow_custom_models {
            let mut known = models
                .iter()
                .map(|model| model.id.as_str().to_string())
                .collect::<BTreeSet<_>>();
            for custom in &self.settings.custom_model_ids {
                if known.insert(custom.clone()) {
                    models.push(custom_provider_model(
                        custom,
                        self.settings
                            .custom_model_labels
                            .get(custom)
                            .map(String::as_str),
                    )?);
                }
            }
        }
        Ok(models)
    }

    pub(super) async fn execute_thread_request(
        &self,
        working_directory: &Path,
        method: &str,
        params: Value,
        context: &DriverOperationContext,
    ) -> ChatResult<Value> {
        let (mut connection, layout) = self.open_connection(working_directory)?;
        let client = connection.client();
        let result = async {
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
            client
                .request(method, params, context)
                .await
                .map_err(|error| error.to_chat_error(method))
        }
        .await;
        let stop_result = connection
            .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
            .await;
        match result {
            Ok(response) => {
                stop_result?;
                Ok(response)
            }
            Err(error) => Err(error),
        }
    }

    pub(super) async fn fetch_mcp_status(
        client: &super::super::transport::CodexRpcClient,
        thread_id: Option<&str>,
        context: &DriverOperationContext,
    ) -> ChatResult<McpStatusRead> {
        let mut cursor: Option<String> = None;
        let mut servers = Vec::new();
        for _ in 0..MAX_MCP_STATUS_PAGES {
            let response = client
                .request(
                    "mcpServerStatus/list",
                    json!({
                        "threadId": thread_id,
                        "cursor": cursor,
                        "limit": 100,
                        "detail": "toolsAndAuthOnly",
                    }),
                    context,
                )
                .await
                .map_err(|error| error.to_chat_error("MCP status"))?;
            let page = parse_mcp_status_page(&response)?;
            servers.extend(page.servers);
            if servers.len() > MAX_MCP_SERVERS {
                return Err(ChatError::new(
                    ChatErrorCode::Protocol,
                    "Codex returned too many MCP server status entries",
                    false,
                ));
            }
            let Some(next_cursor) = page.next_cursor else {
                return Ok(McpStatusRead { servers });
            };
            cursor = Some(next_cursor);
        }
        Err(ChatError::new(
            ChatErrorCode::Protocol,
            "Codex MCP status pagination did not terminate",
            false,
        ))
    }

    pub(super) async fn read_standalone_mcp_status(
        &self,
        working_directory: &Path,
        context: &DriverOperationContext,
    ) -> ChatResult<McpStatusRead> {
        if !working_directory.is_absolute() || !working_directory.is_dir() {
            return Err(ChatError::validation(
                "workingDirectory",
                "Codex MCP status working directory is invalid",
            ));
        }
        let working_directory = std::fs::canonicalize(working_directory).map_err(|_| {
            ChatError::validation(
                "workingDirectory",
                "Codex MCP status working directory could not be canonicalized",
            )
        })?;
        let (mut connection, layout) = self.open_connection(&working_directory)?;
        let client = connection.client();
        let result = async {
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
            Self::fetch_mcp_status(&client, None, context).await
        }
        .await;
        let stop_result = connection
            .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
            .await;
        match result {
            Ok(status) => {
                stop_result?;
                Ok(status)
            }
            Err(error) => Err(error),
        }
    }
}
