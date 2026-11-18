//! What an agent may do in a channel on this device, and the project folders its access
//! reaches.

use super::super::persistence_error;
use super::super::provider_agents::DeviceProviders;
use ganbaru_chat::coordination::access::{ResolvedAgentAccess, resolve_agent_access};
use ganbaru_chat::coordination::agent_settings::{agent_is_present, read_channel_agent_settings};
use ganbaru_chat_contracts::models::*;
use sqlx::SqliteConnection;

/// The access one agent has in one channel on this device.
#[derive(Clone, Debug)]
pub(in crate::chat::organization) struct ChannelAgentAccess {
    /// The access the channel resolves to, before the provider narrows it.
    pub(in crate::chat::organization) level: ChatAgentAccess,
    /// What the agent's provider on this device enforces under `level`.
    pub(in crate::chat::organization) resolved: ResolvedAgentAccess,
    /// The project that owns the channel.
    pub(in crate::chat::organization) project_id: String,
    /// The provider installation that runs the agent here, when one is configured.
    pub(in crate::chat::organization) provider_instance_id: Option<ProviderInstanceId>,
}

impl ChannelAgentAccess {
    /// Stored spelling of the access the agent runs under here, or an empty string when it
    /// cannot work on this device.
    pub(in crate::chat::organization) fn effective_access_name(&self) -> &'static str {
        self.resolved
            .effective
            .map_or("", ChatEffectiveAgentAccess::as_str)
    }
}

/// Resolves the access of `agent_id` in the channel of `conversation_id` with the provider this
/// device runs for the agent's family. The agent does not have to be a member.
pub(in crate::chat::organization) async fn resolve_channel_agent_access(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    agent_id: &str,
    providers: &DeviceProviders,
) -> ChatResult<ChannelAgentAccess> {
    let settings = read_channel_agent_settings(connection, conversation_id).await?;
    let provider_family_id: String =
        sqlx::query_scalar("SELECT provider_family_id FROM chat_agents WHERE participant_id = ?")
            .bind(agent_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(persistence_error)?
            .ok_or_else(|| {
                ChatError::new(ChatErrorCode::NotFound, "The agent was not found", true)
            })?;
    let level = settings.effective.access;
    let support = providers.support(&provider_family_id);
    Ok(ChannelAgentAccess {
        level,
        resolved: resolve_agent_access(level, &support),
        project_id: settings.project_id,
        provider_instance_id: providers
            .get(&provider_family_id)
            .map(|provider| provider.instance_id.clone()),
    })
}

/// Resolves the access of `agent_id` in the channel of `conversation_id` for work: the agent
/// must be a present member and able to work on this device.
pub(in crate::chat::organization) async fn require_working_agent_access(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    agent_id: &str,
    providers: &DeviceProviders,
) -> ChatResult<ChannelAgentAccess> {
    if !agent_is_present(connection, conversation_id, agent_id).await? {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "The agent cannot participate in this channel",
            true,
        ));
    }
    let access =
        resolve_channel_agent_access(connection, conversation_id, agent_id, providers).await?;
    if !access.resolved.can_work() {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "The agent's provider cannot work in this channel on this device",
            true,
        ));
    }
    Ok(access)
}

/// A project folder that channel access reaches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::chat::organization) struct ReachedFolder {
    pub(in crate::chat::organization) working_folder_id: String,
    /// Whether the folder is the project's primary folder.
    pub(in crate::chat::organization) is_primary: bool,
}

/// Lists the folders `access` reaches: every non-archived folder of its project, with the
/// primary folder marked, or none when the access reaches no folder.
pub(in crate::chat::organization) async fn read_reached_folders(
    connection: &mut SqliteConnection,
    access: &ChannelAgentAccess,
) -> ChatResult<Vec<ReachedFolder>> {
    if access.resolved.folder_capability == ChatFolderCapability::None {
        return Ok(Vec::new());
    }
    let primary = ganbaru_chat::repository::workspaces::resolve_primary_workspace(
        &mut *connection,
        &access.project_id,
    )
    .await?
    .map(|folder| folder.id.as_str().to_string());
    let folders: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM project_working_folders
         WHERE project_id = ? AND archived_at IS NULL
         ORDER BY id",
    )
    .bind(&access.project_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(folders
        .into_iter()
        .map(|working_folder_id| ReachedFolder {
            is_primary: primary.as_deref() == Some(working_folder_id.as_str()),
            working_folder_id,
        })
        .collect())
}
