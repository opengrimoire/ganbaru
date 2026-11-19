//! Claude Code provider driver over the native SDK JSONL protocol.

use super::home::*;
use super::instructions::{ClaudeSessionInstructions, session_instructions};
use super::normalizer::{ClaudeEventNormalizer, ClaudeRouteState, usage_reply_status};
use super::organizational::{
    ClaudeOrganizationalDeadline, ClaudeOrganizationalPermissions, organizational_route,
    verify_mcp_status,
};
use super::preferences::user_preferences;
use super::protocol::*;
use super::session::*;
use super::support::*;
use super::transport::ClaudeJsonlConnection;
use crate::confinement::ShellSandbox;
use crate::events::*;
use crate::model_recommendation::{NativeCompany, recommended_model_id};
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
/// Provider name in sandbox errors.
pub(super) const CLAUDE_PROVIDER_NAME: &str = "Claude";
/// Longest wait for the plan limits a status probe reads; a CLI that does not answer the
/// experimental request leaves the probe result unaffected.
const USAGE_LIMITS_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// A started CLI process with its configuration directory, version, and the confined shell
/// its commands run under, when they are confined.
type ClaudeOpenedConnection = (
    ClaudeJsonlConnection,
    ClaudeHome,
    ClaudeVersion,
    Option<Arc<ShellSandbox>>,
);

pub struct ClaudeProviderDriver {
    pub(super) configuration: ProviderInstanceConfig,
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
    /// The scope of a session under the organizational launch surface, which also fixes the
    /// per-turn permission mode mapping; none for direct sessions.
    pub(super) organizational: Option<ProviderOrganizationalScope>,
    /// The gate 2 deadline an organizational session arms with each user message.
    pub(super) organizational_deadline: Option<ClaudeOrganizationalDeadline>,
    /// The instructions in the session system prompt, with the file that carries them when
    /// they are too large to pass inline. Turns compare their instructions to its digest.
    pub(super) instructions: Option<ClaudeSessionInstructions>,
    /// The sandbox an organizational session's shell commands run under; none when they are
    /// unconfined and asked.
    pub(super) shell: Option<Arc<ShellSandbox>>,
}

impl Drop for ClaudeLiveSession {
    fn drop(&mut self) {
        self.expected_shutdown.store(true, Ordering::Release);
        self.router_task.abort();
    }
}

pub(super) struct ClaudeProbeSnapshot {
    pub(super) version: ClaudeVersion,
    /// The config directory the probed CLI ran with.
    pub(super) home: ClaudeHome,
    pub(super) initialize: ClaudeInitializeResponse,
    pub(super) models: Vec<ProviderModel>,
    /// Plan limits read during a status probe, when the account has a Claude plan.
    pub(super) usage_limits: Option<RateLimitStatusEvent>,
}

/// Whether the signed-in account has a Claude plan, the only case with plan limits to read.
fn has_plan(initialize: &ClaudeInitializeResponse) -> bool {
    initialize
        .account
        .as_ref()
        .is_some_and(|account| account.subscription_type.is_some())
}

/// Reads the plan limits once over an initialized connection, without starting a turn.
///
/// The CLI answers the experimental `get_usage` request from the same usage data its own
/// `/usage` view reads, and `skip_behaviors` skips its scan of local transcripts. Any failure,
/// including a CLI that predates the request, leaves the device's last observation in place.
async fn read_plan_limits(
    connection: &ClaudeJsonlConnection,
    context: &DriverOperationContext,
) -> Option<RateLimitStatusEvent> {
    let client = connection.client();
    let request = client.request("get_usage", json!({ "skip_behaviors": true }), context);
    let reply = tokio::time::timeout(USAGE_LIMITS_READ_TIMEOUT, request)
        .await
        .ok()?
        .ok()?;
    usage_reply_status(&reply)
}

impl ClaudeProviderDriver {
    pub fn new(configuration: ProviderInstanceConfig) -> ChatResult<Self> {
        if configuration.family_id.as_str() != "claude" {
            return Err(ChatError::validation(
                "familyId",
                "Claude driver requires the Claude provider family",
            ));
        }
        validate_provider_config(&configuration)?;
        Ok(Self {
            configuration,
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
            maximum_tested_cli_version: Some(CLAUDE_COMPATIBILITY.maximum_tested.to_string()),
            update_command: Some("claude update".to_string()),
            install_guide: crate::cli_install::install_guide("claude"),
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
        read_usage_limits: bool,
    ) -> ChatResult<ClaudeProbeSnapshot> {
        let workspace = crate::probe_directory::ProbeDirectory::create()?;
        let session_uuid = new_uuid(&self.configuration.instance_id);
        let (mut connection, home, version, _) = self
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
                    project_configuration: ProjectConfigurationAccess::Restricted,
                    internal_mcp: None,
                    organizational: None,
                    system_prompt: None,
                    cli_version: None,
                },
            )
            .await?;
        let result = async {
            let initialize = initialize(&connection, context).await?;
            let models = provider_models(initialize.models.clone())?;
            let usage_limits = if read_usage_limits && has_plan(&initialize) {
                read_plan_limits(&connection, context).await
            } else {
                None
            };
            Ok(ClaudeProbeSnapshot {
                version,
                home,
                initialize,
                models,
                usage_limits,
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

    /// Starts the CLI in `working_directory`. An organizational launch whose scope loads the
    /// shell runs its commands under the Ganbaru sandbox when the helper is present and the
    /// CLI version supports it; the returned sandbox must live as long as the process.
    async fn open_connection(
        &self,
        working_directory: &Path,
        options: ClaudeLaunchOptions<'_>,
    ) -> ChatResult<ClaudeOpenedConnection> {
        #[cfg(test)]
        if let Some(factory) = self.connection_factory.as_ref() {
            let (connection, home, version) = factory(working_directory)?;
            return Ok((connection, home, version, None));
        }
        let home = resolve_claude_home(&self.configuration)?;
        let preferences = user_preferences(&home.config_directory);
        let mut environment =
            claude_process_environment(&self.configuration, &home, &preferences.environment)?;
        let executable = resolve_claude_executable(&self.configuration.executable, &environment)?;
        let version =
            cached_probe_version(&executable, working_directory, environment.clone()).await?;
        ensure_supported_version(version)?;
        let server = self.configuration.internal_mcp.as_ref();
        if server.is_some_and(|server| server.organizational_authority) {
            ensure_organizational_version(version)?;
        }
        if let Some(server) = server {
            environment.insert(
                INTERNAL_MCP_TOKEN_ENVIRONMENT.to_string(),
                server.bearer_token.clone(),
            );
        }
        let shell = self.organizational_shell(working_directory, version, options.modes)?;
        if let Some(shell) = shell.as_ref() {
            environment.insert(
                SHELL_PREFIX_ENVIRONMENT.to_string(),
                shell.shell().to_string(),
            );
            let temporary = shell.temporary();
            let temporary = temporary
                .to_str()
                .ok_or_else(|| crate::confinement::sandbox_policy_error(CLAUDE_PROVIDER_NAME))?;
            environment.insert(
                TEMPORARY_DIRECTORY_ENVIRONMENT.to_string(),
                temporary.to_string(),
            );
            let (name, value) = shell.policy_variable(CLAUDE_PROVIDER_NAME)?;
            environment.insert(name, value);
        }
        let arguments = launch_arguments(
            executable.prefix_arguments.clone(),
            &self.configuration.launch_arguments,
            ClaudeLaunchOptions {
                internal_mcp: server.map(|server| ClaudeInternalMcpLaunch {
                    name: &server.name,
                    url: &server.url,
                }),
                organizational: match (server, self.organizational_scope()?) {
                    (Some(server), Some(scope)) => Some(ClaudeOrganizationalLaunch {
                        internal_server_name: &server.name,
                        pre_approved_tools: &server.pre_approved_tools,
                        scope,
                        commands_confined: shell.is_some(),
                        user_settings: &preferences.settings,
                    }),
                    _ => None,
                },
                cli_version: Some(version),
                ..options
            },
        )?;
        let process = spawn_provider_process(ProviderProcessConfig {
            executable: executable.executable,
            arguments,
            working_directory: working_directory.to_path_buf(),
            environment,
            stderr_limit_bytes: CLAUDE_STDERR_LIMIT_BYTES,
            purpose: crate::process::ProviderProcessPurpose::Session,
            provider_instance_label: Some(self.configuration.label.clone()),
        })?;
        Ok((
            ClaudeJsonlConnection::from_process(process)?,
            home,
            version,
            shell.map(Arc::new),
        ))
    }

    /// Prepares the confined shell of an organizational launch whose scope loads the shell,
    /// when the helper is present. Below the CLI version that supports it, the commands stay
    /// unconfined and asked, and a safety mode that needs them confined is refused.
    fn organizational_shell(
        &self,
        working_directory: &Path,
        version: ClaudeVersion,
        modes: TurnModeSnapshot,
    ) -> ChatResult<Option<ShellSandbox>> {
        let Some(scope) = self.organizational_scope()? else {
            return Ok(None);
        };
        let Some(helper) = crate::confinement::sandbox_helper() else {
            return Ok(None);
        };
        if !organizational_runs_commands(scope) {
            return Ok(None);
        }
        if version < MINIMUM_CONFINED_SHELL_CLAUDE_VERSION {
            if modes.safety_mode == SafetyMode::AskForApproval {
                return Ok(None);
            }
            ensure_confined_shell_version(version)?;
        }
        ShellSandbox::prepare(helper, working_directory, &[], CLAUDE_PROVIDER_NAME).map(Some)
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
        let organizational = self.organizational_authority();
        let organizational_scope = self.organizational_scope()?;
        let continuation_group_id = home.continuation_group_with_authority(organizational)?;
        input.verify_continuation(&continuation_group_id)?;
        let cursor = input.resume_cursor(&self.configuration.instance_id)?;
        let model = input.model_id().cloned();
        let effort = selected_effort(input.model_options())?;
        let fast_mode = selected_fast_mode(input.model_options())?;
        let instructions = session_instructions(input.developer_instructions())?
            .map(ClaudeSessionInstructions::prepare)
            .transpose()?;
        let fresh_uuid =
            matches!(&input, SessionOpenInput::Fresh(_)).then_some(cursor.session_uuid.as_str());
        let resume_uuid =
            matches!(&input, SessionOpenInput::Resume(_)).then_some(cursor.session_uuid.as_str());
        let (mut connection, _, _, shell) = self
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
                    project_configuration: input.workspace().project_configuration,
                    internal_mcp: None,
                    organizational: None,
                    cli_version: None,
                    system_prompt: instructions
                        .as_ref()
                        .map(ClaudeSessionInstructions::argument),
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
        let organizational_server = self.organizational_server_name();
        if let Some(expected_server) = organizational_server.as_deref() {
            if let Err(error) =
                verify_mcp_status(&connection.client(), expected_server, context).await
            {
                let _ = connection
                    .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
                    .await;
                return Err(error);
            }
        }
        let models = provider_models(initialize.models.clone())?;
        self.cached_commands = initialize.commands.clone();
        self.cached_models = Some(ProviderModelCatalog {
            instance_id: self.configuration.instance_id.clone(),
            recommended_model_id: recommended_model_id(NativeCompany::Anthropic, &models),
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
        let organizational_permissions = organizational_server
            .as_deref()
            .zip(organizational_scope)
            .map(|(server, scope)| {
                ClaudeOrganizationalPermissions::new(scope, workspace.clone(), server)
                    .with_confined_commands(shell.is_some())
            });
        let (organizational_deadline, organizational_route) = organizational_server
            .zip(organizational_scope)
            .map(|(server, scope)| organizational_route(server, scope, connection.kill_handle()))
            .unzip();
        let router_task = spawn_claude_router(ClaudeRouterResources {
            client: connection.client(),
            inbound,
            normalizer: Arc::clone(&normalizer),
            route: Arc::clone(&route),
            pending_requests: Arc::clone(&pending_requests),
            sink: Arc::clone(&sink),
            expected_shutdown: Arc::clone(&expected_shutdown),
            terminal_error: Arc::clone(&terminal_error),
            organizational: organizational_route,
            organizational_permissions,
            shell: shell.clone(),
            memory_limit: connection.memory_limit_watch(),
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
            organizational: organizational_scope,
            organizational_deadline,
            instructions,
            shell,
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

    /// Whether launches use the organizational surface. Only the internal server's
    /// organizational flag selects it; the server's presence alone never does.
    pub(super) fn organizational_authority(&self) -> bool {
        self.configuration
            .internal_mcp
            .as_ref()
            .is_some_and(|server| server.organizational_authority)
    }

    /// The scope an organizational launch enforces, or none for direct runs.
    ///
    /// # Errors
    ///
    /// Fails closed when the internal server carries organizational authority without a
    /// scope, because the launch could not tell which tool surface to load.
    pub(super) fn organizational_scope(&self) -> ChatResult<Option<ProviderOrganizationalScope>> {
        match self.configuration.internal_mcp.as_ref() {
            Some(server) if server.organizational_authority => server
                .organizational_scope
                .map(Some)
                .ok_or_else(organizational_permission_error),
            _ => Ok(None),
        }
    }

    /// The internal server an organizational session must report, or none for direct runs.
    fn organizational_server_name(&self) -> Option<String> {
        self.configuration
            .internal_mcp
            .as_ref()
            .filter(|server| server.organizational_authority)
            .map(|server| server.name.clone())
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
