import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type {
  ChatDraftRead,
  ChatAttachmentRead,
  ChatInteractionStateRead,
  ChatUserInputDraftRead,
  ChatPromptCatalogEntry,
  McpStatusRead,
  ChatQueuedFollowupRead,
  ProjectWorkingFolderPathPage,
  ChatThreadId,
  ChatTurnId,
  ChatThreadShellRead,
  ChatTimelinePageRead,
  ProjectWorkingFolderId,
  ProviderInstanceId,
  SaveChatDraftRequest,
  SaveQueuedFollowupRequest,
  SendChatTurnCommand,
  SendChatTurnResult,
  SteerChatTurnCommand,
  ResolveChatApprovalCommand,
  ResolveChatUserInputCommand,
  VersionedJson,
} from "$lib/chat/contracts";
import {
  parseChatDraftRead,
  parseChatAttachmentRead,
  parseChatInteractionState,
  parseChatUserInputDraft,
  parseChatPromptCatalog,
  parseMcpStatus,
  parseChatQueuedFollowup,
  parseProjectWorkingFolderPathPage,
  parseTurnDispatchReceipt,
  parseChatError,
  parseChatThreadShell,
  parseChatThreadShells,
  parseChatTimelinePage,
} from "$lib/chat/validation";

export async function listChatThreadWindow(
  workingFolderId: ProjectWorkingFolderId | null,
  archived: boolean,
  limit: number,
): Promise<ChatThreadShellRead[]> {
  return parseChatThreadShells(await invoke<unknown>("chat_list_thread_window", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    archived,
    limit,
  }));
}

export async function readChatThreadShell(threadId: ChatThreadId): Promise<ChatThreadShellRead> {
  return parseChatThreadShell(await invoke<unknown>("chat_read_thread_shell", {
    dbUrl: await ensureDbUrl(),
    threadId,
  }));
}

export async function readChatTimelinePage(
  threadId: ChatThreadId,
  cursor: string | null = null,
  limit = 100,
): Promise<ChatTimelinePageRead> {
  return parseChatTimelinePage(await invoke<unknown>("chat_read_timeline_page", {
    dbUrl: await ensureDbUrl(),
    threadId,
    cursor,
    limit,
  }));
}

export async function readChatTimelineTurn(
  threadId: ChatThreadId,
  turnId: ChatTurnId,
): Promise<ChatTimelinePageRead> {
  return parseChatTimelinePage(await invoke<unknown>("chat_read_timeline_turn", {
    dbUrl: await ensureDbUrl(),
    threadId,
    turnId,
  }));
}

export async function openChatExternalUrl(url: string): Promise<void> {
  await invoke("chat_open_external_url", { url });
}

export async function forkChatThread(
  sourceThreadId: ChatThreadId,
  newThreadId: ChatThreadId,
  title: string,
  lastProviderTurnId: string | null = null,
): Promise<ChatThreadShellRead> {
  return parseChatThreadShell(await invoke<unknown>("chat_fork_thread", {
    dbUrl: await ensureDbUrl(),
    request: { sourceThreadId, newThreadId, title, lastProviderTurnId },
  }));
}

export async function readChatDraft(draftId: string): Promise<ChatDraftRead | null> {
  const value = await invoke<unknown>("chat_read_draft", { dbUrl: await ensureDbUrl(), draftId });
  return value === null ? null : parseChatDraftRead(value);
}

export async function saveChatDraft(draft: SaveChatDraftRequest): Promise<ChatDraftRead> {
  return parseChatDraftRead(await invoke<unknown>("chat_save_draft", {
    dbUrl: await ensureDbUrl(),
    draft,
  }));
}

export async function deleteChatDraft(draftId: string): Promise<boolean> {
  return invoke<boolean>("chat_delete_draft", { dbUrl: await ensureDbUrl(), draftId });
}

export async function importChatImage(
  workingFolderId: ProjectWorkingFolderId,
  attachmentId: string,
  displayName: string,
  bytes: number[],
): Promise<ChatAttachmentRead> {
  return parseChatAttachmentRead(await invoke<unknown>("chat_import_image", {
    dbUrl: await ensureDbUrl(),
    request: { workingFolderId, attachmentId, displayName, bytes },
  }));
}

export async function importChatTextSnippet(
  workingFolderId: ProjectWorkingFolderId,
  attachmentId: string,
  displayName: string,
  text: string,
): Promise<ChatAttachmentRead> {
  return parseChatAttachmentRead(await invoke<unknown>("chat_import_text_snippet", {
    dbUrl: await ensureDbUrl(),
    request: { workingFolderId, attachmentId, displayName, text },
  }));
}

export async function pickChatImages(
  workingFolderId: ProjectWorkingFolderId,
  attachmentIds: string[],
  title: string,
): Promise<ChatAttachmentRead[]> {
  const value = await invoke<unknown>("chat_pick_images", {
    dbUrl: await ensureDbUrl(),
    request: { workingFolderId, attachmentIds, title },
  });
  if (!Array.isArray(value)) throw new Error("Chat image picker response must be an array");
  return value.map((entry, index) => parseChatAttachmentRead(entry, `Chat image picker response[${index}]`));
}

export async function chatAttachmentDataUrl(attachmentId: string): Promise<string> {
  return invoke<string>("chat_attachment_data_url", { dbUrl: await ensureDbUrl(), attachmentId });
}

export async function readChatAttachments(
  workingFolderId: ProjectWorkingFolderId,
  attachmentIds: string[],
): Promise<ChatAttachmentRead[]> {
  const value = await invoke<unknown>("chat_read_attachments", {
    dbUrl: await ensureDbUrl(), workingFolderId, attachmentIds,
  });
  if (!Array.isArray(value)) throw new Error("Chat attachments response must be an array");
  return value.map((entry, index) => parseChatAttachmentRead(entry, `Chat attachments response[${index}]`));
}

export async function searchChatWorkingFolderPaths(
  workingFolderId: ProjectWorkingFolderId,
  query: string,
  includeIgnored: boolean,
  cursor: string | null = null,
  limit = 50,
  executionEnvironmentId: string | null = null,
): Promise<ProjectWorkingFolderPathPage> {
  return parseProjectWorkingFolderPathPage(await invoke<unknown>("chat_search_working_folder_paths", {
    dbUrl: await ensureDbUrl(),
    request: { workingFolderId, query, includeIgnored, cursor, limit, executionEnvironmentId },
  }));
}

export async function validateChatWorkingFolderMentions(
  workingFolderId: ProjectWorkingFolderId,
  relativePaths: string[],
  executionEnvironmentId: string | null = null,
): Promise<void> {
  await invoke("chat_validate_working_folder_mentions", {
    dbUrl: await ensureDbUrl(), workingFolderId, relativePaths, executionEnvironmentId,
  });
}

export async function listChatPromptCatalog(
  workingFolderId: ProjectWorkingFolderId,
  providerInstanceId: ProviderInstanceId,
  threadId: ChatThreadId | null,
): Promise<ChatPromptCatalogEntry[]> {
  return parseChatPromptCatalog(await invoke<unknown>("chat_list_prompt_catalog", {
    dbUrl: await ensureDbUrl(), workingFolderId, providerInstanceId, threadId,
  }));
}

export async function readChatInteractionState(threadId: ChatThreadId): Promise<ChatInteractionStateRead> {
  return parseChatInteractionState(await invoke<unknown>("chat_read_interaction_state", {
    dbUrl: await ensureDbUrl(), threadId,
  }));
}

export async function recoverInterruptedChatTurns(): Promise<void> {
  await invoke("chat_recover_interrupted_turns", { dbUrl: await ensureDbUrl() });
}

export async function compactChatContext(threadId: ChatThreadId): Promise<void> {
  await invoke("chat_compact_context", {
    dbUrl: await ensureDbUrl(), threadId, clientCommandId: crypto.randomUUID(),
  });
}

export async function readChatMcpStatus(
  workingFolderId: ProjectWorkingFolderId,
  providerInstanceId: ProviderInstanceId,
  threadId: ChatThreadId | null,
): Promise<McpStatusRead> {
  return parseMcpStatus(await invoke<unknown>("chat_read_mcp_status", {
    dbUrl: await ensureDbUrl(), workingFolderId, providerInstanceId, threadId,
  }));
}

export async function setChatFullAccessTrust(
  providerInstanceId: ProviderInstanceId,
  workingFolderId: ProjectWorkingFolderId,
  trusted: boolean,
): Promise<boolean> {
  return invoke<boolean>("chat_set_full_access_trust", { providerInstanceId, workingFolderId, trusted });
}

export async function hasChatFullAccessTrust(
  providerInstanceId: ProviderInstanceId,
  workingFolderId: ProjectWorkingFolderId,
): Promise<boolean> {
  return invoke<boolean>("chat_has_full_access_trust", { providerInstanceId, workingFolderId });
}

export async function saveChatQueuedFollowup(request: SaveQueuedFollowupRequest): Promise<ChatQueuedFollowupRead> {
  return parseChatQueuedFollowup(await invoke<unknown>("chat_save_queued_followup", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function cancelChatQueuedFollowup(threadId: ChatThreadId): Promise<boolean> {
  return invoke<boolean>("chat_cancel_queued_followup", { dbUrl: await ensureDbUrl(), threadId });
}

export async function markChatQueuedFollowupDispatched(threadId: ChatThreadId, queuedFollowupId: string): Promise<boolean> {
  return invoke<boolean>("chat_mark_queued_followup_dispatched", {
    dbUrl: await ensureDbUrl(), threadId, queuedFollowupId,
  });
}

export async function readChatUserInputDraft(requestId: string): Promise<ChatUserInputDraftRead | null> {
  const value = await invoke<unknown>("chat_read_user_input_draft", {
    dbUrl: await ensureDbUrl(), requestId,
  });
  return value === null ? null : parseChatUserInputDraft(value);
}

export async function saveChatUserInputDraft(requestId: string, answers: VersionedJson): Promise<ChatUserInputDraftRead> {
  return parseChatUserInputDraft(await invoke<unknown>("chat_save_user_input_draft", {
    dbUrl: await ensureDbUrl(), requestId, answers,
  }));
}

export async function stopChatSession(threadId: ChatThreadId, force: boolean): Promise<void> {
  await invoke("chat_stop_session", {
    dbUrl: await ensureDbUrl(), threadId, force, clientCommandId: crypto.randomUUID(),
  });
}

export async function sendChatTurn(request: SendChatTurnCommand): Promise<SendChatTurnResult> {
  const value = await invoke<unknown>("chat_send_turn", { dbUrl: await ensureDbUrl(), request });
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Chat send response must be an object");
  const record = value as Record<string, unknown>;
  return {
    thread: parseChatThreadShell(record.thread),
    dispatch: record.dispatch === null ? null : parseTurnDispatchReceipt(record.dispatch, "Chat send response.dispatch"),
    launchError: record.launchError === null ? null : parseChatError(record.launchError),
  };
}

export async function steerChatTurn(request: SteerChatTurnCommand): Promise<void> {
  await invoke("chat_steer_turn", { dbUrl: await ensureDbUrl(), request });
}

export async function resolveChatApproval(request: ResolveChatApprovalCommand): Promise<void> {
  await invoke("chat_resolve_approval", { dbUrl: await ensureDbUrl(), request });
}

export async function resolveChatUserInput(request: ResolveChatUserInputCommand): Promise<void> {
  await invoke("chat_resolve_user_input", { dbUrl: await ensureDbUrl(), request });
}

async function threadMutation(command: string, args: Record<string, unknown>): Promise<ChatThreadShellRead> {
  return parseChatThreadShell(await invoke<unknown>(command, { dbUrl: await ensureDbUrl(), ...args }));
}

export function renameChatThread(threadId: ChatThreadId, title: string, expectedRevision: number): Promise<ChatThreadShellRead> {
  return threadMutation("chat_rename_thread", { threadId, title, expectedRevision });
}

export function setChatThreadRead(threadId: ChatThreadId, read: boolean, expectedRevision: number): Promise<ChatThreadShellRead> {
  return threadMutation("chat_set_thread_read", { threadId, read, expectedRevision });
}

export function archiveChatThread(threadId: ChatThreadId, expectedRevision: number): Promise<ChatThreadShellRead> {
  return threadMutation("chat_archive_thread", { threadId, expectedRevision });
}

export function restoreChatThread(threadId: ChatThreadId, expectedRevision: number): Promise<ChatThreadShellRead> {
  return threadMutation("chat_restore_thread", { threadId, expectedRevision });
}

export async function deleteChatThreadPermanently(
  threadId: ChatThreadId,
  expectedRevision: number,
  confirmedTitle: string,
): Promise<void> {
  await invoke("chat_delete_thread_permanently", {
    dbUrl: await ensureDbUrl(),
    threadId,
    expectedRevision,
    confirmedTitle,
  });
}
