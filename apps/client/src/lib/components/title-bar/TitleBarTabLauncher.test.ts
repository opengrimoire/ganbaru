// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildProjectLists } from "$lib/projects/lists";
import type { ProjectGroup } from "$lib/projects/types";

interface TestProject {
  id: string;
  name: string;
  icon: string;
  status: "active";
  groupId: string | null;
}

const group: ProjectGroup = { id: "work", name: "Work", icon: "lucide:folder", orderKey: "a0", collapsed: false, createdAt: "c", updatedAt: "u" };
const report: TestProject = { id: "report", name: "Report", icon: "lucide:folder", status: "active", groupId: "work" };
const garden: TestProject = { id: "garden", name: "Garden", icon: "lucide:folder", status: "active", groupId: null };
const channels = [
  { id: "general", projectId: "report", name: "general", topic: "" },
  { id: "design", projectId: "report", name: "design", topic: "" },
];

const tabs = {
  tabs: [],
  activeTabId: null,
  openKind: vi.fn(),
  openLocation: vi.fn(),
  activate: vi.fn(),
};

vi.mock("$lib/stores/workspace-tabs.svelte", () => ({ getWorkspaceTabs: () => tabs }));
vi.mock("$lib/stores/projects.svelte", () => ({
  useProjects: () => ({
    loaded: true,
    ensureLoaded: () => Promise.resolve(),
    showInactiveProjects: false,
    isProjectStarred: () => false,
    projectLists: () => buildProjectLists([group], (groupId) => [report, garden].filter((project) => project.groupId === groupId)),
  }),
}));
vi.mock("$lib/stores/chat.svelte", () => ({
  getChat: () => ({
    loaded: true,
    prewarmForProject: () => Promise.resolve(),
    channelsForProject: (projectId: string) => channels.filter((channel) => channel.projectId === projectId),
  }),
}));
vi.mock("$lib/stores/chat/sidebar-order.svelte", () => ({
  getChatSidebarOrder: () => ({
    sortChannels: (_projectId: string | null, list: readonly unknown[]) => [...list],
    isChannelStarred: () => false,
  }),
}));
vi.mock("$lib/stores/notes.svelte", () => ({
  getNotes: () => ({
    loaded: true,
    ensureLoaded: () => Promise.resolve(),
    allPages: [],
    folders: [],
    sidebarPageIdsWithChildren: [],
    favoriteFolderIds: [],
    favoritePageIds: [],
    sidebarOrderForProject: () => "name",
    customOrderForProject: () => [],
    loadNavigationChildren: () => Promise.resolve(),
  }),
}));

function press(target: Element, key: string): void {
  const code = /^[0-9]$/.test(key) ? `Digit${key}` : key;
  target.dispatchEvent(new KeyboardEvent("keydown", { key, code, bubbles: true, cancelable: true }));
}

function surface(): HTMLElement {
  const element = document.body.querySelector<HTMLElement>("[data-app-floating-surface]");
  if (!element) throw new Error("Expected the tab launcher");
  return element;
}

function search(): HTMLInputElement {
  const input = surface().querySelector<HTMLInputElement>("input[type=search]");
  if (!input) throw new Error("Expected the launcher search");
  return input;
}

describe("Tab launcher", () => {
  let component: ReturnType<typeof mount> | null = null;
  let onClose = vi.fn();

  beforeEach(async () => {
    vi.clearAllMocks();
    onClose = vi.fn();
    const { default: TitleBarTabLauncher } = await import("./TitleBarTabLauncher.svelte");
    const anchor = document.createElement("button");
    document.body.append(anchor);
    component = mount(TitleBarTabLauncher, { target: document.body, props: { anchor, showOpenTabs: false, onClose } });
    await tick();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    component = null;
    document.body.innerHTML = "";
  });

  it("keeps the digit then Enter fast path to continue where the kind was left", async () => {
    press(surface(), "4");
    await tick();
    expect(surface().textContent).toContain("Continue where you left off");
    await vi.waitFor(() => expect(document.activeElement).toBe(search()));
    press(search(), "Enter");
    expect(onClose).toHaveBeenCalled();
    expect(tabs.openKind).toHaveBeenCalledWith("chat");
  });

  it("drills through a group and a project to open a Chat channel by number", async () => {
    press(surface(), "4");
    await tick();
    expect(surface().textContent).toContain("Work");
    expect(surface().textContent).toContain("Garden");
    press(search(), "1");
    await tick();
    expect(surface().textContent).toContain("Chat / Work");
    press(search(), "1");
    await tick();
    expect(surface().textContent).toContain("Open project");
    press(search(), "2");
    expect(tabs.openLocation).toHaveBeenCalledWith("item", {
      kind: "chat",
      projectId: "report",
      item: { type: "chat-channel", id: "design" },
    });
  });

  it("types digits into a search, highlights the first result, and clears before going back", async () => {
    press(surface(), "2");
    await tick();
    const input = search();
    input.value = "rep";
    input.dispatchEvent(new InputEvent("input", { bubbles: true }));
    await tick();
    expect(surface().textContent).not.toContain("Garden");
    expect(surface().textContent).not.toContain("(press 1)");
    expect(surface().querySelector("[data-highlighted]")?.textContent).toContain("Report");
    press(input, "1");
    expect(tabs.openLocation).not.toHaveBeenCalled();
    press(input, "Escape");
    await tick();
    expect(input.value).toBe("");
    press(input, "Escape");
    await tick();
    expect(surface().querySelector("input[type=search]")).toBeNull();
    press(surface(), "Escape");
    expect(onClose).toHaveBeenCalled();
  });

  it("opens a project from the Projects kind instead of drilling", async () => {
    press(surface(), "2");
    await tick();
    press(search(), "2");
    expect(tabs.openLocation).toHaveBeenCalledWith("project", { kind: "projects", projectId: "garden", item: null });
  });
});
