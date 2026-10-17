//! Cursor provider driver and live-session ownership.

use super::executable::*;
use super::interactions::PendingCursorRequests;
use super::normalizer::{CursorEventNormalizer, CursorRouteState};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpProviderFlavor {
    Cursor,
    Grok,
}

impl AcpProviderFlavor {
    pub fn family_id(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
            Self::Grok => "grok",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor",
            Self::Grok => "Grok",
        }
    }

    pub fn metadata_display_name(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor",
            Self::Grok => "xAI",
        }
    }

    pub fn turn_prefix(self) -> &'static str {
        self.family_id()
    }
}

pub struct CursorProviderDriver {
    pub(super) configuration: ProviderInstanceConfig,
    pub(super) settings: CursorProviderSettings,
    pub(super) flavor: AcpProviderFlavor,
    pub(super) live: Option<CursorLiveSession>,
    pub(super) cached_models: Option<ProviderModelCatalog>,
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
    pub session_id: ProviderSessionId,
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
}

impl CursorProviderDriver {
    pub fn new(configuration: ProviderInstanceConfig) -> ChatResult<Self> {
        Self::new_for(configuration, AcpProviderFlavor::Cursor)
    }

    pub fn new_grok(configuration: ProviderInstanceConfig) -> ChatResult<Self> {
        Self::new_for(configuration, AcpProviderFlavor::Grok)
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
            minimum_tested_cli_version: (flavor == AcpProviderFlavor::Cursor)
                .then(|| MINIMUM_CURSOR_VERSION.to_string()),
            default_executable_candidates: match flavor {
                AcpProviderFlavor::Cursor => {
                    vec!["cursor-agent".to_string(), "agent".to_string()]
                }
                AcpProviderFlavor::Grok => vec!["grok".to_string()],
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
        let about = match self.flavor {
            AcpProviderFlavor::Cursor => {
                let about = probe_about(&executable, workspace, environment).await?;
                ensure_supported_version(about.version)?;
                AcpProviderAbout::from(about)
            }
            AcpProviderFlavor::Grok => {
                probe_grok_about(&executable, workspace, environment).await?
            }
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
        let workspace = canonical_current_directory()?;
        let (mut connection, about) = self.open_connection(&workspace).await?;
        let result = initialize_provider_session(AcpSessionInitialization {
            flavor: self.flavor,
            grok_uses_api_key: self.grok_uses_api_key(),
            connection: &connection,
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
        let started = match initialize_provider_session(AcpSessionInitialization {
            flavor: self.flavor,
            grok_uses_api_key: self.grok_uses_api_key(),
            connection: &connection,
            workspace: workspace.to_string_lossy().as_ref(),
            resume_session_id: cursor.as_ref().map(|cursor| cursor.session_id.as_str()),
            requested: AcpRequestedConfiguration {
                modes: input.modes(),
                model_id: input.model_id(),
                model_options: input.model_options(),
            },
            internal_mcp: self.configuration.internal_mcp.as_ref(),
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
        let capabilities =
            negotiated_capabilities_for(self.flavor, &started.initialize, &started.setup);
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
        let inbound = connection.take_inbound()?;
        let terminal_callbacks =
            AcpTerminalCallbacks::new(workspace.clone(), started.session_id.clone());
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
            terminal_callbacks,
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
            session_id: local_session_id.clone(),
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

    fn grok_uses_api_key(&self) -> bool {
        self.flavor == AcpProviderFlavor::Grok
            && self
                .configuration
                .environment
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("XAI_API_KEY"))
                .map(|(_, value)| !value.trim().is_empty())
                .unwrap_or_else(|| {
                    std::env::var("XAI_API_KEY").is_ok_and(|value| !value.trim().is_empty())
                })
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

pub(super) fn canonical_current_directory() -> ChatResult<PathBuf> {
    let current = std::env::current_dir().map_err(|_| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "ACP provider probe directory is unavailable",
            true,
        )
    })?;
    std::fs::canonicalize(current).map_err(|_| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "ACP provider probe directory is unavailable",
            true,
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
