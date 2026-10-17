//! Queued follow-up validation, persistence, and attachment references.

use super::attachments::MAX_IMAGE_COUNT;
use super::support::{
    chat_pool, corrupt_data_error, json_error, now_timestamp, persistence_error, versioned_row,
};
use crate::chat::interaction_commands::{ChatQueuedFollowupRead, SaveQueuedFollowupRequest};
use ganbaru_chat::repository::attachments;
use ganbaru_chat_contracts::models::{
    ChatAttachmentId, ChatError, ChatErrorCode, ChatResult, ChatThreadId, InteractionMode,
    ProviderInstanceId, SafetyMode, UtcTimestamp,
};
use sqlx::{Row, SqlitePool};

pub(crate) async fn save_queued_followup(
    app: tauri::AppHandle,
    db_url: String,
    request: SaveQueuedFollowupRequest,
) -> ChatResult<ChatQueuedFollowupRead> {
    let text = request.text.trim();
    if text.is_empty() || text.len() > 16_777_216 || request.attachment_ids.len() > MAX_IMAGE_COUNT
    {
        return Err(ChatError::validation(
            "queue",
            "Queued follow-up is invalid",
        ));
    }
    if !request.mentions.value.is_array() {
        return Err(ChatError::validation(
            "mentions",
            "Queued mentions must be an array",
        ));
    }
    let model = request.model_selection.value.as_object().ok_or_else(|| {
        ChatError::validation("modelSelection", "Queued model selection is invalid")
    })?;
    let model_id = model.get("modelId").and_then(serde_json::Value::as_str);
    let provider_managed = model
        .get("providerManaged")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if provider_managed == model_id.is_some()
        || !model
            .get("options")
            .is_some_and(serde_json::Value::is_array)
    {
        return Err(ChatError::validation(
            "modelSelection",
            "Queued model selection is invalid",
        ));
    }
    let pool = chat_pool(app, db_url).await?;
    let thread = sqlx::query(
        "SELECT working_folder_id, provider_instance_id FROM chat_threads
         WHERE id = ? AND archived_at IS NULL",
    )
    .bind(request.thread_id.as_str())
    .fetch_optional(&pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "Chat thread was not found", true))?;
    let working_folder_id: String = thread
        .try_get("working_folder_id")
        .map_err(persistence_error)?;
    let pinned_provider: String = thread
        .try_get("provider_instance_id")
        .map_err(persistence_error)?;
    if request.provider_instance_id.as_str() != pinned_provider {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Queued follow-up provider does not match the native thread",
            true,
        ));
    }
    let mut total_attachment_bytes = 0_u64;
    for attachment_id in &request.attachment_ids {
        let attachment = attachments::read_attachment(&pool, attachment_id)
            .await?
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::NotFound,
                    "Chat attachment was not found",
                    true,
                )
            })?;
        if attachment.working_folder_id.as_str() != working_folder_id {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Queued attachment belongs to another workspace",
                false,
            ));
        }
        total_attachment_bytes = total_attachment_bytes
            .checked_add(attachment.byte_size)
            .ok_or_else(|| {
                ChatError::validation("attachments", "Queued attachments are too large")
            })?;
        if total_attachment_bytes > 50 * 1024 * 1024 {
            return Err(ChatError::validation(
                "attachments",
                "Queued attachments must total 50 MiB or less",
            ));
        }
    }
    let attachment_ids = serde_json::to_string(&request.attachment_ids).map_err(json_error)?;
    let mentions = serde_json::to_string(&request.mentions.value).map_err(json_error)?;
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_queued_followups
            (id, thread_id, text, provider_instance_id, model_selection_schema_version,
             model_selection_data, safety_mode, interaction_mode, attachment_ids_data,
             mentions_schema_version, mentions_data, state, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?)
         ON CONFLICT(id) DO UPDATE SET text = excluded.text,
             provider_instance_id = excluded.provider_instance_id,
             model_selection_schema_version = excluded.model_selection_schema_version,
             model_selection_data = excluded.model_selection_data,
             safety_mode = excluded.safety_mode,
             interaction_mode = excluded.interaction_mode,
             attachment_ids_data = excluded.attachment_ids_data,
             mentions_schema_version = excluded.mentions_schema_version,
             mentions_data = excluded.mentions_data, state = 'queued',
             updated_at = excluded.updated_at",
    )
    .bind(&request.id)
    .bind(request.thread_id.as_str())
    .bind(&request.text)
    .bind(request.provider_instance_id.as_str())
    .bind(i64::from(request.model_selection.schema_version))
    .bind(serde_json::to_string(&request.model_selection.value).map_err(json_error)?)
    .bind(wire_safety(request.safety_mode))
    .bind(wire_interaction(request.interaction_mode))
    .bind(attachment_ids)
    .bind(i64::from(request.mentions.schema_version))
    .bind(mentions)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query("DELETE FROM chat_queued_attachment_references WHERE queued_followup_id = ?")
        .bind(&request.id)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    for attachment_id in &request.attachment_ids {
        sqlx::query(
            "INSERT INTO chat_queued_attachment_references
                (queued_followup_id, attachment_id, created_at) VALUES (?, ?, ?)",
        )
        .bind(&request.id)
        .bind(attachment_id.as_str())
        .bind(now.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    read_queued_followup(&pool, &request.thread_id)
        .await?
        .ok_or_else(corrupt_data_error)
}

pub(crate) async fn cancel_queued_followup(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
) -> ChatResult<bool> {
    let pool = chat_pool(app, db_url).await?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let active_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM chat_queued_followups WHERE thread_id = ? AND state = 'queued'",
    )
    .bind(thread_id.as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let changed = sqlx::query(
        "UPDATE chat_queued_followups SET state = 'cancelled', updated_at = ?
         WHERE thread_id = ? AND state = 'queued'",
    )
    .bind(now_timestamp()?.as_str())
    .bind(thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    if let Some(active_id) = active_id {
        sqlx::query("DELETE FROM chat_queued_attachment_references WHERE queued_followup_id = ?")
            .bind(active_id)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(changed > 0)
}

pub(crate) async fn mark_queued_followup_dispatched(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    queued_followup_id: String,
) -> ChatResult<bool> {
    let pool = chat_pool(app, db_url).await?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let changed = sqlx::query(
        "UPDATE chat_queued_followups SET state = 'dispatched', updated_at = ?
         WHERE id = ? AND thread_id = ? AND state = 'queued'",
    )
    .bind(now_timestamp()?.as_str())
    .bind(&queued_followup_id)
    .bind(thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    if changed > 0 {
        sqlx::query("DELETE FROM chat_queued_attachment_references WHERE queued_followup_id = ?")
            .bind(&queued_followup_id)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(changed > 0)
}

pub(super) async fn read_queued_followup(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<Option<ChatQueuedFollowupRead>> {
    let row = sqlx::query(
        "SELECT id, thread_id, text, provider_instance_id,
                model_selection_schema_version, model_selection_data,
                safety_mode, interaction_mode, attachment_ids_data,
                mentions_schema_version, mentions_data, created_at, updated_at
         FROM chat_queued_followups WHERE thread_id = ? AND state = 'queued'
         ORDER BY updated_at DESC, id DESC LIMIT 1",
    )
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    row.map(|row| {
        let attachment_ids: Vec<ChatAttachmentId> = serde_json::from_str::<Vec<String>>(
            &row.try_get::<String, _>("attachment_ids_data")
                .map_err(persistence_error)?,
        )
        .map_err(json_error)?
        .into_iter()
        .map(ChatAttachmentId::new)
        .collect::<Result<_, _>>()
        .map_err(|_| corrupt_data_error())?;
        Ok(ChatQueuedFollowupRead {
            id: row.try_get("id").map_err(persistence_error)?,
            thread_id: ChatThreadId::new(
                row.try_get::<String, _>("thread_id")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
            text: row.try_get("text").map_err(persistence_error)?,
            provider_instance_id: ProviderInstanceId::new(
                row.try_get::<String, _>("provider_instance_id")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
            model_selection: versioned_row(
                &row,
                "model_selection_schema_version",
                "model_selection_data",
            )?,
            safety_mode: parse_safety_mode(
                &row.try_get::<String, _>("safety_mode")
                    .map_err(persistence_error)?,
            )?,
            interaction_mode: parse_interaction_mode(
                &row.try_get::<String, _>("interaction_mode")
                    .map_err(persistence_error)?,
            )?,
            attachment_ids,
            mentions: versioned_row(&row, "mentions_schema_version", "mentions_data")?,
            created_at: UtcTimestamp::new(
                row.try_get::<String, _>("created_at")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
            updated_at: UtcTimestamp::new(
                row.try_get::<String, _>("updated_at")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
        })
    })
    .transpose()
}
fn wire_safety(value: SafetyMode) -> &'static str {
    match value {
        SafetyMode::AskForApproval => "ask_for_approval",
        SafetyMode::ApproveForMe => "approve_for_me",
        SafetyMode::FullAccess => "full_access",
        SafetyMode::Custom => "custom",
    }
}

fn parse_safety_mode(value: &str) -> ChatResult<SafetyMode> {
    match value {
        "ask_for_approval" => Ok(SafetyMode::AskForApproval),
        "approve_for_me" => Ok(SafetyMode::ApproveForMe),
        "full_access" => Ok(SafetyMode::FullAccess),
        "custom" => Ok(SafetyMode::Custom),
        _ => Err(corrupt_data_error()),
    }
}

fn wire_interaction(value: InteractionMode) -> &'static str {
    match value {
        InteractionMode::Build => "build",
        InteractionMode::Plan => "plan",
    }
}

fn parse_interaction_mode(value: &str) -> ChatResult<InteractionMode> {
    match value {
        "build" => Ok(InteractionMode::Build),
        "plan" => Ok(InteractionMode::Plan),
        _ => Err(corrupt_data_error()),
    }
}
