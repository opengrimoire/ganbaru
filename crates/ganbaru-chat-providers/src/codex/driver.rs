//! Codex provider driver over app-server stdio.

use super::home::*;
use super::normalizer::{CodexEventNormalizer, CodexRouteState};
use super::organizational::*;
use super::protocol::*;
use super::session::*;
use super::transport::{CodexRpcConnection, CodexRpcFailure};
use crate::events::{
    CanonicalEvent, ContentDeltaEvent, ItemLifecycleEvent, NotificationEvent,
    SessionConfiguredEvent, SessionExitedEvent, SessionStartedEvent, TurnCompletedEvent,
    TurnStartedEvent,
};
use crate::models::*;
use crate::process::{ProviderProcessConfig, spawn_provider_process};
use crate::{DriverFuture, DriverOperationContext, ProviderDriver, ProviderEventSink};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinHandle;

mod commands;
mod discovery;
mod session_open;
mod support;

pub(super) use commands::codex_native_command;
#[cfg(test)]
pub(super) use commands::{CodexNativeCommand, goal_request, parse_mcp_status_page};
use commands::{clear_codex_command_route, dispatch_codex_command};
use session_open::SessionOpenInput;
use support::*;
#[cfg(test)]
pub(super) use support::{confirmed_resume_not_found, validated_app_server_arguments};

const CODEX_STDERR_LIMIT_BYTES: usize = 256 * 1024;
const SESSION_GRACEFUL_STOP: Duration = Duration::from_millis(500);
const SESSION_FORCE_STOP: Duration = Duration::from_secs(2);
const PROVIDER_THREAD_NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_MODEL_PAGES: usize = 64;
const MAX_MODELS: usize = 2_048;
const MAX_HISTORY_ITEMS: usize = 100;
const MAX_MCP_STATUS_PAGES: usize = 32;
const MAX_MCP_SERVERS: usize = 3_200;
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

pub struct CodexProviderDriver {
    configuration: ProviderInstanceConfig,
    settings: CodexProviderSettings,
    live: Option<CodexLiveSession>,
    cached_models: Option<ProviderModelCatalog>,
    #[cfg(test)]
    connection_factory: Option<TestConnectionFactory>,
}

#[cfg(test)]
type TestConnectionFactory =
    Arc<dyn Fn(&Path) -> ChatResult<(CodexRpcConnection, CodexHomeLayout)> + Send + Sync>;

struct CodexLiveSession {
    connection: CodexRpcConnection,
    router_task: JoinHandle<()>,
    route: Arc<Mutex<CodexRouteState>>,
    pending_requests: PendingCodexRequests,
    normalizer: Arc<CodexEventNormalizer>,
    sink: Arc<dyn ProviderEventSink>,
    expected_shutdown: Arc<AtomicBool>,
    terminal_error: Arc<Mutex<Option<ChatError>>>,
    session_id: ProviderSessionId,
    workspace: PathBuf,
    effective_model: String,
    refresh_mcp_before_turn: bool,
    organizational: bool,
    internal_mcp_name: Option<String>,
}

impl Drop for CodexLiveSession {
    fn drop(&mut self) {
        self.expected_shutdown.store(true, Ordering::Release);
        self.router_task.abort();
    }
}

struct ProbeSnapshot {
    initialize: InitializeResponse,
    account: AccountReadResponse,
    models: Vec<ProviderModel>,
    authority_support: ProviderAuthoritySupport,
}

impl CodexProviderDriver {
    pub fn new(configuration: ProviderInstanceConfig) -> ChatResult<Self> {
        if configuration.family_id.as_str() != "codex" {
            return Err(ChatError::validation(
                "familyId",
                "Codex driver requires the Codex provider family",
            ));
        }
        let settings = CodexProviderSettings::parse(&configuration)?;
        Ok(Self {
            configuration,
            settings,
            live: None,
            cached_models: None,
            #[cfg(test)]
            connection_factory: None,
        })
    }

    #[cfg(test)]
    pub(super) fn set_connection_factory(&mut self, factory: TestConnectionFactory) {
        self.connection_factory = Some(factory);
    }

    pub fn metadata_read() -> ProviderFamilyMetadataRead {
        ProviderFamilyMetadataRead {
            family_id: ProviderFamilyId::new("codex")
                .expect("static Codex family ID must be valid"),
            display_name: "OpenAI".to_string(),
            configuration_schema_version: 1,
            supported_platforms: vec![
                "linux".to_string(),
                "windows".to_string(),
                "macos".to_string(),
            ],
            minimum_tested_cli_version: None,
            default_executable_candidates: vec!["codex".to_string()],
            implementation_status: ProviderImplementationStatus::Available,
            maturity: ProviderMaturity::Stable,
            protocol_name: "codex-app-server".to_string(),
            potential_capabilities: codex_capability_kinds(),
            unavailable_reason: None,
        }
    }

    fn live_mut(&mut self, session_id: &ProviderSessionId) -> ChatResult<&mut CodexLiveSession> {
        let live = self.live.as_mut().ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::DriverUnavailable,
                "Codex session is not running",
                true,
            )
        })?;
        if &live.session_id != session_id {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Codex session identity does not match the active session",
                false,
            ));
        }
        if let Some(error) = live
            .terminal_error
            .lock()
            .map_err(|_| driver_state_error())?
            .clone()
        {
            return Err(error);
        }
        Ok(live)
    }
}

impl ProviderDriver for CodexProviderDriver {
    fn metadata(&self) -> ProviderFamilyMetadataRead {
        Self::metadata_read()
    }

    fn instance_configuration(&self) -> &ProviderInstanceConfig {
        &self.configuration
    }

    fn capabilities(&self) -> ProviderCapabilities {
        codex_capabilities()
    }

    fn authority_support(&self) -> ProviderAuthoritySupport {
        organizational_authority_support()
    }

    fn cached_model_catalog(&self) -> Option<ProviderModelCatalog> {
        self.cached_models.clone()
    }

    fn prompt_catalog(&self) -> ChatResult<Vec<ChatPromptCatalogEntry>> {
        Ok([
            (
                "/compact",
                "Compact",
                "Compact the active Codex context",
                None,
            ),
            (
                "/goal",
                "Goal",
                "Show, set, pause, resume, or clear the thread goal",
                Some("[objective | pause | resume | clear]"),
            ),
            (
                "/mcp",
                "MCP",
                "Show configured MCP servers and their status",
                None,
            ),
            (
                "/review",
                "Code review",
                "Start a provider-native code review",
                Some("[instructions]"),
            ),
        ]
        .into_iter()
        .map(
            |(value, label, description, argument_hint)| ChatPromptCatalogEntry {
                value: value.to_string(),
                label: label.to_string(),
                description: Some(description.to_string()),
                argument_hint: argument_hint.map(str::to_string),
                kind: "command".to_string(),
                source: "provider".to_string(),
                stale: false,
            },
        )
        .collect())
    }

    fn fork_thread<'a>(
        &'a mut self,
        request: ProviderForkThreadRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderThreadId> {
        Box::pin(async move {
            let workspace = canonical_verified_workspace(&request.workspace)?;
            let response = self
                .execute_thread_request(
                    &workspace,
                    "thread/fork",
                    json!({
                        "threadId": request.provider_thread_id.as_str(),
                        "lastTurnId": request.last_provider_turn_id,
                    }),
                    context,
                )
                .await?;
            let thread_id = response
                .get("thread")
                .and_then(|thread| thread.get("id"))
                .and_then(Value::as_str)
                .ok_or_else(|| protocol_identifier_error("forked provider thread"))?;
            ProviderThreadId::new(thread_id.to_string())
                .map_err(|_| protocol_identifier_error("forked provider thread"))
        })
    }

    fn rename_thread<'a>(
        &'a mut self,
        request: ProviderThreadLifecycleRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let title = request.title.as_deref().ok_or_else(|| {
                ChatError::validation("title", "A provider thread title is required")
            })?;
            self.execute_thread_request(
                &canonical_current_directory()?,
                "thread/name/set",
                json!({ "threadId": request.provider_thread_id.as_str(), "name": title }),
                context,
            )
            .await?;
            Ok(operation_receipt(context, "Codex thread renamed"))
        })
    }

    fn archive_thread<'a>(
        &'a mut self,
        request: ProviderThreadLifecycleRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            self.execute_thread_request(
                &canonical_current_directory()?,
                "thread/archive",
                json!({ "threadId": request.provider_thread_id.as_str() }),
                context,
            )
            .await?;
            Ok(operation_receipt(context, "Codex thread archived"))
        })
    }

    fn delete_thread<'a>(
        &'a mut self,
        request: ProviderThreadLifecycleRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            self.execute_thread_request(
                &canonical_current_directory()?,
                "thread/delete",
                json!({ "threadId": request.provider_thread_id.as_str() }),
                context,
            )
            .await?;
            Ok(operation_receipt(context, "Codex thread deleted"))
        })
    }

    fn unsubscribe_thread<'a>(
        &'a mut self,
        request: ProviderThreadLifecycleRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            self.execute_thread_request(
                &canonical_current_directory()?,
                "thread/unsubscribe",
                json!({ "threadId": request.provider_thread_id.as_str() }),
                context,
            )
            .await?;
            Ok(operation_receipt(context, "Codex thread unsubscribed"))
        })
    }

    fn compact_context<'a>(
        &'a mut self,
        request: CompactContextRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let live = self.live_mut(&request.session_id)?;
            let provider_thread_id = live
                .route
                .lock()
                .map_err(|_| driver_state_error())?
                .provider_thread_id
                .clone()
                .ok_or_else(|| protocol_identifier_error("provider thread"))?;
            {
                let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                if state.active_chat_turn_id.is_some() {
                    return Err(ChatError::new(
                        ChatErrorCode::Busy,
                        "Codex already has an active turn",
                        true,
                    ));
                }
                state.active_chat_turn_id = Some(request.turn_id);
                state.active_provider_turn_id = None;
                state.session_state = ProviderSessionState::Active;
            }
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
            Ok(operation_receipt(
                context,
                "Codex context compaction started",
            ))
        })
    }

    fn read_mcp_status<'a>(
        &'a mut self,
        request: McpStatusRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, McpStatusRead> {
        Box::pin(async move {
            if let Some(session_id) = request.session_id.as_ref() {
                let live = self.live_mut(session_id)?;
                let provider_thread_id = live
                    .route
                    .lock()
                    .map_err(|_| driver_state_error())?
                    .provider_thread_id
                    .clone()
                    .ok_or_else(|| protocol_identifier_error("provider thread"))?;
                return Self::fetch_mcp_status(
                    &live.connection.client(),
                    Some(&provider_thread_id),
                    context,
                )
                .await;
            }
            let working_directory = request.working_directory.as_deref().ok_or_else(|| {
                ChatError::validation(
                    "workingDirectory",
                    "Codex MCP status requires a working directory without a live session",
                )
            })?;
            self.read_standalone_mcp_status(working_directory, context)
                .await
        })
    }

    fn probe<'a>(
        &'a mut self,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderProbeResult> {
        Box::pin(async move {
            let checked_at = now_utc()?;
            match self.probe_snapshot(context).await {
                Ok(snapshot) => {
                    let authenticated = snapshot.account.account.is_some()
                        || !snapshot.account.requires_openai_auth;
                    let state = if authenticated {
                        ProbeState::Healthy
                    } else {
                        ProbeState::AuthenticationRequired
                    };
                    let account_label = snapshot.account.account.as_ref().map(|account| {
                        account
                            .email
                            .clone()
                            .unwrap_or_else(|| account.account_type.clone())
                    });
                    self.cached_models = Some(ProviderModelCatalog {
                        instance_id: self.configuration.instance_id.clone(),
                        models: snapshot.models,
                        source: ModelCatalogSource::Provider,
                        discovered_at: checked_at.clone(),
                        stale: false,
                    });
                    Ok(ProviderProbeResult {
                        instance_id: self.configuration.instance_id.clone(),
                        state,
                        version: parse_user_agent_version(&snapshot.initialize.user_agent),
                        negotiated_protocol_version: Some("2".to_string()),
                        account_label,
                        capabilities: codex_capabilities(),
                        authority_support: snapshot.authority_support,
                        checked_at,
                        detail: (!authenticated)
                            .then_some("Codex requires authentication".to_string()),
                    })
                }
                Err(error) if context.is_cancelled() => Err(error),
                Err(error) => Ok(ProviderProbeResult {
                    instance_id: self.configuration.instance_id.clone(),
                    state: probe_state_for_error(error.code),
                    version: None,
                    negotiated_protocol_version: None,
                    account_label: None,
                    capabilities: codex_capabilities(),
                    authority_support: ProviderAuthoritySupport::default(),
                    checked_at,
                    detail: Some(probe_detail(error.code).to_string()),
                }),
            }
        })
    }

    fn discover_models<'a>(
        &'a mut self,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderModelCatalog> {
        Box::pin(async move {
            let snapshot = self.probe_snapshot(context).await?;
            if snapshot.account.account.is_none() && snapshot.account.requires_openai_auth {
                return Err(ChatError::new(
                    ChatErrorCode::AuthenticationRequired,
                    "Codex authentication is required before model discovery",
                    true,
                ));
            }
            let catalog = ProviderModelCatalog {
                instance_id: self.configuration.instance_id.clone(),
                models: snapshot.models,
                source: ModelCatalogSource::Provider,
                discovered_at: now_utc()?,
                stale: false,
            };
            self.cached_models = Some(catalog.clone());
            Ok(catalog)
        })
    }

    fn derive_continuation_group<'a>(
        &'a mut self,
        request: ContinuationGroupRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ContinuationGroupId> {
        Box::pin(async move {
            let layout = resolve_codex_home_layout(&self.configuration, &self.settings)?;
            if let Some(expected) = request.normalized_provider_home.as_deref() {
                let expected = std::fs::canonicalize(expected).map_err(|_| {
                    ChatError::validation(
                        "normalizedProviderHome",
                        "Codex continuation home is unavailable",
                    )
                })?;
                if expected != layout.shared_home {
                    return Err(ChatError::new(
                        ChatErrorCode::Conflict,
                        "Codex home change requires a thread fork",
                        true,
                    ));
                }
            }
            let organizational = self
                .configuration
                .internal_mcp
                .as_ref()
                .is_some_and(|server| server.organizational_authority);
            layout.continuation_group_with_authority(organizational)
        })
    }

    fn start_session<'a>(
        &'a mut self,
        request: StartSessionRequest,
        event_sink: Arc<dyn ProviderEventSink>,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot> {
        Box::pin(async move {
            self.open_session(SessionOpenInput::Fresh(request), event_sink, context)
                .await
        })
    }

    fn resume_session<'a>(
        &'a mut self,
        request: ResumeSessionRequest,
        event_sink: Arc<dyn ProviderEventSink>,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot> {
        Box::pin(async move {
            self.open_session(SessionOpenInput::Resume(request), event_sink, context)
                .await
        })
    }

    fn send_turn<'a>(
        &'a mut self,
        request: SendTurnRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, TurnDispatchReceipt> {
        Box::pin(async move {
            let live = self.live_mut(&request.session_id)?;
            let client = live.connection.client();
            let command = if live.organizational {
                None
            } else {
                codex_native_command(&request)
            };
            if live.refresh_mcp_before_turn {
                client
                    .request("config/mcpServer/reload", json!({}), context)
                    .await
                    .map_err(|error| error.to_chat_error("MCP refresh"))?;
            }
            let provider_thread_id = live
                .route
                .lock()
                .map_err(|_| driver_state_error())?
                .provider_thread_id
                .clone()
                .ok_or_else(|| protocol_identifier_error("provider thread"))?;
            if live.organizational {
                let status =
                    Self::fetch_mcp_status(&client, Some(&provider_thread_id), context).await?;
                let internal_name = live.internal_mcp_name.as_deref().ok_or_else(|| {
                    ChatError::validation(
                        "internalMcp",
                        "Organizational Codex runs require the internal MCP endpoint",
                    )
                })?;
                verify_organizational_mcp_status(&status, internal_name)?;
            }
            if let Some(command) = command {
                return dispatch_codex_command(live, request, command, context).await;
            }
            let custom_safety =
                if !live.organizational && request.modes.safety_mode == SafetyMode::Custom {
                    let response = client
                        .request(
                            "config/read",
                            json!({ "cwd": live.workspace.to_string_lossy() }),
                            context,
                        )
                        .await
                        .map_err(|error| error.to_chat_error("config read"))?;
                    let response: ConfigReadResponse = decode_response(response, "config response")
                        .map_err(|error| error.to_chat_error("config read"))?;
                    Some(custom_safety_settings(response)?)
                } else {
                    None
                };
            let params = turn_start_params(
                &provider_thread_id,
                &live.workspace,
                &live.effective_model,
                &request,
                custom_safety.as_ref(),
                live.organizational,
            )?;
            {
                let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                state.active_chat_turn_id = Some(request.turn_id.clone());
                state.active_provider_turn_id = None;
                state.modes = request.modes;
                if let Some(model) = request.model_id.clone() {
                    state.effective_model_id = Some(model);
                }
            }
            let response = match client.request("turn/start", params, context).await {
                Ok(response) => response,
                Err(error) => {
                    clear_codex_command_route(live);
                    return Err(error.to_chat_error("turn start"));
                }
            };
            let response: TurnStartResponse = match decode_response(response, "turn start response")
            {
                Ok(response) => response,
                Err(error) => {
                    clear_codex_command_route(live);
                    return Err(error.to_chat_error("turn start"));
                }
            };
            let provider_turn_id = match ProviderTurnId::new(response.turn.id.clone()) {
                Ok(provider_turn_id) => provider_turn_id,
                Err(_) => {
                    clear_codex_command_route(live);
                    return Err(protocol_identifier_error("provider turn"));
                }
            };
            if response.turn.status != "inProgress" {
                clear_codex_command_route(live);
                return Err(ChatError::new(
                    ChatErrorCode::Protocol,
                    "Codex did not accept the turn as in progress",
                    true,
                ));
            }
            let observed_mismatch = {
                let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                let mismatch = state
                    .active_provider_turn_id
                    .as_deref()
                    .is_some_and(|observed| observed != provider_turn_id.as_str());
                if !mismatch {
                    state.active_provider_turn_id = Some(provider_turn_id.as_str().to_string());
                    state.session_state = ProviderSessionState::Active;
                }
                mismatch
            };
            if observed_mismatch {
                clear_codex_command_route(live);
                return Err(ChatError::new(
                    ChatErrorCode::Protocol,
                    "Codex turn notification did not match the response",
                    false,
                ));
            }
            if let Some(model) = request.model_id.as_ref() {
                live.effective_model = model.as_str().to_string();
            }
            Ok(TurnDispatchReceipt {
                turn_id: request.turn_id,
                state: ChatTurnState::Active,
                provider_turn_id: Some(provider_turn_id),
                accepted_at: now_utc()?,
            })
        })
    }

    fn steer_turn<'a>(
        &'a mut self,
        request: SteerTurnRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            if request.prompt.is_empty() || request.prompt.len() > 4 * 1024 * 1024 {
                return Err(ChatError::validation(
                    "prompt",
                    "Codex steering text is invalid",
                ));
            }
            let live = self.live_mut(&request.session_id)?;
            let state = live.route.lock().map_err(|_| driver_state_error())?.clone();
            let provider_thread_id = state
                .provider_thread_id
                .ok_or_else(|| protocol_identifier_error("provider thread"))?;
            let provider_turn_id = state
                .active_provider_turn_id
                .ok_or_else(|| ChatError::invalid_transition("Codex has no active turn"))?;
            live.connection
                .client()
                .request(
                    "turn/steer",
                    json!({
                        "threadId": provider_thread_id,
                        "expectedTurnId": provider_turn_id,
                        "input": [{ "type": "text", "text": request.prompt }],
                        "clientUserMessageId": request.command.client_command_id.as_str(),
                    }),
                    context,
                )
                .await
                .map_err(|error| error.to_chat_error("turn steer"))?;
            Ok(operation_receipt(context, "Codex accepted steering input"))
        })
    }

    fn interrupt_turn<'a>(
        &'a mut self,
        request: InterruptTurnRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let live = self.live_mut(&request.session_id)?;
            interrupt_live_turn(live, context).await?;
            Ok(operation_receipt(context, "Codex turn interrupt requested"))
        })
    }

    fn resolve_approval<'a>(
        &'a mut self,
        request: ResolveApprovalRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let live = self.live_mut(&request.session_id)?;
            resolve_codex_approval(
                &live.connection.client(),
                &live.pending_requests,
                &live.normalizer,
                &live.route,
                &live.sink,
                &request,
                context,
            )
            .await?;
            Ok(DriverOperationReceipt {
                accepted: true,
                operation_id: request.command.client_command_id.as_str().to_string(),
                detail: Some("Codex approval response accepted".to_string()),
            })
        })
    }

    fn resolve_user_input<'a>(
        &'a mut self,
        request: ResolveUserInputRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let live = self.live_mut(&request.session_id)?;
            resolve_codex_user_input(
                &live.connection.client(),
                &live.pending_requests,
                &live.normalizer,
                &live.route,
                &live.sink,
                &request,
            )
            .await?;
            Ok(DriverOperationReceipt {
                accepted: true,
                operation_id: request.command.client_command_id.as_str().to_string(),
                detail: Some("Codex user input response accepted".to_string()),
            })
        })
    }

    fn rollback<'a>(
        &'a mut self,
        _request: RollbackRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async {
            Err(ChatError::unsupported(
                "Codex native rollback is not supported",
            ))
        })
    }

    fn read_history<'a>(
        &'a mut self,
        request: ReadHistoryRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderHistoryPage> {
        Box::pin(async move {
            if request.cursor.is_some() {
                return Err(ChatError::unsupported(
                    "Codex history cursor paging is not yet required by the driver contract",
                ));
            }
            let live = self.live_mut(&request.session_id)?;
            let provider_thread_id = live
                .route
                .lock()
                .map_err(|_| driver_state_error())?
                .provider_thread_id
                .clone()
                .ok_or_else(|| protocol_identifier_error("provider thread"))?;
            let response = live
                .connection
                .client()
                .request(
                    "thread/read",
                    json!({ "threadId": provider_thread_id, "includeTurns": true }),
                    context,
                )
                .await
                .map_err(|error| error.to_chat_error("thread history"))?;
            provider_history(response, request.limit)
        })
    }

    fn stop_session<'a>(
        &'a mut self,
        request: StopSessionRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let Some(mut live) = self.live.take() else {
                return Ok(operation_receipt(
                    context,
                    "Codex session was already stopped",
                ));
            };
            if live.session_id != request.session_id {
                self.live = Some(live);
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "Codex session identity does not match the active session",
                    false,
                ));
            }
            live.expected_shutdown.store(true, Ordering::Release);
            let _ = interrupt_live_turn(&mut live, context).await;
            live.sink.flush().await?;
            let state = live.route.lock().map_err(|_| driver_state_error())?.clone();
            live.sink
                .emit(live.normalizer.event(
                    &state,
                    "session/exited",
                    None,
                    None,
                    None,
                    CanonicalEvent::SessionExited(SessionExitedEvent {
                        session_id: live.session_id.clone(),
                        expected: true,
                        exit_code: None,
                        reason: Some("Codex session stopped".to_string()),
                    }),
                )?)
                .await?;
            live.sink.flush().await?;
            live.connection
                .stop(
                    if request.force {
                        Duration::ZERO
                    } else {
                        SESSION_GRACEFUL_STOP
                    },
                    SESSION_FORCE_STOP,
                )
                .await?;
            let _ = tokio::time::timeout(SESSION_GRACEFUL_STOP, &mut live.router_task).await;
            if !live.router_task.is_finished() {
                live.router_task.abort();
            }
            Ok(operation_receipt(context, "Codex session stopped"))
        })
    }
}

fn codex_capability_kinds() -> Vec<ProviderCapability> {
    vec![
        ProviderCapability::NativeResume,
        ProviderCapability::NativePlan,
        ProviderCapability::Steering,
        ProviderCapability::DynamicModelChange,
        ProviderCapability::Images,
        ProviderCapability::FileReferences,
        ProviderCapability::Skills,
        ProviderCapability::SlashCommands,
        ProviderCapability::Approvals,
        ProviderCapability::StructuredQuestions,
        ProviderCapability::ReasoningSummaries,
        ProviderCapability::StructuredPlans,
        ProviderCapability::ContextUsage,
        ProviderCapability::McpStatus,
        ProviderCapability::AccountStatus,
        ProviderCapability::RateLimitStatus,
        ProviderCapability::ProviderDiffs,
        ProviderCapability::TaskActivity,
    ]
}

fn codex_capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        entries: codex_capability_kinds()
            .into_iter()
            .map(|capability| ProviderCapabilitySupport {
                capability,
                supported: true,
                explanation: None,
            })
            .collect(),
    }
}
