import type {
  ChatChannelId,
  ChatChannelNoteId,
  ChatCommandId,
  ChatConversationItemId,
  ChatParticipantId,
  UtcTimestamp,
} from "./common";

/** Most live notes a person may keep in one channel; merges from other devices may exceed it. */
export const CHAT_MAX_ACTIVE_CHANNEL_NOTES = 50;
/** Longest channel note, in characters. */
export const CHAT_MAX_CHANNEL_NOTE_CHARS = 1_000;
/** Most note proposals one state read may name. */
export const CHAT_MAX_NOTE_PROPOSAL_STATE_READS = 200;

/** A live note people keep for every agent working in one channel. */
export interface ChatChannelNoteRead {
  id: ChatChannelNoteId;
  channelId: ChatChannelId;
  body: string;
  /** The agent proposal the note was saved from, when it came from one. */
  sourceItemId: ChatConversationItemId | null;
  /** The agent that proposed the note, when it came from a proposal. */
  proposedBy: ChatParticipantId | null;
  /** The person who last saved the note, or `null` when that person was removed. */
  editedBy: ChatParticipantId | null;
  revision: number;
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
}

/** A person writes a new note in a channel. */
export interface CreateChatChannelNoteRequest {
  clientCommandId: ChatCommandId;
  channelId: ChatChannelId;
  body: string;
}

/** A person rewords a note at the revision they saw. */
export interface UpdateChatChannelNoteRequest {
  clientCommandId: ChatCommandId;
  noteId: ChatChannelNoteId;
  expectedRevision: number;
  body: string;
}

/** A person deletes a note at the revision they saw. */
export interface DeleteChatChannelNoteRequest {
  clientCommandId: ChatCommandId;
  noteId: ChatChannelNoteId;
  expectedRevision: number;
}

/** A person saves an agent's note proposal, with `body` replacing the suggested text when given. */
export interface AcceptChatNoteProposalRequest {
  clientCommandId: ChatCommandId;
  itemId: ChatConversationItemId;
  body?: string;
}

/** A person declines an agent's note proposal on every device. */
export interface DismissChatNoteProposalRequest {
  clientCommandId: ChatCommandId;
  itemId: ChatConversationItemId;
}

export const CHAT_NOTE_PROPOSAL_STATES = ["pending", "saved", "dismissed"] as const;
/** Pending until a person saves or dismisses the proposal; deleting the saved note reads as dismissed. */
export type ChatNoteProposalState = (typeof CHAT_NOTE_PROPOSAL_STATES)[number];

/** Where one note proposal stands on every linked device. */
export interface ChatNoteProposalStateRead {
  proposalItemId: ChatConversationItemId;
  state: ChatNoteProposalState;
}
