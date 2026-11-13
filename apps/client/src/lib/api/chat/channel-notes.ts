import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import {
  CHAT_MAX_NOTE_PROPOSAL_STATE_READS,
  type AcceptChatNoteProposalRequest,
  type ChatChannelId,
  type ChatChannelNoteRead,
  type ChatConversationItemId,
  type ChatNoteProposalStateRead,
  type CreateChatChannelNoteRequest,
  type DeleteChatChannelNoteRequest,
  type DismissChatNoteProposalRequest,
  type UpdateChatChannelNoteRequest,
} from "$lib/chat/contracts";
import { parseChatChannelNote, parseChatChannelNotes, parseChatNoteProposalStates } from "$lib/chat/validation";

/** Lists the live notes of a channel the local person belongs to, newest updated first. */
export async function listChatChannelNotes(channelId: ChatChannelId): Promise<ChatChannelNoteRead[]> {
  return parseChatChannelNotes(await invoke<unknown>("chat_list_channel_notes", {
    dbUrl: await ensureDbUrl(), channelId,
  }));
}

/** Saves a note a person wrote in a channel. */
export async function createChatChannelNote(request: CreateChatChannelNoteRequest): Promise<ChatChannelNoteRead> {
  return parseChatChannelNote(await invoke<unknown>("chat_create_channel_note", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

/** Rewords a channel note at the revision the person saw. */
export async function updateChatChannelNote(request: UpdateChatChannelNoteRequest): Promise<ChatChannelNoteRead> {
  return parseChatChannelNote(await invoke<unknown>("chat_update_channel_note", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

/** Deletes a channel note at the revision the person saw. */
export async function deleteChatChannelNote(request: DeleteChatChannelNoteRequest): Promise<void> {
  await invoke("chat_delete_channel_note", { dbUrl: await ensureDbUrl(), request });
}

/** Saves a teammate's note proposal as a channel note, as suggested or with the person's text. */
export async function acceptChatNoteProposal(request: AcceptChatNoteProposalRequest): Promise<ChatChannelNoteRead> {
  return parseChatChannelNote(await invoke<unknown>("chat_accept_note_proposal", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

/** Dismisses a teammate's note proposal so no device saves it. */
export async function dismissChatNoteProposal(request: DismissChatNoteProposalRequest): Promise<void> {
  await invoke("chat_dismiss_note_proposal", { dbUrl: await ensureDbUrl(), request });
}

/** Reads where each named note proposal stands; at most {@link CHAT_MAX_NOTE_PROPOSAL_STATE_READS} per read. */
export async function readChatNoteProposalStates(
  proposalItemIds: readonly ChatConversationItemId[],
): Promise<ChatNoteProposalStateRead[]> {
  if (proposalItemIds.length === 0) return [];
  if (proposalItemIds.length > CHAT_MAX_NOTE_PROPOSAL_STATE_READS) {
    throw new Error(`Read at most ${CHAT_MAX_NOTE_PROPOSAL_STATE_READS} note proposals at once`);
  }
  return parseChatNoteProposalStates(await invoke<unknown>("chat_read_note_proposal_states", {
    dbUrl: await ensureDbUrl(), proposalItemIds,
  }));
}
