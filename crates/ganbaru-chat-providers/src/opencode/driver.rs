//! OpenCode driver state, probes, session lifecycle, and event routing.

use super::cli::*;
use super::config::*;
use super::event_stream::OpenCodeEventStream;
use super::http_client::*;
use super::local_server::OwnedOpenCodeServer;
use super::normalizer::{OpenCodeEventNormalizer, OpenCodeRouteState};
use super::permissions::{
    OpenCodeOrganizationalPolicy, OpenCodePermissionRule, organizational_permission_error,
    organizational_rules, permission_override, permission_reply,
};
use super::protocol::{OpenCodeCommand, parse_resume_cursor, resume_cursor};
use super::support::*;
use crate::events::*;
use crate::models::*;
use crate::{DriverOperationContext, ProviderEventSink};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::task::JoinHandle;

const MAX_RECONNECT_ATTEMPTS: usize = 6;
/// Longest wait for any event, heartbeats included, while a turn runs and no request waits for
/// the user before the stream counts as silent and is reconnected. OpenCode heartbeat timing is
/// unconfirmed, so the bound is wide and idle sessions are never timed out.
pub(super) const OPENCODE_BUSY_STREAM_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_RECONCILIATION_PAGES: usize = 100;
const MAX_RECONCILIATION_MESSAGES: usize = 10_000;

pub struct OpenCodeProviderDriver {
    pub(super) configuration: ProviderInstanceConfig,
    pub(super) settings: OpenCodeProviderSettings,
    /// Password configured for an external server; owned servers generate their own.
    pub(super) external_password: Option<OpenCodeSecret>,
    pub(super) live: Option<OpenCodeLiveSession>,
    pub(super) cached_models: Option<ProviderModelCatalog>,
    pub(super) cached_commands: Vec<OpenCodeCommand>,
    /// Silence bound of the event stream during a turn, [`OPENCODE_BUSY_STREAM_TIMEOUT`] outside
    /// tests.
    pub(super) busy_stream_timeout: Duration,
}

pub(super) struct OpenCodeLiveSession {
    pub client: OpenCodeHttpClient,
    pub owned_server: Option<OwnedOpenCodeServer>,
    pub event_task: JoinHandle<()>,
    pub route: Arc<Mutex<OpenCodeRouteState>>,
    pub normalizer: Arc<OpenCodeEventNormalizer>,
    pub sink: Arc<dyn ProviderEventSink>,
    pub expected_shutdown: Arc<AtomicBool>,
    pub terminal_error: Arc<Mutex<Option<ChatError>>>,
    pub session_id: ProviderSessionId,
    pub provider_thread_id: String,
    pub command_task: Option<JoinHandle<()>>,
    /// The session rules last sent to an organizational session, which each turn re-derives
    /// from its modes; none for a direct session.
    pub organizational_rules: Option<Vec<OpenCodePermissionRule>>,
}

impl Drop for OpenCodeLiveSession {
    fn drop(&mut self) {
        self.expected_shutdown.store(true, Ordering::Release);
        self.event_task.abort();
        if let Some(task) = self.command_task.as_ref() {
            task.abort();
        }
    }
}

pub(super) enum OpenCodeSessionInput {
    Fresh(StartSessionRequest),
    Resume(ResumeSessionRequest),
}

impl OpenCodeSessionInput {
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

    fn resume_session_id(&self) -> ChatResult<Option<String>> {
        match self {
            Self::Fresh(_) => Ok(None),
            Self::Resume(request) => {
                let cursor = parse_resume_cursor(&request.resume_cursor)?;
                if cursor.session_id != request.provider_thread_id.as_str() {
                    return Err(ChatError::validation(
                        "resumeCursor",
                        "OpenCode resume cursor does not match the provider session",
                    ));
                }
                Ok(Some(cursor.session_id))
            }
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
}

pub(super) struct OpenCodeCatalogSnapshot {
    pub version: Option<String>,
    pub account_label: Option<String>,
    pub models: Vec<ProviderModel>,
    pub commands: Vec<OpenCodeCommand>,
    pub toolchain_detail: String,
}

impl OpenCodeProviderDriver {
    pub fn new(mut configuration: ProviderInstanceConfig) -> ChatResult<Self> {
        if configuration.family_id.as_str() != "opencode" {
            return Err(ChatError::validation(
                "familyId",
                "OpenCode driver requires the OpenCode provider family",
            ));
        }
        let settings = OpenCodeProviderSettings::parse(&configuration)?;
        let password = take_server_password(&mut configuration);
        Ok(Self {
            configuration,
            settings,
            external_password: password,
            live: None,
            cached_models: None,
            cached_commands: Vec::new(),
            busy_stream_timeout: OPENCODE_BUSY_STREAM_TIMEOUT,
        })
    }

    /// Shortens the silence bound so tests can observe a silent stream in real time.
    #[cfg(test)]
    pub(super) fn with_busy_stream_timeout(mut self, timeout: Duration) -> Self {
        self.busy_stream_timeout = timeout;
        self
    }

    pub fn metadata_read() -> ProviderFamilyMetadataRead {
        ProviderFamilyMetadataRead {
            family_id: ProviderFamilyId::new("opencode")
                .expect("static OpenCode family ID must be valid"),
            display_name: "OpenCode".to_string(),
            configuration_schema_version: 1,
            supported_platforms: vec![
                "linux".to_string(),
                "windows".to_string(),
                "macos".to_string(),
            ],
            minimum_tested_cli_version: Some(MINIMUM_OPENCODE_VERSION.to_string()),
            maximum_tested_cli_version: Some(OPENCODE_COMPATIBILITY.maximum_tested.to_string()),
            default_executable_candidates: vec!["opencode".to_string()],
            implementation_status: ProviderImplementationStatus::Available,
            maturity: ProviderMaturity::Beta,
            protocol_name: "opencode-openapi".to_string(),
            potential_capabilities: capability_kinds(),
            unavailable_reason: None,
        }
    }

    pub(super) async fn catalog_snapshot(
        &self,
        workspace: &Path,
    ) -> ChatResult<OpenCodeCatalogSnapshot> {
        match &self.settings.connection {
            OpenCodeConnectionMode::Local => {
                let environment = process_environment(&self.configuration)?;
                let executable = resolve_executable(&self.configuration.executable, &environment)?;
                let version_output = run_command(
                    &executable.executable,
                    &["--version"],
                    workspace,
                    &environment,
                )
                .await?;
                if !version_output.successful {
                    return Err(ChatError::new(
                        ChatErrorCode::TransportUnavailable,
                        "OpenCode version probe failed",
                        true,
                    ));
                }
                let version = parse_version(&String::from_utf8_lossy(&version_output.stdout))?;
                ensure_supported_version(version)?;
                let models_output = run_command(
                    &executable.executable,
                    &["models", "--verbose"],
                    workspace,
                    &environment,
                )
                .await?;
                if !models_output.successful {
                    return Err(ChatError::new(
                        ChatErrorCode::AuthenticationRequired,
                        "OpenCode model discovery failed; verify provider authentication",
                        true,
                    ));
                }
                let agents_output = run_command(
                    &executable.executable,
                    &["agent", "list"],
                    workspace,
                    &environment,
                )
                .await?;
                if !agents_output.successful {
                    return Err(ChatError::new(
                        ChatErrorCode::Protocol,
                        "OpenCode agent discovery failed",
                        true,
                    ));
                }
                let models = parse_models_cli_output(&models_output.stdout)?;
                let agents = parse_agents_cli_output(&agents_output.stdout)?;
                let mut server = OwnedOpenCodeServer::start(
                    &self.configuration,
                    workspace,
                    ProjectConfigurationAccess::Restricted,
                )
                .await?;
                let server_probe = async {
                    let client = OpenCodeHttpClient::new(
                        &server.origin,
                        workspace,
                        Some(server.password()),
                    )?;
                    client.health().await?;
                    let commands = client.commands().await?;
                    let lsp = client.lsp_status().await?;
                    let formatters = client.formatter_status().await?;
                    Ok((commands, lsp, formatters))
                }
                .await;
                let stop_result = server.stop().await;
                let (commands, lsp, formatters) = server_probe?;
                stop_result?;
                Ok(OpenCodeCatalogSnapshot {
                    version: Some(version.to_string()),
                    account_label: None,
                    models: provider_models(&models, &agents)?,
                    commands,
                    toolchain_detail: toolchain_detail(&lsp, &formatters),
                })
            }
            OpenCodeConnectionMode::External { origin, .. } => {
                self.require_external_workspace_confirmation()?;
                let client =
                    OpenCodeHttpClient::new(origin, workspace, self.external_password.as_ref())?;
                let health = client.health().await?;
                let inventory = client.provider_inventory().await?;
                let agents = client.agents().await?;
                let commands = client.commands().await?;
                let lsp = client.lsp_status().await?;
                let formatters = client.formatter_status().await?;
                let models = parse_provider_inventory(&inventory)?;
                let agents = parse_agent_inventory(&agents)?;
                Ok(OpenCodeCatalogSnapshot {
                    version: health
                        .as_object()
                        .and_then(|object| object.get("version"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    account_label: connected_account_label(&inventory),
                    models: provider_models(&models, &agents)?,
                    commands,
                    toolchain_detail: toolchain_detail(&lsp, &formatters),
                })
            }
        }
    }

    pub(super) async fn open_session(
        &mut self,
        input: OpenCodeSessionInput,
        sink: Arc<dyn ProviderEventSink>,
        _context: &DriverOperationContext,
    ) -> ChatResult<ProviderSessionSnapshot> {
        if self.live.is_some() {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "OpenCode session is already running",
                true,
            ));
        }
        if input.provider_instance_id() != &self.configuration.instance_id {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "OpenCode provider instance does not match the session request",
                false,
            ));
        }
        let workspace = canonical_workspace(input.workspace())?;
        self.require_external_workspace_confirmation()?;
        let organizational_scope = self.organizational_scope()?;
        let organizational_rules = organizational_scope
            .map(|scope| self.organizational_rules_for(scope, input.modes()))
            .transpose()?;
        let continuation_group_id = continuation_group(&self.settings, None)?;
        if input
            .expected_continuation()
            .is_some_and(|expected| expected != &continuation_group_id)
        {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "OpenCode server identity change requires a thread fork",
                true,
            ));
        }
        let resume_session_id = input.resume_session_id()?;
        let mut owned_server = match self.settings.connection {
            OpenCodeConnectionMode::Local => Some(
                OwnedOpenCodeServer::start(
                    &self.configuration,
                    &workspace,
                    input.workspace().project_configuration,
                )
                .await?,
            ),
            OpenCodeConnectionMode::External { .. } => None,
        };
        let origin = owned_server
            .as_ref()
            .map(|server| server.origin.as_str())
            .or_else(|| self.settings.server_origin())
            .ok_or_else(driver_state_error)?;
        let password = server_password(&owned_server, self.external_password.as_ref());
        let client = OpenCodeHttpClient::new(origin, &workspace, password)?;
        if organizational_rules.is_some() {
            let admitted = match client.health().await {
                Ok(health) => ensure_organizational_version(&health),
                Err(error) => Err(error),
            };
            if let Err(error) = admitted {
                stop_owned_server(&mut owned_server).await;
                return Err(error);
            }
        }
        if registers_internal_mcp(&self.settings.connection, password) {
            if let Some(server) = &self.configuration.internal_mcp {
                if let Err(error) = client
                    .add_mcp_server(&server.name, &server.url, &server.bearer_token)
                    .await
                {
                    stop_owned_server(&mut owned_server).await;
                    return Err(error);
                }
            }
        }
        let commands = client.commands().await?;
        self.cached_commands = commands;
        let initial_events = match client.subscribe_events().await {
            Ok(response) => response,
            Err(error) => {
                stop_owned_server(&mut owned_server).await;
                return Err(error);
            }
        };
        let rules = organizational_rules
            .clone()
            .or_else(|| permission_override(input.modes().safety_mode));
        let resolved = match resolve_native_session(
            &client,
            resume_session_id.as_deref(),
            &workspace,
            rules.as_deref(),
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                stop_owned_server(&mut owned_server).await;
                return Err(error);
            }
        };
        let provider_thread_id = session_id(&resolved)?;
        let local_session_id = new_local_session_id(&self.configuration.instance_id)?;
        let provider_thread = ProviderThreadId::new(provider_thread_id.clone())
            .map_err(|_| super::protocol::protocol_error("session ID"))?;
        let mut route_state = OpenCodeRouteState::new(provider_thread_id.clone(), input.modes());
        if let (Some(scope), Some(server)) = (
            organizational_scope,
            self.configuration.internal_mcp.as_ref(),
        ) {
            route_state = route_state
                .with_organizational(OpenCodeOrganizationalPolicy::new(scope, &server.name));
        }
        let route = Arc::new(Mutex::new(route_state));
        let normalizer = Arc::new(OpenCodeEventNormalizer::new(
            self.configuration.instance_id.clone(),
            input.thread_id().clone(),
            local_session_id.clone(),
        ));
        let expected_shutdown = Arc::new(AtomicBool::new(false));
        let terminal_error = Arc::new(Mutex::new(None));
        let state = route.lock().map_err(|_| driver_state_error())?.clone();
        let cursor = resume_cursor(&provider_thread_id)?;
        let capabilities = capabilities();
        sink.emit(normalizer.external_event(
            &state,
            if input.is_resume() {
                "session/resumed"
            } else {
                "session/started"
            },
            CanonicalEvent::SessionStarted(SessionStartedEvent {
                session_id: local_session_id.clone(),
                state: ProviderSessionState::Ready,
                provider_thread_id: Some(provider_thread.clone()),
                resume_cursor: Some(cursor.clone()),
                effective_modes: input.modes(),
                capability_overrides: capabilities.clone(),
            }),
        )?)
        .await?;
        sink.emit(normalizer.external_event(
            &state,
            "session/configured",
            CanonicalEvent::SessionConfigured(SessionConfiguredEvent {
                session_id: local_session_id.clone(),
                effective_modes: input.modes(),
                effective_model_id: input.model_id().cloned(),
                effective_model_options: input.model_options().to_vec(),
            }),
        )?)
        .await?;
        let started_at = now_utc()?;
        let event_task = spawn_event_pump(EventPumpResources {
            client: client.clone(),
            initial_stream: Some(OpenCodeEventStream::new(initial_events)),
            route: Arc::clone(&route),
            normalizer: Arc::clone(&normalizer),
            sink: Arc::clone(&sink),
            expected_shutdown: Arc::clone(&expected_shutdown),
            terminal_error: Arc::clone(&terminal_error),
            session_id: provider_thread_id.clone(),
            busy_stream_timeout: self.busy_stream_timeout,
        });
        self.live = Some(OpenCodeLiveSession {
            client,
            owned_server,
            event_task,
            route,
            normalizer,
            sink,
            expected_shutdown,
            terminal_error,
            session_id: local_session_id.clone(),
            provider_thread_id,
            command_task: None,
            organizational_rules,
        });
        Ok(ProviderSessionSnapshot {
            session_id: local_session_id,
            state: ProviderSessionState::Ready,
            provider_thread_id: Some(provider_thread),
            continuation_group_id,
            resume_cursor: Some(cursor),
            effective_modes: input.modes(),
            capabilities,
            started_at,
        })
    }

    pub(super) async fn fork_native_session(
        &mut self,
        request: ProviderForkThreadRequest,
    ) -> ChatResult<ProviderThreadId> {
        let workspace = canonical_workspace(&request.workspace)?;
        self.require_external_workspace_confirmation()?;
        let mut owned_server = match self.settings.connection {
            OpenCodeConnectionMode::Local => Some(
                OwnedOpenCodeServer::start(
                    &self.configuration,
                    &workspace,
                    request.workspace.project_configuration,
                )
                .await?,
            ),
            OpenCodeConnectionMode::External { .. } => None,
        };
        let origin = owned_server
            .as_ref()
            .map(|server| server.origin.as_str())
            .or_else(|| self.settings.server_origin())
            .ok_or_else(driver_state_error)?;
        let result = async {
            let client = OpenCodeHttpClient::new(
                origin,
                &workspace,
                server_password(&owned_server, self.external_password.as_ref()),
            )?;
            let session = client
                .fork_session(
                    request.provider_thread_id.as_str(),
                    request.last_provider_turn_id.as_deref(),
                )
                .await?;
            ProviderThreadId::new(session_id(&session)?)
                .map_err(|_| super::protocol::protocol_error("forked session ID"))
        }
        .await;
        stop_owned_server(&mut owned_server).await;
        let provider_thread_id = result?;
        Ok(provider_thread_id)
    }

    pub(super) fn live_mut(
        &mut self,
        session_id: &ProviderSessionId,
    ) -> ChatResult<&mut OpenCodeLiveSession> {
        let live = self
            .live
            .as_mut()
            .ok_or_else(|| ChatError::driver_unavailable("OpenCode session is not running"))?;
        if &live.session_id != session_id {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "OpenCode session identity does not match the active session",
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

    /// The scope an organizational launch enforces, or none for direct runs. Only the internal
    /// server's organizational flag selects it, and a flagged server without a scope fails
    /// closed.
    pub(super) fn organizational_scope(&self) -> ChatResult<Option<ProviderOrganizationalScope>> {
        match self.configuration.internal_mcp.as_ref() {
            Some(server) if server.organizational_authority => server
                .organizational_scope
                .map(Some)
                .ok_or_else(organizational_permission_error),
            _ => Ok(None),
        }
    }

    /// The session rules of an organizational run in `scope` under `modes`.
    pub(super) fn organizational_rules_for(
        &self,
        scope: ProviderOrganizationalScope,
        modes: TurnModeSnapshot,
    ) -> ChatResult<Vec<OpenCodePermissionRule>> {
        let server = self
            .configuration
            .internal_mcp
            .as_ref()
            .ok_or_else(organizational_permission_error)?;
        organizational_rules(scope, modes, &server.name, &server.pre_approved_tools)
    }

    fn require_external_workspace_confirmation(&self) -> ChatResult<()> {
        if self.settings.is_external() && !self.settings.external_workspace_access_confirmed {
            return Err(ChatError::validation(
                "providerConfig.confirmExternalWorkspaceAccess",
                "Confirm before sending the local workspace path to the external OpenCode server",
            ));
        }
        Ok(())
    }
}

fn toolchain_detail(lsp: &Value, formatters: &Value) -> String {
    fn entry_count(value: &Value) -> usize {
        value
            .as_array()
            .map(Vec::len)
            .or_else(|| value.as_object().map(serde_json::Map::len))
            .unwrap_or(0)
    }
    format!(
        "OpenCode language servers: {}; formatters: {}",
        entry_count(lsp),
        entry_count(formatters)
    )
}

async fn resolve_native_session(
    client: &OpenCodeHttpClient,
    resume_session_id: Option<&str>,
    workspace: &Path,
    rules: Option<&[super::permissions::OpenCodePermissionRule]>,
) -> ChatResult<Value> {
    if let Some(resume_session_id) = resume_session_id {
        match client.session(resume_session_id).await? {
            OpenCodeSessionLookup::Found(session) => {
                let session = if session_directory(&session)
                    .is_some_and(|directory| !same_directory(directory, workspace))
                {
                    client.fork_session(resume_session_id, None).await?
                } else {
                    session
                };
                let session_id = session_id(&session)?;
                return match rules {
                    Some(rules) => client.update_permission(&session_id, rules).await,
                    None => Ok(session),
                };
            }
            OpenCodeSessionLookup::NotFound => {}
        }
    }
    client.create_session(rules).await
}

struct EventPumpResources {
    client: OpenCodeHttpClient,
    initial_stream: Option<OpenCodeEventStream>,
    route: Arc<Mutex<OpenCodeRouteState>>,
    normalizer: Arc<OpenCodeEventNormalizer>,
    sink: Arc<dyn ProviderEventSink>,
    expected_shutdown: Arc<AtomicBool>,
    terminal_error: Arc<Mutex<Option<ChatError>>>,
    session_id: String,
    busy_stream_timeout: Duration,
}

/// Why the event pump left a stream to reconnect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamInterruption {
    Disconnected,
    Silent,
}

impl StreamInterruption {
    fn code(self) -> &'static str {
        match self {
            Self::Disconnected => "opencode_event_stream_disconnected",
            Self::Silent => "opencode_event_stream_silent",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::Disconnected => "OpenCode event stream disconnected",
            Self::Silent => "OpenCode event stream stayed silent during a turn",
        }
    }
}

/// What one wait on the event stream produced.
enum StreamPoll {
    Event(Value),
    Interrupted(StreamInterruption),
    /// The wait ran out while no turn was running or a request waited for the user; the stream
    /// is kept.
    IdleWait,
}

/// Waits for the next event. A wait that began during a turn, with no request waiting for the
/// user, and outlasts the busy timeout counts as a silent stream. Any other wait just restarts,
/// so a turn starting or a request being answered during it is detected within twice the timeout.
async fn next_stream_event(
    resources: &EventPumpResources,
    stream: &mut OpenCodeEventStream,
) -> StreamPoll {
    let busy = resources
        .route
        .lock()
        .is_ok_and(|state| state.active_turn_id.is_some() && !state.awaiting_user());
    match tokio::time::timeout(resources.busy_stream_timeout, stream.next()).await {
        Ok(Ok(Some(envelope))) => StreamPoll::Event(envelope),
        Ok(Ok(None) | Err(_)) => StreamPoll::Interrupted(StreamInterruption::Disconnected),
        Err(_) if busy => StreamPoll::Interrupted(StreamInterruption::Silent),
        Err(_) => StreamPoll::IdleWait,
    }
}

/// Whether an envelope shows the stream carries session activity again. OpenCode sends
/// `server.connected` on every subscription, so it does not end a reconnect series.
fn resets_reconnects(envelope: &Value) -> bool {
    envelope.get("type").and_then(Value::as_str) != Some("server.connected")
}

fn spawn_event_pump(mut resources: EventPumpResources) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Some(mut stream) = resources.initial_stream.take() else {
            return;
        };
        let mut reconnect_attempt = 0_usize;
        loop {
            match next_stream_event(&resources, &mut stream).await {
                StreamPoll::Event(envelope) => {
                    if resets_reconnects(&envelope) {
                        reconnect_attempt = 0;
                    }
                    let events = resources
                        .route
                        .lock()
                        .map_err(|_| driver_state_error())
                        .and_then(|mut state| resources.normalizer.normalize(&mut state, envelope));
                    let events = match events {
                        Ok(events) => {
                            let events = answer_from_session_rules(&resources, events).await;
                            match reject_outside_scope(&resources, events).await {
                                Ok(events) => reject_unprompted_questions(&resources, events).await,
                                Err(error) => Err(error),
                            }
                        }
                        Err(error) => Err(error),
                    };
                    if emit_events(&resources.sink, events).await.is_err() {
                        settle_event_failure(
                            &resources,
                            StreamInterruption::Disconnected.code(),
                            "OpenCode event normalization or persistence failed",
                        )
                        .await;
                        return;
                    }
                }
                StreamPoll::IdleWait => {}
                StreamPoll::Interrupted(interruption) => {
                    if resources.expected_shutdown.load(Ordering::Acquire) {
                        return;
                    }
                    reconnect_attempt += 1;
                    if reconnect_attempt > MAX_RECONNECT_ATTEMPTS {
                        settle_event_failure(
                            &resources,
                            interruption.code(),
                            interruption.message(),
                        )
                        .await;
                        return;
                    }
                    if reconcile_messages(&resources).await.is_err() {
                        tokio::time::sleep(reconnect_delay(reconnect_attempt)).await;
                    }
                    match resources.client.subscribe_events().await {
                        Ok(response) => stream = OpenCodeEventStream::new(response),
                        Err(_) => {
                            tokio::time::sleep(reconnect_delay(reconnect_attempt)).await;
                        }
                    }
                }
            }
        }
    })
}

/// Rejects at OpenCode the questions it asked while no turn ran, so its run does not wait on an
/// answer no one was asked for.
///
/// A rejection OpenCode refuses adds a warning, because the question then stays pending there.
async fn reject_unprompted_questions(
    resources: &EventPumpResources,
    mut events: Vec<CanonicalRuntimeEvent>,
) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
    let questions = resources
        .route
        .lock()
        .map_err(|_| driver_state_error())?
        .take_unprompted_questions();
    for request_id in questions {
        if let Err(error) = resources.client.reject_question(request_id.as_str()).await {
            let state = resources.route.lock().map_err(|_| driver_state_error())?;
            events.push(resources.normalizer.external_event(
                &state,
                "question/reject",
                CanonicalEvent::RuntimeWarning(NotificationEvent {
                    code: "opencode_question_reject_failed".to_string(),
                    title: "OpenCode kept a question asked outside a turn".to_string(),
                    detail: Some(error.message),
                }),
            )?);
        }
    }
    Ok(events)
}

/// Rejects at OpenCode the permission requests an organizational scope cannot need, which never
/// reach the person.
///
/// A rejection OpenCode refuses adds a warning, because the tool call then waits there.
async fn reject_outside_scope(
    resources: &EventPumpResources,
    mut events: Vec<CanonicalRuntimeEvent>,
) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
    let rejections = resources
        .route
        .lock()
        .map_err(|_| driver_state_error())?
        .take_auto_rejections();
    for request_id in rejections {
        if let Err(error) = resources
            .client
            .reply_permission(
                request_id.as_str(),
                permission_reply(ApprovalDecisionKind::Deny),
            )
            .await
        {
            let state = resources.route.lock().map_err(|_| driver_state_error())?;
            events.push(resources.normalizer.external_event(
                &state,
                "permission/reject",
                CanonicalEvent::RuntimeWarning(NotificationEvent {
                    code: "opencode_scope_reject_failed".to_string(),
                    title: "OpenCode kept a request outside the teammate scope".to_string(),
                    detail: Some(error.message),
                }),
            )?);
        }
    }
    Ok(events)
}

/// Sends the `once` reply for requests a session rule covered.
///
/// A request whose reply OpenCode refuses loses its resolution event and stays open for the user.
async fn answer_from_session_rules(
    resources: &EventPumpResources,
    mut events: Vec<CanonicalRuntimeEvent>,
) -> Vec<CanonicalRuntimeEvent> {
    let answers = match resources.route.lock() {
        Ok(mut state) => state.take_auto_answers(),
        Err(_) => return events,
    };
    for request_id in answers {
        let accepted = resources
            .client
            .reply_permission(
                request_id.as_str(),
                permission_reply(ApprovalDecisionKind::AllowOnce),
            )
            .await
            .is_ok();
        if let Ok(mut state) = resources.route.lock() {
            if accepted {
                state.complete_auto_answer(&request_id);
            } else {
                state.release_auto_answer(&request_id);
            }
        }
        if !accepted {
            events.retain(|event| {
                !matches!(
                    &event.event,
                    CanonicalEvent::RequestResolved(resolved) if resolved.request_id == request_id
                )
            });
        }
    }
    events
}

async fn reconcile_messages(resources: &EventPumpResources) -> ChatResult<()> {
    let mut cursor = None;
    let mut pages = 0_usize;
    let mut messages = Vec::new();
    loop {
        pages += 1;
        if pages > MAX_RECONCILIATION_PAGES {
            return Err(super::protocol::protocol_error("history page count"));
        }
        let page = resources
            .client
            .messages(&resources.session_id, 1_000, cursor.as_deref())
            .await?;
        if page.messages.len() > MAX_RECONCILIATION_MESSAGES.saturating_sub(messages.len()) {
            return Err(super::protocol::protocol_error("history message count"));
        }
        messages.extend(page.messages);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    messages.reverse();
    for message in messages {
        for envelope in reconciliation_envelopes(&resources.session_id, message)? {
            let events = resources
                .route
                .lock()
                .map_err(|_| driver_state_error())
                .and_then(|mut state| resources.normalizer.normalize(&mut state, envelope));
            emit_events(&resources.sink, events).await?;
        }
    }
    Ok(())
}

fn reconciliation_envelopes(session_id: &str, message: Value) -> ChatResult<Vec<Value>> {
    let object = message
        .as_object()
        .ok_or_else(|| super::protocol::protocol_error("history message"))?;
    let info = object
        .get("info")
        .cloned()
        .ok_or_else(|| super::protocol::protocol_error("history message info"))?;
    let parts = object
        .get("parts")
        .and_then(Value::as_array)
        .ok_or_else(|| super::protocol::protocol_error("history message parts"))?;
    let mut envelopes = vec![json!({
        "type": "message.updated",
        "properties": { "sessionID": session_id, "info": info }
    })];
    envelopes.extend(parts.iter().map(|part| {
        json!({
            "type": "message.part.updated",
            "properties": { "sessionID": session_id, "part": part }
        })
    }));
    Ok(envelopes)
}

async fn emit_events(
    sink: &Arc<dyn ProviderEventSink>,
    events: ChatResult<Vec<CanonicalRuntimeEvent>>,
) -> ChatResult<()> {
    let events = events?;
    for event in events {
        sink.emit(event).await?;
    }
    Ok(())
}

/// Ends the session after the event stream failed for good, reporting `code`.
async fn settle_event_failure(resources: &EventPumpResources, code: &str, message: &str) {
    let error = ChatError::new(ChatErrorCode::TransportUnavailable, message, true);
    if let Ok(mut terminal) = resources.terminal_error.lock() {
        *terminal = Some(error);
    }
    let event = resources
        .route
        .lock()
        .map_err(|_| driver_state_error())
        .and_then(|mut state| {
            state.session_state = ProviderSessionState::Failed;
            resources.normalizer.external_event(
                &state,
                "transport/error",
                CanonicalEvent::RuntimeError(RuntimeErrorEvent {
                    code: code.to_string(),
                    message: message.to_string(),
                    recoverable: true,
                    safe_details: None,
                }),
            )
        });
    if let Ok(event) = event {
        let _ = resources.sink.emit(event).await;
        let _ = resources.sink.flush().await;
    }
}

fn reconnect_delay(attempt: usize) -> Duration {
    Duration::from_millis(100_u64.saturating_mul(1_u64 << attempt.min(5)))
}

fn connected_account_label(inventory: &Value) -> Option<String> {
    let connected = inventory
        .as_object()
        .and_then(|object| object.get("connected"))
        .and_then(Value::as_array)?;
    let labels = connected
        .iter()
        .filter_map(Value::as_str)
        .take(8)
        .collect::<Vec<_>>();
    (!labels.is_empty()).then(|| labels.join(", "))
}

async fn stop_owned_server(server: &mut Option<OwnedOpenCodeServer>) {
    if let Some(server) = server.as_mut() {
        let _ = server.stop().await;
    }
}

/// Password the client sends: the owned server's ephemeral secret, or the configured external one.
fn server_password<'a>(
    owned_server: &'a Option<OwnedOpenCodeServer>,
    external_password: Option<&'a OpenCodeSecret>,
) -> Option<&'a OpenCodeSecret> {
    owned_server
        .as_ref()
        .map(OwnedOpenCodeServer::password)
        .or(external_password)
}

/// Decides whether the internal MCP endpoint and its bearer token are registered on a server.
///
/// Only an owned, password-authenticated loopback server receives the token:
/// an unauthenticated server would expose it to every local user, and an
/// external server is outside Ganbaru's process ownership.
pub(super) fn registers_internal_mcp(
    connection: &OpenCodeConnectionMode,
    password: Option<&OpenCodeSecret>,
) -> bool {
    matches!(connection, OpenCodeConnectionMode::Local) && password.is_some()
}
