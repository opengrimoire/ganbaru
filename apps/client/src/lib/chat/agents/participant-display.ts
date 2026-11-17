import type { ChatParticipantKind } from "$lib/chat/contracts";

export interface ChatParticipantDisplaySource {
  id: string;
  kind: ChatParticipantKind;
  displayName: string;
}

export const LOCAL_CHAT_PARTICIPANT_ID = "participant:local-owner";

/**
 * Resolves a participant label without rewriting durable message history.
 *
 * @param participant Participant identity returned by the Chat projection.
 * @param localProfileDisplayName Current folder-local profile display name.
 * @param localFallback Localized fallback used when the profile name is empty.
 * @returns The current local identity or the participant's durable display name.
 */
export function chatParticipantDisplayName(
  participant: ChatParticipantDisplaySource,
  localProfileDisplayName: string,
  localFallback: string,
): string {
  if (participant.id !== LOCAL_CHAT_PARTICIPANT_ID || participant.kind !== "local_user") {
    return participant.displayName;
  }
  return localProfileDisplayName.trim() || localFallback;
}
