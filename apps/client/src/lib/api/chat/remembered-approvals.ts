import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type { ChatChannelId, ChatChannelRememberedApprovalRead } from "$lib/chat/contracts";
import { parseChatRememberedApprovals } from "$lib/chat/validation";

/** Lists a channel's active remembered approvals, newest first, by agent and the origin, command, or tool each allows. */
export async function listChatRememberedApprovals(channelId: ChatChannelId): Promise<ChatChannelRememberedApprovalRead[]> {
  return parseChatRememberedApprovals(await invoke<unknown>("chat_list_remembered_approvals", {
    dbUrl: await ensureDbUrl(), channelId,
  }));
}

/** Revokes a remembered approval, so later matching agent requests ask again on every linked device. */
export async function revokeChatRememberedApproval(approvalId: string): Promise<void> {
  await invoke<unknown>("chat_revoke_remembered_approval", { dbUrl: await ensureDbUrl(), request: { approvalId } });
}
