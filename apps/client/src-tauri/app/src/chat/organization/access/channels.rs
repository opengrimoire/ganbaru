//! Current channel access and proposed membership resolution.

use super::profiles::read_access_profile;
use super::{
    ChatAccessIssueCode, ChatAccessProfileId, ChatAccessValidationIssue, ChatChannelCapabilities,
    ChatChannelId, ChatConversationId, ChatError, ChatErrorCode, ChatFolderGrantRead,
    ChatHistoryBoundary, ChatParticipantId, ChatResult, ChatRuntimeApprovalPolicy,
    ChatTeammateAccessRead, ChatTeammateChannelAccessInput, ChatTeammateChannelAccessRead,
    ProjectWorkingFolderId,
};
use super::{ResolvedChannelAccess, history_boundary_is_expansion, issue};
use crate::chat::channels::{identifier_error, optional_timestamp, persistence_error, u64_value};
use crate::chat::organization::common::{
    parse_folder_capability, parse_history_boundary, parse_runtime_approval_policy,
};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeSet;

pub(in crate::chat::organization) async fn read_teammate_access(
    pool: &SqlitePool,
    teammate_id: &ChatParticipantId,
) -> ChatResult<ChatTeammateAccessRead> {
    let state = sqlx::query(
        "SELECT access_revision, runtime_approval_policy
         FROM chat_teammate_access_state WHERE teammate_id = ?",
    )
    .bind(teammate_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "AI teammate was not found", true))?;
    let rows = sqlx::query(
        "SELECT channel.id AS channel_id, channel.project_id, project.group_id,
                channel.conversation_id, channel.name AS channel_name,
                ai.access_profile_id, profile.latest_revision AS access_profile_revision,
                profile_revision.default_read_history AS profile_read_history,
                profile_revision.default_participate AS profile_participate,
                profile_revision.default_history_boundary AS profile_history_boundary,
                profile_revision.maximum_folder_capability AS profile_folder_capability,
                ai.read_history, ai.read_history_inherits_profile,
                ai.participate, ai.participate_inherits_profile,
                ai.history_boundary, ai.history_boundary_inherits_profile,
                ai.history_from_ordinal, ai.runtime_approval_policy,
                ai.scratch_runtime_approval_policy,
                membership.revision AS membership_revision, membership.removed_at
         FROM chat_teammate_channel_memberships ai
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = ai.conversation_id
          AND membership.participant_id = ai.teammate_id
         JOIN chat_channels channel ON channel.conversation_id = ai.conversation_id
         JOIN projects project ON project.id = channel.project_id
         JOIN chat_access_profiles profile ON profile.id = ai.access_profile_id
         JOIN chat_access_profile_revisions profile_revision
           ON profile_revision.access_profile_id = profile.id
          AND profile_revision.revision = profile.latest_revision
         WHERE ai.teammate_id = ?
         ORDER BY project.group_id, channel.project_id, channel.name COLLATE NOCASE, channel.id",
    )
    .bind(teammate_id.as_str())
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
        let grant_rows = sqlx::query(
            "SELECT grant_row.working_folder_id, folder.display_name,
                    grant_row.capability, grant_row.capability_inherits_profile,
                    grant_row.is_default,
                    grant_row.runtime_approval_policy, grant_row.revision,
                    grant_row.revoked_at
             FROM chat_teammate_working_folder_grants grant_row
             JOIN project_working_folders folder ON folder.id = grant_row.working_folder_id
             WHERE grant_row.conversation_id = ? AND grant_row.teammate_id = ?
               AND grant_row.revoked_at IS NULL
             ORDER BY grant_row.is_default DESC, folder.sort_order, folder.display_name, folder.id",
        )
        .bind(conversation_id.as_str())
        .bind(teammate_id.as_str())
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
        let profile_folder_capability = parse_folder_capability(
            &row.try_get::<String, _>("profile_folder_capability")
                .map_err(persistence_error)?,
        )?;
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
                capability: if grant
                    .try_get::<i64, _>("capability_inherits_profile")
                    .map_err(persistence_error)?
                    != 0
                {
                    profile_folder_capability
                } else {
                    parse_folder_capability(
                        &grant
                            .try_get::<String, _>("capability")
                            .map_err(persistence_error)?,
                    )?
                    .intersect(profile_folder_capability)
                },
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
        let profile_history_boundary = parse_history_boundary(
            &row.try_get::<String, _>("profile_history_boundary")
                .map_err(persistence_error)?,
            None,
        )?;
        let stored_history_boundary = parse_history_boundary(
            &row.try_get::<String, _>("history_boundary")
                .map_err(persistence_error)?,
            row.try_get("history_from_ordinal")
                .map_err(persistence_error)?,
        )?;
        let history_inherits_profile = row
            .try_get::<i64, _>("history_boundary_inherits_profile")
            .map_err(persistence_error)?
            != 0;
        let effective_history_boundary = if history_inherits_profile {
            match profile_history_boundary.clone() {
                ChatHistoryBoundary::Entire => ChatHistoryBoundary::Entire,
                ChatHistoryBoundary::FromGrant { .. } => ChatHistoryBoundary::FromGrant {
                    lower_ordinal: match stored_history_boundary {
                        ChatHistoryBoundary::FromGrant { lower_ordinal } => lower_ordinal,
                        ChatHistoryBoundary::Entire => None,
                    },
                },
            }
        } else if history_boundary_is_expansion(&profile_history_boundary, &stored_history_boundary)
        {
            let lower_ordinal: i64 = sqlx::query_scalar(
                "SELECT coalesce(max(ordinal), 0) + 1
                 FROM chat_conversation_items
                 WHERE conversation_id = ? AND reply_thread_id IS NULL",
            )
            .bind(conversation_id.as_str())
            .fetch_one(pool)
            .await
            .map_err(persistence_error)?;
            ChatHistoryBoundary::FromGrant {
                lower_ordinal: Some(u64_value(lower_ordinal)?),
            }
        } else {
            stored_history_boundary
        };
        channels.push(ChatTeammateChannelAccessRead {
            channel_id: ChatChannelId::new(
                row.try_get::<String, _>("channel_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            project_id: row.try_get("project_id").map_err(persistence_error)?,
            group_id: row.try_get("group_id").map_err(persistence_error)?,
            conversation_id,
            channel_name: row.try_get("channel_name").map_err(persistence_error)?,
            access_profile_id: ChatAccessProfileId::new(
                row.try_get::<String, _>("access_profile_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            access_profile_revision: u64_value(
                row.try_get("access_profile_revision")
                    .map_err(persistence_error)?,
            )?,
            capabilities: ChatChannelCapabilities {
                read_history: if row
                    .try_get::<i64, _>("read_history_inherits_profile")
                    .map_err(persistence_error)?
                    != 0
                {
                    row.try_get::<i64, _>("profile_read_history")
                        .map_err(persistence_error)?
                        != 0
                } else {
                    row.try_get::<i64, _>("read_history")
                        .map_err(persistence_error)?
                        != 0
                        && row
                            .try_get::<i64, _>("profile_read_history")
                            .map_err(persistence_error)?
                            != 0
                },
                participate: if row
                    .try_get::<i64, _>("participate_inherits_profile")
                    .map_err(persistence_error)?
                    != 0
                {
                    row.try_get::<i64, _>("profile_participate")
                        .map_err(persistence_error)?
                        != 0
                } else {
                    row.try_get::<i64, _>("participate")
                        .map_err(persistence_error)?
                        != 0
                        && row
                            .try_get::<i64, _>("profile_participate")
                            .map_err(persistence_error)?
                            != 0
                },
            },
            history_boundary: effective_history_boundary,
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
    Ok(ChatTeammateAccessRead {
        teammate_id: teammate_id.clone(),
        access_revision: u64_value(
            state
                .try_get("access_revision")
                .map_err(persistence_error)?,
        )?,
        teammate_default_runtime_approval: parse_runtime_approval_policy(
            &state
                .try_get::<String, _>("runtime_approval_policy")
                .map_err(persistence_error)?,
        )?,
        channels,
    })
}

pub(super) async fn resolve_channel_access(
    pool: &SqlitePool,
    teammate_id: &ChatParticipantId,
    channels: &[ChatTeammateChannelAccessInput],
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
                "Archived channels cannot receive teammate access",
            ));
            continue;
        }
        let profile = match read_access_profile(pool, &input.access_profile_id).await {
            Ok(profile) if profile.archived_at.is_none() => profile,
            _ => {
                issues.push(issue(
                    ChatAccessIssueCode::ProfileNotFound,
                    &format!("{channel_path}.accessProfileId"),
                    "The selected access profile is unavailable",
                ));
                continue;
            }
        };
        if profile.latest_revision.revision != input.access_profile_revision {
            return Err(ChatError::new(
                ChatErrorCode::StaleRevision,
                "An access profile changed before teammate access could be previewed",
                true,
            ));
        }
        if input.capabilities.read_history
            && !profile
                .latest_revision
                .default_channel_capabilities
                .read_history
            || input.capabilities.participate
                && !profile
                    .latest_revision
                    .default_channel_capabilities
                    .participate
            || history_boundary_is_expansion(
                &profile.latest_revision.default_history_boundary,
                &input.history_boundary,
            )
        {
            issues.push(issue(
                ChatAccessIssueCode::ProfileCeilingExceeded,
                &format!("{channel_path}.capabilities"),
                "Channel access cannot exceed the linked profile",
            ));
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
            if grant.capability.rank() > profile.latest_revision.maximum_folder_capability.rank() {
                issues.push(issue(
                    ChatAccessIssueCode::ProfileCeilingExceeded,
                    &format!("{grant_path}.capability"),
                    "Folder authority cannot exceed the linked profile",
                ));
            }
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
                let current_lower = sqlx::query_scalar::<_, Option<i64>>(
                    "SELECT history_from_ordinal FROM chat_teammate_channel_memberships
                     WHERE conversation_id = ? AND teammate_id = ?
                       AND history_boundary = 'from_grant'",
                )
                .bind(conversation_id.as_str())
                .bind(teammate_id.as_str())
                .fetch_optional(pool)
                .await
                .map_err(persistence_error)?
                .flatten();
                let lower = match current_lower {
                    Some(value) => u64_value(value)?,
                    None => {
                        let value: i64 = sqlx::query_scalar(
                            "SELECT COALESCE(max(ordinal), 0) + 1
                             FROM chat_conversation_items
                             WHERE conversation_id = ? AND reply_thread_id IS NULL",
                        )
                        .bind(conversation_id.as_str())
                        .fetch_one(pool)
                        .await
                        .map_err(persistence_error)?;
                        u64_value(value)?
                    }
                };
                ChatHistoryBoundary::FromGrant {
                    lower_ordinal: Some(lower),
                }
            }
        };
        resolved.push(ResolvedChannelAccess {
            channel_id: input.channel_id.clone(),
            project_id,
            group_id: channel_row.try_get("group_id").map_err(persistence_error)?,
            conversation_id,
            channel_name: channel_row.try_get("name").map_err(persistence_error)?,
            access_profile_id: input.access_profile_id.clone(),
            access_profile_revision: profile.latest_revision.revision,
            profile_maximum_folder_capability: profile.latest_revision.maximum_folder_capability,
            profile_default_channel_capabilities: profile
                .latest_revision
                .default_channel_capabilities,
            profile_default_history_boundary: profile.latest_revision.default_history_boundary,
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
    teammate_id: &ChatParticipantId,
    access_revision: u64,
    teammate_default_runtime_approval: ChatRuntimeApprovalPolicy,
    resolved: &[ResolvedChannelAccess],
) -> ChatTeammateAccessRead {
    ChatTeammateAccessRead {
        teammate_id: teammate_id.clone(),
        access_revision,
        teammate_default_runtime_approval,
        channels: resolved
            .iter()
            .map(|channel| ChatTeammateChannelAccessRead {
                channel_id: channel.channel_id.clone(),
                project_id: channel.project_id.clone(),
                group_id: channel.group_id.clone(),
                conversation_id: channel.conversation_id.clone(),
                channel_name: channel.channel_name.clone(),
                access_profile_id: channel.access_profile_id.clone(),
                access_profile_revision: channel.access_profile_revision,
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
