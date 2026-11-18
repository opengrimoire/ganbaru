import { describe, expect, it } from "vitest";
import type { ChatConversationMembershipRead, ChatParticipantKind } from "$lib/chat/contracts";
import { isActiveChannelMember, summarizeChannelMembers } from "./channel-members";

function membership(id: string, kind: ChatParticipantKind, removedAt: string | null = null): ChatConversationMembershipRead {
  return {
    conversationId: "conversation-1",
    participant: { id, kind, displayName: id, avatar: { schemaVersion: 1, value: {} }, revision: 1, archivedAt: null },
    agentInChannel: kind === "ai_agent" && removedAt === null,
    revision: 1,
    removedAt,
  };
}

describe("summarizeChannelMembers", () => {
  it("counts the local person once even without a membership row", () => {
    expect(summarizeChannelMembers([])).toEqual({ people: [], agentIds: new Set(), count: 1 });
    const summary = summarizeChannelMembers([membership("participant:local-owner", "local_user")]);
    expect(summary.count).toBe(1);
    expect(summary.people).toEqual([]);
  });

  it("groups other people and agents and ignores removed memberships", () => {
    const summary = summarizeChannelMembers([
      membership("participant:local-owner", "local_user"),
      membership("participant:ana", "human"),
      membership("participant:bot", "ai_agent"),
      membership("participant:old-bot", "ai_agent", "2026-10-01T00:00:00.000Z"),
      membership("participant:gone", "human", "2026-10-01T00:00:00.000Z"),
    ]);
    expect(summary.people.map((entry) => entry.participant.id)).toEqual(["participant:ana"]);
    expect([...summary.agentIds]).toEqual(["participant:bot"]);
    expect(summary.count).toBe(3);
  });
});

describe("isActiveChannelMember", () => {
  it("keeps people and agents added to the channel", () => {
    expect(isActiveChannelMember(membership("participant:ana", "human"))).toBe(true);
    expect(isActiveChannelMember(membership("participant:bot", "ai_agent"))).toBe(true);
  });

  it("drops removed memberships and agents whose channel membership ended", () => {
    expect(isActiveChannelMember(membership("participant:ana", "human", "2026-10-01T00:00:00.000Z"))).toBe(false);
    expect(isActiveChannelMember({ ...membership("participant:bot", "ai_agent"), agentInChannel: false })).toBe(false);
  });
});
