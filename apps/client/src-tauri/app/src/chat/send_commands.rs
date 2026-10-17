//! Durable send, steer, approval, and structured-input command facade.

use ganbaru_chat::agent_runs::TurnOrigin;
use ganbaru_chat_contracts::models::{
    ApprovalDecision, ChatAttachmentId, ChatCommandContext, ChatError, ChatMessageId,
    ChatRequestId, ChatResult, ChatThreadId, ChatTurnId, DriverOperationReceipt, ModelId,
    ModelOptionSelection, ProjectWorkingFolderId, ProviderInstanceId, ProviderRequestId,
    TurnDispatchReceipt, TurnModeSnapshot, UserInputAnswer, WorkspaceMentionReference,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendChatTurnCommand {
    pub command: ChatCommandContext,
    #[serde(default)]
    pub working_folder_id: Option<ProjectWorkingFolderId>,
    pub thread_id: Option<ChatThreadId>,
    pub new_thread_id: Option<ChatThreadId>,
    pub execution_environment_id: Option<String>,
    #[serde(default)]
    pub scratch_generation_id: Option<String>,
    pub turn_id: ChatTurnId,
    pub message_id: ChatMessageId,
    pub provider_instance_id: ProviderInstanceId,
    pub provider_managed_model: bool,
    pub model_id: Option<ModelId>,
    pub model_options: Vec<ModelOptionSelection>,
    pub modes: TurnModeSnapshot,
    pub prompt: String,
    pub attachment_ids: Vec<ChatAttachmentId>,
    pub mentions: Vec<WorkspaceMentionReference>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendChatTurnResult {
    pub thread: ganbaru_chat_contracts::models::ChatThreadShellRead,
    pub dispatch: Option<TurnDispatchReceipt>,
    pub launch_error: Option<ChatError>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteerChatTurnCommand {
    pub command: ChatCommandContext,
    pub thread_id: ChatThreadId,
    pub message_id: ChatMessageId,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveChatApprovalCommand {
    pub command: ChatCommandContext,
    pub thread_id: ChatThreadId,
    pub request_id: ChatRequestId,
    pub provider_request_id: ProviderRequestId,
    pub decision: ApprovalDecision,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveChatUserInputCommand {
    pub command: ChatCommandContext,
    pub thread_id: ChatThreadId,
    pub request_id: ChatRequestId,
    pub provider_request_id: ProviderRequestId,
    pub answers: Vec<UserInputAnswer>,
}

#[tauri::command]
pub async fn chat_send_turn(
    app: tauri::AppHandle,
    db_url: String,
    request: SendChatTurnCommand,
) -> ChatResult<SendChatTurnResult> {
    super::turns::send_turn(
        app,
        db_url,
        super::turns::SendChatTurnInvocation {
            command: request,
            origin: TurnOrigin::Direct,
        },
    )
    .await
}

#[tauri::command]
pub async fn chat_steer_turn(
    app: tauri::AppHandle,
    db_url: String,
    request: SteerChatTurnCommand,
) -> ChatResult<DriverOperationReceipt> {
    super::turns::steer_turn(app, db_url, request).await
}

#[tauri::command]
pub async fn chat_resolve_approval(
    app: tauri::AppHandle,
    db_url: String,
    request: ResolveChatApprovalCommand,
) -> ChatResult<DriverOperationReceipt> {
    super::send::resolve_approval(app, db_url, request).await
}

#[tauri::command]
pub async fn chat_resolve_user_input(
    app: tauri::AppHandle,
    db_url: String,
    request: ResolveChatUserInputCommand,
) -> ChatResult<DriverOperationReceipt> {
    super::send::resolve_user_input(app, db_url, request).await
}
