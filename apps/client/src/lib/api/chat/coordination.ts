import { invoke } from "@tauri-apps/api/core";
import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
import { ensureDbUrl } from "$lib/api/db";
import type {
  ChatChannelId,
  ChatChannelRead,
  ChatParticipantId,
  ChatReplyThreadId,
  ChatWorkAssignmentId,
  ChatAssignmentBudgetInput,
  ChatAssignmentBudgetRead,
  ChatTeammateUsageDayRead,
  ChatAssignmentInputEditRequest,
  ChatAssignmentInputId,
  ChatAssignmentInputRead,
  ChatAssignmentReviewRequest,
  ChatAiTeammateRead,
  ChatAccessProfileRead,
  ChatAccessProfileImpactPreviewRead,
  ChatTeammateAccessPreviewRead,
  ChatTeammateAccessRead,
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
  CreateChatTeammateRequest,
  ReplaceChatTeammateAccessRequest,
  CreateChatAccessProfileRequest,
  DuplicateChatAccessProfileRequest,
  PublishChatAccessProfileRevisionRequest,
  ArchiveChatAccessProfileRequest,
  PostChatMessageRequest,
  PostChatMessageResult,
  ScheduleChatMessageRequest,
  ChatScheduledMessageDispatchRead,
  ChatScheduledMessageId,
  ChatScheduledMessageRead,
  ChatWaitingAssignmentsRead,
  ChatReviewQueueRead,
  ChatSecretRequestRead,
  CreateChatChannelRequest,
  UpdateChatChannelDetailsRequest,
  ProjectWorkingFolderId,
} from "$lib/chat/contracts";
import {
  parseChatChannel,
  parseChatAssignmentBudget,
  parseChatTeammateUsageDay,
  parseChatChannels,
  parseChatAiTeammate,
  parseChatAiTeammates,
  parseChatAccessProfiles,
  parseChatAccessProfile,
  parseChatAccessProfileImpactPreview,
  parseChatTeammateAccess,
  parseChatTeammateAccessPreview,
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
  parseChatWaitingAssignments,
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

export async function listChatTeammates(archived = false): Promise<ChatAiTeammateRead[]> {
  return parseChatAiTeammates(await invoke<unknown>("chat_list_teammates", {
    dbUrl: await ensureDbUrl(), archived,
  }));
}

export async function readChatTeammate(teammateId: ChatParticipantId): Promise<ChatAiTeammateRead> {
  return parseChatAiTeammate(await invoke<unknown>("chat_read_teammate", {
    dbUrl: await ensureDbUrl(), teammateId,
  }));
}

export async function listChatAccessProfiles(archived = false): Promise<ChatAccessProfileRead[]> {
  return parseChatAccessProfiles(await invoke<unknown>("chat_list_access_profiles", {
    dbUrl: await ensureDbUrl(), archived,
  }));
}

export async function createChatAccessProfile(
  request: CreateChatAccessProfileRequest,
): Promise<ChatAccessProfileRead> {
  return parseChatAccessProfile(await invoke<unknown>("chat_create_access_profile", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function duplicateChatAccessProfile(
  request: DuplicateChatAccessProfileRequest,
): Promise<ChatAccessProfileRead> {
  return parseChatAccessProfile(await invoke<unknown>("chat_duplicate_access_profile", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function previewChatAccessProfileRevision(
  request: PublishChatAccessProfileRevisionRequest,
): Promise<ChatAccessProfileImpactPreviewRead> {
  return parseChatAccessProfileImpactPreview(await invoke<unknown>(
    "chat_preview_access_profile_revision",
    { dbUrl: await ensureDbUrl(), request },
  ));
}

export async function publishChatAccessProfileRevision(
  request: PublishChatAccessProfileRevisionRequest,
): Promise<ChatAccessProfileRead> {
  return parseChatAccessProfile(await invoke<unknown>("chat_publish_access_profile_revision", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function archiveChatAccessProfile(
  request: ArchiveChatAccessProfileRequest,
): Promise<ChatAccessProfileRead> {
  return parseChatAccessProfile(await invoke<unknown>("chat_archive_access_profile", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function readChatTeammateAccess(
  teammateId: ChatParticipantId,
): Promise<ChatTeammateAccessRead> {
  return parseChatTeammateAccess(await invoke<unknown>("chat_read_teammate_access", {
    dbUrl: await ensureDbUrl(), teammateId,
  }));
}

export async function previewChatTeammateAccess(
  request: ReplaceChatTeammateAccessRequest,
): Promise<ChatTeammateAccessPreviewRead> {
  return parseChatTeammateAccessPreview(await invoke<unknown>("chat_preview_teammate_access", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function replaceChatTeammateAccess(
  request: ReplaceChatTeammateAccessRequest,
): Promise<ChatTeammateAccessRead> {
  return parseChatTeammateAccess(await invoke<unknown>("chat_replace_teammate_access", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function createChatTeammate(request: CreateChatTeammateRequest): Promise<ChatAiTeammateRead> {
  return parseChatAiTeammate(await invoke<unknown>("chat_create_teammate", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function archiveChatTeammate(
  teammateId: ChatParticipantId,
  expectedRevision: number,
  archived: boolean,
): Promise<ChatAiTeammateRead> {
  return parseChatAiTeammate(await invoke<unknown>("chat_archive_teammate", {
    dbUrl: await ensureDbUrl(), teammateId, expectedRevision, archived,
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
  teammateId: ChatParticipantId | null = null,
  replyThreadId: ChatReplyThreadId | null = null,
): Promise<ChatScratchScopeRead[]> {
  return parseChatScratchScopes(await invoke<unknown>("chat_list_scratch_scopes", {
    dbUrl: await ensureDbUrl(), teammateId, replyThreadId,
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
 * Starts synced work that waits for a local action on this device. `seenDigest` names the details the user saw;
 * when the work changed since then nothing starts and this returns null, so the caller refreshes the offer.
 */
export async function acceptChatAssignmentHere(
  assignmentId: ChatWorkAssignmentId,
  seenDigest: string,
): Promise<import("$lib/chat/contracts").ChatWorkAssignmentRead | null> {
  const started = await invoke<unknown>("chat_accept_assignment_here", {
    dbUrl: await ensureDbUrl(), assignmentId, seenDigest,
  });
  return started === null ? null : parseChatWorkAssignment(started);
}

/** Lists the work that waits for a local action on this device across the vault, newest first, capped, with the total. */
export async function listChatWaitingAssignments(): Promise<ChatWaitingAssignmentsRead> {
  return parseChatWaitingAssignments(await invoke<unknown>("chat_list_waiting_assignments", {
    dbUrl: await ensureDbUrl(),
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

/** Replaces the text of a reply that has not reached the teammate yet. */
export async function editChatAssignmentInput(
  inputId: ChatAssignmentInputId,
  edit: ChatAssignmentInputEditRequest,
): Promise<ChatAssignmentInputRead> {
  return parseChatAssignmentInput(await invoke<unknown>("chat_edit_assignment_input", {
    dbUrl: await ensureDbUrl(), inputId, edit,
  }));
}

/** Cancels a reply that has not reached the teammate yet, keeping it in the thread. */
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

/** Sends one waiting reply into the teammate's current turn now, resuming it when held. */
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

/** Sets the token and working time limits of an assignment, keeping what its work already used. */
export async function setChatAssignmentBudget(
  assignmentId: ChatWorkAssignmentId,
  budget: ChatAssignmentBudgetInput,
): Promise<ChatAssignmentBudgetRead> {
  return parseChatAssignmentBudget(await invoke<unknown>("chat_set_assignment_budget", {
    dbUrl: await ensureDbUrl(),
    assignmentId,
    tokenLimit: budget.tokenLimit,
    workingSecondsLimit: budget.workingSecondsLimit,
  }));
}

/** Clears the budget of an assignment, so its work runs without a limit. */
export async function clearChatAssignmentBudget(assignmentId: ChatWorkAssignmentId): Promise<void> {
  await invoke<unknown>("chat_clear_assignment_budget", { dbUrl: await ensureDbUrl(), assignmentId });
}

/** Reads a teammate's usage per local day from `fromDate` to `toDate` (`YYYY-MM-DD`, both included). */
export async function readChatTeammateUsage(
  teammateId: ChatParticipantId,
  fromDate: string,
  toDate: string,
): Promise<ChatTeammateUsageDayRead[]> {
  const days = await invoke<unknown>("chat_read_teammate_usage", {
    dbUrl: await ensureDbUrl(), teammateId, fromDate, toDate,
  });
  if (!Array.isArray(days)) throw new Error("Chat teammate usage must be an array");
  return days.map((day: unknown, index) => parseChatTeammateUsageDay(day, `Chat teammate usage day ${index}`));
}
