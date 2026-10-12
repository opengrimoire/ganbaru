// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getChat } from "$lib/stores/chat.svelte";
import ChatWorkspaceHeader from "./ChatWorkspaceHeader.svelte";

const projectState = vi.hoisted(() => {
  const group = {
    id: "group-1",
    name: "Work",
    icon: "📁",
    sortOrder: 0,
    collapsed: false,
    createdAt: "2026-07-26T12:00:00.000Z",
    updatedAt: "2026-07-26T12:00:00.000Z",
  };
  const project = {
    id: "project-1",
    groupId: group.id,
    name: "Ganbaru",
    icon: "💬",
    sortOrder: 0,
    status: "active" as const,
    defaultEventName: null,
    defaultEventTimeMode: "timed" as const,
    defaultEventDurationMinutes: null,
    defaultPomodoroMode: "preset" as const,
    defaultIdleSettingsSource: "global" as const,
    defaultIdlePauseEnabled: true,
    defaultIdleThresholdMinutes: 5 as const,
    createdAt: "2026-07-26T12:00:00.000Z",
    updatedAt: "2026-07-26T12:00:00.000Z",
  };
  const secondProject = {
    ...project,
    id: "project-2",
    name: "Website",
    icon: "🌐",
    sortOrder: 1,
  };
  const store = {
    selectedProjectId: project.id as string | null,
    selectedProject: project,
    selectedGroup: group,
    projects: [project, secondProject],
    groups: [group],
    customEmojis: [],
    loading: false,
    loaded: true,
    loadError: null,
    projectById: (id: string | null | undefined) => (
      [project, secondProject].find((entry) => entry.id === id)
    ),
    groupById: (id: string | null | undefined) => id === group.id ? group : undefined,
    visibleGroups: () => [group],
    projectsForGroup: (groupId: string) => groupId === group.id ? [project, secondProject] : [],
    projectsForGroupIncludingInactive: (groupId: string) => groupId === group.id ? [project, secondProject] : [],
    selectProject: vi.fn(async (_id: string) => undefined),
    addGroup: vi.fn(async () => group),
    addProject: vi.fn(async () => project),
    ensureLoaded: vi.fn(async () => undefined),
  };
  return { store };
});

vi.mock("$lib/stores/projects.svelte", () => ({
  getProjects: () => projectState.store,
}));

class ResizeObserverMock implements ResizeObserver {
  constructor(_callback: ResizeObserverCallback) {}

  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

describe("ChatWorkspaceHeader", () => {
  const mounted: { target: HTMLDivElement; component: ReturnType<typeof mount> }[] = [];

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    const chat = getChat();
    chat.workingFolders = [{
      workingFolder: {
        id: "managed",
        projectId: "project-1",
        displayName: "Ganbaru files",
        kind: "managed",
        managedRelativePath: "projects/project-1",
        sortOrder: 0,
        repositoryKind: "git",
        repositoryIdentity: "git-sha256:ganbaru",
        createdAt: "2026-07-26T12:00:00.000Z",
        updatedAt: "2026-07-26T12:00:00.000Z",
        archivedAt: null,
        revision: 1,
      },
      bindingStatus: "available",
      canonicalPath: "/work/ganbaru",
      lastVerifiedAt: "2026-07-26T12:00:00.000Z",
      currentBranch: "feat/chat-shell",
    }];
    chat.activeThreads = [];
    chat.archivedThreads = [];
    chat.activeChannels = [{
      id: "channel-general",
      conversationId: "conversation-general",
      projectId: "project-1",
      name: "general",
      topic: "Project coordination",
      isDefault: true,
      memberships: [],
      messageCount: 0,
      unreadCount: 0,
      latestPreview: null,
      lastActivityAt: "2026-07-26T12:00:00.000Z",
      attentionState: null,
      revision: 1,
      archivedAt: null,
      createdAt: "2026-07-26T12:00:00.000Z",
      updatedAt: "2026-07-26T12:00:00.000Z",
    }, {
      id: "channel-design",
      conversationId: "conversation-design",
      projectId: "project-1",
      name: "design",
      topic: "Product design",
      isDefault: false,
      memberships: [],
      messageCount: 0,
      unreadCount: 0,
      latestPreview: null,
      lastActivityAt: "2026-07-26T12:00:00.000Z",
      attentionState: null,
      revision: 1,
      archivedAt: null,
      createdAt: "2026-07-26T12:00:00.000Z",
      updatedAt: "2026-07-26T12:00:00.000Z",
    }, {
      id: "channel-research",
      conversationId: "conversation-research",
      projectId: "project-2",
      name: "research",
      topic: "Website research",
      isDefault: true,
      memberships: [],
      messageCount: 0,
      unreadCount: 0,
      latestPreview: null,
      lastActivityAt: "2026-07-26T12:00:00.000Z",
      attentionState: null,
      revision: 1,
      archivedAt: null,
      createdAt: "2026-07-26T12:00:00.000Z",
      updatedAt: "2026-07-26T12:00:00.000Z",
    }];
    chat.archivedChannels = [];
    chat.selectedChannelId = "channel-general";
    chat.selectedWorkingFolderId = "managed";
    chat.selectedThreadId = null;
    chat.inspectorOpen = false;
  });

  afterEach(async () => {
    while (mounted.length > 0) {
      const entry = mounted.pop();
      if (!entry) continue;
      await unmount(entry.component);
      entry.target.remove();
    }
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  function setup(
    explorerExpanded: boolean,
    reserveGlobalActions = true,
    editingTitle = false,
  ): HTMLDivElement {
    const target = document.createElement("div");
    target.className = "chat-workspace";
    document.body.append(target);
    const component = mount(ChatWorkspaceHeader, {
      target,
      props: {
        explorerExpanded,
        showRailButton: false,
        reserveGlobalActions,
        editingTitle,
        onOpenRail: vi.fn(),
      },
    });
    mounted.push({ target, component });
    return target;
  }

  it("uses the shared project navigation panels from both project identity triggers", async () => {
    const target = setup(true);
    const groupTrigger = target.querySelector<HTMLButtonElement>("[data-chat-group-trigger]");
    const projectTrigger = target.querySelector<HTMLButtonElement>("[data-chat-project-trigger]");

    groupTrigger?.click();
    await tick();
    expect(document.querySelector(".project-picker-panel")).not.toBeNull();
    expect(document.querySelector<HTMLInputElement>('.project-picker-panel input')?.placeholder).toBe("Search projects...");

    projectTrigger?.click();
    await tick();
    expect(document.querySelector(".project-picker-panel")).not.toBeNull();
    expect(document.querySelector(".project-picker-panel")?.textContent).toContain("Ganbaru");
  });

  it("leaves Chat synchronization to the workspace project observer", async () => {
    const chat = getChat();
    const syncProjectSelection = vi.spyOn(chat, "syncProjectSelection");
    const target = setup(true);
    target.querySelector<HTMLButtonElement>("[data-chat-project-trigger]")?.click();
    await tick();
    const projectButton = [...document.querySelectorAll<HTMLButtonElement>(
      ".project-picker-panel button",
    )].find((button) => button.textContent?.includes("Ganbaru"));

    projectButton?.click();
    await vi.waitFor(() => expect(projectState.store.selectProject).toHaveBeenCalledWith("project-1"));

    expect(syncProjectSelection).not.toHaveBeenCalled();
  });

  it("shows the channel breadcrumb only when the explorer is collapsed", () => {
    const expanded = setup(true);
    expect(expanded.querySelector("[data-chat-group-trigger]")?.textContent).toContain("Work");
    expect(expanded.querySelector("[data-chat-project-trigger]")?.textContent).toContain("Ganbaru");
    expect(expanded.querySelector("[data-chat-channel-trigger]")).toBeNull();
    expect(expanded.querySelector("[data-chat-channel-topic]")).toBeNull();
    expect(expanded.querySelector("[data-chat-context-chevron]")?.closest("button")).toBe(expanded.querySelector("[data-chat-project-trigger]"));
    expect(expanded.querySelectorAll("[data-chat-context-chevron]")).toHaveLength(1);
    expect(expanded.querySelector("[data-chat-new-channel-button]")).not.toBeNull();

    const collapsed = setup(false);
    expect(collapsed.querySelector("[data-chat-channel-trigger]")?.textContent).toContain("general");
    expect(collapsed.querySelector("[data-chat-channel-topic]")?.textContent).toBe("Project coordination");
    expect(collapsed.querySelector("[data-chat-context-chevron]")?.closest("button")).toBe(collapsed.querySelector("[data-chat-channel-trigger]"));
    expect(collapsed.querySelectorAll("[data-chat-context-chevron]")).toHaveLength(1);
    expect(collapsed.querySelector("[data-chat-new-channel-button]")).not.toBeNull();
  });

  it("opens the selected channel's members panel from the avatar stack", async () => {
    const target = setup(true);
    const membersTrigger = target.querySelector<HTMLButtonElement>("[data-chat-members-trigger]");

    expect(membersTrigger?.querySelector(".chat-members-count")?.textContent).toBe("1");
    expect(membersTrigger?.querySelector(".chat-members-stack")?.getAttribute("aria-hidden")).toBe("true");
    expect(membersTrigger?.getAttribute("aria-expanded")).toBe("false");
    expect(document.querySelector("[data-chat-members-panel]")).toBeNull();

    membersTrigger?.click();
    await vi.waitFor(() => {
      expect(document.querySelector("[data-chat-members-panel]")).not.toBeNull();
    });

    const panel = document.querySelector("[data-chat-members-panel]");
    expect(membersTrigger?.getAttribute("aria-expanded")).toBe("true");
    expect(panel?.getAttribute("role")).toBe("dialog");
    expect(panel?.textContent).toContain("#general");
    expect(panel?.textContent).toContain("Project coordination");
    expect(panel?.textContent).toContain("Owner");

    membersTrigger?.click();
    await tick();

    expect(document.querySelector("[data-chat-members-panel]")).toBeNull();
    expect(membersTrigger?.getAttribute("aria-expanded")).toBe("false");
  });

  it("opens the current project's complete channel navigator from the channel segment", async () => {
    const target = setup(false);
    const channelTrigger = target.querySelector("[data-chat-channel-trigger]");

    (channelTrigger as HTMLButtonElement | null)?.click();
    await tick();

    const channelPicker = document.querySelector("[data-chat-channel-picker]");
    expect(channelPicker?.textContent).toContain("general");
    expect(channelPicker?.textContent).toContain("design");
    expect(channelPicker?.querySelectorAll("[data-chat-channel-option]")).toHaveLength(2);
    expect(channelTrigger?.querySelector("svg")?.getAttribute("stroke-width")).toBe("1.5");
    const search = channelPicker?.querySelector<HTMLInputElement>("input");
    expect(search).not.toBeNull();
    search!.value = "design";
    search!.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(channelPicker?.querySelectorAll("[data-chat-channel-option]")).toHaveLength(1);
    expect(channelPicker?.querySelector("[data-chat-channel-option]")?.textContent).toContain("design");
  });

  it("cascades from a hovered project to that project's channels", async () => {
    const target = setup(true);
    target.querySelector<HTMLButtonElement>("[data-chat-project-trigger]")?.click();
    await tick();
    const projectButton = [...document.querySelectorAll<HTMLButtonElement>(
      ".project-picker-panel button",
    )].find((button) => button.textContent?.includes("Website"));

    projectButton?.focus();
    await tick();
    await tick();

    const channelPicker = document.querySelector("[data-chat-channel-picker]");
    expect(channelPicker?.textContent).toContain("research");
    expect(channelPicker?.textContent).not.toContain("general");
    expect(channelPicker?.querySelector("input")).toBeNull();
    expect(target.querySelectorAll('[role="dialog"] input')).toHaveLength(1);
  });

  it("keeps the full group to project to channel hover hierarchy", async () => {
    const target = setup(true);
    target.querySelector<HTMLButtonElement>("[data-chat-group-trigger]")?.click();
    await tick();
    const groupButton = [...document.querySelectorAll<HTMLButtonElement>(
      ".project-picker-panel button",
    )].find((button) => button.textContent?.includes("Work"));

    groupButton?.focus();
    await tick();
    await tick();
    const projectButton = [...document.querySelectorAll<HTMLButtonElement>(
      ".project-picker-panel.fixed button",
    )].find((button) => button.textContent?.includes("Ganbaru"));

    projectButton?.focus();
    await tick();
    await tick();

    expect(document.querySelector("[data-chat-channel-picker]")?.textContent).toContain("general");
    expect(target.querySelectorAll('[role="dialog"] input')).toHaveLength(1);
    expect(target.querySelectorAll('.project-picker-panel.fixed input, [data-chat-channel-picker] input')).toHaveLength(0);
  });

  it("reveals the chat segment editor when renaming from an expanded explorer", () => {
    const chat = getChat();
    chat.activeChannels = chat.activeChannels.map((channel) => ({
      ...channel,
      id: "channel-shell-redesign",
      name: "shell-redesign",
      isDefault: false,
    }));
    chat.selectedChannelId = "channel-shell-redesign";
    const target = setup(true, true, true);

    expect(target.querySelector("[data-chat-title-editor]")).not.toBeNull();
    expect(target.querySelector("[data-chat-project-trigger]")).not.toBeNull();
  });

});
