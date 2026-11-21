import type { ChatConversationMembershipRead } from "$lib/chat/contracts";

/** Active members of a channel grouped the way member lists show them. */
export interface ChannelMemberSummary {
  /** Other people in the channel; the local person is always a member and is never listed here. */
  readonly people: readonly ChatConversationMembershipRead[];
  readonly agentIds: ReadonlySet<string>;
  /** Everyone, counting the local person once. */
  readonly count: number;
}

/**
 * Whether a membership is active in the channel now: not removed, and for an agent also added to the channel, which
 * the backend tracks apart from the conversation membership.
 */
export function isActiveChannelMember(
  membership: Pick<ChatConversationMembershipRead, "participant" | "agentInChannel" | "removedAt">,
): boolean {
  if (membership.removedAt !== null) return false;
  return membership.participant.kind !== "ai_agent" || membership.agentInChannel;
}

/**
 * Groups a channel's active memberships into people and agents. The channel creator is a member from the
 * start, so the count includes the local person even when the projection carries no membership row for them yet.
 */
export function summarizeChannelMembers(memberships: readonly ChatConversationMembershipRead[]): ChannelMemberSummary {
  const people: ChatConversationMembershipRead[] = [];
  const agentIds = new Set<string>();
  for (const membership of memberships) {
    if (!isActiveChannelMember(membership)) continue;
    if (membership.participant.kind === "ai_agent") agentIds.add(membership.participant.id);
    else if (membership.participant.kind === "human") people.push(membership);
  }
  return { people, agentIds, count: 1 + people.length + agentIds.size };
}
