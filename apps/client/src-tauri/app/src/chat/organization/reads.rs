//! SQLite read models for organizational Chat state.

use super::common::{
    PageCursor, parse_json, parse_participant_kind, parse_review_outcome, parse_work_state,
    stored_value_error, u32_value,
};
use super::inputs::AWAITING_DELIVERY_SQL;
use super::workflow::{parse_model_selection, read_message_references};
use super::{
    LOCAL_PARTICIPANT_ID, identifier_error, now_timestamp, optional_timestamp, persistence_error,
    timestamp, u64_value,
};
use crate::chat::channels::{sync_key, unseen_thread_failure_sql};
use ganbaru_chat::conversion::reply_cursor_key;
use ganbaru_chat_contracts::models::*;
use sqlx::{AssertSqlSafe, Row, SqlitePool};

/// The provider instance that runs each provider family's agent on this device, by family id.
pub(crate) type LocalProviderInstances = std::collections::BTreeMap<String, ProviderInstanceId>;

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
        "SELECT membership.revision, membership.removed_at,
                EXISTS(
                    SELECT 1 FROM chat_agent_channel_memberships agent_membership
                    WHERE agent_membership.conversation_id = membership.conversation_id
                      AND agent_membership.agent_id = membership.participant_id
                      AND agent_membership.removed_at IS NULL
                ) AS agent_in_channel
         FROM chat_conversation_memberships membership
         WHERE membership.conversation_id = ? AND membership.participant_id = ?",
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
    let removed_at = optional_timestamp(row.try_get("removed_at").map_err(persistence_error)?)?;
    let agent_in_channel = participant.kind == ChatParticipantKind::AiAgent
        && removed_at.is_none()
        && row
            .try_get::<i64, _>("agent_in_channel")
            .map_err(persistence_error)?
            != 0;
    Ok(ChatConversationMembershipRead {
        conversation_id: conversation_id.clone(),
        participant,
        agent_in_channel,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        removed_at,
    })
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

/// Reads one agent. `local_instances` maps each provider family to the provider instance that
/// runs it on this device, so the read says whether the agent is installed here.
pub(super) async fn read_agent(
    pool: &SqlitePool,
    agent_id: &ChatParticipantId,
    local_instances: &LocalProviderInstances,
) -> ChatResult<ChatAiAgentRead> {
    let lifecycle = super::agent_lifecycle::read_agent_lifecycle(pool, agent_id).await?;
    let row = sqlx::query(
        "SELECT agent.provider_family_id, policy.id AS policy_revision_id,
                (SELECT count(*) FROM chat_agent_channel_memberships membership
                 WHERE membership.agent_id = agent.participant_id
                   AND membership.removed_at IS NULL) AS channel_count
         FROM chat_agents agent
         LEFT JOIN chat_agent_effective_policies policy
           ON policy.agent_id = agent.participant_id
         WHERE agent.participant_id = ?",
    )
    .bind(agent_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "AI agent was not found", true))?;
    let policy_revision_id: Option<String> = row
        .try_get("policy_revision_id")
        .map_err(persistence_error)?;
    let latest_policy = match policy_revision_id {
        Some(policy_revision_id) => Some(read_policy(pool, agent_id, &policy_revision_id).await?),
        None => None,
    };
    let provider_family_id = ProviderFamilyId::new(
        row.try_get::<String, _>("provider_family_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let local_provider_instance_id = local_instances.get(provider_family_id.as_str()).cloned();
    Ok(ChatAiAgentRead {
        participant: read_participant(pool, agent_id).await?,
        configuration_state: if local_provider_instance_id.is_some() {
            ChatAgentConfigurationState::Healthy
        } else {
            ChatAgentConfigurationState::ProviderUnavailable
        },
        provider_family_id,
        local_provider_instance_id,
        latest_policy,
        channel_count: u64_value(row.try_get("channel_count").map_err(persistence_error)?)?,
        active_assignment_count: lifecycle.active_assignment_count,
    })
}

/// Reads one agent policy revision together with its display number.
pub(super) async fn read_policy(
    pool: &SqlitePool,
    agent_id: &ChatParticipantId,
    policy_revision_id: &str,
) -> ChatResult<ChatAgentPolicyRead> {
    let row = sqlx::query(
        "SELECT numbered.id, numbered.revision, numbered.model_selection_data,
                numbered.provider_options_schema_version, numbered.provider_options_data,
                numbered.interaction_mode, numbered.created_at
         FROM chat_agent_policy_revisions_numbered numbered
         WHERE numbered.agent_id = ? AND numbered.id = ?",
    )
    .bind(agent_id.as_str())
    .bind(policy_revision_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "Agent policy was not found", true))?;
    let selection = parse_model_selection(
        row.try_get("model_selection_data")
            .map_err(persistence_error)?,
    )?;
    let interaction_mode: String = row.try_get("interaction_mode").map_err(persistence_error)?;
    Ok(ChatAgentPolicyRead {
        id: ChatAgentPolicyRevisionId::new(
            row.try_get::<String, _>("id").map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        agent_id: agent_id.clone(),
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        provider_managed_model: selection.provider_managed_model,
        model_id: selection.model_id,
        model_options: selection.model_options,
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
        interaction_mode: match interaction_mode.as_str() {
            "build" => InteractionMode::Build,
            "plan" => InteractionMode::Plan,
            _ => return Err(stored_value_error("interaction mode")),
        },
        created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
    })
}

pub(super) async fn read_message(
    pool: &SqlitePool,
    item_id: &ChatConversationItemId,
) -> ChatResult<ChatMessageRead> {
    let row = sqlx::query(AssertSqlSafe(format!(
        "SELECT item.conversation_id, item.reply_thread_id, item.order_key, item.created_at,
                {AWAITING_DELIVERY_SQL} AS awaiting_delivery,
                message.author_participant_id, message.author_label_snapshot,
                message.current_revision_id,
                message.edited_at, revision.revision, revision.normalized_markdown,
                revision.rich_content_schema_version, revision.rich_content_data
         FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         JOIN chat_communication_message_revisions_numbered revision
           ON revision.id = message.current_revision_id
         WHERE item.id = ?"
    )))
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
    let author_id = row
        .try_get::<Option<String>, _>("author_participant_id")
        .map_err(persistence_error)?
        .map(ChatParticipantId::new)
        .transpose()
        .map_err(identifier_error)?;
    let author = match author_id {
        Some(participant_id) => Some(read_participant(pool, &participant_id).await?),
        None => None,
    };
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
        author,
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
        order_key: row.try_get("order_key").map_err(persistence_error)?,
        edited_at: optional_timestamp(row.try_get("edited_at").map_err(persistence_error)?)?,
        created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
        awaiting_delivery: row
            .try_get("awaiting_delivery")
            .map_err(persistence_error)?,
    })
}

pub(crate) async fn read_reply_thread_summary(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
) -> ChatResult<ChatReplyThreadSummaryRead> {
    let row = sqlx::query(AssertSqlSafe(format!(
        "SELECT thread.last_activity_at,
            (SELECT count(*)
             FROM chat_conversation_items item
             JOIN chat_communication_messages message ON message.item_id = item.id
             WHERE item.reply_thread_id = thread.id
               AND item.item_kind = 'message'
               AND message.deleted_at IS NULL
               AND NOT {AWAITING_DELIVERY_SQL}) AS reply_count
         FROM chat_reply_threads thread WHERE thread.id = ?"
    )))
    .bind(reply_thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "Reply thread was not found", true))?;
    let participants = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT message.author_participant_id
         FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         WHERE item.reply_thread_id = ? AND message.author_participant_id IS NOT NULL
         ORDER BY item.order_key DESC, item.id DESC LIMIT 3",
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
              AND message.author_participant_id IS NOT ?
              AND NOT EXISTS (
                  SELECT 1 FROM chat_reply_thread_read_cursors cursor
                  WHERE cursor.reply_thread_id = ? AND cursor.participant_id = ?
                    AND (cursor.last_read_order_key, cursor.last_read_item_id)
                        >= (item.order_key, item.id)
              )
         )",
    )
    .bind(reply_thread_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(reply_thread_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    let unseen_failure: i64 = sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT {} FROM chat_reply_threads thread WHERE thread.id = ?",
        unseen_thread_failure_sql("")
    )))
    .bind(reply_thread_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    Ok(ChatReplyThreadSummaryRead {
        id: reply_thread_id.clone(),
        reply_count,
        last_activity_at: timestamp(row.try_get("last_activity_at").map_err(persistence_error)?)?,
        participants: participant_reads,
        unread: unread != 0,
        unseen_failure: unseen_failure != 0,
        work_state: read_active_or_latest_assignment(pool, reply_thread_id)
            .await?
            .map(|assignment| assignment.state),
    })
}

pub(super) async fn read_reply_thread_page(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
    end: Option<PageCursor>,
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
        "SELECT id, order_key FROM chat_conversation_items
         WHERE reply_thread_id = ? AND item_kind = 'message'
           AND (? IS NULL OR (order_key, id) < (?, ?) OR (? AND order_key = ? AND id = ?))
         ORDER BY order_key DESC, id DESC LIMIT ?",
    )
    .bind(reply_thread_id.as_str())
    .bind(end.as_ref().map(|end| end.order_key.as_str()))
    .bind(end.as_ref().map(|end| end.order_key.as_str()))
    .bind(end.as_ref().map(|end| end.item_id.as_str()))
    .bind(end.as_ref().is_some_and(|end| end.inclusive))
    .bind(end.as_ref().map(|end| end.order_key.as_str()))
    .bind(end.as_ref().map(|end| end.item_id.as_str()))
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let previous_cursor = reply_rows
        .last()
        .filter(|_| reply_rows.len() == limit as usize)
        .map(|reply| {
            Ok::<_, sqlx::Error>(PageCursor::before(
                &reply.try_get::<String, _>("order_key")?,
                &reply.try_get::<String, _>("id")?,
            ))
        })
        .transpose()
        .map_err(persistence_error)?;
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
    mark_reply_thread_read(
        pool,
        reply_thread_id,
        LOCAL_PARTICIPANT_ID,
        &now_timestamp()?,
    )
    .await?;
    Ok(ChatReplyThreadPageRead {
        thread: ChatReplyThreadSummaryRead {
            unread: false,
            unseen_failure: false,
            ..summary
        },
        root_message: read_message(pool, &root_id).await?,
        replies,
        assignment: read_active_or_latest_assignment(pool, reply_thread_id).await?,
        agent_runs: runs,
        delegated_children: read_delegated_children(pool, reply_thread_id).await?,
        previous_cursor,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
    })
}

/// Advances a participant's read cursor of a reply thread to its last reply.
///
/// The cursor never moves backward and the row is not rewritten when the position does not
/// grow, because sync merges read cursors as a maximum.
pub(super) async fn mark_reply_thread_read(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
    participant_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let last = sqlx::query_as::<_, (String, String)>(
        "SELECT order_key, id FROM chat_conversation_items
         WHERE reply_thread_id = ?
         ORDER BY order_key DESC, id DESC LIMIT 1",
    )
    .bind(reply_thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some((order_key, item_id)) = last else {
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO chat_reply_thread_read_cursors
            (reply_thread_id, participant_id, sync_key, last_read_order_key,
             last_read_item_id, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(reply_thread_id, participant_id) DO UPDATE SET
            last_read_order_key = excluded.last_read_order_key,
            last_read_item_id = excluded.last_read_item_id,
            updated_at = excluded.updated_at
         WHERE (excluded.last_read_order_key, excluded.last_read_item_id)
             > (last_read_order_key, last_read_item_id)",
    )
    .bind(reply_thread_id.as_str())
    .bind(participant_id)
    .bind(sync_key(reply_cursor_key(
        reply_thread_id.as_str(),
        participant_id,
    ))?)
    .bind(order_key)
    .bind(item_id)
    .bind(now.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

pub(super) async fn read_assignment(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<ChatWorkAssignmentRead> {
    let row = sqlx::query(
        "SELECT reply_thread_id, agent_id, triggering_message_item_id,
                previous_assignment_id, state, state_reason, revision, settled_at,
                review_outcome, reviewed_at, reviewed_by_participant_id, created_at, updated_at,
                model_selection_data, instructions, model_label
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
    let agent_id = ChatParticipantId::new(
        row.try_get::<String, _>("agent_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let reply_thread_id = ChatReplyThreadId::new(
        row.try_get::<String, _>("reply_thread_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    Ok(ChatWorkAssignmentRead {
        id: assignment_id.clone(),
        latest_run_provider_thread_id: latest_run_provider_thread_id(pool, &reply_thread_id)
            .await?,
        reply_thread_id,
        agent: read_participant(pool, &agent_id).await?,
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
        delegation: read_assignment_delegation(pool, assignment_id).await?,
        model_selection: parse_model_selection(
            row.try_get("model_selection_data")
                .map_err(persistence_error)?,
        )?,
        instructions: row.try_get("instructions").map_err(persistence_error)?,
        model_label: row.try_get("model_label").map_err(persistence_error)?,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        settled_at: optional_timestamp(row.try_get("settled_at").map_err(persistence_error)?)?,
        created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
        updated_at: timestamp(row.try_get("updated_at").map_err(persistence_error)?)?,
        review: read_assignment_review(&row)?,
        local_acceptance: None,
    })
}

/// The provider thread of the newest run in a reply thread that executed on this device.
///
/// Runs are device-local, so a reply thread whose work ran elsewhere has none and the composer
/// shows no usage instead of inventing it.
async fn latest_run_provider_thread_id(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
) -> ChatResult<Option<ChatThreadId>> {
    sqlx::query_scalar::<_, String>(
        "SELECT run.provider_thread_id
         FROM chat_agent_runs run
         JOIN chat_work_assignments assignment ON assignment.id = run.assignment_id
         WHERE assignment.reply_thread_id = ? AND run.provider_thread_id IS NOT NULL
         ORDER BY assignment.created_at DESC, assignment.id DESC, run.run_ordinal DESC
         LIMIT 1",
    )
    .bind(reply_thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .map(ChatThreadId::new)
    .transpose()
    .map_err(identifier_error)
}

/// The review stored on an assignment row: all three review columns, or none.
fn read_assignment_review(
    row: &sqlx::sqlite::SqliteRow,
) -> ChatResult<Option<ChatAssignmentReviewRead>> {
    let outcome: Option<String> = row.try_get("review_outcome").map_err(persistence_error)?;
    let reviewed_at: Option<String> = row.try_get("reviewed_at").map_err(persistence_error)?;
    let reviewer: Option<String> = row
        .try_get("reviewed_by_participant_id")
        .map_err(persistence_error)?;
    match (outcome, reviewed_at, reviewer) {
        (None, None, None) => Ok(None),
        (Some(outcome), Some(reviewed_at), Some(reviewer)) => Ok(Some(ChatAssignmentReviewRead {
            outcome: parse_review_outcome(&outcome)?,
            reviewed_at: timestamp(reviewed_at)?,
            reviewed_by_participant_id: ChatParticipantId::new(reviewer)
                .map_err(identifier_error)?,
        })),
        _ => Err(stored_value_error("assignment review")),
    }
}

/// Where an assignment came from when an agent delegated it, or none for work a person started.
async fn read_assignment_delegation(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<Option<ChatAssignmentDelegationRead>> {
    let row = sqlx::query(
        "SELECT child.parent_assignment_id, child.delegation_depth, child.delegation_objective,
                child.delegation_expected_result, parent.reply_thread_id AS parent_reply_thread_id
         FROM chat_work_assignments child
         LEFT JOIN chat_work_assignments parent ON parent.id = child.parent_assignment_id
         WHERE child.id = ? AND child.parent_assignment_id IS NOT NULL",
    )
    .bind(assignment_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let depth: i64 = row.try_get("delegation_depth").map_err(persistence_error)?;
    let objective: Option<String> = row
        .try_get("delegation_objective")
        .map_err(persistence_error)?;
    let expected_result: Option<String> = row
        .try_get("delegation_expected_result")
        .map_err(persistence_error)?;
    let (Some(objective), Some(expected_result)) = (objective, expected_result) else {
        return Err(stored_value_error("assignment delegation"));
    };
    Ok(Some(ChatAssignmentDelegationRead {
        parent_assignment_id: ChatWorkAssignmentId::new(
            row.try_get::<String, _>("parent_assignment_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        parent_reply_thread_id: row
            .try_get::<Option<String>, _>("parent_reply_thread_id")
            .map_err(persistence_error)?
            .map(ChatReplyThreadId::new)
            .transpose()
            .map_err(identifier_error)?,
        depth: u8::try_from(depth).map_err(|_| stored_value_error("delegation depth"))?,
        objective,
        expected_result,
    }))
}

/// Work delegated from any assignment of a reply thread, oldest first, with its live state.
pub(super) async fn read_delegated_children(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
) -> ChatResult<Vec<ChatDelegatedChildRead>> {
    let rows = sqlx::query(
        "SELECT child.id, child.parent_assignment_id, child.reply_thread_id, child.agent_id,
                child.delegation_objective, child.state, child.state_reason, child.created_at
         FROM chat_work_assignments child
         JOIN chat_work_assignments parent ON parent.id = child.parent_assignment_id
         WHERE parent.reply_thread_id = ?
         ORDER BY child.created_at, child.id",
    )
    .bind(reply_thread_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut children = Vec::with_capacity(rows.len());
    for row in rows {
        let agent_id = ChatParticipantId::new(
            row.try_get::<String, _>("agent_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        children.push(ChatDelegatedChildRead {
            assignment_id: ChatWorkAssignmentId::new(
                row.try_get::<String, _>("id").map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            parent_assignment_id: ChatWorkAssignmentId::new(
                row.try_get::<String, _>("parent_assignment_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            reply_thread_id: ChatReplyThreadId::new(
                row.try_get::<String, _>("reply_thread_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            agent: read_participant(pool, &agent_id).await?,
            objective: row
                .try_get::<Option<String>, _>("delegation_objective")
                .map_err(persistence_error)?
                .ok_or_else(|| stored_value_error("assignment delegation"))?,
            state: parse_work_state(
                &row.try_get::<String, _>("state")
                    .map_err(persistence_error)?,
            )?,
            state_reason: row.try_get("state_reason").map_err(persistence_error)?,
            created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
        });
    }
    Ok(children)
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
                run.provider_turn_id,
                run.provider_thread_id, run.state,
                run.run_ordinal, run.attempt_reason, run.handoff_reason, run.created_at,
                run.updated_at
         FROM chat_agent_runs run
         JOIN chat_work_assignments assignment ON assignment.id = run.assignment_id
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
                attempt_reason: row.try_get("attempt_reason").map_err(persistence_error)?,
                handoff_reason: row.try_get("handoff_reason").map_err(persistence_error)?,
                created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
                updated_at: timestamp(row.try_get("updated_at").map_err(persistence_error)?)?,
            })
        })
        .collect()
}
