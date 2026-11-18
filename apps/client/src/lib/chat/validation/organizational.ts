import {
  CHAT_AGENT_RUN_ATTEMPT_REASONS,
  CHAT_AGENT_RUN_HANDOFF_REASONS,
  CHAT_ASSIGNMENT_INPUT_DELIVERY_STATES,
  CHAT_ASSIGNMENT_INPUT_ROUTING_KINDS,
  CHAT_ASSIGNMENT_REVIEW_OUTCOMES,
  CHAT_ASSIGNMENT_TARGET_BINDING_STATES,
  CHAT_ASSIGNMENT_TARGET_KINDS,
  CHAT_ASSIGNMENT_TARGET_LIFECYCLE_STATES,
  CHAT_FOLDER_CAPABILITIES,
  CHAT_FOLLOWUP_ROUTINGS,
  CHAT_PARTICIPANT_KINDS,
  CHAT_RUNTIME_APPROVAL_POLICIES,
  CHAT_SCRATCH_CLEANUP_JOB_STATES,
  CHAT_SCRATCH_DEVICE_AVAILABILITIES,
  CHAT_SCRATCH_DIRECTORY_ENTRY_KINDS,
  CHAT_SCRATCH_GENERATION_LIFECYCLE_STATES,
  CHAT_SCRATCH_RECLAIM_REASONS,
  CHAT_SCRATCH_SCOPE_LIFECYCLE_STATES,
  CHAT_REVIEW_QUEUE_KINDS,
  CHAT_SCHEDULED_MESSAGE_STATES,
  CHAT_AGENT_CONFIGURATION_STATES,
  CHAT_WORK_ASSIGNMENT_STATES,
  CHAT_ACCESS_DOWNGRADE_REASONS,
  CHAT_AGENT_ACCESS_LEVELS,
  CHAT_AGENT_ACCESS_SOURCES,
  CHAT_EFFECTIVE_AGENT_ACCESS_LEVELS,
  INTERACTION_MODES,
  type ChatAgentRunRead,
  type ChatAssignmentInputRead,
  type ChatAiAgentRead,
  type ChatAssignmentAcceptanceFolderRead,
  type ChatAssignmentDelegationRead,
  type ChatAssignmentAcceptanceOfferRead,
  type ChatAgentUsageDayRead,
  type ChatAssignmentReview,
  type ChatAssignmentTargetRead,
  type ChatChannelPageRead,
  type ChatChannelMembershipRemovalPreview,
  type ChatChannelRosterRead,
  type ChatConversationMembershipRead,
  type ChatDelegatedChildRead,
  type ChatExecutionTarget,
  type ChatMessageRead,
  type ChatMessageReference,
  type ChatMessageSearchResultRead,
  type ChatParticipantRead,
  type ChatProjectPrimaryWorkingFolderRead,
  type ChatReplyThreadPageRead,
  type ChatReplyThreadSummaryRead,
  type ChatReviewQueueItemRead,
  type ChatReviewQueueRead,
  type ChatScheduledMessageDispatchRead,
  type ChatScheduledMessageRead,
  type ChatScratchCleanupPreviewRead,
  type ChatScratchCleanupResultRead,
  type ChatScratchReclaimReportRead,
  type ChatScratchReclaimRow,
  type ChatScratchDirectoryEntryRead,
  type ChatScratchDirectoryPageRead,
  type ChatScratchGenerationRead,
  type ChatScratchPromotionDestinationRead,
  type ChatScratchPromotionResultRead,
  type ChatScratchScopeRead,
  type ChatScratchSourceSummaryRead,
  type ChatSecretRequestRead,
  type ChatAgentPolicyRead,
  type ChatAgentAccess,
  type ChatAgentResolvedAccessRead,
  type ChatAppAgentSettingsRead,
  type ChatChannelAgentSettingsRead,
  type ChatEffectiveChannelAccessRead,
  type ChatModelSelection,
  type ChatProjectAgentSettingsRead,
  type ChatWaitingAssignmentRead,
  type ChatWaitingAssignmentsRead,
  type ChatWorkAssignmentRead,
  type PostChatMessageResult,
} from "../contracts";
import { parseModelOptionSelection } from "./provider";
import { parseChatPushApprovals } from "./push-requests";
import { normalizeChatMessageReferences } from "$lib/chat/composer/message-references";
import {
  readArray,
  readBoolean,
  readEnum,
  readIdentifier,
  readNullable,
  readNonNegativeSafeInteger,
  readOrderKey,
  readRecord,
  readString,
  readUtcTimestamp,
  readVersionedJson,
} from "./readers";
import { compareCodeUnits } from "$lib/utils/order-key";
import { parseChatScheduledRecurrence } from "./scheduling";

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

export function parseChatAgentPolicy(value: unknown, label = "Chat agent policy"): ChatAgentPolicyRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    agentId: readIdentifier(record.agentId, `${label}.agentId`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    providerManagedModel: readBoolean(record.providerManagedModel, `${label}.providerManagedModel`),
    modelId: readNullable(record.modelId, `${label}.modelId`, readIdentifier),
    modelOptions: readArray(record.modelOptions, `${label}.modelOptions`, parseModelOptionSelection),
    providerOptions: readVersionedJson(record.providerOptions, `${label}.providerOptions`),
    interactionMode: readEnum(record.interactionMode, INTERACTION_MODES, `${label}.interactionMode`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
  };
}

export function parseChatAiAgent(value: unknown, label = "Chat AI agent"): ChatAiAgentRead {
  const record = readRecord(value, label);
  return {
    participant: parseChatParticipant(record.participant, `${label}.participant`),
    providerFamilyId: readIdentifier(record.providerFamilyId, `${label}.providerFamilyId`),
    localProviderInstanceId: readNullable(
      record.localProviderInstanceId,
      `${label}.localProviderInstanceId`,
      readIdentifier,
    ),
    configurationState: readEnum(
      record.configurationState,
      CHAT_AGENT_CONFIGURATION_STATES,
      `${label}.configurationState`,
    ),
    latestPolicy: readNullable(record.latestPolicy, `${label}.latestPolicy`, parseChatAgentPolicy),
    channelCount: readNonNegativeSafeInteger(record.channelCount, `${label}.channelCount`),
    activeAssignmentCount: readNonNegativeSafeInteger(
      record.activeAssignmentCount,
      `${label}.activeAssignmentCount`,
    ),
  };
}

export function parseChatAiAgents(value: unknown, label = "Chat AI agents"): ChatAiAgentRead[] {
  return readArray(value, label, parseChatAiAgent);
}

export function parseChatConversationMembership(
  value: unknown,
  label = "Chat conversation membership",
): ChatConversationMembershipRead {
  const record = readRecord(value, label);
  return {
    conversationId: readIdentifier(record.conversationId, `${label}.conversationId`),
    participant: parseChatParticipant(record.participant, `${label}.participant`),
    agentInChannel: readBoolean(record.agentInChannel, `${label}.agentInChannel`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    removedAt: readNullable(record.removedAt, `${label}.removedAt`, readUtcTimestamp),
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
    agentId: readIdentifier(record.agentId, `${label}.agentId`),
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
  };
}

function readAgentAccess(value: unknown, label: string): ChatAgentAccess {
  return readEnum(value, CHAT_AGENT_ACCESS_LEVELS, label);
}

/** Reads a model selection; the backend fills absent fields with the provider default. */
export function parseChatModelSelection(value: unknown, label = "Chat model selection"): ChatModelSelection {
  const record = readRecord(value, label);
  const modelId = record.modelId === undefined
    ? null
    : readNullable(record.modelId, `${label}.modelId`, readIdentifier);
  const providerManagedModel = record.providerManagedModel === undefined
    ? false
    : readBoolean(record.providerManagedModel, `${label}.providerManagedModel`);
  return {
    providerManagedModel,
    modelId,
    modelOptions: record.modelOptions === undefined
      ? []
      : readArray(record.modelOptions, `${label}.modelOptions`, parseModelOptionSelection),
  };
}

function parseEffectiveChannelAccess(value: unknown, label: string): ChatEffectiveChannelAccessRead {
  const record = readRecord(value, label);
  return {
    access: readAgentAccess(record.access, `${label}.access`),
    source: readEnum(record.source, CHAT_AGENT_ACCESS_SOURCES, `${label}.source`),
  };
}

function parseAgentResolvedAccess(value: unknown, label: string): ChatAgentResolvedAccessRead {
  const record = readRecord(value, label);
  return {
    agentId: readIdentifier(record.agentId, `${label}.agentId`),
    requested: readAgentAccess(record.requested, `${label}.requested`),
    effective: readNullable(
      record.effective,
      `${label}.effective`,
      (entry, entryLabel) => readEnum(entry, CHAT_EFFECTIVE_AGENT_ACCESS_LEVELS, entryLabel),
    ),
    downgradeReason: readNullable(
      record.downgradeReason,
      `${label}.downgradeReason`,
      (entry, entryLabel) => readEnum(entry, CHAT_ACCESS_DOWNGRADE_REASONS, entryLabel),
    ),
    installed: readBoolean(record.installed, `${label}.installed`),
    ownOffered: readBoolean(record.ownOffered, `${label}.ownOffered`),
  };
}

export function parseChatAppAgentSettings(
  value: unknown,
  label = "Chat app agent settings",
): ChatAppAgentSettingsRead {
  const record = readRecord(value, label);
  return {
    access: readAgentAccess(record.access, `${label}.access`),
    instructions: readString(record.instructions, `${label}.instructions`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

export function parseChatProjectAgentSettings(
  value: unknown,
  label = "Chat project agent settings",
): ChatProjectAgentSettingsRead {
  const record = readRecord(value, label);
  return {
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    access: readNullable(record.access, `${label}.access`, readAgentAccess),
    instructions: readString(record.instructions, `${label}.instructions`),
    effective: parseEffectiveChannelAccess(record.effective, `${label}.effective`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
  };
}

export function parseChatChannelAgentSettings(
  value: unknown,
  label = "Chat channel agent settings",
): ChatChannelAgentSettingsRead {
  const record = readRecord(value, label);
  return {
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    conversationId: readIdentifier(record.conversationId, `${label}.conversationId`),
    access: readNullable(record.access, `${label}.access`, readAgentAccess),
    instructions: readString(record.instructions, `${label}.instructions`),
    effective: parseEffectiveChannelAccess(record.effective, `${label}.effective`),
    agents: readArray(record.agents, `${label}.agents`, parseAgentResolvedAccess),
    modelOverrides: readArray(record.modelOverrides, `${label}.modelOverrides`, (entry, entryLabel) => {
      const override = readRecord(entry, entryLabel);
      return {
        agentId: readIdentifier(override.agentId, `${entryLabel}.agentId`),
        model: parseChatModelSelection(override.model, `${entryLabel}.model`),
      };
    }),
    pushApprovals: parseChatPushApprovals(record.pushApprovals, `${label}.pushApprovals`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
  };
}

export function parseChatProjectPrimaryWorkingFolder(
  value: unknown,
  label = "Chat project primary working folder",
): ChatProjectPrimaryWorkingFolderRead {
  const record = readRecord(value, label);
  return {
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    workingFolderId: record.workingFolderId === null
      ? null
      : readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
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
  const lowerOrderKey = readOrderKey(record.lowerOrderKey, `${label}.lowerOrderKey`);
  // An empty high key marks a range that held no message when the generation began.
  const highOrderKey = readOrderKey(record.highOrderKey, `${label}.highOrderKey`, true);
  if (highOrderKey !== "" && compareCodeUnits(highOrderKey, lowerOrderKey) < 0) {
    throw new Error(`${label}.highOrderKey must not precede lowerOrderKey`);
  }
  return {
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    channelName: readString(record.channelName, `${label}.channelName`),
    lowerOrderKey,
    highOrderKey,
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
    agentId: readIdentifier(record.agentId, `${label}.agentId`),
    agentName: readString(record.agentName, `${label}.agentName`),
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    channelName: readString(record.channelName, `${label}.channelName`),
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    projectName: readString(record.projectName, `${label}.projectName`),
    groupId: readNullable(record.groupId, `${label}.groupId`, readIdentifier),
    groupName: readNullable(record.groupName, `${label}.groupName`, readString),
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

function parseChatScratchReclaimRow(
  value: unknown,
  label = "Chat scratch reclaim row",
): ChatScratchReclaimRow {
  const record = readRecord(value, label);
  return {
    scratchScopeId: readIdentifier(record.scratchScopeId, `${label}.scratchScopeId`),
    scratchGenerationId: readIdentifier(
      record.scratchGenerationId,
      `${label}.scratchGenerationId`,
    ),
    replyThreadId: readIdentifier(record.replyThreadId, `${label}.replyThreadId`),
    generation: readNonNegativeSafeInteger(record.generation, `${label}.generation`),
    agentName: readString(record.agentName, `${label}.agentName`),
    channelName: readString(record.channelName, `${label}.channelName`),
    projectName: readString(record.projectName, `${label}.projectName`),
    groupName: readNullable(record.groupName, `${label}.groupName`, readString),
    reason: readEnum(record.reason, CHAT_SCRATCH_RECLAIM_REASONS, `${label}.reason`),
    byteSize: readNonNegativeSafeInteger(record.byteSize, `${label}.byteSize`),
    sizeTruncated: readBoolean(record.sizeTruncated, `${label}.sizeTruncated`),
    deviceAvailability: readEnum(
      record.deviceAvailability,
      CHAT_SCRATCH_DEVICE_AVAILABILITIES,
      `${label}.deviceAvailability`,
    ),
    lastActivityAt: readUtcTimestamp(record.lastActivityAt, `${label}.lastActivityAt`),
  };
}

/** Validates the reclaimable scratch report returned by the native command. */
export function parseChatScratchReclaimReport(
  value: unknown,
  label = "Chat scratch reclaim report",
): ChatScratchReclaimReportRead {
  const record = readRecord(value, label);
  return {
    rows: readArray(record.rows, `${label}.rows`, parseChatScratchReclaimRow),
    totalBytes: readNonNegativeSafeInteger(record.totalBytes, `${label}.totalBytes`),
    totalTruncated: readBoolean(record.totalTruncated, `${label}.totalTruncated`),
    omitted: readNonNegativeSafeInteger(record.omitted, `${label}.omitted`),
    idleDays: readNonNegativeSafeInteger(record.idleDays, `${label}.idleDays`),
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
    unseenFailure: readBoolean(record.unseenFailure, `${label}.unseenFailure`),
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
    author: readNullable(record.author, `${label}.author`, parseChatParticipant),
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
    orderKey: readOrderKey(record.orderKey, `${label}.orderKey`),
    editedAt: readNullable(record.editedAt, `${label}.editedAt`, readUtcTimestamp),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    awaitingDelivery: readBoolean(record.awaitingDelivery, `${label}.awaitingDelivery`),
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
  const state = readEnum(record.state, CHAT_WORK_ASSIGNMENT_STATES, `${label}.state`);
  const review = readNullable(record.review, `${label}.review`, parseAssignmentReview);
  if (review && state !== "completed") {
    throw new Error(`${label}.review is only valid on completed work`);
  }
  return {
    id: readIdentifier(record.id, `${label}.id`),
    replyThreadId: readIdentifier(record.replyThreadId, `${label}.replyThreadId`),
    agent: parseChatParticipant(record.agent, `${label}.agent`),
    triggeringMessageItemId: readIdentifier(record.triggeringMessageItemId, `${label}.triggeringMessageItemId`),
    previousAssignmentId: readNullable(record.previousAssignmentId, `${label}.previousAssignmentId`, readIdentifier),
    state,
    stateReason: readNullable(record.stateReason, `${label}.stateReason`, readString),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    settledAt: readNullable(record.settledAt, `${label}.settledAt`, readUtcTimestamp),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
    review,
    localAcceptance: readNullable(record.localAcceptance, `${label}.localAcceptance`, parseAssignmentAcceptanceOffer),
    delegation: readNullable(record.delegation, `${label}.delegation`, parseAssignmentDelegation),
    modelSelection: parseChatModelSelection(record.modelSelection, `${label}.modelSelection`),
    instructions: readString(record.instructions, `${label}.instructions`),
    modelLabel: readNullable(record.modelLabel, `${label}.modelLabel`, readString),
    latestRunProviderThreadId: readNullable(
      record.latestRunProviderThreadId,
      `${label}.latestRunProviderThreadId`,
      readIdentifier,
    ),
  };
}

const LOCAL_DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

/** Validates one local day of an agent's usage. */
export function parseChatAgentUsageDay(value: unknown, label = "Chat agent usage day"): ChatAgentUsageDayRead {
  const record = readRecord(value, label);
  const localDate = readString(record.localDate, `${label}.localDate`);
  if (!LOCAL_DATE_PATTERN.test(localDate)) throw new Error(`${label}.localDate must be a YYYY-MM-DD date`);
  return {
    localDate,
    countedTokens: readNonNegativeSafeInteger(record.countedTokens, `${label}.countedTokens`),
    workingSeconds: readNonNegativeSafeInteger(record.workingSeconds, `${label}.workingSeconds`),
    runCount: readNonNegativeSafeInteger(record.runCount, `${label}.runCount`),
    deviceIds: readArray(record.deviceIds, `${label}.deviceIds`, readIdentifier),
  };
}

function parseAssignmentReview(value: unknown, label: string): ChatAssignmentReview {
  const record = readRecord(value, label);
  return {
    outcome: readEnum(record.outcome, CHAT_ASSIGNMENT_REVIEW_OUTCOMES, `${label}.outcome`),
    reviewedAt: readUtcTimestamp(record.reviewedAt, `${label}.reviewedAt`),
    reviewedByParticipantId: readIdentifier(record.reviewedByParticipantId, `${label}.reviewedByParticipantId`),
  };
}

function parseAssignmentAcceptanceFolder(value: unknown, label: string): ChatAssignmentAcceptanceFolderRead {
  const record = readRecord(value, label);
  return {
    displayName: readString(record.displayName, `${label}.displayName`),
    capability: readEnum(record.capability, CHAT_FOLDER_CAPABILITIES, `${label}.capability`),
    runtimeApproval: readEnum(record.runtimeApproval, CHAT_RUNTIME_APPROVAL_POLICIES, `${label}.runtimeApproval`),
  };
}

function parseAssignmentAcceptanceOffer(value: unknown, label: string): ChatAssignmentAcceptanceOfferRead {
  const record = readRecord(value, label);
  return {
    digest: readIdentifier(record.digest, `${label}.digest`),
    providerInstanceId: readIdentifier(record.providerInstanceId, `${label}.providerInstanceId`),
    access: readEnum(record.access, CHAT_EFFECTIVE_AGENT_ACCESS_LEVELS, `${label}.access`),
    folders: readArray(record.folders, `${label}.folders`, parseAssignmentAcceptanceFolder),
  };
}

/** Deepest delegation chain the host accepts, matching the backend delegation limit. */
const MAX_CHAT_DELEGATION_DEPTH = 2;

function parseAssignmentDelegation(value: unknown, label: string): ChatAssignmentDelegationRead {
  const record = readRecord(value, label);
  const depth = readNonNegativeSafeInteger(record.depth, `${label}.depth`);
  if (depth < 1 || depth > MAX_CHAT_DELEGATION_DEPTH) {
    throw new Error(`${label}.depth must be between 1 and ${MAX_CHAT_DELEGATION_DEPTH}`);
  }
  return {
    parentAssignmentId: readIdentifier(record.parentAssignmentId, `${label}.parentAssignmentId`),
    parentReplyThreadId: readNullable(record.parentReplyThreadId, `${label}.parentReplyThreadId`, readIdentifier),
    depth,
    objective: readString(record.objective, `${label}.objective`),
    expectedResult: readString(record.expectedResult, `${label}.expectedResult`),
  };
}

function parseDelegatedChild(value: unknown, label: string): ChatDelegatedChildRead {
  const record = readRecord(value, label);
  return {
    assignmentId: readIdentifier(record.assignmentId, `${label}.assignmentId`),
    parentAssignmentId: readIdentifier(record.parentAssignmentId, `${label}.parentAssignmentId`),
    replyThreadId: readIdentifier(record.replyThreadId, `${label}.replyThreadId`),
    agent: parseChatParticipant(record.agent, `${label}.agent`),
    objective: readString(record.objective, `${label}.objective`),
    state: readEnum(record.state, CHAT_WORK_ASSIGNMENT_STATES, `${label}.state`),
    stateReason: readNullable(record.stateReason, `${label}.stateReason`, readString),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
  };
}

function parseSecretRequest(value: unknown, label: string): ChatSecretRequestRead {
  const record = readRecord(value, label);
  const createdAt = readUtcTimestamp(record.createdAt, `${label}.createdAt`);
  const expiresAt = readUtcTimestamp(record.expiresAt, `${label}.expiresAt`);
  if (Date.parse(expiresAt) <= Date.parse(createdAt)) {
    throw new Error(`${label}.expiresAt must be after createdAt`);
  }
  return {
    id: readIdentifier(record.id, `${label}.id`),
    assignmentId: readIdentifier(record.assignmentId, `${label}.assignmentId`),
    label: readString(record.label, `${label}.label`),
    purpose: readString(record.purpose, `${label}.purpose`),
    createdAt,
    expiresAt,
  };
}

/** Validates the pending secret requests of one reply thread. */
export function parseChatSecretRequests(value: unknown, label = "Chat secret requests"): ChatSecretRequestRead[] {
  return readArray(value, label, parseSecretRequest);
}

function parseWaitingAssignment(value: unknown, label: string): ChatWaitingAssignmentRead {
  const record = readRecord(value, label);
  return {
    assignmentId: readIdentifier(record.assignmentId, `${label}.assignmentId`),
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    channelName: readString(record.channelName, `${label}.channelName`),
    replyThreadId: readIdentifier(record.replyThreadId, `${label}.replyThreadId`),
    agentDisplayName: readString(record.agentDisplayName, `${label}.agentDisplayName`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
  };
}

/** Validates the capped waiting list and its total, which never counts fewer than the items. */
export function parseChatWaitingAssignments(
  value: unknown,
  label = "Chat waiting assignments",
): ChatWaitingAssignmentsRead {
  const record = readRecord(value, label);
  const items = readArray(record.items, `${label}.items`, parseWaitingAssignment);
  const total = readNonNegativeSafeInteger(record.total, `${label}.total`);
  if (total < items.length) throw new Error(`${label}.total is below the item count`);
  return { items, total };
}

function parseReviewQueueItem(value: unknown, label: string): ChatReviewQueueItemRead {
  const record = readRecord(value, label);
  return {
    assignmentId: readIdentifier(record.assignmentId, `${label}.assignmentId`),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    kind: readEnum(record.kind, CHAT_REVIEW_QUEUE_KINDS, `${label}.kind`),
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    projectName: readString(record.projectName, `${label}.projectName`),
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    channelName: readString(record.channelName, `${label}.channelName`),
    replyThreadId: readIdentifier(record.replyThreadId, `${label}.replyThreadId`),
    agentParticipantId: readIdentifier(record.agentParticipantId, `${label}.agentParticipantId`),
    agentDisplayName: readString(record.agentDisplayName, `${label}.agentDisplayName`),
    stateReason: readNullable(record.stateReason, `${label}.stateReason`, readString),
    changedFileCount: readNonNegativeSafeInteger(record.changedFileCount, `${label}.changedFileCount`),
    executedHere: readBoolean(record.executedHere, `${label}.executedHere`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

/**
 * Validates the capped review queue. The total never counts fewer than the items, and `truncated` must agree with
 * whether it counts more.
 */
export function parseChatReviewQueue(value: unknown, label = "Chat review queue"): ChatReviewQueueRead {
  const record = readRecord(value, label);
  const items = readArray(record.items, `${label}.items`, parseReviewQueueItem);
  const total = readNonNegativeSafeInteger(record.total, `${label}.total`);
  const failureCount = readNonNegativeSafeInteger(record.failureCount, `${label}.failureCount`);
  const truncated = readBoolean(record.truncated, `${label}.truncated`);
  if (total < items.length) throw new Error(`${label}.total is below the item count`);
  if (truncated !== total > items.length) throw new Error(`${label}.truncated disagrees with the total`);
  if (failureCount > total) throw new Error(`${label}.failureCount exceeds the total`);
  return { items, total, failureCount, truncated };
}

/** Validates one assignment input read. */
export function parseChatAssignmentInput(value: unknown, label = "Chat assignment input"): ChatAssignmentInputRead {
  const record = readRecord(value, label);
  return {
    inputId: readIdentifier(record.inputId, `${label}.inputId`),
    messageItemId: readIdentifier(record.messageItemId, `${label}.messageItemId`),
    ordinal: readNonNegativeSafeInteger(record.ordinal, `${label}.ordinal`),
    routingKind: readEnum(record.routingKind, CHAT_ASSIGNMENT_INPUT_ROUTING_KINDS, `${label}.routingKind`),
    deliveryState: readEnum(record.deliveryState, CHAT_ASSIGNMENT_INPUT_DELIVERY_STATES, `${label}.deliveryState`),
    held: readBoolean(record.held, `${label}.held`),
    deliveredAt: readNullable(record.deliveredAt, `${label}.deliveredAt`, readUtcTimestamp),
    deliveredRevisionId: readNullable(record.deliveredRevisionId, `${label}.deliveredRevisionId`, readIdentifier),
    currentRevisionId: readNullable(record.currentRevisionId, `${label}.currentRevisionId`, readIdentifier),
    preview: readString(record.preview, `${label}.preview`),
    previewTruncated: readBoolean(record.previewTruncated, `${label}.previewTruncated`),
    authorParticipantId: readNullable(record.authorParticipantId, `${label}.authorParticipantId`, readIdentifier),
  };
}

/** Validates the inputs of one assignment, in delivery order. */
export function parseChatAssignmentInputs(value: unknown, label = "Chat assignment inputs"): ChatAssignmentInputRead[] {
  return readArray(value, label, parseChatAssignmentInput);
}

/** Validates an agent run read, whose handoff reason is set exactly when it started as a handoff. */
export function parseChatAgentRun(value: unknown, label = "Chat agent run"): ChatAgentRunRead {
  const record = readRecord(value, label);
  const attemptReason = readEnum(record.attemptReason, CHAT_AGENT_RUN_ATTEMPT_REASONS, `${label}.attemptReason`);
  const handoffReason = readNullable(
    record.handoffReason,
    `${label}.handoffReason`,
    (reason, reasonLabel) => readEnum(reason, CHAT_AGENT_RUN_HANDOFF_REASONS, reasonLabel),
  );
  if ((attemptReason === "handoff") !== (handoffReason !== null)) {
    throw new Error(`${label}.handoffReason must be set exactly when the run is a handoff`);
  }
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
    providerExecutionTurnId: readIdentifier(record.providerExecutionTurnId, `${label}.providerExecutionTurnId`),
    providerExecutionThreadId: readNullable(
      record.providerExecutionThreadId,
      `${label}.providerExecutionThreadId`,
      readIdentifier,
    ),
    state: readEnum(record.state, AGENT_RUN_STATES, `${label}.state`),
    runOrdinal: readNonNegativeSafeInteger(record.runOrdinal, `${label}.runOrdinal`),
    attemptReason,
    handoffReason,
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
    agentRuns: readArray(record.agentRuns, `${label}.agentRuns`, parseChatAgentRun),
    delegatedChildren: readArray(record.delegatedChildren, `${label}.delegatedChildren`, parseDelegatedChild),
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
    routedAs: readNullable(record.routedAs, `${label}.routedAs`, (entry, path) =>
      readEnum(entry, CHAT_FOLLOWUP_ROUTINGS, path),
    ),
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
    recurrence: readNullable(record.recurrence, `${label}.recurrence`, parseChatScheduledRecurrence),
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
  const authorParticipantId = readNullable(record.authorParticipantId, `${label}.authorParticipantId`, readIdentifier);
  const authorKind = readNullable(
    record.authorKind,
    `${label}.authorKind`,
    (kind, path) => readEnum(kind, CHAT_PARTICIPANT_KINDS, path),
  );
  if ((authorParticipantId === null) !== (authorKind === null)) {
    throw new Error(`${label}.authorKind must be present exactly when authorParticipantId is`);
  }
  return {
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    channelName: readString(record.channelName, `${label}.channelName`),
    conversationId: readIdentifier(record.conversationId, `${label}.conversationId`),
    replyThreadId: readNullable(record.replyThreadId, `${label}.replyThreadId`, readIdentifier),
    messageItemId: readIdentifier(record.messageItemId, `${label}.messageItemId`),
    anchorCursor: readString(record.anchorCursor, `${label}.anchorCursor`),
    authorParticipantId,
    authorKind,
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
