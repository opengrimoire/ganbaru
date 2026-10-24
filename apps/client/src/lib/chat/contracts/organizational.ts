import type {
  ChatAgentRunId,
  ChatAccessProfileId,
  ChatAccessProfileRevisionId,
  ChatAttachmentId,
  ChatChannelId,
  ChatCommandId,
  ChatConversationId,
  ChatConversationItemId,
  ChatMessageRevisionId,
  ChatMessageReferenceId,
  ChatParticipantId,
  ChatReplyThreadId,
  ChatScheduledMessageId,
  ChatTeammatePolicyRevisionId,
  ChatExecutionEnvironmentId,
  ChatScratchGenerationId,
  ChatScratchScopeId,
  ChatScratchPromotionId,
  ChatScratchCleanupJobId,
  ChatThreadId,
  ChatTurnId,
  ChatWorkAssignmentId,
  ModelId,
  ProjectWorkingFolderId,
  ProviderInstanceId,
  SafetyMode,
  UtcTimestamp,
  VersionedJson,
} from "./common";
import type { ModelOptionSelection } from "./provider";

export const CHAT_PARTICIPANT_KINDS = ["local_user", "ai_teammate", "human"] as const;
export type ChatParticipantKind = (typeof CHAT_PARTICIPANT_KINDS)[number];

export const CHAT_TEAMMATE_CONFIGURATION_STATES = [
  "healthy",
  "needs_setup",
  "provider_unavailable",
  "folder_access_missing",
] as const;
export type ChatTeammateConfigurationState = (typeof CHAT_TEAMMATE_CONFIGURATION_STATES)[number];

export const CHAT_WORK_ASSIGNMENT_STATES = [
  "queued",
  "working",
  "waiting_for_answer",
  "waiting_for_approval",
  "ready_for_review",
  "completed",
  "failed",
  "cancelled",
] as const;
export type ChatWorkAssignmentState = (typeof CHAT_WORK_ASSIGNMENT_STATES)[number];

export const CHAT_FOLDER_CAPABILITIES = ["none", "read", "edit", "execute", "publish"] as const;
export type ChatFolderCapability = (typeof CHAT_FOLDER_CAPABILITIES)[number];

export const CHAT_RUNTIME_APPROVAL_POLICIES = [
  "ask",
  "autoApprove",
  "unattended",
  "providerCustom",
] as const;
export type ChatRuntimeApprovalPolicy = (typeof CHAT_RUNTIME_APPROVAL_POLICIES)[number];

export interface ChatChannelCapabilities {
  readHistory: boolean;
  participate: boolean;
}

export type ChatHistoryBoundary =
  | { kind: "entire" }
  | { kind: "fromGrant"; lowerOrdinal?: number };

export const CHAT_ACCESS_PROFILE_BUILTIN_KEYS = [
  "conversationOnly",
  "readOnly",
  "editFiles",
  "buildAndTest",
  "publishChanges",
] as const;
export type ChatAccessProfileBuiltinKey = (typeof CHAT_ACCESS_PROFILE_BUILTIN_KEYS)[number];

export const CHAT_ACCESS_ISSUE_CODES = [
  "archived_channel",
  "archived_folder",
  "cross_project_folder",
  "duplicate_channel",
  "duplicate_folder",
  "multiple_default_folders",
  "profile_ceiling_exceeded",
  "profile_not_found",
  "retained_reference_disclosure",
  "teammate_not_found",
] as const;
export type ChatAccessIssueCode = (typeof CHAT_ACCESS_ISSUE_CODES)[number];

export interface ChatAccessProfileRevisionInput {
  defaultChannelCapabilities: ChatChannelCapabilities;
  defaultHistoryBoundary: ChatHistoryBoundary;
  maximumFolderCapability: ChatFolderCapability;
}

export interface ChatAccessProfileRevision {
  id: ChatAccessProfileRevisionId;
  accessProfileId: ChatAccessProfileId;
  revision: number;
  defaultChannelCapabilities: ChatChannelCapabilities;
  defaultHistoryBoundary: ChatHistoryBoundary;
  maximumFolderCapability: ChatFolderCapability;
  createdAt: UtcTimestamp;
}

export interface ChatAccessProfileRead {
  id: ChatAccessProfileId;
  builtinKey: ChatAccessProfileBuiltinKey | null;
  displayName: string;
  latestRevision: ChatAccessProfileRevision;
  revision: number;
  archivedAt: UtcTimestamp | null;
}

export interface CreateChatAccessProfileRequest {
  accessProfileId: ChatAccessProfileId;
  displayName: string;
  revision: ChatAccessProfileRevisionInput;
}

export interface DuplicateChatAccessProfileRequest {
  sourceAccessProfileId: ChatAccessProfileId;
  accessProfileId: ChatAccessProfileId;
  displayName: string;
}

export interface PublishChatAccessProfileRevisionRequest {
  accessProfileId: ChatAccessProfileId;
  expectedRevision: number;
  revision: ChatAccessProfileRevisionInput;
}

export interface ArchiveChatAccessProfileRequest {
  accessProfileId: ChatAccessProfileId;
  expectedRevision: number;
  archived: boolean;
}

export interface ChatFolderGrant {
  workingFolderId: ProjectWorkingFolderId;
  displayName: string;
  capability: ChatFolderCapability;
  isDefault: boolean;
  runtimeApprovalOverride: ChatRuntimeApprovalPolicy | null;
  revision: number;
  revokedAt: UtcTimestamp | null;
}

export interface ChatTeammateChannelAccess {
  channelId: ChatChannelId;
  conversationId: ChatConversationId;
  projectId: string;
  groupId: string;
  channelName: string;
  accessProfileId: ChatAccessProfileId;
  accessProfileRevision: number;
  capabilities: ChatChannelCapabilities;
  historyBoundary: ChatHistoryBoundary;
  runtimeApprovalOverride: ChatRuntimeApprovalPolicy | null;
  scratchRuntimeApprovalOverride: ChatRuntimeApprovalPolicy | null;
  folderGrants: ChatFolderGrant[];
  membershipRevision: number;
  removedAt: UtcTimestamp | null;
}

export interface ChatTeammateAccessRead {
  teammateId: ChatParticipantId;
  accessRevision: number;
  teammateDefaultRuntimeApproval: ChatRuntimeApprovalPolicy;
  channels: ChatTeammateChannelAccess[];
}

export interface ChatTeammateChannelAccessInput {
  channelId: ChatChannelId;
  accessProfileId: ChatAccessProfileId;
  accessProfileRevision: number;
  capabilities: ChatChannelCapabilities;
  historyBoundary: ChatHistoryBoundary;
  runtimeApprovalOverride: ChatRuntimeApprovalPolicy | null;
  scratchRuntimeApprovalOverride: ChatRuntimeApprovalPolicy | null;
  folderGrants: Array<{
    workingFolderId: ProjectWorkingFolderId;
    capability: ChatFolderCapability;
    isDefault: boolean;
    runtimeApprovalOverride: ChatRuntimeApprovalPolicy | null;
  }>;
}

export interface ReplaceChatTeammateAccessRequest {
  teammateId: ChatParticipantId;
  expectedAccessRevision: number;
  channels: ChatTeammateChannelAccessInput[];
  teammateProfile?: UpdateChatTeammateProfileRequest | null;
  policy?: ChatTeammatePolicyInput | null;
}

export interface ChatTeammateAccessPreviewIssue {
  code: ChatAccessIssueCode;
  fieldPath: string;
  message: string;
}

export interface ChatTeammateAccessPreviewRead {
  proposed: ChatTeammateAccessRead | null;
  isExpansion: boolean;
  addedChannelIds: ChatChannelId[];
  removedChannelIds: ChatChannelId[];
  issues: ChatTeammateAccessPreviewIssue[];
}

export interface ChatAccessProfileImpactPreviewRead {
  accessProfileId: ChatAccessProfileId;
  currentRevision: ChatAccessProfileRevision;
  isExpansion: boolean;
  isReduction: boolean;
  affectedTeammateIds: ChatParticipantId[];
  affectedChannelIds: ChatChannelId[];
  activeAuthorizationCount: number;
  issues: ChatTeammateAccessPreviewIssue[];
}

export type ChatExecutionTarget =
  | {
      kind: "scratch";
      scratchScopeId: string;
      scratchGenerationId: ChatScratchGenerationId;
      executionEnvironmentId: ChatExecutionEnvironmentId;
    }
  | {
      kind: "workingFolder";
      workingFolderId: ProjectWorkingFolderId;
      executionEnvironmentId: ChatExecutionEnvironmentId;
    };

export const CHAT_ASSIGNMENT_TARGET_KINDS = ["scratch", "currentFolder", "worktree"] as const;
export type ChatAssignmentTargetKind = (typeof CHAT_ASSIGNMENT_TARGET_KINDS)[number];

export const CHAT_ASSIGNMENT_TARGET_BINDING_STATES = [
  "ready",
  "locate",
  "relink",
  "clone",
  "missing",
  "unavailableOnThisDevice",
] as const;
export type ChatAssignmentTargetBindingState = (typeof CHAT_ASSIGNMENT_TARGET_BINDING_STATES)[number];

export const CHAT_ASSIGNMENT_TARGET_LIFECYCLE_STATES = [
  "creating",
  "available",
  "missing",
  "cleanup_pending",
  "cleanup_failed",
  "removed",
] as const;
export type ChatAssignmentTargetLifecycleState = (typeof CHAT_ASSIGNMENT_TARGET_LIFECYCLE_STATES)[number];

export interface ChatAssignmentTargetRead {
  executionTarget: ChatExecutionTarget | null;
  kind: ChatAssignmentTargetKind;
  displayName: string;
  folderCapability: ChatFolderCapability | null;
  effectiveRuntimeApproval: ChatRuntimeApprovalPolicy;
  isDefault: boolean;
  bindingState: ChatAssignmentTargetBindingState;
  lifecycleState: ChatAssignmentTargetLifecycleState;
  isBusy: boolean;
  isDirty: boolean | null;
  eligible: boolean;
  unavailableReason: string | null;
}

export interface ListChatAssignmentTargetsRequest {
  teammateId: ChatParticipantId;
  channelId: ChatChannelId;
  replyThreadId: ChatReplyThreadId | null;
}

export const CHAT_SCRATCH_SCOPE_LIFECYCLE_STATES = [
  "active",
  "archived",
  "cleanupPending",
  "cleanupFailed",
  "removed",
] as const;
export type ChatScratchScopeLifecycleState =
  (typeof CHAT_SCRATCH_SCOPE_LIFECYCLE_STATES)[number];

export const CHAT_SCRATCH_GENERATION_LIFECYCLE_STATES = [
  "active",
  "quarantined",
  "cleanupPending",
  "cleanupFailed",
  "removed",
] as const;
export type ChatScratchGenerationLifecycleState =
  (typeof CHAT_SCRATCH_GENERATION_LIFECYCLE_STATES)[number];

export const CHAT_SCRATCH_DEVICE_AVAILABILITIES = [
  "available",
  "unavailableOnThisDevice",
] as const;
export type ChatScratchDeviceAvailability =
  (typeof CHAT_SCRATCH_DEVICE_AVAILABILITIES)[number];

export interface ChatScratchSourceSummaryRead {
  channelId: ChatChannelId;
  channelName: string;
  lowerOrdinal: number;
  highOrdinal: number;
  audienceRevision: number;
}

export interface ChatScratchGenerationRead {
  id: ChatScratchGenerationId;
  executionEnvironmentId: ChatExecutionEnvironmentId;
  generation: number;
  lifecycleState: ChatScratchGenerationLifecycleState;
  byteSize: number;
  entryCount: number;
  sizeTruncated: boolean;
  deviceAvailability: ChatScratchDeviceAvailability;
  retainedSources: ChatScratchSourceSummaryRead[];
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
}

export interface ChatScratchScopeRead {
  id: ChatScratchScopeId;
  replyThreadId: ChatReplyThreadId;
  teammateId: ChatParticipantId;
  teammateName: string;
  channelId: ChatChannelId;
  channelName: string;
  projectId: string;
  projectName: string;
  groupId: string;
  groupName: string;
  lifecycleState: ChatScratchScopeLifecycleState;
  revision: number;
  generations: ChatScratchGenerationRead[];
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
}

export const CHAT_SCRATCH_DIRECTORY_ENTRY_KINDS = ["directory", "file"] as const;
export type ChatScratchDirectoryEntryKind =
  (typeof CHAT_SCRATCH_DIRECTORY_ENTRY_KINDS)[number];

export interface ChatScratchDirectoryEntryRead {
  relativePath: string;
  displayName: string;
  kind: ChatScratchDirectoryEntryKind;
  byteSize: number | null;
  contentRevision: string | null;
  promotable: boolean;
}

export interface ChatScratchDirectoryPageRead {
  scratchGenerationId: ChatScratchGenerationId;
  relativePath: string;
  entries: ChatScratchDirectoryEntryRead[];
  nextCursor: string | null;
}

export type ChatScratchPromotionDestinationRead =
  | {
      kind: "workingFolder";
      workingFolderId: ProjectWorkingFolderId;
      relativePath: string;
    }
  | {
      kind: "managedAttachment";
      channelId: ChatChannelId;
      attachmentId: ChatAttachmentId;
    };

export interface ChatScratchPromotionResultRead {
  id: ChatScratchPromotionId;
  scratchGenerationId: ChatScratchGenerationId;
  sourceRelativePath: string;
  sourceSha256: string;
  destination: ChatScratchPromotionDestinationRead;
  createdAt: UtcTimestamp;
}

export interface ChatScratchCleanupPreviewRead {
  scratchScopeId: ChatScratchScopeId;
  scratchGenerationId: ChatScratchGenerationId;
  expectedScopeRevision: number;
  lifecycleState: ChatScratchGenerationLifecycleState;
  byteSize: number;
  entryCount: number;
  sizeTruncated: boolean;
  deviceAvailability: ChatScratchDeviceAvailability;
  activeRunCount: number;
  willRemoveScope: boolean;
}

export const CHAT_SCRATCH_CLEANUP_JOB_STATES = [
  "pending",
  "running",
  "completed",
  "failed",
  "unavailableOnThisDevice",
] as const;
export type ChatScratchCleanupJobState =
  (typeof CHAT_SCRATCH_CLEANUP_JOB_STATES)[number];

export interface ChatScratchCleanupResultRead {
  jobId: ChatScratchCleanupJobId;
  scratchScopeId: ChatScratchScopeId;
  scratchGenerationId: ChatScratchGenerationId;
  scopeRevision: number;
  state: ChatScratchCleanupJobState;
  removedBytes: number;
  deviceAvailability: ChatScratchDeviceAvailability;
  completedAt: UtcTimestamp | null;
}

export interface BrowseChatScratchGenerationRequest {
  scratchGenerationId: ChatScratchGenerationId;
  relativePath: string;
  cursor: string | null;
  limit: number;
  allowRestrictedInspection: boolean;
}

export type ChatScratchPromotionDestinationInput =
  | {
      kind: "workingFolder";
      channelId: ChatChannelId;
      workingFolderId: ProjectWorkingFolderId;
      relativePath: string;
    }
  | {
      kind: "managedAttachment";
      channelId: ChatChannelId;
      attachmentId: ChatAttachmentId;
      displayName: string;
    };

export interface PromoteChatScratchFileRequest {
  promotionId: ChatScratchPromotionId;
  scratchGenerationId: ChatScratchGenerationId;
  sourceRelativePath: string;
  expectedContentRevision: string;
  destination: ChatScratchPromotionDestinationInput;
}

export interface PreviewChatScratchCleanupRequest {
  scratchGenerationId: ChatScratchGenerationId;
}

export interface CleanupChatScratchRequest {
  scratchGenerationId: ChatScratchGenerationId;
  expectedScopeRevision: number;
  confirmed: boolean;
}

export interface ChatParticipantRead {
  id: ChatParticipantId;
  kind: ChatParticipantKind;
  displayName: string;
  avatar: VersionedJson;
  revision: number;
  archivedAt: UtcTimestamp | null;
}

export interface ChatTeammatePolicyRead {
  id: ChatTeammatePolicyRevisionId;
  teammateId: ChatParticipantId;
  revision: number;
  providerInstanceId: ProviderInstanceId;
  safetyMode: SafetyMode;
  providerManagedModel: boolean;
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
  effort: string | null;
  speed: string | null;
  providerOptions: VersionedJson;
  createdAt: UtcTimestamp;
}

export interface ChatAiTeammateRead {
  participant: ChatParticipantRead;
  role: string;
  instructions: string;
  configurationState: ChatTeammateConfigurationState;
  latestPolicy: ChatTeammatePolicyRead | null;
  channelCount: number;
  activeAssignmentCount: number;
}

export interface ChatConversationMembershipRead {
  conversationId: ChatConversationId;
  participant: ChatParticipantRead;
  aiAccess: ChatChannelRosterAiSummary | null;
  revision: number;
  removedAt: UtcTimestamp | null;
}

export interface ChatChannelRosterAiSummary {
  accessProfileId: ChatAccessProfileId;
  accessProfileRevision: number;
  accessProfileBuiltinKey: ChatAccessProfileBuiltinKey | null;
  accessProfileName: string;
  capabilities: ChatChannelCapabilities;
  historyBoundary: ChatHistoryBoundary;
  runtimeApprovalOverride: ChatRuntimeApprovalPolicy | null;
  scratchRuntimeApprovalOverride: ChatRuntimeApprovalPolicy | null;
  folderGrants: ChatFolderGrant[];
}

export interface ChatChannelRosterRead {
  channelId: ChatChannelId;
  conversationId: ChatConversationId;
  audienceRevision: number;
  memberships: ChatConversationMembershipRead[];
}

export interface PreviewChatChannelMembershipRemovalRequest {
  teammateId: ChatParticipantId;
  channelId: ChatChannelId;
  expectedAccessRevision: number;
}

export interface ChatChannelMembershipRemovalPreview {
  teammateId: ChatParticipantId;
  channelId: ChatChannelId;
  activeAssignmentCount: number;
  activeAuthorizationCount: number;
  willRevokeActiveWork: boolean;
  proposedAccess: ChatTeammateAccessRead;
}

export interface ChatProjectPrimaryWorkingFolderRead {
  projectId: string;
  workingFolderId: ProjectWorkingFolderId;
  revision: number;
}

export interface ChatReferenceMetadata {
  referenceId: ChatMessageReferenceId;
  labelSnapshot: string;
  startOffset: number;
  endOffset: number;
  plainTextProjection: string;
}

export type ChatWorkspacePathKind = "file" | "folder";

export type ChatMessageReference =
  | {
      kind: "participant";
      metadata: ChatReferenceMetadata;
      participantId: ChatParticipantId;
      participantKind: ChatParticipantKind;
    }
  | {
      kind: "channel";
      metadata: ChatReferenceMetadata;
      channelId: ChatChannelId;
    }
  | {
      kind: "workingFolder";
      metadata: ChatReferenceMetadata;
      workingFolderId: ProjectWorkingFolderId;
    }
  | {
      kind: "workspacePath";
      metadata: ChatReferenceMetadata;
      workingFolderId: ProjectWorkingFolderId;
      pathKind: ChatWorkspacePathKind;
      relativePath: string;
    }
  | {
      kind: "executionEnvironment";
      metadata: ChatReferenceMetadata;
      executionEnvironmentId: ChatExecutionEnvironmentId;
    };

export interface ChatReplyThreadSummaryRead {
  id: ChatReplyThreadId;
  replyCount: number;
  lastActivityAt: UtcTimestamp;
  participants: ChatParticipantRead[];
  unread: boolean;
  workState: ChatWorkAssignmentState | null;
}

export interface ChatMessageRead {
  itemId: ChatConversationItemId;
  conversationId: ChatConversationId;
  replyThreadId: ChatReplyThreadId | null;
  revisionId: ChatMessageRevisionId;
  revision: number;
  author: ChatParticipantRead;
  authorLabelSnapshot: string;
  normalizedMarkdown: string;
  richContent: VersionedJson;
  attachmentIds: ChatAttachmentId[];
  references: ChatMessageReference[];
  replyThread: ChatReplyThreadSummaryRead | null;
  ordinal: number;
  editedAt: UtcTimestamp | null;
  createdAt: UtcTimestamp;
}

export interface ChatChannelPageRead {
  channelId: ChatChannelId;
  messages: ChatMessageRead[];
  previousCursor: string | null;
  revision: number;
}

export interface ChatWorkAssignmentRead {
  id: ChatWorkAssignmentId;
  replyThreadId: ChatReplyThreadId;
  teammate: ChatParticipantRead;
  triggeringMessageItemId: ChatConversationItemId;
  previousAssignmentId: ChatWorkAssignmentId | null;
  state: ChatWorkAssignmentState;
  stateReason: string | null;
  revision: number;
  settledAt: UtcTimestamp | null;
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
}

export interface ChatAgentRunRead {
  id: ChatAgentRunId;
  assignmentId: ChatWorkAssignmentId;
  projectId: string;
  workingFolderId: ProjectWorkingFolderId | null;
  executionEnvironmentId: ChatExecutionEnvironmentId;
  scratchGenerationId: ChatScratchGenerationId | null;
  teammatePolicyRevisionId: ChatTeammatePolicyRevisionId;
  effort: string | null;
  providerExecutionTurnId: ChatTurnId;
  providerExecutionThreadId: ChatThreadId | null;
  state: "queued" | "starting" | "working" | "waiting" | "completed" | "failed" | "cancelled";
  runOrdinal: number;
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
}

export interface ChatReplyThreadPageRead {
  thread: ChatReplyThreadSummaryRead;
  rootMessage: ChatMessageRead;
  replies: ChatMessageRead[];
  assignment: ChatWorkAssignmentRead | null;
  agentRuns: ChatAgentRunRead[];
  previousCursor: string | null;
  revision: number;
}

export interface ChatMessageSearchResultRead {
  projectId: string;
  channelId: ChatChannelId;
  channelName: string;
  conversationId: ChatConversationId;
  replyThreadId: ChatReplyThreadId | null;
  messageItemId: ChatConversationItemId;
  ordinal: number;
  authorParticipantId: ChatParticipantId;
  authorKind: ChatParticipantKind;
  authorDisplayName: string;
  excerpt: string;
  createdAt: UtcTimestamp;
}

export interface ChatTeammatePolicyInput {
  providerInstanceId: ProviderInstanceId;
  safetyMode: SafetyMode;
  providerManagedModel: boolean;
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
  effort: string | null;
  speed: string | null;
  providerOptions: VersionedJson;
}

export interface CreateChatTeammateRequest {
  teammateId: ChatParticipantId;
  displayName: string;
  avatar: VersionedJson;
  role: string;
  instructions: string;
  policy: ChatTeammatePolicyInput;
}

export interface UpdateChatTeammateProfileRequest {
  teammateId: ChatParticipantId;
  displayName: string;
  avatar: VersionedJson;
  role: string;
  instructions: string;
  expectedRevision: number;
}

export interface PostChatMessageRequest {
  clientCommandId: ChatCommandId;
  channelId: ChatChannelId;
  replyThreadId: ChatReplyThreadId | null;
  normalizedMarkdown: string;
  richContent: VersionedJson;
  attachmentIds: ChatAttachmentId[];
  references: ChatMessageReference[];
  executionTarget: ChatExecutionTarget | null;
  alsoSendToChannel: boolean;
}

export interface PostChatMessageResult {
  message: ChatMessageRead;
  replyThreadId: ChatReplyThreadId | null;
  assignment: ChatWorkAssignmentRead | null;
  assignmentInputQueued: boolean;
}

export const CHAT_SCHEDULED_MESSAGE_STATES = ["scheduled", "dispatching", "failed"] as const;
export type ChatScheduledMessageState = (typeof CHAT_SCHEDULED_MESSAGE_STATES)[number];

export interface ScheduleChatMessageRequest {
  scheduledMessageId: ChatScheduledMessageId;
  scheduledFor: UtcTimestamp;
  message: PostChatMessageRequest;
}

export interface ChatScheduledMessageRead {
  id: ChatScheduledMessageId;
  channelId: ChatChannelId;
  replyThreadId: ChatReplyThreadId | null;
  normalizedMarkdown: string;
  richContent: VersionedJson;
  attachmentIds: ChatAttachmentId[];
  references: ChatMessageReference[];
  alsoSendToChannel: boolean;
  state: ChatScheduledMessageState;
  scheduledFor: UtcTimestamp;
  lastError: string | null;
  createdAt: UtcTimestamp;
}

export interface ChatScheduledMessageDispatchRead {
  processedCount: number;
  dispatchedCount: number;
  dispatchedChannelIds: ChatChannelId[];
  nextDispatchAt: UtcTimestamp | null;
}
