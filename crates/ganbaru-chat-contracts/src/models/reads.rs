use super::{
    ChatActivityId, ChatExecutionEnvironmentId, ChatScratchGenerationId, ChatThreadId,
    ChatThreadState, ChatTurnId, ChatTurnState, ContinuationGroupId, ModelId, ModelOptionSelection,
    ProjectWorkingFolderId, ProviderCapabilities, ProviderFamilyId, ProviderInstanceId,
    ProviderSessionId, ProviderSessionState, ProviderThreadId, TurnModeSnapshot, UtcTimestamp,
    VersionedJson,
};
use crate::events::{ChangedFileSummary, ThreadUsageUpdatedEvent};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSessionSnapshot {
    pub session_id: ProviderSessionId,
    pub state: ProviderSessionState,
    pub provider_thread_id: Option<ProviderThreadId>,
    pub continuation_group_id: ContinuationGroupId,
    pub resume_cursor: Option<VersionedJson>,
    pub effective_modes: TurnModeSnapshot,
    pub capabilities: ProviderCapabilities,
    pub started_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnDispatchReceipt {
    pub turn_id: ChatTurnId,
    pub state: ChatTurnState,
    pub provider_turn_id: Option<super::ProviderTurnId>,
    pub accepted_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverOperationReceipt {
    pub accepted: bool,
    pub operation_id: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactContextRequest {
    pub session_id: ProviderSessionId,
    pub turn_id: ChatTurnId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusRequest {
    pub session_id: Option<ProviderSessionId>,
    pub working_directory: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatusRead {
    pub name: String,
    pub auth_status: Option<String>,
    pub enabled: bool,
    pub runtime_status: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusRead {
    pub servers: Vec<McpServerStatusRead>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPromptCatalogEntry {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub kind: String,
    pub source: String,
    pub stale: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHistoryItem {
    pub provider_item_id: Option<super::ProviderItemId>,
    pub provider_turn_id: Option<super::ProviderTurnId>,
    pub kind: String,
    pub data: VersionedJson,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHistoryPage {
    pub items: Vec<ProviderHistoryItem>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadShellRead {
    pub id: ChatThreadId,
    pub working_folder_id: Option<ProjectWorkingFolderId>,
    pub execution_environment_id: ChatExecutionEnvironmentId,
    pub scratch_generation_id: Option<ChatScratchGenerationId>,
    pub project_id: String,
    pub title: String,
    pub provider_family_id: ProviderFamilyId,
    pub provider_instance_id: ProviderInstanceId,
    pub provider_thread_id: Option<ProviderThreadId>,
    pub model_id: Option<ModelId>,
    pub model_options: Vec<ModelOptionSelection>,
    pub modes: TurnModeSnapshot,
    pub state: ChatThreadState,
    pub latest_turn_state: Option<ChatTurnState>,
    pub latest_preview: Option<String>,
    pub message_count: u64,
    pub revision: u64,
    pub last_event_sequence: u64,
    pub last_activity_at: UtcTimestamp,
    pub unread_at: Option<UtcTimestamp>,
    pub archived_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChangeNotification {
    pub thread_id: ChatThreadId,
    pub sequence: u64,
    pub revision: u64,
    pub changed_projection_keys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTimelineItemRead {
    pub activity_id: ChatActivityId,
    pub turn_id: Option<ChatTurnId>,
    pub sequence_anchor: u64,
    pub kind: String,
    pub data: VersionedJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_thread_id: Option<ChatThreadId>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTimelineTurnRead {
    pub turn_id: ChatTurnId,
    pub state: ChatTurnState,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
    pub stop_reason: Option<String>,
    pub model_id: Option<ModelId>,
    pub model_options: Vec<ModelOptionSelection>,
    pub modes: TurnModeSnapshot,
    pub usage: Option<ThreadUsageUpdatedEvent>,
    pub changed_files: Vec<ChangedFileSummary>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTimelinePageRead {
    pub thread_id: ChatThreadId,
    pub items: Vec<ChatTimelineItemRead>,
    pub turns: Vec<ChatTimelineTurnRead>,
    pub previous_cursor: Option<String>,
    pub next_cursor: Option<String>,
    pub thread_revision: u64,
}
