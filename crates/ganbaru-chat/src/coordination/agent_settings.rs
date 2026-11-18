//! Reads of the app, project, and channel scopes of agent settings, the agents present in a
//! channel, their channel model overrides, and the channel's standing push approvals.
//!
//! Resolving what each agent does on this device needs the device's provider support, so the
//! caller combines these stored values with [`crate::coordination::access::resolve_agent_access`].

use crate::coordination::access::{
    APP_AGENT_SETTINGS_ID, compose_instructions, effective_channel_access,
};
use crate::models::{
    ChatAgentAccess, ChatAppAgentSettingsRead, ChatChannelId, ChatChannelModelOverrideRead,
    ChatChannelPushApprovalRead, ChatConversationId, ChatEffectiveChannelAccessRead, ChatError,
    ChatErrorCode, ChatModelSelection, ChatParticipantId, ChatProjectAgentSettingsRead, ChatResult,
    ProjectWorkingFolderId, UtcTimestamp,
};
use sqlx::SqliteConnection;

/// The stored channel scope of agent settings with the access it resolves to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelAgentSettingsRecord {
    pub channel_id: ChatChannelId,
    pub conversation_id: ChatConversationId,
    pub project_id: String,
    /// The channel's own access, absent when it inherits.
    pub access: Option<ChatAgentAccess>,
    pub instructions: String,
    pub effective: ChatEffectiveChannelAccessRead,
    pub revision: u64,
}

/// An AI agent that is a present member of a channel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelAgentMember {
    pub agent_id: ChatParticipantId,
    pub provider_family_id: String,
}

/// Reads the app scope of agent settings.
pub async fn read_app_agent_settings(
    connection: &mut SqliteConnection,
) -> ChatResult<ChatAppAgentSettingsRead> {
    let row: Option<(String, String, i64, String)> = sqlx::query_as(
        "SELECT agent_access, agent_instructions, revision, updated_at
         FROM chat_agent_app_settings WHERE id = ?",
    )
    .bind(APP_AGENT_SETTINGS_ID)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let (access, instructions, revision, updated_at) = row.ok_or_else(corrupt_data_error)?;
    Ok(ChatAppAgentSettingsRead {
        access: parse_access(&access)?,
        instructions,
        revision: parse_revision(revision)?,
        updated_at: UtcTimestamp::new(updated_at).map_err(|_| corrupt_data_error())?,
    })
}

/// Reads the project scope of agent settings. A project that never saved its own settings
/// inherits the app access, has no instructions, and reads revision zero.
pub async fn read_project_agent_settings(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> ChatResult<ChatProjectAgentSettingsRead> {
    let app = read_app_agent_settings(connection).await?;
    let row: Option<(Option<String>, String, i64)> = sqlx::query_as(
        "SELECT agent_access, agent_instructions, revision
         FROM chat_project_agent_settings WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let (access, instructions, revision) = match row {
        Some((access, instructions, revision)) => (
            access.as_deref().map(parse_access).transpose()?,
            instructions,
            parse_revision(revision)?,
        ),
        None => (None, String::new(), 0),
    };
    let (level, source) = effective_channel_access(None, access, app.access);
    Ok(ChatProjectAgentSettingsRead {
        project_id: project_id.to_string(),
        access,
        instructions,
        effective: ChatEffectiveChannelAccessRead {
            access: level,
            source,
        },
        revision,
    })
}

/// Stored channel, project, and app values of one channel, in that order.
struct ChannelScopes {
    channel_id: String,
    project_id: String,
    channel_access: Option<String>,
    channel_instructions: String,
    revision: i64,
    project_access: Option<String>,
    project_instructions: Option<String>,
    app_access: Option<String>,
    app_instructions: Option<String>,
}

async fn read_channel_scopes(
    connection: &mut SqliteConnection,
    conversation_id: &str,
) -> ChatResult<ChannelScopes> {
    type Row = (
        String,
        String,
        Option<String>,
        String,
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT channel.id, channel.project_id, channel.agent_access, channel.agent_instructions,
                channel.revision, project.agent_access, project.agent_instructions,
                app.agent_access, app.agent_instructions
         FROM chat_channels channel
         LEFT JOIN chat_project_agent_settings project ON project.project_id = channel.project_id
         LEFT JOIN chat_agent_app_settings app ON app.id = ?2
         WHERE channel.conversation_id = ?1",
    )
    .bind(conversation_id)
    .bind(APP_AGENT_SETTINGS_ID)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let row = row.ok_or_else(|| {
        ChatError::new(ChatErrorCode::NotFound, "Chat channel was not found", false)
    })?;
    Ok(ChannelScopes {
        channel_id: row.0,
        project_id: row.1,
        channel_access: row.2,
        channel_instructions: row.3,
        revision: row.4,
        project_access: row.5,
        project_instructions: row.6,
        app_access: row.7,
        app_instructions: row.8,
    })
}

/// Reads the channel scope of agent settings of the channel whose conversation is
/// `conversation_id`, with the access it resolves to.
pub async fn read_channel_agent_settings(
    connection: &mut SqliteConnection,
    conversation_id: &str,
) -> ChatResult<ChannelAgentSettingsRecord> {
    let scopes = read_channel_scopes(connection, conversation_id).await?;
    let access = scopes
        .channel_access
        .as_deref()
        .map(parse_access)
        .transpose()?;
    let project = scopes
        .project_access
        .as_deref()
        .map(parse_access)
        .transpose()?;
    let app = parse_access(
        scopes
            .app_access
            .as_deref()
            .ok_or_else(corrupt_data_error)?,
    )?;
    let (level, source) = effective_channel_access(access, project, app);
    Ok(ChannelAgentSettingsRecord {
        channel_id: ChatChannelId::new(scopes.channel_id).map_err(|_| corrupt_data_error())?,
        conversation_id: ChatConversationId::new(conversation_id.to_string())
            .map_err(|_| corrupt_data_error())?,
        project_id: scopes.project_id,
        access,
        instructions: scopes.channel_instructions,
        effective: ChatEffectiveChannelAccessRead {
            access: level,
            source,
        },
        revision: parse_revision(scopes.revision)?,
    })
}

/// Reads the app, project, and channel instructions of a channel composed with
/// [`compose_instructions`], as work in the channel starts with them.
pub async fn read_channel_instructions(
    connection: &mut SqliteConnection,
    conversation_id: &str,
) -> ChatResult<String> {
    let scopes = read_channel_scopes(connection, conversation_id).await?;
    Ok(compose_instructions(
        scopes.app_instructions.as_deref().unwrap_or_default(),
        scopes.project_instructions.as_deref().unwrap_or_default(),
        &scopes.channel_instructions,
    ))
}

/// Returns the AI agents that are present members of a conversation: both the conversation
/// membership and the AI channel membership are unremoved and the agent is not archived.
pub async fn read_channel_agent_members(
    connection: &mut SqliteConnection,
    conversation_id: &str,
) -> ChatResult<Vec<ChannelAgentMember>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT agent.participant_id, agent.provider_family_id
         FROM chat_agent_channel_memberships channel_membership
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = channel_membership.conversation_id
          AND membership.participant_id = channel_membership.agent_id
          AND membership.removed_at IS NULL
         JOIN chat_agents agent ON agent.participant_id = channel_membership.agent_id
         JOIN chat_participants participant ON participant.id = agent.participant_id
         WHERE channel_membership.conversation_id = ?
           AND channel_membership.removed_at IS NULL
           AND participant.archived_at IS NULL
         ORDER BY agent.participant_id",
    )
    .bind(conversation_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|(agent_id, provider_family_id)| {
            Ok(ChannelAgentMember {
                agent_id: ChatParticipantId::new(agent_id).map_err(|_| corrupt_data_error())?,
                provider_family_id,
            })
        })
        .collect()
}

/// Returns whether `agent_id` is a present, unarchived AI agent member of a conversation.
pub async fn agent_is_present(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    agent_id: &str,
) -> ChatResult<bool> {
    Ok(read_channel_agent_members(connection, conversation_id)
        .await?
        .iter()
        .any(|member| member.agent_id.as_str() == agent_id))
}

/// Returns the model overrides of the unremoved AI channel memberships of a conversation.
pub async fn read_channel_model_overrides(
    connection: &mut SqliteConnection,
    conversation_id: &str,
) -> ChatResult<Vec<ChatChannelModelOverrideRead>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT agent_id, model_selection_data FROM chat_agent_channel_memberships
         WHERE conversation_id = ? AND removed_at IS NULL
           AND model_selection_data IS NOT NULL
         ORDER BY agent_id",
    )
    .bind(conversation_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|(agent_id, data)| {
            Ok(ChatChannelModelOverrideRead {
                agent_id: ChatParticipantId::new(agent_id).map_err(|_| corrupt_data_error())?,
                model: parse_model_selection(&data)?,
            })
        })
        .collect()
}

/// Returns the model override of one agent in one conversation, absent when the channel uses
/// the agent's default model or the agent has no unremoved AI channel membership.
pub async fn read_channel_model_override(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    agent_id: &str,
) -> ChatResult<Option<ChatModelSelection>> {
    let data: Option<Option<String>> = sqlx::query_scalar(
        "SELECT model_selection_data FROM chat_agent_channel_memberships
         WHERE conversation_id = ? AND agent_id = ? AND removed_at IS NULL",
    )
    .bind(conversation_id)
    .bind(agent_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    data.flatten()
        .map(|data| parse_model_selection(&data))
        .transpose()
}

/// Returns the unrevoked push approvals of a conversation, newest first. The approval id is the
/// row's sync key, which names it on every device.
pub async fn read_channel_push_approvals(
    connection: &mut SqliteConnection,
    conversation_id: &str,
) -> ChatResult<Vec<ChatChannelPushApprovalRead>> {
    type Row = (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT approval.sync_key, channel.id, approval.agent_id, approval.working_folder_id,
                folder.display_name, approval.push_url, approval.destination_branch,
                approval.approved_at
         FROM chat_push_approvals approval
         JOIN chat_channels channel ON channel.conversation_id = approval.conversation_id
         JOIN project_working_folders folder ON folder.id = approval.working_folder_id
         WHERE approval.conversation_id = ? AND approval.revoked_at IS NULL
         ORDER BY approval.approved_at DESC, approval.sync_key",
    )
    .bind(conversation_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(
            |(approval_id, channel_id, agent_id, folder_id, folder_name, url, branch, at)| {
                Ok(ChatChannelPushApprovalRead {
                    approval_id,
                    channel_id: ChatChannelId::new(channel_id).map_err(|_| corrupt_data_error())?,
                    agent_id: ChatParticipantId::new(agent_id).map_err(|_| corrupt_data_error())?,
                    working_folder_id: ProjectWorkingFolderId::new(folder_id)
                        .map_err(|_| corrupt_data_error())?,
                    working_folder_name: folder_name,
                    push_url: url,
                    destination_branch: branch,
                    approved_at: UtcTimestamp::new(at).map_err(|_| corrupt_data_error())?,
                })
            },
        )
        .collect()
}

fn parse_access(value: &str) -> ChatResult<ChatAgentAccess> {
    ChatAgentAccess::parse(value).ok_or_else(corrupt_data_error)
}

fn parse_revision(value: i64) -> ChatResult<u64> {
    u64::try_from(value).map_err(|_| corrupt_data_error())
}

fn parse_model_selection(data: &str) -> ChatResult<ChatModelSelection> {
    serde_json::from_str(data).map_err(|_| corrupt_data_error())
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat agent settings persistence failed",
        true,
    )
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat agent settings are invalid",
        false,
    )
}

#[cfg(test)]
mod tests;
