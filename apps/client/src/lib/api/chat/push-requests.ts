import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type {
  ChatConversationItemId,
  ChatPushRequestStateRead,
  ConfirmPushRequestRequest,
  PostChatMessageResult,
} from "$lib/chat/contracts";
import { parseChatPushRequestStates, parsePostChatMessageResult } from "$lib/chat/validation";
import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";

/** Push requests are confirmed only where Chat runs teammates, since the push runs in the folder. */
export const chatPushRequestsAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "chat.local-execution");

/**
 * Pushes the commit a teammate's open push request names and posts the confirming reply. Rust refuses
 * when the request is no longer open, the teammate lost its publish grant, or the folder changed.
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
