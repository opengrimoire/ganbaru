import { CHAT_MAX_ACTIVE_CHANNEL_NOTES, CHAT_MAX_CHANNEL_NOTE_CHARS } from "$lib/chat/contracts";
import {
  chatParticipantDisplayName,
  type ChatParticipantDisplaySource,
} from "$lib/chat/agents/participant-display";

/** Where the names a note credits come from. */
export interface ChannelNoteNameSources {
  /** Channel members and agents, in any order; the first match wins. */
  readonly participants: readonly ChatParticipantDisplaySource[];
  /** Current profile name of the local person, possibly empty. */
  readonly localProfileDisplayName: string;
  /** Localized label for the local person when the profile name is empty. */
  readonly localFallback: string;
  /** Localized label for a participant this device no longer knows. */
  readonly unknownName: string;
}

/**
 * Resolves the name a note credits for `participantId`.
 *
 * @param participantId The saving person or proposing agent, or `null` when the note credits nobody.
 * @param sources Known participants and the localized fallbacks.
 * @returns The display name, the unknown label for an unlisted participant, or `null` when there is nobody to credit.
 */
export function channelNoteParticipantName(
  participantId: string | null,
  sources: ChannelNoteNameSources,
): string | null {
  if (participantId === null) return null;
  const participant = sources.participants.find((candidate) => candidate.id === participantId);
  if (!participant) return sources.unknownName;
  return chatParticipantDisplayName(participant, sources.localProfileDisplayName, sources.localFallback);
}

/** Why a note draft cannot be saved yet. */
export type ChannelNoteDraftProblem = "empty" | "too_long" | "limit";

/**
 * Checks a note draft against the bounds the backend enforces.
 *
 * @param body The draft text; surrounding whitespace is ignored as the backend trims it.
 * @param liveNoteCount Live notes the channel holds now.
 * @param adding Whether the draft is a new note, which needs room under the limit; an edit does not.
 * @returns The first problem, or `null` when the draft can be saved.
 */
export function channelNoteDraftProblem(
  body: string,
  liveNoteCount: number,
  adding: boolean,
): ChannelNoteDraftProblem | null {
  if (adding && liveNoteCount >= CHAT_MAX_ACTIVE_CHANNEL_NOTES) return "limit";
  const text = body.trim();
  if (!text) return "empty";
  if ([...text].length > CHAT_MAX_CHANNEL_NOTE_CHARS) return "too_long";
  return null;
}
