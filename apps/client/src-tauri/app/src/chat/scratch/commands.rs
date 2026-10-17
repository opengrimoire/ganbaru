//! Public lifecycle commands for private organizational scratch storage.

mod cleanup;
mod inspection;
mod promotion;

// Re-export the commands and their generated Tauri wrappers through the stable facade.
pub use cleanup::*;
pub use inspection::*;
pub use promotion::*;

use crate::chat::channels::{identifier_error, persistence_error, u64_value};
use crate::chat::scratch;
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, SqlitePool};

const LOCAL_PARTICIPANT_ID: &str = "participant:local-owner";

struct ScratchIdentity {
    scope_id: ChatScratchScopeId,
    scope_revision: u64,
    scope_lifecycle: ChatScratchScopeLifecycleState,
    conversation_id: ChatConversationId,
    teammate_id: ChatParticipantId,
    generation_id: ChatScratchGenerationId,
    generation_lifecycle: ChatScratchGenerationLifecycleState,
    execution_environment_id: ChatExecutionEnvironmentId,
}

async fn read_scratch_identity(
    pool: &SqlitePool,
    generation_id: &ChatScratchGenerationId,
) -> ChatResult<ScratchIdentity> {
    let row = sqlx::query(
        "SELECT scope.id AS scope_id, scope.revision AS scope_revision,
                scope.lifecycle_state AS scope_lifecycle, scope.reply_thread_id,
                scope.teammate_id, thread.conversation_id,
                generation.id AS generation_id,
                generation.lifecycle_state AS generation_lifecycle,
                environment.id AS execution_environment_id
         FROM chat_scratch_generations generation
         JOIN chat_scratch_scopes scope ON scope.id = generation.scratch_scope_id
         JOIN chat_reply_threads thread ON thread.id = scope.reply_thread_id
         JOIN chat_execution_environments environment
           ON environment.scratch_generation_id = generation.id
          AND environment.kind = 'scratch'
         WHERE generation.id = ? AND generation.removed_at IS NULL
           AND scope.removed_at IS NULL",
    )
    .bind(generation_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(scratch_not_found_error)?;
    Ok(ScratchIdentity {
        scope_id: parse_id(row.try_get("scope_id").map_err(persistence_error)?)?,
        scope_revision: u64_value(row.try_get("scope_revision").map_err(persistence_error)?)?,
        scope_lifecycle: parse_scope_lifecycle(
            &row.try_get::<String, _>("scope_lifecycle")
                .map_err(persistence_error)?,
        )?,
        conversation_id: parse_id(row.try_get("conversation_id").map_err(persistence_error)?)?,
        teammate_id: parse_id(row.try_get("teammate_id").map_err(persistence_error)?)?,
        generation_id: parse_id(row.try_get("generation_id").map_err(persistence_error)?)?,
        generation_lifecycle: parse_generation_lifecycle(
            &row.try_get::<String, _>("generation_lifecycle")
                .map_err(persistence_error)?,
        )?,
        execution_environment_id: parse_id(
            row.try_get("execution_environment_id")
                .map_err(persistence_error)?,
        )?,
    })
}

async fn require_scratch_inspection_authority(
    pool: &SqlitePool,
    identity: &ScratchIdentity,
    allow_restricted: bool,
) -> ChatResult<()> {
    if !matches!(
        identity.scope_lifecycle,
        ChatScratchScopeLifecycleState::Active
            | ChatScratchScopeLifecycleState::Archived
            | ChatScratchScopeLifecycleState::CleanupFailed
    ) {
        return Err(scratch_not_found_error());
    }
    match identity.generation_lifecycle {
        ChatScratchGenerationLifecycleState::Active => {
            require_current_scratch_authority(pool, identity).await
        }
        ChatScratchGenerationLifecycleState::Quarantined
        | ChatScratchGenerationLifecycleState::CleanupFailed
            if allow_restricted =>
        {
            require_local_owner_destination_membership(pool, &identity.conversation_id).await?;
            if scratch::generation_sources_readable_by(
                pool,
                identity.generation_id.as_str(),
                LOCAL_PARTICIPANT_ID,
            )
            .await?
            {
                Ok(())
            } else {
                Err(ChatError::new(
                    ChatErrorCode::Permission,
                    "Private scratch sources are no longer readable by the local owner",
                    false,
                ))
            }
        }
        ChatScratchGenerationLifecycleState::Quarantined
        | ChatScratchGenerationLifecycleState::CleanupFailed => Err(ChatError::new(
            ChatErrorCode::Permission,
            "Restricted scratch requires explicit owner inspection",
            false,
        )),
        _ => Err(scratch_not_found_error()),
    }
}

async fn require_current_scratch_authority(
    pool: &SqlitePool,
    identity: &ScratchIdentity,
) -> ChatResult<()> {
    require_local_owner_destination_membership(pool, &identity.conversation_id).await?;
    let can_participate: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1
           FROM chat_conversation_memberships membership
           JOIN chat_teammate_channel_memberships access
             ON access.conversation_id = membership.conversation_id
            AND access.teammate_id = membership.participant_id
           JOIN chat_access_profiles profile ON profile.id = access.access_profile_id
           JOIN chat_access_profile_revisions revision
             ON revision.access_profile_id = profile.id
            AND revision.revision = profile.latest_revision
           WHERE membership.conversation_id = ? AND membership.participant_id = ?
             AND membership.removed_at IS NULL
             AND CASE
                   WHEN access.participate_inherits_profile = 1 THEN revision.default_participate
                   ELSE access.participate AND revision.default_participate
                 END = 1
         )",
    )
    .bind(identity.conversation_id.as_str())
    .bind(identity.teammate_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if !can_participate
        || !scratch::generation_constraints_hold(
            pool,
            identity.generation_id.as_str(),
            identity.conversation_id.as_str(),
            identity.teammate_id.as_str(),
            LOCAL_PARTICIPANT_ID,
        )
        .await?
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Private scratch authorization is no longer active",
            false,
        ));
    }
    Ok(())
}

async fn require_local_owner(pool: &SqlitePool) -> ChatResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_participants
           WHERE id = ? AND participant_kind = 'local_user' AND archived_at IS NULL
         )",
    )
    .bind(LOCAL_PARTICIPANT_ID)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if exists {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "Private scratch management is unavailable",
            false,
        ))
    }
}

async fn require_local_owner_destination_membership(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
) -> ChatResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_conversation_memberships
           WHERE conversation_id = ? AND participant_id = ? AND removed_at IS NULL
         )",
    )
    .bind(conversation_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if exists {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "The local owner cannot inspect this reply thread",
            false,
        ))
    }
}

fn parse_scope_lifecycle(value: &str) -> ChatResult<ChatScratchScopeLifecycleState> {
    match value {
        "active" => Ok(ChatScratchScopeLifecycleState::Active),
        "archived" => Ok(ChatScratchScopeLifecycleState::Archived),
        "cleanup_pending" => Ok(ChatScratchScopeLifecycleState::CleanupPending),
        "cleanup_failed" => Ok(ChatScratchScopeLifecycleState::CleanupFailed),
        "removed" => Ok(ChatScratchScopeLifecycleState::Removed),
        _ => Err(corrupt_scratch_error()),
    }
}

fn parse_generation_lifecycle(value: &str) -> ChatResult<ChatScratchGenerationLifecycleState> {
    match value {
        "active" => Ok(ChatScratchGenerationLifecycleState::Active),
        "quarantined" => Ok(ChatScratchGenerationLifecycleState::Quarantined),
        "cleanup_pending" => Ok(ChatScratchGenerationLifecycleState::CleanupPending),
        "cleanup_failed" => Ok(ChatScratchGenerationLifecycleState::CleanupFailed),
        "removed" => Ok(ChatScratchGenerationLifecycleState::Removed),
        _ => Err(corrupt_scratch_error()),
    }
}

fn parse_id<T>(value: String) -> ChatResult<T>
where
    T: TryFrom<String, Error = String>,
{
    T::try_from(value).map_err(identifier_error)
}

fn scratch_not_found_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::NotFound,
        "Private scratch generation was not found",
        true,
    )
}

fn corrupt_scratch_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored private scratch state is invalid",
        false,
    )
}
