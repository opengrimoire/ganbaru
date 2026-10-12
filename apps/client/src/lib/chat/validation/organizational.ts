import {
  CHAT_ACCESS_ISSUE_CODES,
  CHAT_ACCESS_PROFILE_BUILTIN_KEYS,
  CHAT_ASSIGNMENT_TARGET_BINDING_STATES,
  CHAT_ASSIGNMENT_TARGET_KINDS,
  CHAT_ASSIGNMENT_TARGET_LIFECYCLE_STATES,
  CHAT_FOLDER_CAPABILITIES,
  CHAT_PARTICIPANT_KINDS,
  CHAT_RUNTIME_APPROVAL_POLICIES,
  CHAT_SCRATCH_CLEANUP_JOB_STATES,
  CHAT_SCRATCH_DEVICE_AVAILABILITIES,
  CHAT_SCRATCH_DIRECTORY_ENTRY_KINDS,
  CHAT_SCRATCH_GENERATION_LIFECYCLE_STATES,
  CHAT_SCRATCH_SCOPE_LIFECYCLE_STATES,
  CHAT_SCHEDULED_MESSAGE_STATES,
  CHAT_TEAMMATE_CONFIGURATION_STATES,
  CHAT_WORK_ASSIGNMENT_STATES,
  SAFETY_MODES,
  type ChatAgentRunRead,
  type ChatAccessProfileRead,
  type ChatAccessProfileImpactPreviewRead,
  type ChatAccessProfileRevision,
  type ChatAiTeammateRead,
  type ChatAssignmentTargetRead,
  type ChatChannelPageRead,
  type ChatChannelMembershipRemovalPreview,
  type ChatChannelRosterAiSummary,
  type ChatChannelRosterRead,
  type ChatConversationMembershipRead,
  type ChatFolderGrant,
  type ChatExecutionTarget,
  type ChatHistoryBoundary,
  type ChatMessageRead,
  type ChatMessageReference,
  type ChatMessageSearchResultRead,
  type ChatParticipantRead,
  type ChatProjectPrimaryWorkingFolderRead,
  type ChatReplyThreadPageRead,
  type ChatReplyThreadSummaryRead,
  type ChatScheduledMessageDispatchRead,
  type ChatScheduledMessageRead,
  type ChatScratchCleanupPreviewRead,
  type ChatScratchCleanupResultRead,
  type ChatScratchDirectoryEntryRead,
  type ChatScratchDirectoryPageRead,
  type ChatScratchGenerationRead,
  type ChatScratchPromotionDestinationRead,
  type ChatScratchPromotionResultRead,
  type ChatScratchScopeRead,
  type ChatScratchSourceSummaryRead,
  type ChatTeammatePolicyRead,
  type ChatTeammateAccessPreviewRead,
  type ChatTeammateAccessPreviewIssue,
  type ChatTeammateAccessRead,
  type ChatTeammateChannelAccess,
  type ChatWorkAssignmentRead,
  type PostChatMessageResult,
} from "../contracts";
import { parseModelOptionSelection } from "./provider";
import { normalizeChatMessageReferences } from "$lib/chat/composer/message-references";
import {
  readArray,
  readBoolean,
  readEnum,
  readIdentifier,
  readNullable,
  readNonNegativeSafeInteger,
  readRecord,
  readString,
  readUtcTimestamp,
  readVersionedJson,
} from "./readers";

const AGENT_RUN_STATES = [
  "queued", "starting", "working", "waiting", "completed", "failed", "cancelled",
] as const;
const MAX_SCRATCH_DIRECTORY_PAGE_ENTRIES = 50;
const MAX_SCRATCH_RELATIVE_PATH_BYTES = 4_096;

function readScratchRelativePath(value: unknown, label: string, allowEmpty = false): string {
  const path = readString(value, label);
  if (allowEmpty && path.length === 0) return path;
  if (
    path.length === 0
    || new TextEncoder().encode(path).byteLength > MAX_SCRATCH_RELATIVE_PATH_BYTES
    || path.startsWith("/")
    || path.includes("\\")
    || path.split("/").some((segment) => segment === "." || segment === "..")
    || [...path].some((character) => {
      const point = character.codePointAt(0) ?? 0;
      return point <= 0x1f || (point >= 0x7f && point <= 0x9f);
    })
  ) {
    throw new Error(`${label} must be a bounded relative path`);
  }
  return path;
}

function readSha256(value: unknown, label: string): string {
  const digest = readString(value, label);
  if (!/^[0-9a-f]{64}$/i.test(digest)) throw new Error(`${label} must be a SHA-256 digest`);
  return digest;
}

export function parseChatParticipant(value: unknown, label = "Chat participant"): ChatParticipantRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    kind: readEnum(record.kind, CHAT_PARTICIPANT_KINDS, `${label}.kind`),
    displayName: readString(record.displayName, `${label}.displayName`),
    avatar: readVersionedJson(record.avatar, `${label}.avatar`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    archivedAt: readNullable(record.archivedAt, `${label}.archivedAt`, readUtcTimestamp),
  };
}

export function parseChatTeammatePolicy(value: unknown, label = "Chat teammate policy"): ChatTeammatePolicyRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    teammateId: readIdentifier(record.teammateId, `${label}.teammateId`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    providerInstanceId: readIdentifier(record.providerInstanceId, `${label}.providerInstanceId`),
    safetyMode: readEnum(record.safetyMode, SAFETY_MODES, `${label}.safetyMode`),
    providerManagedModel: readBoolean(record.providerManagedModel, `${label}.providerManagedModel`),
    modelId: readNullable(record.modelId, `${label}.modelId`, readIdentifier),
    modelOptions: readArray(record.modelOptions, `${label}.modelOptions`, parseModelOptionSelection),
    effort: readNullable(record.effort, `${label}.effort`, readString),
    speed: readNullable(record.speed, `${label}.speed`, readString),
    providerOptions: readVersionedJson(record.providerOptions, `${label}.providerOptions`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
  };
}

export function parseChatAiTeammate(value: unknown, label = "Chat AI teammate"): ChatAiTeammateRead {
  const record = readRecord(value, label);
  return {
    participant: parseChatParticipant(record.participant, `${label}.participant`),
    role: readString(record.role, `${label}.role`),
    instructions: readString(record.instructions, `${label}.instructions`),
    configurationState: readEnum(
      record.configurationState,
      CHAT_TEAMMATE_CONFIGURATION_STATES,
      `${label}.configurationState`,
    ),
    latestPolicy: readNullable(record.latestPolicy, `${label}.latestPolicy`, parseChatTeammatePolicy),
    channelCount: readNonNegativeSafeInteger(record.channelCount, `${label}.channelCount`),
    activeAssignmentCount: readNonNegativeSafeInteger(
      record.activeAssignmentCount,
      `${label}.activeAssignmentCount`,
    ),
    hasDurableHistory: readBoolean(record.hasDurableHistory, `${label}.hasDurableHistory`),
  };
}

export function parseChatAiTeammates(value: unknown, label = "Chat AI teammates"): ChatAiTeammateRead[] {
  return readArray(value, label, parseChatAiTeammate);
}

export function parseChatConversationMembership(
  value: unknown,
  label = "Chat conversation membership",
): ChatConversationMembershipRead {
  const record = readRecord(value, label);
  return {
    conversationId: readIdentifier(record.conversationId, `${label}.conversationId`),
    participant: parseChatParticipant(record.participant, `${label}.participant`),
    aiAccess: readNullable(record.aiAccess, `${label}.aiAccess`, parseChatChannelRosterAiSummary),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    removedAt: readNullable(record.removedAt, `${label}.removedAt`, readUtcTimestamp),
  };
}

function parseChatHistoryBoundary(value: unknown, label: string): ChatHistoryBoundary {
  const record = readRecord(value, label);
  const kind = readEnum(record.kind, ["entire", "fromGrant"] as const, `${label}.kind`);
  if (kind === "entire") return { kind };
  return record.lowerOrdinal === undefined
    ? { kind }
    : { kind, lowerOrdinal: readNonNegativeSafeInteger(record.lowerOrdinal, `${label}.lowerOrdinal`) };
}

function parseChatFolderGrant(value: unknown, label: string): ChatFolderGrant {
  const record = readRecord(value, label);
  return {
    workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
    displayName: readString(record.displayName, `${label}.displayName`),
    capability: readEnum(record.capability, CHAT_FOLDER_CAPABILITIES, `${label}.capability`),
    isDefault: readBoolean(record.isDefault, `${label}.isDefault`),
    runtimeApprovalOverride: readNullable(
      record.runtimeApprovalOverride,
      `${label}.runtimeApprovalOverride`,
      (entry, entryLabel) => readEnum(entry, CHAT_RUNTIME_APPROVAL_POLICIES, entryLabel),
    ),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    revokedAt: readNullable(record.revokedAt, `${label}.revokedAt`, readUtcTimestamp),
  };
}

function parseChatChannelRosterAiSummary(
  value: unknown,
  label: string,
): ChatChannelRosterAiSummary {
  const record = readRecord(value, label);
  const capabilities = readRecord(record.capabilities, `${label}.capabilities`);
  return {
    accessProfileId: readIdentifier(record.accessProfileId, `${label}.accessProfileId`),
    accessProfileRevision: readNonNegativeSafeInteger(
      record.accessProfileRevision,
      `${label}.accessProfileRevision`,
    ),
    accessProfileBuiltinKey: readNullable(
      record.accessProfileBuiltinKey,
      `${label}.accessProfileBuiltinKey`,
      (entry, entryLabel) => readEnum(entry, CHAT_ACCESS_PROFILE_BUILTIN_KEYS, entryLabel),
    ),
    accessProfileName: readString(record.accessProfileName, `${label}.accessProfileName`),
    capabilities: {
      readHistory: readBoolean(capabilities.readHistory, `${label}.capabilities.readHistory`),
      participate: readBoolean(capabilities.participate, `${label}.capabilities.participate`),
    },
    historyBoundary: parseChatHistoryBoundary(record.historyBoundary, `${label}.historyBoundary`),
    runtimeApprovalOverride: readNullable(
      record.runtimeApprovalOverride,
      `${label}.runtimeApprovalOverride`,
      (entry, entryLabel) => readEnum(entry, CHAT_RUNTIME_APPROVAL_POLICIES, entryLabel),
    ),
    scratchRuntimeApprovalOverride: readNullable(
      record.scratchRuntimeApprovalOverride,
      `${label}.scratchRuntimeApprovalOverride`,
      (entry, entryLabel) => readEnum(entry, CHAT_RUNTIME_APPROVAL_POLICIES, entryLabel),
    ),
    folderGrants: readArray(record.folderGrants, `${label}.folderGrants`, parseChatFolderGrant),
  };
}

export function parseChatChannelRoster(
  value: unknown,
  label = "Chat channel roster",
): ChatChannelRosterRead {
  const record = readRecord(value, label);
  return {
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    conversationId: readIdentifier(record.conversationId, `${label}.conversationId`),
    audienceRevision: readNonNegativeSafeInteger(record.audienceRevision, `${label}.audienceRevision`),
    memberships: readArray(record.memberships, `${label}.memberships`, parseChatConversationMembership),
  };
}

export function parseChatChannelMembershipRemovalPreview(
  value: unknown,
  label = "Chat channel membership removal preview",
): ChatChannelMembershipRemovalPreview {
  const record = readRecord(value, label);
  return {
    teammateId: readIdentifier(record.teammateId, `${label}.teammateId`),
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    activeAssignmentCount: readNonNegativeSafeInteger(
      record.activeAssignmentCount,
      `${label}.activeAssignmentCount`,
    ),
    activeAuthorizationCount: readNonNegativeSafeInteger(
      record.activeAuthorizationCount,
      `${label}.activeAuthorizationCount`,
    ),
    willRevokeActiveWork: readBoolean(record.willRevokeActiveWork, `${label}.willRevokeActiveWork`),
    proposedAccess: parseChatTeammateAccess(record.proposedAccess, `${label}.proposedAccess`),
  };
}

export function parseChatTeammateChannelAccess(
  value: unknown,
  label = "Chat teammate channel access",
): ChatTeammateChannelAccess {
  const record = readRecord(value, label);
  const capabilities = readRecord(record.capabilities, `${label}.capabilities`);
  return {
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    conversationId: readIdentifier(record.conversationId, `${label}.conversationId`),
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    groupId: readIdentifier(record.groupId, `${label}.groupId`),
    channelName: readString(record.channelName, `${label}.channelName`),
    accessProfileId: readIdentifier(record.accessProfileId, `${label}.accessProfileId`),
    accessProfileRevision: readNonNegativeSafeInteger(
      record.accessProfileRevision,
      `${label}.accessProfileRevision`,
    ),
    capabilities: {
      readHistory: readBoolean(capabilities.readHistory, `${label}.capabilities.readHistory`),
      participate: readBoolean(capabilities.participate, `${label}.capabilities.participate`),
    },
    historyBoundary: parseChatHistoryBoundary(record.historyBoundary, `${label}.historyBoundary`),
    runtimeApprovalOverride: readNullable(
      record.runtimeApprovalOverride,
      `${label}.runtimeApprovalOverride`,
      (entry, entryLabel) => readEnum(entry, CHAT_RUNTIME_APPROVAL_POLICIES, entryLabel),
    ),
    scratchRuntimeApprovalOverride: readNullable(
      record.scratchRuntimeApprovalOverride,
      `${label}.scratchRuntimeApprovalOverride`,
      (entry, entryLabel) => readEnum(entry, CHAT_RUNTIME_APPROVAL_POLICIES, entryLabel),
    ),
    folderGrants: readArray(record.folderGrants, `${label}.folderGrants`, parseChatFolderGrant),
    membershipRevision: readNonNegativeSafeInteger(
      record.membershipRevision,
      `${label}.membershipRevision`,
    ),
    removedAt: readNullable(record.removedAt, `${label}.removedAt`, readUtcTimestamp),
  };
}

export function parseChatTeammateAccess(
  value: unknown,
  label = "Chat teammate access",
): ChatTeammateAccessRead {
  const record = readRecord(value, label);
  return {
    teammateId: readIdentifier(record.teammateId, `${label}.teammateId`),
    accessRevision: readNonNegativeSafeInteger(record.accessRevision, `${label}.accessRevision`),
    teammateDefaultRuntimeApproval: readEnum(
      record.teammateDefaultRuntimeApproval,
      CHAT_RUNTIME_APPROVAL_POLICIES,
      `${label}.teammateDefaultRuntimeApproval`,
    ),
    channels: readArray(record.channels, `${label}.channels`, parseChatTeammateChannelAccess),
  };
}

export function parseChatAccessProfileRevision(
  value: unknown,
  label = "Chat access profile revision",
): ChatAccessProfileRevision {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    accessProfileId: readIdentifier(record.accessProfileId, `${label}.accessProfileId`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    defaultChannelCapabilities: (() => {
      const capabilities = readRecord(
        record.defaultChannelCapabilities,
        `${label}.defaultChannelCapabilities`,
      );
      return {
        readHistory: readBoolean(
          capabilities.readHistory,
          `${label}.defaultChannelCapabilities.readHistory`,
        ),
        participate: readBoolean(
          capabilities.participate,
          `${label}.defaultChannelCapabilities.participate`,
        ),
      };
    })(),
    defaultHistoryBoundary: parseChatHistoryBoundary(
      record.defaultHistoryBoundary,
      `${label}.defaultHistoryBoundary`,
    ),
    maximumFolderCapability: readEnum(
      record.maximumFolderCapability,
      CHAT_FOLDER_CAPABILITIES,
      `${label}.maximumFolderCapability`,
    ),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
  };
}

export function parseChatAccessProfile(
  value: unknown,
  label = "Chat access profile",
): ChatAccessProfileRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    builtinKey: readNullable(
      record.builtinKey,
      `${label}.builtinKey`,
      (entry, entryLabel) => readEnum(
        entry,
        CHAT_ACCESS_PROFILE_BUILTIN_KEYS,
        entryLabel,
      ),
    ),
    displayName: readString(record.displayName, `${label}.displayName`),
    latestRevision: parseChatAccessProfileRevision(record.latestRevision, `${label}.latestRevision`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    archivedAt: readNullable(record.archivedAt, `${label}.archivedAt`, readUtcTimestamp),
  };
}

export function parseChatAccessProfiles(
  value: unknown,
  label = "Chat access profiles",
): ChatAccessProfileRead[] {
  return readArray(value, label, parseChatAccessProfile);
}

function parseChatTeammateAccessPreviewIssue(
  value: unknown,
  label: string,
): ChatTeammateAccessPreviewIssue {
  const record = readRecord(value, label);
  return {
    code: readEnum(record.code, CHAT_ACCESS_ISSUE_CODES, `${label}.code`),
    fieldPath: readString(record.fieldPath, `${label}.fieldPath`),
    message: readString(record.message, `${label}.message`),
  };
}

export function parseChatTeammateAccessPreview(
  value: unknown,
  label = "Chat teammate access preview",
): ChatTeammateAccessPreviewRead {
  const record = readRecord(value, label);
  return {
    proposed: readNullable(record.proposed, `${label}.proposed`, parseChatTeammateAccess),
    isExpansion: readBoolean(record.isExpansion, `${label}.isExpansion`),
    addedChannelIds: readArray(record.addedChannelIds, `${label}.addedChannelIds`, readIdentifier),
    removedChannelIds: readArray(record.removedChannelIds, `${label}.removedChannelIds`, readIdentifier),
    issues: readArray(record.issues, `${label}.issues`, parseChatTeammateAccessPreviewIssue),
  };
}

export function parseChatAccessProfileImpactPreview(
  value: unknown,
  label = "Chat access profile impact preview",
): ChatAccessProfileImpactPreviewRead {
  const record = readRecord(value, label);
  return {
    accessProfileId: readIdentifier(record.accessProfileId, `${label}.accessProfileId`),
    currentRevision: parseChatAccessProfileRevision(record.currentRevision, `${label}.currentRevision`),
    isExpansion: readBoolean(record.isExpansion, `${label}.isExpansion`),
    isReduction: readBoolean(record.isReduction, `${label}.isReduction`),
    affectedTeammateIds: readArray(
      record.affectedTeammateIds,
      `${label}.affectedTeammateIds`,
      readIdentifier,
    ),
    affectedChannelIds: readArray(
      record.affectedChannelIds,
      `${label}.affectedChannelIds`,
      readIdentifier,
    ),
    activeAuthorizationCount: readNonNegativeSafeInteger(
      record.activeAuthorizationCount,
      `${label}.activeAuthorizationCount`,
    ),
    issues: readArray(record.issues, `${label}.issues`, parseChatTeammateAccessPreviewIssue),
  };
}

export function parseChatProjectPrimaryWorkingFolder(
  value: unknown,
  label = "Chat project primary working folder",
): ChatProjectPrimaryWorkingFolderRead {
  const record = readRecord(value, label);
  return {
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
  };
}

function parseChatExecutionTarget(value: unknown, label: string): ChatExecutionTarget {
  const record = readRecord(value, label);
  const kind = readEnum(record.kind, ["scratch", "workingFolder"] as const, `${label}.kind`);
  if (kind === "scratch") {
    return {
      kind,
      scratchScopeId: readIdentifier(record.scratchScopeId, `${label}.scratchScopeId`),
      scratchGenerationId: readIdentifier(
        record.scratchGenerationId,
        `${label}.scratchGenerationId`,
      ),
      executionEnvironmentId: readIdentifier(
        record.executionEnvironmentId,
        `${label}.executionEnvironmentId`,
      ),
    };
  }
  return {
    kind,
    workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
    executionEnvironmentId: readIdentifier(
      record.executionEnvironmentId,
      `${label}.executionEnvironmentId`,
    ),
  };
}

export function parseChatMessageReference(
  value: unknown,
  label = "Chat message reference",
): ChatMessageReference {
  const record = readRecord(value, label);
  const kind = readEnum(
    record.kind,
    ["participant", "channel", "workingFolder", "workspacePath", "executionEnvironment"] as const,
    `${label}.kind`,
  );
  const metadataRecord = readRecord(record.metadata, `${label}.metadata`);
  const metadata = {
    referenceId: readIdentifier(metadataRecord.referenceId, `${label}.metadata.referenceId`),
    labelSnapshot: readString(metadataRecord.labelSnapshot, `${label}.metadata.labelSnapshot`),
    startOffset: readNonNegativeSafeInteger(metadataRecord.startOffset, `${label}.metadata.startOffset`),
    endOffset: readNonNegativeSafeInteger(metadataRecord.endOffset, `${label}.metadata.endOffset`),
    plainTextProjection: readString(
      metadataRecord.plainTextProjection,
      `${label}.metadata.plainTextProjection`,
    ),
  };
  switch (kind) {
    case "participant":
      return {
        kind,
        metadata,
        participantId: readIdentifier(record.participantId, `${label}.participantId`),
        participantKind: readEnum(
          record.participantKind,
          CHAT_PARTICIPANT_KINDS,
          `${label}.participantKind`,
        ),
      };
    case "channel":
      return { kind, metadata, channelId: readIdentifier(record.channelId, `${label}.channelId`) };
    case "workingFolder":
      return {
        kind,
        metadata,
        workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
      };
    case "workspacePath":
      return {
        kind,
        metadata,
        workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
        pathKind: readEnum(record.pathKind, ["file", "folder"] as const, `${label}.pathKind`),
        relativePath: readString(record.relativePath, `${label}.relativePath`),
      };
    case "executionEnvironment":
      return {
        kind,
        metadata,
        executionEnvironmentId: readIdentifier(
          record.executionEnvironmentId,
          `${label}.executionEnvironmentId`,
        ),
      };
  }
}

export function parseChatAssignmentTarget(
  value: unknown,
  label = "Chat assignment target",
): ChatAssignmentTargetRead {
  const record = readRecord(value, label);
  return {
    executionTarget: readNullable(
      record.executionTarget,
      `${label}.executionTarget`,
      parseChatExecutionTarget,
    ),
    kind: readEnum(record.kind, CHAT_ASSIGNMENT_TARGET_KINDS, `${label}.kind`),
    displayName: readString(record.displayName, `${label}.displayName`),
    folderCapability: readNullable(
      record.folderCapability,
      `${label}.folderCapability`,
      (entry, entryLabel) => readEnum(entry, CHAT_FOLDER_CAPABILITIES, entryLabel),
    ),
    effectiveRuntimeApproval: readEnum(
      record.effectiveRuntimeApproval,
      CHAT_RUNTIME_APPROVAL_POLICIES,
      `${label}.effectiveRuntimeApproval`,
    ),
    isDefault: readBoolean(record.isDefault, `${label}.isDefault`),
    bindingState: readEnum(
      record.bindingState,
      CHAT_ASSIGNMENT_TARGET_BINDING_STATES,
      `${label}.bindingState`,
    ),
    lifecycleState: readEnum(
      record.lifecycleState,
      CHAT_ASSIGNMENT_TARGET_LIFECYCLE_STATES,
      `${label}.lifecycleState`,
    ),
    isBusy: readBoolean(record.isBusy, `${label}.isBusy`),
    isDirty: readNullable(record.isDirty, `${label}.isDirty`, readBoolean),
    eligible: readBoolean(record.eligible, `${label}.eligible`),
    unavailableReason: readNullable(record.unavailableReason, `${label}.unavailableReason`, readString),
  };
}

export function parseChatAssignmentTargets(
  value: unknown,
  label = "Chat assignment targets",
): ChatAssignmentTargetRead[] {
  return readArray(value, label, parseChatAssignmentTarget);
}

function parseChatScratchSourceSummary(
  value: unknown,
  label: string,
): ChatScratchSourceSummaryRead {
  const record = readRecord(value, label);
  const lowerOrdinal = readNonNegativeSafeInteger(record.lowerOrdinal, `${label}.lowerOrdinal`);
  const highOrdinal = readNonNegativeSafeInteger(record.highOrdinal, `${label}.highOrdinal`);
  if (highOrdinal < lowerOrdinal) {
    throw new Error(`${label}.highOrdinal must not precede lowerOrdinal`);
  }
  return {
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    channelName: readString(record.channelName, `${label}.channelName`),
    lowerOrdinal,
    highOrdinal,
    audienceRevision: readNonNegativeSafeInteger(
      record.audienceRevision,
      `${label}.audienceRevision`,
    ),
  };
}

function parseChatScratchGeneration(
  value: unknown,
  label: string,
): ChatScratchGenerationRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    executionEnvironmentId: readIdentifier(
      record.executionEnvironmentId,
      `${label}.executionEnvironmentId`,
    ),
    generation: readNonNegativeSafeInteger(record.generation, `${label}.generation`),
    lifecycleState: readEnum(
      record.lifecycleState,
      CHAT_SCRATCH_GENERATION_LIFECYCLE_STATES,
      `${label}.lifecycleState`,
    ),
    byteSize: readNonNegativeSafeInteger(record.byteSize, `${label}.byteSize`),
    entryCount: readNonNegativeSafeInteger(record.entryCount, `${label}.entryCount`),
    sizeTruncated: readBoolean(record.sizeTruncated, `${label}.sizeTruncated`),
    deviceAvailability: readEnum(
      record.deviceAvailability,
      CHAT_SCRATCH_DEVICE_AVAILABILITIES,
      `${label}.deviceAvailability`,
    ),
    retainedSources: readArray(
      record.retainedSources,
      `${label}.retainedSources`,
      parseChatScratchSourceSummary,
    ),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

export function parseChatScratchScope(
  value: unknown,
  label = "Chat scratch scope",
): ChatScratchScopeRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    replyThreadId: readIdentifier(record.replyThreadId, `${label}.replyThreadId`),
    teammateId: readIdentifier(record.teammateId, `${label}.teammateId`),
    teammateName: readString(record.teammateName, `${label}.teammateName`),
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    channelName: readString(record.channelName, `${label}.channelName`),
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    projectName: readString(record.projectName, `${label}.projectName`),
    groupId: readIdentifier(record.groupId, `${label}.groupId`),
    groupName: readString(record.groupName, `${label}.groupName`),
    lifecycleState: readEnum(
      record.lifecycleState,
      CHAT_SCRATCH_SCOPE_LIFECYCLE_STATES,
      `${label}.lifecycleState`,
    ),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    generations: readArray(
      record.generations,
      `${label}.generations`,
      parseChatScratchGeneration,
    ),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

export function parseChatScratchScopes(
  value: unknown,
  label = "Chat scratch scopes",
): ChatScratchScopeRead[] {
  return readArray(value, label, parseChatScratchScope);
}

function parseChatScratchDirectoryEntry(
  value: unknown,
  label: string,
): ChatScratchDirectoryEntryRead {
  const record = readRecord(value, label);
  return {
    relativePath: readScratchRelativePath(record.relativePath, `${label}.relativePath`),
    displayName: readString(record.displayName, `${label}.displayName`),
    kind: readEnum(record.kind, CHAT_SCRATCH_DIRECTORY_ENTRY_KINDS, `${label}.kind`),
    byteSize: readNullable(record.byteSize, `${label}.byteSize`, readNonNegativeSafeInteger),
    contentRevision: readNullable(record.contentRevision, `${label}.contentRevision`, readSha256),
    promotable: readBoolean(record.promotable, `${label}.promotable`),
  };
}

export function parseChatScratchDirectoryPage(
  value: unknown,
  label = "Chat scratch directory page",
): ChatScratchDirectoryPageRead {
  const record = readRecord(value, label);
  const entries = readArray(
    record.entries,
    `${label}.entries`,
    parseChatScratchDirectoryEntry,
  );
  if (entries.length > MAX_SCRATCH_DIRECTORY_PAGE_ENTRIES) {
    throw new Error(`${label}.entries exceeds the bounded page size`);
  }
  return {
    scratchGenerationId: readIdentifier(
      record.scratchGenerationId,
      `${label}.scratchGenerationId`,
    ),
    relativePath: readScratchRelativePath(
      record.relativePath,
      `${label}.relativePath`,
      true,
    ),
    entries,
    nextCursor: readNullable(record.nextCursor, `${label}.nextCursor`, readString),
  };
}

function parseChatScratchPromotionDestination(
  value: unknown,
  label: string,
): ChatScratchPromotionDestinationRead {
  const record = readRecord(value, label);
  const kind = readEnum(
    record.kind,
    ["workingFolder", "managedAttachment"] as const,
    `${label}.kind`,
  );
  if (kind === "workingFolder") {
    return {
      kind,
      workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
      relativePath: readScratchRelativePath(record.relativePath, `${label}.relativePath`),
    };
  }
  return {
    kind,
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    attachmentId: readIdentifier(record.attachmentId, `${label}.attachmentId`),
  };
}

export function parseChatScratchPromotionResult(
  value: unknown,
  label = "Chat scratch promotion result",
): ChatScratchPromotionResultRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    scratchGenerationId: readIdentifier(
      record.scratchGenerationId,
      `${label}.scratchGenerationId`,
    ),
    sourceRelativePath: readScratchRelativePath(
      record.sourceRelativePath,
      `${label}.sourceRelativePath`,
    ),
    sourceSha256: readSha256(record.sourceSha256, `${label}.sourceSha256`),
    destination: parseChatScratchPromotionDestination(
      record.destination,
      `${label}.destination`,
    ),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
  };
}

export function parseChatScratchCleanupPreview(
  value: unknown,
  label = "Chat scratch cleanup preview",
): ChatScratchCleanupPreviewRead {
  const record = readRecord(value, label);
  return {
    scratchScopeId: readIdentifier(record.scratchScopeId, `${label}.scratchScopeId`),
    scratchGenerationId: readIdentifier(
      record.scratchGenerationId,
      `${label}.scratchGenerationId`,
    ),
    expectedScopeRevision: readNonNegativeSafeInteger(
      record.expectedScopeRevision,
      `${label}.expectedScopeRevision`,
    ),
    lifecycleState: readEnum(
      record.lifecycleState,
      CHAT_SCRATCH_GENERATION_LIFECYCLE_STATES,
      `${label}.lifecycleState`,
    ),
    byteSize: readNonNegativeSafeInteger(record.byteSize, `${label}.byteSize`),
    entryCount: readNonNegativeSafeInteger(record.entryCount, `${label}.entryCount`),
    sizeTruncated: readBoolean(record.sizeTruncated, `${label}.sizeTruncated`),
    deviceAvailability: readEnum(
      record.deviceAvailability,
      CHAT_SCRATCH_DEVICE_AVAILABILITIES,
      `${label}.deviceAvailability`,
    ),
    activeRunCount: readNonNegativeSafeInteger(
      record.activeRunCount,
      `${label}.activeRunCount`,
    ),
    willRemoveScope: readBoolean(record.willRemoveScope, `${label}.willRemoveScope`),
  };
}

export function parseChatScratchCleanupResult(
  value: unknown,
  label = "Chat scratch cleanup result",
): ChatScratchCleanupResultRead {
  const record = readRecord(value, label);
  return {
    jobId: readIdentifier(record.jobId, `${label}.jobId`),
    scratchScopeId: readIdentifier(record.scratchScopeId, `${label}.scratchScopeId`),
    scratchGenerationId: readIdentifier(
      record.scratchGenerationId,
      `${label}.scratchGenerationId`,
    ),
    scopeRevision: readNonNegativeSafeInteger(record.scopeRevision, `${label}.scopeRevision`),
    state: readEnum(record.state, CHAT_SCRATCH_CLEANUP_JOB_STATES, `${label}.state`),
    removedBytes: readNonNegativeSafeInteger(record.removedBytes, `${label}.removedBytes`),
    deviceAvailability: readEnum(
      record.deviceAvailability,
      CHAT_SCRATCH_DEVICE_AVAILABILITIES,
      `${label}.deviceAvailability`,
    ),
    completedAt: readNullable(record.completedAt, `${label}.completedAt`, readUtcTimestamp),
  };
}

export function parseChatReplyThreadSummary(
  value: unknown,
  label = "Chat reply thread summary",
): ChatReplyThreadSummaryRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    replyCount: readNonNegativeSafeInteger(record.replyCount, `${label}.replyCount`),
    lastActivityAt: readUtcTimestamp(record.lastActivityAt, `${label}.lastActivityAt`),
    participants: readArray(record.participants, `${label}.participants`, parseChatParticipant),
    unread: readBoolean(record.unread, `${label}.unread`),
    workState: readNullable(
      record.workState,
      `${label}.workState`,
      (entry, entryLabel) => readEnum(entry, CHAT_WORK_ASSIGNMENT_STATES, entryLabel),
    ),
  };
}

export function parseChatMessage(value: unknown, label = "Chat message"): ChatMessageRead {
  const record = readRecord(value, label);
  const normalizedMarkdown = readString(record.normalizedMarkdown, `${label}.normalizedMarkdown`);
  return {
    itemId: readIdentifier(record.itemId, `${label}.itemId`),
    conversationId: readIdentifier(record.conversationId, `${label}.conversationId`),
    replyThreadId: readNullable(record.replyThreadId, `${label}.replyThreadId`, readIdentifier),
    revisionId: readIdentifier(record.revisionId, `${label}.revisionId`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    author: parseChatParticipant(record.author, `${label}.author`),
    authorLabelSnapshot: readString(record.authorLabelSnapshot, `${label}.authorLabelSnapshot`),
    normalizedMarkdown,
    richContent: readVersionedJson(record.richContent, `${label}.richContent`),
    attachmentIds: readArray(record.attachmentIds, `${label}.attachmentIds`, readIdentifier),
    references: parseChatMessageReferencesForText(
      record.references,
      normalizedMarkdown,
      `${label}.references`,
    ),
    replyThread: readNullable(record.replyThread, `${label}.replyThread`, parseChatReplyThreadSummary),
    ordinal: readNonNegativeSafeInteger(record.ordinal, `${label}.ordinal`),
    editedAt: readNullable(record.editedAt, `${label}.editedAt`, readUtcTimestamp),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
  };
}

export function parseChatChannelPage(value: unknown, label = "Chat channel page"): ChatChannelPageRead {
  const record = readRecord(value, label);
  return {
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    messages: readArray(record.messages, `${label}.messages`, parseChatMessage),
    previousCursor: readNullable(record.previousCursor, `${label}.previousCursor`, readString),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
  };
}

export function parseChatWorkAssignment(
  value: unknown,
  label = "Chat work assignment",
): ChatWorkAssignmentRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    replyThreadId: readIdentifier(record.replyThreadId, `${label}.replyThreadId`),
    teammate: parseChatParticipant(record.teammate, `${label}.teammate`),
    triggeringMessageItemId: readIdentifier(record.triggeringMessageItemId, `${label}.triggeringMessageItemId`),
    previousAssignmentId: readNullable(record.previousAssignmentId, `${label}.previousAssignmentId`, readIdentifier),
    state: readEnum(record.state, CHAT_WORK_ASSIGNMENT_STATES, `${label}.state`),
    stateReason: readNullable(record.stateReason, `${label}.stateReason`, readString),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    settledAt: readNullable(record.settledAt, `${label}.settledAt`, readUtcTimestamp),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

function parseAgentRun(value: unknown, label: string): ChatAgentRunRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    assignmentId: readIdentifier(record.assignmentId, `${label}.assignmentId`),
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    workingFolderId: readNullable(record.workingFolderId, `${label}.workingFolderId`, readIdentifier),
    executionEnvironmentId: readIdentifier(
      record.executionEnvironmentId,
      `${label}.executionEnvironmentId`,
    ),
    scratchGenerationId: readNullable(
      record.scratchGenerationId,
      `${label}.scratchGenerationId`,
      readIdentifier,
    ),
    teammatePolicyRevisionId: readIdentifier(record.teammatePolicyRevisionId, `${label}.teammatePolicyRevisionId`),
    effort: readNullable(record.effort, `${label}.effort`, readString),
    providerExecutionTurnId: readIdentifier(record.providerExecutionTurnId, `${label}.providerExecutionTurnId`),
    providerExecutionThreadId: readNullable(
      record.providerExecutionThreadId,
      `${label}.providerExecutionThreadId`,
      readIdentifier,
    ),
    state: readEnum(record.state, AGENT_RUN_STATES, `${label}.state`),
    runOrdinal: readNonNegativeSafeInteger(record.runOrdinal, `${label}.runOrdinal`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

export function parseChatReplyThreadPage(
  value: unknown,
  label = "Chat reply thread page",
): ChatReplyThreadPageRead {
  const record = readRecord(value, label);
  return {
    thread: parseChatReplyThreadSummary(record.thread, `${label}.thread`),
    rootMessage: parseChatMessage(record.rootMessage, `${label}.rootMessage`),
    replies: readArray(record.replies, `${label}.replies`, parseChatMessage),
    assignment: readNullable(record.assignment, `${label}.assignment`, parseChatWorkAssignment),
    agentRuns: readArray(record.agentRuns, `${label}.agentRuns`, parseAgentRun),
    previousCursor: readNullable(record.previousCursor, `${label}.previousCursor`, readString),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
  };
}

export function parsePostChatMessageResult(
  value: unknown,
  label = "Post Chat message result",
): PostChatMessageResult {
  const record = readRecord(value, label);
  return {
    message: parseChatMessage(record.message, `${label}.message`),
    replyThreadId: readNullable(record.replyThreadId, `${label}.replyThreadId`, readIdentifier),
    assignment: readNullable(record.assignment, `${label}.assignment`, parseChatWorkAssignment),
    assignmentInputQueued: readBoolean(record.assignmentInputQueued, `${label}.assignmentInputQueued`),
  };
}

export function parseChatScheduledMessage(
  value: unknown,
  label = "Chat scheduled message",
): ChatScheduledMessageRead {
  const record = readRecord(value, label);
  const normalizedMarkdown = readString(record.normalizedMarkdown, `${label}.normalizedMarkdown`);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    replyThreadId: readNullable(record.replyThreadId, `${label}.replyThreadId`, readIdentifier),
    normalizedMarkdown,
    richContent: readVersionedJson(record.richContent, `${label}.richContent`),
    attachmentIds: readArray(record.attachmentIds, `${label}.attachmentIds`, readIdentifier),
    references: parseChatMessageReferencesForText(
      record.references,
      normalizedMarkdown,
      `${label}.references`,
    ),
    alsoSendToChannel: readBoolean(record.alsoSendToChannel, `${label}.alsoSendToChannel`),
    state: readEnum(record.state, CHAT_SCHEDULED_MESSAGE_STATES, `${label}.state`),
    scheduledFor: readUtcTimestamp(record.scheduledFor, `${label}.scheduledFor`),
    lastError: readNullable(record.lastError, `${label}.lastError`, readString),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
  };
}

function parseChatMessageReferencesForText(
  value: unknown,
  text: string,
  label: string,
): ChatMessageReference[] {
  const parsed = readArray(value, label, parseChatMessageReference);
  const normalized = normalizeChatMessageReferences(text, parsed);
  if (normalized.length !== parsed.length) {
    throw new Error(`${label} contains an invalid, overlapping, or duplicate reference`);
  }
  return normalized;
}

export function parseChatScheduledMessages(
  value: unknown,
  label = "Chat scheduled messages",
): ChatScheduledMessageRead[] {
  return readArray(value, label, parseChatScheduledMessage);
}

export function parseChatScheduledMessageDispatch(
  value: unknown,
  label = "Chat scheduled message dispatch",
): ChatScheduledMessageDispatchRead {
  const record = readRecord(value, label);
  return {
    processedCount: readNonNegativeSafeInteger(record.processedCount, `${label}.processedCount`),
    dispatchedCount: readNonNegativeSafeInteger(record.dispatchedCount, `${label}.dispatchedCount`),
    dispatchedChannelIds: readArray(record.dispatchedChannelIds, `${label}.dispatchedChannelIds`, readIdentifier),
    nextDispatchAt: readNullable(record.nextDispatchAt, `${label}.nextDispatchAt`, readUtcTimestamp),
  };
}

export function parseChatMessageSearchResult(
  value: unknown,
  label = "Chat message search result",
): ChatMessageSearchResultRead {
  const record = readRecord(value, label);
  return {
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    channelName: readString(record.channelName, `${label}.channelName`),
    conversationId: readIdentifier(record.conversationId, `${label}.conversationId`),
    replyThreadId: readNullable(record.replyThreadId, `${label}.replyThreadId`, readIdentifier),
    messageItemId: readIdentifier(record.messageItemId, `${label}.messageItemId`),
    ordinal: readNonNegativeSafeInteger(record.ordinal, `${label}.ordinal`),
    authorParticipantId: readIdentifier(record.authorParticipantId, `${label}.authorParticipantId`),
    authorKind: readEnum(record.authorKind, CHAT_PARTICIPANT_KINDS, `${label}.authorKind`),
    authorDisplayName: readString(record.authorDisplayName, `${label}.authorDisplayName`),
    excerpt: readString(record.excerpt, `${label}.excerpt`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
  };
}

export function parseChatMessageSearchResults(
  value: unknown,
  label = "Chat message search results",
): ChatMessageSearchResultRead[] {
  return readArray(value, label, parseChatMessageSearchResult);
}
