//! Cursor provider driver and live-session ownership.

use super::confinement::AcpCallbackConfinement;
use super::executable::*;
use super::interactions::PendingCursorRequests;
use super::normalizer::{CursorEventNormalizer, CursorRouteState};
use super::operations::InstructionsDigest;
use super::protocol::*;
use super::session::*;
use super::transport::AcpRpcConnection;
use crate::events::*;
use crate::models::*;
use crate::{DriverOperationContext, ProviderEventSink};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::task::JoinHandle;

pub const SESSION_GRACEFUL_STOP: Duration = Duration::from_millis(500);
pub const SESSION_FORCE_STOP: Duration = Duration::from_secs(2);
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// The ACP agent a driver speaks to: one with a dedicated integration, or any other agent
/// through the base protocol alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpProviderFlavor {
    Cursor,
    Grok,
    /// An agent the user points at, with no vendor extension, default executable, or version
    /// range.
    Generic,
}

impl AcpProviderFlavor {
    pub fn family_id(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
            Self::Grok => "grok",
            Self::Generic => "acp",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor",
            Self::Grok => "Grok",
            Self::Generic => "ACP agent",
        }
    }

    pub fn metadata_display_name(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor",
            Self::Grok => "xAI",
            Self::Generic => "ACP agent",
        }
    }

    /// The CLI versions this agent supports, or `None` for an agent with no known scheme.
    pub fn compatibility(self) -> Option<crate::registry::ProviderCompatibilityRange> {
        match self {
            Self::Cursor => Some(CURSOR_COMPATIBILITY),
            Self::Grok => Some(GROK_COMPATIBILITY),
            Self::Generic => None,
        }
    }

    /// The command that updates this agent's CLI, or `None` when the user names the program.
    pub fn update_command(self) -> Option<&'static str> {
        match self {
            Self::Cursor => Some("cursor-agent update"),
            Self::Grok => Some("grok update"),
            Self::Generic => None,
        }
    }

    pub fn turn_prefix(self) -> &'static str {
        self.family_id()
    }

    /// Whether the agent serves Ganbaru's structured extensions: question and plan requests,
    /// todo updates, and edit diffs on tool calls.
    pub fn offers_structured_extensions(self) -> bool {
        matches!(self, Self::Cursor | Self::Grok)
    }

    /// Whether the agent streams reasoning as `agent_thought_chunk` from its first turn.
    pub fn streams_reasoning(self) -> bool {
        matches!(self, Self::Cursor | Self::Grok)
    }

    /// Whether the agent takes HTTP MCP servers without advertising `mcpCapabilities.http`.
    ///
    /// Cursor and Grok accept the internal HTTP server and no recorded `initialize` shows whether
    /// they advertise it, so both keep it by flavor; any other agent needs the advertisement.
    pub fn accepts_http_mcp_by_flavor(self) -> bool {
        matches!(self, Self::Cursor | Self::Grok)
    }
}

pub struct CursorProviderDriver {
    pub(super) configuration: ProviderInstanceConfig,
    pub(super) settings: CursorProviderSettings,
    pub(super) flavor: AcpProviderFlavor,
    pub(super) live: Option<CursorLiveSession>,
    pub(super) cached_models: Option<ProviderModelCatalog>,
    /// Capabilities earlier sessions of this driver showed, reported when the next one starts.
    pub(super) observed: ObservedAcpCapabilities,
    #[cfg(test)]
    connection_factory: Option<TestConnectionFactory>,
}

#[cfg(test)]
type TestConnectionFactory =
    Arc<dyn Fn(&Path) -> ChatResult<(AcpRpcConnection, AcpProviderAbout)> + Send + Sync>;

pub(super) struct CursorLiveSession {
    pub connection: AcpRpcConnection,
    pub router_task: JoinHandle<()>,
    pub prompt_task: Option<JoinHandle<()>>,
    pub route: Arc<Mutex<CursorRouteState>>,
    pub setup: Arc<Mutex<AcpSessionSetup>>,
    pub pending: PendingCursorRequests,
    pub prompt_completions: PendingPromptCompletions,
    pub normalizer: Arc<CursorEventNormalizer>,
    pub capabilities: ProviderCapabilities,
    pub sink: Arc<dyn ProviderEventSink>,
    pub expected_shutdown: Arc<AtomicBool>,
    pub terminal_error: Arc<Mutex<Option<ChatError>>>,
    /// Starts the terminal commands the person approves after the router held them.
    pub terminal_callbacks: AcpTerminalCallbacks,
    pub session_id: ProviderSessionId,
    /// Instructions this native session holds: accepted in a prompt of this live session, or
    /// reported as delivered when it was loaded for resume. A later turn with the same
    /// instructions omits them.
    pub delivered_instructions: Arc<Mutex<Option<InstructionsDigest>>>,
}

impl Drop for CursorLiveSession {
    fn drop(&mut self) {
        self.expected_shutdown.store(true, Ordering::Release);
        self.router_task.abort();
        if let Some(task) = self.prompt_task.as_ref() {
            task.abort();
        }
    }
}

pub(super) enum CursorSessionInput {
    Fresh(StartSessionRequest),
    Resume(ResumeSessionRequest),
}

impl CursorSessionInput {
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

    fn provider_instance_id(&self) -> &ProviderInstanceId {
        match self {
            Self::Fresh(request) => &request.provider_instance_id,
            Self::Resume(request) => &request.provider_instance_id,
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

    fn model_options(&self) -> &[ModelOptionSelection] {
        match self {
            Self::Fresh(request) => &request.model_options,
            Self::Resume(_) => &[],
        }
    }

    fn resume_cursor(&self) -> ChatResult<Option<AcpResumeCursor>> {
        match self {
            Self::Fresh(_) => Ok(None),
            Self::Resume(request) => parse_resume_cursor(&request.resume_cursor).map(Some),
        }
    }

    fn expected_continuation(&self) -> Option<&ContinuationGroupId> {
        match self {
            Self::Fresh(_) => None,
            Self::Resume(request) => Some(&request.continuation_group_id),
        }
    }

    fn is_resume(&self) -> bool {
        matches!(self, Self::Resume(_))
    }

    /// Instructions the native session holds when it opens: those of a resume whose caller
    /// reports them already delivered, and none for a new session.
    fn held_instructions(&self) -> Option<InstructionsDigest> {
        match self {
            Self::Fresh(_) => None,
            Self::Resume(request) if request.developer_instructions_delivered => request
                .developer_instructions
                .as_deref()
                .and_then(InstructionsDigest::of_instructions),
            Self::Resume(_) => None,
        }
    }
}

impl CursorProviderDriver {
    pub fn new(configuration: ProviderInstanceConfig) -> ChatResult<Self> {
        Self::new_for(configuration, AcpProviderFlavor::Cursor)
    }

    pub fn new_grok(configuration: ProviderInstanceConfig) -> ChatResult<Self> {
        Self::new_for(configuration, AcpProviderFlavor::Grok)
    }

    /// A driver for any ACP agent, launched from the instance's executable and arguments.
    pub fn new_generic(configuration: ProviderInstanceConfig) -> ChatResult<Self> {
        Self::new_for(configuration, AcpProviderFlavor::Generic)
    }

    fn new_for(
        configuration: ProviderInstanceConfig,
        flavor: AcpProviderFlavor,
    ) -> ChatResult<Self> {
        if configuration.family_id.as_str() != flavor.family_id() {
            return Err(ChatError::validation(
                "familyId",
                format!(
                    "{} driver requires the {} provider family",
                    flavor.display_name(),
                    flavor.display_name()
                ),
            ));
        }
        let settings = CursorProviderSettings::parse_for(&configuration, flavor.display_name())?;
        Ok(Self {
            configuration,
            settings,
            flavor,
            live: None,
            cached_models: None,
            observed: ObservedAcpCapabilities::default(),
            #[cfg(test)]
            connection_factory: None,
        })
    }

    #[cfg(test)]
    pub(super) fn set_connection_factory(&mut self, factory: TestConnectionFactory) {
        self.connection_factory = Some(factory);
    }

    pub fn metadata_read() -> ProviderFamilyMetadataRead {
        Self::metadata_for(AcpProviderFlavor::Cursor)
    }

    pub fn grok_metadata_read() -> ProviderFamilyMetadataRead {
        Self::metadata_for(AcpProviderFlavor::Grok)
    }

    pub fn generic_metadata_read() -> ProviderFamilyMetadataRead {
        Self::metadata_for(AcpProviderFlavor::Generic)
    }

    fn metadata_for(flavor: AcpProviderFlavor) -> ProviderFamilyMetadataRead {
        ProviderFamilyMetadataRead {
            family_id: ProviderFamilyId::new(flavor.family_id())
                .expect("static ACP provider family ID must be valid"),
            display_name: flavor.metadata_display_name().to_string(),
            configuration_schema_version: 1,
            supported_platforms: vec![
                "linux".to_string(),
                "windows".to_string(),
                "macos".to_string(),
            ],
            minimum_tested_cli_version: flavor
                .compatibility()
                .map(|range| range.minimum.to_string()),
            maximum_tested_cli_version: flavor
                .compatibility()
                .map(|range| range.maximum_tested.to_string()),
            update_command: flavor.update_command().map(str::to_string),
            default_executable_candidates: match flavor {
                AcpProviderFlavor::Cursor => {
                    vec!["cursor-agent".to_string(), "agent".to_string()]
                }
                AcpProviderFlavor::Grok => vec!["grok".to_string()],
                // The user names the agent's program; no command is assumed.
                AcpProviderFlavor::Generic => Vec::new(),
            },
            implementation_status: ProviderImplementationStatus::Available,
            maturity: ProviderMaturity::Beta,
            protocol_name: "acp".to_string(),
            potential_capabilities: acp_capability_kinds(flavor),
            unavailable_reason: None,
        }
    }

    pub(super) async fn open_connection(
        &self,
        workspace: &Path,
    ) -> ChatResult<(AcpRpcConnection, AcpProviderAbout)> {
        #[cfg(test)]
        if let Some(factory) = self.connection_factory.as_ref() {
            return factory(workspace);
        }
        let environment = process_environment_for(&self.configuration, self.flavor.display_name())?;
        let executable = resolve_executable(&self.configuration.executable, &environment)?;
        // An organizational launch never runs its agent unconfined in its folder, so the
        // version probe runs in a neutral directory.
        let neutral = if self.flavor != AcpProviderFlavor::Generic
            && self.configuration.internal_mcp.is_some()
        {
            Some(crate::probe_directory::ProbeDirectory::create()?)
        } else {
            None
        };
        let probe_directory = neutral.as_deref().unwrap_or(workspace);
        let about = match self.flavor {
            AcpProviderFlavor::Cursor => {
                let about = probe_about(&executable, probe_directory, environment).await?;
                ensure_supported_version(about.version)?;
                AcpProviderAbout::from(about)
            }
            AcpProviderFlavor::Grok => {
                let about = probe_grok_about(&executable, probe_directory, environment).await?;
                ensure_supported_grok_version(&about.version)?;
                about
            }
            // An arbitrary agent has no known version flag, so its version comes from the
            // `agentInfo` it sends in `initialize`.
            AcpProviderFlavor::Generic => AcpProviderAbout {
                version: String::new(),
                account_label: None,
                authenticated: None,
            },
        };
        let process = spawn_acp_connection_process(
            &self.configuration,
            &self.settings,
            self.flavor,
            workspace,
        )?;
        Ok((AcpRpcConnection::from_process(process)?, about))
    }

    pub(super) async fn probe_snapshot(
        &self,
        context: &DriverOperationContext,
    ) -> ChatResult<(AcpStartedSession, AcpProviderAbout)> {
        let workspace = crate::probe_directory::ProbeDirectory::create()?;
        let (mut connection, about) = self.open_connection(&workspace).await?;
        let mut inbound = match connection.take_inbound() {
            Ok(receiver) => AcpInboundQueue::new(receiver),
            Err(error) => {
                let _ = connection
                    .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
                    .await;
                return Err(error);
            }
        };
        let result = initialize_provider_session(AcpSessionInitialization {
            flavor: self.flavor,
            auth: self.auth_preference(),
            connection: &connection,
            inbound: &mut inbound,
            workspace: workspace.to_string_lossy().as_ref(),
            resume_session_id: None,
            requested: AcpRequestedConfiguration {
                modes: TurnModeSnapshot {
                    safety_mode: SafetyMode::AskForApproval,
                    interaction_mode: InteractionMode::Build,
                },
                model_id: None,
                model_options: &[],
            },
            internal_mcp: None,
            offers_terminal: true,
            context,
        })
        .await;
        let stop = connection
            .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
            .await;
        match result {
            Ok(started) => {
                stop?;
                Ok((started, about))
            }
            Err(error) => Err(error),
        }
    }

    /// Spawns the agent in a probe directory, sends only `initialize`, and stops it.
    ///
    /// No `authenticate`, session, or extension request reaches the agent, so a probe never
    /// creates provider state.
    pub(super) async fn probe_initialize(
        &self,
        context: &DriverOperationContext,
    ) -> ChatResult<(AcpInitializeResponse, AcpProviderAbout)> {
        let workspace = crate::probe_directory::ProbeDirectory::create()?;
        let (mut connection, mut about) = self.open_connection(&workspace).await?;
        // The receiver stays alive until the stop so agent notifications never fail a write.
        let (inbound, result) = match connection.take_inbound() {
            Ok(receiver) => (
                Some(receiver),
                negotiate_initialize(self.flavor, &connection.client(), true, context).await,
            ),
            Err(error) => (None, Err(error)),
        };
        let stop = connection
            .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
            .await;
        drop(inbound);
        let initialize = result?;
        stop?;
        if self.flavor == AcpProviderFlavor::Generic {
            about.version = initialize
                .agent_info
                .as_ref()
                .map(|agent| agent.version.clone())
                .unwrap_or_default();
        }
        Ok((initialize, about))
    }

    pub(super) async fn open_session(
        &mut self,
        input: CursorSessionInput,
        sink: Arc<dyn ProviderEventSink>,
        context: &DriverOperationContext,
    ) -> ChatResult<ProviderSessionSnapshot> {
        if self.live.is_some() {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                format!("{} session is already running", self.flavor.display_name()),
                true,
            ));
        }
        if input.provider_instance_id() != &self.configuration.instance_id {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                format!(
                    "{} provider instance does not match the session request",
                    self.flavor.display_name()
                ),
                false,
            ));
        }
        let workspace = canonical_verified_workspace(input.workspace())?;
        let cursor = input.resume_cursor()?;
        let confinement = AcpCallbackConfinement::for_session(
            self.flavor,
            self.configuration.internal_mcp.as_ref(),
            &workspace,
        )?;
        let offers_terminal = confinement
            .as_ref()
            .is_none_or(AcpCallbackConfinement::offers_terminal);
        let (mut connection, about) = self.open_connection(&workspace).await?;
        let group = acp_continuation_group(
            &self.configuration,
            &self.settings,
            self.flavor,
            about.account_label.as_deref(),
        )?;
        if input
            .expected_continuation()
            .is_some_and(|expected| expected != &group)
        {
            let _ = connection
                .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
                .await;
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                format!(
                    "{} account context change requires a thread fork",
                    self.flavor.display_name()
                ),
                true,
            ));
        }
        let mut inbound = match connection.take_inbound() {
            Ok(receiver) => AcpInboundQueue::new(receiver),
            Err(error) => {
                let _ = connection
                    .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
                    .await;
                return Err(error);
            }
        };
        let started = match initialize_provider_session(AcpSessionInitialization {
            flavor: self.flavor,
            auth: self.auth_preference(),
            connection: &connection,
            inbound: &mut inbound,
            workspace: workspace.to_string_lossy().as_ref(),
            resume_session_id: cursor.as_ref().map(|cursor| cursor.session_id.as_str()),
            requested: AcpRequestedConfiguration {
                modes: input.modes(),
                model_id: input.model_id(),
                model_options: input.model_options(),
            },
            internal_mcp: self.configuration.internal_mcp.as_ref(),
            offers_terminal,
            context,
        })
        .await
        {
            Ok(started) => started,
            Err(error) => {
                let _ = connection
                    .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
                    .await;
                return Err(error);
            }
        };
        let local_session_id = new_session_id_for(self.flavor, &self.configuration.instance_id)?;
        let provider_thread_id = ProviderThreadId::new(started.session_id.clone())
            .map_err(|_| protocol_error("session ID"))?;
        let capabilities = negotiated_capabilities_for(
            self.flavor,
            &started.initialize,
            Some(&started.setup),
            self.observed,
        );
        let effective_model_id = input.model_id().cloned().or_else(|| {
            started
                .setup
                .models
                .as_ref()
                .and_then(|models| ModelId::new(models.current_model_id.clone()).ok())
        });
        let route = Arc::new(Mutex::new(CursorRouteState::new(
            started.session_id.clone(),
            input.modes(),
            effective_model_id.clone(),
            started.setup.config_options.clone(),
            workspace.clone(),
        )));
        let setup = Arc::new(Mutex::new(started.setup));
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let prompt_completions = Arc::new(Mutex::new(HashMap::new()));
        let normalizer = Arc::new(CursorEventNormalizer::new_for_provider(
            self.configuration.instance_id.clone(),
            input.thread_id().clone(),
            local_session_id.clone(),
            self.flavor.family_id(),
            self.flavor.display_name(),
        ));
        let expected_shutdown = Arc::new(AtomicBool::new(false));
        let terminal_error = Arc::new(Mutex::new(None));
        let terminal_callbacks =
            AcpTerminalCallbacks::new(workspace.clone(), started.session_id.clone())
                .with_confinement(confinement);
        let router_task = spawn_cursor_router(CursorRouterResources {
            client: connection.client(),
            inbound,
            normalizer: Arc::clone(&normalizer),
            route: Arc::clone(&route),
            pending: Arc::clone(&pending),
            sink: Arc::clone(&sink),
            expected_shutdown: Arc::clone(&expected_shutdown),
            terminal_error: Arc::clone(&terminal_error),
            flavor: self.flavor,
            prompt_completions: Arc::clone(&prompt_completions),
            terminal_callbacks: terminal_callbacks.clone(),
            memory_limit: connection.memory_limit_watch(),
        });
        let state = route.lock().map_err(|_| driver_state_error())?.clone();
        let cursor = resume_cursor(&started.session_id);
        sink.emit(normalizer.external_event(
            &state,
            if input.is_resume() {
                "session/resumed"
            } else {
                "session/started"
            },
            None,
            CanonicalEvent::SessionStarted(SessionStartedEvent {
                session_id: local_session_id.clone(),
                state: ProviderSessionState::Ready,
                provider_thread_id: Some(provider_thread_id.clone()),
                resume_cursor: Some(cursor.clone()),
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
                session_id: local_session_id.clone(),
                effective_modes: input.modes(),
                effective_model_id,
                effective_model_options: input.model_options().to_vec(),
            }),
        )?)
        .await?;
        if started.host_tools_omitted {
            sink.emit(normalizer.external_event(
                &state,
                "session/mcp",
                None,
                CanonicalEvent::RuntimeWarning(NotificationEvent {
                    code: "acp_host_tools_unavailable".to_string(),
                    title: format!(
                        "{} runs without Ganbaru host tools",
                        self.flavor.display_name()
                    ),
                    detail: Some("The agent did not advertise HTTP MCP servers".to_string()),
                }),
            )?)
            .await?;
        }
        self.cached_models = Some(ProviderModelCatalog {
            instance_id: self.configuration.instance_id.clone(),
            models: started.models,
            source: ModelCatalogSource::Provider,
            discovered_at: now_utc()?,
            stale: false,
        });
        let started_at = now_utc()?;
        self.live = Some(CursorLiveSession {
            connection,
            router_task,
            prompt_task: None,
            route,
            setup,
            pending,
            prompt_completions,
            normalizer,
            capabilities: capabilities.clone(),
            sink,
            expected_shutdown,
            terminal_error,
            terminal_callbacks,
            session_id: local_session_id.clone(),
            delivered_instructions: Arc::new(Mutex::new(input.held_instructions())),
        });
        Ok(ProviderSessionSnapshot {
            session_id: local_session_id,
            state: ProviderSessionState::Ready,
            provider_thread_id: Some(provider_thread_id),
            continuation_group_id: group,
            resume_cursor: Some(cursor),
            effective_modes: input.modes(),
            capabilities,
            started_at,
        })
    }

    /// How this instance picks an advertised authentication method.
    pub(super) fn auth_preference(&self) -> AcpAuthPreference<'_> {
        auth_preference(
            self.flavor,
            self.grok_uses_api_key(),
            self.settings.auth_method.as_deref(),
        )
    }

    /// Whether this Grok instance authenticates with the xAI API key its configured
    /// environment supplies, which a credential reference or an inherited entry resolves into
    /// before the driver starts.
    pub(super) fn grok_uses_api_key(&self) -> bool {
        self.flavor == AcpProviderFlavor::Grok
            && grok_api_key_configured(&self.configuration.environment)
    }

    /// How this instance's sessions authenticate, for the flavors whose source is known.
    pub(super) fn auth_source(&self) -> Option<ProviderAuthSource> {
        match self.flavor {
            AcpProviderFlavor::Grok => Some(Self::grok_auth_source(&self.configuration)),
            AcpProviderFlavor::Cursor | AcpProviderFlavor::Generic => None,
        }
    }

    /// How a Grok instance authenticates once its configuration is resolved for launch.
    pub fn grok_auth_source(configuration: &ProviderInstanceConfig) -> ProviderAuthSource {
        if grok_api_key_configured(&configuration.environment) {
            ProviderAuthSource::ApiKey
        } else {
            ProviderAuthSource::AccountLogin
        }
    }

    pub(super) fn live_mut(
        &mut self,
        session_id: &ProviderSessionId,
    ) -> ChatResult<&mut CursorLiveSession> {
        let provider_name = self.flavor.display_name();
        let live = self.live.as_mut().ok_or_else(|| {
            ChatError::driver_unavailable(format!("{provider_name} session is not running"))
        })?;
        if &live.session_id != session_id {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                format!("{provider_name} session identity does not match the active session"),
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

fn canonical_verified_workspace(input: &VerifiedWorkspaceContext) -> ChatResult<PathBuf> {
    let path = PathBuf::from(&input.canonical_path);
    if !path.is_absolute() || !path.is_dir() {
        return Err(ChatError::validation(
            "workspace.canonicalPath",
            "ACP provider workspace is unavailable",
        ));
    }
    std::fs::canonicalize(path).map_err(|_| {
        ChatError::validation(
            "workspace.canonicalPath",
            "ACP provider workspace is unavailable",
        )
    })
}

pub(super) fn potential_capabilities_for(flavor: AcpProviderFlavor) -> ProviderCapabilities {
    ProviderCapabilities {
        entries: acp_capability_kinds(flavor)
            .into_iter()
            .map(|capability| ProviderCapabilitySupport {
                capability,
                supported: true,
                explanation: None,
            })
            .collect(),
    }
}

pub(super) fn new_session_id_for(
    flavor: AcpProviderFlavor,
    instance: &ProviderInstanceId,
) -> ChatResult<ProviderSessionId> {
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| driver_state_error())?
        .as_nanos();
    ProviderSessionId::new(format!(
        "{}-{}-{}-{created}-{}",
        flavor.family_id(),
        instance.as_str(),
        std::process::id(),
        NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
    ))
    .map_err(|_| protocol_error("local session ID"))
}

pub(super) fn now_utc() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .map_err(|_| protocol_error("timestamp"))
}
