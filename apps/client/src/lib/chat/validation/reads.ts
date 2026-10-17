import {
  CHAT_ERROR_CODES,
  CHAT_THREAD_STATES,
  CHAT_TURN_STATES,
  type ChatChangeNotification,
  type ChatError,
  type ChatThreadShellRead,
  type ChatTimelineItemRead,
  type ChatTimelinePageRead,
  type ChatTimelineTurnRead,
  type ProviderHistoryItem,
  type TurnDispatchReceipt,
} from "../contracts";
import { normalizeChangedFileSummaries } from "$lib/chat/review/changed-files";
import { parseChangedFile, parseThreadUsage } from "./events";
import { parseModelOptionSelection } from "./provider";
import {
  readArray,
  readBoolean,
  readEnum,
  readIdentifier,
  readJsonValue,
  readNullable,
  readNonNegativeSafeInteger,
  readRecord,
  readString,
  readStringArray,
  readTurnModeSnapshot,
  readUtcTimestamp,
  readVersionedJson,
} from "./readers";

export function parseChatError(value: unknown, label = "Chat error"): ChatError {
  const record = readRecord(value, label);
  return {
    code: readEnum(record.code, CHAT_ERROR_CODES, `${label}.code`),
    message: readString(record.message, `${label}.message`),
    field: readNullable(record.field, `${label}.field`, readString),
    recoverable: readBoolean(record.recoverable, `${label}.recoverable`),
    details: readNullable(record.details, `${label}.details`, readJsonValue),
  };
}

export function parseChatThreadShells(value: unknown, label = "Chat thread shells"): ChatThreadShellRead[] {
  return readArray(value, label, parseChatThreadShell);
}

export function parseTurnDispatchReceipt(value: unknown, label = "turn receipt"): TurnDispatchReceipt {
  const record = readRecord(value, label);
  return {
    turnId: readIdentifier(record.turnId, `${label}.turnId`),
    state: readEnum(record.state, CHAT_TURN_STATES, `${label}.state`),
    providerTurnId: readNullable(record.providerTurnId, `${label}.providerTurnId`, readIdentifier),
    acceptedAt: readUtcTimestamp(record.acceptedAt, `${label}.acceptedAt`),
  };
}

export function parseChatThreadShell(value: unknown, label = "Chat thread shell"): ChatThreadShellRead {
  const record = readRecord(value, label);
  const workingFolderId = readNullable(record.workingFolderId, `${label}.workingFolderId`, readIdentifier);
  const scratchGenerationId = readNullable(
    record.scratchGenerationId,
    `${label}.scratchGenerationId`,
    readIdentifier,
  );
  if ((workingFolderId === null) === (scratchGenerationId === null)) {
    throw new Error(`${label} must identify exactly one working folder or scratch generation`);
  }
  return {
    id: readIdentifier(record.id, `${label}.id`),
    workingFolderId,
    executionEnvironmentId: readIdentifier(
      record.executionEnvironmentId,
      `${label}.executionEnvironmentId`,
    ),
    scratchGenerationId,
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    title: readString(record.title, `${label}.title`),
    providerFamilyId: readIdentifier(record.providerFamilyId, `${label}.providerFamilyId`),
    providerInstanceId: readIdentifier(record.providerInstanceId, `${label}.providerInstanceId`),
    providerThreadId: readNullable(record.providerThreadId, `${label}.providerThreadId`, readIdentifier),
    modelId: readNullable(record.modelId, `${label}.modelId`, readIdentifier),
    modelOptions: readArray(record.modelOptions, `${label}.modelOptions`, parseModelOptionSelection),
    modes: readTurnModeSnapshot(record.modes, `${label}.modes`),
    state: readEnum(record.state, CHAT_THREAD_STATES, `${label}.state`),
    latestTurnState: readNullable(
      record.latestTurnState,
      `${label}.latestTurnState`,
      (entry, entryLabel) => readEnum(entry, CHAT_TURN_STATES, entryLabel),
    ),
    latestPreview: readNullable(record.latestPreview, `${label}.latestPreview`, readString),
    messageCount: readNonNegativeSafeInteger(record.messageCount, `${label}.messageCount`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    lastEventSequence: readNonNegativeSafeInteger(record.lastEventSequence, `${label}.lastEventSequence`),
    lastActivityAt: readUtcTimestamp(record.lastActivityAt, `${label}.lastActivityAt`),
    unreadAt: readNullable(record.unreadAt, `${label}.unreadAt`, readUtcTimestamp),
    archivedAt: readNullable(record.archivedAt, `${label}.archivedAt`, readUtcTimestamp),
  };
}

export function parseChatChangeNotification(value: unknown, label = "Chat change notification"): ChatChangeNotification {
  const record = readRecord(value, label);
  return {
    threadId: readIdentifier(record.threadId, `${label}.threadId`),
    sequence: readNonNegativeSafeInteger(record.sequence, `${label}.sequence`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    changedProjectionKeys: readStringArray(record.changedProjectionKeys, `${label}.changedProjectionKeys`),
  };
}

export function parseChatTimelineItem(value: unknown, label: string): ChatTimelineItemRead {
  const record = readRecord(value, label);
  return {
    activityId: readIdentifier(record.activityId, `${label}.activityId`),
    turnId: readNullable(record.turnId, `${label}.turnId`, readIdentifier),
    sequenceAnchor: readNonNegativeSafeInteger(record.sequenceAnchor, `${label}.sequenceAnchor`),
    kind: readString(record.kind, `${label}.kind`),
    data: readVersionedJson(record.data, `${label}.data`),
    sourceThreadId: record.sourceThreadId === undefined
      ? undefined
      : readIdentifier(record.sourceThreadId, `${label}.sourceThreadId`),
  };
}

export function parseChatTimelineTurn(value: unknown, label: string): ChatTimelineTurnRead {
  const record = readRecord(value, label);
  return {
    turnId: readIdentifier(record.turnId, `${label}.turnId`),
    state: readEnum(record.state, CHAT_TURN_STATES, `${label}.state`),
    startedAt: readNullable(record.startedAt, `${label}.startedAt`, readUtcTimestamp),
    completedAt: readNullable(record.completedAt, `${label}.completedAt`, readUtcTimestamp),
    stopReason: readNullable(record.stopReason, `${label}.stopReason`, readString),
    modelId: readNullable(record.modelId, `${label}.modelId`, readIdentifier),
    modelOptions: readArray(record.modelOptions, `${label}.modelOptions`, parseModelOptionSelection),
    modes: readTurnModeSnapshot(record.modes, `${label}.modes`),
    usage: readNullable(record.usage, `${label}.usage`, parseThreadUsage),
    changedFiles: normalizeChangedFileSummaries(
      readArray(record.changedFiles, `${label}.changedFiles`, parseChangedFile),
    ),
  };
}

export function parseChatTimelinePage(value: unknown, label = "Chat timeline page"): ChatTimelinePageRead {
  const record = readRecord(value, label);
  return {
    threadId: readIdentifier(record.threadId, `${label}.threadId`),
    items: readArray(record.items, `${label}.items`, parseChatTimelineItem),
    turns: readArray(record.turns, `${label}.turns`, parseChatTimelineTurn),
    previousCursor: readNullable(record.previousCursor, `${label}.previousCursor`, readString),
    nextCursor: readNullable(record.nextCursor, `${label}.nextCursor`, readString),
    threadRevision: readNonNegativeSafeInteger(record.threadRevision, `${label}.threadRevision`),
  };
}
