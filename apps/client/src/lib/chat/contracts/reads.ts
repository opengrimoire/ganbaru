import type {
  ChatActivityId,
  ChatErrorCode,
  ChatExecutionEnvironmentId,
  ChatScratchGenerationId,
  ChatThreadId,
  ChatThreadState,
  ChatTurnId,
  ChatTurnState,
  ChatWorkAssignmentId,
  ProjectWorkingFolderId,
  ContinuationGroupId,
  ModelId,
  ProviderFamilyId,
  ProviderInstanceId,
  ProviderItemId,
  ProviderSessionId,
  ProviderSessionState,
  ProviderThreadId,
  ProviderTurnId,
  TurnModeSnapshot,
  UtcTimestamp,
  VersionedJson,
} from "./common";
import type { ChangedFileSummary, ThreadUsageUpdatedEvent } from "./events";
import type { ModelOptionSelection, ProviderCapabilities } from "./provider";

export interface ProviderSessionSnapshot {
  sessionId: ProviderSessionId;
  state: ProviderSessionState;
  providerThreadId: ProviderThreadId | null;
  continuationGroupId: ContinuationGroupId;
  resumeCursor: VersionedJson | null;
  effectiveModes: TurnModeSnapshot;
  capabilities: ProviderCapabilities;
  startedAt: UtcTimestamp;
}

export interface TurnDispatchReceipt {
  turnId: ChatTurnId;
  state: ChatTurnState;
  providerTurnId: ProviderTurnId | null;
  acceptedAt: UtcTimestamp;
}

export interface DriverOperationReceipt {
  accepted: boolean;
  operationId: string;
  detail: string | null;
}

export interface ProviderHistoryItem {
  providerItemId: ProviderItemId | null;
  providerTurnId: ProviderTurnId | null;
  kind: string;
  data: VersionedJson;
}

export interface ProviderHistoryPage {
  items: ProviderHistoryItem[];
  nextCursor: string | null;
}

export interface ChatThreadShellRead {
  id: ChatThreadId;
  workingFolderId: ProjectWorkingFolderId | null;
  executionEnvironmentId: ChatExecutionEnvironmentId;
  scratchGenerationId: ChatScratchGenerationId | null;
  projectId: string;
  title: string;
  providerFamilyId: ProviderFamilyId;
  providerInstanceId: ProviderInstanceId;
  providerThreadId: ProviderThreadId | null;
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
  modes: TurnModeSnapshot;
  state: ChatThreadState;
  latestTurnState: ChatTurnState | null;
  latestPreview: string | null;
  messageCount: number;
  revision: number;
  lastEventSequence: number;
  lastActivityAt: UtcTimestamp;
  unreadAt: UtcTimestamp | null;
  archivedAt: UtcTimestamp | null;
}

export interface ChatProjectShellRead {
  projectId: string;
  workingFolderId: ProjectWorkingFolderId;
  workingFolderName: string;
  workingFolderArchivedAt: UtcTimestamp | null;
  activeThreadCount: number;
  archivedThreadCount: number;
}

/**
 * Projections a change notification can name. `unknown` stands for a key this build does not
 * recognize, which readers treat as a change to everything.
 */
export const CHAT_PROJECTION_KEYS = [
  "thread",
  "turns",
  "messages",
  "activities",
  "plans",
  "pending_requests",
  "organizational",
  "unknown",
] as const;
export type ChatProjectionKey = (typeof CHAT_PROJECTION_KEYS)[number];

export interface ChatChangeNotification {
  threadId: ChatThreadId;
  /** The newest event sequence the notification covers. */
  sequence: number;
  /** The first event sequence the notification covers; equal to `sequence` for one event. */
  sinceSequence: number;
  revision: number;
  changedProjectionKeys: ChatProjectionKey[];
}

export interface ChatTimelineItemRead {
  activityId: ChatActivityId;
  turnId: ChatTurnId | null;
  sequenceAnchor: number;
  /** The thread change clock value of the row's last write. */
  changeSequence: number;
  kind: string;
  data: VersionedJson;
  sourceThreadId?: ChatThreadId;
}

export interface ChatTimelineTurnRead {
  turnId: ChatTurnId;
  state: ChatTurnState;
  startedAt: UtcTimestamp | null;
  completedAt: UtcTimestamp | null;
  stopReason: string | null;
  /** Code of the error that failed the turn, when it failed with a stored structured error; `stopReason` keeps the diagnostic text. */
  errorCode: ChatErrorCode | null;
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
  modes: TurnModeSnapshot;
  usage: ThreadUsageUpdatedEvent | null;
  changedFiles: ChangedFileSummary[];
}

export interface ChatTimelinePageRead {
  threadId: ChatThreadId;
  items: ChatTimelineItemRead[];
  turns: ChatTimelineTurnRead[];
  previousCursor: string | null;
  nextCursor: string | null;
  threadRevision: number;
  /** The thread change clock the rows were read at. */
  changeCursor: number;
}

/** Timeline rows that changed after a change clock value. */
export interface ChatTimelineDeltaRead {
  threadId: ChatThreadId;
  items: ChatTimelineItemRead[];
  turns: ChatTimelineTurnRead[];
  /** Rows deleted or hidden by an invalidated turn and not visible again. */
  removedItemIds: ChatActivityId[];
  changeCursor: number;
  /** More changes follow `changeCursor` than one read returns. */
  hasMore: boolean;
  /** The reader must reload the timeline because older removals were pruned. */
  resetRequired: boolean;
  threadRevision: number;
}

/**
 * The live execution of one work assignment, read by a phone from the linked desktop that runs it. Rows carry only what
 * the read-only timeline shows, with long text shortened.
 */
export interface ChatLiveExecutionRead {
  assignmentId: ChatWorkAssignmentId;
  /** The thread the newest run writes, once a run started. */
  threadId: ChatThreadId | null;
  /** The turn of the newest run, once a run started. */
  turnId: ChatTurnId | null;
  providerFamilyId: ProviderFamilyId | null;
  modelId: ModelId | null;
  /** Rows of the run's turn written after the cursor, ordered by change. */
  items: ChatTimelineItemRead[];
  turns: ChatTimelineTurnRead[];
  /** Rows of the run's turn deleted or hidden after the cursor. */
  removedItemIds: ChatActivityId[];
  changeCursor: number;
  /** More changes follow `changeCursor` than one read returns. */
  hasMore: boolean;
  /** The cursor no longer applies, so the reader reads again from the start. */
  resetRequired: boolean;
  /** The run ended or the work left its active states, so no further changes follow. */
  settled: boolean;
}

/** What a phone can show of the live execution of work a linked desktop runs. */
export type ChatLiveFollowRead =
  | { kind: "page"; read: ChatLiveExecutionRead }
  /** The executing desktop cannot be reached now, so the synced work row stands in and the read is tried again. */
  | { kind: "unreachable" }
  /** No other device serves this execution, so the synced work row stands in. */
  | { kind: "notServed" };

/** The whole detail of one activity, read when its disclosure opens. */
export interface ChatActivityDetailRead {
  activityId: ChatActivityId;
  detail: string | null;
  updatedAt: UtcTimestamp;
}
