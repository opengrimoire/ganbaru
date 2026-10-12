//! Durable commands for the Chat composer draft.

use crate::db;
use chrono::{SecondsFormat, Utc};
use ganbaru_chat::repository::drafts::{self, ChatDraftRead, ChatDraftWrite};
use ganbaru_chat_contracts::models::{
    ChatAttachmentId, ChatError, ChatErrorCode, ChatResult, ChatThreadId, InteractionMode,
    ProjectWorkingFolderId, ProviderInstanceId, SafetyMode, UtcTimestamp, VersionedJson,
};
use serde::Deserialize;
use sqlx::SqlitePool;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveChatDraftRequest {
    id: String,
    working_folder_id: ProjectWorkingFolderId,
    thread_id: Option<ChatThreadId>,
    text: String,
    rich_content: Option<VersionedJson>,
    attachment_ids: Vec<ChatAttachmentId>,
    mentions: VersionedJson,
    provider_instance_id: Option<ProviderInstanceId>,
    model_selection: Option<VersionedJson>,
    safety_mode: Option<SafetyMode>,
    interaction_mode: Option<InteractionMode>,
    sent_snapshot: Option<VersionedJson>,
}

#[tauri::command]
pub async fn chat_save_draft(
    app: tauri::AppHandle,
    db_url: String,
    draft: SaveChatDraftRequest,
) -> ChatResult<ChatDraftRead> {
    let draft = ChatDraftWrite {
        id: draft.id,
        working_folder_id: draft.working_folder_id,
        thread_id: draft.thread_id,
        text: draft.text,
        rich_content: draft.rich_content,
        attachment_ids: draft.attachment_ids,
        mentions: draft.mentions,
        provider_instance_id: draft.provider_instance_id,
        model_selection: draft.model_selection,
        safety_mode: draft.safety_mode,
        interaction_mode: draft.interaction_mode,
        sent_snapshot: draft.sent_snapshot,
        updated_at: now_timestamp()?,
    };
    drafts::save_draft(&chat_pool(app, db_url).await?, &draft).await
}

#[tauri::command]
pub async fn chat_read_draft(
    app: tauri::AppHandle,
    db_url: String,
    draft_id: String,
) -> ChatResult<Option<ChatDraftRead>> {
    validate_draft_id(&draft_id)?;
    drafts::read_draft(&chat_pool(app, db_url).await?, &draft_id).await
}

#[tauri::command]
pub async fn chat_delete_draft(
    app: tauri::AppHandle,
    db_url: String,
    draft_id: String,
) -> ChatResult<bool> {
    validate_draft_id(&draft_id)?;
    drafts::delete_draft(&chat_pool(app, db_url).await?, &draft_id, &now_timestamp()?).await
}

fn validate_draft_id(value: &str) -> ChatResult<()> {
    if value.is_empty() || value.len() > 1_024 || value.chars().any(char::is_control) {
        return Err(ChatError::validation("draftId", "Chat draft ID is invalid"));
    }
    Ok(())
}

async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}
