import { describe, expect, it, vi } from "vitest";
import type { ChatChannelRead } from "$lib/chat/contracts";
import { ChatChannelNavigationController } from "./channel-navigation-controller.svelte";

vi.mock("$lib/vault/config", () => ({ getConfigKey: () => undefined, setConfigKey: () => undefined }));

function channel(id: string, projectId: string, name: string, archivedAt: string | null = null): ChatChannelRead {
  return {
    id,
    conversationId: `conversation-${id}`,
    projectId,
    name,
    topic: "",
    isDefault: false,
    memberships: [],
    messageCount: 0,
    unreadCount: 0,
    unreadReplyCount: 0,
    unseenFailure: false,
    latestPreview: null,
    lastActivityAt: "2026-01-01T00:00:00Z",
    attentionState: null,
    revision: 1,
    archivedAt,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

function createController(currentProjectId: string | null): ChatChannelNavigationController {
  return new ChatChannelNavigationController({
    currentProjectId: () => currentProjectId,
    selectedChannelId: () => null,
    selectChannel: () => Promise.resolve(),
    onChanged: () => undefined,
  });
}

describe("ChatChannelNavigationController navigation refresh", () => {
  it("drops channels missing from a fresh listing and keeps the current project's channels sorted", () => {
    const controller = createController("project-a");
    controller.upsert(channel("general", "project-a", "general"));
    controller.upsert(channel("deleted", "project-a", "design"));

    controller.refreshNavigation([
      channel("general", "project-a", "general"),
      channel("added", "project-a", "backend"),
      channel("other", "project-b", "notes"),
    ]);

    expect(controller.activeChannels.map(({ id }) => id)).toEqual(["added", "general"]);
    expect(controller.find("deleted")).toBeNull();
    expect(controller.find("other")).toBeNull();
  });

  it("keeps loaded archived channels, which the active listing never includes, unless a listing shows them active again", () => {
    const controller = createController("project-a");
    controller.upsert(channel("old", "project-a", "old", "2026-01-02T00:00:00Z"));
    controller.upsert(channel("restored", "project-a", "restored", "2026-01-02T00:00:00Z"));

    controller.refreshNavigation([channel("restored", "project-a", "restored")]);

    expect(controller.archivedChannels.map(({ id }) => id)).toEqual(["old"]);
    expect(controller.activeChannels.map(({ id }) => id)).toEqual(["restored"]);
  });

  it("shows no active channels once the current project is gone", () => {
    const controller = createController(null);

    controller.refreshNavigation([channel("general", "project-a", "general")]);

    expect(controller.activeChannels).toEqual([]);
  });
});
