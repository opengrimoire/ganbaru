//! Claude Code provider driver over the native SDK JSONL protocol.

use super::home::*;
use super::normalizer::{ClaudeEventNormalizer, ClaudeRouteState};
use super::protocol::*;
use super::session::*;
use super::support::*;
use super::transport::ClaudeJsonlConnection;
use crate::events::*;
use crate::models::*;
use crate::process::{ProviderProcessConfig, spawn_provider_process};
use crate::{DriverOperationContext, ProviderEventSink};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::task::JoinHandle;

pub(super) const CLAUDE_STDERR_LIMIT_BYTES: usize = 256 * 1024;
pub(super) const VERSION_OUTPUT_LIMIT_BYTES: u64 = 4 * 1024;
pub(super) const SESSION_GRACEFUL_STOP: Duration = Duration::from_millis(500);
pub(super) const SESSION_FORCE_STOP: Duration = Duration::from_secs(2);
pub(super) static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
pub(super) static NEXT_UUID: AtomicU64 = AtomicU64::new(1);

pub struct ClaudeProviderDriver {
    pub(super) configuration: ProviderInstanceConfig,
    pub(super) settings: ClaudeProviderSettings,
    pub(super) live: Option<ClaudeLiveSession>,
    pub(super) cached_models: Option<ProviderModelCatalog>,
    pub(super) cached_commands: Vec<ClaudeCommand>,
    #[cfg(test)]
    connection_factory: Option<TestConnectionFactory>,
}

#[cfg(test)]
type TestConnectionFactory = Arc<
    dyn Fn(&Path) -> ChatResult<(ClaudeJsonlConnection, ClaudeHome, ClaudeVersion)> + Send + Sync,
>;

pub(super) struct ClaudeLiveSession {
    pub(super) connection: ClaudeJsonlConnection,
    pub(super) router_task: JoinHandle<()>,
    pub(super) route: Arc<Mutex<ClaudeRouteState>>,
    pub(super) pending_requests: PendingClaudeRequests,
    pub(super) normalizer: Arc<ClaudeEventNormalizer>,
    pub(super) sink: Arc<dyn ProviderEventSink>,
    pub(super) expected_shutdown: Arc<AtomicBool>,
    pub(super) terminal_error: Arc<Mutex<Option<ChatError>>>,
    pub(super) session_id: ProviderSessionId,
    pub(super) effective_model: Option<ModelId>,
    pub(super) effective_effort: Option<String>,
    pub(super) effective_fast_mode: Option<bool>,
}

impl Drop for ClaudeLiveSession {
    fn drop(&mut self) {
        self.expected_shutdown.store(true, Ordering::Release);
        self.router_task.abort();
    }
}

pub(super) struct ClaudeProbeSnapshot {
    pub(super) version: ClaudeVersion,
    pub(super) initialize: ClaudeInitializeResponse,
    pub(super) models: Vec<ProviderModel>,
}

impl ClaudeProviderDriver {
    pub fn new(configuration: ProviderInstanceConfig) -> ChatResult<Self> {
        if configuration.family_id.as_str() != "claude" {
            return Err(ChatError::validation(
                "familyId",
                "Claude driver requires the Claude provider family",
            ));
        }
        let settings = ClaudeProviderSettings::parse(&configuration)?;
        Ok(Self {
            configuration,
            settings,
            live: None,
            cached_models: None,
            cached_commands: Vec::new(),
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
            family_id: ProviderFamilyId::new("claude")
                .expect("static Claude family ID must be valid"),
            display_name: "Anthropic".to_string(),
            configuration_schema_version: 1,
            supported_platforms: vec![
                "linux".to_string(),
                "windows".to_string(),
                "macos".to_string(),
            ],
            minimum_tested_cli_version: Some(MINIMUM_CLAUDE_VERSION.to_string()),
            default_executable_candidates: vec!["claude".to_string()],
            implementation_status: ProviderImplementationStatus::Available,
            maturity: ProviderMaturity::Stable,
            protocol_name: "claude-stream-json".to_string(),
            potential_capabilities: claude_capability_kinds(),
            unavailable_reason: None,
        }
    }

    pub(super) async fn probe_snapshot(
        &self,
        context: &DriverOperationContext,
    ) -> ChatResult<ClaudeProbeSnapshot> {
        let workspace = canonical_current_directory()?;
        let session_uuid = new_uuid(&self.configuration.instance_id);
        let (mut connection, _, version) = self
            .open_connection(
                &workspace,
                ClaudeLaunchOptions {
                    fresh_session_uuid: Some(&session_uuid),
                    resume_session_uuid: None,
                    last_assistant_uuid: None,
                    model: None,
                    effort: None,
                    fast_mode: None,
                    modes: TurnModeSnapshot {
                        safety_mode: SafetyMode::AskForApproval,
                        interaction_mode: InteractionMode::Build,
                    },
                },
            )
            .await?;
        let result = async {
            let initialize = initialize(&connection, context).await?;
            let models = provider_models(
                initialize.models.clone(),
                if self.settings.allow_custom_models {
                    &self.settings.custom_model_ids
                } else {
                    &[]
                },
                &self.settings.custom_model_labels,
            )?;
            Ok(ClaudeProbeSnapshot {
                version,
                initialize,
                models,
            })
        }
        .await;
        let stop = connection
            .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
            .await;
        match result {
            Ok(snapshot) => {
                stop?;
                Ok(snapshot)
            }
            Err(error) => Err(error),
        }
    }

    async fn open_connection(
        &self,
        working_directory: &Path,
        options: ClaudeLaunchOptions<'_>,
    ) -> ChatResult<(ClaudeJsonlConnection, ClaudeHome, ClaudeVersion)> {
        #[cfg(test)]
        if let Some(factory) = self.connection_factory.as_ref() {
            return factory(working_directory);
        }
        let home = resolve_claude_home(&self.configuration)?;
        let mut environment = claude_process_environment(&self.configuration, &home)?;
        let executable = resolve_claude_executable(&self.configuration.executable, &environment)?;
        let version = probe_version(&executable, working_directory, environment.clone()).await?;
        ensure_supported_version(version)?;
        let mut arguments = launch_arguments(
            executable.prefix_arguments.clone(),
            &self.configuration.launch_arguments,
            options,
        )?;
        if let Some(server) = &self.configuration.internal_mcp {
            const TOKEN_ENVIRONMENT: &str = "GANBARU_CHAT_MCP_TOKEN";
            environment.insert(TOKEN_ENVIRONMENT.to_string(), server.bearer_token.clone());
            arguments.push("--mcp-config".to_string());
            arguments.push(
                json!({
                    "mcpServers": {
                        server.name.clone(): {
                            "type": "http",
                            "url": server.url,
                            "headers": {
                                "Authorization": format!("Bearer ${{{TOKEN_ENVIRONMENT}}}")
                            }
                        }
                    }
                })
                .to_string(),
            );
        }
        let process = spawn_provider_process(ProviderProcessConfig {
            executable: executable.executable,
            arguments,
            working_directory: working_directory.to_path_buf(),
            environment,
            stderr_limit_bytes: CLAUDE_STDERR_LIMIT_BYTES,
        })?;
        Ok((ClaudeJsonlConnection::from_process(process)?, home, version))
    }

    pub(super) async fn open_session(
        &mut self,
        input: SessionOpenInput,
        sink: Arc<dyn ProviderEventSink>,
        context: &DriverOperationContext,
    ) -> ChatResult<ProviderSessionSnapshot> {
        if self.live.is_some() {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Claude session is already running",
                true,
            ));
        }
        if input.provider_instance_id() != &self.configuration.instance_id {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Claude provider instance does not match the session request",
                false,
            ));
        }
        let workspace = canonical_verified_workspace(input.workspace())?;
        let home = resolve_claude_home(&self.configuration)?;
        let continuation_group_id = home.continuation_group()?;
        input.verify_continuation(&continuation_group_id)?;
        let cursor = input.resume_cursor(&self.configuration.instance_id)?;
        let model = input.model_id().cloned();
        let effort = selected_effort(input.model_options())?;
        let fast_mode = selected_fast_mode(input.model_options())?;
        let fresh_uuid =
            matches!(&input, SessionOpenInput::Fresh(_)).then_some(cursor.session_uuid.as_str());
        let resume_uuid =
            matches!(&input, SessionOpenInput::Resume(_)).then_some(cursor.session_uuid.as_str());
        let (mut connection, _, _) = self
            .open_connection(
                &workspace,
                ClaudeLaunchOptions {
                    fresh_session_uuid: fresh_uuid,
                    resume_session_uuid: resume_uuid,
                    last_assistant_uuid: cursor.last_assistant_uuid.as_deref(),
                    model: model.as_ref(),
                    effort: effort.as_deref(),
                    fast_mode,
                    modes: input.modes(),
                },
            )
            .await?;
        let initialize = match initialize(&connection, context).await {
            Ok(response) => response,
            Err(error) if input.is_resume() => {
                let diagnostic = connection.diagnostic()?.unwrap_or_default();
                let _ = connection
                    .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
                    .await;
                if confirmed_resume_not_found(&error)
                    || diagnostic_confirms_resume_not_found(&diagnostic)
                {
                    return Err(resume_not_found_error(&diagnostic));
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let models = provider_models(
            initialize.models.clone(),
            if self.settings.allow_custom_models {
                &self.settings.custom_model_ids
            } else {
                &[]
            },
            &self.settings.custom_model_labels,
        )?;
        self.cached_commands = initialize.commands.clone();
        self.cached_models = Some(ProviderModelCatalog {
            instance_id: self.configuration.instance_id.clone(),
            models,
            source: ModelCatalogSource::Provider,
            discovered_at: now_utc()?,
            stale: false,
        });
        let session_id = new_session_id(&self.configuration.instance_id)?;
        let provider_thread_id = ProviderThreadId::new(cursor.session_uuid.clone())
            .map_err(|_| protocol_identifier_error("session UUID"))?;
        let mut route_state = ClaudeRouteState::new(input.modes(), model.clone(), cursor.clone());
        route_state.provider_thread_id = Some(cursor.session_uuid.clone());
        route_state.session_state = ProviderSessionState::Ready;
        let route = Arc::new(Mutex::new(route_state));
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));
        let normalizer = Arc::new(ClaudeEventNormalizer::new(
            self.configuration.instance_id.clone(),
            input.thread_id().clone(),
            session_id.clone(),
        ));
        let expected_shutdown = Arc::new(AtomicBool::new(false));
        let terminal_error = Arc::new(Mutex::new(None));
        let inbound = connection.take_inbound()?;
        let router_task = spawn_claude_router(ClaudeRouterResources {
            client: connection.client(),
            inbound,
            normalizer: Arc::clone(&normalizer),
            route: Arc::clone(&route),
            pending_requests: Arc::clone(&pending_requests),
            sink: Arc::clone(&sink),
            expected_shutdown: Arc::clone(&expected_shutdown),
            terminal_error: Arc::clone(&terminal_error),
        });
        let capabilities = claude_capabilities();
        let started_at = now_utc()?;
        let resume = resume_cursor(&cursor);
        let state = route.lock().map_err(|_| driver_state_error())?.clone();
        sink.emit(normalizer.external_event(
            &state,
            if input.is_resume() {
                "session/resumed"
            } else {
                "session/started"
            },
            None,
            CanonicalEvent::SessionStarted(SessionStartedEvent {
                session_id: session_id.clone(),
                state: ProviderSessionState::Ready,
                provider_thread_id: Some(provider_thread_id.clone()),
                resume_cursor: Some(resume.clone()),
                effective_modes: input.modes(),
                capability_overrides: capabilities.clone(),
            }),
        )?)
        .await?;
        sink.emit(normalizer.external_event(
            &state,
            "session/configured",
            None,
            CanonicalEvent::SessionConfigured(SessionConfiguredEvent {
                session_id: session_id.clone(),
                effective_modes: input.modes(),
                effective_model_id: model.clone(),
                effective_model_options: input.model_options().to_vec(),
            }),
        )?)
        .await?;
        self.live = Some(ClaudeLiveSession {
            connection,
            router_task,
            route,
            pending_requests,
            normalizer,
            sink,
            expected_shutdown,
            terminal_error,
            session_id: session_id.clone(),
            effective_model: model,
            effective_effort: effort,
            effective_fast_mode: fast_mode,
        });
        Ok(ProviderSessionSnapshot {
            session_id,
            state: ProviderSessionState::Ready,
            provider_thread_id: Some(provider_thread_id),
            continuation_group_id,
            resume_cursor: Some(resume),
            effective_modes: input.modes(),
            capabilities,
            started_at,
        })
    }

    pub(super) fn live_mut(
        &mut self,
        session_id: &ProviderSessionId,
    ) -> ChatResult<&mut ClaudeLiveSession> {
        let live = self
            .live
            .as_mut()
            .ok_or_else(|| ChatError::driver_unavailable("Claude session is not running"))?;
        if &live.session_id != session_id {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Claude session identity does not match the active session",
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
