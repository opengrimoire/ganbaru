//! Send-turn coordinator across validation, persistence, and runtime launch.

use super::checkpoints::ensure_pre_turn_checkpoint;
use super::persistence::{
    PersistUserTurnContext, TurnPersistenceTarget, mark_turn_dispatch_failed,
    persist_fresh_session_handoff, persist_user_turn, read_attachment_references,
    read_thread_runtime_data, replay_send_receipt,
};
use super::session::{
    EnsureSessionContext, InternalMcpLaunch, ensure_session_with_executable_recovery,
};
use super::support::{
    PROVIDER_START_TIMEOUT, TURN_OPERATION_TIMEOUT, chat_pool, device_state_error, now_timestamp,
    operation_context, versioned_value,
};
use super::validation::{
    validate_explicit_model, validate_mentions, validate_modes, validate_prompt,
    validate_provider_selection, validate_send_mentions,
};
use crate::chat::device_state::{full_access_is_trusted, read_active_device_scope};
use crate::chat::internal_mcp::EndpointIssue;
use crate::chat::send_commands::{SendChatTurnCommand, SendChatTurnResult};
use crate::chat::workspace::WorkingFolderAuthorizationOperation;
use crate::vault;
use ganbaru_chat::agent_runs::{
    AgentRunBinding, AttemptReason, RunLaunch, TurnOrigin, opened_fresh_native_session,
};
use ganbaru_chat::repository::receipts::{
    CommandReceiptState, complete_command_receipt, read_command_receipt,
};
use ganbaru_chat::repository::{reads, workspaces};
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::models::*;
use ganbaru_chat_providers::{ProviderDriverFactory, ProviderDriverRegistry};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use tauri::Manager;

/// Projections a persisted or failed user turn changes without an event.
const SENT_TURN_KEYS: &[ChatProjectionKey] = &[
    ChatProjectionKey::Thread,
    ChatProjectionKey::Turns,
    ChatProjectionKey::Messages,
];

/// Projections a continuation that handed off to a fresh native session changes.
const HANDED_OFF_TURN_KEYS: &[ChatProjectionKey] = &[
    ChatProjectionKey::Thread,
    ChatProjectionKey::Messages,
    ChatProjectionKey::PendingRequests,
];

/// Detects that the provider opened a fresh native session for a continuation instead of the
/// stored one, which some providers do without reporting the loss, and records the run as a
/// handoff that sends its full package. Returns the prompt and launch the turn then uses.
///
/// A continuation without a full package fails as a lost session, so its dispatch hands off.
pub(super) async fn hand_off_on_fresh_session(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    request: &SendChatTurnCommand,
    origin: &TurnOrigin,
    existing: Option<&super::persistence::ThreadRuntimeData>,
    session: &ProviderSessionSnapshot,
) -> ChatResult<Option<(String, RunLaunch)>> {
    let Some((binding, launch)) = origin.assignment_run() else {
        return Ok(None);
    };
    if launch.attempt.reason != AttemptReason::Continuation
        || !opened_fresh_native_session(
            existing.and_then(|value| value.provider_thread_id.as_ref()),
            session.provider_thread_id.as_ref(),
        )
    {
        return Ok(None);
    }
    let (prompt, handed_off) = launch.handed_off_to_fresh_session().ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::ResumeNotFound,
            "The provider opened a new session in place of the one this run continues",
            true,
        )
    })?;
    persist_fresh_session_handoff(
        pool,
        thread_id,
        request,
        binding,
        &handed_off,
        &prompt,
        &now_timestamp()?,
    )
    .await?;
    Ok(Some((prompt, handed_off)))
}

pub(crate) async fn send_turn(
    app: tauri::AppHandle,
    db_url: String,
    request: SendChatTurnCommand,
    origin: TurnOrigin,
) -> ChatResult<SendChatTurnResult> {
    validate_prompt(
        &request.prompt,
        !request.attachment_ids.is_empty() || !request.mentions.is_empty(),
    )?;
    validate_explicit_model(request.provider_managed_model, request.model_id.as_ref())?;
    validate_mentions(&request.mentions)?;
    let thread_id = match (&request.thread_id, &request.new_thread_id) {
        (Some(thread_id), _) => thread_id.clone(),
        (None, Some(thread_id)) => thread_id.clone(),
        (None, None) => {
            return Err(ChatError::validation(
                "newThreadId",
                "A new Chat thread ID is required",
            ));
        }
    };
    let pool = chat_pool(app.clone(), db_url).await?;
    if let Some(receipt) = read_command_receipt(&pool, &request.command.client_command_id).await? {
        return replay_send_receipt(&pool, &thread_id, &receipt).await;
    }
    let existing = match request.thread_id.as_ref() {
        Some(existing_thread_id) => {
            Some(read_thread_runtime_data(&pool, existing_thread_id).await?)
        }
        None => None,
    };
    let provider =
        crate::chat::settings_commands::read_provider(&app, &request.provider_instance_id)?;
    validate_provider_selection(&provider, &request)?;
    let mut configuration = crate::chat::settings::approved_runtime(&app, &provider.configuration)?;
    let inspection_driver = ProviderDriverRegistry.create_driver(configuration.clone())?;
    let (target, authorized) = resolve_turn_target(
        &app,
        &pool,
        &request,
        &origin,
        existing.as_ref(),
        inspection_driver.project_configuration_restriction(),
        inspection_driver.project_configuration_paths(),
    )
    .await?;
    require_project_accepts_ai_work(&pool, &target.project_id).await?;
    let authority_support = inspection_driver.authority_support();
    let organizational_scope = match origin.run() {
        Some(binding) => Some(
            validate_organizational_authority(
                &pool,
                binding,
                authority_support,
                request.modes.safety_mode,
            )
            .await?,
        ),
        None => None,
    };
    if let Some(existing) = &existing {
        if existing.working_folder_id != request.working_folder_id
            || existing.scratch_generation_id != request.scratch_generation_id
            || existing.provider_instance_id != request.provider_instance_id
            || request
                .execution_environment_id
                .as_deref()
                .is_some_and(|requested| {
                    existing.execution_environment_id.as_deref() != Some(requested)
                })
        {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Changing workspace, provider, or execution environment requires a new Chat thread",
                true,
            ));
        }
    }
    let owner = app
        .state::<ChatRuntimeRegistry>()
        .owner(thread_id.clone())?;
    let mcp_registry = app.state::<crate::chat::internal_mcp::InternalMcpRegistry>();
    let mut snapshot = owner.snapshot()?;
    if origin.run().is_some() {
        if snapshot.turn_active {
            return Err(ChatError::new(
                ChatErrorCode::Busy,
                "The previous organizational turn must settle before authority can rotate",
                true,
            ));
        }
        mcp_registry.revoke_run_scope(&thread_id).await;
        if snapshot.session_id.is_some() {
            let _ = owner
                .stop_session(
                    false,
                    operation_context("rotate-organizational-session", PROVIDER_START_TIMEOUT),
                )
                .await;
            snapshot = owner.snapshot()?;
            // Each organizational run launches a new process with its own target scope, so a
            // process that survives the stop must never be reused for this run.
            if snapshot.session_state == ProviderSessionState::Ready {
                return Err(ChatError::new(
                    ChatErrorCode::Busy,
                    "The previous organizational session could not be stopped",
                    true,
                ));
            }
        }
    }
    let fresh_native_session = !matches!(snapshot.session_state, ProviderSessionState::Ready)
        || snapshot.session_id.is_none();
    let internal_mcp = if authority_support.internal_host_tools {
        if fresh_native_session {
            mcp_registry.stop_thread_endpoint(&thread_id).await;
        }
        let resource_endpoint = mcp_registry
            .ensure_thread_endpoint(
                app.clone(),
                pool.clone(),
                vault::active_vault_path(&app).map_err(|_| {
                    ChatError::new(
                        ChatErrorCode::Persistence,
                        "Active Ganbaru folder is unavailable",
                        true,
                    )
                })?,
                thread_id.clone(),
                origin.run().is_some(),
            )
            .await?;
        let launch = InternalMcpLaunch {
            organizational_scope,
        };
        configuration.internal_mcp = match resource_endpoint {
            EndpointIssue::Created(endpoint) => Some(launch.config(endpoint)),
            // Another send created the endpoint first. A fresh session starts a process, so it
            // takes a rotated token now; a reused one keeps the token its process holds.
            EndpointIssue::Running if fresh_native_session => {
                Some(launch.rotated_config(&app, &thread_id).await?)
            }
            EndpointIssue::Running => None,
        };
        Some(launch)
    } else {
        configuration.internal_mcp = None;
        None
    };
    let mut new_driver = None;
    let continuation_group_id = if let Some(existing) = &existing {
        existing.continuation_group_id.clone()
    } else {
        let mut driver = ProviderDriverRegistry.create_driver(configuration.clone())?;
        validate_modes(&driver.capabilities(), request.modes)?;
        let continuation = driver
            .derive_continuation_group(
                ContinuationGroupRequest {
                    provider_instance_id: request.provider_instance_id.clone(),
                    normalized_provider_home: configuration.provider_home.clone(),
                    account_identity: provider
                        .last_probe
                        .as_ref()
                        .and_then(|probe| probe.account_label.clone()),
                    server_identity: None,
                    provider_fields: BTreeMap::new(),
                },
                &operation_context("derive-continuation", PROVIDER_START_TIMEOUT),
            )
            .await?;
        new_driver = Some(driver);
        continuation
    };
    let attachment_references = read_attachment_references(
        &app,
        &pool,
        request.working_folder_id.as_ref(),
        &request.attachment_ids,
    )
    .await?;
    let persistence_now = now_timestamp()?;
    let mutation_registry =
        app.state::<crate::chat::workspace::mutation::ChatWorkspaceMutationRegistry>();
    let reservation = mutation_registry.begin_provider_turn(
        &authorized.canonical_path,
        &thread_id,
        &request.turn_id,
    )?;
    let persistence = persist_user_turn(PersistUserTurnContext {
        pool: &pool,
        target: &target,
        thread_id: &thread_id,
        existing: existing.as_ref(),
        continuation_group_id: &continuation_group_id,
        provider_family_id: &provider.configuration.family_id,
        request: &request,
        origin: &origin,
        attachments: &attachment_references,
        now: &persistence_now,
    })
    .await;
    persistence?;
    crate::chat::ingestion::notify_rows_changed(&app, &pool, &thread_id, SENT_TURN_KEYS).await;

    // Every step after persistence runs here, so any failure reaches the turn failure mark and
    // the launch settlement below and a persisted run never stays starting.
    let mut handed_off = None;
    let operation = async {
        if authority_support.internal_host_tools {
            if let Some(binding) = origin.run() {
                let scope = load_internal_mcp_scope(&pool, &thread_id, binding).await?;
                mcp_registry.activate_run_scope(&thread_id, scope).await?;
            }
        }
        if request.working_folder_id.is_some() {
            ensure_pre_turn_checkpoint(
                &pool,
                &authorized,
                &thread_id,
                &request.turn_id,
                &persistence_now,
            )
            .await;
        }
        let session = ensure_session_with_executable_recovery(EnsureSessionContext {
            app: &app,
            pool: &pool,
            owner: &owner,
            new_driver,
            configuration,
            internal_mcp,
            workspace: &authorized,
            thread_id: &thread_id,
            existing: existing.as_ref(),
            continuation_group_id: &continuation_group_id,
            request: &request,
            authorization: origin.run(),
            developer_instructions: origin.developer_instructions(),
            developer_instructions_delivered: origin.instructions_delivered_to_native_session(),
        })
        .await?;
        let prompt = match hand_off_on_fresh_session(
            &pool,
            &thread_id,
            &request,
            &origin,
            existing.as_ref(),
            &session,
        )
        .await?
        {
            Some((prompt, launch)) => {
                crate::chat::ingestion::notify_rows_changed(
                    &app,
                    &pool,
                    &thread_id,
                    HANDED_OFF_TURN_KEYS,
                )
                .await;
                handed_off = Some(launch);
                prompt
            }
            None => request.prompt.clone(),
        };
        if origin.run().is_some() && authority_support.internal_host_tools {
            mcp_registry.wait_until_ready(&thread_id).await?;
        }
        let reservation = reservation.handoff_to_runtime()?;
        if authority_support.internal_host_tools {
            if origin.run().is_none() {
                mcp_registry
                    .begin_direct_turn(&thread_id, &request.turn_id)
                    .await;
            }
            mcp_registry.begin_turn_network(&thread_id).await;
        }
        owner
            .send_turn(
                SendTurnRequest {
                    command: request.command.clone(),
                    session_id: session.session_id,
                    turn_id: request.turn_id.clone(),
                    prompt,
                    attachments: attachment_references,
                    mentions: request.mentions.clone(),
                    model_id: request.model_id.clone(),
                    model_options: request.model_options.clone(),
                    modes: request.modes,
                    developer_instructions: origin.developer_instructions().map(str::to_string),
                },
                reservation,
                operation_context("send-turn", TURN_OPERATION_TIMEOUT),
            )
            .await
    }
    .await;
    if authority_support.internal_host_tools {
        let network_access = operation
            .as_ref()
            .ok()
            .and_then(|dispatch| dispatch.sandbox_network_access);
        mcp_registry
            .settle_turn_network(&thread_id, network_access)
            .await;
        if operation.is_err() {
            mcp_registry
                .end_direct_turn(&thread_id, Some(&request.turn_id))
                .await;
        }
    }
    let (dispatch, launch_error, turn_failure_marked) = match operation {
        Ok(dispatch) => (Some(dispatch), None, Ok(())),
        Err(error) => {
            let marked = match now_timestamp() {
                Ok(now) => {
                    mark_turn_dispatch_failed(&pool, &thread_id, &request.turn_id, &error, &now)
                        .await
                }
                Err(timestamp_error) => Err(timestamp_error),
            };
            (None, Some(error), marked)
        }
    };
    if turn_failure_marked.is_ok() && launch_error.is_some() {
        crate::chat::ingestion::notify_rows_changed(&app, &pool, &thread_id, SENT_TURN_KEYS).await;
    }
    let mut launch_settlement = None;
    if let Some((binding, launch)) = origin.assignment_run() {
        let settlement = ganbaru_chat::agent_runs::settle_launch(
            &pool,
            binding,
            handed_off.as_ref().unwrap_or(launch),
            launch_error.as_ref(),
            &now_timestamp()?,
        )
        .await?;
        launch_settlement = Some(settlement);
        crate::chat::authority_epoch::bump_authority_epoch(&app);
        if settlement == ganbaru_chat::agent_runs::LaunchSettlement::HandoffRequired {
            crate::chat::ingestion::notify_rows_changed(
                &app,
                &pool,
                &thread_id,
                &[ChatProjectionKey::PendingRequests],
            )
            .await;
        }
        if settlement.stops_session() {
            mcp_registry.revoke_run_scope(&thread_id).await;
            if let Err(error) = owner
                .stop_session(
                    false,
                    operation_context("settled-assignment-launch", PROVIDER_START_TIMEOUT),
                )
                .await
            {
                eprintln!(
                    "Chat thread {} could not stop the run of a settled assignment: {error}",
                    thread_id.as_str()
                );
            }
        }
    }
    turn_failure_marked?;
    let result = SendChatTurnResult {
        thread: reads::read_thread_shell(&pool, &thread_id).await?,
        dispatch,
        launch_error,
        launch_settlement,
    };
    let receipt_result = versioned_value(&result)?;
    complete_command_receipt(
        &pool,
        &request.command.client_command_id,
        CommandReceiptState::Completed,
        Some(&receipt_result),
        None,
        &now_timestamp()?,
    )
    .await?;
    Ok(result)
}

async fn require_project_accepts_ai_work(pool: &SqlitePool, project_id: &str) -> ChatResult<()> {
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Persistence,
                "Project state could not be read",
                true,
            )
        })?;
    match status.as_deref() {
        Some("archived") => Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Restore the archived project before starting new AI work",
            true,
        )),
        Some(_) => Ok(()),
        None => Err(ChatError::new(
            ChatErrorCode::NotFound,
            "The Chat project was not found",
            true,
        )),
    }
}

async fn resolve_turn_target(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    request: &SendChatTurnCommand,
    origin: &TurnOrigin,
    existing: Option<&super::persistence::ThreadRuntimeData>,
    restriction: ganbaru_chat_contracts::models::ProjectConfigurationRestriction,
    reads: &[ganbaru_chat_contracts::models::ProviderConfigurationPath],
) -> ChatResult<(
    TurnPersistenceTarget,
    crate::chat::workspace::AuthorizedWorkingFolder,
)> {
    match (
        request.working_folder_id.as_ref(),
        request.scratch_generation_id.as_deref(),
    ) {
        (Some(working_folder_id), None) => {
            if let Some(binding) = origin.run() {
                if binding.working_folder_id.as_deref() != Some(working_folder_id.as_str())
                    || binding.scratch_generation_id.is_some()
                    || request.execution_environment_id.as_deref()
                        != binding.execution_environment_id.as_deref()
                {
                    return Err(ChatError::new(
                        ChatErrorCode::Permission,
                        "The working-folder target does not match this assignment",
                        false,
                    ));
                }
            }
            let logical_workspace = workspaces::read_workspace(pool, working_folder_id).await?;
            crate::chat::channels::ensure_chat_defaults(
                pool,
                crate::chat::channels::ChatDefaultsScope::WorkingFolder(working_folder_id.as_str()),
            )
            .await?;
            let scope = read_active_device_scope(app).map_err(device_state_error)?;
            let authorized = crate::chat::workspace::commands::authorize_working_folder(
                app,
                pool,
                working_folder_id,
                WorkingFolderAuthorizationOperation::ProviderStart,
            )
            .await?;
            let selected_environment = existing
                .and_then(|data| data.execution_environment_id.as_deref())
                .or(request.execution_environment_id.as_deref());
            let root_path = authorized.canonical_path.clone();
            let authorized = crate::chat::execution_environment::resolve_environment_workspace(
                app,
                pool,
                authorized,
                selected_environment,
            )
            .await?;
            let authorized = crate::chat::workspace::commands::settle_project_configuration(
                app,
                &root_path,
                authorized,
                if origin.run().is_some() {
                    crate::chat::workspace::commands::UndecidedProjectConfiguration::Restrict
                } else {
                    crate::chat::workspace::commands::UndecidedProjectConfiguration::Ask
                },
                restriction,
                reads,
            )
            .await?;
            if matches!(
                request.modes.safety_mode,
                SafetyMode::FullAccess | SafetyMode::Custom
            ) && !full_access_is_trusted(
                &scope,
                &request.provider_instance_id,
                working_folder_id,
            ) {
                return Err(ChatError::new(
                    ChatErrorCode::Permission,
                    "Broad permissions are not trusted for this provider and workspace",
                    true,
                ));
            }
            validate_send_mentions(&authorized, &request.mentions)?;
            Ok((
                TurnPersistenceTarget {
                    project_id: logical_workspace.project_id,
                    working_folder_id: Some(working_folder_id.clone()),
                    scratch_generation_id: None,
                    execution_environment_id: Some(
                        selected_environment.map(str::to_string).unwrap_or_else(|| {
                            format!("current-folder:{}", working_folder_id.as_str())
                        }),
                    ),
                },
                authorized,
            ))
        }
        (None, Some(scratch_generation_id)) => {
            let binding = origin.run().ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Permission,
                    "Private scratch requires an authorized organizational assignment",
                    false,
                )
            })?;
            if binding.scratch_generation_id.as_deref() != Some(scratch_generation_id)
                || request.execution_environment_id.as_deref()
                    != binding.execution_environment_id.as_deref()
                || !request.mentions.is_empty()
            {
                return Err(ChatError::new(
                    ChatErrorCode::Permission,
                    "The scratch execution target does not match this assignment",
                    false,
                ));
            }
            let execution_environment_id =
                binding.execution_environment_id.as_deref().ok_or_else(|| {
                    ChatError::new(
                        ChatErrorCode::Permission,
                        "Private scratch assignment has no execution environment",
                        false,
                    )
                })?;
            let authorized = crate::chat::scratch::authorize_scratch_target(
                app,
                pool,
                scratch_generation_id,
                execution_environment_id,
            )
            .await?;
            Ok((
                TurnPersistenceTarget {
                    project_id: binding.project_id.clone(),
                    working_folder_id: None,
                    scratch_generation_id: Some(scratch_generation_id.to_string()),
                    execution_environment_id: binding.execution_environment_id.clone(),
                },
                authorized,
            ))
        }
        (None, None) => {
            let binding = origin.run().ok_or_else(|| {
                ChatError::validation("executionTarget", "Direct Chat requires a working folder")
            })?;
            if binding.working_folder_id.is_some()
                || binding.scratch_generation_id.is_some()
                || binding.execution_environment_id.is_some()
                || request.execution_environment_id.is_some()
                || !request.mentions.is_empty()
            {
                return Err(ChatError::new(
                    ChatErrorCode::Permission,
                    "The conversation assignment does not authorize a native workspace",
                    false,
                ));
            }
            let thread_id = request
                .thread_id
                .as_ref()
                .or(request.new_thread_id.as_ref())
                .ok_or_else(|| {
                    ChatError::validation("newThreadId", "A provider thread is required")
                })?;
            let authorized = crate::chat::scratch::authorize_conversation_runtime(app, thread_id)?;
            Ok((
                TurnPersistenceTarget {
                    project_id: binding.project_id.clone(),
                    working_folder_id: None,
                    scratch_generation_id: None,
                    execution_environment_id: None,
                },
                authorized,
            ))
        }
        (Some(_), Some(_)) => Err(ChatError::validation(
            "executionTarget",
            "Select at most one native execution target",
        )),
    }
}

/// Checks that an agent run still matches its stored authorization and that the provider
/// enforces the authorized target, returning the scope the driver must enforce.
///
/// # Errors
///
/// Returns a permission error when the authorization, target, safety mode, or provider
/// enforcement no longer admits the run.
pub(super) async fn validate_organizational_authority(
    pool: &SqlitePool,
    binding: &AgentRunBinding,
    support: ProviderAuthoritySupport,
    requested_safety_mode: SafetyMode,
) -> ChatResult<ProviderOrganizationalScope> {
    let authorization = sqlx::query(
        "SELECT authorization.resolved_runtime_approval_policy, authorization.working_folder_id,
                authorization.execution_environment_id,
                policy.safety_mode AS agent_safety_mode
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_agent_policy_revisions policy
           ON policy.id = authorization.agent_policy_revision_id
         WHERE authorization.id = ? AND authorization.assignment_id = ?
           AND authorization.scope_digest = ?
           AND authorization.decision_state = 'allowed' AND authorization.revoked_at IS NULL",
    )
    .bind(binding.authorization_revision_id.as_str())
    .bind(binding.assignment_id.as_str())
    .bind(&binding.authorization_scope_digest)
    .fetch_optional(pool)
    .await
    .map_err(|_| provider_authority_error("Assignment authority could not be verified"))?;
    let Some(authorization) = authorization else {
        return Err(provider_authority_error(
            "Assignment authority is no longer active",
        ));
    };
    let runtime_approval_policy: String = authorization
        .try_get("resolved_runtime_approval_policy")
        .map_err(|_| provider_authority_error("Assignment authority is invalid"))?;
    let authorization_folder_id: Option<String> = authorization
        .try_get("working_folder_id")
        .map_err(|_| provider_authority_error("Assignment authority is invalid"))?;
    let authorization_environment_id: Option<String> = authorization
        .try_get("execution_environment_id")
        .map_err(|_| provider_authority_error("Assignment authority is invalid"))?;
    let has_native_target = binding.execution_environment_id.is_some();
    let target_shape_is_valid = matches!(
        (
            binding.working_folder_id.is_some(),
            binding.scratch_generation_id.is_some(),
            has_native_target,
        ),
        (false, false, false) | (true, false, true) | (false, true, true)
    );
    if authorization_folder_id != binding.working_folder_id
        || authorization_environment_id != binding.execution_environment_id
        || !target_shape_is_valid
    {
        return Err(provider_authority_error(
            "Assignment execution target does not match its authorization",
        ));
    }
    let agent_approval = crate::chat::organization::parse_approval_policy(
        &authorization
            .try_get::<String, _>("agent_safety_mode")
            .map_err(|_| provider_authority_error("Assignment authority is invalid"))?,
    )
    .map_err(|_| provider_authority_error("Assignment authority is invalid"))?;
    let runtime_approval = parse_runtime_approval(&runtime_approval_policy)
        .map_err(|_| provider_authority_error("Assignment authority is invalid"))?;
    if has_native_target
        && runtime_approval == ChatRuntimeApprovalPolicy::ProviderCustom
        && agent_approval != ChatApprovalPolicy::Custom
    {
        return Err(provider_authority_error(
            "Provider-custom approval cannot prove this assignment's exact authority",
        ));
    }
    let permitted_safety_mode = if has_native_target {
        crate::chat::organization::native_assignment_safety_mode(agent_approval, runtime_approval)
    } else {
        SafetyMode::AskForApproval
    };
    if requested_safety_mode != permitted_safety_mode {
        return Err(provider_authority_error(
            "Assignment approval mode does not match its authorization",
        ));
    }
    let host_tools_required: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM chat_assignment_authorized_channel_sources
            WHERE authorization_revision_id = ?
            UNION ALL
            SELECT 1 FROM chat_assignment_authorized_folder_sources
            WHERE authorization_revision_id = ?
         )",
    )
    .bind(binding.authorization_revision_id.as_str())
    .bind(binding.authorization_revision_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(|_| provider_authority_error("Assignment sources could not be verified"))?;
    if !has_native_target && !support.isolated_conversation {
        return Err(provider_authority_error(
            "This provider cannot run an isolated organizational conversation",
        ));
    }
    if host_tools_required && !support.internal_host_tools {
        return Err(provider_authority_error(
            "This provider cannot use the assignment's scoped host tools",
        ));
    }
    let target_capability: Option<String> = sqlx::query_scalar(
        "SELECT capability FROM chat_assignment_authorized_folder_sources
         WHERE authorization_revision_id = ? AND is_execution_target = 1",
    )
    .bind(binding.authorization_revision_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| provider_authority_error("Assignment authority could not be verified"))?;
    let commands_person_approved = runtime_approval == ChatRuntimeApprovalPolicy::Ask
        && permitted_safety_mode == SafetyMode::AskForApproval;
    let scope = organizational_scope(
        binding.working_folder_id.is_some(),
        binding.scratch_generation_id.is_some(),
        target_capability.as_deref(),
    )?;
    if !provider_enforces_scope(support, scope, commands_person_approved) {
        return Err(provider_authority_error(match scope {
            ProviderOrganizationalScope::Scratch => {
                "This provider cannot confine writable private scratch work"
            }
            ProviderOrganizationalScope::Folder { .. } => {
                "This provider cannot enforce the selected folder capability"
            }
            ProviderOrganizationalScope::Conversation => {
                "This provider cannot run an isolated organizational conversation"
            }
        }));
    }
    Ok(scope)
}

/// The scope of an organizational run from its validated target shape. A folder target must
/// carry an execution target grant above `none`.
fn organizational_scope(
    has_folder: bool,
    has_scratch: bool,
    target_capability: Option<&str>,
) -> ChatResult<ProviderOrganizationalScope> {
    match (has_folder, has_scratch) {
        (false, false) => Ok(ProviderOrganizationalScope::Conversation),
        (false, true) => Ok(ProviderOrganizationalScope::Scratch),
        (true, false) => target_capability
            .and_then(ProviderFolderCapability::from_storage)
            .map(|capability| ProviderOrganizationalScope::Folder { capability })
            .ok_or_else(|| {
                provider_authority_error("Assignment folder target has no capability grant")
            }),
        (true, true) => Err(provider_authority_error(
            "Assignment execution target does not match its authorization",
        )),
    }
}

/// Whether a provider's declared boundaries enforce a scope.
///
/// Commands are confined when the provider confines them itself, or when every command reaches
/// the person for approval and the run's effective approval policy is Ask, so each approval is
/// the boundary for that command. Publish also needs the provider to classify publication: a
/// agent reaches a remote only through the push request host tool, which a person confirms.
pub(super) fn provider_enforces_scope(
    support: ProviderAuthoritySupport,
    scope: ProviderOrganizationalScope,
    commands_person_approved: bool,
) -> bool {
    let commands_confined =
        support.confined_commands || (support.person_approved_commands && commands_person_approved);
    let runs_commands = support.writable_root && commands_confined && support.network_boundary;
    match scope {
        ProviderOrganizationalScope::Conversation => support.isolated_conversation,
        ProviderOrganizationalScope::Scratch => runs_commands,
        ProviderOrganizationalScope::Folder { capability } => match capability {
            ProviderFolderCapability::Read => {
                support.read_only_root && support.deny_shell && support.network_boundary
            }
            ProviderFolderCapability::Edit => {
                support.writable_root && support.deny_shell && support.network_boundary
            }
            ProviderFolderCapability::Execute => runs_commands,
            ProviderFolderCapability::Publish => runs_commands && support.classified_publish,
        },
    }
}

pub(super) async fn load_internal_mcp_scope(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    binding: &AgentRunBinding,
) -> ChatResult<crate::chat::internal_mcp::InternalMcpRunScope> {
    let row = sqlx::query(
        "SELECT authorization.destination_conversation_id,
                authorization.resolved_runtime_approval_policy,
                authorization.execution_environment_id, run.provider_turn_id,
                assignment.delegation_depth,
                EXISTS (
                  SELECT 1 FROM chat_agent_channel_memberships other
                  JOIN chat_participants participant ON participant.id = other.agent_id
                  JOIN chat_conversation_memberships membership
                    ON membership.conversation_id = other.conversation_id
                   AND membership.participant_id = other.agent_id
                   AND membership.removed_at IS NULL
                  WHERE other.conversation_id = authorization.destination_conversation_id
                    AND other.agent_id <> assignment.agent_id
                    AND other.removed_at IS NULL AND participant.archived_at IS NULL
                ) AS delegation_target_available
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_agent_runs run
           ON run.authorization_revision_id = authorization.id
          AND run.assignment_id = authorization.assignment_id
          AND run.authorization_scope_digest = authorization.scope_digest
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE authorization.id = ? AND authorization.assignment_id = ?
           AND authorization.scope_digest = ? AND run.id = ?
           AND run.provider_thread_id = ?
           AND authorization.decision_state = 'allowed'
           AND authorization.revoked_at IS NULL",
    )
    .bind(binding.authorization_revision_id.as_str())
    .bind(binding.assignment_id.as_str())
    .bind(&binding.authorization_scope_digest)
    .bind(binding.run_id.as_str())
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| provider_authority_unavailable("Assignment authority could not be loaded"))?
    .ok_or_else(|| provider_authority_error("Assignment authority is no longer active"))?;
    let channel_rows = sqlx::query(
        "SELECT source_handle, message_reference_id, conversation_id, label_snapshot,
                lower_order_key, lower_item_id, high_order_key, high_item_id,
                source_revision_cutoff_id, destination_audience_revision
         FROM chat_assignment_authorized_channel_sources
         WHERE authorization_revision_id = ? ORDER BY conversation_id",
    )
    .bind(binding.authorization_revision_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(|_| provider_authority_unavailable("Channel authority could not be loaded"))?;
    let folder_rows = sqlx::query(
        "SELECT frozen.root_handle, frozen.working_folder_id, frozen.capability,
                frozen.is_execution_target,
                frozen.resolved_runtime_approval_policy
         FROM chat_assignment_authorized_folder_sources frozen
         JOIN chat_assignment_authorization_revisions authorization
           ON authorization.id = frozen.authorization_revision_id
         JOIN chat_work_assignments assignment
           ON assignment.id = authorization.assignment_id
         JOIN chat_agent_working_folder_grants live
           ON live.conversation_id = authorization.destination_conversation_id
          AND live.agent_id = assignment.agent_id
          AND live.working_folder_id = frozen.working_folder_id
          AND live.revoked_at IS NULL
         JOIN chat_agent_channel_memberships channel_access
           ON channel_access.conversation_id = live.conversation_id
          AND channel_access.agent_id = live.agent_id
          AND channel_access.removed_at IS NULL
         WHERE frozen.authorization_revision_id = ?
         ORDER BY frozen.working_folder_id",
    )
    .bind(binding.authorization_revision_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(|_| provider_authority_unavailable("Folder authority could not be loaded"))?;
    let expected_folder_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_assignment_authorized_folder_sources
         WHERE authorization_revision_id = ?",
    )
    .bind(binding.authorization_revision_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(|_| provider_authority_unavailable("Folder authority could not be loaded"))?;
    if usize::try_from(expected_folder_count).ok() != Some(folder_rows.len()) {
        return Err(provider_authority_error(
            "Folder authority is no longer active",
        ));
    }
    Ok(crate::chat::internal_mcp::InternalMcpRunScope {
        run_id: binding.run_id.clone(),
        provider_turn_id: ChatTurnId::new(
            row.try_get::<String, _>("provider_turn_id")
                .map_err(|_| provider_authority_error("Provider turn authority is invalid"))?,
        )
        .map_err(|_| provider_authority_error("Provider turn authority is invalid"))?,
        assignment_id: binding.assignment_id.clone(),
        authorization_revision_id: binding.authorization_revision_id.clone(),
        destination_conversation_id: ChatConversationId::new(
            row.try_get::<String, _>("destination_conversation_id")
                .map_err(|_| provider_authority_error("Destination authority is invalid"))?,
        )
        .map_err(|_| provider_authority_error("Destination authority is invalid"))?,
        authorization_scope_digest: binding.authorization_scope_digest.clone(),
        execution_environment_id: row
            .try_get("execution_environment_id")
            .map_err(|_| provider_authority_error("Execution authority is invalid"))?,
        scratch_generation_id: binding.scratch_generation_id.clone(),
        runtime_approval_policy: parse_runtime_approval(
            &row.try_get::<String, _>("resolved_runtime_approval_policy")
                .map_err(|_| provider_authority_error("Runtime approval is invalid"))?,
        )?,
        channel_sources: channel_rows
            .into_iter()
            .map(|row| {
                Ok(crate::chat::internal_mcp::InternalMcpChannelSource {
                    source_handle: row.try_get("source_handle").map_err(scope_row_error)?,
                    message_reference_id: row
                        .try_get("message_reference_id")
                        .map_err(scope_row_error)?,
                    conversation_id: row.try_get("conversation_id").map_err(scope_row_error)?,
                    label_snapshot: row.try_get("label_snapshot").map_err(scope_row_error)?,
                    lower_order_key: row.try_get("lower_order_key").map_err(scope_row_error)?,
                    lower_item_id: row.try_get("lower_item_id").map_err(scope_row_error)?,
                    high_order_key: row.try_get("high_order_key").map_err(scope_row_error)?,
                    high_item_id: row.try_get("high_item_id").map_err(scope_row_error)?,
                    source_revision_cutoff_id: row
                        .try_get("source_revision_cutoff_id")
                        .map_err(scope_row_error)?,
                    destination_audience_revision: row_u64(&row, "destination_audience_revision")?,
                })
            })
            .collect::<ChatResult<Vec<_>>>()?,
        folder_sources: folder_rows
            .into_iter()
            .map(|row| {
                Ok(crate::chat::internal_mcp::InternalMcpFolderSource {
                    root_handle: row.try_get("root_handle").map_err(scope_row_error)?,
                    working_folder_id: ProjectWorkingFolderId::new(
                        row.try_get::<String, _>("working_folder_id")
                            .map_err(scope_row_error)?,
                    )
                    .map_err(|_| scope_row_error(()))?,
                    capability: parse_folder_capability(
                        &row.try_get::<String, _>("capability")
                            .map_err(scope_row_error)?,
                    )?,
                    is_execution_target: row
                        .try_get::<i64, _>("is_execution_target")
                        .map_err(scope_row_error)?
                        == 1,
                    runtime_approval_policy: parse_runtime_approval(
                        &row.try_get::<String, _>("resolved_runtime_approval_policy")
                            .map_err(scope_row_error)?,
                    )?,
                })
            })
            .collect::<ChatResult<Vec<_>>>()?,
        delegation_depth: row
            .try_get::<i64, _>("delegation_depth")
            .ok()
            .and_then(|depth| u8::try_from(depth).ok())
            .ok_or_else(|| provider_authority_error("Delegation authority is invalid"))?,
        delegation_target_available: row
            .try_get::<i64, _>("delegation_target_available")
            .map_err(|_| provider_authority_error("Delegation authority is invalid"))?
            != 0,
    })
}

fn parse_runtime_approval(value: &str) -> ChatResult<ChatRuntimeApprovalPolicy> {
    match value {
        "ask" => Ok(ChatRuntimeApprovalPolicy::Ask),
        "auto_approve" => Ok(ChatRuntimeApprovalPolicy::AutoApprove),
        "unattended" => Ok(ChatRuntimeApprovalPolicy::Unattended),
        "provider_custom" => Ok(ChatRuntimeApprovalPolicy::ProviderCustom),
        _ => Err(scope_row_error(())),
    }
}

fn parse_folder_capability(value: &str) -> ChatResult<ChatFolderCapability> {
    match value {
        "read" => Ok(ChatFolderCapability::Read),
        "edit" => Ok(ChatFolderCapability::Edit),
        "execute" => Ok(ChatFolderCapability::Execute),
        "publish" => Ok(ChatFolderCapability::Publish),
        _ => Err(scope_row_error(())),
    }
}

fn row_u64(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<u64> {
    u64::try_from(row.try_get::<i64, _>(column).map_err(scope_row_error)?)
        .map_err(|_| scope_row_error(()))
}

fn provider_authority_error(message: &str) -> ChatError {
    ChatError::new(ChatErrorCode::Permission, message, false)
}

/// Reports a database failure while loading authority, which is not a denial.
fn provider_authority_unavailable(message: &str) -> ChatError {
    ChatError::new(ChatErrorCode::Persistence, message, true)
}

fn scope_row_error<T>(_error: T) -> ChatError {
    provider_authority_error("The assignment authorization scope is invalid")
}
