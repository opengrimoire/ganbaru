//! Native composer resources and interaction command facade.

use super::command_support::{chat_pool, now_timestamp};
use super::interaction;
use ganbaru_chat::repository::{attachments, recovery::recover_orphaned_turns};
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::events::{
    AccountStatusEvent, RateLimitStatusEvent, ThreadUsageUpdatedEvent,
};
use ganbaru_chat_contracts::models::{
    ChatAttachmentId, ChatCommandId, ChatPromptCatalogEntry, ChatResult, ChatThreadId,
    DriverOperationReceipt, McpStatusRead, ProjectWorkingFolderId, ProviderCapabilities,
    ProviderInstanceId, ProviderSessionState, UtcTimestamp, VersionedJson,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tauri::Manager;

pub use ganbaru_chat::composer::workspace_mentions::ProjectWorkingFolderPathPage;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportChatImageRequest {
    pub attachment_id: ChatAttachmentId,
    pub working_folder_id: ProjectWorkingFolderId,
    pub display_name: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickChatImagesRequest {
    pub working_folder_id: ProjectWorkingFolderId,
    pub attachment_ids: Vec<ChatAttachmentId>,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportChatTextSnippetRequest {
    pub working_folder_id: ProjectWorkingFolderId,
    pub attachment_id: ChatAttachmentId,
    pub display_name: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchWorkingFolderPathsRequest {
    pub working_folder_id: ProjectWorkingFolderId,
    pub query: String,
    pub include_ignored: bool,
    pub cursor: Option<String>,
    pub limit: u32,
    pub execution_environment_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPendingRequestRead {
    pub id: String,
    pub turn_id: Option<String>,
    pub provider_request_id: String,
    pub request_kind: String,
    pub safe_display: VersionedJson,
    pub allowed_decisions: VersionedJson,
    pub opened_at: UtcTimestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatQueuedFollowupRead {
    pub id: String,
    pub thread_id: ChatThreadId,
    pub text: String,
    pub provider_instance_id: ProviderInstanceId,
    pub model_selection: VersionedJson,
    pub safety_mode: ganbaru_chat_contracts::models::SafetyMode,
    pub interaction_mode: ganbaru_chat_contracts::models::InteractionMode,
    pub attachment_ids: Vec<ChatAttachmentId>,
    pub mentions: VersionedJson,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatInteractionStateRead {
    pub session_id: Option<String>,
    pub session_state: ProviderSessionState,
    pub active_turn_id: Option<String>,
    pub capabilities: ProviderCapabilities,
    pub pending_request: Option<ChatPendingRequestRead>,
    pub queued_followup: Option<ChatQueuedFollowupRead>,
    pub usage: Option<ThreadUsageUpdatedEvent>,
    pub account_status: Option<AccountStatusEvent>,
    pub rate_limit_status: Option<RateLimitStatusEvent>,
    pub automatic_compaction_reported: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveQueuedFollowupRequest {
    pub id: String,
    pub thread_id: ChatThreadId,
    pub text: String,
    pub provider_instance_id: ProviderInstanceId,
    pub model_selection: VersionedJson,
    pub safety_mode: ganbaru_chat_contracts::models::SafetyMode,
    pub interaction_mode: ganbaru_chat_contracts::models::InteractionMode,
    pub attachment_ids: Vec<ChatAttachmentId>,
    pub mentions: VersionedJson,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatUserInputDraftRead {
    pub request_id: String,
    pub answers: VersionedJson,
    pub updated_at: UtcTimestamp,
}

#[tauri::command]
pub async fn chat_import_image(
    app: tauri::AppHandle,
    db_url: String,
    request: ImportChatImageRequest,
) -> ChatResult<attachments::ChatAttachmentRead> {
    interaction::import_image(app, db_url, request).await
}

#[tauri::command]
pub async fn chat_pick_images(
    app: tauri::AppHandle,
    db_url: String,
    request: PickChatImagesRequest,
) -> ChatResult<Vec<attachments::ChatAttachmentRead>> {
    interaction::pick_images(app, db_url, request).await
}

#[tauri::command]
pub async fn chat_attachment_data_url(
    app: tauri::AppHandle,
    db_url: String,
    attachment_id: ChatAttachmentId,
) -> ChatResult<String> {
    interaction::attachment_data_url(app, db_url, attachment_id).await
}

#[tauri::command]
pub async fn chat_read_attachments(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    attachment_ids: Vec<ChatAttachmentId>,
) -> ChatResult<Vec<attachments::ChatAttachmentRead>> {
    interaction::read_attachments(app, db_url, working_folder_id, attachment_ids).await
}

#[tauri::command]
pub async fn chat_import_text_snippet(
    app: tauri::AppHandle,
    db_url: String,
    request: ImportChatTextSnippetRequest,
) -> ChatResult<attachments::ChatAttachmentRead> {
    interaction::import_text_snippet(app, db_url, request).await
}

#[tauri::command]
pub async fn chat_search_working_folder_paths(
    app: tauri::AppHandle,
    db_url: String,
    request: SearchWorkingFolderPathsRequest,
) -> ChatResult<ProjectWorkingFolderPathPage> {
    interaction::search_working_folder_paths(app, db_url, request).await
}

#[tauri::command]
pub async fn chat_validate_working_folder_mentions(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    relative_paths: Vec<String>,
    execution_environment_id: Option<String>,
) -> ChatResult<()> {
    interaction::validate_working_folder_mentions(
        app,
        db_url,
        working_folder_id,
        relative_paths,
        execution_environment_id,
    )
    .await
}

#[tauri::command]
pub async fn chat_list_prompt_catalog(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    provider_instance_id: ProviderInstanceId,
    thread_id: Option<ChatThreadId>,
) -> ChatResult<Vec<ChatPromptCatalogEntry>> {
    interaction::list_prompt_catalog(
        app,
        db_url,
        working_folder_id,
        provider_instance_id,
        thread_id,
    )
    .await
}

#[tauri::command]
pub async fn chat_read_interaction_state(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
) -> ChatResult<ChatInteractionStateRead> {
    interaction::read_interaction_state(app, db_url, thread_id).await
}

#[tauri::command]
pub async fn chat_recover_interrupted_turns(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<()> {
    let mut live_threads = HashSet::new();
    for owner in app.state::<ChatRuntimeRegistry>().owners()? {
        if owner.snapshot()?.session_id.is_some() {
            live_threads.insert(owner.thread_id().clone());
        }
    }
    recover_orphaned_turns(
        &chat_pool(app, db_url).await?,
        &live_threads,
        &now_timestamp()?,
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn chat_compact_context(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    client_command_id: ChatCommandId,
) -> ChatResult<DriverOperationReceipt> {
    interaction::compact_context(app, db_url, thread_id, client_command_id).await
}

#[tauri::command]
pub async fn chat_read_mcp_status(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    provider_instance_id: ProviderInstanceId,
    thread_id: Option<ChatThreadId>,
) -> ChatResult<McpStatusRead> {
    interaction::read_mcp_status(
        app,
        db_url,
        working_folder_id,
        provider_instance_id,
        thread_id,
    )
    .await
}

#[tauri::command]
pub fn chat_set_full_access_trust(
    app: tauri::AppHandle,
    provider_instance_id: ProviderInstanceId,
    working_folder_id: ProjectWorkingFolderId,
    trusted: bool,
) -> ChatResult<bool> {
    interaction::set_full_access_trust(app, provider_instance_id, working_folder_id, trusted)
}

#[tauri::command]
pub fn chat_has_full_access_trust(
    app: tauri::AppHandle,
    provider_instance_id: ProviderInstanceId,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<bool> {
    interaction::has_full_access_trust(app, provider_instance_id, working_folder_id)
}

#[tauri::command]
pub async fn chat_save_queued_followup(
    app: tauri::AppHandle,
    db_url: String,
    request: SaveQueuedFollowupRequest,
) -> ChatResult<ChatQueuedFollowupRead> {
    interaction::save_queued_followup(app, db_url, request).await
}

#[tauri::command]
pub async fn chat_cancel_queued_followup(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
) -> ChatResult<bool> {
    interaction::cancel_queued_followup(app, db_url, thread_id).await
}

#[tauri::command]
pub async fn chat_mark_queued_followup_dispatched(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    queued_followup_id: String,
) -> ChatResult<bool> {
    interaction::mark_queued_followup_dispatched(app, db_url, thread_id, queued_followup_id).await
}

#[tauri::command]
pub async fn chat_read_user_input_draft(
    app: tauri::AppHandle,
    db_url: String,
    request_id: String,
) -> ChatResult<Option<ChatUserInputDraftRead>> {
    interaction::read_user_input_draft(app, db_url, request_id).await
}

#[tauri::command]
pub async fn chat_save_user_input_draft(
    app: tauri::AppHandle,
    db_url: String,
    request_id: String,
    answers: VersionedJson,
) -> ChatResult<ChatUserInputDraftRead> {
    interaction::save_user_input_draft(app, db_url, request_id, answers).await
}

#[tauri::command]
pub async fn chat_stop_session(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    force: bool,
    client_command_id: ChatCommandId,
) -> ChatResult<DriverOperationReceipt> {
    interaction::stop_session(app, db_url, thread_id, force, client_command_id).await
}

pub(crate) fn workspace_mention_is_safety_excluded(relative_path: &str) -> bool {
    interaction::workspace_mention_is_safety_excluded(relative_path)
}
