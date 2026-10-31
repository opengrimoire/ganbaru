//! Durable scheduled organizational message delivery.
//!
//! A scheduled message replicates, but only its executing device sends it, retries it, or sends
//! it at once; that device keeps the claim, attempt count, and retry time in its local dispatch
//! row. Any device may cancel a scheduled message.

use super::common::{
    AttachmentReferenceOwner, insert_attachment_references, new_id, serialization_error,
    this_device_id, validate_message_request, wire_participant_kind,
};
use super::{
    ScheduledSend, chat_pool, i64_value, identifier_error, now_timestamp, persistence_error,
    post_message, require_continuation_scope_is_unchanged, require_reply_thread,
    resolve_invoked_teammate,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat_contracts::models::*;
use ganbaru_sync::domains::chat::{
    MAX_SCHEDULED_CONTENT_BYTES, MAX_SCHEDULED_REFERENCES, MAX_SCHEDULED_REQUEST_BYTES,
};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::time::SystemTime;

const MIN_SCHEDULE_LEAD_SECONDS: i64 = 30;
const MAX_SCHEDULE_DAYS: i64 = 366;
const MAX_SCHEDULE_DISPATCH_ATTEMPTS: i64 = 3;
const SCHEDULE_RETRY_SECONDS: i64 = 60;

/// Schedules `request` for delivery. The scheduling device sends it: it records itself as the
/// executing device and keeps the claim and retry state in its local dispatch row.
pub(super) async fn schedule_message(
    app: tauri::AppHandle,
    db_url: String,
    request: ScheduleChatMessageCommand,
) -> ChatResult<ChatScheduledMessageRead> {
    validate_message_request(&request.message)?;
    let now = now_timestamp()?;
    validate_scheduled_for(&request.scheduled_for, &now)?;
    let serialized = serde_json::to_string(&request.message).map_err(serialization_error)?;
    validate_scheduled_content(&serialized, &request.message.references)?;
    let device_id = this_device_id(&app)?;
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
            (id, client_command_id, channel_id, reply_thread_id, executing_device_id,
             request_data, scheduled_for, created_at, updated_at)
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
    .bind(&device_id)
    .bind(&serialized)
    .bind(request.scheduled_for.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_scheduled_message_dispatch
            (scheduled_message_id, attempt_count, available_at, updated_at)
         VALUES (?, 0, ?, ?)",
    )
    .bind(request.scheduled_message_id.as_str())
    .bind(request.scheduled_for.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    insert_attachment_references(
        &mut transaction,
        AttachmentReferenceOwner::ScheduledMessage,
        request.scheduled_message_id.as_str(),
        &request.message.attachment_ids,
    )
    .await?;
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

/// Refuses a scheduled request whose stored content would not fit one sync operation: the
/// serialized request within [`MAX_SCHEDULED_REQUEST_BYTES`], at most
/// [`MAX_SCHEDULED_REFERENCES`] mentions, and the request with its mentions within
/// [`MAX_SCHEDULED_CONTENT_BYTES`]. The serialized mentions overstate their stored encoding, so
/// an accepted request always replicates.
pub(super) fn validate_scheduled_content(
    serialized_request: &str,
    references: &[ChatMessageReference],
) -> ChatResult<()> {
    if references.len() > MAX_SCHEDULED_REFERENCES {
        return Err(ChatError::validation(
            "references",
            "A scheduled message has too many mentions",
        ));
    }
    let references_bytes = serde_json::to_string(references)
        .map_err(serialization_error)?
        .len();
    if serialized_request.len() > MAX_SCHEDULED_REQUEST_BYTES
        || serialized_request.len().saturating_add(references_bytes) > MAX_SCHEDULED_CONTENT_BYTES
    {
        return Err(ChatError::validation(
            "message",
            "This message is too large to schedule",
        ));
    }
    Ok(())
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
            (scheduled_message_id, reference_kind, label_snapshot,
             plain_text_projection, start_offset, end_offset, participant_id,
             participant_kind, channel_id, working_folder_id, path_kind,
             relative_path, execution_environment_id, ordinal)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
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

fn scheduled_not_found() -> ChatError {
    ChatError::new(
        ChatErrorCode::NotFound,
        "Scheduled message was not found",
        true,
    )
}

fn scheduled_busy() -> ChatError {
    ChatError::new(
        ChatErrorCode::Busy,
        "This scheduled message is already being sent",
        true,
    )
}

/// Cancels a scheduled message from any device. Only a send this device has claimed refuses the
/// cancel; a send claimed on the executing device fails to post once the cancel reaches it.
pub(super) async fn cancel_scheduled_message(
    app: tauri::AppHandle,
    db_url: String,
    scheduled_message_id: ChatScheduledMessageId,
) -> ChatResult<()> {
    let pool = chat_pool(app, db_url).await?;
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
         WHERE id = ?1 AND NOT EXISTS (
           SELECT 1 FROM chat_scheduled_message_dispatch dispatch
           WHERE dispatch.scheduled_message_id = ?1 AND dispatch.claim_token IS NOT NULL
         )",
    )
    .bind(scheduled_message_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if deleted.rows_affected() != 1 {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_scheduled_messages WHERE id = ?)")
                .bind(scheduled_message_id.as_str())
                .fetch_one(&mut *transaction)
                .await
                .map_err(persistence_error)?;
        return Err(if exists {
            scheduled_busy()
        } else {
            scheduled_not_found()
        });
    }
    for attachment_id in attachment_ids {
        mark_attachment_unreferenced_if_unused(&mut transaction, &attachment_id, &now).await?;
    }
    transaction.commit().await.map_err(persistence_error)
}

/// Refuses work on the scheduled message `scheduled_message_id` unless the device `device_id`
/// sends it: it is the recorded executing device, or no device is recorded and this device holds
/// the message's local dispatch row.
pub(super) async fn require_scheduled_executor(
    pool: &SqlitePool,
    scheduled_message_id: &ChatScheduledMessageId,
    device_id: &str,
) -> ChatResult<()> {
    let row = sqlx::query(
        "SELECT scheduled.executing_device_id,
                EXISTS(
                  SELECT 1 FROM chat_scheduled_message_dispatch dispatch
                  WHERE dispatch.scheduled_message_id = scheduled.id
                ) AS dispatches_here
         FROM chat_scheduled_messages scheduled WHERE scheduled.id = ?",
    )
    .bind(scheduled_message_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(scheduled_not_found)?;
    let executing: Option<String> = row
        .try_get("executing_device_id")
        .map_err(persistence_error)?;
    let executes_here = match executing {
        Some(executing) => executing == device_id,
        None => row
            .try_get::<bool, _>("dispatches_here")
            .map_err(persistence_error)?,
    };
    if executes_here {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Conflict,
            "The device that scheduled this message sends it",
            true,
        ))
    }
}

/// Makes the failed scheduled message `scheduled_message_id` due again on its executing device.
pub(super) async fn retry_scheduled_message(
    app: tauri::AppHandle,
    db_url: String,
    scheduled_message_id: ChatScheduledMessageId,
) -> ChatResult<ChatScheduledMessageRead> {
    let device_id = this_device_id(&app)?;
    let pool = chat_pool(app, db_url).await?;
    let now = now_timestamp()?;
    require_scheduled_executor(&pool, &scheduled_message_id, &device_id).await?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let updated = sqlx::query(
        "UPDATE chat_scheduled_messages
         SET failed_at = NULL, last_error = NULL, updated_at = ?
         WHERE id = ? AND failed_at IS NOT NULL",
    )
    .bind(now.as_str())
    .bind(scheduled_message_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Only a failed scheduled message can be retried",
            true,
        ));
    }
    sqlx::query(
        "INSERT INTO chat_scheduled_message_dispatch
            (scheduled_message_id, attempt_count, available_at, updated_at)
         VALUES (?1, 0, ?2, ?2)
         ON CONFLICT(scheduled_message_id) DO UPDATE SET
            attempt_count = 0, available_at = excluded.available_at, claimed_at = NULL,
            claim_token = NULL, updated_at = excluded.updated_at",
    )
    .bind(scheduled_message_id.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    read_scheduled_message(&pool, &scheduled_message_id).await
}

/// Sends the scheduled message `scheduled_message_id` at once from its executing device.
pub(super) async fn send_scheduled_message_now(
    app: tauri::AppHandle,
    db_url: String,
    scheduled_message_id: ChatScheduledMessageId,
) -> ChatResult<PostChatMessageResult> {
    let device_id = this_device_id(&app)?;
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let now = now_timestamp()?;
    let claim_token = new_id("scheduled-message-immediate-claim")?;
    let request_data = claim_scheduled_message_for_immediate_send(
        &pool,
        &scheduled_message_id,
        &device_id,
        &now,
        &claim_token,
    )
    .await?;
    let send = ScheduledSend {
        scheduled_message_id: &scheduled_message_id,
        claim_token: &claim_token,
    };
    let result = match serde_json::from_str::<PostChatMessageCommand>(&request_data) {
        Ok(request) => post_message(app, db_url, &request, Some(send)).await,
        Err(error) => Err(serialization_error(error)),
    };
    match result {
        Ok(result) => Ok(result),
        Err(error) => {
            record_send_failure(&pool, &scheduled_message_id, &claim_token, &error, None).await?;
            Err(error)
        }
    }
}

/// Releases the claim `claim_token` of a send that failed with `error`. With `retry_at` the
/// message stays due at that time and keeps the error for display; without it the message
/// fails until it is retried.
async fn record_send_failure(
    pool: &SqlitePool,
    scheduled_message_id: &ChatScheduledMessageId,
    claim_token: &str,
    error: &ChatError,
    retry_at: Option<&str>,
) -> ChatResult<()> {
    let now = now_timestamp()?;
    let last_error = error.message.chars().take(4000).collect::<String>();
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let released = sqlx::query(
        "UPDATE chat_scheduled_message_dispatch
         SET available_at = coalesce(?, available_at), claimed_at = NULL, claim_token = NULL,
             updated_at = ?
         WHERE scheduled_message_id = ? AND claim_token = ?",
    )
    .bind(retry_at)
    .bind(now.as_str())
    .bind(scheduled_message_id.as_str())
    .bind(claim_token)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if released.rows_affected() == 1 {
        sqlx::query(
            "UPDATE chat_scheduled_messages
             SET failed_at = ?, last_error = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(retry_at.map_or(Some(now.as_str()), |_| None))
        .bind(last_error)
        .bind(now.as_str())
        .bind(scheduled_message_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)
}

/// Sends the scheduled messages this device executes that are due, and reports when the next
/// one is due here.
pub(super) async fn dispatch_due_scheduled_messages(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<ChatScheduledMessageDispatchRead> {
    let device_id = this_device_id(&app)?;
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let now = now_timestamp()?;
    prepare_scheduled_dispatch(&pool, &device_id, &now).await?;
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT scheduled.id
         FROM chat_scheduled_message_dispatch dispatch
         JOIN chat_scheduled_messages scheduled ON scheduled.id = dispatch.scheduled_message_id
         WHERE dispatch.claim_token IS NULL AND dispatch.available_at <= ?1
           AND scheduled.failed_at IS NULL
           AND coalesce(scheduled.executing_device_id, ?2) = ?2
         ORDER BY dispatch.available_at, scheduled.scheduled_for, scheduled.id LIMIT 32",
    )
    .bind(now.as_str())
    .bind(&device_id)
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    let mut processed_count = 0_u32;
    let mut dispatched_count = 0_u32;
    let mut dispatched_channel_ids = Vec::new();
    for id in ids {
        let scheduled_message_id = ChatScheduledMessageId::new(id).map_err(identifier_error)?;
        let claim_token = new_id("scheduled-message-claim")?;
        let claimed = sqlx::query(
            "UPDATE chat_scheduled_message_dispatch
             SET attempt_count = attempt_count + 1, claimed_at = ?1, claim_token = ?2,
                 updated_at = ?1
             WHERE scheduled_message_id = ?3 AND claim_token IS NULL AND available_at <= ?1",
        )
        .bind(now.as_str())
        .bind(&claim_token)
        .bind(scheduled_message_id.as_str())
        .execute(&pool)
        .await
        .map_err(persistence_error)?;
        if claimed.rows_affected() != 1 {
            continue;
        }
        processed_count = processed_count.saturating_add(1);
        let row = sqlx::query(
            "SELECT scheduled.request_data, dispatch.attempt_count
             FROM chat_scheduled_message_dispatch dispatch
             JOIN chat_scheduled_messages scheduled ON scheduled.id = dispatch.scheduled_message_id
             WHERE dispatch.scheduled_message_id = ? AND dispatch.claim_token = ?",
        )
        .bind(scheduled_message_id.as_str())
        .bind(&claim_token)
        .fetch_optional(&pool)
        .await
        .map_err(persistence_error)?;
        let Some(row) = row else {
            continue;
        };
        let request_data: String = row.try_get("request_data").map_err(persistence_error)?;
        let attempt_count: i64 = row.try_get("attempt_count").map_err(persistence_error)?;
        let message = serde_json::from_str::<PostChatMessageCommand>(&request_data)
            .map_err(serialization_error);
        let result = match message {
            Ok(message) => {
                let send = ScheduledSend {
                    scheduled_message_id: &scheduled_message_id,
                    claim_token: &claim_token,
                };
                post_message(app.clone(), db_url.clone(), &message, Some(send))
                    .await
                    .map(|_| message.channel_id.clone())
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(channel_id) => {
                dispatched_count = dispatched_count.saturating_add(1);
                if !dispatched_channel_ids.contains(&channel_id) {
                    dispatched_channel_ids.push(channel_id);
                }
            }
            Err(error) => {
                let retry_at = (DateTime::<Utc>::from(SystemTime::now())
                    + Duration::seconds(SCHEDULE_RETRY_SECONDS))
                .to_rfc3339_opts(SecondsFormat::Millis, true);
                let retry_at = (attempt_count < MAX_SCHEDULE_DISPATCH_ATTEMPTS).then_some(retry_at);
                record_send_failure(
                    &pool,
                    &scheduled_message_id,
                    &claim_token,
                    &error,
                    retry_at.as_deref(),
                )
                .await?;
            }
        }
    }
    let next_dispatch_at = sqlx::query_scalar::<_, Option<String>>(
        "SELECT MIN(dispatch.available_at)
         FROM chat_scheduled_message_dispatch dispatch
         JOIN chat_scheduled_messages scheduled ON scheduled.id = dispatch.scheduled_message_id
         WHERE dispatch.claim_token IS NULL AND scheduled.failed_at IS NULL
           AND coalesce(scheduled.executing_device_id, ?) = ?",
    )
    .bind(&device_id)
    .bind(&device_id)
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

/// Releases claims a stopped send left behind and gives every scheduled message this device
/// executes a dispatch row, such as one that reached this device's restored vault.
pub(super) async fn prepare_scheduled_dispatch(
    pool: &SqlitePool,
    device_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_scheduled_message_dispatch
         SET claimed_at = NULL, claim_token = NULL, available_at = ?1, updated_at = ?1
         WHERE claim_token IS NOT NULL
           AND claimed_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-5 minutes')",
    )
    .bind(now.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_scheduled_message_dispatch
            (scheduled_message_id, attempt_count, available_at, updated_at)
         SELECT scheduled.id, 0, scheduled.scheduled_for, ?1
         FROM chat_scheduled_messages scheduled
         WHERE scheduled.executing_device_id = ?2
         ON CONFLICT(scheduled_message_id) DO NOTHING",
    )
    .bind(now.as_str())
    .bind(device_id)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
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

/// Claims the scheduled message `scheduled_message_id` for an immediate send by its executing
/// device `device_id` and returns its stored request.
pub(super) async fn claim_scheduled_message_for_immediate_send(
    pool: &SqlitePool,
    scheduled_message_id: &ChatScheduledMessageId,
    device_id: &str,
    now: &UtcTimestamp,
    claim_token: &str,
) -> ChatResult<String> {
    require_scheduled_executor(pool, scheduled_message_id, device_id).await?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_scheduled_message_dispatch
            (scheduled_message_id, attempt_count, available_at, updated_at)
         VALUES (?1, 0, ?2, ?2)
         ON CONFLICT(scheduled_message_id) DO NOTHING",
    )
    .bind(scheduled_message_id.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let claimed = sqlx::query(
        "UPDATE chat_scheduled_message_dispatch
         SET attempt_count = attempt_count + 1, claimed_at = ?1, claim_token = ?2,
             updated_at = ?1
         WHERE scheduled_message_id = ?3 AND claim_token IS NULL",
    )
    .bind(now.as_str())
    .bind(claim_token)
    .bind(scheduled_message_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if claimed.rows_affected() != 1 {
        return Err(scheduled_busy());
    }
    let request_data = sqlx::query_scalar::<_, String>(
        "SELECT request_data FROM chat_scheduled_messages WHERE id = ?",
    )
    .bind(scheduled_message_id.as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .ok_or_else(scheduled_not_found)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(request_data)
}

/// Reads the scheduled message `scheduled_message_id`. It is `failed` once its executing device
/// gave up, `dispatching` while this device holds its send claim, and `scheduled` otherwise.
async fn read_scheduled_message(
    pool: &SqlitePool,
    scheduled_message_id: &ChatScheduledMessageId,
) -> ChatResult<ChatScheduledMessageRead> {
    let row = sqlx::query(
        "SELECT scheduled.id, scheduled.channel_id, scheduled.reply_thread_id,
                scheduled.request_data, scheduled.scheduled_for, scheduled.last_error,
                scheduled.created_at,
                CASE
                  WHEN EXISTS (
                    SELECT 1 FROM chat_scheduled_message_dispatch dispatch
                    WHERE dispatch.scheduled_message_id = scheduled.id
                      AND dispatch.claim_token IS NOT NULL
                  ) THEN 'dispatching'
                  WHEN scheduled.failed_at IS NOT NULL THEN 'failed'
                  ELSE 'scheduled'
                END AS state
         FROM chat_scheduled_messages scheduled WHERE scheduled.id = ?",
    )
    .bind(scheduled_message_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(scheduled_not_found)?;
    let request = serde_json::from_str::<PostChatMessageCommand>(
        &row.try_get::<String, _>("request_data")
            .map_err(persistence_error)?,
    )
    .map_err(serialization_error)?;
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
        state: row.try_get("state").map_err(persistence_error)?,
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
