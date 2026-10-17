import type { ChatConversationMembershipRead } from "$lib/chat/contracts";

/** Active members of a channel grouped the way member lists show them. */
export interface ChannelMemberSummary {
  /** Other people in the channel; the local person is always a member and is never listed here. */
  readonly people: readonly ChatConversationMembershipRead[];
  readonly teammateIds: ReadonlySet<string>;
  /** Everyone, counting the local person once. */
  readonly count: number;
}

/**
 * Groups a channel's active memberships into people and teammates. The channel creator is a member from the
 * start, so the count includes the local person even when the projection carries no membership row for them yet.
 */
export function summarizeChannelMembers(memberships: readonly ChatConversationMembershipRead[]): ChannelMemberSummary {
  const people: ChatConversationMembershipRead[] = [];
  const teammateIds = new Set<string>();
  for (const membership of memberships) {
    if (membership.removedAt !== null) continue;
    if (membership.participant.kind === "ai_teammate") teammateIds.add(membership.participant.id);
    else if (membership.participant.kind === "human") people.push(membership);
  }
  return { people, teammateIds, count: 1 + people.length + teammateIds.size };
}
