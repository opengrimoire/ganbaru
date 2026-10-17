import { invoke } from "@tauri-apps/api/core";
import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
import { ensureDbUrl } from "$lib/api/db";
import type {
  ChatChannelId,
  ChatChannelRead,
  ChatParticipantId,
  ChatReplyThreadId,
  ChatWorkAssignmentId,
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
  CreateChatChannelRequest,
  UpdateChatChannelDetailsRequest,
  ProjectWorkingFolderId,
} from "$lib/chat/contracts";
import {
  parseChatChannel,
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
  parseChatScratchScopes,
  parseChatProjectPrimaryWorkingFolder,
  parseChatChannelPage,
  parseChatReplyThreadPage,
  parseChatMessageSearchResults,
  parsePostChatMessageResult,
  parseChatScheduledMessage,
  parseChatScheduledMessageDispatch,
  parseChatScheduledMessages,
  parseChatWorkAssignment,
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

export async function deleteUnusedChatTeammate(
  teammateId: ChatParticipantId,
  expectedRevision: number,
): Promise<void> {
  await invoke("chat_delete_unused_teammate", {
    dbUrl: await ensureDbUrl(), teammateId, expectedRevision,
  });
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

export async function retryChatAssignment(
  assignmentId: ChatWorkAssignmentId,
  expectedRevision: number,
): Promise<import("$lib/chat/contracts").ChatWorkAssignmentRead> {
  return parseChatWorkAssignment(await invoke<unknown>("chat_retry_assignment", {
    dbUrl: await ensureDbUrl(), assignmentId, expectedRevision,
  }));
}

export async function recoverChatAssignmentDispatchJobs(): Promise<number> {
  return invoke<number>("chat_recover_assignment_dispatch_jobs", { dbUrl: await ensureDbUrl() });
}
