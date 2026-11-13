import type {
  ChatAgentRunId,
  ChatAccessProfileId,
  ChatAccessProfileRevisionId,
  ChatAssignmentInputId,
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
import type { ChatRecurrenceSpec, ChatScheduledRecurrenceRead } from "./scheduling";

export const CHAT_PARTICIPANT_KINDS = ["local_user", "ai_teammate", "human"] as const;
export type ChatParticipantKind = (typeof CHAT_PARTICIPANT_KINDS)[number];

/** How a reply reaches active work: `steer` into the running turn, or `queue` after it ends. */
export const CHAT_FOLLOWUP_ROUTINGS = ["steer", "queue"] as const;
export type ChatFollowupRouting = (typeof CHAT_FOLLOWUP_ROUTINGS)[number];

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

export const CHAT_ASSIGNMENT_REVIEW_OUTCOMES = ["accepted", "changes_requested"] as const;
export type ChatAssignmentReviewOutcome = (typeof CHAT_ASSIGNMENT_REVIEW_OUTCOMES)[number];

/** Why an agent run started its provider session the way it did. */
export const CHAT_AGENT_RUN_ATTEMPT_REASONS = ["fresh", "continuation", "handoff", "retry"] as const;
export type ChatAgentRunAttemptReason = (typeof CHAT_AGENT_RUN_ATTEMPT_REASONS)[number];

/** Why a handoff run left the earlier provider session. */
export const CHAT_AGENT_RUN_HANDOFF_REASONS = [
  "key_changed", "authority_revoked", "native_session_lost",
] as const;
export type ChatAgentRunHandoffReason = (typeof CHAT_AGENT_RUN_HANDOFF_REASONS)[number];

/** How a person ended ready-for-review work, recorded only on completed assignments. */
export interface ChatAssignmentReview {
  outcome: ChatAssignmentReviewOutcome;
  reviewedAt: UtcTimestamp;
  reviewedByParticipantId: ChatParticipantId;
}

/** A review of ready-for-review work: Accept completes it, Request changes posts the note as a follow-up reply. */
export type ChatAssignmentReviewRequest =
  | { kind: "accept" }
  | { kind: "requestChanges"; note: string };

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
  | { kind: "fromGrant"; lowerOrderKey?: string };

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

/** Token and working time limits of one assignment with what its executing device recorded as used. */
export interface ChatAssignmentBudgetRead {
  assignmentId: ChatWorkAssignmentId;
  tokenLimit: number | null;
  workingSecondsLimit: number | null;
  /** The person who set the limits; null when a teammate default or a delegating parent gave them. */
  setByParticipantId: ChatParticipantId | null;
  setAt: UtcTimestamp;
  /** Input, cache write, and output tokens, leaving out cache reads. */
  usedCountedTokens: number;
  /** Seconds runs spent working, leaving out time waiting for a person. */
  usedWorkingSeconds: number;
}

/** Token and working time limits for one assignment; at least one is set. */
export interface ChatAssignmentBudgetInput {
  tokenLimit: number | null;
  workingSecondsLimit: number | null;
}

/** One local day of a teammate's usage, summed across the devices that ran it. */
export interface ChatTeammateUsageDayRead {
  /** The day in `YYYY-MM-DD` form. */
  localDate: string;
  countedTokens: number;
  workingSeconds: number;
  runCount: number;
  deviceIds: string[];
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
  /** Group of the channel's project, `null` when the project is outside every group. */
  groupId: string | null;
  channelName: string;
  accessProfileId: ChatAccessProfileId;
  accessProfileRevisionId: ChatAccessProfileRevisionId;
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
  /** Effective profile revision the draft was made against; saving refuses any other. */
  accessProfileRevisionId: ChatAccessProfileRevisionId;
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
  lowerOrderKey: string;
  highOrderKey: string;
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
  /** Group of the channel's project, `null` when the project is outside every group. */
  groupId: string | null;
  /** Name of that group, `null` with it. */
  groupName: string | null;
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

/** Why a scratch generation is safe to reclaim, in reporting priority order. */
export const CHAT_SCRATCH_RECLAIM_REASONS = [
  "scopeArchived",
  "scopeCleanupFailed",
  "generationCleanupFailed",
  "generationQuarantined",
  "idle",
] as const;
export type ChatScratchReclaimReason = (typeof CHAT_SCRATCH_RECLAIM_REASONS)[number];

/** One scratch generation that the existing confirmed cleanup can remove. */
export interface ChatScratchReclaimRow {
  scratchScopeId: ChatScratchScopeId;
  scratchGenerationId: ChatScratchGenerationId;
  replyThreadId: ChatReplyThreadId;
  generation: number;
  teammateName: string;
  channelName: string;
  projectName: string;
  groupName: string | null;
  reason: ChatScratchReclaimReason;
  /** Bounded size on this device, or the stored size when unavailable here. */
  byteSize: number;
  sizeTruncated: boolean;
  deviceAvailability: ChatScratchDeviceAvailability;
  lastActivityAt: UtcTimestamp;
}

/** Largest reclaimable scratch generations with totals over every qualifying generation. */
export interface ChatScratchReclaimReportRead {
  rows: ChatScratchReclaimRow[];
  /** Sum of the sizes available on this device across every qualifying generation. */
  totalBytes: number;
  totalTruncated: boolean;
  /** Qualifying generations left out of `rows` by the row limit. */
  omitted: number;
  /** Days without activity after which active scratch counts as idle. */
  idleDays: number;
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
  /** Token limit a new assignment of this teammate starts with when its post sets no budget. */
  defaultAssignmentTokenLimit: number | null;
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
  /** `null` when the project has no usable folder on this replica. */
  workingFolderId: ProjectWorkingFolderId | null;
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
  /** Author identity, null when the message carries only its author label snapshot. */
  author: ChatParticipantRead | null;
  authorLabelSnapshot: string;
  normalizedMarkdown: string;
  richContent: VersionedJson;
  attachmentIds: ChatAttachmentId[];
  references: ChatMessageReference[];
  replyThread: ChatReplyThreadSummaryRead | null;
  /** Position within its scope; messages order by `(orderKey, itemId)`. */
  orderKey: string;
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
  /** Where this work came from when another teammate delegated it; null for work a person started. */
  delegation: ChatAssignmentDelegationRead | null;
  revision: number;
  settledAt: UtcTimestamp | null;
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
  /** How a person ended the work, present only when it is completed after review. */
  review: ChatAssignmentReview | null;
  /** What starting this work here would run, present while it waits for a local action on the reading device. */
  localAcceptance: ChatAssignmentAcceptanceOfferRead | null;
  /** Chat thread of the newest run of this reply thread on the reading device, used to show its usage; null when none ran here. */
  latestRunProviderThreadId: ChatThreadId | null;
  /** Token and working time limits of this work with what it used; null when it runs without a limit. */
  budget: ChatAssignmentBudgetRead | null;
}

/** Where delegated work came from and what it was asked to return. */
export interface ChatAssignmentDelegationRead {
  parentAssignmentId: ChatWorkAssignmentId;
  /** Reply thread of the parent work; null while the parent has not reached this device. */
  parentReplyThreadId: ChatReplyThreadId | null;
  /** How many delegations separate this work from work a person started, from 1. */
  depth: number;
  objective: string;
  expectedResult: string;
}

/** Work a teammate delegated from an assignment, with its live state. */
export interface ChatDelegatedChildRead {
  assignmentId: ChatWorkAssignmentId;
  parentAssignmentId: ChatWorkAssignmentId;
  replyThreadId: ChatReplyThreadId;
  teammate: ChatParticipantRead;
  objective: string;
  state: ChatWorkAssignmentState;
  stateReason: string | null;
  createdAt: UtcTimestamp;
}

/** A secret value a running teammate asked for, open until a person provides or declines it or it expires. */
export interface ChatSecretRequestRead {
  id: string;
  assignmentId: ChatWorkAssignmentId;
  label: string;
  purpose: string;
  createdAt: UtcTimestamp;
  expiresAt: UtcTimestamp;
}

/** A folder an assignment waiting for local acceptance would run in. */
export interface ChatAssignmentAcceptanceFolderRead {
  displayName: string;
  capability: ChatFolderCapability;
  runtimeApproval: ChatRuntimeApprovalPolicy;
}

/** What the user accepts by starting work here. */
export interface ChatAssignmentAcceptanceOfferRead {
  /** Identifies these exact details; Start here passes it back so a change refreshes the offer. */
  digest: string;
  providerInstanceId: ProviderInstanceId;
  folders: ChatAssignmentAcceptanceFolderRead[];
  scratchRuntimeApproval: ChatRuntimeApprovalPolicy;
}

/** Work that waits for a local action on this device, wherever it sits in the vault. */
export interface ChatWaitingAssignmentRead {
  assignmentId: ChatWorkAssignmentId;
  projectId: string;
  channelId: ChatChannelId;
  channelName: string;
  replyThreadId: ChatReplyThreadId;
  teammateDisplayName: string;
  createdAt: UtcTimestamp;
}

/** The waiting list, capped, with how much waits in all. */
export interface ChatWaitingAssignmentsRead {
  items: ChatWaitingAssignmentRead[];
  /** Every waiting assignment, including those past the cap. */
  total: number;
}

/** What a review queue item asks of the person, in priority order. */
export const CHAT_REVIEW_QUEUE_KINDS = ["answer", "approval", "review", "failure"] as const;
/**
 * `answer` and `approval` wait for the person's reply, `review` is finished work to review, and `failure` is recent
 * work that failed with nothing continuing it.
 */
export type ChatReviewQueueKind = (typeof CHAT_REVIEW_QUEUE_KINDS)[number];

/** One assignment that waits for the person, in any channel of any project. */
export interface ChatReviewQueueItemRead {
  assignmentId: ChatWorkAssignmentId;
  /** The assignment revision the item was read at, passed back by accept and retry. */
  revision: number;
  kind: ChatReviewQueueKind;
  projectId: string;
  projectName: string;
  channelId: ChatChannelId;
  channelName: string;
  replyThreadId: ChatReplyThreadId;
  teammateParticipantId: ChatParticipantId;
  teammateDisplayName: string;
  /** The assignment state reason, clipped for display. */
  stateReason: string | null;
  /** Distinct files the assignment's review updates report as changed. */
  changedFileCount: number;
  /** Whether this device executed the assignment. */
  executedHere: boolean;
  updatedAt: UtcTimestamp;
}

/** The review queue: the first items by priority and recency, with how many match in all. */
export interface ChatReviewQueueRead {
  items: ChatReviewQueueItemRead[];
  /** Every matching item, including those past the cap. */
  total: number;
  /** Whether `total` exceeds the items read. */
  truncated: boolean;
}

export const CHAT_ASSIGNMENT_INPUT_ROUTING_KINDS =["steer", "queued_continuation", "follow_up"] as const;
/** How a reply reaches running or finished work: into the current turn, after it, or as new work. */
export type ChatAssignmentInputRoutingKind = (typeof CHAT_ASSIGNMENT_INPUT_ROUTING_KINDS)[number];

export const CHAT_ASSIGNMENT_INPUT_DELIVERY_STATES = ["pending", "delivered", "failed", "ignored"] as const;
export type ChatAssignmentInputDeliveryState = (typeof CHAT_ASSIGNMENT_INPUT_DELIVERY_STATES)[number];

/** A reply addressed to running work, editable and cancellable while it is pending. */
export interface ChatAssignmentInputRead {
  inputId: ChatAssignmentInputId;
  messageItemId: ChatConversationItemId;
  ordinal: number;
  routingKind: ChatAssignmentInputRoutingKind;
  deliveryState: ChatAssignmentInputDeliveryState;
  deliveredAt: UtcTimestamp | null;
  /** The message revision the run received, present once the input is delivered. */
  deliveredRevisionId: ChatMessageRevisionId | null;
  currentRevisionId: ChatMessageRevisionId | null;
  preview: string;
  /** Whether the message continues past the preview. */
  previewTruncated: boolean;
  authorParticipantId: ChatParticipantId | null;
}

/** New text for a pending assignment input, which replaces its message's current revision. */
export interface ChatAssignmentInputEditRequest {
  normalizedMarkdown: string;
  richContent: VersionedJson;
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
  attemptReason: ChatAgentRunAttemptReason;
  /** Set only when `attemptReason` is `handoff`. */
  handoffReason: ChatAgentRunHandoffReason | null;
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
}

export interface ChatReplyThreadPageRead {
  thread: ChatReplyThreadSummaryRead;
  rootMessage: ChatMessageRead;
  replies: ChatMessageRead[];
  assignment: ChatWorkAssignmentRead | null;
  agentRuns: ChatAgentRunRead[];
  /** Work delegated from this reply thread's assignments, oldest first, with its live state. */
  delegatedChildren: ChatDelegatedChildRead[];
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
  /** Channel page cursor that ends at the result, or at its reply thread root. */
  anchorCursor: string;
  /** Author identity, null when the message carries only its author label snapshot. */
  authorParticipantId: ChatParticipantId | null;
  /** Author kind, null together with the author identity. */
  authorKind: ChatParticipantKind | null;
  /** Author display name, or the author label snapshot when the identity is null. */
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
  /** Token limit a new assignment starts with when its post sets no budget; null for none. */
  defaultAssignmentTokenLimit: number | null;
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
  /** How a reply to a thread with active work reaches it; absent steers working work and queues anything else. Scheduled messages refuse it. */
  followupRouting?: ChatFollowupRouting;
  /** Model for the new work this post starts, over the teammate's default. A post that reaches active work, or a scheduled message, refuses it. */
  modelOverride?: ChatAssignmentModelOverride;
  /** Budget for the new work this post starts, over the teammate's default. A post that reaches active work, or a scheduled message, refuses it. */
  budget?: ChatAssignmentBudgetInput;
}

/** Model selection one assignment runs with instead of its teammate's default; effort is one of the model options. */
export interface ChatAssignmentModelOverride {
  providerManagedModel: boolean;
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
}

export interface PostChatMessageResult {
  message: ChatMessageRead;
  replyThreadId: ChatReplyThreadId | null;
  assignment: ChatWorkAssignmentRead | null;
  assignmentInputQueued: boolean;
  /** How the reply reached active work, when it became an input of it; a steer on work that is not working reports `queue`. */
  routedAs: ChatFollowupRouting | null;
}

export const CHAT_SCHEDULED_MESSAGE_STATES = ["scheduled", "dispatching", "failed"] as const;
export type ChatScheduledMessageState = (typeof CHAT_SCHEDULED_MESSAGE_STATES)[number];

export interface ScheduleChatMessageRequest {
  scheduledMessageId: ChatScheduledMessageId;
  scheduledFor: UtcTimestamp;
  message: PostChatMessageRequest;
  /** Repeat rule anchored at `scheduledFor`; `null` sends the message once. */
  recurrence: ChatRecurrenceSpec | null;
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
  /** Repeat rule and progress, or `null` for a one-time message. */
  recurrence: ChatScheduledRecurrenceRead | null;
}

export interface ChatScheduledMessageDispatchRead {
  processedCount: number;
  dispatchedCount: number;
  dispatchedChannelIds: ChatChannelId[];
  nextDispatchAt: UtcTimestamp | null;
}
