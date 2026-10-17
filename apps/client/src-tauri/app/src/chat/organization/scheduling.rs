//! Durable scheduled organizational message delivery.

use super::common::{new_id, serialization_error, validate_message_request, wire_participant_kind};
use super::{
    chat_pool, chat_post_message, i64_value, identifier_error, now_timestamp, persistence_error,
    require_continuation_scope_is_unchanged, require_reply_thread, resolve_invoked_teammate,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::time::SystemTime;

const MIN_SCHEDULE_LEAD_SECONDS: i64 = 30;
const MAX_SCHEDULE_DAYS: i64 = 366;
const MAX_SCHEDULE_DISPATCH_ATTEMPTS: i64 = 3;
const SCHEDULE_RETRY_SECONDS: i64 = 60;

pub(super) async fn schedule_message(
    app: tauri::AppHandle,
    db_url: String,
    request: ScheduleChatMessageCommand,
) -> ChatResult<ChatScheduledMessageRead> {
    validate_message_request(&request.message)?;
    let now = now_timestamp()?;
    validate_scheduled_for(&request.scheduled_for, &now)?;
    let pool = chat_pool(app, db_url).await?;
    let channel = super::super::channels::read_channel(&pool, &request.message.channel_id).await?;
    if channel.archived_at.is_some() {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Restore the channel before scheduling a message",
            true,
        ));
    }
    if let Some(reply_thread_id) = request.message.reply_thread_id.as_ref() {
        require_reply_thread(&pool, reply_thread_id, &channel.conversation_id).await?;
    }
    let resolved_invocation = resolve_invoked_teammate(
        &pool,
        &channel.conversation_id,
        request.message.reply_thread_id.as_ref(),
        &request.message.references,
    )
    .await?;
    require_continuation_scope_is_unchanged(
        resolved_invocation
            .as_ref()
            .and_then(|invocation| invocation.active_assignment.as_ref())
            .is_some(),
        &request.message.references,
    )?;
    validate_scheduled_attachments(&pool, &request.message.attachment_ids).await?;
    let serialized = serde_json::to_string(&request.message).map_err(serialization_error)?;
    if let Some(existing) =
        sqlx::query("SELECT request_data, scheduled_for FROM chat_scheduled_messages WHERE id = ?")
            .bind(request.scheduled_message_id.as_str())
            .fetch_optional(&pool)
            .await
            .map_err(persistence_error)?
    {
        let existing_request: String = existing
            .try_get("request_data")
            .map_err(persistence_error)?;
        let existing_schedule: String = existing
            .try_get("scheduled_for")
            .map_err(persistence_error)?;
        if existing_request == serialized && existing_schedule == request.scheduled_for.as_str() {
            return read_scheduled_message(&pool, &request.scheduled_message_id).await;
        }
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Scheduled message ID already belongs to another request",
            false,
        ));
    }
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_scheduled_messages
            (id, client_command_id, channel_id, reply_thread_id, request_data,
             scheduled_for, available_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(request.scheduled_message_id.as_str())
    .bind(request.message.client_command_id.as_str())
    .bind(request.message.channel_id.as_str())
    .bind(
        request
            .message
            .reply_thread_id
            .as_ref()
            .map(ChatReplyThreadId::as_str),
    )
    .bind(&serialized)
    .bind(request.scheduled_for.as_str())
    .bind(request.scheduled_for.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    for (index, attachment_id) in request.message.attachment_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO chat_scheduled_message_attachment_references
                (scheduled_message_id, attachment_id, ordinal, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(request.scheduled_message_id.as_str())
        .bind(attachment_id.as_str())
        .bind(i64::try_from(index).unwrap_or(i64::MAX))
        .bind(now.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    for (index, reference) in request.message.references.iter().enumerate() {
        insert_scheduled_reference(
            &mut transaction,
            &request.scheduled_message_id,
            reference,
            index,
        )
        .await?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    read_scheduled_message(&pool, &request.scheduled_message_id).await
}

async fn insert_scheduled_reference(
    transaction: &mut Transaction<'_, Sqlite>,
    scheduled_message_id: &ChatScheduledMessageId,
    reference: &ChatMessageReference,
    ordinal: usize,
) -> ChatResult<()> {
    let metadata = reference.metadata();
    let (
        reference_kind,
        participant_id,
        participant_kind,
        channel_id,
        working_folder_id,
        path_kind,
        relative_path,
        execution_environment_id,
    ) = match reference {
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
            let expected_kind = wire_participant_kind(*participant_kind);
            if stored_kind != expected_kind {
                return Err(ChatError::validation(
                    "references",
                    "Referenced participant identity is stale",
                ));
            }
            (
                "participant",
                Some(participant_id.as_str()),
                Some(expected_kind),
                None,
                None,
                None,
                None,
                None,
            )
        }
        ChatMessageReference::Channel { channel_id, .. } => (
            "channel",
            None,
            None,
            Some(channel_id.as_str()),
            None,
            None,
            None,
            None,
        ),
        ChatMessageReference::WorkingFolder {
            working_folder_id, ..
        } => (
            "working_folder",
            None,
            None,
            None,
            Some(working_folder_id.as_str()),
            None,
            None,
            None,
        ),
        ChatMessageReference::WorkspacePath {
            working_folder_id,
            path_kind,
            relative_path,
            ..
        } => (
            "workspace_path",
            None,
            None,
            None,
            Some(working_folder_id.as_str()),
            Some(match path_kind {
                ChatWorkspacePathKind::File => "file",
                ChatWorkspacePathKind::Folder => "folder",
            }),
            Some(relative_path.as_str()),
            None,
        ),
        ChatMessageReference::ExecutionEnvironment {
            execution_environment_id,
            ..
        } => (
            "execution_environment",
            None,
            None,
            None,
            None,
            None,
            None,
            Some(execution_environment_id.as_str()),
        ),
    };
    sqlx::query(
        "INSERT INTO chat_scheduled_message_references
            (id, scheduled_message_id, reference_kind, label_snapshot,
             plain_text_projection, start_offset, end_offset, participant_id,
             participant_kind, channel_id, working_folder_id, path_kind,
             relative_path, execution_environment_id, ordinal)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(metadata.reference_id.as_str())
    .bind(scheduled_message_id.as_str())
    .bind(reference_kind)
    .bind(&metadata.label_snapshot)
    .bind(&metadata.plain_text_projection)
    .bind(i64_value(metadata.start_offset)?)
    .bind(i64_value(metadata.end_offset)?)
    .bind(participant_id)
    .bind(participant_kind)
    .bind(channel_id)
    .bind(working_folder_id)
    .bind(path_kind)
    .bind(relative_path)
    .bind(execution_environment_id)
    .bind(i64::try_from(ordinal).unwrap_or(i64::MAX))
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

pub(super) async fn list_scheduled_messages(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
    reply_thread_id: Option<ChatReplyThreadId>,
) -> ChatResult<Vec<ChatScheduledMessageRead>> {
    let pool = chat_pool(app, db_url).await?;
    super::super::channels::read_channel(&pool, &channel_id).await?;
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM chat_scheduled_messages
         WHERE channel_id = ?
           AND ((? IS NULL AND reply_thread_id IS NULL) OR reply_thread_id = ?)
         ORDER BY scheduled_for, created_at, id LIMIT 100",
    )
    .bind(channel_id.as_str())
    .bind(reply_thread_id.as_ref().map(ChatReplyThreadId::as_str))
    .bind(reply_thread_id.as_ref().map(ChatReplyThreadId::as_str))
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    let mut scheduled = Vec::with_capacity(ids.len());
    for id in ids {
        scheduled.push(
            read_scheduled_message(
                &pool,
                &ChatScheduledMessageId::new(id).map_err(identifier_error)?,
            )
            .await?,
        );
    }
    Ok(scheduled)
}

pub(super) async fn cancel_scheduled_message(
    app: tauri::AppHandle,
    db_url: String,
    scheduled_message_id: ChatScheduledMessageId,
) -> ChatResult<()> {
    let pool = chat_pool(app, db_url).await?;
    let state =
        sqlx::query_scalar::<_, String>("SELECT state FROM chat_scheduled_messages WHERE id = ?")
            .bind(scheduled_message_id.as_str())
            .fetch_optional(&pool)
            .await
            .map_err(persistence_error)?
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::NotFound,
                    "Scheduled message was not found",
                    true,
                )
            })?;
    if state == "dispatching" {
        return Err(ChatError::new(
            ChatErrorCode::Busy,
            "This scheduled message is already being sent",
            true,
        ));
    }
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let attachment_ids = sqlx::query_scalar::<_, String>(
        "SELECT attachment_id FROM chat_scheduled_message_attachment_references
         WHERE scheduled_message_id = ?",
    )
    .bind(scheduled_message_id.as_str())
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let deleted = sqlx::query(
        "DELETE FROM chat_scheduled_messages
         WHERE id = ? AND state != 'dispatching'",
    )
    .bind(scheduled_message_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if deleted.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::Busy,
            "This scheduled message is already being sent",
            true,
        ));
    }
    for attachment_id in attachment_ids {
        mark_attachment_unreferenced_if_unused(&mut transaction, &attachment_id, &now).await?;
    }
    transaction.commit().await.map_err(persistence_error)
}

pub(super) async fn retry_scheduled_message(
    app: tauri::AppHandle,
    db_url: String,
    scheduled_message_id: ChatScheduledMessageId,
) -> ChatResult<ChatScheduledMessageRead> {
    let pool = chat_pool(app, db_url).await?;
    let now = now_timestamp()?;
    let updated = sqlx::query(
        "UPDATE chat_scheduled_messages
         SET state = 'scheduled', available_at = ?, attempt_count = 0, claimed_at = NULL,
             claim_token = NULL, last_error = NULL, updated_at = ?
         WHERE id = ? AND state = 'failed'",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(scheduled_message_id.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Only a failed scheduled message can be retried",
            true,
        ));
    }
    read_scheduled_message(&pool, &scheduled_message_id).await
}

pub(super) async fn send_scheduled_message_now(
    app: tauri::AppHandle,
    db_url: String,
    scheduled_message_id: ChatScheduledMessageId,
) -> ChatResult<PostChatMessageResult> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let now = now_timestamp()?;
    let claim_token = new_id("scheduled-message-immediate-claim");
    let request_data = claim_scheduled_message_for_immediate_send(
        &pool,
        &scheduled_message_id,
        &now,
        &claim_token,
    )
    .await?;
    let result = match serde_json::from_str::<PostChatMessageCommand>(&request_data) {
        Ok(request) => chat_post_message(app, db_url, request).await,
        Err(error) => Err(serialization_error(error)),
    };
    match result {
        Ok(result) => {
            sqlx::query("DELETE FROM chat_scheduled_messages WHERE id = ? AND claim_token = ?")
                .bind(scheduled_message_id.as_str())
                .bind(&claim_token)
                .execute(&pool)
                .await
                .map_err(persistence_error)?;
            Ok(result)
        }
        Err(error) => {
            let last_error = error.message.chars().take(4000).collect::<String>();
            sqlx::query(
                "UPDATE chat_scheduled_messages
                 SET state = 'failed', claimed_at = NULL, claim_token = NULL,
                     last_error = ?, updated_at = ?
                 WHERE id = ? AND claim_token = ?",
            )
            .bind(last_error)
            .bind(now_timestamp()?.as_str())
            .bind(scheduled_message_id.as_str())
            .bind(&claim_token)
            .execute(&pool)
            .await
            .map_err(persistence_error)?;
            Err(error)
        }
    }
}

pub(super) async fn dispatch_due_scheduled_messages(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<ChatScheduledMessageDispatchRead> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let now = now_timestamp()?;
    sqlx::query(
        "UPDATE chat_scheduled_messages
         SET state = 'scheduled', claimed_at = NULL, claim_token = NULL,
             available_at = ?, updated_at = ?
         WHERE state = 'dispatching'
           AND claimed_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-5 minutes')",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM chat_scheduled_messages
         WHERE state = 'scheduled' AND available_at <= ?
         ORDER BY available_at, scheduled_for, id LIMIT 32",
    )
    .bind(now.as_str())
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    let mut processed_count = 0_u32;
    let mut dispatched_count = 0_u32;
    let mut dispatched_channel_ids = Vec::new();
    for id in ids {
        let scheduled_message_id = ChatScheduledMessageId::new(id).map_err(identifier_error)?;
        let claim_token = new_id("scheduled-message-claim");
        let claimed = sqlx::query(
            "UPDATE chat_scheduled_messages
             SET state = 'dispatching', attempt_count = attempt_count + 1,
                 claimed_at = ?, claim_token = ?, updated_at = ?
             WHERE id = ? AND state = 'scheduled' AND available_at <= ?",
        )
        .bind(now.as_str())
        .bind(&claim_token)
        .bind(now.as_str())
        .bind(scheduled_message_id.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .map_err(persistence_error)?;
        if claimed.rows_affected() != 1 {
            continue;
        }
        processed_count = processed_count.saturating_add(1);
        let row = sqlx::query(
            "SELECT request_data, attempt_count FROM chat_scheduled_messages
             WHERE id = ? AND claim_token = ?",
        )
        .bind(scheduled_message_id.as_str())
        .bind(&claim_token)
        .fetch_one(&pool)
        .await
        .map_err(persistence_error)?;
        let request_data: String = row.try_get("request_data").map_err(persistence_error)?;
        let attempt_count: i64 = row.try_get("attempt_count").map_err(persistence_error)?;
        let message = serde_json::from_str::<PostChatMessageCommand>(&request_data)
            .map_err(serialization_error);
        let result = match message {
            Ok(message) => {
                let channel_id = message.channel_id.clone();
                chat_post_message(app.clone(), db_url.clone(), message)
                    .await
                    .map(|_| channel_id)
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(channel_id) => {
                sqlx::query("DELETE FROM chat_scheduled_messages WHERE id = ? AND claim_token = ?")
                    .bind(scheduled_message_id.as_str())
                    .bind(&claim_token)
                    .execute(&pool)
                    .await
                    .map_err(persistence_error)?;
                dispatched_count = dispatched_count.saturating_add(1);
                if !dispatched_channel_ids.contains(&channel_id) {
                    dispatched_channel_ids.push(channel_id);
                }
            }
            Err(error) => {
                let state = if attempt_count >= MAX_SCHEDULE_DISPATCH_ATTEMPTS {
                    "failed"
                } else {
                    "scheduled"
                };
                let retry_at = (DateTime::<Utc>::from(SystemTime::now())
                    + Duration::seconds(SCHEDULE_RETRY_SECONDS))
                .to_rfc3339_opts(SecondsFormat::Millis, true);
                let last_error = error.message.chars().take(4000).collect::<String>();
                sqlx::query(
                    "UPDATE chat_scheduled_messages
                     SET state = ?, available_at = ?, claimed_at = NULL, claim_token = NULL,
                         last_error = ?, updated_at = ?
                     WHERE id = ? AND claim_token = ?",
                )
                .bind(state)
                .bind(&retry_at)
                .bind(last_error)
                .bind(now.as_str())
                .bind(scheduled_message_id.as_str())
                .bind(&claim_token)
                .execute(&pool)
                .await
                .map_err(persistence_error)?;
            }
        }
    }
    let next_dispatch_at = sqlx::query_scalar::<_, Option<String>>(
        "SELECT MIN(available_at) FROM chat_scheduled_messages WHERE state = 'scheduled'",
    )
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?
    .map(UtcTimestamp::new)
    .transpose()
    .map_err(identifier_error)?;
    Ok(ChatScheduledMessageDispatchRead {
        processed_count,
        dispatched_count,
        dispatched_channel_ids,
        next_dispatch_at,
    })
}

pub(super) fn validate_scheduled_for(
    scheduled_for: &UtcTimestamp,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let scheduled = DateTime::parse_from_rfc3339(scheduled_for.as_str())
        .map_err(|_| ChatError::validation("scheduledFor", "Scheduled delivery time is invalid"))?;
    let current = DateTime::parse_from_rfc3339(now.as_str()).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Current time could not be resolved",
            true,
        )
    })?;
    if scheduled < current + Duration::seconds(MIN_SCHEDULE_LEAD_SECONDS) {
        return Err(ChatError::validation(
            "scheduledFor",
            "Choose a delivery time at least 30 seconds from now",
        ));
    }
    if scheduled > current + Duration::days(MAX_SCHEDULE_DAYS) {
        return Err(ChatError::validation(
            "scheduledFor",
            "Scheduled delivery must be within one year",
        ));
    }
    Ok(())
}

async fn validate_scheduled_attachments(
    pool: &SqlitePool,
    attachment_ids: &[ChatAttachmentId],
) -> ChatResult<()> {
    for attachment_id in attachment_ids {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM chat_attachments
                WHERE id = ? AND deletion_state = 'active'
             )",
        )
        .bind(attachment_id.as_str())
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
        if !exists {
            return Err(ChatError::validation(
                "attachmentIds",
                "A scheduled attachment is no longer available",
            ));
        }
    }
    Ok(())
}

pub(super) async fn claim_scheduled_message_for_immediate_send(
    pool: &SqlitePool,
    scheduled_message_id: &ChatScheduledMessageId,
    now: &UtcTimestamp,
    claim_token: &str,
) -> ChatResult<String> {
    let claimed = sqlx::query(
        "UPDATE chat_scheduled_messages
         SET state = 'dispatching', attempt_count = attempt_count + 1,
             claimed_at = ?, claim_token = ?, last_error = NULL, updated_at = ?
         WHERE id = ? AND state IN ('scheduled', 'failed')",
    )
    .bind(now.as_str())
    .bind(claim_token)
    .bind(now.as_str())
    .bind(scheduled_message_id.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if claimed.rows_affected() != 1 {
        let state = sqlx::query_scalar::<_, String>(
            "SELECT state FROM chat_scheduled_messages WHERE id = ?",
        )
        .bind(scheduled_message_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
        return Err(match state {
            Some(_) => ChatError::new(
                ChatErrorCode::Busy,
                "This scheduled message is already being sent",
                true,
            ),
            None => ChatError::new(
                ChatErrorCode::NotFound,
                "Scheduled message was not found",
                true,
            ),
        });
    }
    sqlx::query_scalar::<_, String>(
        "SELECT request_data FROM chat_scheduled_messages
         WHERE id = ? AND claim_token = ?",
    )
    .bind(scheduled_message_id.as_str())
    .bind(claim_token)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)
}

async fn read_scheduled_message(
    pool: &SqlitePool,
    scheduled_message_id: &ChatScheduledMessageId,
) -> ChatResult<ChatScheduledMessageRead> {
    let row = sqlx::query(
        "SELECT id, channel_id, reply_thread_id, request_data, state,
                scheduled_for, last_error, created_at
         FROM chat_scheduled_messages WHERE id = ?",
    )
    .bind(scheduled_message_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Scheduled message was not found",
            true,
        )
    })?;
    let request = serde_json::from_str::<PostChatMessageCommand>(
        &row.try_get::<String, _>("request_data")
            .map_err(persistence_error)?,
    )
    .map_err(serialization_error)?;
    let state: String = row.try_get("state").map_err(persistence_error)?;
    if !matches!(state.as_str(), "scheduled" | "dispatching" | "failed") {
        return Err(ChatError::new(
            ChatErrorCode::Internal,
            "Scheduled message state is invalid",
            false,
        ));
    }
    Ok(ChatScheduledMessageRead {
        id: ChatScheduledMessageId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
            .map_err(identifier_error)?,
        channel_id: ChatChannelId::new(
            row.try_get::<String, _>("channel_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        reply_thread_id: row
            .try_get::<Option<String>, _>("reply_thread_id")
            .map_err(persistence_error)?
            .map(ChatReplyThreadId::new)
            .transpose()
            .map_err(identifier_error)?,
        normalized_markdown: request.normalized_markdown,
        rich_content: request.rich_content,
        attachment_ids: request.attachment_ids,
        references: request.references,
        also_send_to_channel: request.also_send_to_channel,
        state,
        scheduled_for: UtcTimestamp::new(
            row.try_get::<String, _>("scheduled_for")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        last_error: row.try_get("last_error").map_err(persistence_error)?,
        created_at: UtcTimestamp::new(
            row.try_get::<String, _>("created_at")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
    })
}

async fn mark_attachment_unreferenced_if_unused(
    transaction: &mut Transaction<'_, Sqlite>,
    attachment_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_attachments SET unreferenced_at = ?
         WHERE id = ?
           AND NOT EXISTS (
             SELECT 1 FROM chat_attachment_references WHERE attachment_id = ?
           )
           AND NOT EXISTS (
             SELECT 1 FROM chat_queued_attachment_references WHERE attachment_id = ?
           )
           AND NOT EXISTS (
             SELECT 1 FROM chat_communication_attachment_references WHERE attachment_id = ?
           )
           AND NOT EXISTS (
             SELECT 1 FROM chat_scheduled_message_attachment_references WHERE attachment_id = ?
           )",
    )
    .bind(now.as_str())
    .bind(attachment_id)
    .bind(attachment_id)
    .bind(attachment_id)
    .bind(attachment_id)
    .bind(attachment_id)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}
