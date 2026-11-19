//! Durable organizational messages: communication messages with their references and channel
//! copies, and the references and post receipts read back from them.

use super::super::common::{
    AttachmentSource, ScheduledPostIds, conversation_item_id, has_thread_eligible_mention,
    insert_revision_attachments, json_object, message_revision_id, new_id, parse_participant_kind,
    reply_thread_id, serialization_error, wire_participant_kind,
};
use super::super::reads::{read_active_or_latest_assignment, read_assignment, read_message};
use super::super::{
    LOCAL_PARTICIPANT_ID, StoredPostMessageReceipt, i64_value, identifier_error, persistence_error,
    u64_value,
};
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

/// Who writes a communication message.
#[derive(Clone, Copy, Debug)]
pub(in crate::chat::organization) enum MessageAuthor<'a> {
    /// The person using this device.
    LocalPerson,
    /// An agent, as when it delegates work in its channel.
    #[cfg_attr(mobile, allow(dead_code))]
    Agent(&'a ChatParticipantId),
}

impl MessageAuthor<'_> {
    fn participant_id(&self) -> &str {
        match self {
            Self::LocalPerson => LOCAL_PARTICIPANT_ID,
            Self::Agent(agent_id) => agent_id.as_str(),
        }
    }
}

pub(in crate::chat::organization) struct CommunicationMessageWrite<'a> {
    pub(in crate::chat::organization) author: MessageAuthor<'a>,
    pub(in crate::chat::organization) item_id: &'a ChatConversationItemId,
    pub(in crate::chat::organization) revision_id: &'a ChatMessageRevisionId,
    pub(in crate::chat::organization) conversation_id: &'a ChatConversationId,
    pub(in crate::chat::organization) reply_thread_id: Option<&'a ChatReplyThreadId>,
    pub(in crate::chat::organization) order_key: &'a str,
    pub(in crate::chat::organization) request: &'a PostChatMessageCommand,
    pub(in crate::chat::organization) attachments: AttachmentSource<'a>,
    pub(in crate::chat::organization) invoked_agent_id: Option<&'a ChatParticipantId>,
    pub(in crate::chat::organization) now: &'a UtcTimestamp,
}

pub(in crate::chat::organization) async fn insert_communication_message(
    transaction: &mut Transaction<'_, Sqlite>,
    write: CommunicationMessageWrite<'_>,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES (?, ?, ?, 'message', ?, ?)",
    )
    .bind(write.item_id.as_str())
    .bind(write.conversation_id.as_str())
    .bind(write.reply_thread_id.map(ChatReplyThreadId::as_str))
    .bind(write.order_key)
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
    .bind(write.author.participant_id())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown,
             rich_content_schema_version, rich_content_data, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
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
        insert_message_reference(transaction, &write, reference, index).await?;
    }
    insert_revision_attachments(transaction, write.revision_id.as_str(), write.attachments).await?;
    sqlx::query("UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?")
        .bind(write.revision_id.as_str())
        .bind(write.item_id.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

/// Writes `reference` of the message `write` posts at `ordinal`.
async fn insert_message_reference(
    transaction: &mut Transaction<'_, Sqlite>,
    write: &CommunicationMessageWrite<'_>,
    reference: &ChatMessageReference,
    ordinal: usize,
) -> ChatResult<()> {
    let revision_id = write.revision_id;
    let destination_conversation_id = write.conversation_id;
    let invoked_agent_id = write.invoked_agent_id;
    let now = write.now;
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
            let (source_high_order_key, source_high_item_id) =
                sqlx::query_as::<_, (String, String)>(
                    "SELECT item.order_key, item.id
                     FROM chat_conversation_items item
                     JOIN chat_communication_messages message ON message.item_id = item.id
                     WHERE item.conversation_id = ? AND item.reply_thread_id IS NULL
                       AND message.deleted_at IS NULL
                     ORDER BY item.order_key DESC, item.id DESC LIMIT 1",
                )
                .bind(&source_conversation_id)
                .fetch_optional(&mut **transaction)
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
                 LEFT JOIN chat_agent_channel_memberships destination_ai
                   ON destination_ai.conversation_id = destination.conversation_id
                  AND destination_ai.agent_id = destination.participant_id
                  AND destination_ai.removed_at IS NULL
                 WHERE destination.conversation_id = ?
                   AND destination.removed_at IS NULL
                   AND (
                     participant.participant_kind != 'ai_agent'
                     OR (
                       destination_ai.agent_id IS NOT NULL
                     )
                   )
                   AND NOT EXISTS (
                     SELECT 1
                     FROM chat_conversation_memberships source_membership
                     LEFT JOIN chat_agent_channel_memberships source_ai
                       ON source_ai.conversation_id = source_membership.conversation_id
                      AND source_ai.agent_id = source_membership.participant_id
                      AND source_ai.removed_at IS NULL
                     WHERE source_membership.conversation_id = ?
                       AND source_membership.participant_id = destination.participant_id
                       AND source_membership.removed_at IS NULL
                       AND (
                         participant.participant_kind != 'ai_agent'
                         OR (
                           source_ai.agent_id IS NOT NULL
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
            // A channel member reads the entire history, which starts at the first root item.
            let source_lower_order_key: String = sqlx::query_scalar(
                "SELECT min(order_key) FROM chat_conversation_items
                 WHERE conversation_id = ? AND reply_thread_id IS NULL",
            )
            .bind(&source_conversation_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            if let Some(agent_id) = invoked_agent_id {
                let agent_in_source: bool = sqlx::query_scalar(
                    "SELECT EXISTS (
                       SELECT 1
                       FROM chat_conversation_memberships membership
                       JOIN chat_agent_channel_memberships ai_membership
                         ON ai_membership.conversation_id = membership.conversation_id
                        AND ai_membership.agent_id = membership.participant_id
                        AND ai_membership.removed_at IS NULL
                       WHERE membership.conversation_id = ?
                         AND membership.participant_id = ?
                         AND membership.removed_at IS NULL
                     )",
                )
                .bind(&source_conversation_id)
                .bind(agent_id.as_str())
                .fetch_one(&mut **transaction)
                .await
                .map_err(persistence_error)?;
                if !agent_in_source {
                    return Err(ChatError::new(
                        ChatErrorCode::Conflict,
                        "The assigned agent cannot read the referenced channel history",
                        true,
                    ));
                }
            }
            if source_lower_order_key > source_high_order_key {
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "No referenced channel history is readable by the complete destination audience",
                    true,
                ));
            }
            let source_revision_cutoff_id = sqlx::query_scalar::<_, String>(
                "SELECT revision.id
                 FROM chat_conversation_items item
                 JOIN chat_communication_messages message ON message.item_id = item.id
                 JOIN chat_communication_message_revisions revision
                   ON revision.id = message.current_revision_id
                 WHERE item.conversation_id = ? AND message.deleted_at IS NULL
                 ORDER BY revision.created_at DESC, revision.id DESC
                 LIMIT 1",
            )
            .bind(&source_conversation_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?
            .unwrap_or_else(|| revision_id.as_str().to_string());
            sqlx::query(
                "INSERT INTO chat_channel_reference_targets
                    (reference_id, channel_id, source_conversation_id,
                     destination_conversation_id, source_lower_order_key,
                     source_lower_item_id, source_high_order_key, source_high_item_id,
                     source_revision_cutoff_id)
                 VALUES (?, ?, ?, ?, ?, '', ?, ?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(channel_id.as_str())
            .bind(&source_conversation_id)
            .bind(destination_conversation_id.as_str())
            .bind(&source_lower_order_key)
            .bind(&source_high_order_key)
            .bind(&source_high_item_id)
            .bind(&source_revision_cutoff_id)
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

/// Identity and attachment source of the channel copy of a reply.
pub(in crate::chat::organization) struct ChannelCopy<'a> {
    /// Derived ids for a scheduled send, or `None` for random ids.
    pub(in crate::chat::organization) ids: Option<ScheduledPostIds>,
    /// Where the copy's attachment references come from.
    pub(in crate::chat::organization) attachments: AttachmentSource<'a>,
}

pub(in crate::chat::organization) async fn insert_channel_copy(
    transaction: &mut Transaction<'_, Sqlite>,
    conversation_id: &ChatConversationId,
    request: &PostChatMessageCommand,
    copy: ChannelCopy<'_>,
    invoked_agent_id: Option<&ChatParticipantId>,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let (item_id, revision_id) = match copy.ids {
        Some(ids) => (ids.item_id, ids.revision_id),
        None => (conversation_item_id()?, message_revision_id()?),
    };
    let order_key = next_item_order_key(transaction, conversation_id, None).await?;
    let mut copied_request = request.clone();
    for reference in &mut copied_request.references {
        reference.metadata_mut().reference_id =
            ChatMessageReferenceId::new(new_id("message-reference")?).map_err(identifier_error)?;
    }
    insert_communication_message(
        transaction,
        CommunicationMessageWrite {
            author: MessageAuthor::LocalPerson,
            item_id: &item_id,
            revision_id: &revision_id,
            conversation_id,
            reply_thread_id: None,
            order_key: &order_key,
            request: &copied_request,
            attachments: copy.attachments,
            invoked_agent_id,
            now,
        },
    )
    .await?;
    if has_thread_eligible_mention(request) {
        let thread_id = reply_thread_id(&item_id)?;
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

pub(in crate::chat::organization) async fn next_item_order_key(
    transaction: &mut Transaction<'_, Sqlite>,
    conversation_id: &ChatConversationId,
    reply_thread_id: Option<&ChatReplyThreadId>,
) -> ChatResult<String> {
    ganbaru_chat::timeline::next_order_key(
        transaction,
        conversation_id.as_str(),
        reply_thread_id.map(ChatReplyThreadId::as_str),
    )
    .await
}

pub(in crate::chat::organization) async fn read_message_references(
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
                // A mention can outlive its target when a linked device deletes a named agent
                // concurrently. Only named agents are ever deleted, so a missing target is one.
                let target = sqlx::query(
                    "SELECT target.participant_id,
                            coalesce(participant.participant_kind, 'ai_agent') AS participant_kind
                     FROM chat_participant_reference_targets target
                     LEFT JOIN chat_participants participant
                       ON participant.id = target.participant_id
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

pub(in crate::chat::organization) async fn read_post_receipt(
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
        routed_as,
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
        routed_as,
    }))
}
