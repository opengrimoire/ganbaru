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
  ThreadUsageUpdatedEvent,
  TurnModeSnapshot,
  UtcTimestamp,
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
  /** The detail text this window holds: the whole detail, or a shortened preview when `detailTruncated`. */
  detailPreview: string | null;
  /** The UTF-8 length of the whole detail. */
  detailBytes: number;
  /** Whether `detailPreview` is shorter than the detail, so the whole detail loads when it is opened. */
  detailTruncated: boolean;
  /** When the stored activity last changed, for rows read from the projection. */
  updatedAt?: UtcTimestamp;
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

/** Timeline rows and turn summaries projected from one window of native reads. */
export interface TimelineProjection {
  rows: TimelineRow[];
  turns: TimelineTurn[];
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
