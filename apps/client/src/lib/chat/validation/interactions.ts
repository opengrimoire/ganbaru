import type {
  ChatAttachmentRead,
  ChatInteractionStateRead,
  ChatPendingRequestRead,
  ChatPromptCatalogEntry,
  ChatQueuedFollowupRead,
  McpServerStatusRead,
  McpStatusRead,
  ProjectWorkingFolderPathPage,
  ProjectWorkingFolderPathRead,
  ChatUserInputDraftRead,
} from "../contracts";
import { INTERACTION_MODES, PROVIDER_SESSION_STATES, SAFETY_MODES } from "../contracts";
import { parseProviderCapabilities } from "./provider";
import { parseAccountStatus, parseRateLimitStatus, parseThreadUsage } from "./events";
import { parseChatRememberableApproval } from "./remembered-approvals";
import {
  readArray,
  readBoolean,
  readEnum,
  readIdentifier,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readString,
  readUtcTimestamp,
  readVersionedJson,
} from "./readers";

const ATTACHMENT_KINDS = ["image", "text_snippet"] as const;
const PATH_KINDS = ["file", "directory"] as const;
const CATALOG_KINDS = ["skill", "command"] as const;
const CATALOG_SOURCES = ["app", "provider", "workspace", "user"] as const;
const REQUEST_KINDS = ["approval", "user_input"] as const;

export function parseChatAttachmentRead(value: unknown, label = "Chat attachment"): ChatAttachmentRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
    kind: readEnum(record.kind, ATTACHMENT_KINDS, `${label}.kind`),
    originalDisplayName: readString(record.originalDisplayName, `${label}.originalDisplayName`),
    mimeType: readString(record.mimeType, `${label}.mimeType`),
    byteSize: readNonNegativeSafeInteger(record.byteSize, `${label}.byteSize`),
    sha256: readString(record.sha256, `${label}.sha256`),
    managedRelativePath: readString(record.managedRelativePath, `${label}.managedRelativePath`),
    signatureKind: readString(record.signatureKind, `${label}.signatureKind`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
  };
}

function parseWorkspacePath(value: unknown, label: string): ProjectWorkingFolderPathRead {
  const record = readRecord(value, label);
  return {
    relativePath: readString(record.relativePath, `${label}.relativePath`),
    displayName: readString(record.displayName, `${label}.displayName`),
    kind: readEnum(record.kind, PATH_KINDS, `${label}.kind`),
    ignored: readBoolean(record.ignored, `${label}.ignored`),
  };
}

export function parseProjectWorkingFolderPathPage(value: unknown): ProjectWorkingFolderPathPage {
  const record = readRecord(value, "workspace path page");
  return {
    entries: readArray(record.entries, "workspace path page.entries", parseWorkspacePath),
    nextCursor: readNullable(record.nextCursor, "workspace path page.nextCursor", readString),
  };
}

export function parseChatPromptCatalog(value: unknown): ChatPromptCatalogEntry[] {
  return readArray(value, "prompt catalog", (entry, label) => {
    const record = readRecord(entry, label);
    return {
      value: readString(record.value, `${label}.value`),
      label: readString(record.label, `${label}.label`),
      description: readNullable(record.description, `${label}.description`, readString),
      argumentHint: readNullable(record.argumentHint, `${label}.argumentHint`, readString),
      kind: readEnum(record.kind, CATALOG_KINDS, `${label}.kind`),
      source: readEnum(record.source, CATALOG_SOURCES, `${label}.source`),
      stale: readBoolean(record.stale, `${label}.stale`),
    };
  });
}

function parsePendingRequest(value: unknown, label: string): ChatPendingRequestRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    turnId: readNullable(record.turnId, `${label}.turnId`, readIdentifier),
    providerRequestId: readIdentifier(record.providerRequestId, `${label}.providerRequestId`),
    requestKind: readEnum(record.requestKind, REQUEST_KINDS, `${label}.requestKind`),
    safeDisplay: readVersionedJson(record.safeDisplay, `${label}.safeDisplay`),
    allowedDecisions: readVersionedJson(record.allowedDecisions, `${label}.allowedDecisions`),
    openedAt: readUtcTimestamp(record.openedAt, `${label}.openedAt`),
    rememberable: readNullable(record.rememberable, `${label}.rememberable`, parseChatRememberableApproval),
  };
}

function parseQueuedFollowup(value: unknown, label: string): ChatQueuedFollowupRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    threadId: readIdentifier(record.threadId, `${label}.threadId`),
    text: readString(record.text, `${label}.text`),
    providerInstanceId: readIdentifier(record.providerInstanceId, `${label}.providerInstanceId`),
    modelSelection: readVersionedJson(record.modelSelection, `${label}.modelSelection`),
    safetyMode: readEnum(record.safetyMode, SAFETY_MODES, `${label}.safetyMode`),
    interactionMode: readEnum(record.interactionMode, INTERACTION_MODES, `${label}.interactionMode`),
    attachmentIds: readArray(record.attachmentIds, `${label}.attachmentIds`, readIdentifier),
    mentions: readVersionedJson(record.mentions, `${label}.mentions`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

export function parseChatInteractionState(value: unknown): ChatInteractionStateRead {
  const record = readRecord(value, "Chat interaction state");
  return {
    sessionId: readNullable(record.sessionId, "Chat interaction state.sessionId", readIdentifier),
    sessionState: readEnum(record.sessionState, PROVIDER_SESSION_STATES, "Chat interaction state.sessionState"),
    activeTurnId: readNullable(record.activeTurnId, "Chat interaction state.activeTurnId", readIdentifier),
    capabilities: parseProviderCapabilities(record.capabilities, "Chat interaction state.capabilities"),
    pendingRequests: readArray(record.pendingRequests, "Chat interaction state.pendingRequests", parsePendingRequest),
    queuedFollowup: readNullable(record.queuedFollowup, "Chat interaction state.queuedFollowup", parseQueuedFollowup),
    usage: readNullable(record.usage, "Chat interaction state.usage", parseThreadUsage),
    accountStatus: readNullable(record.accountStatus, "Chat interaction state.accountStatus", parseAccountStatus),
    rateLimitStatus: readNullable(record.rateLimitStatus, "Chat interaction state.rateLimitStatus", parseRateLimitStatus),
    automaticCompactionReported: readBoolean(record.automaticCompactionReported, "Chat interaction state.automaticCompactionReported"),
  };
}

function parseMcpServerStatus(value: unknown, label: string): McpServerStatusRead {
  const record = readRecord(value, label);
  return {
    name: readString(record.name, `${label}.name`),
    authStatus: readNullable(record.authStatus, `${label}.authStatus`, readString),
    enabled: readBoolean(record.enabled, `${label}.enabled`),
    runtimeStatus: readNullable(record.runtimeStatus, `${label}.runtimeStatus`, readString),
  };
}

export function parseMcpStatus(value: unknown): McpStatusRead {
  const record = readRecord(value, "MCP status");
  return {
    servers: readArray(record.servers, "MCP status.servers", parseMcpServerStatus),
  };
}

export function parseChatQueuedFollowup(value: unknown): ChatQueuedFollowupRead {
  return parseQueuedFollowup(value, "queued follow-up");
}

export function parseChatUserInputDraft(value: unknown): ChatUserInputDraftRead {
  const record = readRecord(value, "Chat user input draft");
  return {
    requestId: readIdentifier(record.requestId, "Chat user input draft.requestId"),
    answers: readVersionedJson(record.answers, "Chat user input draft.answers"),
    updatedAt: readUtcTimestamp(record.updatedAt, "Chat user input draft.updatedAt"),
  };
}
