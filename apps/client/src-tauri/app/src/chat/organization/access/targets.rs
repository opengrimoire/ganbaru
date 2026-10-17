//! Device-local working-folder and scratch assignment-target discovery.

use super::channels::read_teammate_access;
use super::{
    ChatAssignmentTargetBindingState, ChatAssignmentTargetKind, ChatAssignmentTargetLifecycleState,
    ChatAssignmentTargetRead, ChatError, ChatErrorCode, ChatExecutionEnvironmentId,
    ChatExecutionTarget, ChatFolderCapability, ChatParticipantId, ChatReplyThreadId, ChatResult,
    ChatRuntimeApprovalPolicy, ChatScratchGenerationId, ChatScratchScopeId,
    ListChatAssignmentTargetsCommand,
};
use crate::chat::channels::{chat_pool, identifier_error, persistence_error};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

#[tauri::command]
pub async fn chat_list_assignment_targets(
    app: tauri::AppHandle,
    db_url: String,
    request: ListChatAssignmentTargetsCommand,
) -> ChatResult<Vec<ChatAssignmentTargetRead>> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let channel = crate::chat::channels::read_channel(&pool, &request.channel_id).await?;
    let access = read_teammate_access(&pool, &request.teammate_id).await?;
    let channel_access = access
        .channels
        .iter()
        .find(|candidate| candidate.conversation_id == channel.conversation_id)
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::NotFound,
                "The teammate is not a member of this channel",
                true,
            )
        })?;
    if !channel_access.capabilities.participate {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "The teammate cannot participate in this channel",
            true,
        ));
    }
    let folder_reads =
        crate::chat::workspace::commands::projects_list_working_folders_cached(app.clone(), db_url)
            .await?;
    let bindings = folder_reads
        .into_iter()
        .map(|read| (read.working_folder.id.clone(), read.binding_status))
        .collect::<BTreeMap<_, _>>();
    let mut targets = Vec::new();
    for grant in &channel_access.folder_grants {
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
        .bind(grant.working_folder_id.as_str())
        .fetch_all(&pool)
        .await
        .map_err(persistence_error)?;
        let binding_state = match bindings.get(&grant.working_folder_id) {
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
            let eligible = grant.capability != ChatFolderCapability::None
                && binding_state == ChatAssignmentTargetBindingState::Ready
                && lifecycle_state == ChatAssignmentTargetLifecycleState::Available;
            targets.push(ChatAssignmentTargetRead {
                execution_target: Some(ChatExecutionTarget::WorkingFolder {
                    working_folder_id: grant.working_folder_id.clone(),
                    execution_environment_id: environment_id,
                }),
                kind: if is_worktree {
                    ChatAssignmentTargetKind::Worktree
                } else {
                    ChatAssignmentTargetKind::CurrentFolder
                },
                display_name: row.try_get("display_name").map_err(persistence_error)?,
                folder_capability: Some(grant.capability),
                effective_runtime_approval: grant
                    .runtime_approval_override
                    .or(channel_access.runtime_approval_override)
                    .unwrap_or(access.teammate_default_runtime_approval),
                is_default: grant.is_default && !is_worktree,
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
    let scratch = read_scratch_assignment_target(
        &app,
        &pool,
        &request.teammate_id,
        request.reply_thread_id.as_ref(),
        channel_access.scratch_runtime_approval_override,
        channel_access.runtime_approval_override,
        access.teammate_default_runtime_approval,
    )
    .await?;
    targets.push(scratch);
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

async fn read_scratch_assignment_target(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    teammate_id: &ChatParticipantId,
    reply_thread_id: Option<&ChatReplyThreadId>,
    scratch_override: Option<ChatRuntimeApprovalPolicy>,
    channel_override: Option<ChatRuntimeApprovalPolicy>,
    teammate_default: ChatRuntimeApprovalPolicy,
) -> ChatResult<ChatAssignmentTargetRead> {
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
             WHERE scope.reply_thread_id = ? AND scope.teammate_id = ?
               AND scope.removed_at IS NULL",
        )
        .bind(reply_thread_id.as_str())
        .bind(teammate_id.as_str())
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
        None => (
            None,
            ChatAssignmentTargetLifecycleState::Available,
            false,
            ChatAssignmentTargetBindingState::Ready,
        ),
    };
    Ok(ChatAssignmentTargetRead {
        execution_target,
        kind: ChatAssignmentTargetKind::Scratch,
        display_name: "Private scratch".to_string(),
        folder_capability: None,
        effective_runtime_approval: scratch_override
            .or(channel_override)
            .unwrap_or(teammate_default),
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
    })
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
