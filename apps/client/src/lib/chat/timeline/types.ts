import type {
  ActivityStatus,
  CanonicalItemKind,
  ChangedFileSummary,
  ChatThreadId,
  ChatTurnId,
  ChatTurnState,
  ContentStreamKind,
  ModelId,
  PlanStep,
  RequestResolutionState,
  ThreadUsageUpdatedEvent,
  TurnModeSnapshot,
  UtcTimestamp,
  UserInputQuestion,
  VersionedJson,
} from "$lib/chat/contracts";

export type TimelineMessageState = "pending" | "streaming" | "complete" | "interrupted" | "failed";
export type TimelineMessagePhase = "commentary" | "final_answer";

export interface TimelineAssistantMetadata {
  durationMs: number | null;
  modelId: ModelId | null;
  usage: ThreadUsageUpdatedEvent | null;
  changedFiles: ChangedFileSummary[];
  diffSources: string[];
}

export interface TimelineAttachmentSummary {
  attachmentId: string;
  displayName: string;
  kind: string;
  byteSize: number;
  status: string;
}

export interface TimelineMentionSummary {
  relativePath: string;
  kind: string | null;
}

export interface TimelineUserContext {
  attachments: TimelineAttachmentSummary[];
  mentions: TimelineMentionSummary[];
  terminalContext: string[];
  preCheckpointId: string | null;
}

export interface TimelineMessageRow {
  id: string;
  kind: "message";
  role: "user" | "assistant";
  turnId: ChatTurnId | null;
  sequence: number;
  createdAt: UtcTimestamp;
  markdown: string;
  state: TimelineMessageState;
  phase: TimelineMessagePhase | null;
  userContext: TimelineUserContext | null;
  metadata: TimelineAssistantMetadata | null;
  sourceThreadId?: ChatThreadId;
}

export interface TimelineActivityRow {
  id: string;
  kind: "activity";
  turnId: ChatTurnId | null;
  sequence: number;
  createdAt: UtcTimestamp;
  activityKind: CanonicalItemKind | ContentStreamKind | "task" | "hook" | "tool" | "mcp" | "notice" | "channel_session_boundary";
  status: ActivityStatus;
  title: string;
  detail: string | null;
  metadata: VersionedJson | null;
  sourceThreadId?: ChatThreadId;
}

export interface TimelinePlanRow {
  id: string;
  kind: "plan";
  turnId: ChatTurnId | null;
  sequence: number;
  createdAt: UtcTimestamp;
  planKind: "structured" | "proposed";
  markdown: string;
  steps: PlanStep[];
  state: "streaming" | "complete";
  sourceThreadId?: ChatThreadId;
}

export type TimelineRow = TimelineMessageRow | TimelineActivityRow | TimelinePlanRow;

export interface TimelinePendingRequest {
  id: string;
  turnId: ChatTurnId | null;
  sequence: number;
  createdAt: UtcTimestamp;
  kind: "approval" | "user_input";
  title: string;
  detail: string | null;
  questions: UserInputQuestion[];
  state: RequestResolutionState;
}

export interface TimelineTurn {
  id: ChatTurnId;
  state: ChatTurnState;
  startedAt: UtcTimestamp;
  completedAt: UtcTimestamp | null;
  durationMs: number | null;
  modes: TurnModeSnapshot;
  modelId: ModelId | null;
  effectiveModelId: ModelId | null;
  usage: ThreadUsageUpdatedEvent | null;
  changedFiles: ChangedFileSummary[];
  diffSources: string[];
  stopReason: string | null;
  recoverable: boolean;
}

export interface TimelineProjection {
  rows: TimelineRow[];
  turns: TimelineTurn[];
  pendingRequests: TimelinePendingRequest[];
  threadUsage: ThreadUsageUpdatedEvent | null;
  ignoredDuplicateEventIds: string[];
}

export interface TimelineActivityGroupRow {
  id: string;
  kind: "activity_group";
  turnId: ChatTurnId | null;
  sequence: number;
  createdAt: UtcTimestamp;
  latest: TimelineActivityRow;
  earlierRows: TimelineActivityRow[];
  expanded: boolean;
  sourceThreadId?: ChatThreadId;
}

export interface TimelineTurnFoldRow {
  id: string;
  kind: "turn_fold";
  turnId: ChatTurnId;
  sequence: number;
  createdAt: UtcTimestamp;
  state: "completed" | "interrupted" | "failed";
  durationMs: number | null;
  hiddenRows: (TimelineActivityRow | TimelineMessageRow | TimelineActivityGroupRow)[];
  expanded: boolean;
  sourceThreadId?: ChatThreadId;
}

export type TimelineDisplayRow = TimelineRow | TimelineTurnFoldRow | TimelineActivityGroupRow;
