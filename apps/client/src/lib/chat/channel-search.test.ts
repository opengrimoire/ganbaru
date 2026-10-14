import { describe, expect, it } from "vitest";
import type { ChatChannelRead } from "$lib/chat/contracts";
import { chatChannelMatchesSearch, chatChannelSearchGroups } from "./channel-search";

function channel(id: string, projectId: string, name: string, topic = ""): ChatChannelRead {
  const timestamp = "2026-10-13T12:00:00.000Z";
  return {
    id,
    conversationId: `conversation-${id}`,
    projectId,
    name,
    topic,
    isDefault: false,
    memberships: [],
    messageCount: 0,
    unreadCount: 0,
    latestPreview: null,
    lastActivityAt: timestamp,
    attentionState: null,
    revision: 1,
    archivedAt: null,
    createdAt: timestamp,
    updatedAt: timestamp,
  };
}

const channelsByProject: Record<string, ChatChannelRead[]> = {
  alpha: [channel("a-general", "alpha", "general"), channel("a-release", "alpha", "release", "Ship planning")],
  beta: [channel("b-design", "beta", "design"), channel("b-releases", "beta", "Releases")],
  gamma: [channel("g-general", "gamma", "general")],
};

function channelsFor(projectId: string): ChatChannelRead[] {
  return channelsByProject[projectId] ?? [];
}

describe("chatChannelSearchGroups", () => {
  it("groups matching channels by project in the given project order", () => {
    const groups = chatChannelSearchGroups([{ id: "beta" }, { id: "alpha" }, { id: "gamma" }], channelsFor, "release");

    expect(groups.map((group) => [group.project.id, group.channels.map((entry) => entry.id)])).toEqual([
      ["beta", ["b-releases"]],
      ["alpha", ["a-release"]],
    ]);
  });

  it("matches topics and ignores case and surrounding spaces", () => {
    const groups = chatChannelSearchGroups([{ id: "alpha" }], channelsFor, "  SHIP ");

    expect(groups[0]?.channels.map((entry) => entry.id)).toEqual(["a-release"]);
  });

  it("leaves out projects that are not listed", () => {
    const groups = chatChannelSearchGroups([{ id: "gamma" }], channelsFor, "general");

    expect(groups.map((group) => group.project.id)).toEqual(["gamma"]);
  });

  it("returns nothing for a blank query", () => {
    expect(chatChannelSearchGroups([{ id: "alpha" }], channelsFor, "   ")).toEqual([]);
  });
});

describe("chatChannelMatchesSearch", () => {
  it("matches every channel when the normalized query is empty", () => {
    expect(chatChannelMatchesSearch({ name: "general", topic: "" }, "")).toBe(true);
  });

  it("rejects a channel whose name and topic both miss", () => {
    expect(chatChannelMatchesSearch({ name: "general", topic: "Daily talk" }, "release")).toBe(false);
  });
});
