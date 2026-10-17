//! Durable organizational messages, reply threads, teammates, and work assignments.

use super::channels::{
    chat_pool, i64_value, identifier_error, now_timestamp, optional_timestamp, persistence_error,
    timestamp, u64_value,
};
pub use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat_contracts::models::*;
use serde::{Deserialize, Serialize};
use sqlx::Row;
#[cfg(desktop)]
use sqlx::{Sqlite, Transaction};

const LOCAL_PARTICIPANT_ID: &str = "participant:local-owner";
#[cfg(desktop)]
const MAX_CHANNEL_CONTEXT_MESSAGES: usize = 20;
#[cfg(desktop)]
const MAX_THREAD_CONTEXT_REPLIES: usize = 50;
const MAX_PAGE_SIZE: u32 = 100;
const DEFAULT_PAGE_SIZE: u32 = 50;
const MAX_SEARCH_RESULTS: u32 = 100;

#[cfg(desktop)]
pub mod access;
#[cfg(desktop)]
mod assignments;
mod common;
#[cfg(desktop)]
mod context;
#[cfg(desktop)]
mod dispatch;
mod reads;
mod scheduling;
mod teammate_lifecycle;
mod workflow;

use common::{
    conversation_item_id, has_thread_eligible_mention, map_command_receipt_error,
    message_revision_id, normalized_fts_query, parse_cursor, parse_participant_kind,
    reply_thread_id, serialization_error, validate_message_request,
};
#[cfg(desktop)]
use common::{
    json_object, map_teammate_write_error, validate_display_name, validate_policy,
    validate_profile_text, validate_teammate_role,
};
#[cfg(desktop)]
use dispatch::{deliver_assignment_input, dispatch_assignment_job};
pub(crate) use reads::read_memberships_for_conversation;
use reads::{
    read_active_or_latest_assignment, read_assignment, read_message, read_reply_thread_page,
    read_teammate,
};
#[cfg(desktop)]
use teammate_lifecycle::{delete_unused_teammate, set_teammate_archived};
#[cfg(desktop)]
use workflow::insert_policy_revision;
use workflow::{
    AssignmentWrite, CommunicationMessageWrite, insert_channel_copy, insert_communication_message,
    next_item_ordinal, persist_assignment_routing, read_post_receipt,
    require_continuation_scope_is_unchanged, require_reply_thread, resolve_invoked_teammate,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "receiptKind", rename_all = "snake_case")]
enum StoredPostMessageReceipt {
    Locator {
        message_item_id: ChatConversationItemId,
        reply_thread_id: Option<ChatReplyThreadId>,
        assignment_id: Option<ChatWorkAssignmentId>,
        assignment_input_queued: bool,
    },
}

#[tauri::command]
pub async fn chat_list_teammates(
    app: tauri::AppHandle,
    db_url: String,
    archived: bool,
) -> ChatResult<Vec<ChatAiTeammateRead>> {
    let pool = chat_pool(app, db_url).await?;
    let rows = sqlx::query(
        "SELECT participant_id
         FROM chat_teammates teammate
         JOIN chat_participants participant ON participant.id = teammate.participant_id
         WHERE (participant.archived_at IS NOT NULL) = ?
         ORDER BY participant.display_name COLLATE NOCASE, participant.id",
    )
    .bind(i64::from(archived))
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    let mut teammates = Vec::with_capacity(rows.len());
    for row in rows {
        let id = ChatParticipantId::new(
            row.try_get::<String, _>("participant_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        teammates.push(read_teammate(&pool, &id).await?);
    }
    Ok(teammates)
}

#[tauri::command]
pub async fn chat_read_teammate(
    app: tauri::AppHandle,
    db_url: String,
    teammate_id: ChatParticipantId,
) -> ChatResult<ChatAiTeammateRead> {
    read_teammate(&chat_pool(app, db_url).await?, &teammate_id).await
}

#[tauri::command]
#[cfg(desktop)]
pub async fn chat_create_teammate(
    app: tauri::AppHandle,
    db_url: String,
    request: CreateChatTeammateCommand,
) -> ChatResult<ChatAiTeammateRead> {
    let display_name = validate_display_name(&request.display_name)?;
    let role = validate_teammate_role(&request.role)?;
    validate_profile_text(&request.instructions, 65_536, "instructions")?;
    validate_policy(&app, &request.policy)?;
    let avatar_data = json_object(&request.avatar, "avatar")?;
    let pool = chat_pool(app, db_url).await?;
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_participants
            (id, participant_kind, display_name, avatar_schema_version,
             avatar_data, created_at, updated_at)
         VALUES (?, 'ai_teammate', ?, ?, ?, ?, ?)",
    )
    .bind(request.teammate_id.as_str())
    .bind(display_name)
    .bind(i64::from(request.avatar.schema_version))
    .bind(avatar_data)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(map_teammate_write_error)?;
    sqlx::query(
        "INSERT INTO chat_teammates
            (participant_id, role, instructions, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(request.teammate_id.as_str())
    .bind(role)
    .bind(request.instructions.trim())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    insert_policy_revision(
        &mut transaction,
        &request.teammate_id,
        1,
        &request.policy,
        &now,
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)?;
    read_teammate(&pool, &request.teammate_id).await
}

#[tauri::command]
#[cfg(desktop)]
pub async fn chat_archive_teammate(
    app: tauri::AppHandle,
    db_url: String,
    teammate_id: ChatParticipantId,
    expected_revision: u64,
    archived: bool,
) -> ChatResult<ChatAiTeammateRead> {
    let pool = chat_pool(app, db_url).await?;
    set_teammate_archived(&pool, &teammate_id, expected_revision, archived).await
}

#[tauri::command]
#[cfg(desktop)]
pub async fn chat_delete_unused_teammate(
    app: tauri::AppHandle,
    db_url: String,
    teammate_id: ChatParticipantId,
    expected_revision: u64,
) -> ChatResult<()> {
    let pool = chat_pool(app, db_url).await?;
    delete_unused_teammate(&pool, &teammate_id, expected_revision).await
}

#[tauri::command]
#[cfg(any(test, mobile))]
pub async fn chat_read_mobile_channel_roster(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
) -> ChatResult<ChatChannelRosterRead> {
    let pool = chat_pool(app, db_url).await?;
    let channel = super::channels::read_channel(&pool, &channel_id).await?;
    let audience_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM chat_conversation_audience_state WHERE conversation_id = ?",
    )
    .bind(channel.conversation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    Ok(ChatChannelRosterRead {
        channel_id,
        conversation_id: channel.conversation_id.clone(),
        audience_revision: u64_value(audience_revision)?,
        memberships: read_memberships_for_conversation(&pool, &channel.conversation_id, false)
            .await?,
    })
}

#[tauri::command]
#[cfg(desktop)]
pub async fn chat_read_project_primary_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    project_id: String,
) -> ChatResult<ChatProjectPrimaryWorkingFolderRead> {
    let row = sqlx::query(
        "SELECT project_id, working_folder_id, revision
         FROM project_primary_working_folders WHERE project_id = ?",
    )
    .bind(&project_id)
    .fetch_optional(&chat_pool(app, db_url).await?)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Primary working folder was not found",
            true,
        )
    })?;
    Ok(ChatProjectPrimaryWorkingFolderRead {
        project_id: row.try_get("project_id").map_err(persistence_error)?,
        working_folder_id: ProjectWorkingFolderId::new(
            row.try_get::<String, _>("working_folder_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
    })
}

#[tauri::command]
#[cfg(desktop)]
pub async fn chat_set_project_primary_working_folder(
    app: tauri::AppHandle,
    db_url: String,
    project_id: String,
    working_folder_id: ProjectWorkingFolderId,
    expected_revision: u64,
) -> ChatResult<ChatProjectPrimaryWorkingFolderRead> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let now = now_timestamp()?;
    let updated = sqlx::query(
        "UPDATE project_primary_working_folders
         SET working_folder_id = ?, revision = revision + 1, updated_at = ?
         WHERE project_id = ? AND revision = ?",
    )
    .bind(working_folder_id.as_str())
    .bind(now.as_str())
    .bind(&project_id)
    .bind(i64_value(expected_revision)?)
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The primary working folder changed before the update",
            true,
        ));
    }
    chat_read_project_primary_working_folder(app, db_url, project_id).await
}

#[tauri::command]
pub async fn chat_post_message(
    app: tauri::AppHandle,
    db_url: String,
    request: PostChatMessageCommand,
) -> ChatResult<PostChatMessageResult> {
    validate_message_request(&request)?;
    #[cfg(desktop)]
    let dispatch_app = app.clone();
    #[cfg(desktop)]
    let dispatch_db_url = db_url.clone();
    let pool = chat_pool(app, db_url).await?;
    if let Some(result) = read_post_receipt(&pool, &request.client_command_id).await? {
        return Ok(result);
    }
    let channel = super::channels::read_channel(&pool, &request.channel_id).await?;
    if channel.archived_at.is_some() {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Restore the channel before posting a message",
            true,
        ));
    }
    if let Some(reply_thread_id) = request.reply_thread_id.as_ref() {
        require_reply_thread(&pool, reply_thread_id, &channel.conversation_id).await?;
    }
    let resolved_invocation = resolve_invoked_teammate(
        &pool,
        &channel.conversation_id,
        request.reply_thread_id.as_ref(),
        &request.references,
    )
    .await?;
    require_continuation_scope_is_unchanged(
        resolved_invocation
            .as_ref()
            .and_then(|invocation| invocation.active_assignment.as_ref())
            .is_some(),
        &request.references,
    )?;
    let now = now_timestamp()?;
    let item_id = conversation_item_id()?;
    let revision_id = message_revision_id()?;
    let target_reply_thread_id = match request.reply_thread_id.as_ref() {
        Some(reply_thread_id) => Some(reply_thread_id.clone()),
        None if has_thread_eligible_mention(&request) => Some(reply_thread_id()?),
        None => None,
    };
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_organizational_command_receipts
            (client_command_id, command_kind, state, created_at, updated_at)
         VALUES (?, 'post_message', 'accepted', ?, ?)",
    )
    .bind(request.client_command_id.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(map_command_receipt_error)?;
    let ordinal = next_item_ordinal(
        &mut transaction,
        &channel.conversation_id,
        request.reply_thread_id.as_ref(),
    )
    .await?;
    insert_communication_message(
        &mut transaction,
        CommunicationMessageWrite {
            item_id: &item_id,
            revision_id: &revision_id,
            conversation_id: &channel.conversation_id,
            reply_thread_id: request.reply_thread_id.as_ref(),
            ordinal,
            request: &request,
            invoked_teammate_id: resolved_invocation
                .as_ref()
                .map(|invocation| &invocation.teammate_id),
            now: &now,
        },
    )
    .await?;
    if request.reply_thread_id.is_none() {
        if let Some(reply_thread_id) = target_reply_thread_id.as_ref() {
            sqlx::query(
                "INSERT INTO chat_reply_threads
                (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(reply_thread_id.as_str())
            .bind(channel.conversation_id.as_str())
            .bind(item_id.as_str())
            .bind(now.as_str())
            .bind(now.as_str())
            .bind(now.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        }
    } else {
        let reply_thread_id = target_reply_thread_id.as_ref().ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Persistence,
                "A reply message lost its thread identity",
                true,
            )
        })?;
        sqlx::query(
            "UPDATE chat_reply_threads
             SET reply_count = reply_count + 1, last_activity_at = ?,
                 revision = revision + 1, updated_at = ? WHERE id = ?",
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(reply_thread_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    sqlx::query(
        "UPDATE chat_conversations
         SET last_activity_at = ?, revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(channel.conversation_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let assignment_write = match target_reply_thread_id.as_ref() {
        Some(reply_thread_id) => {
            persist_assignment_routing(
                &mut transaction,
                reply_thread_id,
                &item_id,
                resolved_invocation.as_ref(),
                request.execution_target.as_ref(),
                &now,
            )
            .await?
        }
        None => AssignmentWrite {
            assignment_id: None,
            input_queued: false,
        },
    };
    if request.also_send_to_channel && request.reply_thread_id.is_some() {
        insert_channel_copy(
            &mut transaction,
            &channel.conversation_id,
            &request,
            resolved_invocation
                .as_ref()
                .map(|invocation| &invocation.teammate_id),
            &now,
        )
        .await?;
    }
    let receipt_locator = StoredPostMessageReceipt::Locator {
        message_item_id: item_id.clone(),
        reply_thread_id: target_reply_thread_id.clone(),
        assignment_id: assignment_write.assignment_id.clone(),
        assignment_input_queued: assignment_write.input_queued,
    };
    sqlx::query(
        "UPDATE chat_organizational_command_receipts
         SET state = 'completed', result_schema_version = 1, result_data = ?, updated_at = ?
         WHERE client_command_id = ? AND state = 'accepted'",
    )
    .bind(serde_json::to_string(&receipt_locator).map_err(serialization_error)?)
    .bind(now.as_str())
    .bind(request.client_command_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    let message = read_message(&pool, &item_id).await?;
    let assignment = match assignment_write.assignment_id.as_ref() {
        Some(assignment_id) => Some(read_assignment(&pool, assignment_id).await?),
        None => match target_reply_thread_id.as_ref() {
            Some(reply_thread_id) => {
                read_active_or_latest_assignment(&pool, reply_thread_id).await?
            }
            None => None,
        },
    };
    let result = PostChatMessageResult {
        message,
        reply_thread_id: target_reply_thread_id,
        assignment,
        assignment_input_queued: assignment_write.input_queued,
    };
    let serialized = serde_json::to_string(&result).map_err(serialization_error)?;
    sqlx::query(
        "UPDATE chat_organizational_command_receipts
         SET state = 'completed', result_schema_version = 1, result_data = ?, updated_at = ?
         WHERE client_command_id = ?",
    )
    .bind(serialized)
    .bind(now_timestamp()?.as_str())
    .bind(request.client_command_id.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    #[cfg(desktop)]
    if let Some(assignment_id) = assignment_write.assignment_id.clone() {
        if assignment_write.input_queued {
            let input_message_id = item_id.clone();
            tauri::async_runtime::spawn(async move {
                let _ = deliver_assignment_input(
                    dispatch_app,
                    dispatch_db_url,
                    assignment_id,
                    input_message_id,
                )
                .await;
            });
        } else {
            tauri::async_runtime::spawn(async move {
                let _ = dispatch_assignment_job(dispatch_app, dispatch_db_url, assignment_id).await;
            });
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn chat_schedule_message(
    app: tauri::AppHandle,
    db_url: String,
    request: ScheduleChatMessageCommand,
) -> ChatResult<ChatScheduledMessageRead> {
    scheduling::schedule_message(app, db_url, request).await
}

#[tauri::command]
pub async fn chat_list_scheduled_messages(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
    reply_thread_id: Option<ChatReplyThreadId>,
) -> ChatResult<Vec<ChatScheduledMessageRead>> {
    scheduling::list_scheduled_messages(app, db_url, channel_id, reply_thread_id).await
}

#[tauri::command]
pub async fn chat_cancel_scheduled_message(
    app: tauri::AppHandle,
    db_url: String,
    scheduled_message_id: ChatScheduledMessageId,
) -> ChatResult<()> {
    scheduling::cancel_scheduled_message(app, db_url, scheduled_message_id).await
}

#[tauri::command]
pub async fn chat_retry_scheduled_message(
    app: tauri::AppHandle,
    db_url: String,
    scheduled_message_id: ChatScheduledMessageId,
) -> ChatResult<ChatScheduledMessageRead> {
    scheduling::retry_scheduled_message(app, db_url, scheduled_message_id).await
}

#[tauri::command]
pub async fn chat_send_scheduled_message_now(
    app: tauri::AppHandle,
    db_url: String,
    scheduled_message_id: ChatScheduledMessageId,
) -> ChatResult<PostChatMessageResult> {
    scheduling::send_scheduled_message_now(app, db_url, scheduled_message_id).await
}

#[tauri::command]
pub async fn chat_dispatch_due_scheduled_messages(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<ChatScheduledMessageDispatchRead> {
    scheduling::dispatch_due_scheduled_messages(app, db_url).await
}

#[tauri::command]
#[cfg(desktop)]
pub async fn chat_recover_assignment_dispatch_jobs(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<u32> {
    assignments::recover_assignment_dispatch_jobs(app, db_url).await
}

#[tauri::command]
pub async fn chat_read_channel_page(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
    cursor: Option<String>,
    limit: Option<u32>,
) -> ChatResult<ChatChannelPageRead> {
    let pool = chat_pool(app, db_url).await?;
    let channel = super::channels::read_channel(&pool, &channel_id).await?;
    let before = parse_cursor(cursor.as_deref())?;
    let limit = limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
    let rows = sqlx::query(
        "SELECT id, ordinal
         FROM chat_conversation_items
         WHERE conversation_id = ? AND reply_thread_id IS NULL AND item_kind = 'message'
           AND (? IS NULL OR ordinal < ?)
         ORDER BY ordinal DESC LIMIT ?",
    )
    .bind(channel.conversation_id.as_str())
    .bind(before)
    .bind(before)
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    let previous_cursor = rows
        .last()
        .map(|row| row.try_get::<i64, _>("ordinal"))
        .transpose()
        .map_err(persistence_error)?
        .filter(|_| rows.len() == limit as usize)
        .map(|ordinal| ordinal.to_string());
    let mut messages = Vec::with_capacity(rows.len());
    for row in rows.into_iter().rev() {
        let item_id =
            ChatConversationItemId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
                .map_err(identifier_error)?;
        messages.push(read_message(&pool, &item_id).await?);
    }
    Ok(ChatChannelPageRead {
        channel_id,
        messages,
        previous_cursor,
        revision: channel.revision,
    })
}

#[tauri::command]
pub async fn chat_read_reply_thread_page(
    app: tauri::AppHandle,
    db_url: String,
    reply_thread_id: ChatReplyThreadId,
    cursor: Option<String>,
    limit: Option<u32>,
) -> ChatResult<ChatReplyThreadPageRead> {
    let pool = chat_pool(app, db_url).await?;
    read_reply_thread_page(
        &pool,
        &reply_thread_id,
        parse_cursor(cursor.as_deref())?,
        limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE),
    )
    .await
}

#[tauri::command]
pub async fn chat_search_messages(
    app: tauri::AppHandle,
    db_url: String,
    query: String,
    project_id: Option<String>,
    limit: Option<u32>,
) -> ChatResult<Vec<ChatMessageSearchResultRead>> {
    let fts_query = normalized_fts_query(&query)?;
    let pool = chat_pool(app, db_url).await?;
    let rows = sqlx::query(
        "SELECT
            channel.project_id,
            channel.id AS channel_id,
            channel.name AS channel_name,
            search.conversation_id,
            search.reply_thread_id,
            search.message_item_id,
            item.ordinal,
            participant.id AS author_participant_id,
            participant.participant_kind AS author_participant_kind,
            participant.display_name AS author_display_name,
            snippet(chat_communication_search_fts, 4, '<mark>', '</mark>', '…', 24) AS excerpt,
            item.created_at
         FROM chat_communication_search_fts search
         JOIN chat_conversation_items item ON item.id = search.message_item_id
         JOIN chat_communication_messages message ON message.item_id = item.id
         JOIN chat_participants participant ON participant.id = message.author_participant_id
         JOIN chat_channels channel ON channel.conversation_id = search.conversation_id
         WHERE chat_communication_search_fts MATCH ?
           AND (? IS NULL OR channel.project_id = ?)
         ORDER BY rank, item.created_at DESC
         LIMIT ?",
    )
    .bind(fts_query)
    .bind(project_id.as_deref())
    .bind(project_id.as_deref())
    .bind(i64::from(limit.unwrap_or(50).clamp(1, MAX_SEARCH_RESULTS)))
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(ChatMessageSearchResultRead {
                project_id: row.try_get("project_id").map_err(persistence_error)?,
                channel_id: ChatChannelId::new(
                    row.try_get::<String, _>("channel_id")
                        .map_err(persistence_error)?,
                )
                .map_err(identifier_error)?,
                channel_name: row.try_get("channel_name").map_err(persistence_error)?,
                conversation_id: ChatConversationId::new(
                    row.try_get::<String, _>("conversation_id")
                        .map_err(persistence_error)?,
                )
                .map_err(identifier_error)?,
                reply_thread_id: row
                    .try_get::<Option<String>, _>("reply_thread_id")
                    .map_err(persistence_error)?
                    .map(ChatReplyThreadId::new)
                    .transpose()
                    .map_err(identifier_error)?,
                message_item_id: ChatConversationItemId::new(
                    row.try_get::<String, _>("message_item_id")
                        .map_err(persistence_error)?,
                )
                .map_err(identifier_error)?,
                ordinal: u64_value(row.try_get("ordinal").map_err(persistence_error)?)?,
                author_participant_id: ChatParticipantId::new(
                    row.try_get::<String, _>("author_participant_id")
                        .map_err(persistence_error)?,
                )
                .map_err(identifier_error)?,
                author_kind: parse_participant_kind(
                    &row.try_get::<String, _>("author_participant_kind")
                        .map_err(persistence_error)?,
                )?,
                author_display_name: row
                    .try_get("author_display_name")
                    .map_err(persistence_error)?,
                excerpt: row.try_get("excerpt").map_err(persistence_error)?,
                created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
            })
        })
        .collect()
}

#[tauri::command]
#[cfg(desktop)]
pub async fn chat_cancel_assignment(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    expected_revision: u64,
) -> ChatResult<ChatWorkAssignmentRead> {
    assignments::cancel_assignment(app, db_url, assignment_id, expected_revision).await
}

#[tauri::command]
#[cfg(desktop)]
pub async fn chat_retry_assignment(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    expected_revision: u64,
) -> ChatResult<ChatWorkAssignmentRead> {
    assignments::retry_assignment(app, db_url, assignment_id, expected_revision).await
}

#[cfg(test)]
mod tests;
