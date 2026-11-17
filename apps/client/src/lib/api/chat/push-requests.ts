import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type {
  ChatConversationItemId,
  ChatPushApproval,
  ChatPushRequestStateRead,
  ConfirmPushRequestRequest,
  PostChatMessageResult,
} from "$lib/chat/contracts";
import { parseChatPushApprovals, parseChatPushRequestStates, parsePostChatMessageResult } from "$lib/chat/validation";
import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";

/** Push requests are confirmed only where Chat runs agents, since the push runs in the folder. */
export const chatPushRequestsAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "chat.local-execution");

/**
 * Pushes the commit an agent's open push request names and posts the confirming reply. Rust refuses
 * when the request is no longer open, the agent lost its publish grant, or the folder changed.
 */
export async function confirmChatPushRequest(request: ConfirmPushRequestRequest): Promise<PostChatMessageResult> {
  if (!chatPushRequestsAvailable) throw new Error("Chat push requests need local execution on this device");
  return parsePostChatMessageResult(await invoke<unknown>("chat_confirm_push_request", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

/** Reads where each named push request stands; at most 200 per read. */
export async function readChatPushRequestStates(
  requestItemIds: readonly ChatConversationItemId[],
): Promise<ChatPushRequestStateRead[]> {
  if (requestItemIds.length === 0 || !chatPushRequestsAvailable) return [];
  return parseChatPushRequestStates(await invoke<unknown>("chat_read_push_request_states", {
    dbUrl: await ensureDbUrl(), requestItemIds,
  }));
}

/** Lists an agent's active standing push approvals, by folder name, push URL, and branch. */
export async function listChatPushApprovals(agentId: string): Promise<ChatPushApproval[]> {
  return parseChatPushApprovals(await invoke<unknown>("chat_list_push_approvals", {
    dbUrl: await ensureDbUrl(), agentId,
  }));
}

/** Revokes a standing push approval, so later matching push requests ask again on every linked device. */
export async function revokeChatPushApproval(approvalId: string): Promise<void> {
  await invoke<unknown>("chat_revoke_push_approval", { dbUrl: await ensureDbUrl(), request: { approvalId } });
}
