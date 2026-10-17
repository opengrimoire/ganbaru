//! Provider session startup and serialized durable event ingestion.

use super::checkpoints::ensure_post_turn_checkpoint;
use super::persistence::ThreadRuntimeData;
use super::support::{PROVIDER_START_TIMEOUT, now_timestamp, operation_context};
use super::validation::validate_modes;
use crate::chat::credentials::{PlatformCredentialStore, materialize_provider_environment};
use crate::chat::ingestion::{ChatEventIngestor, TauriChatChangeEmitter};
use crate::chat::send_commands::SendChatTurnCommand;
use crate::chat::workspace::AuthorizedWorkingFolder;
use ganbaru_chat::agent_runs::AgentRunBinding;
use ganbaru_chat::repository::events::AppendCanonicalEventRequest;
use ganbaru_chat::runtime::ThreadRuntimeOwner;
use ganbaru_chat_contracts::events::{CanonicalEvent, CanonicalRuntimeEvent, ChangedFileSummary};
use ganbaru_chat_contracts::models::{
    ChatAgentRunId, ChatAuthorizationRevisionId, ChatError, ChatErrorCode, ChatResult,
    ChatTeammatePolicyRevisionId, ChatThreadId, ChatTurnId, ChatWorkAssignmentId,
    ContinuationGroupId, ProviderInstanceConfig, ProviderSessionSnapshot, ProviderSessionState,
    ResumeSessionRequest, StartSessionRequest, VerifiedWorkspaceContext,
};
use ganbaru_chat_providers::{
    DriverFuture, ProviderDriver, ProviderDriverFactory, ProviderDriverRegistry, ProviderEventSink,
};
use sqlx::{Row, SqlitePool};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

const MAX_PROVIDER_CHANGED_FILES: usize = 512;
const MAX_PROVIDER_CHANGED_FILE_PATH_BYTES: usize = 4_096;

pub(super) struct EnsureSessionContext<'a> {
    pub(super) app: &'a tauri::AppHandle,
    pub(super) pool: &'a SqlitePool,
    pub(super) owner: &'a Arc<ThreadRuntimeOwner>,
    pub(super) new_driver: Option<Box<dyn ProviderDriver>>,
    pub(super) configuration: ProviderInstanceConfig,
    pub(super) workspace: &'a AuthorizedWorkingFolder,
    pub(super) thread_id: &'a ChatThreadId,
    pub(super) existing: Option<&'a ThreadRuntimeData>,
    pub(super) continuation_group_id: &'a ContinuationGroupId,
    pub(super) request: &'a SendChatTurnCommand,
    pub(super) authorization: Option<&'a AgentRunBinding>,
}

pub(super) async fn ensure_session_with_executable_recovery(
    context: EnsureSessionContext<'_>,
) -> ChatResult<ProviderSessionSnapshot> {
    let EnsureSessionContext {
        app,
        pool,
        owner,
        new_driver,
        configuration,
        workspace,
        thread_id,
        existing,
        continuation_group_id,
        request,
        authorization,
    } = context;
    let internal_mcp = configuration.internal_mcp.clone();
    let initial = ensure_session(EnsureSessionContext {
        app,
        pool,
        owner,
        new_driver,
        configuration,
        workspace,
        thread_id,
        existing,
        continuation_group_id,
        request,
        authorization,
    })
    .await;
    let error = match initial {
        Ok(session) => return Ok(session),
        Err(error) if error.code == ChatErrorCode::ExecutableMissing => error,
        Err(error) => return Err(error),
    };
    let repaired_probe = crate::chat::settings_commands::chat_probe_provider(
        app.clone(),
        request.provider_instance_id.clone(),
    )
    .await;
    if !repaired_probe.as_ref().is_ok_and(|probe| {
        probe.state != ganbaru_chat_contracts::models::ProbeState::ExecutableMissing
    }) {
        return Err(error);
    }
    let repaired_provider =
        crate::chat::settings_commands::read_provider(app, &request.provider_instance_id)?;
    let mut repaired_configuration = materialize_provider_environment(
        &repaired_provider.configuration,
        &PlatformCredentialStore::default(),
    )?;
    repaired_configuration.internal_mcp = internal_mcp;
    let retry = ensure_session(EnsureSessionContext {
        app,
        pool,
        owner,
        new_driver: None,
        configuration: repaired_configuration,
        workspace,
        thread_id,
        existing,
        continuation_group_id,
        request,
        authorization,
    })
    .await;
    if retry
        .as_ref()
        .is_err_and(|retry_error| retry_error.code == ChatErrorCode::ExecutableMissing)
    {
        let _ = crate::chat::settings_commands::chat_probe_provider(
            app.clone(),
            request.provider_instance_id.clone(),
        )
        .await;
    }
    retry
}

pub(super) async fn ensure_session(
    context: EnsureSessionContext<'_>,
) -> ChatResult<ProviderSessionSnapshot> {
    let EnsureSessionContext {
        app,
        pool,
        owner,
        new_driver,
        configuration,
        workspace,
        thread_id,
        existing,
        continuation_group_id,
        request,
        authorization,
    } = context;
    let snapshot = owner.snapshot()?;
    if reuse_existing_session(snapshot.session_id.is_some(), snapshot.session_state)? {
        let session_id = snapshot.session_id.ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::InvalidStateTransition,
                "Chat provider session identity is unavailable",
                true,
            )
        })?;
        validate_modes(&snapshot.capabilities, request.modes)?;
        return Ok(ProviderSessionSnapshot {
            session_id,
            state: snapshot.session_state,
            provider_thread_id: existing.and_then(|value| value.provider_thread_id.clone()),
            continuation_group_id: continuation_group_id.clone(),
            resume_cursor: existing.and_then(|value| value.resume_cursor.clone()),
            effective_modes: request.modes,
            capabilities: snapshot.capabilities,
            started_at: now_timestamp()?,
        });
    }
    let driver = match new_driver {
        Some(driver) => driver,
        None => ProviderDriverRegistry.create_driver(configuration)?,
    };
    validate_modes(&driver.capabilities(), request.modes)?;
    let verified = VerifiedWorkspaceContext {
        working_folder_id: workspace.working_folder_id.clone(),
        canonical_path: workspace
            .canonical_path
            .to_str()
            .ok_or_else(|| ChatError::validation("workspace", "Workspace path is not supported"))?
            .to_string(),
        repository_kind: workspace.repository_kind,
        repository_identity: workspace.repository_identity.clone(),
    };
    let native_workspace_authorized =
        request.working_folder_id.is_some() || request.scratch_generation_id.is_some();
    let checkpoint_enabled = request.working_folder_id.is_some();
    crate::chat::diagnostics_commands::prune_expired_diagnostics(pool).await?;
    let sink: Arc<dyn ProviderEventSink> = Arc::new(DurableChatEventSink::new(
        app.clone(),
        pool.clone(),
        Arc::new(TauriChatChangeEmitter::new(app.clone())),
        workspace.clone(),
        authorization.cloned(),
        native_workspace_authorized,
        checkpoint_enabled,
    ));
    if let Some(existing) = existing {
        if let (Some(provider_thread_id), Some(resume_cursor)) = (
            existing.provider_thread_id.clone(),
            existing.resume_cursor.clone(),
        ) {
            return owner
                .resume_session(
                    driver,
                    ResumeSessionRequest {
                        thread_id: thread_id.clone(),
                        workspace: verified,
                        provider_instance_id: request.provider_instance_id.clone(),
                        provider_thread_id,
                        continuation_group_id: continuation_group_id.clone(),
                        resume_cursor,
                        modes: request.modes,
                    },
                    sink,
                    operation_context("resume-session", PROVIDER_START_TIMEOUT),
                )
                .await;
        }
        if existing.provider_thread_id.is_some() {
            return Err(ChatError::new(
                ChatErrorCode::ResumeNotFound,
                "Native continuation data is missing. Start a fresh thread instead",
                true,
            ));
        }
    }
    owner
        .start_session(
            driver,
            StartSessionRequest {
                thread_id: thread_id.clone(),
                workspace: verified,
                provider_instance_id: request.provider_instance_id.clone(),
                modes: request.modes,
                model_id: request.model_id.clone(),
                model_options: request.model_options.clone(),
            },
            sink,
            operation_context("start-session", PROVIDER_START_TIMEOUT),
        )
        .await
}

pub(super) fn reuse_existing_session(
    has_session_id: bool,
    session_state: ProviderSessionState,
) -> ChatResult<bool> {
    if session_state == ProviderSessionState::Ready {
        if has_session_id {
            return Ok(true);
        }
        return Err(ChatError::new(
            ChatErrorCode::InvalidStateTransition,
            "Chat provider session identity is unavailable",
            true,
        ));
    }
    if matches!(
        session_state,
        ProviderSessionState::Stopped | ProviderSessionState::Failed
    ) {
        return Ok(false);
    }
    Err(ChatError::new(
        ChatErrorCode::InvalidStateTransition,
        "Chat provider session is not ready",
        true,
    ))
}

struct DurableChatEventSink {
    app: tauri::AppHandle,
    pool: SqlitePool,
    workspace: AuthorizedWorkingFolder,
    organizational: bool,
    native_workspace_authorized: bool,
    checkpoint_enabled: bool,
    ingestor: Mutex<ChatEventIngestor>,
}

impl DurableChatEventSink {
    fn new(
        app: tauri::AppHandle,
        pool: SqlitePool,
        emitter: Arc<dyn crate::chat::ingestion::ChatChangeEmitter>,
        workspace: AuthorizedWorkingFolder,
        authorization: Option<AgentRunBinding>,
        native_workspace_authorized: bool,
        checkpoint_enabled: bool,
    ) -> Self {
        Self {
            app,
            ingestor: Mutex::new(ChatEventIngestor::new(pool.clone(), emitter)),
            pool,
            workspace,
            organizational: authorization.is_some(),
            native_workspace_authorized,
            checkpoint_enabled,
        }
    }
}

impl ProviderEventSink for DurableChatEventSink {
    fn emit<'a>(&'a self, mut event: CanonicalRuntimeEvent) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            if self.organizational {
                if let Err(error) =
                    require_live_authorization(&self.pool, &event.thread_id, event.turn_id.as_ref())
                        .await
                {
                    self.app
                        .state::<crate::chat::internal_mcp::InternalMcpRegistry>()
                        .revoke_run_scope(&event.thread_id)
                        .await;
                    return Err(error);
                }
                if !self.native_workspace_authorized && event_changes_files(&event.event) {
                    return Err(ChatError::new(
                        ChatErrorCode::Permission,
                        "Conversation assignments cannot change workspace files",
                        false,
                    ));
                }
            }
            normalize_changed_file_paths(&mut event.event, &self.workspace.canonical_path);
            let settled_turn = matches!(
                &event.event,
                CanonicalEvent::TurnCompleted(_) | CanonicalEvent::TurnAborted(_)
            )
            .then(|| (event.thread_id.clone(), event.turn_id.clone()))
            .and_then(|(thread_id, turn_id)| turn_id.map(|turn_id| (thread_id, turn_id)));
            let stopped_thread = matches!(&event.event, CanonicalEvent::SessionExited(_))
                .then(|| event.thread_id.clone());
            let ingestion: ChatResult<()> = async {
                let diagnostic_expires_at =
                    crate::chat::diagnostics_commands::attach_opt_in_diagnostic(
                        &self.app, &mut event,
                    )?;
                self.ingestor
                    .lock()
                    .await
                    .ingest(AppendCanonicalEventRequest {
                        runtime: event,
                        ingested_at: now_timestamp()?,
                        diagnostic_expires_at,
                    })
                    .await
            }
            .await;
            if let Some((thread_id, turn_id)) = settled_turn {
                if ingestion.is_ok() && self.checkpoint_enabled {
                    if let Ok(settled_at) = now_timestamp() {
                        ensure_post_turn_checkpoint(
                            &self.pool,
                            &self.workspace,
                            &thread_id,
                            &turn_id,
                            &settled_at,
                        )
                        .await;
                    }
                }
                self.app
                    .state::<crate::chat::workspace::mutation::ChatWorkspaceMutationRegistry>()
                    .finish_provider_turn(&thread_id, &turn_id);
            }
            if let Some(thread_id) = stopped_thread {
                self.app
                    .state::<crate::chat::workspace::mutation::ChatWorkspaceMutationRegistry>()
                    .finish_thread(&thread_id);
            }
            ingestion
        })
    }

    fn flush(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move { self.ingestor.lock().await.flush().await })
    }
}

fn event_changes_files(event: &CanonicalEvent) -> bool {
    match event {
        CanonicalEvent::DiffUpdated(event) => !event.files.is_empty(),
        CanonicalEvent::TurnCompleted(event) => !event.changed_files.is_empty(),
        _ => false,
    }
}

async fn require_live_authorization(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    turn_id: Option<&ChatTurnId>,
) -> ChatResult<()> {
    let active = sqlx::query(
        "SELECT run.id, run.assignment_id, run.project_id, run.working_folder_id,
                run.execution_environment_id, run.scratch_generation_id,
                run.teammate_policy_revision_id, run.authorization_revision_id,
                run.authorization_scope_digest, run.run_ordinal
         FROM chat_agent_runs run
         JOIN chat_assignment_authorization_revisions authorization
           ON authorization.id = run.authorization_revision_id
          AND authorization.assignment_id = run.assignment_id
          AND authorization.scope_digest = run.authorization_scope_digest
         JOIN chat_work_assignments assignment ON assignment.id = run.assignment_id
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = authorization.destination_conversation_id
          AND membership.participant_id = assignment.teammate_id
          AND membership.removed_at IS NULL
         JOIN chat_teammate_channel_memberships channel_access
           ON channel_access.conversation_id = membership.conversation_id
          AND channel_access.teammate_id = membership.participant_id
         JOIN chat_access_profiles profile ON profile.id = channel_access.access_profile_id
         JOIN chat_access_profile_revisions profile_revision
           ON profile_revision.access_profile_id = profile.id
          AND profile_revision.revision = profile.latest_revision
         WHERE run.provider_thread_id = ?
           AND (? IS NULL OR run.provider_turn_id = ?)
           AND run.state IN ('starting', 'working', 'waiting')
           AND authorization.decision_state = 'allowed'
           AND authorization.revoked_at IS NULL
           AND CASE
                 WHEN channel_access.participate_inherits_profile = 1
                   THEN profile_revision.default_participate
                 ELSE channel_access.participate AND profile_revision.default_participate
               END = 1
         ORDER BY run.created_at DESC, run.id DESC
         LIMIT 1",
    )
    .bind(thread_id.as_str())
    .bind(turn_id.map(ChatTurnId::as_str))
    .bind(turn_id.map(ChatTurnId::as_str))
    .fetch_optional(pool)
    .await
    .map_err(|_| live_authorization_error())?;
    let active = active.ok_or_else(live_authorization_error)?;
    let binding = AgentRunBinding {
        run_id: ChatAgentRunId::new(
            active
                .try_get::<String, _>("id")
                .map_err(|_| live_authorization_error())?,
        )
        .map_err(|_| live_authorization_error())?,
        assignment_id: ChatWorkAssignmentId::new(
            active
                .try_get::<String, _>("assignment_id")
                .map_err(|_| live_authorization_error())?,
        )
        .map_err(|_| live_authorization_error())?,
        project_id: active
            .try_get("project_id")
            .map_err(|_| live_authorization_error())?,
        teammate_policy_revision_id: ChatTeammatePolicyRevisionId::new(
            active
                .try_get::<String, _>("teammate_policy_revision_id")
                .map_err(|_| live_authorization_error())?,
        )
        .map_err(|_| live_authorization_error())?,
        authorization_revision_id: ChatAuthorizationRevisionId::new(
            active
                .try_get::<String, _>("authorization_revision_id")
                .map_err(|_| live_authorization_error())?,
        )
        .map_err(|_| live_authorization_error())?,
        authorization_scope_digest: active
            .try_get("authorization_scope_digest")
            .map_err(|_| live_authorization_error())?,
        working_folder_id: active
            .try_get("working_folder_id")
            .map_err(|_| live_authorization_error())?,
        scratch_generation_id: active
            .try_get("scratch_generation_id")
            .map_err(|_| live_authorization_error())?,
        execution_environment_id: active
            .try_get("execution_environment_id")
            .map_err(|_| live_authorization_error())?,
        run_ordinal: u64::try_from(
            active
                .try_get::<i64, _>("run_ordinal")
                .map_err(|_| live_authorization_error())?,
        )
        .map_err(|_| live_authorization_error())?,
    };
    let scope = super::coordinator::load_internal_mcp_scope(pool, thread_id, &binding).await?;
    crate::chat::internal_mcp_tools::verify_publication_scope(pool, thread_id, &scope).await
}

fn live_authorization_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "The organizational run authorization is no longer active",
        false,
    )
}

pub(super) fn normalize_changed_file_paths(
    event: &mut CanonicalEvent,
    workspace: &std::path::Path,
) {
    let files = match event {
        CanonicalEvent::DiffUpdated(event) => &mut event.files,
        CanonicalEvent::TurnCompleted(event) => &mut event.changed_files,
        _ => return,
    };
    let mut normalized: Vec<ChangedFileSummary> = Vec::with_capacity(files.len());
    for mut file in std::mem::take(files)
        .into_iter()
        .take(MAX_PROVIDER_CHANGED_FILES)
    {
        let Some(relative_path) = workspace_relative_provider_path(workspace, &file.relative_path)
        else {
            continue;
        };
        file.relative_path = relative_path;
        file.previous_relative_path = file
            .previous_relative_path
            .as_deref()
            .and_then(|path| workspace_relative_provider_path(workspace, path));
        if let Some(index) = normalized
            .iter()
            .position(|candidate| candidate.relative_path == file.relative_path)
        {
            normalized[index] = file;
        } else {
            normalized.push(file);
        }
    }
    *files = normalized;
}

fn workspace_relative_provider_path(
    workspace: &std::path::Path,
    provider_path: &str,
) -> Option<String> {
    if provider_path.is_empty()
        || provider_path.len() > MAX_PROVIDER_CHANGED_FILE_PATH_BYTES
        || provider_path.contains('\0')
        || (cfg!(not(windows)) && provider_path.contains('\\'))
        || is_windows_absolute_provider_path(provider_path)
        || provider_path.chars().any(char::is_control)
    {
        return None;
    }
    let path = std::path::Path::new(provider_path);
    let relative = if path.is_absolute() {
        path.strip_prefix(workspace).ok()?
    } else {
        path
    };
    let mut components = Vec::new();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return None;
        };
        components.push(component.to_str()?);
    }
    (!components.is_empty()).then(|| components.join("/"))
}

fn is_windows_absolute_provider_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes
        .first()
        .is_some_and(|value| value.is_ascii_alphabetic())
        && bytes.get(1) == Some(&b':')
        && bytes
            .get(2)
            .is_some_and(|separator| matches!(*separator, b'/' | b'\\'))
}
