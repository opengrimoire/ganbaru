//! Device-local working-folder and scratch assignment-target discovery.

use super::super::provider_agents::read_device_providers;
use super::resolve::{read_reached_folders, resolve_channel_agent_access};
use super::{
    ChatAssignmentTargetBindingState, ChatAssignmentTargetKind, ChatAssignmentTargetLifecycleState,
    ChatAssignmentTargetRead, ChatError, ChatErrorCode, ChatExecutionEnvironmentId,
    ChatExecutionTarget, ChatFolderCapability, ChatParticipantId, ChatReplyThreadId, ChatResult,
    ChatRuntimeApprovalPolicy, ChatScratchGenerationId, ChatScratchScopeId,
    ListChatAssignmentTargetsCommand,
};
use crate::chat::channels::{chat_pool, identifier_error, persistence_error};
use ganbaru_chat::coordination::agent_settings::agent_is_present;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

/// Lists the targets the agent's access reaches in the channel on this device: the
/// environments of each project folder, with the primary folder's current folder as the
/// default, and the agent's private scratch in the reply thread when the access allows scratch.
/// It lists nothing when the agent's provider cannot work on this device, since work posted
/// here then runs on a linked desktop that resolves its own target.
#[tauri::command]
pub async fn chat_list_assignment_targets(
    app: tauri::AppHandle,
    db_url: String,
    request: ListChatAssignmentTargetsCommand,
) -> ChatResult<Vec<ChatAssignmentTargetRead>> {
    let providers = read_device_providers(&app);
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let channel = crate::chat::channels::read_channel(&pool, &request.channel_id).await?;
    let (access, folders) = {
        let mut connection = pool.acquire().await.map_err(persistence_error)?;
        if !agent_is_present(
            &mut connection,
            channel.conversation_id.as_str(),
            request.agent_id.as_str(),
        )
        .await?
        {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "The agent cannot participate in this channel",
                true,
            ));
        }
        let access = resolve_channel_agent_access(
            &mut connection,
            channel.conversation_id.as_str(),
            request.agent_id.as_str(),
            &providers,
        )
        .await?;
        if !access.resolved.can_work() {
            return Ok(Vec::new());
        }
        let folders = read_reached_folders(&mut connection, &access).await?;
        (access, folders)
    };
    let folder_capability = access.resolved.folder_capability;
    let runtime_approval = access.resolved.runtime_approval;
    let folder_reads =
        crate::chat::workspace::commands::projects_list_working_folders_cached(app.clone(), db_url)
            .await?;
    let bindings = folder_reads
        .into_iter()
        .map(|read| {
            (
                read.working_folder.id.as_str().to_string(),
                read.binding_status,
            )
        })
        .collect::<BTreeMap<_, _>>();
    crate::chat::channels::ensure_chat_defaults(
        &pool,
        crate::chat::channels::ChatDefaultsScope::Project(&channel.project_id),
    )
    .await?;
    let mut targets = Vec::new();
    for folder in &folders {
        let working_folder_id = ganbaru_chat_contracts::models::ProjectWorkingFolderId::new(
            folder.working_folder_id.clone(),
        )
        .map_err(identifier_error)?;
        let rows = sqlx::query(
            "SELECT environment.id, environment.kind, environment.display_name,
                    environment.lifecycle_state, worktree.cleanup_state
             FROM chat_execution_environments environment
             LEFT JOIN chat_worktrees worktree
               ON worktree.execution_environment_id = environment.id
             WHERE environment.working_folder_id = ?
               AND environment.archived_at IS NULL
             ORDER BY environment.kind = 'current_folder' DESC,
                      environment.created_at, environment.id",
        )
        .bind(folder.working_folder_id.as_str())
        .fetch_all(&pool)
        .await
        .map_err(persistence_error)?;
        let binding_state = match bindings.get(&folder.working_folder_id) {
            Some(ganbaru_chat::workspace::WorkingFolderBindingStatus::Available) => {
                ChatAssignmentTargetBindingState::Ready
            }
            Some(ganbaru_chat::workspace::WorkingFolderBindingStatus::Unbound) => {
                ChatAssignmentTargetBindingState::Locate
            }
            Some(ganbaru_chat::workspace::WorkingFolderBindingStatus::Missing) => {
                ChatAssignmentTargetBindingState::Missing
            }
            Some(ganbaru_chat::workspace::WorkingFolderBindingStatus::RepositoryMismatch) => {
                ChatAssignmentTargetBindingState::Relink
            }
            None => ChatAssignmentTargetBindingState::UnavailableOnThisDevice,
        };
        for row in rows {
            let environment_id = ChatExecutionEnvironmentId::new(
                row.try_get::<String, _>("id").map_err(persistence_error)?,
            )
            .map_err(identifier_error)?;
            let environment_kind: String = row.try_get("kind").map_err(persistence_error)?;
            let lifecycle_state = parse_assignment_target_lifecycle(
                &row.try_get::<String, _>("lifecycle_state")
                    .map_err(persistence_error)?,
            )?;
            let is_busy: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM chat_agent_runs
                    WHERE execution_environment_id = ?
                      AND state IN ('queued', 'starting', 'working', 'waiting')
                 )",
            )
            .bind(environment_id.as_str())
            .fetch_one(&pool)
            .await
            .map_err(persistence_error)?;
            let is_worktree = environment_kind == "worktree";
            let eligible = folder_capability != ChatFolderCapability::None
                && binding_state == ChatAssignmentTargetBindingState::Ready
                && lifecycle_state == ChatAssignmentTargetLifecycleState::Available;
            targets.push(ChatAssignmentTargetRead {
                execution_target: Some(ChatExecutionTarget::WorkingFolder {
                    working_folder_id: working_folder_id.clone(),
                    execution_environment_id: environment_id,
                }),
                kind: if is_worktree {
                    ChatAssignmentTargetKind::Worktree
                } else {
                    ChatAssignmentTargetKind::CurrentFolder
                },
                display_name: row.try_get("display_name").map_err(persistence_error)?,
                folder_capability: Some(folder_capability),
                effective_runtime_approval: runtime_approval,
                is_default: folder.is_primary && !is_worktree,
                binding_state,
                lifecycle_state,
                is_busy: is_busy != 0,
                is_dirty: row
                    .try_get::<Option<String>, _>("cleanup_state")
                    .map_err(persistence_error)?
                    .and_then(|state| (state == "dirty").then_some(true)),
                eligible,
                unavailable_reason: None,
            });
        }
    }
    if let Some(reply_thread_id) = request.reply_thread_id.as_ref() {
        prefer_thread_folder(&pool, reply_thread_id, &mut targets).await?;
    }
    if access.resolved.scratch_allowed
        && let Some(scratch) = read_scratch_assignment_target(
            &app,
            &pool,
            &request.agent_id,
            request.reply_thread_id.as_ref(),
            access.resolved.scratch_approval,
        )
        .await?
    {
        targets.push(scratch);
    }
    targets.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| {
                assignment_target_kind_rank(left.kind).cmp(&assignment_target_kind_rank(right.kind))
            })
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    Ok(targets)
}

/// Makes the folder the latest earlier work in the reply thread ran in the default target, when
/// the agent's access still reaches it: its environment while that is still available, else the
/// folder's current folder. Otherwise the primary folder's current folder stays the default.
async fn prefer_thread_folder(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
    targets: &mut [ChatAssignmentTargetRead],
) -> ChatResult<()> {
    let previous: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT authorization.working_folder_id, authorization.execution_environment_id
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE assignment.reply_thread_id = ?
           AND authorization.working_folder_id IS NOT NULL
           AND authorization.decision_state = 'allowed'
         ORDER BY authorization.created_at DESC, authorization.id DESC
         LIMIT 1",
    )
    .bind(reply_thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some((folder_id, environment_id)) = previous else {
        return Ok(());
    };
    let in_folder = |target: &ChatAssignmentTargetRead| -> Option<String> {
        match &target.execution_target {
            Some(ChatExecutionTarget::WorkingFolder {
                working_folder_id,
                execution_environment_id,
            }) if working_folder_id.as_str() == folder_id => {
                Some(execution_environment_id.as_str().to_owned())
            }
            _ => None,
        }
    };
    let same_environment = targets.iter().position(|target| {
        target.lifecycle_state == ChatAssignmentTargetLifecycleState::Available
            && in_folder(target).is_some()
            && in_folder(target) == environment_id
    });
    let chosen = same_environment.or_else(|| {
        targets.iter().position(|target| {
            target.kind == ChatAssignmentTargetKind::CurrentFolder && in_folder(target).is_some()
        })
    });
    if let Some(chosen) = chosen {
        for (index, target) in targets.iter_mut().enumerate() {
            target.is_default = index == chosen;
        }
    }
    Ok(())
}

/// Reads the agent's active scratch generation in the reply thread, or `None` when none exists, so the
/// listed targets never name a scratch that is not there.
async fn read_scratch_assignment_target(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    agent_id: &ChatParticipantId,
    reply_thread_id: Option<&ChatReplyThreadId>,
    scratch_approval: ChatRuntimeApprovalPolicy,
) -> ChatResult<Option<ChatAssignmentTargetRead>> {
    let row = match reply_thread_id {
        Some(reply_thread_id) => sqlx::query(
            "SELECT scope.id AS scratch_scope_id, generation.id AS scratch_generation_id,
                    environment.id AS execution_environment_id,
                    environment.lifecycle_state
             FROM chat_scratch_scopes scope
             JOIN chat_scratch_generations generation
               ON generation.scratch_scope_id = scope.id
              AND generation.lifecycle_state = 'active'
             JOIN chat_execution_environments environment
               ON environment.scratch_generation_id = generation.id
              AND environment.archived_at IS NULL
             WHERE scope.reply_thread_id = ? AND scope.agent_id = ?
               AND scope.removed_at IS NULL",
        )
        .bind(reply_thread_id.as_str())
        .bind(agent_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?,
        None => None,
    };
    let (execution_target, lifecycle_state, is_busy, binding_state) = match row {
        Some(row) => {
            let environment_id = ChatExecutionEnvironmentId::new(
                row.try_get::<String, _>("execution_environment_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?;
            let is_busy: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM chat_agent_runs
                    WHERE execution_environment_id = ?
                      AND state IN ('queued', 'starting', 'working', 'waiting')
                 )",
            )
            .bind(environment_id.as_str())
            .fetch_one(pool)
            .await
            .map_err(persistence_error)?;
            let generation_id = ChatScratchGenerationId::new(
                row.try_get::<String, _>("scratch_generation_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?;
            let binding_state =
                if crate::chat::scratch::resolve_managed_scratch_path_for_inspection(
                    app,
                    pool,
                    generation_id.as_str(),
                    environment_id.as_str(),
                )
                .await
                .is_ok()
                {
                    ChatAssignmentTargetBindingState::Ready
                } else {
                    ChatAssignmentTargetBindingState::UnavailableOnThisDevice
                };
            (
                Some(ChatExecutionTarget::Scratch {
                    scratch_scope_id: ChatScratchScopeId::new(
                        row.try_get::<String, _>("scratch_scope_id")
                            .map_err(persistence_error)?,
                    )
                    .map_err(identifier_error)?,
                    scratch_generation_id: generation_id,
                    execution_environment_id: environment_id,
                }),
                parse_assignment_target_lifecycle(
                    &row.try_get::<String, _>("lifecycle_state")
                        .map_err(persistence_error)?,
                )?,
                is_busy != 0,
                binding_state,
            )
        }
        None => return Ok(None),
    };
    Ok(Some(ChatAssignmentTargetRead {
        execution_target,
        kind: ChatAssignmentTargetKind::Scratch,
        display_name: "Private scratch".to_string(),
        folder_capability: None,
        effective_runtime_approval: scratch_approval,
        is_default: false,
        binding_state,
        eligible: lifecycle_state == ChatAssignmentTargetLifecycleState::Available
            && binding_state == ChatAssignmentTargetBindingState::Ready,
        lifecycle_state,
        is_busy,
        is_dirty: None,
        unavailable_reason: (binding_state
            == ChatAssignmentTargetBindingState::UnavailableOnThisDevice)
            .then(|| "Private scratch is unavailable on this device".to_string()),
    }))
}

fn parse_assignment_target_lifecycle(
    value: &str,
) -> ChatResult<ChatAssignmentTargetLifecycleState> {
    match value {
        "creating" => Ok(ChatAssignmentTargetLifecycleState::Creating),
        "available" => Ok(ChatAssignmentTargetLifecycleState::Available),
        "missing" => Ok(ChatAssignmentTargetLifecycleState::Missing),
        "cleanup_pending" => Ok(ChatAssignmentTargetLifecycleState::CleanupPending),
        "cleanup_failed" => Ok(ChatAssignmentTargetLifecycleState::CleanupFailed),
        "removed" => Ok(ChatAssignmentTargetLifecycleState::Removed),
        _ => Err(ChatError::new(
            ChatErrorCode::Persistence,
            "Stored assignment target lifecycle is invalid",
            false,
        )),
    }
}

const fn assignment_target_kind_rank(kind: ChatAssignmentTargetKind) -> u8 {
    match kind {
        ChatAssignmentTargetKind::CurrentFolder => 0,
        ChatAssignmentTargetKind::Worktree => 1,
        ChatAssignmentTargetKind::Scratch => 2,
    }
}
