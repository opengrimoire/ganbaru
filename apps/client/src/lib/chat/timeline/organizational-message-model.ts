import type { ChatMessageRead, ChatReplyThreadSummaryRead } from "$lib/chat/contracts";

export interface ChatMessageReactionParticipant {
  participantId: string;
  displayName: string;
}

export interface ChatMessageReaction {
  value: string;
  participants: readonly ChatMessageReactionParticipant[];
}

/** Locates the first unread message while excluding messages authored by the local user. */
export function unreadMessageStartIndex(
  messages: readonly ChatMessageRead[],
  unreadCount: number,
): number {
  let remaining = Math.max(0, Math.floor(unreadCount));
  if (remaining === 0) return messages.length;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index]?.author.kind === "local_user") continue;
    remaining -= 1;
    if (remaining === 0) return index;
  }
  return 0;
}

/** Applies the latest reply-thread summary to its root message when it is loaded. */
export function applyReplyThreadSummary(
  messages: readonly ChatMessageRead[],
  summary: ChatReplyThreadSummaryRead,
): ChatMessageRead[] {
  return messages.map((message) => message.replyThread?.id === summary.id
    ? { ...message, replyThread: summary }
    : message);
}

/** Returns whether a picker value is supported by message reactions. */
export function isChatMessageReactionValue(value: string): boolean {
  const normalized = value.trim();
  return (normalized.startsWith("emoji:") && normalized.slice("emoji:".length).trim().length > 0)
    || (normalized.startsWith("custom-emoji:") && normalized.slice("custom-emoji:".length).trim().length > 0);
}

/** Adds or removes one participant from a reaction without disturbing other participants. */
export function toggleChatMessageReactionParticipant(
  reactions: readonly ChatMessageReaction[],
  value: string,
  participant: ChatMessageReactionParticipant,
): ChatMessageReaction[] {
  const normalizedValue = value.trim();
  if (!isChatMessageReactionValue(normalizedValue)) return [...reactions];
  const reactionIndex = reactions.findIndex((reaction) => reaction.value === normalizedValue);
  if (reactionIndex < 0) {
    return [...reactions, { value: normalizedValue, participants: [participant] }];
  }

  const reaction = reactions[reactionIndex];
  if (!reaction) return [...reactions];
  const alreadySelected = reaction.participants.some(
    (entry) => entry.participantId === participant.participantId,
  );
  const participants = alreadySelected
    ? reaction.participants.filter((entry) => entry.participantId !== participant.participantId)
    : [...reaction.participants, participant];
  if (participants.length === 0) {
    return reactions.filter((_, index) => index !== reactionIndex);
  }
  return reactions.map((entry, index) => index === reactionIndex
    ? { ...entry, participants }
    : entry);
}
