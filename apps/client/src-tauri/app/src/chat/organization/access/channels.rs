//! Current channel access and proposed membership resolution.

use super::{
    ChatAccessIssueCode, ChatAccessValidationIssue, ChatAgentAccessRead,
    ChatAgentChannelAccessInput, ChatAgentChannelAccessRead, ChatChannelCapabilities,
    ChatChannelId, ChatConversationId, ChatError, ChatErrorCode, ChatFolderGrantRead,
    ChatHistoryBoundary, ChatParticipantId, ChatResult, ChatRuntimeApprovalPolicy,
    ProjectWorkingFolderId,
};
use super::{ResolvedChannelAccess, issue};
use crate::chat::channels::{identifier_error, optional_timestamp, persistence_error, u64_value};
use crate::chat::organization::common::{
    ACTIVE_FOLDER_GRANTS_SQL, agent_runtime_approval, from_grant_lower_order_key,
    parse_approval_policy, parse_folder_capability, parse_history_boundary,
    parse_runtime_approval_policy,
};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeSet;

pub(in crate::chat::organization) async fn read_agent_access(
    pool: &SqlitePool,
    agent_id: &ChatParticipantId,
) -> ChatResult<ChatAgentAccessRead> {
    let state = sqlx::query(
        "SELECT access_state.access_revision, policy.safety_mode
         FROM chat_agent_access_state access_state
         JOIN chat_agents agent ON agent.participant_id = access_state.agent_id
         JOIN chat_agent_effective_policies policy
           ON policy.agent_id = agent.participant_id
         WHERE access_state.agent_id = ?",
    )
    .bind(agent_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "AI agent was not found", true))?;
    let rows = sqlx::query(
        "SELECT channel.id AS channel_id, channel.project_id, project.group_id,
                channel.conversation_id, channel.name AS channel_name,
                ai.read_history, ai.participate, ai.history_boundary,
                ai.history_from_order_key, ai.runtime_approval_policy,
                ai.scratch_runtime_approval_policy,
                membership.revision AS membership_revision, membership.removed_at
         FROM chat_agent_channel_memberships ai
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = ai.conversation_id
          AND membership.participant_id = ai.agent_id
         JOIN chat_channels channel ON channel.conversation_id = ai.conversation_id
         JOIN projects project ON project.id = channel.project_id
         WHERE ai.agent_id = ? AND ai.removed_at IS NULL
         ORDER BY project.group_id, channel.project_id, channel.name COLLATE NOCASE, channel.id",
    )
    .bind(agent_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut channels = Vec::with_capacity(rows.len());
    for row in rows {
        let conversation_id = ChatConversationId::new(
            row.try_get::<String, _>("conversation_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        let grant_rows = sqlx::query(ACTIVE_FOLDER_GRANTS_SQL)
            .bind(conversation_id.as_str())
            .bind(agent_id.as_str())
            .fetch_all(pool)
            .await
            .map_err(persistence_error)?;
        let mut folder_grants = Vec::with_capacity(grant_rows.len());
        for grant in grant_rows {
            folder_grants.push(ChatFolderGrantRead {
                working_folder_id: ProjectWorkingFolderId::new(
                    grant
                        .try_get::<String, _>("working_folder_id")
                        .map_err(persistence_error)?,
                )
                .map_err(identifier_error)?,
                display_name: grant.try_get("display_name").map_err(persistence_error)?,
                capability: parse_folder_capability(
                    &grant
                        .try_get::<String, _>("capability")
                        .map_err(persistence_error)?,
                )?,
                is_default: grant
                    .try_get::<i64, _>("is_default")
                    .map_err(persistence_error)?
                    != 0,
                runtime_approval_override: grant
                    .try_get::<Option<String>, _>("runtime_approval_policy")
                    .map_err(persistence_error)?
                    .as_deref()
                    .map(parse_runtime_approval_policy)
                    .transpose()?,
                revision: u64_value(grant.try_get("revision").map_err(persistence_error)?)?,
                revoked_at: optional_timestamp(
                    grant.try_get("revoked_at").map_err(persistence_error)?,
                )?,
            });
        }
        let history_boundary = parse_history_boundary(
            &row.try_get::<String, _>("history_boundary")
                .map_err(persistence_error)?,
            row.try_get("history_from_order_key")
                .map_err(persistence_error)?,
        )?;
        channels.push(ChatAgentChannelAccessRead {
            channel_id: ChatChannelId::new(
                row.try_get::<String, _>("channel_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            project_id: row.try_get("project_id").map_err(persistence_error)?,
            group_id: row.try_get("group_id").map_err(persistence_error)?,
            conversation_id,
            channel_name: row.try_get("channel_name").map_err(persistence_error)?,
            capabilities: ChatChannelCapabilities {
                read_history: row
                    .try_get::<i64, _>("read_history")
                    .map_err(persistence_error)?
                    != 0,
                participate: row
                    .try_get::<i64, _>("participate")
                    .map_err(persistence_error)?
                    != 0,
            },
            history_boundary,
            runtime_approval_override: row
                .try_get::<Option<String>, _>("runtime_approval_policy")
                .map_err(persistence_error)?
                .as_deref()
                .map(parse_runtime_approval_policy)
                .transpose()?,
            scratch_runtime_approval_override: row
                .try_get::<Option<String>, _>("scratch_runtime_approval_policy")
                .map_err(persistence_error)?
                .as_deref()
                .map(parse_runtime_approval_policy)
                .transpose()?,
            folder_grants,
            membership_revision: u64_value(
                row.try_get("membership_revision")
                    .map_err(persistence_error)?,
            )?,
            removed_at: optional_timestamp(row.try_get("removed_at").map_err(persistence_error)?)?,
        });
    }
    Ok(ChatAgentAccessRead {
        agent_id: agent_id.clone(),
        access_revision: u64_value(
            state
                .try_get("access_revision")
                .map_err(persistence_error)?,
        )?,
        agent_default_runtime_approval: agent_runtime_approval(parse_approval_policy(
            &state
                .try_get::<String, _>("safety_mode")
                .map_err(persistence_error)?,
        )?),
        channels,
    })
}

pub(super) async fn resolve_channel_access(
    pool: &SqlitePool,
    agent_id: &ChatParticipantId,
    channels: &[ChatAgentChannelAccessInput],
) -> ChatResult<(Vec<ResolvedChannelAccess>, Vec<ChatAccessValidationIssue>)> {
    let mut resolved = Vec::with_capacity(channels.len());
    let mut issues = Vec::new();
    let mut channel_ids = BTreeSet::new();
    for (channel_index, input) in channels.iter().enumerate() {
        let channel_path = format!("channels[{channel_index}]");
        if !channel_ids.insert(input.channel_id.as_str()) {
            issues.push(issue(
                ChatAccessIssueCode::DuplicateChannel,
                &format!("{channel_path}.channelId"),
                "Each channel may be selected only once",
            ));
            continue;
        }
        let channel_row = sqlx::query(
            "SELECT channel.project_id, project.group_id, channel.conversation_id,
                    channel.name, channel.archived_at
             FROM chat_channels channel
             JOIN projects project ON project.id = channel.project_id
             WHERE channel.id = ?",
        )
        .bind(input.channel_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
        let Some(channel_row) = channel_row else {
            issues.push(issue(
                ChatAccessIssueCode::ArchivedChannel,
                &format!("{channel_path}.channelId"),
                "The selected channel is unavailable",
            ));
            continue;
        };
        if channel_row
            .try_get::<Option<String>, _>("archived_at")
            .map_err(persistence_error)?
            .is_some()
        {
            issues.push(issue(
                ChatAccessIssueCode::ArchivedChannel,
                &format!("{channel_path}.channelId"),
                "Archived channels cannot receive agent access",
            ));
            continue;
        }
        let project_id: String = channel_row
            .try_get("project_id")
            .map_err(persistence_error)?;
        let mut folder_ids = BTreeSet::new();
        let mut defaults = 0_u32;
        for (grant_index, grant) in input.folder_grants.iter().enumerate() {
            let grant_path = format!("{channel_path}.folderGrants[{grant_index}]");
            if !folder_ids.insert(grant.working_folder_id.as_str()) {
                issues.push(issue(
                    ChatAccessIssueCode::DuplicateFolder,
                    &format!("{grant_path}.workingFolderId"),
                    "Each folder may be granted only once",
                ));
                continue;
            }
            defaults += u32::from(grant.is_default);
            let folder = sqlx::query(
                "SELECT project_id, archived_at FROM project_working_folders WHERE id = ?",
            )
            .bind(grant.working_folder_id.as_str())
            .fetch_optional(pool)
            .await
            .map_err(persistence_error)?;
            match folder {
                None => issues.push(issue(
                    ChatAccessIssueCode::ArchivedFolder,
                    &format!("{grant_path}.workingFolderId"),
                    "The selected folder is unavailable",
                )),
                Some(row)
                    if row
                        .try_get::<Option<String>, _>("archived_at")
                        .map_err(persistence_error)?
                        .is_some() =>
                {
                    issues.push(issue(
                        ChatAccessIssueCode::ArchivedFolder,
                        &format!("{grant_path}.workingFolderId"),
                        "Archived folders cannot be granted",
                    ));
                }
                Some(row)
                    if row
                        .try_get::<String, _>("project_id")
                        .map_err(persistence_error)?
                        != project_id =>
                {
                    issues.push(issue(
                        ChatAccessIssueCode::CrossProjectFolder,
                        &format!("{grant_path}.workingFolderId"),
                        "Folders must belong to the channel project",
                    ));
                }
                Some(_) => {}
            }
        }
        if defaults > 1 {
            issues.push(issue(
                ChatAccessIssueCode::MultipleDefaultFolders,
                &format!("{channel_path}.folderGrants"),
                "Choose at most one default execution folder",
            ));
        }
        let conversation_id = ChatConversationId::new(
            channel_row
                .try_get::<String, _>("conversation_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        let history_boundary = match input.history_boundary {
            ChatHistoryBoundary::Entire => ChatHistoryBoundary::Entire,
            ChatHistoryBoundary::FromGrant { .. } => {
                let current_lower = sqlx::query_scalar::<_, Option<String>>(
                    "SELECT history_from_order_key FROM chat_agent_channel_memberships
                     WHERE conversation_id = ? AND agent_id = ? AND removed_at IS NULL
                       AND history_boundary = 'from_grant'",
                )
                .bind(conversation_id.as_str())
                .bind(agent_id.as_str())
                .fetch_optional(pool)
                .await
                .map_err(persistence_error)?
                .flatten();
                let lower = match current_lower {
                    Some(value) => value,
                    None => {
                        from_grant_lower_order_key(
                            &mut *pool.acquire().await.map_err(persistence_error)?,
                            conversation_id.as_str(),
                        )
                        .await?
                    }
                };
                ChatHistoryBoundary::FromGrant {
                    lower_order_key: Some(lower),
                }
            }
        };
        resolved.push(ResolvedChannelAccess {
            channel_id: input.channel_id.clone(),
            project_id,
            group_id: channel_row.try_get("group_id").map_err(persistence_error)?,
            conversation_id,
            channel_name: channel_row.try_get("name").map_err(persistence_error)?,
            capabilities: input.capabilities,
            history_boundary,
            runtime_approval_override: input.runtime_approval_override,
            scratch_runtime_approval_override: input.scratch_runtime_approval_override,
            folder_grants: input.folder_grants.clone(),
        });
    }
    Ok((resolved, issues))
}

pub(super) fn proposed_access_read(
    agent_id: &ChatParticipantId,
    access_revision: u64,
    agent_default_runtime_approval: ChatRuntimeApprovalPolicy,
    resolved: &[ResolvedChannelAccess],
) -> ChatAgentAccessRead {
    ChatAgentAccessRead {
        agent_id: agent_id.clone(),
        access_revision,
        agent_default_runtime_approval,
        channels: resolved
            .iter()
            .map(|channel| ChatAgentChannelAccessRead {
                channel_id: channel.channel_id.clone(),
                project_id: channel.project_id.clone(),
                group_id: channel.group_id.clone(),
                conversation_id: channel.conversation_id.clone(),
                channel_name: channel.channel_name.clone(),
                capabilities: channel.capabilities,
                history_boundary: channel.history_boundary.clone(),
                runtime_approval_override: channel.runtime_approval_override,
                scratch_runtime_approval_override: channel.scratch_runtime_approval_override,
                folder_grants: channel
                    .folder_grants
                    .iter()
                    .map(|grant| ChatFolderGrantRead {
                        working_folder_id: grant.working_folder_id.clone(),
                        display_name: grant.working_folder_id.to_string(),
                        capability: grant.capability,
                        is_default: grant.is_default,
                        runtime_approval_override: grant.runtime_approval_override,
                        revision: 1,
                        revoked_at: None,
                    })
                    .collect(),
                membership_revision: 1,
                removed_at: None,
            })
            .collect(),
    }
}
