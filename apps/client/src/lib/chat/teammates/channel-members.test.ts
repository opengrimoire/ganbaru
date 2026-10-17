import { describe, expect, it } from "vitest";
import type { ChatConversationMembershipRead, ChatParticipantKind } from "$lib/chat/contracts";
import { summarizeChannelMembers } from "./channel-members";

function membership(id: string, kind: ChatParticipantKind, removedAt: string | null = null): ChatConversationMembershipRead {
  return {
    conversationId: "conversation-1",
    participant: { id, kind, displayName: id, avatar: { schemaVersion: 1, value: {} }, revision: 1, archivedAt: null },
    aiAccess: null,
    revision: 1,
    removedAt,
  };
}

describe("summarizeChannelMembers", () => {
  it("counts the local person once even without a membership row", () => {
    expect(summarizeChannelMembers([])).toEqual({ people: [], teammateIds: new Set(), count: 1 });
    const summary = summarizeChannelMembers([membership("participant:local-owner", "local_user")]);
    expect(summary.count).toBe(1);
    expect(summary.people).toEqual([]);
  });

  it("groups other people and teammates and ignores removed memberships", () => {
    const summary = summarizeChannelMembers([
      membership("participant:local-owner", "local_user"),
      membership("participant:ana", "human"),
      membership("participant:bot", "ai_teammate"),
      membership("participant:old-bot", "ai_teammate", "2026-10-01T00:00:00.000Z"),
      membership("participant:gone", "human", "2026-10-01T00:00:00.000Z"),
    ]);
    expect(summary.people.map((entry) => entry.participant.id)).toEqual(["participant:ana"]);
    expect([...summary.teammateIds]).toEqual(["participant:bot"]);
    expect(summary.count).toBe(3);
  });
});
