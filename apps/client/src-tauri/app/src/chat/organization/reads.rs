//! SQLite read models for organizational Chat state.

use super::common::{
    parse_access_profile_builtin_key, parse_approval_policy, parse_folder_capability,
    parse_history_boundary, parse_json, parse_participant_kind, parse_runtime_approval_policy,
    parse_work_state, u32_value,
};
use super::workflow::{parse_model_selection, read_message_references};
use super::{
    LOCAL_PARTICIPANT_ID, i64_value, identifier_error, now_timestamp, optional_timestamp,
    persistence_error, timestamp, u64_value,
};
use ganbaru_chat::coordination::access::history_boundary_is_expansion;
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, SqlitePool};

pub(crate) async fn read_memberships_for_conversation(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
    include_removed: bool,
) -> ChatResult<Vec<ChatConversationMembershipRead>> {
    let rows = sqlx::query(
        "SELECT participant_id
         FROM chat_conversation_memberships
         WHERE conversation_id = ? AND (? OR removed_at IS NULL)
         ORDER BY membership_role = 'owner' DESC, created_at, participant_id",
    )
    .bind(conversation_id.as_str())
    .bind(include_removed)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut memberships = Vec::with_capacity(rows.len());
    for row in rows {
        let participant_id = ChatParticipantId::new(
            row.try_get::<String, _>("participant_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        memberships.push(read_membership(pool, conversation_id, &participant_id).await?);
    }
    Ok(memberships)
}

pub(super) async fn read_membership(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
    participant_id: &ChatParticipantId,
) -> ChatResult<ChatConversationMembershipRead> {
    let row = sqlx::query(
        "SELECT revision, removed_at
         FROM chat_conversation_memberships
         WHERE conversation_id = ? AND participant_id = ?",
    )
    .bind(conversation_id.as_str())
    .bind(participant_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Channel membership was not found",
            true,
        )
    })?;
    let participant = read_participant(pool, participant_id).await?;
    let ai_access = if participant.kind == ChatParticipantKind::AiTeammate {
        read_roster_ai_access(pool, conversation_id, participant_id).await?
    } else {
        None
    };
    Ok(ChatConversationMembershipRead {
        conversation_id: conversation_id.clone(),
        participant,
        ai_access,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        removed_at: optional_timestamp(row.try_get("removed_at").map_err(persistence_error)?)?,
    })
}

async fn read_roster_ai_access(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
    teammate_id: &ChatParticipantId,
) -> ChatResult<Option<ChatChannelRosterAiSummary>> {
    let row = sqlx::query(
        "SELECT membership.access_profile_id, profile.latest_revision,
                profile.builtin_key, profile.display_name,
                profile_revision.default_read_history AS profile_read_history,
                profile_revision.default_participate AS profile_participate,
                profile_revision.default_history_boundary AS profile_history_boundary,
                profile_revision.maximum_folder_capability AS profile_folder_capability,
                membership.read_history, membership.read_history_inherits_profile,
                membership.participate, membership.participate_inherits_profile,
                membership.history_boundary,
                membership.history_boundary_inherits_profile,
                membership.history_from_ordinal,
                membership.runtime_approval_policy,
                membership.scratch_runtime_approval_policy
         FROM chat_teammate_channel_memberships membership
         JOIN chat_access_profiles profile ON profile.id = membership.access_profile_id
         JOIN chat_access_profile_revisions profile_revision
           ON profile_revision.access_profile_id = profile.id
          AND profile_revision.revision = profile.latest_revision
         WHERE membership.conversation_id = ? AND membership.teammate_id = ?",
    )
    .bind(conversation_id.as_str())
    .bind(teammate_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
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
    let mut folder_grants = Vec::with_capacity(grant_rows.len());
    let profile_folder_capability = parse_folder_capability(
        &row.try_get::<String, _>("profile_folder_capability")
            .map_err(persistence_error)?,
    )?;
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
        match profile_history_boundary {
            ChatHistoryBoundary::Entire => ChatHistoryBoundary::Entire,
            ChatHistoryBoundary::FromGrant { .. } => ChatHistoryBoundary::FromGrant {
                lower_ordinal: match stored_history_boundary {
                    ChatHistoryBoundary::FromGrant { lower_ordinal } => lower_ordinal,
                    ChatHistoryBoundary::Entire => None,
                },
            },
        }
    } else if history_boundary_is_expansion(&profile_history_boundary, &stored_history_boundary) {
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
    Ok(Some(ChatChannelRosterAiSummary {
        access_profile_id: ChatAccessProfileId::new(
            row.try_get::<String, _>("access_profile_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        access_profile_revision: u64_value(
            row.try_get("latest_revision").map_err(persistence_error)?,
        )?,
        access_profile_builtin_key: parse_access_profile_builtin_key(
            row.try_get::<Option<String>, _>("builtin_key")
                .map_err(persistence_error)?
                .as_deref(),
        )?,
        access_profile_name: row.try_get("display_name").map_err(persistence_error)?,
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
    }))
}

pub(super) async fn read_participant(
    pool: &SqlitePool,
    participant_id: &ChatParticipantId,
) -> ChatResult<ChatParticipantRead> {
    let row = sqlx::query(
        "SELECT participant_kind, display_name, avatar_schema_version,
                avatar_data, revision, archived_at
         FROM chat_participants WHERE id = ?",
    )
    .bind(participant_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Chat participant was not found",
            true,
        )
    })?;
    Ok(ChatParticipantRead {
        id: participant_id.clone(),
        kind: parse_participant_kind(
            &row.try_get::<String, _>("participant_kind")
                .map_err(persistence_error)?,
        )?,
        display_name: row.try_get("display_name").map_err(persistence_error)?,
        avatar: VersionedJson {
            schema_version: u32_value(
                row.try_get("avatar_schema_version")
                    .map_err(persistence_error)?,
            )?,
            value: parse_json(row.try_get("avatar_data").map_err(persistence_error)?)?,
        },
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        archived_at: optional_timestamp(row.try_get("archived_at").map_err(persistence_error)?)?,
    })
}

pub(super) async fn read_teammate(
    pool: &SqlitePool,
    teammate_id: &ChatParticipantId,
) -> ChatResult<ChatAiTeammateRead> {
    let lifecycle = super::teammate_lifecycle::read_teammate_lifecycle(pool, teammate_id).await?;
    let row = sqlx::query(
        "SELECT role, instructions, latest_policy_revision,
                (SELECT count(*) FROM chat_conversation_memberships membership
                 WHERE membership.participant_id = teammate.participant_id
                   AND membership.removed_at IS NULL) AS channel_count
         FROM chat_teammates teammate WHERE participant_id = ?",
    )
    .bind(teammate_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "AI teammate was not found", true))?;
    let latest_revision = u64_value(
        row.try_get("latest_policy_revision")
            .map_err(persistence_error)?,
    )?;
    Ok(ChatAiTeammateRead {
        participant: read_participant(pool, teammate_id).await?,
        role: row.try_get("role").map_err(persistence_error)?,
        instructions: row.try_get("instructions").map_err(persistence_error)?,
        configuration_state: if latest_revision == 0 {
            ChatTeammateConfigurationState::NeedsSetup
        } else {
            ChatTeammateConfigurationState::Healthy
        },
        latest_policy: if latest_revision == 0 {
            None
        } else {
            Some(read_policy(pool, teammate_id, latest_revision).await?)
        },
        channel_count: u64_value(row.try_get("channel_count").map_err(persistence_error)?)?,
        active_assignment_count: lifecycle.active_assignment_count,
        has_durable_history: lifecycle.has_durable_history,
    })
}

pub(super) async fn read_policy(
    pool: &SqlitePool,
    teammate_id: &ChatParticipantId,
    revision: u64,
) -> ChatResult<ChatTeammatePolicyRead> {
    let row = sqlx::query(
        "SELECT id, provider_instance_id, safety_mode, model_selection_schema_version,
                model_selection_data, effort, speed, provider_options_schema_version,
                provider_options_data, created_at
         FROM chat_teammate_policy_revisions
         WHERE teammate_id = ? AND revision = ?",
    )
    .bind(teammate_id.as_str())
    .bind(i64_value(revision)?)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Teammate policy was not found",
            true,
        )
    })?;
    let selection = parse_model_selection(
        row.try_get("model_selection_data")
            .map_err(persistence_error)?,
    )?;
    Ok(ChatTeammatePolicyRead {
        id: ChatTeammatePolicyRevisionId::new(
            row.try_get::<String, _>("id").map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        teammate_id: teammate_id.clone(),
        revision,
        provider_instance_id: ProviderInstanceId::new(
            row.try_get::<String, _>("provider_instance_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        safety_mode: parse_approval_policy(
            row.try_get::<String, _>("safety_mode")
                .map_err(persistence_error)?
                .as_str(),
        )?,
        provider_managed_model: selection.provider_managed_model,
        model_id: selection.model_id,
        model_options: selection.model_options,
        effort: row.try_get("effort").map_err(persistence_error)?,
        speed: row.try_get("speed").map_err(persistence_error)?,
        provider_options: VersionedJson {
            schema_version: u32_value(
                row.try_get("provider_options_schema_version")
                    .map_err(persistence_error)?,
            )?,
            value: parse_json(
                row.try_get("provider_options_data")
                    .map_err(persistence_error)?,
            )?,
        },
        created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
    })
}

pub(super) async fn read_message(
    pool: &SqlitePool,
    item_id: &ChatConversationItemId,
) -> ChatResult<ChatMessageRead> {
    let row = sqlx::query(
        "SELECT item.conversation_id, item.reply_thread_id, item.ordinal, item.created_at,
                message.author_participant_id, message.author_label_snapshot,
                message.current_revision_id,
                message.edited_at, revision.revision, revision.normalized_markdown,
                revision.rich_content_schema_version, revision.rich_content_data
         FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE item.id = ?",
    )
    .bind(item_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "Chat message was not found", true))?;
    let revision_id = ChatMessageRevisionId::new(
        row.try_get::<String, _>("current_revision_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let participant_id = ChatParticipantId::new(
        row.try_get::<String, _>("author_participant_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let references = read_message_references(pool, &revision_id).await?;
    let attachment_ids = sqlx::query_scalar::<_, String>(
        "SELECT attachment_id FROM chat_communication_attachment_references
         WHERE message_revision_id = ? ORDER BY ordinal",
    )
    .bind(revision_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?
    .into_iter()
    .map(ChatAttachmentId::new)
    .collect::<Result<Vec<_>, _>>()
    .map_err(identifier_error)?;
    let reply_thread_id = row
        .try_get::<Option<String>, _>("reply_thread_id")
        .map_err(persistence_error)?
        .map(ChatReplyThreadId::new)
        .transpose()
        .map_err(identifier_error)?;
    let root_thread_id = if reply_thread_id.is_none() {
        sqlx::query_scalar::<_, String>("SELECT id FROM chat_reply_threads WHERE root_item_id = ?")
            .bind(item_id.as_str())
            .fetch_optional(pool)
            .await
            .map_err(persistence_error)?
            .map(ChatReplyThreadId::new)
            .transpose()
            .map_err(identifier_error)?
    } else {
        None
    };
    Ok(ChatMessageRead {
        item_id: item_id.clone(),
        conversation_id: ChatConversationId::new(
            row.try_get::<String, _>("conversation_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        reply_thread_id,
        revision_id,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        author: read_participant(pool, &participant_id).await?,
        author_label_snapshot: row
            .try_get("author_label_snapshot")
            .map_err(persistence_error)?,
        normalized_markdown: row
            .try_get("normalized_markdown")
            .map_err(persistence_error)?,
        rich_content: VersionedJson {
            schema_version: u32_value(
                row.try_get("rich_content_schema_version")
                    .map_err(persistence_error)?,
            )?,
            value: parse_json(
                row.try_get("rich_content_data")
                    .map_err(persistence_error)?,
            )?,
        },
        attachment_ids,
        references,
        reply_thread: match root_thread_id {
            Some(thread_id) => Some(read_reply_thread_summary(pool, &thread_id).await?),
            None => None,
        },
        ordinal: u64_value(row.try_get("ordinal").map_err(persistence_error)?)?,
        edited_at: optional_timestamp(row.try_get("edited_at").map_err(persistence_error)?)?,
        created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
    })
}

pub(super) async fn read_reply_thread_summary(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
) -> ChatResult<ChatReplyThreadSummaryRead> {
    let row = sqlx::query(
        "SELECT reply_count, last_activity_at
         FROM chat_reply_threads WHERE id = ?",
    )
    .bind(reply_thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "Reply thread was not found", true))?;
    let participants = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT message.author_participant_id
         FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         WHERE item.reply_thread_id = ?
         ORDER BY item.ordinal DESC LIMIT 3",
    )
    .bind(reply_thread_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut participant_reads = Vec::with_capacity(participants.len());
    for participant_id in participants {
        participant_reads.push(
            read_participant(
                pool,
                &ChatParticipantId::new(participant_id).map_err(identifier_error)?,
            )
            .await?,
        );
    }
    let reply_count = u64_value(row.try_get("reply_count").map_err(persistence_error)?)?;
    let unread: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM chat_conversation_items item
            JOIN chat_communication_messages message ON message.item_id = item.id
            WHERE item.reply_thread_id = ?
              AND item.item_kind = 'message'
              AND message.author_participant_id != ?
              AND item.ordinal > coalesce((
                  SELECT cursor.last_read_reply_ordinal
                  FROM chat_reply_thread_read_cursors cursor
                  WHERE cursor.reply_thread_id = ? AND cursor.participant_id = ?
              ), 0)
         )",
    )
    .bind(reply_thread_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(reply_thread_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    Ok(ChatReplyThreadSummaryRead {
        id: reply_thread_id.clone(),
        reply_count,
        last_activity_at: timestamp(row.try_get("last_activity_at").map_err(persistence_error)?)?,
        participants: participant_reads,
        unread: unread != 0,
        work_state: read_active_or_latest_assignment(pool, reply_thread_id)
            .await?
            .map(|assignment| assignment.state),
    })
}

pub(super) async fn read_reply_thread_page(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
    before: Option<i64>,
    limit: u32,
) -> ChatResult<ChatReplyThreadPageRead> {
    let row = sqlx::query("SELECT root_item_id, revision FROM chat_reply_threads WHERE id = ?")
        .bind(reply_thread_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?
        .ok_or_else(|| {
            ChatError::new(ChatErrorCode::NotFound, "Reply thread was not found", true)
        })?;
    let root_id = ChatConversationItemId::new(
        row.try_get::<String, _>("root_item_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let reply_rows = sqlx::query(
        "SELECT id, ordinal FROM chat_conversation_items
         WHERE reply_thread_id = ? AND item_kind = 'message'
           AND (? IS NULL OR ordinal < ?)
         ORDER BY ordinal DESC LIMIT ?",
    )
    .bind(reply_thread_id.as_str())
    .bind(before)
    .bind(before)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let previous_cursor = reply_rows
        .last()
        .map(|reply| reply.try_get::<i64, _>("ordinal"))
        .transpose()
        .map_err(persistence_error)?
        .filter(|_| reply_rows.len() == limit as usize)
        .map(|ordinal| ordinal.to_string());
    let mut replies = Vec::with_capacity(reply_rows.len());
    for reply in reply_rows.into_iter().rev() {
        let item_id = ChatConversationItemId::new(
            reply
                .try_get::<String, _>("id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        replies.push(read_message(pool, &item_id).await?);
    }
    let runs = read_agent_runs(pool, reply_thread_id).await?;
    let summary = read_reply_thread_summary(pool, reply_thread_id).await?;
    let latest_ordinal: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(ordinal), 0)
         FROM chat_conversation_items WHERE reply_thread_id = ?",
    )
    .bind(reply_thread_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_reply_thread_read_cursors
            (reply_thread_id, participant_id, last_read_reply_ordinal, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(reply_thread_id, participant_id) DO UPDATE SET
            last_read_reply_ordinal = max(
                last_read_reply_ordinal,
                excluded.last_read_reply_ordinal
            ),
            updated_at = excluded.updated_at",
    )
    .bind(reply_thread_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(latest_ordinal)
    .bind(now_timestamp()?.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(ChatReplyThreadPageRead {
        thread: ChatReplyThreadSummaryRead {
            unread: false,
            ..summary
        },
        root_message: read_message(pool, &root_id).await?,
        replies,
        assignment: read_active_or_latest_assignment(pool, reply_thread_id).await?,
        agent_runs: runs,
        previous_cursor,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
    })
}

pub(super) async fn read_assignment(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<ChatWorkAssignmentRead> {
    let row = sqlx::query(
        "SELECT reply_thread_id, teammate_id, triggering_message_item_id,
                previous_assignment_id, state, state_reason, revision, settled_at,
                created_at, updated_at
         FROM chat_work_assignments WHERE id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Work assignment was not found",
            true,
        )
    })?;
    let teammate_id = ChatParticipantId::new(
        row.try_get::<String, _>("teammate_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    Ok(ChatWorkAssignmentRead {
        id: assignment_id.clone(),
        reply_thread_id: ChatReplyThreadId::new(
            row.try_get::<String, _>("reply_thread_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        teammate: read_participant(pool, &teammate_id).await?,
        triggering_message_item_id: ChatConversationItemId::new(
            row.try_get::<String, _>("triggering_message_item_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        previous_assignment_id: row
            .try_get::<Option<String>, _>("previous_assignment_id")
            .map_err(persistence_error)?
            .map(ChatWorkAssignmentId::new)
            .transpose()
            .map_err(identifier_error)?,
        state: parse_work_state(
            &row.try_get::<String, _>("state")
                .map_err(persistence_error)?,
        )?,
        state_reason: row.try_get("state_reason").map_err(persistence_error)?,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        settled_at: optional_timestamp(row.try_get("settled_at").map_err(persistence_error)?)?,
        created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
        updated_at: timestamp(row.try_get("updated_at").map_err(persistence_error)?)?,
    })
}

pub(super) async fn read_active_or_latest_assignment(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
) -> ChatResult<Option<ChatWorkAssignmentRead>> {
    let id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM chat_work_assignments
         WHERE reply_thread_id = ?
         ORDER BY
           CASE WHEN state IN (
             'queued', 'working', 'waiting_for_answer', 'waiting_for_approval', 'ready_for_review'
           ) THEN 0 ELSE 1 END,
           created_at DESC
         LIMIT 1",
    )
    .bind(reply_thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    match id {
        Some(id) => Ok(Some(
            read_assignment(
                pool,
                &ChatWorkAssignmentId::new(id).map_err(identifier_error)?,
            )
            .await?,
        )),
        None => Ok(None),
    }
}

pub(super) async fn read_agent_runs(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
) -> ChatResult<Vec<ChatAgentRunRead>> {
    let rows = sqlx::query(
        "SELECT run.id, run.assignment_id, run.project_id, run.working_folder_id,
                run.execution_environment_id, run.scratch_generation_id,
                run.teammate_policy_revision_id, policy.effort, run.provider_turn_id,
                run.provider_thread_id, run.state,
                run.run_ordinal, run.created_at, run.updated_at
         FROM chat_agent_runs run
         JOIN chat_work_assignments assignment ON assignment.id = run.assignment_id
         JOIN chat_teammate_policy_revisions policy ON policy.id = run.teammate_policy_revision_id
         WHERE assignment.reply_thread_id = ?
         ORDER BY assignment.created_at, run.run_ordinal",
    )
    .bind(reply_thread_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(ChatAgentRunRead {
                id: ChatAgentRunId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
                    .map_err(identifier_error)?,
                assignment_id: ChatWorkAssignmentId::new(
                    row.try_get::<String, _>("assignment_id")
                        .map_err(persistence_error)?,
                )
                .map_err(identifier_error)?,
                project_id: row.try_get("project_id").map_err(persistence_error)?,
                working_folder_id: row
                    .try_get::<Option<String>, _>("working_folder_id")
                    .map_err(persistence_error)?
                    .map(ProjectWorkingFolderId::new)
                    .transpose()
                    .map_err(identifier_error)?,
                execution_environment_id: ChatExecutionEnvironmentId::new(
                    row.try_get::<String, _>("execution_environment_id")
                        .map_err(persistence_error)?,
                )
                .map_err(identifier_error)?,
                scratch_generation_id: row
                    .try_get::<Option<String>, _>("scratch_generation_id")
                    .map_err(persistence_error)?
                    .map(ChatScratchGenerationId::new)
                    .transpose()
                    .map_err(identifier_error)?,
                teammate_policy_revision_id: ChatTeammatePolicyRevisionId::new(
                    row.try_get::<String, _>("teammate_policy_revision_id")
                        .map_err(persistence_error)?,
                )
                .map_err(identifier_error)?,
                effort: row.try_get("effort").map_err(persistence_error)?,
                provider_execution_turn_id: ChatTurnId::new(
                    row.try_get::<String, _>("provider_turn_id")
                        .map_err(persistence_error)?,
                )
                .map_err(identifier_error)?,
                provider_execution_thread_id: row
                    .try_get::<Option<String>, _>("provider_thread_id")
                    .map_err(persistence_error)?
                    .map(ChatThreadId::new)
                    .transpose()
                    .map_err(identifier_error)?,
                state: row.try_get("state").map_err(persistence_error)?,
                run_ordinal: u64_value(row.try_get("run_ordinal").map_err(persistence_error)?)?,
                created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
                updated_at: timestamp(row.try_get("updated_at").map_err(persistence_error)?)?,
            })
        })
        .collect()
}
