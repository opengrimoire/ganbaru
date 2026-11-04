import type {
  ActivityStatus,
  ApprovalDecisionKind,
  ApprovalSubjectKind,
  CanonicalItemKind,
  CanonicalRequestKind,
  ChatEventId,
  ChatCheckpointId,
  ChatThreadId,
  ChatThreadState,
  ChatTurnId,
  ChatTurnState,
  ContentStreamKind,
  ModelId,
  ProviderFamilyId,
  ProviderInstanceId,
  ProviderItemId,
  ProviderRequestId,
  ProviderSessionId,
  ProviderSessionState,
  ProviderTaskId,
  ProviderThreadId,
  ProviderTurnId,
  RequestResolutionState,
  TurnModeSnapshot,
  UtcTimestamp,
  VersionedJson,
} from "./common";
import type { ApprovalDecision, UserInputAnswer } from "./commands";
import type { ModelOptionSelection, ProviderCapabilities } from "./provider";

export const CANONICAL_EVENT_SCHEMA_VERSION = 1;

export interface CanonicalRuntimeEvent {
  schemaVersion: number;
  eventId: ChatEventId;
  providerFamilyId: ProviderFamilyId;
  providerInstanceId: ProviderInstanceId;
  threadId: ChatThreadId;
  createdAt: UtcTimestamp;
  turnId: ChatTurnId | null;
  providerTurnId: ProviderTurnId | null;
  providerItemId: ProviderItemId | null;
  providerRequestId: ProviderRequestId | null;
  providerTaskId: ProviderTaskId | null;
  providerReference: VersionedJson | null;
  event: CanonicalEvent;
  redactedDiagnostic: VersionedJson | null;
}

export interface CanonicalStoredEvent extends CanonicalRuntimeEvent {
  sequence: number;
  ingestedAt: UtcTimestamp;
}

export interface SessionStartedEvent {
  sessionId: ProviderSessionId;
  state: ProviderSessionState;
  providerThreadId: ProviderThreadId | null;
  resumeCursor: VersionedJson | null;
  effectiveModes: TurnModeSnapshot;
  capabilityOverrides: ProviderCapabilities;
}

export interface SessionConfiguredEvent {
  sessionId: ProviderSessionId;
  effectiveModes: TurnModeSnapshot;
  effectiveModelId: ModelId | null;
  effectiveModelOptions: ModelOptionSelection[];
}

export interface SessionStateChangedEvent {
  sessionId: ProviderSessionId;
  previousState: ProviderSessionState;
  state: ProviderSessionState;
  reason: string | null;
}

export interface SessionExitedEvent {
  sessionId: ProviderSessionId;
  expected: boolean;
  exitCode: number | null;
  reason: string | null;
}

export interface ThreadStartedEvent {
  providerThreadId: ProviderThreadId;
  title: string | null;
}

export interface ThreadStateChangedEvent {
  previousState: ChatThreadState;
  state: ChatThreadState;
  reason: string | null;
}

export interface ThreadMetadataUpdatedEvent {
  title: string | null;
  providerThreadId: ProviderThreadId | null;
  resumeCursor: VersionedJson | null;
  metadata: VersionedJson | null;
}

export interface ThreadRevertedEvent {
  checkpointId: ChatCheckpointId;
  revertedTurnIds: ChatTurnId[];
  providerHistoryAction: "rolled_back" | "fork_required";
}

export interface ProviderAttributedCost {
  amount: number;
  currency: string;
  providerReported: boolean;
}

export interface ThreadUsageUpdatedEvent {
  inputTokens: number | null;
  outputTokens: number | null;
  cachedInputTokens: number | null;
  contextTokens: number | null;
  contextLimit: number | null;
  cost: ProviderAttributedCost | null;
}

export interface TurnStartedEvent {
  providerTurnId: ProviderTurnId | null;
  state: ChatTurnState;
  modes: TurnModeSnapshot;
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
}

export interface ChangedFileSummary {
  relativePath: string;
  previousRelativePath: string | null;
  additions: number | null;
  deletions: number | null;
  binary: boolean;
  status: string;
}

export interface TurnCompletedEvent {
  state: ChatTurnState;
  stopReason: string | null;
  usage: ThreadUsageUpdatedEvent | null;
  changedFiles: ChangedFileSummary[];
}

export interface TurnAbortedEvent {
  state: ChatTurnState;
  reason: string;
  recoverable: boolean;
}

export interface PlanStep {
  id: string;
  text: string;
  status: ActivityStatus;
}

export interface PlanUpdatedEvent {
  markdown: string;
  steps: PlanStep[];
}

export interface ProposedPlanDeltaEvent {
  planId: string;
  delta: string;
  contentIndex: number;
}

export interface ProposedPlanCompletedEvent {
  planId: string;
  markdown: string;
}

export interface DiffUpdatedEvent {
  source: string;
  files: ChangedFileSummary[];
  providerDiff: string | null;
}

export interface ItemLifecycleEvent {
  itemId: string;
  kind: CanonicalItemKind;
  status: ActivityStatus;
  title: string | null;
  detail: string | null;
  safeMetadata: VersionedJson | null;
}

export interface ContentDeltaEvent {
  itemId: string;
  streamKind: ContentStreamKind;
  contentIndex: number;
  delta: string;
}

export interface ApprovalDecisionOption {
  id: string;
  label: string;
  decisionKind: ApprovalDecisionKind;
  description: string | null;
}

/** Concrete target of an approval, such as the exact command or path, shown before the decision. */
export interface ApprovalSubject {
  kind: ApprovalSubjectKind;
  text: string;
  location: string | null;
  truncated: boolean;
}

/** Permission that an allow-for-session decision forwards to the running provider session. */
export type SessionPermissionGrant =
  | { kind: "allow_rule"; toolName: string; ruleContent: string | null }
  | { kind: "directory"; path: string }
  | { kind: "accept_edits" };

export interface RequestOpenedEvent {
  requestId: ProviderRequestId;
  kind: CanonicalRequestKind;
  title: string;
  detail: string | null;
  allowedDecisions: ApprovalDecisionOption[];
  safePayload: VersionedJson;
  subject: ApprovalSubject | null;
  sessionGrants: SessionPermissionGrant[];
}

export interface RequestResolvedEvent {
  requestId: ProviderRequestId;
  state: RequestResolutionState;
  decision: ApprovalDecision | null;
}

export interface UserInputOption {
  id: string;
  label: string;
  description: string | null;
}

export interface UserInputQuestion {
  id: string;
  header: string | null;
  question: string;
  options: UserInputOption[];
  multiple: boolean;
  freeFormAllowed: boolean;
  required: boolean;
}

export interface UserInputRequestedEvent {
  requestId: ProviderRequestId;
  questions: UserInputQuestion[];
}

export interface UserInputResolvedEvent {
  requestId: ProviderRequestId;
  state: RequestResolutionState;
  answers: UserInputAnswer[];
}

export interface TaskLifecycleEvent {
  taskId: string;
  parentTaskId: string | null;
  status: ActivityStatus;
  title: string;
  detail: string | null;
  safeMetadata: VersionedJson | null;
}

export interface HookLifecycleEvent {
  hookId: string;
  status: ActivityStatus;
  title: string;
  detail: string | null;
}

export interface ToolProgressEvent {
  toolId: string;
  status: ActivityStatus;
  title: string;
  progress: number | null;
  summary: string | null;
}

export interface AuthenticationStatusEvent {
  authenticated: boolean;
  accountLabel: string | null;
  actionRequired: boolean;
  detail: string | null;
}

export interface AccountStatusEvent {
  accountLabel: string | null;
  planLabel: string | null;
  usage: VersionedJson | null;
}

export interface RateLimitStatusEvent {
  limited: boolean;
  resetsAt: UtcTimestamp | null;
  detail: string | null;
  providerData: VersionedJson | null;
}

export interface McpStatusEvent {
  serverId: string;
  status: ActivityStatus;
  detail: string | null;
}

export interface McpOauthCompletedEvent {
  serverId: string;
  successful: boolean;
  detail: string | null;
}

export interface ModelReroutedEvent {
  requestedModelId: ModelId;
  effectiveModelId: ModelId;
  reason: string | null;
}

export interface NotificationEvent {
  code: string;
  title: string;
  detail: string | null;
}

export interface FilesPersistedEvent {
  relativePaths: string[];
}

export interface RuntimeErrorEvent {
  code: string;
  message: string;
  recoverable: boolean;
  safeDetails: VersionedJson | null;
}

export interface UnknownEvent {
  sourceType: string;
  summary: string;
  safePayload: VersionedJson | null;
}

export type CanonicalEvent =
  | { type: "session_started"; payload: SessionStartedEvent }
  | { type: "session_configured"; payload: SessionConfiguredEvent }
  | { type: "session_state_changed"; payload: SessionStateChangedEvent }
  | { type: "session_exited"; payload: SessionExitedEvent }
  | { type: "thread_started"; payload: ThreadStartedEvent }
  | { type: "thread_state_changed"; payload: ThreadStateChangedEvent }
  | { type: "thread_metadata_updated"; payload: ThreadMetadataUpdatedEvent }
  | { type: "thread_reverted"; payload: ThreadRevertedEvent }
  | { type: "thread_usage_updated"; payload: ThreadUsageUpdatedEvent }
  | { type: "turn_started"; payload: TurnStartedEvent }
  | { type: "turn_completed"; payload: TurnCompletedEvent }
  | { type: "turn_aborted"; payload: TurnAbortedEvent }
  | { type: "plan_updated"; payload: PlanUpdatedEvent }
  | { type: "proposed_plan_delta"; payload: ProposedPlanDeltaEvent }
  | { type: "proposed_plan_completed"; payload: ProposedPlanCompletedEvent }
  | { type: "diff_updated"; payload: DiffUpdatedEvent }
  | { type: "item_started"; payload: ItemLifecycleEvent }
  | { type: "item_updated"; payload: ItemLifecycleEvent }
  | { type: "item_completed"; payload: ItemLifecycleEvent }
  | { type: "content_delta"; payload: ContentDeltaEvent }
  | { type: "request_opened"; payload: RequestOpenedEvent }
  | { type: "request_resolved"; payload: RequestResolvedEvent }
  | { type: "user_input_requested"; payload: UserInputRequestedEvent }
  | { type: "user_input_resolved"; payload: UserInputResolvedEvent }
  | { type: "task_lifecycle"; payload: TaskLifecycleEvent }
  | { type: "hook_lifecycle"; payload: HookLifecycleEvent }
  | { type: "tool_progress"; payload: ToolProgressEvent }
  | { type: "authentication_status"; payload: AuthenticationStatusEvent }
  | { type: "account_status"; payload: AccountStatusEvent }
  | { type: "rate_limit_status"; payload: RateLimitStatusEvent }
  | { type: "mcp_status"; payload: McpStatusEvent }
  | { type: "mcp_oauth_completed"; payload: McpOauthCompletedEvent }
  | { type: "model_rerouted"; payload: ModelReroutedEvent }
  | { type: "configuration_warning"; payload: NotificationEvent }
  | { type: "deprecation_notice"; payload: NotificationEvent }
  | { type: "files_persisted"; payload: FilesPersistedEvent }
  | { type: "runtime_warning"; payload: NotificationEvent }
  | { type: "runtime_error"; payload: RuntimeErrorEvent }
  | { type: "unknown"; payload: UnknownEvent };
