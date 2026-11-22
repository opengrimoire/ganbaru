import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatChannelRead } from "$lib/chat/contracts";
import { ChatChannelNavigationController } from "./channel-navigation-controller.svelte";

vi.mock("$lib/vault/config", () => ({ getConfigKey: () => undefined, setConfigKey: () => undefined }));

const api = vi.hoisted(() => ({ listChatChannels: vi.fn() }));

vi.mock("$lib/api/chat", () => ({ listChatChannels: api.listChatChannels }));

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

describe("ChatChannelNavigationController archive loading", () => {
  let currentProjectId: string | null = "project-a";

  function controllerFollowingProject(): ChatChannelNavigationController {
    return new ChatChannelNavigationController({
      currentProjectId: () => currentProjectId,
      selectedChannelId: () => null,
      selectChannel: () => Promise.resolve(),
      onChanged: () => undefined,
    });
  }

  function deferredListing() {
    let resolve: (channels: ChatChannelRead[]) => void = () => undefined;
    const promise = new Promise<ChatChannelRead[]>((settle) => { resolve = settle; });
    return { promise, resolve };
  }

  beforeEach(() => {
    currentProjectId = "project-a";
    api.listChatChannels.mockReset();
  });

  it("stops showing archive loading once another project loads", async () => {
    const archived = deferredListing();
    api.listChatChannels.mockReturnValueOnce(archived.promise).mockResolvedValueOnce([]);
    const controller = controllerFollowingProject();

    const loading = controller.loadArchived("project-a");
    currentProjectId = "project-b";
    await controller.loadProject("project-b");
    archived.resolve([channel("old", "project-a", "old", "2026-01-02T00:00:00Z")]);
    await loading;

    expect(controller.archivedChannelsLoading).toBe(false);
    expect(controller.archivedChannels).toEqual([]);
  });

  it("keeps the latest archive listing when an earlier one finishes after it", async () => {
    const earlier = deferredListing();
    const later = deferredListing();
    api.listChatChannels.mockReturnValueOnce(earlier.promise).mockReturnValueOnce(later.promise);
    const controller = controllerFollowingProject();

    const first = controller.loadArchived("project-a");
    const second = controller.loadArchived("project-a");
    earlier.resolve([]);
    await first;
    expect(controller.archivedChannelsLoading).toBe(true);
    later.resolve([channel("old", "project-a", "old", "2026-01-02T00:00:00Z")]);
    await second;

    expect(controller.archivedChannelsLoading).toBe(false);
    expect(controller.archivedChannels.map(({ id }) => id)).toEqual(["old"]);
  });
});

describe("ChatChannelNavigationController open archive", () => {
  it("lists the loaded project's archive when the archive is open", async () => {
    const archivedChannel = channel("old", "project-a", "old", "2026-01-02T00:00:00Z");
    api.listChatChannels.mockReset();
    api.listChatChannels.mockImplementation(async (_projectId: string, archived: boolean) => archived ? [archivedChannel] : []);
    const controller = createController("project-a");
    controller.archiveOpen = true;

    await controller.loadProject("project-a");
    await Promise.resolve();

    expect(controller.archivedChannels.map(({ id }) => id)).toEqual(["old"]);
    expect(controller.archivedChannelsLoading).toBe(false);
  });
});

describe("ChatChannelNavigationController archive loading for another project", () => {
  it("ends archive loading when the listed project is no longer shown", async () => {
    let currentProjectId = "project-a";
    let resolve: (channels: ChatChannelRead[]) => void = () => undefined;
    api.listChatChannels.mockReset();
    api.listChatChannels.mockReturnValueOnce(new Promise<ChatChannelRead[]>((settle) => { resolve = settle; }));
    const controller = new ChatChannelNavigationController({
      currentProjectId: () => currentProjectId,
      selectedChannelId: () => null,
      selectChannel: () => Promise.resolve(),
      onChanged: () => undefined,
    });

    const loading = controller.loadArchived("project-a");
    currentProjectId = "project-b";
    resolve([]);
    await loading;

    expect(controller.archivedChannelsLoading).toBe(false);
    expect(controller.archivedChannels).toEqual([]);
  });
});
