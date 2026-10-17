use super::{
    ApprovalDecisionKind, ChatAttachmentId, ChatCheckpointId, ChatCommandId, ChatRequestId,
    ChatThreadId, ChatTurnId, ContinuationGroupId, ModelId, ModelOptionSelection,
    ProjectWorkingFolderId, ProviderInstanceId, ProviderRequestId, ProviderSessionId,
    ProviderThreadId, RepositoryKind, TurnModeSnapshot, VersionedJson,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCommandContext {
    pub client_command_id: ChatCommandId,
    pub expected_thread_revision: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedWorkspaceContext {
    pub working_folder_id: ProjectWorkingFolderId,
    pub canonical_path: String,
    pub repository_kind: RepositoryKind,
    pub repository_identity: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptAttachmentReference {
    pub attachment_id: ChatAttachmentId,
    pub kind: String,
    pub display_name: String,
    pub managed_relative_path: String,
    pub resource_uri: String,
    pub mime_type: Option<String>,
    pub byte_size: u64,
    pub local_path: Option<String>,
    pub text_content: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMentionReference {
    pub relative_path: String,
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSessionRequest {
    pub thread_id: ChatThreadId,
    pub workspace: VerifiedWorkspaceContext,
    pub provider_instance_id: ProviderInstanceId,
    pub modes: TurnModeSnapshot,
    pub model_id: Option<ModelId>,
    pub model_options: Vec<ModelOptionSelection>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSessionRequest {
    pub thread_id: ChatThreadId,
    pub workspace: VerifiedWorkspaceContext,
    pub provider_instance_id: ProviderInstanceId,
    pub provider_thread_id: ProviderThreadId,
    pub continuation_group_id: ContinuationGroupId,
    pub resume_cursor: VersionedJson,
    pub modes: TurnModeSnapshot,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderThreadLifecycleRequest {
    pub provider_thread_id: ProviderThreadId,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderForkThreadRequest {
    pub provider_thread_id: ProviderThreadId,
    pub workspace: VerifiedWorkspaceContext,
    pub last_provider_turn_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendTurnRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub turn_id: ChatTurnId,
    pub prompt: String,
    pub attachments: Vec<PromptAttachmentReference>,
    pub mentions: Vec<WorkspaceMentionReference>,
    pub model_id: Option<ModelId>,
    pub model_options: Vec<ModelOptionSelection>,
    pub modes: TurnModeSnapshot,
    pub developer_instructions: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteerTurnRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub turn_id: ChatTurnId,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptTurnRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub turn_id: ChatTurnId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalDecision {
    pub kind: ApprovalDecisionKind,
    pub provider_option_id: Option<String>,
    pub updated_tool_input: Option<VersionedJson>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveApprovalRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub request_id: ChatRequestId,
    pub provider_request_id: ProviderRequestId,
    pub decision: ApprovalDecision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputAnswer {
    pub question_id: String,
    pub selected_option_ids: Vec<String>,
    pub free_form_text: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveUserInputRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub request_id: ChatRequestId,
    pub provider_request_id: ProviderRequestId,
    pub answers: Vec<UserInputAnswer>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub checkpoint_id: Option<ChatCheckpointId>,
    pub provider_cursor: Option<VersionedJson>,
    pub target_turn_id: Option<ChatTurnId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadHistoryRequest {
    pub session_id: ProviderSessionId,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopSessionRequest {
    pub session_id: ProviderSessionId,
    pub force: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationGroupRequest {
    pub provider_instance_id: ProviderInstanceId,
    pub normalized_provider_home: Option<String>,
    pub account_identity: Option<String>,
    pub server_identity: Option<String>,
    pub provider_fields: BTreeMap<String, String>,
}
