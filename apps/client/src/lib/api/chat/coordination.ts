import { invoke } from "@tauri-apps/api/core";
import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
import { ensureDbUrl } from "$lib/api/db";
import type {
  ChatChannelId,
  ChatChannelRead,
  ChatParticipantId,
  ChatReplyThreadId,
  ChatWorkAssignmentId,
  ChatAgentUsageDayRead,
  ChatAssignmentInputEditRequest,
  ChatAssignmentInputId,
  ChatAssignmentInputRead,
  ChatAssignmentReviewRequest,
  ChatAiAgentRead,
  ChatCustomAgentRemoval,
  ChatAppAgentSettingsRead,
  ChatChannelAgentRequest,
  ChatChannelAgentSettingsRead,
  ChatProjectAgentSettingsRead,
  CreateChatAgentRequest,
  SetChatAgentPolicyRequest,
  UpdateChatAppAgentSettingsRequest,
  UpdateChatChannelAgentSettingsRequest,
  UpdateChatProjectAgentSettingsRequest,
  ChatChannelRosterRead,
  ChatChannelMembershipRemovalPreview,
  PreviewChatChannelMembershipRemovalRequest,
  ChatAssignmentTargetRead,
  ListChatAssignmentTargetsRequest,
  BrowseChatScratchGenerationRequest,
  CleanupChatScratchRequest,
  PreviewChatScratchCleanupRequest,
  PromoteChatScratchFileRequest,
  ChatScratchCleanupPreviewRead,
  ChatScratchCleanupResultRead,
  ChatScratchDirectoryPageRead,
  ChatScratchPromotionResultRead,
  ChatScratchReclaimReportRead,
  ChatScratchScopeRead,
  ChatProjectPrimaryWorkingFolderRead,
  ChatChannelPageRead,
  ChatReplyThreadPageRead,
  ChatMessageSearchResultRead,
  PostChatMessageRequest,
  PostChatMessageResult,
  ScheduleChatMessageRequest,
  UpdateScheduledChatMessageRequest,
  ChatScheduledMessageDispatchRead,
  ChatScheduledMessageId,
  ChatScheduledMessageRead,
  ChatReviewQueueRead,
  ChatSecretRequestRead,
  CreateChatChannelRequest,
  UpdateChatChannelDetailsRequest,
  ProjectWorkingFolderId,
} from "$lib/chat/contracts";
import {
  parseChatChannel,
  parseChatAgentUsageDay,
  parseChatChannels,
  parseChatAiAgent,
  parseChatAiAgents,
  parseChatCustomAgentRemoval,
  parseChatAppAgentSettings,
  parseChatChannelAgentSettings,
  parseChatProjectAgentSettings,
  parseChatChannelRoster,
  parseChatChannelMembershipRemovalPreview,
  parseChatAssignmentTargets,
  parseChatScratchCleanupPreview,
  parseChatScratchCleanupResult,
  parseChatScratchDirectoryPage,
  parseChatScratchPromotionResult,
  parseChatScratchReclaimReport,
  parseChatScratchScopes,
  parseChatProjectPrimaryWorkingFolder,
  parseChatChannelPage,
  parseChatReplyThreadPage,
  parseChatMessageSearchResults,
  parseChatReviewQueue,
  parseChatSecretRequests,
  parsePostChatMessageResult,
  parseChatScheduledMessage,
  parseChatScheduledMessageDispatch,
  parseChatScheduledMessages,
  parseChatWorkAssignment,
  parseChatAssignmentInput,
  parseChatAssignmentInputs,
} from "$lib/chat/validation";

const localExecutionAvailable = platformHasCapability(
  BUILD_PLATFORM_PROFILE,
  "chat.local-execution",
);

export async function listChatChannels(projectId: string, archived = false): Promise<ChatChannelRead[]> {
  return parseChatChannels(await invoke<unknown>("chat_list_channels", {
    dbUrl: await ensureDbUrl(), projectId, archived,
  }));
}

export async function listChatNavigationChannels(): Promise<ChatChannelRead[]> {
  return parseChatChannels(await invoke<unknown>("chat_list_navigation_channels", {
    dbUrl: await ensureDbUrl(),
  }));
}

export async function readChatChannel(channelId: ChatChannelId): Promise<ChatChannelRead> {
  return parseChatChannel(await invoke<unknown>("chat_read_channel", {
    dbUrl: await ensureDbUrl(), channelId,
  }));
}

export async function createChatChannel(request: CreateChatChannelRequest): Promise<ChatChannelRead> {
  return parseChatChannel(await invoke<unknown>("chat_create_channel", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function updateChatChannelDetails(
  request: UpdateChatChannelDetailsRequest,
): Promise<ChatChannelRead> {
  return parseChatChannel(await invoke<unknown>("chat_update_channel_details", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function archiveChatChannel(
  channelId: ChatChannelId,
  expectedRevision: number,
): Promise<ChatChannelRead> {
  return parseChatChannel(await invoke<unknown>("chat_archive_channel", {
    dbUrl: await ensureDbUrl(), channelId, expectedRevision,
  }));
}

export async function restoreChatChannel(
  channelId: ChatChannelId,
  expectedRevision: number,
): Promise<ChatChannelRead> {
  return parseChatChannel(await invoke<unknown>("chat_restore_channel", {
    dbUrl: await ensureDbUrl(), channelId, expectedRevision,
  }));
}

export async function setChatChannelRead(channelId: ChatChannelId, read: boolean): Promise<ChatChannelRead> {
  return parseChatChannel(await invoke<unknown>("chat_set_channel_read", {
    dbUrl: await ensureDbUrl(), channelId, read,
  }));
}

export async function listChatAgents(archived = false): Promise<ChatAiAgentRead[]> {
  return parseChatAiAgents(await invoke<unknown>("chat_list_agents", {
    dbUrl: await ensureDbUrl(), archived,
  }));
}

export async function readChatAgent(agentId: ChatParticipantId): Promise<ChatAiAgentRead> {
  return parseChatAiAgent(await invoke<unknown>("chat_read_agent", {
    dbUrl: await ensureDbUrl(), agentId,
  }));
}

/** Sets an agent's default model, which work starts on unless a channel or a thread overrides it. */
export async function setChatAgentPolicy(request: SetChatAgentPolicyRequest): Promise<ChatAiAgentRead> {
  return parseChatAiAgent(await invoke<unknown>("chat_set_agent_policy", {
    dbUrl: await ensureDbUrl(),
    request: {
      agentId: request.agentId,
      policy: {
        providerManagedModel: request.policy.providerManagedModel,
        modelId: request.policy.modelId,
        modelOptions: request.policy.modelOptions,
        providerOptions: request.policy.providerOptions,
        interactionMode: request.policy.interactionMode,
      },
    },
  }));
}

/**
 * Creates a named agent that runs one provider family with the model and options it keeps. Names are unique, ignoring
 * case, and cannot be a model line such as "Opus" or "Sol"; a taken or malformed name fails validation on `displayName`.
 */
export async function createChatAgent(request: CreateChatAgentRequest): Promise<ChatAiAgentRead> {
  return parseChatAiAgent(await invoke<unknown>("chat_create_agent", {
    dbUrl: await ensureDbUrl(),
    request: {
      displayName: request.displayName,
      providerFamilyId: request.providerFamilyId,
      policy: {
        providerManagedModel: request.policy.providerManagedModel,
        modelId: request.policy.modelId,
        modelOptions: request.policy.modelOptions,
        providerOptions: request.policy.providerOptions,
        interactionMode: request.policy.interactionMode,
      },
    },
  }));
}

/** Reads the app access and instructions every project and channel inherits. */
export async function readChatAppAgentSettings(): Promise<ChatAppAgentSettingsRead> {
  return parseChatAppAgentSettings(await invoke<unknown>("chat_read_app_agent_settings", {
    dbUrl: await ensureDbUrl(),
  }));
}

/** Saves the app access and instructions; a stale revision fails with `stale_revision`. */
export async function updateChatAppAgentSettings(
  request: UpdateChatAppAgentSettingsRequest,
): Promise<ChatAppAgentSettingsRead> {
  return parseChatAppAgentSettings(await invoke<unknown>("chat_update_app_agent_settings", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

/** Reads a project's access and instructions with the access its channels inherit. */
export async function readChatProjectAgentSettings(projectId: string): Promise<ChatProjectAgentSettingsRead> {
  return parseChatProjectAgentSettings(await invoke<unknown>("chat_read_project_agent_settings", {
    dbUrl: await ensureDbUrl(), projectId,
  }));
}

/** Saves a project's access and instructions; revision 0 creates them. */
export async function updateChatProjectAgentSettings(
  request: UpdateChatProjectAgentSettingsRequest,
): Promise<ChatProjectAgentSettingsRead> {
  return parseChatProjectAgentSettings(await invoke<unknown>("chat_update_project_agent_settings", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

/** Reads a channel's agent settings with what each agent in it does on this device. */
export async function readChatChannelAgentSettings(
  channelId: ChatChannelId,
): Promise<ChatChannelAgentSettingsRead> {
  return parseChatChannelAgentSettings(await invoke<unknown>("chat_read_channel_agent_settings", {
    dbUrl: await ensureDbUrl(), channelId,
  }));
}

/** Saves a channel's access and instructions; a null access inherits. */
export async function updateChatChannelAgentSettings(
  request: UpdateChatChannelAgentSettingsRequest,
): Promise<ChatChannelAgentSettingsRead> {
  return parseChatChannelAgentSettings(await invoke<unknown>("chat_update_channel_agent_settings", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

/** Adds an agent to a channel under the channel's access; adding a present agent changes nothing. */
export async function addChatChannelAgent(request: ChatChannelAgentRequest): Promise<ChatChannelAgentSettingsRead> {
  return parseChatChannelAgentSettings(await invoke<unknown>("chat_add_channel_agent", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

/** Removes an agent from a channel, which stops its work there and ends its push approvals. */
export async function removeChatChannelAgent(
  request: ChatChannelAgentRequest,
): Promise<ChatChannelAgentSettingsRead> {
  return parseChatChannelAgentSettings(await invoke<unknown>("chat_remove_channel_agent", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function archiveChatAgent(
  agentId: ChatParticipantId,
  expectedRevision: number,
  archived: boolean,
): Promise<ChatAiAgentRead> {
  return parseChatAiAgent(await invoke<unknown>("chat_archive_agent", {
    dbUrl: await ensureDbUrl(), agentId, expectedRevision, archived,
  }));
}

/**
 * Removes an agent a person named. A never-used agent is deleted on every device; a used one is
 * archived and hidden from the agent list, and the result carries it.
 */
export async function removeCustomChatAgent(
  agentId: ChatParticipantId,
  expectedRevision: number,
): Promise<ChatCustomAgentRemoval> {
  return parseChatCustomAgentRemoval(await invoke<unknown>("chat_remove_custom_agent", {
    dbUrl: await ensureDbUrl(), agentId, expectedRevision,
  }));
}

/** Brings a hidden agent a person named back to the agent list. It stays archived, so it stays off. */
export async function restoreCustomChatAgent(
  agentId: ChatParticipantId,
  expectedRevision: number,
): Promise<ChatAiAgentRead> {
  return parseChatAiAgent(await invoke<unknown>("chat_restore_custom_agent", {
    dbUrl: await ensureDbUrl(), agentId, expectedRevision,
  }));
}

export async function readChatChannelRoster(channelId: ChatChannelId): Promise<ChatChannelRosterRead> {
  const command = localExecutionAvailable
    ? "chat_read_channel_roster"
    : "chat_read_mobile_channel_roster";
  return parseChatChannelRoster(await invoke<unknown>(command, {
    dbUrl: await ensureDbUrl(), channelId,
  }));
}

export async function listChatAssignmentTargets(
  request: ListChatAssignmentTargetsRequest,
): Promise<ChatAssignmentTargetRead[]> {
  return parseChatAssignmentTargets(await invoke<unknown>("chat_list_assignment_targets", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function listChatScratchScopes(
  agentId: ChatParticipantId | null = null,
  replyThreadId: ChatReplyThreadId | null = null,
): Promise<ChatScratchScopeRead[]> {
  return parseChatScratchScopes(await invoke<unknown>("chat_list_scratch_scopes", {
    dbUrl: await ensureDbUrl(), agentId, replyThreadId,
  }));
}

export async function browseChatScratchGeneration(
  request: BrowseChatScratchGenerationRequest,
): Promise<ChatScratchDirectoryPageRead> {
  return parseChatScratchDirectoryPage(await invoke<unknown>("chat_browse_scratch_generation", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function promoteChatScratchFile(
  request: PromoteChatScratchFileRequest,
): Promise<ChatScratchPromotionResultRead> {
  return parseChatScratchPromotionResult(await invoke<unknown>("chat_promote_scratch_file", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function previewChatScratchCleanup(
  request: PreviewChatScratchCleanupRequest,
): Promise<ChatScratchCleanupPreviewRead> {
  return parseChatScratchCleanupPreview(await invoke<unknown>("chat_preview_scratch_cleanup", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function cleanupChatScratch(
  request: CleanupChatScratchRequest,
): Promise<ChatScratchCleanupResultRead> {
  return parseChatScratchCleanupResult(await invoke<unknown>("chat_cleanup_scratch", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

/**
 * Reads the largest scratch generations that the confirmed cleanup can reclaim.
 *
 * The report never deletes anything; cleanup stays in the scratch manager.
 */
export async function readChatScratchReclaimReport(): Promise<ChatScratchReclaimReportRead> {
  return parseChatScratchReclaimReport(await invoke<unknown>("chat_read_scratch_reclaim_report", {
    dbUrl: await ensureDbUrl(),
  }));
}

export async function previewChatChannelMembershipRemoval(
  request: PreviewChatChannelMembershipRemovalRequest,
): Promise<ChatChannelMembershipRemovalPreview> {
  return parseChatChannelMembershipRemovalPreview(await invoke<unknown>(
    "chat_preview_channel_membership_removal",
    { dbUrl: await ensureDbUrl(), request },
  ));
}

export async function readChatProjectPrimaryWorkingFolder(
  projectId: string,
): Promise<ChatProjectPrimaryWorkingFolderRead> {
  return parseChatProjectPrimaryWorkingFolder(await invoke<unknown>(
    "chat_read_project_primary_working_folder",
    { dbUrl: await ensureDbUrl(), projectId },
  ));
}

export async function setChatProjectPrimaryWorkingFolder(
  projectId: string,
  workingFolderId: ProjectWorkingFolderId,
  expectedRevision: number,
): Promise<ChatProjectPrimaryWorkingFolderRead> {
  return parseChatProjectPrimaryWorkingFolder(await invoke<unknown>(
    "chat_set_project_primary_working_folder",
    { dbUrl: await ensureDbUrl(), projectId, workingFolderId, expectedRevision },
  ));
}

export async function postChatMessage(request: PostChatMessageRequest): Promise<PostChatMessageResult> {
  return parsePostChatMessageResult(await invoke<unknown>("chat_post_message", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function scheduleChatMessage(
  request: ScheduleChatMessageRequest,
): Promise<ChatScheduledMessageRead> {
  return parseChatScheduledMessage(await invoke<unknown>("chat_schedule_message", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function listScheduledChatMessages(
  channelId: ChatChannelId,
  replyThreadId: ChatReplyThreadId | null,
): Promise<ChatScheduledMessageRead[]> {
  return parseChatScheduledMessages(await invoke<unknown>("chat_list_scheduled_messages", {
    dbUrl: await ensureDbUrl(), channelId, replyThreadId,
  }));
}

export async function cancelScheduledChatMessage(
  scheduledMessageId: ChatScheduledMessageId,
): Promise<void> {
  await invoke("chat_cancel_scheduled_message", {
    dbUrl: await ensureDbUrl(), scheduledMessageId,
  });
}

/** Replaces the content, time, and repeat rule of a scheduled message that is not being sent yet. */
export async function updateScheduledChatMessage(
  request: UpdateScheduledChatMessageRequest,
): Promise<ChatScheduledMessageRead> {
  return parseChatScheduledMessage(await invoke<unknown>("chat_update_scheduled_message", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function retryScheduledChatMessage(
  scheduledMessageId: ChatScheduledMessageId,
): Promise<ChatScheduledMessageRead> {
  return parseChatScheduledMessage(await invoke<unknown>("chat_retry_scheduled_message", {
    dbUrl: await ensureDbUrl(), scheduledMessageId,
  }));
}

export async function sendScheduledChatMessageNow(
  scheduledMessageId: ChatScheduledMessageId,
): Promise<PostChatMessageResult> {
  return parsePostChatMessageResult(await invoke<unknown>("chat_send_scheduled_message_now", {
    dbUrl: await ensureDbUrl(), scheduledMessageId,
  }));
}

export async function dispatchDueScheduledChatMessages(): Promise<ChatScheduledMessageDispatchRead> {
  return parseChatScheduledMessageDispatch(await invoke<unknown>("chat_dispatch_due_scheduled_messages", {
    dbUrl: await ensureDbUrl(),
  }));
}

export async function readChatChannelPage(
  channelId: ChatChannelId,
  cursor: string | null = null,
  limit = 50,
): Promise<ChatChannelPageRead> {
  return parseChatChannelPage(await invoke<unknown>("chat_read_channel_page", {
    dbUrl: await ensureDbUrl(), channelId, cursor, limit,
  }));
}

export async function readChatReplyThreadPage(
  replyThreadId: ChatReplyThreadId,
  cursor: string | null = null,
  limit = 50,
): Promise<ChatReplyThreadPageRead> {
  return parseChatReplyThreadPage(await invoke<unknown>("chat_read_reply_thread_page", {
    dbUrl: await ensureDbUrl(), replyThreadId, cursor, limit,
  }));
}

export async function searchChatMessages(
  query: string,
  projectId: string | null = null,
  limit = 50,
): Promise<ChatMessageSearchResultRead[]> {
  return parseChatMessageSearchResults(await invoke<unknown>("chat_search_messages", {
    dbUrl: await ensureDbUrl(), query, projectId, limit,
  }));
}

export async function cancelChatAssignment(
  assignmentId: ChatWorkAssignmentId,
  expectedRevision: number,
): Promise<import("$lib/chat/contracts").ChatWorkAssignmentRead> {
  return parseChatWorkAssignment(await invoke<unknown>("chat_cancel_assignment", {
    dbUrl: await ensureDbUrl(), assignmentId, expectedRevision,
  }));
}

/** Accepts ready-for-review work, or posts a note that continues it in a follow-up assignment. */
export async function reviewChatAssignment(
  assignmentId: ChatWorkAssignmentId,
  expectedRevision: number,
  review: ChatAssignmentReviewRequest,
): Promise<import("$lib/chat/contracts").ChatWorkAssignmentRead> {
  return parseChatWorkAssignment(await invoke<unknown>("chat_review_assignment", {
    dbUrl: await ensureDbUrl(), assignmentId, expectedRevision, review,
  }));
}

export async function retryChatAssignment(
  assignmentId: ChatWorkAssignmentId,
  expectedRevision: number,
): Promise<import("$lib/chat/contracts").ChatWorkAssignmentRead> {
  return parseChatWorkAssignment(await invoke<unknown>("chat_retry_assignment", {
    dbUrl: await ensureDbUrl(), assignmentId, expectedRevision,
  }));
}

/**
 * Lists the assignments that wait for the person, ordered by priority and recency and capped,
 * with the total count.
 *
 * @param projectId Limits the queue to one project, or `null` for every project.
 */
export async function listChatReviewQueue(projectId: string | null): Promise<ChatReviewQueueRead> {
  return parseChatReviewQueue(await invoke<unknown>("chat_list_review_queue", {
    dbUrl: await ensureDbUrl(),
    projectId,
  }));
}

export async function recoverChatAssignmentDispatchJobs(): Promise<number> {
  return invoke<number>("chat_recover_assignment_dispatch_jobs", { dbUrl: await ensureDbUrl() });
}

/**
 * Lists the secret requests still open in a reply thread. Secret requests come from local runs, so a
 * device without local execution has none.
 */
export async function listChatSecretRequests(
  replyThreadId: ChatReplyThreadId,
): Promise<ChatSecretRequestRead[]> {
  if (!localExecutionAvailable) return [];
  return parseChatSecretRequests(await invoke<unknown>("chat_list_secret_requests", {
    dbUrl: await ensureDbUrl(), replyThreadId,
  }));
}

/**
 * Hands a secret value to the run that asked for it. The value goes to a private file for the run and
 * is never stored in the vault.
 *
 * @throws When this device cannot run agents, or when the backend refuses the value with a secret refusal code.
 */
export async function provideChatSecret(requestId: string, value: string): Promise<void> {
  requireLocalSecretRequests();
  await invoke<unknown>("chat_provide_secret", { dbUrl: await ensureDbUrl(), requestId, value });
}

/**
 * Declines a secret request so the run continues without the value.
 *
 * @throws When this device cannot run agents, or when the request is no longer open.
 */
export async function declineChatSecret(requestId: string): Promise<void> {
  requireLocalSecretRequests();
  await invoke<unknown>("chat_decline_secret", { dbUrl: await ensureDbUrl(), requestId });
}

function requireLocalSecretRequests(): void {
  if (!localExecutionAvailable) {
    throw new Error("Chat secret requests need local execution on this device");
  }
}

/** Lists the replies addressed to one assignment, in delivery order. */
export async function listChatAssignmentInputs(assignmentId: ChatWorkAssignmentId): Promise<ChatAssignmentInputRead[]> {
  return parseChatAssignmentInputs(await invoke<unknown>("chat_list_assignment_inputs", {
    dbUrl: await ensureDbUrl(), assignmentId,
  }));
}

/** Replaces the text of a reply that has not reached the agent yet. */
export async function editChatAssignmentInput(
  inputId: ChatAssignmentInputId,
  edit: ChatAssignmentInputEditRequest,
): Promise<ChatAssignmentInputRead> {
  return parseChatAssignmentInput(await invoke<unknown>("chat_edit_assignment_input", {
    dbUrl: await ensureDbUrl(), inputId, edit,
  }));
}

/** Cancels a reply that has not reached the agent yet, keeping it in the thread. */
export async function cancelChatAssignmentInput(inputId: ChatAssignmentInputId): Promise<ChatAssignmentInputRead> {
  return parseChatAssignmentInput(await invoke<unknown>("chat_cancel_assignment_input", {
    dbUrl: await ensureDbUrl(), inputId,
  }));
}

/** Holds a waiting reply back from delivery, or resumes it so it is delivered in its turn. */
export async function setChatAssignmentInputHeld(
  inputId: ChatAssignmentInputId,
  held: boolean,
): Promise<ChatAssignmentInputRead> {
  return parseChatAssignmentInput(await invoke<unknown>("chat_set_assignment_input_held", {
    dbUrl: await ensureDbUrl(), inputId, held,
  }));
}

/** Sends one waiting reply into the agent's current turn now, resuming it when held. */
export async function steerChatAssignmentInput(inputId: ChatAssignmentInputId): Promise<ChatAssignmentInputRead> {
  return parseChatAssignmentInput(await invoke<unknown>("chat_steer_assignment_input", {
    dbUrl: await ensureDbUrl(), inputId,
  }));
}

/**
 * Reorders the waiting replies of one assignment.
 *
 * @param orderedInputIds Every waiting reply of the assignment, in the new delivery order.
 * @returns The replies addressed to the assignment, in delivery order.
 * @throws When the waiting replies changed before the reorder.
 */
export async function reorderChatAssignmentInputs(
  assignmentId: ChatWorkAssignmentId,
  orderedInputIds: readonly ChatAssignmentInputId[],
): Promise<ChatAssignmentInputRead[]> {
  return parseChatAssignmentInputs(await invoke<unknown>("chat_reorder_assignment_inputs", {
    dbUrl: await ensureDbUrl(), assignmentId, orderedInputIds,
  }));
}

/** Reads an agent's usage per local day from `fromDate` to `toDate` (`YYYY-MM-DD`, both included). */
export async function readChatAgentUsage(
  agentId: ChatParticipantId,
  fromDate: string,
  toDate: string,
): Promise<ChatAgentUsageDayRead[]> {
  const days = await invoke<unknown>("chat_read_agent_usage", {
    dbUrl: await ensureDbUrl(), agentId, fromDate, toDate,
  });
  if (!Array.isArray(days)) throw new Error("Chat agent usage must be an array");
  return days.map((day: unknown, index) => parseChatAgentUsageDay(day, `Chat agent usage day ${index}`));
}
