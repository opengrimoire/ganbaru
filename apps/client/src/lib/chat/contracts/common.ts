export type JsonPrimitive = boolean | number | string | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

export type ProjectWorkingFolderId = string;
export type ChatChannelId = string;
export type ChatParticipantId = string;
export type ChatConversationId = string;
export type ChatConversationItemId = string;
export type ChatMessageRevisionId = string;
export type ChatReplyThreadId = string;
export type ChatTeammatePolicyRevisionId = string;
export type ChatAccessProfileId = string;
export type ChatAccessProfileRevisionId = string;
export type ChatAuthorizationRevisionId = string;
export type ChatMessageReferenceId = string;
export type ChatExecutionEnvironmentId = string;
export type ChatScratchScopeId = string;
export type ChatScratchGenerationId = string;
export type ChatScratchPromotionId = string;
export type ChatScratchCleanupJobId = string;
export type ChatWorkAssignmentId = string;
export type ChatAgentRunId = string;
export type ChatThreadId = string;
export type ChatTurnId = string;
export type ChatMessageId = string;
export type ChatActivityId = string;
export type ChatRequestId = string;
export type ChatPlanId = string;
export type ChatAttachmentId = string;
export type ChatEventId = string;
export type ChatCommandId = string;
export type ChatScheduledMessageId = string;
export type ChatCheckpointId = string;
export type CredentialReferenceId = string;
export type ProviderFamilyId = string;
export type ProviderInstanceId = string;
export type ProviderSessionId = string;
export type ProviderThreadId = string;
export type ProviderTurnId = string;
export type ProviderItemId = string;
export type ProviderRequestId = string;
export type ProviderTaskId = string;
export type ContinuationGroupId = string;
export type ModelId = string;
export type UtcTimestamp = string;

export const KNOWN_PROVIDER_FAMILIES = ["codex", "claude", "cursor", "grok", "opencode"] as const;
export type KnownProviderFamily = (typeof KNOWN_PROVIDER_FAMILIES)[number];

export const SAFETY_MODES = ["ask_for_approval", "approve_for_me", "full_access", "custom"] as const;
export type SafetyMode = (typeof SAFETY_MODES)[number];

export const INTERACTION_MODES = ["build", "plan"] as const;
export type InteractionMode = (typeof INTERACTION_MODES)[number];

export const REPOSITORY_KINDS = ["git", "none"] as const;
export type RepositoryKind = (typeof REPOSITORY_KINDS)[number];

export const CHAT_THREAD_STATES = ["draft", "active", "waiting", "idle", "error", "archived", "closed"] as const;
export type ChatThreadState = (typeof CHAT_THREAD_STATES)[number];

export const PROVIDER_SESSION_STATES = [
  "stopped",
  "starting",
  "ready",
  "active",
  "waiting_for_approval",
  "waiting_for_user_input",
  "stopping",
  "failed",
] as const;
export type ProviderSessionState = (typeof PROVIDER_SESSION_STATES)[number];

export const CHAT_TURN_STATES = [
  "pending",
  "dispatching",
  "active",
  "waiting_for_approval",
  "waiting_for_user_input",
  "completed",
  "interrupted",
  "failed",
] as const;
export type ChatTurnState = (typeof CHAT_TURN_STATES)[number];

export const MESSAGE_ROLES = ["user", "assistant", "system"] as const;
export type MessageRole = (typeof MESSAGE_ROLES)[number];

export const MESSAGE_STREAMING_STATES = ["pending", "streaming", "complete", "interrupted", "failed"] as const;
export type MessageStreamingState = (typeof MESSAGE_STREAMING_STATES)[number];

export const CANONICAL_ITEM_KINDS = [
  "user_message",
  "assistant_message",
  "reasoning",
  "plan",
  "command_execution",
  "file_change",
  "mcp_tool_call",
  "dynamic_tool_call",
  "collaboration_task",
  "web_search",
  "image_view",
  "review_transition",
  "context_compaction",
  "error",
  "unknown",
] as const;
export type CanonicalItemKind = (typeof CANONICAL_ITEM_KINDS)[number];

export const CONTENT_STREAM_KINDS = [
  "assistant_text",
  "reasoning_text",
  "reasoning_summary",
  "plan_text",
  "command_output",
  "file_change_output",
  "unknown",
] as const;
export type ContentStreamKind = (typeof CONTENT_STREAM_KINDS)[number];

export const ACTIVITY_STATUSES = ["pending", "active", "waiting", "completed", "interrupted", "failed", "unknown"] as const;
export type ActivityStatus = (typeof ACTIVITY_STATUSES)[number];

export const CANONICAL_REQUEST_KINDS = [
  "command_execution",
  "file_read",
  "file_change",
  "patch",
  "executable_command",
  "tool_input",
  "dynamic_tool",
  "authentication_refresh",
  "unknown",
] as const;
export type CanonicalRequestKind = (typeof CANONICAL_REQUEST_KINDS)[number];

export const APPROVAL_SUBJECT_KINDS = ["command", "path", "url", "pattern", "query", "tool"] as const;
export type ApprovalSubjectKind = (typeof APPROVAL_SUBJECT_KINDS)[number];

export const REQUEST_RESOLUTION_STATES =["open", "resolved", "stale", "interrupted"] as const;
export type RequestResolutionState = (typeof REQUEST_RESOLUTION_STATES)[number];

export const APPROVAL_DECISION_KINDS = ["allow_once", "allow_session", "deny", "cancel"] as const;
export type ApprovalDecisionKind = (typeof APPROVAL_DECISION_KINDS)[number];

export const PROBE_STATES = [
  "healthy",
  "executable_missing",
  "unsupported_version",
  "authentication_required",
  "configuration_invalid",
  "transport_unavailable",
] as const;
export type ProbeState = (typeof PROBE_STATES)[number];

export const MODEL_AVAILABILITIES = ["available", "unavailable", "deprecated", "stale", "unknown"] as const;
export type ModelAvailability = (typeof MODEL_AVAILABILITIES)[number];

export const MODEL_CATALOG_SOURCES = ["provider", "cache", "user"] as const;
export type ModelCatalogSource = (typeof MODEL_CATALOG_SOURCES)[number];

export const PROVIDER_CAPABILITIES = [
  "native_resume",
  "native_rollback",
  "native_plan",
  "steering",
  "queued_follow_up",
  "dynamic_model_change",
  "images",
  "file_references",
  "skills",
  "slash_commands",
  "approvals",
  "structured_questions",
  "reasoning_summaries",
  "structured_plans",
  "context_usage",
  "cost_reporting",
  "mcp_status",
  "account_status",
  "rate_limit_status",
  "provider_diffs",
  "task_activity",
] as const;
export type ProviderCapability = (typeof PROVIDER_CAPABILITIES)[number];

export const PROVIDER_IMPLEMENTATION_STATUSES = ["metadata_only", "available", "unsupported"] as const;
export type ProviderImplementationStatus = (typeof PROVIDER_IMPLEMENTATION_STATUSES)[number];
export const PROVIDER_MATURITIES = ["stable", "beta", "experimental"] as const;
export type ProviderMaturity = (typeof PROVIDER_MATURITIES)[number];

export const CHAT_ERROR_CODES = [
  "validation",
  "not_found",
  "conflict",
  "stale_revision",
  "invalid_state_transition",
  "busy",
  "capability_unsupported",
  "driver_unavailable",
  "executable_missing",
  "unsupported_version",
  "authentication_required",
  "configuration_invalid",
  "transport_unavailable",
  "resume_not_found",
  "protocol",
  "permission",
  "trust_decision_required",
  "runtime_approval_required",
  "timeout",
  "cancelled",
  "persistence",
  "internal",
] as const;
export type ChatErrorCode = (typeof CHAT_ERROR_CODES)[number];

export interface VersionedJson {
  schemaVersion: number;
  value: JsonValue;
}

export interface TurnModeSnapshot {
  safetyMode: SafetyMode;
  interactionMode: InteractionMode;
}

export interface ChatError {
  code: ChatErrorCode;
  message: string;
  field: string | null;
  recoverable: boolean;
  details: JsonValue | null;
}
