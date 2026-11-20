import type {
  ChatAgentRunId,
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
  ChatAgentPolicyRevisionId,
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
  InteractionMode,
  ProviderFamilyId,
  UtcTimestamp,
  VersionedJson,
} from "./common";
import type { ModelOptionSelection } from "./provider";
import type { ChatRecurrenceSpec, ChatScheduledRecurrenceRead } from "./scheduling";

export const CHAT_PARTICIPANT_KINDS = ["local_user", "ai_agent", "human"] as const;
export type ChatParticipantKind = (typeof CHAT_PARTICIPANT_KINDS)[number];

/** How a reply reaches active work: `steer` into the running turn, or `queue` after it ends. */
export const CHAT_FOLLOWUP_ROUTINGS = ["steer", "queue"] as const;
export type ChatFollowupRouting = (typeof CHAT_FOLLOWUP_ROUTINGS)[number];

/** What an agent stands for: a provider family, a built-in model line such as Opus, or a named agent. */
export const CHAT_AGENT_KINDS = ["line", "model", "custom"] as const;
export type ChatAgentKind = (typeof CHAT_AGENT_KINDS)[number];

export const CHAT_AGENT_CONFIGURATION_STATES = [
  "healthy",
  "needs_setup",
  "provider_unavailable",
  "folder_access_missing",
] as const;
export type ChatAgentConfigurationState = (typeof CHAT_AGENT_CONFIGURATION_STATES)[number];

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

/** Runtime approval after channel access is resolved: a person approves each command, or confined commands run without asking. */
export const CHAT_RUNTIME_APPROVAL_POLICIES = ["ask", "autoApprove"] as const;
export type ChatRuntimeApprovalPolicy = (typeof CHAT_RUNTIME_APPROVAL_POLICIES)[number];

/** How agents may work in a channel, from narrowest to widest, set at the app, project, or channel scope. */
export const CHAT_AGENT_ACCESS_LEVELS = ["talk", "read", "ask", "own"] as const;
export type ChatAgentAccess = (typeof CHAT_AGENT_ACCESS_LEVELS)[number];

/** The scope an effective access comes from. */
export const CHAT_AGENT_ACCESS_SOURCES = ["channel", "project", "app"] as const;
export type ChatAgentAccessSource = (typeof CHAT_AGENT_ACCESS_SOURCES)[number];

/** What one agent actually does on this device, from narrowest to widest; `edit_only` edits files without running commands. */
export const CHAT_EFFECTIVE_AGENT_ACCESS_LEVELS = ["talk", "read", "edit_only", "ask", "own"] as const;
export type ChatEffectiveAgentAccess = (typeof CHAT_EFFECTIVE_AGENT_ACCESS_LEVELS)[number];

/** Why an agent works below the channel's access on this device. */
export const CHAT_ACCESS_DOWNGRADE_REASONS = [
  "cannot_confine_commands",
  "cannot_ask_before_commands",
  "cannot_read_only",
  "cannot_isolate",
] as const;
export type ChatAccessDowngradeReason = (typeof CHAT_ACCESS_DOWNGRADE_REASONS)[number];

/** The access a scope resolves to, with the scope it comes from. */
export interface ChatEffectiveChannelAccessRead {
  access: ChatAgentAccess;
  source: ChatAgentAccessSource;
}

/** What one agent in a channel does on the reading device under the channel's access. */
export interface ChatAgentResolvedAccessRead {
  agentId: ChatParticipantId;
  requested: ChatAgentAccess;
  /** Null when the agent cannot work in the channel on this device at all. */
  effective: ChatEffectiveAgentAccess | null;
  /** Why `effective` is narrower than `requested`, null when it is not or the provider is not installed. */
  downgradeReason: ChatAccessDowngradeReason | null;
  /** Whether this device has a provider installation for the agent. */
  installed: boolean;
  /** Whether the agent's provider on this device can work on its own. */
  ownOffered: boolean;
}

/** A model choice with its options, as agent defaults, channel overrides, and assignments store it. */
export interface ChatModelSelection {
  /** Whether the provider picks the model; `modelId` is then null. */
  providerManagedModel: boolean;
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
}

/** The app scope of agent settings, which every project and channel inherits. */
export interface ChatAppAgentSettingsRead {
  access: ChatAgentAccess;
  instructions: string;
  revision: number;
  updatedAt: UtcTimestamp;
}

/** The project scope of agent settings; a null access inherits the app access. */
export interface ChatProjectAgentSettingsRead {
  projectId: string;
  access: ChatAgentAccess | null;
  instructions: string;
  /** The access the project's channels inherit, with its source. */
  effective: ChatEffectiveChannelAccessRead;
  /** Zero while the project has never saved its own settings. */
  revision: number;
}

/** One standing push approval of an agent in a channel. */
export interface ChatChannelPushApprovalRead {
  approvalId: string;
  channelId: ChatChannelId;
  agentId: ChatParticipantId;
  workingFolderId: ProjectWorkingFolderId;
  workingFolderName: string;
  pushUrl: string;
  destinationBranch: string;
  approvedAt: UtcTimestamp;
}

/** The channel scope of agent settings with what it resolves to on this device. */
export interface ChatChannelAgentSettingsRead {
  channelId: ChatChannelId;
  conversationId: ChatConversationId;
  /** The channel's own access, null when it inherits. */
  access: ChatAgentAccess | null;
  instructions: string;
  effective: ChatEffectiveChannelAccessRead;
  agents: ChatAgentResolvedAccessRead[];
  pushApprovals: ChatChannelPushApprovalRead[];
  revision: number;
}

export interface UpdateChatAppAgentSettingsRequest {
  access: ChatAgentAccess;
  instructions: string;
  expectedRevision: number;
}

/** Saving with `expectedRevision` 0 creates the project's row. */
export interface UpdateChatProjectAgentSettingsRequest {
  projectId: string;
  access: ChatAgentAccess | null;
  instructions: string;
  expectedRevision: number;
}

export interface UpdateChatChannelAgentSettingsRequest {
  channelId: ChatChannelId;
  access: ChatAgentAccess | null;
  instructions: string;
  expectedRevision: number;
}

export interface ChatChannelAgentRequest {
  channelId: ChatChannelId;
  agentId: ChatParticipantId;
}

/** One local day of an agent's usage, summed across the devices that ran it. */
export interface ChatAgentUsageDayRead {
  /** The day in `YYYY-MM-DD` form. */
  localDate: string;
  countedTokens: number;
  workingSeconds: number;
  runCount: number;
  deviceIds: string[];
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

export const CHAT_CUSTOM_AGENT_REMOVAL_KINDS = ["deleted", "archived"] as const;

/** What removing an agent a person named did: a never-used agent is deleted on every device, and a used one is archived and hidden. */
export type ChatCustomAgentRemoval =
  | { kind: "deleted" }
  | { kind: "archived"; agent: ChatAiAgentRead };

export interface ListChatAssignmentTargetsRequest {
  agentId: ChatParticipantId;
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
  agentId: ChatParticipantId;
  agentName: string;
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
  agentName: string;
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

/** An agent's default model, set in Settings > Agents. */
export interface ChatAgentPolicyRead {
  id: ChatAgentPolicyRevisionId;
  agentId: ChatParticipantId;
  revision: number;
  providerManagedModel: boolean;
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
  providerOptions: VersionedJson;
  interactionMode: InteractionMode;
  createdAt: UtcTimestamp;
}

/** An agent of a provider family: a built-in model line such as `@Opus`, a catalog model such as `@Opus 5.5`, or one a person named. */
export interface ChatAiAgentRead {
  participant: ChatParticipantRead;
  providerFamilyId: ProviderFamilyId;
  agentKind: ChatAgentKind;
  /** This device's provider installation for the agent, null when it has none. */
  localProviderInstanceId: ProviderInstanceId | null;
  configurationState: ChatAgentConfigurationState;
  /** Null while the agent uses the provider default. */
  latestPolicy: ChatAgentPolicyRead | null;
  channelCount: number;
  activeAssignmentCount: number;
  /** When a person removed the agent after it had been used, which archived it and hides it from the Settings agent list; null while shown. A hidden agent is always off. */
  hiddenAt: UtcTimestamp | null;
}

export interface ChatConversationMembershipRead {
  conversationId: ChatConversationId;
  participant: ChatParticipantRead;
  /** Whether the participant is an agent added to the channel. */
  agentInChannel: boolean;
  revision: number;
  removedAt: UtcTimestamp | null;
}

export interface ChatChannelRosterRead {
  channelId: ChatChannelId;
  conversationId: ChatConversationId;
  audienceRevision: number;
  memberships: ChatConversationMembershipRead[];
}

export interface PreviewChatChannelMembershipRemovalRequest {
  agentId: ChatParticipantId;
  channelId: ChatChannelId;
}

export interface ChatChannelMembershipRemovalPreview {
  agentId: ChatParticipantId;
  channelId: ChatChannelId;
  activeAssignmentCount: number;
  activeAuthorizationCount: number;
  willRevokeActiveWork: boolean;
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
  /** Whether a failed run in the thread has not been read in the thread yet. */
  unseenFailure: boolean;
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
  /** When the message entered its scope: the post time, or for a reply that waited for running work, the time the work received it. */
  createdAt: UtcTimestamp;
  /** Whether the message is a reply waiting for running work, or one cancelled before it reached a run. The timeline leaves it out, and the queue of its work lists a waiting one. */
  awaitingDelivery: boolean;
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
  agent: ChatParticipantRead;
  triggeringMessageItemId: ChatConversationItemId;
  previousAssignmentId: ChatWorkAssignmentId | null;
  state: ChatWorkAssignmentState;
  stateReason: string | null;
  /** Where this work came from when another agent delegated it; null for work a person started. */
  delegation: ChatAssignmentDelegationRead | null;
  /** The model the work starts on, frozen when it is created. */
  modelSelection: ChatModelSelection;
  /** The composed app, project, and channel instructions, frozen when the work is created. */
  instructions: string;
  /** The display name of the model that ran, null until the work is dispatched or when the provider chose the model. */
  modelLabel: string | null;
  revision: number;
  settledAt: UtcTimestamp | null;
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
  /** How a person ended the work, present only when it is completed after review. */
  review: ChatAssignmentReview | null;
  /** Chat thread of the newest run of this reply thread on the reading device, used to show its usage; null when none ran here. */
  latestRunProviderThreadId: ChatThreadId | null;
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

/** Work an agent delegated from an assignment, with its live state. */
export interface ChatDelegatedChildRead {
  assignmentId: ChatWorkAssignmentId;
  parentAssignmentId: ChatWorkAssignmentId;
  replyThreadId: ChatReplyThreadId;
  agent: ChatParticipantRead;
  objective: string;
  state: ChatWorkAssignmentState;
  stateReason: string | null;
  createdAt: UtcTimestamp;
}

/** A secret value a running agent asked for, open until a person provides or declines it or it expires. */
export interface ChatSecretRequestRead {
  id: string;
  assignmentId: ChatWorkAssignmentId;
  label: string;
  purpose: string;
  createdAt: UtcTimestamp;
  expiresAt: UtcTimestamp;
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
  agentParticipantId: ChatParticipantId;
  agentDisplayName: string;
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
  /** How many of `total` are failures. Notifications announce them, while the sidebar leaves them to the channel and thread markers. */
  failureCount: number;
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
  /** Whether a person holds the pending reply back from delivery until they resume it. */
  held: boolean;
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

/** An agent's default model; every field is required because the command refuses unknown or missing ones. */
export interface ChatAgentPolicyInput {
  providerManagedModel: boolean;
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
  providerOptions: VersionedJson;
  interactionMode: InteractionMode;
}

export interface SetChatAgentPolicyRequest {
  agentId: ChatParticipantId;
  policy: ChatAgentPolicyInput;
}

/** A named agent: a person-chosen name, the provider family it runs through, and the model and options it keeps. */
export interface CreateChatAgentRequest {
  displayName: string;
  providerFamilyId: ProviderFamilyId;
  policy: ChatAgentPolicyInput;
}

export interface PostChatMessageRequest {
  clientCommandId: ChatCommandId;
  channelId: ChatChannelId;
  replyThreadId: ChatReplyThreadId | null;
  normalizedMarkdown: string;
  richContent: VersionedJson;
  attachmentIds: ChatAttachmentId[];
  references: ChatMessageReference[];
  alsoSendToChannel: boolean;
  /** How a reply to a thread with active work reaches it; absent steers working work and queues anything else. Scheduled messages refuse it. */
  followupRouting?: ChatFollowupRouting;
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

/** New content, delivery time, and repeat rule for a message still waiting to be sent; its channel, reply thread, and command id stay. */
export interface UpdateScheduledChatMessageRequest {
  scheduledMessageId: ChatScheduledMessageId;
  /** Delivery time of a one-time message, or the time from which a repeating one next posts. */
  scheduledFor: UtcTimestamp;
  normalizedMarkdown: string;
  richContent: VersionedJson;
  attachmentIds: ChatAttachmentId[];
  references: ChatMessageReference[];
  alsoSendToChannel: boolean;
  /** Repeat rule; `null` sends the message once. */
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
