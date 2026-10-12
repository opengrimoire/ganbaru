//! Provider-neutral runtime and persistence events.

use crate::models::{
    ActivityStatus, ApprovalDecisionKind, CanonicalItemKind, CanonicalRequestKind, ChatEventId,
    ChatThreadId, ChatThreadState, ChatTurnId, ChatTurnState, ContentStreamKind, ModelId,
    ProviderCapabilities, ProviderFamilyId, ProviderInstanceId, ProviderItemId, ProviderRequestId,
    ProviderSessionId, ProviderSessionState, ProviderTaskId, ProviderThreadId, ProviderTurnId,
    RequestResolutionState, TurnModeSnapshot, UtcTimestamp, VersionedJson,
};
use serde::{Deserialize, Serialize};

pub const CANONICAL_EVENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalRuntimeEvent {
    pub schema_version: u32,
    pub event_id: ChatEventId,
    pub provider_family_id: ProviderFamilyId,
    pub provider_instance_id: ProviderInstanceId,
    pub thread_id: ChatThreadId,
    pub created_at: UtcTimestamp,
    pub turn_id: Option<ChatTurnId>,
    pub provider_turn_id: Option<ProviderTurnId>,
    pub provider_item_id: Option<ProviderItemId>,
    pub provider_request_id: Option<ProviderRequestId>,
    pub provider_task_id: Option<ProviderTaskId>,
    pub provider_reference: Option<VersionedJson>,
    pub event: CanonicalEvent,
    pub redacted_diagnostic: Option<VersionedJson>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalStoredEvent {
    pub sequence: u64,
    pub ingested_at: UtcTimestamp,
    #[serde(flatten)]
    pub runtime: CanonicalRuntimeEvent,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CanonicalEvent {
    SessionStarted(SessionStartedEvent),
    SessionConfigured(SessionConfiguredEvent),
    SessionStateChanged(SessionStateChangedEvent),
    SessionExited(SessionExitedEvent),
    ThreadStarted(ThreadStartedEvent),
    ThreadStateChanged(ThreadStateChangedEvent),
    ThreadMetadataUpdated(ThreadMetadataUpdatedEvent),
    ThreadReverted(ThreadRevertedEvent),
    ThreadUsageUpdated(ThreadUsageUpdatedEvent),
    TurnStarted(TurnStartedEvent),
    TurnCompleted(TurnCompletedEvent),
    TurnAborted(TurnAbortedEvent),
    PlanUpdated(PlanUpdatedEvent),
    ProposedPlanDelta(ProposedPlanDeltaEvent),
    ProposedPlanCompleted(ProposedPlanCompletedEvent),
    DiffUpdated(DiffUpdatedEvent),
    ItemStarted(ItemLifecycleEvent),
    ItemUpdated(ItemLifecycleEvent),
    ItemCompleted(ItemLifecycleEvent),
    ContentDelta(ContentDeltaEvent),
    RequestOpened(RequestOpenedEvent),
    RequestResolved(RequestResolvedEvent),
    UserInputRequested(UserInputRequestedEvent),
    UserInputResolved(UserInputResolvedEvent),
    TaskLifecycle(TaskLifecycleEvent),
    HookLifecycle(HookLifecycleEvent),
    ToolProgress(ToolProgressEvent),
    AuthenticationStatus(AuthenticationStatusEvent),
    AccountStatus(AccountStatusEvent),
    RateLimitStatus(RateLimitStatusEvent),
    McpStatus(McpStatusEvent),
    McpOauthCompleted(McpOauthCompletedEvent),
    ModelRerouted(ModelReroutedEvent),
    ConfigurationWarning(NotificationEvent),
    DeprecationNotice(NotificationEvent),
    FilesPersisted(FilesPersistedEvent),
    RuntimeWarning(NotificationEvent),
    RuntimeError(RuntimeErrorEvent),
    Unknown(UnknownEvent),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStartedEvent {
    pub session_id: ProviderSessionId,
    pub state: ProviderSessionState,
    pub provider_thread_id: Option<ProviderThreadId>,
    pub resume_cursor: Option<VersionedJson>,
    pub effective_modes: TurnModeSnapshot,
    pub capability_overrides: ProviderCapabilities,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfiguredEvent {
    pub session_id: ProviderSessionId,
    pub effective_modes: TurnModeSnapshot,
    pub effective_model_id: Option<ModelId>,
    pub effective_model_options: Vec<crate::models::ModelOptionSelection>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStateChangedEvent {
    pub session_id: ProviderSessionId,
    pub previous_state: ProviderSessionState,
    pub state: ProviderSessionState,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionExitedEvent {
    pub session_id: ProviderSessionId,
    pub expected: bool,
    pub exit_code: Option<i32>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartedEvent {
    pub provider_thread_id: ProviderThreadId,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStateChangedEvent {
    pub previous_state: ChatThreadState,
    pub state: ChatThreadState,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMetadataUpdatedEvent {
    pub title: Option<String>,
    pub provider_thread_id: Option<ProviderThreadId>,
    pub resume_cursor: Option<VersionedJson>,
    pub metadata: Option<VersionedJson>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRevertedEvent {
    pub checkpoint_id: crate::models::ChatCheckpointId,
    pub reverted_turn_ids: Vec<ChatTurnId>,
    pub provider_history_action: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUsageUpdatedEvent {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub context_tokens: Option<u64>,
    pub context_limit: Option<u64>,
    pub cost: Option<ProviderAttributedCost>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAttributedCost {
    pub amount: f64,
    pub currency: String,
    pub provider_reported: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartedEvent {
    pub provider_turn_id: Option<ProviderTurnId>,
    pub state: ChatTurnState,
    pub modes: TurnModeSnapshot,
    pub model_id: Option<ModelId>,
    pub model_options: Vec<crate::models::ModelOptionSelection>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCompletedEvent {
    pub state: ChatTurnState,
    pub stop_reason: Option<String>,
    pub usage: Option<ThreadUsageUpdatedEvent>,
    pub changed_files: Vec<ChangedFileSummary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnAbortedEvent {
    pub state: ChatTurnState,
    pub reason: String,
    pub recoverable: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanUpdatedEvent {
    pub markdown: String,
    pub steps: Vec<PlanStep>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub id: String,
    pub text: String,
    pub status: ActivityStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedPlanDeltaEvent {
    pub plan_id: String,
    pub delta: String,
    pub content_index: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedPlanCompletedEvent {
    pub plan_id: String,
    pub markdown: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffUpdatedEvent {
    pub source: String,
    pub files: Vec<ChangedFileSummary>,
    pub provider_diff: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFileSummary {
    pub relative_path: String,
    pub previous_relative_path: Option<String>,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary: bool,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemLifecycleEvent {
    pub item_id: String,
    pub kind: CanonicalItemKind,
    pub status: ActivityStatus,
    pub title: Option<String>,
    pub detail: Option<String>,
    pub safe_metadata: Option<VersionedJson>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentDeltaEvent {
    pub item_id: String,
    pub stream_kind: ContentStreamKind,
    pub content_index: u32,
    pub delta: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestOpenedEvent {
    pub request_id: ProviderRequestId,
    pub kind: CanonicalRequestKind,
    pub title: String,
    pub detail: Option<String>,
    pub allowed_decisions: Vec<ApprovalDecisionOption>,
    pub safe_payload: VersionedJson,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalDecisionOption {
    pub id: String,
    pub label: String,
    pub decision_kind: ApprovalDecisionKind,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestResolvedEvent {
    pub request_id: ProviderRequestId,
    pub state: RequestResolutionState,
    pub decision: Option<crate::models::ApprovalDecision>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputRequestedEvent {
    pub request_id: ProviderRequestId,
    pub questions: Vec<UserInputQuestion>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputQuestion {
    pub id: String,
    pub header: Option<String>,
    pub question: String,
    pub options: Vec<UserInputOption>,
    pub multiple: bool,
    pub free_form_allowed: bool,
    pub required: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputResolvedEvent {
    pub request_id: ProviderRequestId,
    pub state: RequestResolutionState,
    pub answers: Vec<crate::models::UserInputAnswer>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLifecycleEvent {
    pub task_id: String,
    pub parent_task_id: Option<String>,
    pub status: ActivityStatus,
    pub title: String,
    pub detail: Option<String>,
    pub safe_metadata: Option<VersionedJson>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookLifecycleEvent {
    pub hook_id: String,
    pub status: ActivityStatus,
    pub title: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProgressEvent {
    pub tool_id: String,
    pub status: ActivityStatus,
    pub title: String,
    pub progress: Option<f64>,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationStatusEvent {
    pub authenticated: bool,
    pub account_label: Option<String>,
    pub action_required: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatusEvent {
    pub account_label: Option<String>,
    pub plan_label: Option<String>,
    pub usage: Option<VersionedJson>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitStatusEvent {
    pub limited: bool,
    pub resets_at: Option<UtcTimestamp>,
    pub detail: Option<String>,
    pub provider_data: Option<VersionedJson>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusEvent {
    pub server_id: String,
    pub status: ActivityStatus,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOauthCompletedEvent {
    pub server_id: String,
    pub successful: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReroutedEvent {
    pub requested_model_id: ModelId,
    pub effective_model_id: ModelId,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationEvent {
    pub code: String,
    pub title: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesPersistedEvent {
    pub relative_paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeErrorEvent {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
    pub safe_details: Option<VersionedJson>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnknownEvent {
    pub source_type: String,
    pub summary: String,
    pub safe_payload: Option<VersionedJson>,
}
