//! Agent settings at the app, project, and channel scopes, and channel membership of agents.
//!
//! Every write that can narrow access or remove an agent revokes, in its transaction, the work
//! and the push approvals that current settings no longer cover, and drains the revocation jobs
//! after it commits, so running work stops at once.

use super::super::provider_agents::{DeviceProviders, read_device_providers};
use super::super::*;
use crate::chat::revocation::drain_access_revocation_jobs;
use ganbaru_chat::conversion::{channel_membership_key, membership_key};
use ganbaru_chat::coordination::access::{APP_AGENT_SETTINGS_ID, resolve_agent_access};
use ganbaru_chat::coordination::agent_settings::{
    agent_is_present, read_app_agent_settings, read_channel_agent_members,
    read_channel_agent_settings, read_channel_push_approvals, read_project_agent_settings,
};
use ganbaru_chat::sync_effects::{
    revoke_exceeding_authorizations, revoke_lapsed_push_approvals,
    revoke_lapsed_remembered_approvals,
};
use ganbaru_sync::domains::chat::MAX_INSTRUCTIONS_CHARS;
use sqlx::{SqliteConnection, SqlitePool};

#[tauri::command]
pub async fn chat_read_app_agent_settings(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<ChatAppAgentSettingsRead> {
    let pool = chat_pool(app, db_url).await?;
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    read_app_agent_settings(&mut connection).await
}

#[tauri::command]
pub async fn chat_update_app_agent_settings(
    app: tauri::AppHandle,
    db_url: String,
    request: UpdateChatAppAgentSettingsCommand,
) -> ChatResult<ChatAppAgentSettingsRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let settings = save_app_settings(&mut transaction, &request, &now).await?;
    transaction.commit().await.map_err(persistence_error)?;
    drain_revocations(&app, &pool).await;
    Ok(settings)
}

/// Saves the vault scope at `expectedRevision` and revokes what the saved settings no longer
/// cover.
pub(in crate::chat::organization) async fn save_app_settings(
    connection: &mut SqliteConnection,
    request: &UpdateChatAppAgentSettingsCommand,
    now: &UtcTimestamp,
) -> ChatResult<ChatAppAgentSettingsRead> {
    validate_instructions(&request.instructions)?;
    let updated = sqlx::query(
        "UPDATE chat_agent_app_settings
         SET agent_access = ?, agent_instructions = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ?",
    )
    .bind(request.access.as_str())
    .bind(&request.instructions)
    .bind(now.as_str())
    .bind(APP_AGENT_SETTINGS_ID)
    .bind(i64_value(request.expected_revision)?)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(stale_settings_error());
    }
    revoke_uncovered(connection, now).await?;
    read_app_agent_settings(connection).await
}

#[tauri::command]
pub async fn chat_read_project_agent_settings(
    app: tauri::AppHandle,
    db_url: String,
    project_id: String,
) -> ChatResult<ChatProjectAgentSettingsRead> {
    let pool = chat_pool(app, db_url).await?;
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    require_project(&mut connection, &project_id).await?;
    read_project_agent_settings(&mut connection, &project_id).await
}

#[tauri::command]
pub async fn chat_update_project_agent_settings(
    app: tauri::AppHandle,
    db_url: String,
    request: UpdateChatProjectAgentSettingsCommand,
) -> ChatResult<ChatProjectAgentSettingsRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let settings = save_project_settings(&mut transaction, &request, &now).await?;
    transaction.commit().await.map_err(persistence_error)?;
    drain_revocations(&app, &pool).await;
    Ok(settings)
}

/// Saves the project scope at `expectedRevision`, creating its row on the first save, when
/// `expectedRevision` is 0, and revokes what the saved settings no longer cover.
pub(in crate::chat::organization) async fn save_project_settings(
    connection: &mut SqliteConnection,
    request: &UpdateChatProjectAgentSettingsCommand,
    now: &UtcTimestamp,
) -> ChatResult<ChatProjectAgentSettingsRead> {
    validate_instructions(&request.instructions)?;
    require_project(connection, &request.project_id).await?;
    let access = request.access.map(ChatAgentAccess::as_str);
    let written = if request.expected_revision == 0 {
        sqlx::query(
            "INSERT INTO chat_project_agent_settings
                (project_id, agent_access, agent_instructions, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(project_id) DO NOTHING",
        )
        .bind(&request.project_id)
        .bind(access)
        .bind(&request.instructions)
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&mut *connection)
        .await
    } else {
        sqlx::query(
            "UPDATE chat_project_agent_settings
             SET agent_access = ?, agent_instructions = ?, revision = revision + 1,
                 updated_at = ?
             WHERE project_id = ? AND revision = ?",
        )
        .bind(access)
        .bind(&request.instructions)
        .bind(now.as_str())
        .bind(&request.project_id)
        .bind(i64_value(request.expected_revision)?)
        .execute(&mut *connection)
        .await
    }
    .map_err(persistence_error)?;
    if written.rows_affected() != 1 {
        return Err(stale_settings_error());
    }
    revoke_uncovered(connection, now).await?;
    read_project_agent_settings(connection, &request.project_id).await
}

#[tauri::command]
pub async fn chat_read_channel_agent_settings(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
) -> ChatResult<ChatChannelAgentSettingsRead> {
    let providers = read_device_providers(&app);
    let pool = chat_pool(app, db_url).await?;
    let conversation_id = channel_conversation_id(&pool, &channel_id).await?;
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    read_channel_settings(&mut connection, conversation_id.as_str(), &providers).await
}

#[tauri::command]
pub async fn chat_update_channel_agent_settings(
    app: tauri::AppHandle,
    db_url: String,
    request: UpdateChatChannelAgentSettingsCommand,
) -> ChatResult<ChatChannelAgentSettingsRead> {
    let providers = read_device_providers(&app);
    let pool = chat_pool(app.clone(), db_url).await?;
    let conversation_id = channel_conversation_id(&pool, &request.channel_id).await?;
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    save_channel_settings(&mut transaction, &request, &now).await?;
    let settings =
        read_channel_settings(&mut transaction, conversation_id.as_str(), &providers).await?;
    transaction.commit().await.map_err(persistence_error)?;
    drain_revocations(&app, &pool).await;
    Ok(settings)
}

/// Saves the channel scope at `expectedRevision` and revokes what the saved settings no longer
/// cover.
pub(in crate::chat::organization) async fn save_channel_settings(
    connection: &mut SqliteConnection,
    request: &UpdateChatChannelAgentSettingsCommand,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    validate_instructions(&request.instructions)?;
    let updated = sqlx::query(
        "UPDATE chat_channels
         SET agent_access = ?, agent_instructions = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ? AND archived_at IS NULL",
    )
    .bind(request.access.map(ChatAgentAccess::as_str))
    .bind(&request.instructions)
    .bind(now.as_str())
    .bind(request.channel_id.as_str())
    .bind(i64_value(request.expected_revision)?)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(stale_settings_error());
    }
    revoke_uncovered(connection, now).await
}

/// Adds an agent to a channel, where it works under the channel's access at once. Adding a
/// present member changes nothing.
#[tauri::command]
pub async fn chat_add_channel_agent(
    app: tauri::AppHandle,
    db_url: String,
    request: AddChatChannelAgentCommand,
) -> ChatResult<ChatChannelAgentSettingsRead> {
    let providers = read_device_providers(&app);
    let pool = chat_pool(app, db_url).await?;
    let conversation_id = channel_conversation_id(&pool, &request.channel_id).await?;
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    add_agent_to_conversation(
        &mut transaction,
        &request.agent_id,
        conversation_id.as_str(),
        &now,
    )
    .await?;
    let settings =
        read_channel_settings(&mut transaction, conversation_id.as_str(), &providers).await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(settings)
}

/// Removes an agent from a channel, revoking its work there and its push approvals.
#[tauri::command]
pub async fn chat_remove_channel_agent(
    app: tauri::AppHandle,
    db_url: String,
    request: RemoveChatChannelAgentCommand,
) -> ChatResult<ChatChannelAgentSettingsRead> {
    let providers = read_device_providers(&app);
    let pool = chat_pool(app.clone(), db_url).await?;
    let conversation_id = channel_conversation_id(&pool, &request.channel_id).await?;
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    if !agent_is_present(
        &mut transaction,
        conversation_id.as_str(),
        request.agent_id.as_str(),
    )
    .await?
    {
        return Err(not_a_member_error());
    }
    super::remove_agent_from_conversation(
        &mut transaction,
        &request.agent_id,
        conversation_id.as_str(),
        &now,
    )
    .await?;
    revoke_uncovered(&mut transaction, &now).await?;
    let settings =
        read_channel_settings(&mut transaction, conversation_id.as_str(), &providers).await?;
    transaction.commit().await.map_err(persistence_error)?;
    drain_revocations(&app, &pool).await;
    Ok(settings)
}

/// Reads the channel scope of agent settings with what each member agent does here.
pub(in crate::chat::organization) async fn read_channel_settings(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    providers: &DeviceProviders,
) -> ChatResult<ChatChannelAgentSettingsRead> {
    let record = read_channel_agent_settings(connection, conversation_id).await?;
    let level = record.effective.access;
    let agents = read_channel_agent_members(connection, conversation_id)
        .await?
        .into_iter()
        .map(|member| {
            let installed = providers
                .get(&member.provider_family_id)
                .is_some_and(|provider| provider.ready);
            let support = providers.support(&member.provider_family_id);
            let resolved = resolve_agent_access(level, &support);
            let own_offered = resolve_agent_access(ChatAgentAccess::Own, &support).effective
                == Some(ChatEffectiveAgentAccess::Own);
            ChatAgentResolvedAccessRead {
                agent_id: member.agent_id,
                requested: resolved.requested,
                effective: resolved.effective,
                downgrade_reason: resolved.downgrade_reason,
                installed,
                own_offered,
            }
        })
        .collect();
    let push_approvals = read_channel_push_approvals(connection, conversation_id).await?;
    Ok(ChatChannelAgentSettingsRead {
        channel_id: record.channel_id,
        conversation_id: record.conversation_id,
        access: record.access,
        instructions: record.instructions,
        effective: record.effective,
        agents,
        push_approvals,
        revision: record.revision,
    })
}

/// Makes `agent_id` a present member of the conversation.
pub(in crate::chat::organization) async fn add_agent_to_conversation(
    connection: &mut SqliteConnection,
    agent_id: &ChatParticipantId,
    conversation_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let active_agent: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_agents agent
           JOIN chat_participants participant ON participant.id = agent.participant_id
           WHERE agent.participant_id = ? AND participant.archived_at IS NULL
         )",
    )
    .bind(agent_id.as_str())
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)?;
    if !active_agent {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "The agent was not found",
            true,
        ));
    }
    sqlx::query(
        "INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES (?, ?, ?, 'member', ?, ?)
         ON CONFLICT(conversation_id, participant_id) DO UPDATE SET
            membership_role = 'member', removed_at = NULL,
            revision = chat_conversation_memberships.revision + 1,
            updated_at = excluded.updated_at
         WHERE chat_conversation_memberships.removed_at IS NOT NULL",
    )
    .bind(conversation_id)
    .bind(agent_id.as_str())
    .bind(sync_key(membership_key(
        conversation_id,
        agent_id.as_str(),
    ))?)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_agent_channel_memberships
            (conversation_id, agent_id, sync_key, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(conversation_id, agent_id) DO UPDATE SET
            removed_at = NULL, model_selection_schema_version = NULL,
            model_selection_data = NULL, updated_at = excluded.updated_at
         WHERE chat_agent_channel_memberships.removed_at IS NOT NULL",
    )
    .bind(conversation_id)
    .bind(agent_id.as_str())
    .bind(sync_key(channel_membership_key(
        conversation_id,
        agent_id.as_str(),
    ))?)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Revokes the work and the push and remembered approvals that current settings and
/// memberships no longer cover.
pub(in crate::chat::organization) async fn revoke_uncovered(
    connection: &mut SqliteConnection,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    revoke_exceeding_authorizations(connection, now.as_str()).await?;
    revoke_lapsed_push_approvals(connection, now.as_str()).await?;
    revoke_lapsed_remembered_approvals(connection, now.as_str()).await?;
    Ok(())
}

/// Stops the runs whose authorization a committed write revoked. The revocations are durable
/// jobs, so a failed drain is logged and retried by the next drain.
pub(in crate::chat::organization) async fn drain_revocations(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
) {
    if let Err(error) = drain_access_revocation_jobs(app, pool).await {
        eprintln!("Chat could not stop revoked agent work: {error}");
    }
}

async fn channel_conversation_id(
    pool: &SqlitePool,
    channel_id: &ChatChannelId,
) -> ChatResult<ChatConversationId> {
    Ok(
        super::super::super::channels::read_channel(pool, channel_id)
            .await?
            .conversation_id,
    )
}

async fn require_project(connection: &mut SqliteConnection, project_id: &str) -> ChatResult<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?)")
        .bind(project_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(persistence_error)?;
    if exists {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::NotFound,
            "The project was not found",
            true,
        ))
    }
}

fn validate_instructions(instructions: &str) -> ChatResult<()> {
    if instructions.chars().count() > MAX_INSTRUCTIONS_CHARS {
        return Err(ChatError::validation(
            "instructions",
            "Agent instructions are too long",
        ));
    }
    Ok(())
}

fn stale_settings_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::StaleRevision,
        "The agent settings changed before this save",
        true,
    )
}

fn not_a_member_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::NotFound,
        "The agent is not a member of this channel",
        true,
    )
}

#[cfg(test)]
mod tests;
