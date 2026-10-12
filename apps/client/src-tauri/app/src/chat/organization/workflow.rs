//! Assignment routing, frozen context, and durable organizational messages.

use super::common::{
    conversation_item_id, has_thread_eligible_mention, json_object, message_revision_id, new_id,
    parse_participant_kind, reply_thread_id, serialization_error, wire_participant_kind,
};
#[cfg(desktop)]
use super::common::{wire_approval_policy, wire_work_state, work_assignment_id};
#[cfg(desktop)]
use super::context::freeze_context_package;
#[cfg(desktop)]
use super::now_timestamp;
use super::reads::{
    read_active_or_latest_assignment, read_assignment, read_message, read_participant,
};
use super::{
    LOCAL_PARTICIPANT_ID, StoredPostMessageReceipt, i64_value, identifier_error, persistence_error,
    u64_value,
};
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat_contracts::models::*;
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

#[derive(Clone, Debug)]
pub(super) struct ResolvedInvocation {
    pub(super) teammate_id: ChatParticipantId,
    pub(super) active_assignment: Option<ChatWorkAssignmentRead>,
    #[cfg(desktop)]
    pub(super) latest_assignment: Option<ChatWorkAssignmentRead>,
}

pub(super) struct AssignmentWrite {
    pub(super) assignment_id: Option<ChatWorkAssignmentId>,
    pub(super) input_queued: bool,
}

pub(super) fn has_authority_bearing_references(references: &[ChatMessageReference]) -> bool {
    references.iter().any(|reference| {
        matches!(
            reference,
            ChatMessageReference::Channel { .. }
                | ChatMessageReference::WorkingFolder { .. }
                | ChatMessageReference::WorkspacePath { .. }
                | ChatMessageReference::ExecutionEnvironment { .. }
        )
    })
}

pub(super) fn require_continuation_scope_is_unchanged(
    has_active_assignment: bool,
    references: &[ChatMessageReference],
) -> ChatResult<()> {
    if has_active_assignment && has_authority_bearing_references(references) {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Resource references require a new linked assignment after current work settles",
            true,
        ));
    }
    Ok(())
}

pub(super) async fn resolve_invoked_teammate(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
    reply_thread_id: Option<&ChatReplyThreadId>,
    references: &[ChatMessageReference],
) -> ChatResult<Option<ResolvedInvocation>> {
    let mut ai_mentions = Vec::new();
    for reference in references {
        let ChatMessageReference::Participant {
            participant_id,
            participant_kind,
            ..
        } = reference
        else {
            continue;
        };
        let participant = read_participant(pool, participant_id).await?;
        if participant.kind != *participant_kind {
            return Err(ChatError::validation(
                "references",
                "A participant reference has stale identity data",
            ));
        }
        if participant.kind == ChatParticipantKind::AiTeammate {
            ai_mentions.push(participant_id.clone());
        }
    }
    ai_mentions.sort();
    ai_mentions.dedup();
    if ai_mentions.len() > 1 {
        return Err(ChatError::validation(
            "references",
            "Assign one AI teammate at a time",
        ));
    }
    let latest_assignment = match reply_thread_id {
        Some(reply_thread_id) => read_active_or_latest_assignment(pool, reply_thread_id).await?,
        None => None,
    };
    let active_assignment = latest_assignment
        .as_ref()
        .filter(|assignment| assignment.state.accepts_continuation())
        .cloned();
    let teammate_id = ai_mentions.into_iter().next().or_else(|| {
        latest_assignment
            .as_ref()
            .map(|assignment| assignment.teammate.id.clone())
    });
    let Some(teammate_id) = teammate_id else {
        return Ok(None);
    };
    if active_assignment
        .as_ref()
        .is_some_and(|assignment| assignment.teammate.id != teammate_id)
    {
        return Err(ChatError::validation(
            "references",
            "This reply thread already has an active AI teammate",
        ));
    }
    require_participating_teammate(pool, conversation_id, &teammate_id).await?;
    Ok(Some(ResolvedInvocation {
        teammate_id,
        active_assignment,
        #[cfg(desktop)]
        latest_assignment,
    }))
}

#[cfg(desktop)]
pub(super) async fn persist_assignment_routing(
    transaction: &mut Transaction<'_, Sqlite>,
    reply_thread_id: &ChatReplyThreadId,
    message_item_id: &ChatConversationItemId,
    invocation: Option<&ResolvedInvocation>,
    execution_target: Option<&ChatExecutionTarget>,
    now: &UtcTimestamp,
) -> ChatResult<AssignmentWrite> {
    let Some(invocation) = invocation else {
        return Ok(AssignmentWrite {
            assignment_id: None,
            input_queued: false,
        });
    };
    if let Some(active) = &invocation.active_assignment {
        let ordinal: i64 = sqlx::query_scalar(
            "SELECT coalesce(max(ordinal), 0) + 1
             FROM chat_work_assignment_inputs WHERE assignment_id = ?",
        )
        .bind(active.id.as_str())
        .fetch_one(&mut **transaction)
        .await
        .map_err(persistence_error)?;
        let routing_kind = if active.state == ChatWorkAssignmentState::Working {
            "steer"
        } else {
            "queued_continuation"
        };
        sqlx::query(
            "INSERT INTO chat_work_assignment_inputs
                (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id("assignment-input"))
        .bind(active.id.as_str())
        .bind(message_item_id.as_str())
        .bind(ordinal)
        .bind(routing_kind)
        .bind(now.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
        return Ok(AssignmentWrite {
            assignment_id: Some(active.id.clone()),
            input_queued: true,
        });
    }
    let assignment_id = work_assignment_id()?;
    let previous_id = invocation
        .latest_assignment
        .as_ref()
        .map(|assignment| &assignment.id);
    if let Some(previous) = invocation
        .latest_assignment
        .as_ref()
        .filter(|assignment| assignment.state == ChatWorkAssignmentState::ReadyForReview)
    {
        let settled = sqlx::query(
            "UPDATE chat_work_assignments
             SET state = 'completed', settled_at = ?, revision = revision + 1, updated_at = ?
             WHERE id = ? AND state = 'ready_for_review'",
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(previous.id.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
        if settled.rows_affected() != 1 {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "The reviewed assignment changed before the follow-up was created",
                true,
            ));
        }
    }
    sqlx::query(
        "INSERT INTO chat_work_assignments
            (id, reply_thread_id, teammate_id, triggering_message_item_id,
             previous_assignment_id, state, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 'queued', ?, ?)",
    )
    .bind(assignment_id.as_str())
    .bind(reply_thread_id.as_str())
    .bind(invocation.teammate_id.as_str())
    .bind(message_item_id.as_str())
    .bind(previous_id.map(ChatWorkAssignmentId::as_str))
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_work_assignment_inputs
            (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
         VALUES (?, ?, ?, 1, ?, ?)",
    )
    .bind(new_id("assignment-input"))
    .bind(assignment_id.as_str())
    .bind(message_item_id.as_str())
    .bind(if previous_id.is_some() {
        "follow_up"
    } else {
        "trigger"
    })
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    freeze_context_package(
        transaction,
        &assignment_id,
        reply_thread_id,
        message_item_id,
        &invocation.teammate_id,
        execution_target,
        now,
    )
    .await?;
    sqlx::query(
        "INSERT INTO chat_assignment_dispatch_jobs
            (id, assignment_id, state, available_at, created_at, updated_at)
         VALUES (?, ?, 'queued', ?, ?, ?)",
    )
    .bind(new_id("dispatch"))
    .bind(assignment_id.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(AssignmentWrite {
        assignment_id: Some(assignment_id),
        input_queued: false,
    })
}

#[cfg(mobile)]
pub(super) async fn persist_assignment_routing(
    _transaction: &mut Transaction<'_, Sqlite>,
    _reply_thread_id: &ChatReplyThreadId,
    _message_item_id: &ChatConversationItemId,
    invocation: Option<&ResolvedInvocation>,
    _execution_target: Option<&ChatExecutionTarget>,
    _now: &UtcTimestamp,
) -> ChatResult<AssignmentWrite> {
    if invocation.is_some() {
        return Err(ChatError::new(
            ChatErrorCode::DriverUnavailable,
            "Local coding-agent execution is unavailable on this device",
            true,
        ));
    }
    Ok(AssignmentWrite {
        assignment_id: None,
        input_queued: false,
    })
}

pub(super) struct CommunicationMessageWrite<'a> {
    pub(super) item_id: &'a ChatConversationItemId,
    pub(super) revision_id: &'a ChatMessageRevisionId,
    pub(super) conversation_id: &'a ChatConversationId,
    pub(super) reply_thread_id: Option<&'a ChatReplyThreadId>,
    pub(super) ordinal: i64,
    pub(super) request: &'a PostChatMessageCommand,
    pub(super) invoked_teammate_id: Option<&'a ChatParticipantId>,
    pub(super) now: &'a UtcTimestamp,
}

pub(super) async fn insert_communication_message(
    transaction: &mut Transaction<'_, Sqlite>,
    write: CommunicationMessageWrite<'_>,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, ordinal, created_at)
         VALUES (?, ?, ?, 'message', ?, ?)",
    )
    .bind(write.item_id.as_str())
    .bind(write.conversation_id.as_str())
    .bind(write.reply_thread_id.map(ChatReplyThreadId::as_str))
    .bind(write.ordinal)
    .bind(write.now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_communication_messages
            (item_id, author_participant_id, author_label_snapshot, created_at)
         SELECT ?, participant.id, participant.display_name, ?
         FROM chat_participants participant WHERE participant.id = ?",
    )
    .bind(write.item_id.as_str())
    .bind(write.now.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, revision, normalized_markdown,
             rich_content_schema_version, rich_content_data, created_at)
         VALUES (?, ?, 1, ?, ?, ?, ?)",
    )
    .bind(write.revision_id.as_str())
    .bind(write.item_id.as_str())
    .bind(write.request.normalized_markdown.trim())
    .bind(i64::from(write.request.rich_content.schema_version))
    .bind(json_object(&write.request.rich_content, "richContent")?)
    .bind(write.now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for (index, reference) in write.request.references.iter().enumerate() {
        insert_message_reference(
            transaction,
            write.revision_id,
            write.conversation_id,
            reference,
            write.invoked_teammate_id,
            index,
            write.now,
        )
        .await?;
    }
    for (index, attachment_id) in write.request.attachment_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO chat_communication_attachment_references
                (message_revision_id, attachment_id, ordinal, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(write.revision_id.as_str())
        .bind(attachment_id.as_str())
        .bind(i64::try_from(index).unwrap_or(i64::MAX))
        .bind(write.now.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    sqlx::query("UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?")
        .bind(write.revision_id.as_str())
        .bind(write.item_id.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

async fn insert_message_reference(
    transaction: &mut Transaction<'_, Sqlite>,
    revision_id: &ChatMessageRevisionId,
    destination_conversation_id: &ChatConversationId,
    reference: &ChatMessageReference,
    invoked_teammate_id: Option<&ChatParticipantId>,
    ordinal: usize,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let metadata = reference.metadata();
    let reference_kind = match reference {
        ChatMessageReference::Participant { .. } => "participant",
        ChatMessageReference::Channel { .. } => "channel",
        ChatMessageReference::WorkingFolder { .. } => "working_folder",
        ChatMessageReference::WorkspacePath { .. } => "workspace_path",
        ChatMessageReference::ExecutionEnvironment { .. } => "execution_environment",
    };
    sqlx::query(
        "INSERT INTO chat_message_references
            (id, message_revision_id, reference_kind, label_snapshot,
             plain_text_projection, start_offset, end_offset, ordinal, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(metadata.reference_id.as_str())
    .bind(revision_id.as_str())
    .bind(reference_kind)
    .bind(&metadata.label_snapshot)
    .bind(&metadata.plain_text_projection)
    .bind(i64_value(metadata.start_offset)?)
    .bind(i64_value(metadata.end_offset)?)
    .bind(i64::try_from(ordinal).unwrap_or(i64::MAX))
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;

    match reference {
        ChatMessageReference::Participant {
            participant_id,
            participant_kind,
            ..
        } => {
            let stored_kind = sqlx::query_scalar::<_, String>(
                "SELECT participant_kind FROM chat_participants WHERE id = ?",
            )
            .bind(participant_id.as_str())
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?
            .ok_or_else(|| {
                ChatError::validation("references", "Referenced participant was not found")
            })?;
            if stored_kind != wire_participant_kind(*participant_kind) {
                return Err(ChatError::validation(
                    "references",
                    "Referenced participant identity is stale",
                ));
            }
            sqlx::query(
                "INSERT INTO chat_participant_reference_targets (reference_id, participant_id)
                 VALUES (?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(participant_id.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
        }
        ChatMessageReference::Channel { channel_id, .. } => {
            let source = sqlx::query(
                "SELECT channel.conversation_id, channel.archived_at
                 FROM chat_channels channel WHERE channel.id = ?",
            )
            .bind(channel_id.as_str())
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?
            .ok_or_else(|| {
                ChatError::validation("references", "Referenced channel was not found")
            })?;
            if source
                .try_get::<Option<String>, _>("archived_at")
                .map_err(persistence_error)?
                .is_some()
            {
                return Err(ChatError::validation(
                    "references",
                    "Archived channels cannot be referenced",
                ));
            }
            let source_conversation_id: String = source
                .try_get("conversation_id")
                .map_err(persistence_error)?;
            let source_high_ordinal = sqlx::query_scalar::<_, Option<i64>>(
                "SELECT max(item.ordinal)
                 FROM chat_conversation_items item
                 JOIN chat_communication_messages message ON message.item_id = item.id
                 WHERE item.conversation_id = ? AND item.reply_thread_id IS NULL
                   AND message.deleted_at IS NULL",
            )
            .bind(&source_conversation_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?
            .ok_or_else(|| {
                ChatError::validation(
                    "references",
                    "An empty channel cannot be used as an assignment context source",
                )
            })?;
            let audience_gap: i64 = sqlx::query_scalar(
                "SELECT count(*)
                 FROM chat_conversation_memberships destination
                 JOIN chat_participants participant ON participant.id = destination.participant_id
                 LEFT JOIN chat_teammate_channel_memberships destination_ai
                   ON destination_ai.conversation_id = destination.conversation_id
                  AND destination_ai.teammate_id = destination.participant_id
                 LEFT JOIN chat_access_profiles destination_profile
                   ON destination_profile.id = destination_ai.access_profile_id
                 LEFT JOIN chat_access_profile_revisions destination_revision
                   ON destination_revision.access_profile_id = destination_profile.id
                  AND destination_revision.revision = destination_profile.latest_revision
                 WHERE destination.conversation_id = ?
                   AND destination.removed_at IS NULL
                   AND (
                     participant.participant_kind != 'ai_teammate'
                     OR (
                       CASE WHEN destination_ai.read_history_inherits_profile = 1
                         THEN destination_revision.default_read_history
                         ELSE min(destination_ai.read_history,
                                  destination_revision.default_read_history)
                       END = 1
                     )
                   )
                   AND NOT EXISTS (
                     SELECT 1
                     FROM chat_conversation_memberships source_membership
                     LEFT JOIN chat_teammate_channel_memberships source_ai
                       ON source_ai.conversation_id = source_membership.conversation_id
                      AND source_ai.teammate_id = source_membership.participant_id
                     LEFT JOIN chat_access_profiles source_profile
                       ON source_profile.id = source_ai.access_profile_id
                     LEFT JOIN chat_access_profile_revisions source_revision
                       ON source_revision.access_profile_id = source_profile.id
                      AND source_revision.revision = source_profile.latest_revision
                     WHERE source_membership.conversation_id = ?
                       AND source_membership.participant_id = destination.participant_id
                       AND source_membership.removed_at IS NULL
                       AND (
                         participant.participant_kind != 'ai_teammate'
                         OR (
                           CASE WHEN source_ai.read_history_inherits_profile = 1
                             THEN source_revision.default_read_history
                             ELSE min(source_ai.read_history,
                                      source_revision.default_read_history)
                           END = 1
                         )
                       )
                   )",
            )
            .bind(destination_conversation_id.as_str())
            .bind(&source_conversation_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            if audience_gap > 0 {
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "The destination audience cannot read the referenced channel",
                    true,
                ));
            }
            let mut source_lower_ordinal: i64 = sqlx::query_scalar(
                "SELECT coalesce(max(
                   CASE
                     WHEN participant.participant_kind = 'ai_teammate'
                       AND CASE WHEN source_ai.history_boundary_inherits_profile = 1
                         THEN source_revision.default_history_boundary
                         WHEN source_revision.default_history_boundary = 'from_grant'
                           OR source_ai.history_boundary = 'from_grant'
                         THEN 'from_grant' ELSE 'entire' END = 'from_grant'
                     THEN coalesce(source_ai.history_from_ordinal, ? + 1)
                     ELSE 1
                   END
                 ), 1)
                 FROM chat_conversation_memberships destination
                 JOIN chat_participants participant ON participant.id = destination.participant_id
                 LEFT JOIN chat_teammate_channel_memberships destination_ai
                   ON destination_ai.conversation_id = destination.conversation_id
                  AND destination_ai.teammate_id = destination.participant_id
                 LEFT JOIN chat_access_profiles destination_profile
                   ON destination_profile.id = destination_ai.access_profile_id
                 LEFT JOIN chat_access_profile_revisions destination_revision
                   ON destination_revision.access_profile_id = destination_profile.id
                  AND destination_revision.revision = destination_profile.latest_revision
                 JOIN chat_conversation_memberships source_membership
                   ON source_membership.participant_id = destination.participant_id
                  AND source_membership.conversation_id = ?
                  AND source_membership.removed_at IS NULL
                 LEFT JOIN chat_teammate_channel_memberships source_ai
                   ON source_ai.conversation_id = source_membership.conversation_id
                  AND source_ai.teammate_id = source_membership.participant_id
                 LEFT JOIN chat_access_profiles source_profile
                   ON source_profile.id = source_ai.access_profile_id
                 LEFT JOIN chat_access_profile_revisions source_revision
                   ON source_revision.access_profile_id = source_profile.id
                  AND source_revision.revision = source_profile.latest_revision
                 WHERE destination.conversation_id = ?
                   AND destination.removed_at IS NULL
                   AND (
                     participant.participant_kind != 'ai_teammate'
                     OR (
                       CASE WHEN destination_ai.read_history_inherits_profile = 1
                         THEN destination_revision.default_read_history
                         ELSE min(destination_ai.read_history,
                                  destination_revision.default_read_history)
                       END = 1
                     )
                   )",
            )
            .bind(source_high_ordinal)
            .bind(&source_conversation_id)
            .bind(destination_conversation_id.as_str())
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            if let Some(teammate_id) = invoked_teammate_id {
                let teammate_lower_ordinal = sqlx::query_scalar::<_, i64>(
                    "SELECT CASE
                              WHEN CASE
                                WHEN ai_membership.history_boundary_inherits_profile = 1
                                THEN profile_revision.default_history_boundary
                                WHEN ai_membership.history_boundary = 'from_grant'
                                  OR profile_revision.default_history_boundary = 'from_grant'
                                THEN 'from_grant' ELSE 'entire' END = 'from_grant'
                              THEN coalesce(ai_membership.history_from_ordinal, ? + 1)
                              ELSE 1
                            END
                        FROM chat_conversation_memberships membership
                        JOIN chat_teammate_channel_memberships ai_membership
                          ON ai_membership.conversation_id = membership.conversation_id
                         AND ai_membership.teammate_id = membership.participant_id
                        JOIN chat_access_profiles profile
                          ON profile.id = ai_membership.access_profile_id
                        JOIN chat_access_profile_revisions profile_revision
                          ON profile_revision.access_profile_id = profile.id
                         AND profile_revision.revision = profile.latest_revision
                        WHERE membership.conversation_id = ?
                          AND membership.participant_id = ?
                          AND membership.removed_at IS NULL
                          AND CASE WHEN ai_membership.read_history_inherits_profile = 1
                            THEN profile_revision.default_read_history
                            ELSE min(ai_membership.read_history,
                                     profile_revision.default_read_history)
                          END = 1",
                )
                .bind(source_high_ordinal)
                .bind(&source_conversation_id)
                .bind(teammate_id.as_str())
                .fetch_optional(&mut **transaction)
                .await
                .map_err(persistence_error)?;
                let Some(teammate_lower_ordinal) = teammate_lower_ordinal else {
                    return Err(ChatError::new(
                        ChatErrorCode::Conflict,
                        "The assigned teammate cannot read the referenced channel history",
                        true,
                    ));
                };
                source_lower_ordinal = source_lower_ordinal.max(teammate_lower_ordinal);
            }
            if source_lower_ordinal > source_high_ordinal {
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "No referenced channel history is readable by the complete destination audience",
                    true,
                ));
            }
            let source_revision_cutoff_id = sqlx::query_scalar::<_, String>(
                "SELECT frozen.id
                 FROM (
                   SELECT revision.id, revision_ordinal.ordinal
                   FROM chat_conversation_items item
                   JOIN chat_communication_messages message ON message.item_id = item.id
                   JOIN chat_communication_message_revisions revision
                     ON revision.id = message.current_revision_id
                   JOIN chat_communication_message_revision_ordinals revision_ordinal
                     ON revision_ordinal.message_revision_id = revision.id
                   WHERE item.conversation_id = ? AND item.reply_thread_id IS NULL
                     AND message.deleted_at IS NULL
                   UNION ALL
                   SELECT revision.id, revision_ordinal.ordinal
                   FROM chat_conversation_items item
                   JOIN chat_communication_messages message ON message.item_id = item.id
                   JOIN chat_communication_message_revisions revision
                     ON revision.id = message.current_revision_id
                   JOIN chat_communication_message_revision_ordinals revision_ordinal
                     ON revision_ordinal.message_revision_id = revision.id
                   WHERE item.conversation_id = ? AND item.reply_thread_id IS NOT NULL
                     AND message.deleted_at IS NULL
                 ) frozen
                 ORDER BY frozen.ordinal DESC
                 LIMIT 1",
            )
            .bind(&source_conversation_id)
            .bind(&source_conversation_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?
            .unwrap_or_else(|| revision_id.as_str().to_string());
            let destination_audience_revision: i64 = sqlx::query_scalar(
                "SELECT revision FROM chat_conversation_audience_state
                 WHERE conversation_id = ?",
            )
            .bind(destination_conversation_id.as_str())
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            sqlx::query(
                "INSERT INTO chat_channel_reference_targets
                    (reference_id, channel_id, source_conversation_id,
                     destination_conversation_id, source_lower_ordinal,
                     source_high_ordinal, source_revision_cutoff_id,
                     destination_audience_revision)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(channel_id.as_str())
            .bind(&source_conversation_id)
            .bind(destination_conversation_id.as_str())
            .bind(source_lower_ordinal)
            .bind(source_high_ordinal)
            .bind(&source_revision_cutoff_id)
            .bind(destination_audience_revision)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
        }
        ChatMessageReference::WorkingFolder {
            working_folder_id, ..
        } => {
            require_destination_project_folder(
                transaction,
                destination_conversation_id,
                working_folder_id,
            )
            .await?;
            sqlx::query(
                "INSERT INTO chat_working_folder_reference_targets
                    (reference_id, working_folder_id) VALUES (?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(working_folder_id.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
        }
        ChatMessageReference::WorkspacePath {
            working_folder_id,
            path_kind,
            relative_path,
            ..
        } => {
            require_destination_project_folder(
                transaction,
                destination_conversation_id,
                working_folder_id,
            )
            .await?;
            let path_kind = match path_kind {
                ChatWorkspacePathKind::File => "file",
                ChatWorkspacePathKind::Folder => "folder",
            };
            sqlx::query(
                "INSERT INTO chat_workspace_path_reference_targets
                    (reference_id, working_folder_id, path_kind, relative_path)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(working_folder_id.as_str())
            .bind(path_kind)
            .bind(relative_path)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
        }
        ChatMessageReference::ExecutionEnvironment {
            execution_environment_id,
            ..
        } => {
            let valid: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1
                    FROM chat_execution_environments environment
                    JOIN project_working_folders folder
                      ON folder.id = environment.working_folder_id
                    JOIN chat_conversations conversation
                      ON conversation.project_id = folder.project_id
                    WHERE environment.id = ? AND conversation.id = ?
                      AND environment.kind IN ('current_folder', 'worktree')
                      AND environment.archived_at IS NULL
                      AND environment.lifecycle_state = 'available'
                 )",
            )
            .bind(execution_environment_id.as_str())
            .bind(destination_conversation_id.as_str())
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            if valid == 0 {
                return Err(ChatError::validation(
                    "references",
                    "Execution environment does not belong to the channel project",
                ));
            }
            sqlx::query(
                "INSERT INTO chat_execution_environment_reference_targets
                    (reference_id, execution_environment_id) VALUES (?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(execution_environment_id.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
        }
    }
    Ok(())
}

async fn require_destination_project_folder(
    transaction: &mut Transaction<'_, Sqlite>,
    destination_conversation_id: &ChatConversationId,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<()> {
    let valid: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM project_working_folders folder
            JOIN chat_conversations conversation ON conversation.project_id = folder.project_id
            WHERE folder.id = ? AND conversation.id = ? AND folder.archived_at IS NULL
         )",
    )
    .bind(working_folder_id.as_str())
    .bind(destination_conversation_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if valid == 0 {
        return Err(ChatError::validation(
            "references",
            "Working folder does not belong to the channel project",
        ));
    }
    Ok(())
}

pub(super) async fn insert_channel_copy(
    transaction: &mut Transaction<'_, Sqlite>,
    conversation_id: &ChatConversationId,
    request: &PostChatMessageCommand,
    invoked_teammate_id: Option<&ChatParticipantId>,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let item_id = conversation_item_id()?;
    let revision_id = message_revision_id()?;
    let ordinal = next_item_ordinal(transaction, conversation_id, None).await?;
    let mut copied_request = request.clone();
    for reference in &mut copied_request.references {
        reference.metadata_mut().reference_id =
            ChatMessageReferenceId::new(new_id("message-reference")).map_err(identifier_error)?;
    }
    insert_communication_message(
        transaction,
        CommunicationMessageWrite {
            item_id: &item_id,
            revision_id: &revision_id,
            conversation_id,
            reply_thread_id: None,
            ordinal,
            request: &copied_request,
            invoked_teammate_id,
            now,
        },
    )
    .await?;
    if has_thread_eligible_mention(request) {
        let thread_id = reply_thread_id()?;
        sqlx::query(
            "INSERT INTO chat_reply_threads
                (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(thread_id.as_str())
        .bind(conversation_id.as_str())
        .bind(item_id.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

pub(super) async fn next_item_ordinal(
    transaction: &mut Transaction<'_, Sqlite>,
    conversation_id: &ChatConversationId,
    reply_thread_id: Option<&ChatReplyThreadId>,
) -> ChatResult<i64> {
    sqlx::query_scalar(
        "SELECT coalesce(max(ordinal), 0) + 1
         FROM chat_conversation_items
         WHERE conversation_id = ?
           AND ((? IS NULL AND reply_thread_id IS NULL) OR reply_thread_id = ?)",
    )
    .bind(conversation_id.as_str())
    .bind(reply_thread_id.map(ChatReplyThreadId::as_str))
    .bind(reply_thread_id.map(ChatReplyThreadId::as_str))
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)
}

pub(super) async fn require_reply_thread(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
    conversation_id: &ChatConversationId,
) -> ChatResult<ChatReplyThreadId> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM chat_reply_threads WHERE id = ? AND conversation_id = ?
         )",
    )
    .bind(reply_thread_id.as_str())
    .bind(conversation_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if exists == 0 {
        return Err(ChatError::validation(
            "replyThreadId",
            "Reply thread does not belong to the selected channel",
        ));
    }
    Ok(reply_thread_id.clone())
}

pub(super) async fn require_participating_teammate(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
    teammate_id: &ChatParticipantId,
) -> ChatResult<()> {
    let row = sqlx::query(
        "SELECT membership.removed_at, participant.archived_at,
                teammate.latest_policy_revision, ai_membership.participate,
                ai_membership.participate_inherits_profile,
                profile_revision.default_participate
         FROM chat_conversation_memberships membership
         JOIN chat_participants participant ON participant.id = membership.participant_id
         JOIN chat_teammates teammate ON teammate.participant_id = membership.participant_id
         JOIN chat_teammate_channel_memberships ai_membership
           ON ai_membership.conversation_id = membership.conversation_id
          AND ai_membership.teammate_id = membership.participant_id
         JOIN chat_access_profiles profile ON profile.id = ai_membership.access_profile_id
         JOIN chat_access_profile_revisions profile_revision
           ON profile_revision.access_profile_id = profile.id
          AND profile_revision.revision = profile.latest_revision
         WHERE membership.conversation_id = ? AND membership.participant_id = ?",
    )
    .bind(conversation_id.as_str())
    .bind(teammate_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::validation(
            "references",
            "The AI teammate is not a member of this channel",
        )
    })?;
    let available = row
        .try_get::<Option<String>, _>("removed_at")
        .map_err(persistence_error)?
        .is_none()
        && row
            .try_get::<Option<String>, _>("archived_at")
            .map_err(persistence_error)?
            .is_none()
        && row
            .try_get::<i64, _>("latest_policy_revision")
            .map_err(persistence_error)?
            > 0
        && if row
            .try_get::<i64, _>("participate_inherits_profile")
            .map_err(persistence_error)?
            != 0
        {
            row.try_get::<i64, _>("default_participate")
                .map_err(persistence_error)?
                != 0
        } else {
            row.try_get::<i64, _>("participate")
                .map_err(persistence_error)?
                != 0
                && row
                    .try_get::<i64, _>("default_participate")
                    .map_err(persistence_error)?
                    != 0
        };
    if !available {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "The AI teammate needs setup before it can be assigned",
            true,
        ));
    }
    Ok(())
}

#[cfg(desktop)]
pub(super) async fn insert_policy_revision(
    transaction: &mut Transaction<'_, Sqlite>,
    teammate_id: &ChatParticipantId,
    revision: u64,
    policy: &ChatTeammatePolicyInput,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let selection = serde_json::to_string(&StoredModelSelection {
        provider_managed_model: policy.provider_managed_model,
        model_id: policy.model_id.clone(),
        model_options: policy.model_options.clone(),
    })
    .map_err(serialization_error)?;
    sqlx::query(
        "INSERT INTO chat_teammate_policy_revisions
            (id, teammate_id, revision, provider_instance_id,
             safety_mode,
             model_selection_schema_version, model_selection_data, effort, speed,
             provider_options_schema_version, provider_options_data,
             interaction_mode, created_at)
         VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, 'build', ?)",
    )
    .bind(new_id("teammate-policy"))
    .bind(teammate_id.as_str())
    .bind(i64_value(revision)?)
    .bind(policy.provider_instance_id.as_str())
    .bind(wire_approval_policy(policy.safety_mode))
    .bind(selection)
    .bind(policy.effort.as_deref())
    .bind(policy.speed.as_deref())
    .bind(i64::from(policy.provider_options.schema_version))
    .bind(json_object(&policy.provider_options, "providerOptions")?)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

pub(super) async fn read_message_references(
    pool: &SqlitePool,
    revision_id: &ChatMessageRevisionId,
) -> ChatResult<Vec<ChatMessageReference>> {
    let rows = sqlx::query(
        "SELECT id, reference_kind, label_snapshot, plain_text_projection,
                start_offset, end_offset
         FROM chat_message_references WHERE message_revision_id = ?
         ORDER BY ordinal",
    )
    .bind(revision_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut references = Vec::with_capacity(rows.len());
    for row in rows {
        let reference_id =
            ChatMessageReferenceId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
                .map_err(identifier_error)?;
        let metadata = ChatReferenceMetadata {
            reference_id: reference_id.clone(),
            label_snapshot: row.try_get("label_snapshot").map_err(persistence_error)?,
            start_offset: u64_value(row.try_get("start_offset").map_err(persistence_error)?)?,
            end_offset: u64_value(row.try_get("end_offset").map_err(persistence_error)?)?,
            plain_text_projection: row
                .try_get("plain_text_projection")
                .map_err(persistence_error)?,
        };
        let reference_kind: String = row.try_get("reference_kind").map_err(persistence_error)?;
        let reference = match reference_kind.as_str() {
            "participant" => {
                let target = sqlx::query(
                    "SELECT target.participant_id, participant.participant_kind
                     FROM chat_participant_reference_targets target
                     JOIN chat_participants participant ON participant.id = target.participant_id
                     WHERE target.reference_id = ?",
                )
                .bind(reference_id.as_str())
                .fetch_one(pool)
                .await
                .map_err(persistence_error)?;
                ChatMessageReference::Participant {
                    metadata,
                    participant_id: ChatParticipantId::new(
                        target
                            .try_get::<String, _>("participant_id")
                            .map_err(persistence_error)?,
                    )
                    .map_err(identifier_error)?,
                    participant_kind: parse_participant_kind(
                        &target
                            .try_get::<String, _>("participant_kind")
                            .map_err(persistence_error)?,
                    )?,
                }
            }
            "channel" => {
                let channel_id: String = sqlx::query_scalar(
                    "SELECT channel_id FROM chat_channel_reference_targets
                     WHERE reference_id = ?",
                )
                .bind(reference_id.as_str())
                .fetch_one(pool)
                .await
                .map_err(persistence_error)?;
                ChatMessageReference::Channel {
                    metadata,
                    channel_id: ChatChannelId::new(channel_id).map_err(identifier_error)?,
                }
            }
            "working_folder" => {
                let folder_id: String = sqlx::query_scalar(
                    "SELECT working_folder_id FROM chat_working_folder_reference_targets
                     WHERE reference_id = ?",
                )
                .bind(reference_id.as_str())
                .fetch_one(pool)
                .await
                .map_err(persistence_error)?;
                ChatMessageReference::WorkingFolder {
                    metadata,
                    working_folder_id: ProjectWorkingFolderId::new(folder_id)
                        .map_err(identifier_error)?,
                }
            }
            "workspace_path" => {
                let target = sqlx::query(
                    "SELECT working_folder_id, path_kind, relative_path
                     FROM chat_workspace_path_reference_targets WHERE reference_id = ?",
                )
                .bind(reference_id.as_str())
                .fetch_one(pool)
                .await
                .map_err(persistence_error)?;
                let path_kind = match target
                    .try_get::<String, _>("path_kind")
                    .map_err(persistence_error)?
                    .as_str()
                {
                    "file" => ChatWorkspacePathKind::File,
                    "folder" => ChatWorkspacePathKind::Folder,
                    _ => {
                        return Err(ChatError::new(
                            ChatErrorCode::Persistence,
                            "Stored workspace path kind is invalid",
                            false,
                        ));
                    }
                };
                ChatMessageReference::WorkspacePath {
                    metadata,
                    working_folder_id: ProjectWorkingFolderId::new(
                        target
                            .try_get::<String, _>("working_folder_id")
                            .map_err(persistence_error)?,
                    )
                    .map_err(identifier_error)?,
                    path_kind,
                    relative_path: target.try_get("relative_path").map_err(persistence_error)?,
                }
            }
            "execution_environment" => {
                let environment_id: String = sqlx::query_scalar(
                    "SELECT execution_environment_id
                     FROM chat_execution_environment_reference_targets
                     WHERE reference_id = ?",
                )
                .bind(reference_id.as_str())
                .fetch_one(pool)
                .await
                .map_err(persistence_error)?;
                ChatMessageReference::ExecutionEnvironment {
                    metadata,
                    execution_environment_id: ChatExecutionEnvironmentId::new(environment_id)
                        .map_err(identifier_error)?,
                }
            }
            _ => {
                return Err(ChatError::new(
                    ChatErrorCode::Persistence,
                    "Stored message reference kind is invalid",
                    false,
                ));
            }
        };
        references.push(reference);
    }
    Ok(references)
}

pub(super) async fn read_post_receipt(
    pool: &SqlitePool,
    client_command_id: &ChatCommandId,
) -> ChatResult<Option<PostChatMessageResult>> {
    let row = sqlx::query(
        "SELECT state, result_data FROM chat_organizational_command_receipts
         WHERE client_command_id = ?",
    )
    .bind(client_command_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let state: String = row.try_get("state").map_err(persistence_error)?;
    if state != "completed" {
        return Err(ChatError::new(
            ChatErrorCode::Busy,
            "This message command is still being processed",
            true,
        ));
    }
    let data: String = row
        .try_get::<Option<String>, _>("result_data")
        .map_err(persistence_error)?
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Persistence,
                "Message receipt is incomplete",
                true,
            )
        })?;
    if let Ok(result) = serde_json::from_str::<PostChatMessageResult>(&data) {
        return Ok(Some(result));
    }
    let locator =
        serde_json::from_str::<StoredPostMessageReceipt>(&data).map_err(serialization_error)?;
    let StoredPostMessageReceipt::Locator {
        message_item_id,
        reply_thread_id,
        assignment_id,
        assignment_input_queued,
    } = locator;
    let message = read_message(pool, &message_item_id).await?;
    let assignment = match assignment_id.as_ref() {
        Some(assignment_id) => Some(read_assignment(pool, assignment_id).await?),
        None => match reply_thread_id.as_ref() {
            Some(reply_thread_id) => {
                read_active_or_latest_assignment(pool, reply_thread_id).await?
            }
            None => None,
        },
    };
    Ok(Some(PostChatMessageResult {
        message,
        reply_thread_id,
        assignment,
        assignment_input_queued,
    }))
}

#[cfg(desktop)]
pub(super) async fn set_assignment_state(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    expected_revision: u64,
    state: ChatWorkAssignmentState,
    reason: Option<&str>,
) -> ChatResult<ChatWorkAssignmentRead> {
    let now = now_timestamp()?;
    let updated = sqlx::query(
        "UPDATE chat_work_assignments
         SET state = ?, state_reason = ?, settled_at = ?,
             revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ?",
    )
    .bind(wire_work_state(state))
    .bind(reason)
    .bind((!state.is_active()).then(|| now.as_str()))
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .bind(i64_value(expected_revision)?)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The assignment changed before the update",
            true,
        ));
    }
    if !state.is_active() {
        sqlx::query(
            "UPDATE chat_assignment_dispatch_jobs SET state = ?, updated_at = ?
             WHERE assignment_id = ? AND state IN ('queued', 'claimed')",
        )
        .bind(if state == ChatWorkAssignmentState::Cancelled {
            "cancelled"
        } else {
            "failed"
        })
        .bind(now.as_str())
        .bind(assignment_id.as_str())
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    }
    read_assignment(pool, assignment_id).await
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredModelSelection {
    pub(super) provider_managed_model: bool,
    pub(super) model_id: Option<ModelId>,
    pub(super) model_options: Vec<ModelOptionSelection>,
}

pub(super) fn parse_model_selection(data: String) -> ChatResult<StoredModelSelection> {
    serde_json::from_str(&data).map_err(serialization_error)
}
