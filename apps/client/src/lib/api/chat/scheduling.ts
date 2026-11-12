import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type {
  ChatConversationItemId,
  ChatQuietPeriodPreferences,
  ChatQuietStateRead,
  ChatScheduledMessageRead,
  ChatScheduleProposalStateRead,
  ConfirmScheduleProposalRequest,
} from "$lib/chat/contracts";
import {
  parseChatQuietPeriodPreferences,
  parseChatQuietState,
  parseChatScheduledMessage,
  parseChatScheduleProposalStates,
} from "$lib/chat/validation";

/** Reads this device's quiet windows. */
export async function readChatQuietPeriods(): Promise<ChatQuietPeriodPreferences> {
  return parseChatQuietPeriodPreferences(await invoke<unknown>("chat_read_quiet_periods"));
}

/** Saves this device's quiet windows and returns what was stored. Quiet periods never sync. */
export async function saveChatQuietPeriods(
  preferences: ChatQuietPeriodPreferences,
): Promise<ChatQuietPeriodPreferences> {
  return parseChatQuietPeriodPreferences(await invoke<unknown>("chat_save_quiet_periods", { preferences }));
}

/** Reads whether this device is quiet now because of a quiet window or a Focus phase. */
export async function readChatQuietState(): Promise<ChatQuietStateRead> {
  return parseChatQuietState(await invoke<unknown>("chat_read_quiet_state"));
}

/** Schedules the message a person confirmed from a teammate's schedule proposal. */
export async function confirmChatScheduleProposal(
  request: ConfirmScheduleProposalRequest,
): Promise<ChatScheduledMessageRead> {
  return parseChatScheduledMessage(await invoke<unknown>("chat_confirm_schedule_proposal", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

/** Hides a schedule proposal on this device only. */
export async function dismissChatScheduleProposal(proposalItemId: ChatConversationItemId): Promise<void> {
  await invoke("chat_dismiss_schedule_proposal", { proposalItemId });
}

/** Reads where each named schedule proposal stands on this device; at most 200 per read. */
export async function readChatScheduleProposalStates(
  proposalItemIds: readonly ChatConversationItemId[],
): Promise<ChatScheduleProposalStateRead[]> {
  if (proposalItemIds.length === 0) return [];
  return parseChatScheduleProposalStates(await invoke<unknown>("chat_read_schedule_proposal_states", {
    dbUrl: await ensureDbUrl(), proposalItemIds,
  }));
}
