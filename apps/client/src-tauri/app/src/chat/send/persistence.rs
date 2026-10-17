//! Atomic turn persistence, attachment loading, and send receipt replay.

use super::support::{corrupt_data_error, i64_value, json_error, persistence_error};
use super::validation::{prompt_preview, prompt_title, wire_interaction, wire_safety};
use crate::chat::interaction::MAX_IMAGE_COUNT;
use crate::chat::send_commands::{SendChatTurnCommand, SendChatTurnResult, SteerChatTurnCommand};
use crate::vault;
use ganbaru_chat::agent_runs::{StartingAgentRun, TurnOrigin};
use ganbaru_chat::repository::receipts::{CommandReceiptRead, CommandReceiptState};
use ganbaru_chat::repository::{attachments, reads};
use ganbaru_chat_contracts::models::*;
use serde_json::json;
use sqlx::{Row, SqlitePool};

#[derive(Clone, Debug)]
pub(super) struct ThreadRuntimeData {
    pub(super) working_folder_id: Option<ProjectWorkingFolderId>,
    pub(super) scratch_generation_id: Option<String>,
    pub(super) provider_instance_id: ProviderInstanceId,
    pub(super) continuation_group_id: ContinuationGroupId,
    pub(super) provider_thread_id: Option<ProviderThreadId>,
    pub(super) resume_cursor: Option<VersionedJson>,
    pub(super) revision: u64,
    pub(super) execution_environment_id: Option<String>,
}

pub(super) async fn read_thread_runtime_data(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<ThreadRuntimeData> {
    let row = sqlx::query(
        "SELECT working_folder_id, provider_instance_id, continuation_group_id,
                provider_thread_id, resume_cursor_schema_version, resume_cursor_data, revision,
                execution_environment_id, scratch_generation_id
         FROM chat_threads WHERE id = ? AND archived_at IS NULL AND state != 'closed'",
    )
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "Chat thread was not found", true))?;
    let resume_cursor = match (
        row.try_get::<Option<i64>, _>("resume_cursor_schema_version")
            .map_err(persistence_error)?,
        row.try_get::<Option<String>, _>("resume_cursor_data")
            .map_err(persistence_error)?,
    ) {
        (None, None) => None,
        (Some(version), Some(data)) => Some(VersionedJson {
            schema_version: u32::try_from(version).map_err(|_| corrupt_data_error())?,
            value: serde_json::from_str(&data).map_err(json_error)?,
        }),
        _ => return Err(corrupt_data_error()),
    };
    Ok(ThreadRuntimeData {
        working_folder_id: row
            .try_get::<Option<String>, _>("working_folder_id")
            .map_err(persistence_error)?
            .map(ProjectWorkingFolderId::new)
            .transpose()
            .map_err(|_| corrupt_data_error())?,
        scratch_generation_id: row
            .try_get("scratch_generation_id")
            .map_err(persistence_error)?,
        provider_instance_id: ProviderInstanceId::new(
            row.try_get::<String, _>("provider_instance_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        continuation_group_id: ContinuationGroupId::new(
            row.try_get::<String, _>("continuation_group_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        provider_thread_id: row
            .try_get::<Option<String>, _>("provider_thread_id")
            .map_err(persistence_error)?
            .map(ProviderThreadId::new)
            .transpose()
            .map_err(|_| corrupt_data_error())?,
        resume_cursor,
        revision: u64::try_from(
            row.try_get::<i64, _>("revision")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        execution_environment_id: row
            .try_get("execution_environment_id")
            .map_err(persistence_error)?,
    })
}

pub(super) async fn read_attachment_references(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    working_folder_id: Option<&ProjectWorkingFolderId>,
    attachment_ids: &[ChatAttachmentId],
) -> ChatResult<Vec<PromptAttachmentReference>> {
    if attachment_ids.len() > MAX_IMAGE_COUNT {
        return Err(ChatError::validation(
            "attachments",
            "Too many Chat attachments",
        ));
    }
    let mut result = Vec::with_capacity(attachment_ids.len());
    let mut total_bytes = 0_u64;
    let vault_root = vault::active_vault_path(app).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "Active Ganbaru folder is unavailable",
            true,
        )
    })?;
    for id in attachment_ids {
        let attachment = attachments::read_attachment(pool, id)
            .await?
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::NotFound,
                    "Chat attachment was not found",
                    true,
                )
            })?;
        if working_folder_id
            .is_some_and(|working_folder_id| &attachment.working_folder_id != working_folder_id)
        {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Chat attachment belongs to another workspace",
                false,
            ));
        }
        total_bytes = total_bytes
            .checked_add(attachment.byte_size)
            .ok_or_else(|| {
                ChatError::validation("attachments", "Chat attachment total is too large")
            })?;
        if total_bytes > 50 * 1024 * 1024 {
            return Err(ChatError::validation(
                "attachments",
                "Chat attachments must total 50 MiB or less",
            ));
        }
        let (local_path, bytes) =
            attachments::read_managed_attachment_bytes(&vault_root, &attachment)?;
        let (local_path, text_content) = match attachment.kind {
            attachments::ChatAttachmentKind::Image => (
                Some(
                    local_path
                        .to_str()
                        .ok_or_else(|| {
                            ChatError::validation(
                                "attachments",
                                "Managed attachment path is unsupported",
                            )
                        })?
                        .to_string(),
                ),
                None,
            ),
            attachments::ChatAttachmentKind::TextSnippet => (
                None,
                Some(String::from_utf8(bytes).map_err(|_| {
                    ChatError::validation("attachments", "Text context is invalid")
                })?),
            ),
        };
        result.push(PromptAttachmentReference {
            attachment_id: id.clone(),
            kind: match attachment.kind {
                attachments::ChatAttachmentKind::Image => "image",
                attachments::ChatAttachmentKind::TextSnippet => "text_snippet",
            }
            .to_string(),
            display_name: attachment.original_display_name,
            resource_uri: format!("ganbaru://chat/resource/{}", id.as_str()),
            managed_relative_path: attachment.managed_relative_path,
            mime_type: Some(attachment.mime_type),
            byte_size: attachment.byte_size,
            local_path,
            text_content,
        });
    }
    Ok(result)
}

pub(super) struct PersistUserTurnContext<'a> {
    pub(super) pool: &'a SqlitePool,
    pub(super) target: &'a TurnPersistenceTarget,
    pub(super) thread_id: &'a ChatThreadId,
    pub(super) existing: Option<&'a ThreadRuntimeData>,
    pub(super) continuation_group_id: &'a ContinuationGroupId,
    pub(super) provider_family_id: &'a ProviderFamilyId,
    pub(super) request: &'a SendChatTurnCommand,
    pub(super) origin: &'a TurnOrigin,
    pub(super) attachments: &'a [PromptAttachmentReference],
    pub(super) now: &'a UtcTimestamp,
}

#[derive(Clone, Debug)]
pub(super) struct TurnPersistenceTarget {
    pub(super) project_id: String,
    pub(super) working_folder_id: Option<ProjectWorkingFolderId>,
    pub(super) scratch_generation_id: Option<String>,
    pub(super) execution_environment_id: Option<String>,
}

pub(super) async fn persist_user_turn(context: PersistUserTurnContext<'_>) -> ChatResult<()> {
    let PersistUserTurnContext {
        pool,
        target,
        thread_id,
        existing,
        continuation_group_id,
        provider_family_id,
        request,
        origin,
        attachments,
        now,
    } = context;
    let model_selection = serde_json::to_string(&json!({
        "modelId": request.model_id,
        "providerManaged": request.provider_managed_model,
        "options": request.model_options,
    }))
    .map_err(json_error)?;
    let user_context = serde_json::to_string(&json!({
        "attachments": attachments.iter().map(|attachment| json!({
            "attachmentId": attachment.attachment_id,
            "displayName": attachment.display_name,
            "kind": attachment.kind,
            "mimeType": attachment.mime_type,
            "byteSize": attachment.byte_size,
            "status": "managed",
        })).collect::<Vec<_>>(),
        "mentions": request.mentions,
        "terminalContext": attachments.iter().filter(|attachment| attachment.kind == "text_snippet").map(|attachment| json!({
            "attachmentId": attachment.attachment_id,
            "displayName": attachment.display_name,
            "byteSize": attachment.byte_size,
        })).collect::<Vec<_>>(),
        "preCheckpointId": null,
    }))
    .map_err(json_error)?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    if let Some(existing) = existing {
        if request
            .command
            .expected_thread_revision
            .is_some_and(|revision| revision != existing.revision)
        {
            return Err(ChatError::new(
                ChatErrorCode::StaleRevision,
                "Chat thread changed before send",
                true,
            ));
        }
        let duplicate: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM chat_command_receipts WHERE client_command_id = ?)",
        )
        .bind(request.command.client_command_id.as_str())
        .fetch_one(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        if duplicate {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "This Chat send command was already submitted",
                true,
            ));
        }
    } else {
        let title = prompt_title(&request.prompt);
        sqlx::query(
            "INSERT INTO chat_threads
                (id, working_folder_id, execution_environment_id, scratch_generation_id,
                 project_id, title, provider_family_id,
                 provider_instance_id, continuation_group_id, model_selection_data,
                 safety_mode, interaction_mode, state, latest_turn_state,
                 last_activity_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', 'pending', ?, ?, ?)",
        )
        .bind(thread_id.as_str())
        .bind(
            target
                .working_folder_id
                .as_ref()
                .map(ProjectWorkingFolderId::as_str),
        )
        .bind(&target.execution_environment_id)
        .bind(&target.scratch_generation_id)
        .bind(&target.project_id)
        .bind(title)
        .bind(provider_family_id.as_str())
        .bind(request.provider_instance_id.as_str())
        .bind(continuation_group_id.as_str())
        .bind(&model_selection)
        .bind(wire_safety(request.modes.safety_mode))
        .bind(wire_interaction(request.modes.interaction_mode))
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    let ordinal: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM chat_turns WHERE thread_id = ?",
    )
    .bind(thread_id.as_str())
    .fetch_one(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let sequence: i64 =
        sqlx::query_scalar("SELECT last_event_sequence FROM chat_threads WHERE id = ?")
            .bind(thread_id.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_turns
            (id, thread_id, ordinal, user_message_id, state, model_selection_data,
             safety_mode, interaction_mode, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?)",
    )
    .bind(request.turn_id.as_str())
    .bind(thread_id.as_str())
    .bind(ordinal)
    .bind(request.message_id.as_str())
    .bind(&model_selection)
    .bind(wire_safety(request.modes.safety_mode))
    .bind(wire_interaction(request.modes.interaction_mode))
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_messages
            (id, thread_id, turn_id, sequence_anchor, role, normalized_markdown,
             streaming_state, content_metadata_data, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'user', ?, 'complete', ?, ?, ?)",
    )
    .bind(request.message_id.as_str())
    .bind(thread_id.as_str())
    .bind(request.turn_id.as_str())
    .bind(sequence)
    .bind(&request.prompt)
    .bind(user_context)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    for (index, attachment) in attachments.iter().enumerate() {
        sqlx::query(
            "INSERT INTO chat_attachment_references
                (id, attachment_id, message_id, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(format!("message:{}:{index}", request.message_id.as_str()))
        .bind(attachment.attachment_id.as_str())
        .bind(request.message_id.as_str())
        .bind(now.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    sqlx::query(
        "INSERT INTO chat_command_receipts
            (client_command_id, thread_id, command_kind, submitted_revision,
             state, created_at, updated_at)
         VALUES (?, ?, 'send_turn', ?, 'accepted', ?, ?)",
    )
    .bind(request.command.client_command_id.as_str())
    .bind(thread_id.as_str())
    .bind(
        request
            .command
            .expected_thread_revision
            .map(i64_value)
            .transpose()?,
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_threads SET model_selection_data = ?, safety_mode = ?,
                interaction_mode = ?, state = 'active', latest_turn_state = 'pending',
                latest_preview = ?, message_count = message_count + 1,
                revision = revision + 1, last_activity_at = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(model_selection)
    .bind(wire_safety(request.modes.safety_mode))
    .bind(wire_interaction(request.modes.interaction_mode))
    .bind(prompt_preview(&request.prompt))
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if let Some(binding) = origin.run() {
        ganbaru_chat::agent_runs::insert_starting_run(
            &mut transaction,
            StartingAgentRun {
                binding,
                provider_turn_id: &request.turn_id,
                provider_thread_id: thread_id,
                now,
            },
        )
        .await?;
    }
    transaction.commit().await.map_err(persistence_error)
}

pub(super) async fn persist_steer_message(
    pool: &SqlitePool,
    request: &SteerChatTurnCommand,
    turn_id: &ChatTurnId,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let sequence: i64 =
        sqlx::query_scalar("SELECT last_event_sequence FROM chat_threads WHERE id = ?")
            .bind(request.thread_id.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_messages
            (id, thread_id, turn_id, sequence_anchor, role, normalized_markdown,
             streaming_state, content_metadata_data, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'user', ?, 'complete', ?, ?, ?)",
    )
    .bind(request.message_id.as_str())
    .bind(request.thread_id.as_str())
    .bind(turn_id.as_str())
    .bind(sequence)
    .bind(&request.prompt)
    .bind(json!({ "steer": true }).to_string())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_threads SET latest_preview = ?, message_count = message_count + 1,
                revision = revision + 1, last_activity_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(prompt_preview(&request.prompt))
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(request.thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)
}

pub(super) async fn mark_turn_dispatch_failed(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    turn_id: &ChatTurnId,
    error: &ChatError,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let error_data = serde_json::to_string(error).map_err(json_error)?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_turns SET state = 'failed', completed_at = ?, stop_reason = ?,
                error_schema_version = 1, error_data = ?, updated_at = ?
         WHERE id = ? AND thread_id = ?",
    )
    .bind(now.as_str())
    .bind(&error.message)
    .bind(error_data)
    .bind(now.as_str())
    .bind(turn_id.as_str())
    .bind(thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_threads SET state = 'error', latest_turn_state = 'failed',
                revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)
}

pub(super) async fn replay_send_receipt(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    receipt: &CommandReceiptRead,
) -> ChatResult<SendChatTurnResult> {
    if receipt.thread_id != *thread_id || receipt.command_kind != "send_turn" {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Chat command ID was already used for a different operation",
            false,
        ));
    }
    match receipt.state {
        CommandReceiptState::Accepted => Err(ChatError::new(
            ChatErrorCode::Busy,
            "This Chat send command is still being processed",
            true,
        )),
        CommandReceiptState::Failed => Err(receipt_error(receipt)?),
        CommandReceiptState::Completed => {
            let mut result: SendChatTurnResult = serde_json::from_value(
                receipt
                    .result
                    .as_ref()
                    .ok_or_else(corrupt_data_error)?
                    .value
                    .clone(),
            )
            .map_err(json_error)?;
            result.thread = reads::read_thread_shell(pool, thread_id).await?;
            Ok(result)
        }
    }
}

fn receipt_error(receipt: &CommandReceiptRead) -> ChatResult<ChatError> {
    serde_json::from_value(
        receipt
            .error
            .as_ref()
            .ok_or_else(corrupt_data_error)?
            .value
            .clone(),
    )
    .map_err(json_error)
}
