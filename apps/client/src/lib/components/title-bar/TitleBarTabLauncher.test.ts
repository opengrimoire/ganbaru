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
const freshGroup: ProjectGroup = { ...group, id: "fresh", name: "Fresh", orderKey: "a1" };
const report: TestProject = { id: "report", name: "Report", icon: "lucide:folder", status: "active", groupId: "work" };
const garden: TestProject = { id: "garden", name: "Garden", icon: "lucide:folder", status: "active", groupId: null };
const channels = [
  { id: "general", projectId: "report", name: "general", topic: "" },
  { id: "design", projectId: "report", name: "design", topic: "" },
];

const tabs = {
  tabs: [],
  activeTabId: "tab-1",
  openKind: vi.fn(),
  openLocation: vi.fn(),
  activate: vi.fn(),
  paneScope: vi.fn(() => "scope-1"),
  whenRestored: vi.fn(() => Promise.resolve()),
};

const projects = {
  loaded: true,
  projects: [report, garden],
  ensureLoaded: () => Promise.resolve(),
  showInactiveProjects: false,
  isProjectStarred: () => false,
  projectLists: () => buildProjectLists([group, freshGroup], (groupId) => [report, garden].filter((project) => project.groupId === groupId)),
  addGroup: vi.fn((_name: string, _icon?: string) => Promise.resolve<string | undefined>("fresh")),
  addProjectWithoutSelecting: vi.fn((_groupId: string | null, _name: string, _icon?: string) => Promise.resolve<string | undefined>("garden")),
};

const notes = {
  loaded: true,
  ensureLoaded: () => Promise.resolve(),
  allPages: [],
  folders: [],
  sidebarPageIdsWithChildren: [],
  favoriteFolderIds: [],
  favoritePageIds: [],
  showArchivedInPickers: false,
  pickerArchivedPages: [],
  pickerArchiveError: null,
  ensurePickerArchivedPages: vi.fn(),
  reloadPickerArchivedPages: vi.fn(),
  sidebarOrderForProject: () => "name-asc",
  sidebarOrderOverride: () => null,
  setProjectSidebarOrder: vi.fn(),
  customOrderForProject: () => [],
  loadNavigationChildren: () => Promise.resolve(),
  createFolder: vi.fn(),
  createPageInBackground: vi.fn((_title: string, _parentPageId: string | null, _options?: object) => Promise.resolve("new-page")),
  unarchivePage: vi.fn(() => Promise.resolve()),
};

const paneEvents = new EventTarget();

vi.mock("$lib/stores/workspace-tabs.svelte", () => ({ getWorkspaceTabs: () => tabs }));
vi.mock("$lib/stores/projects.svelte", () => ({ useProjects: () => projects }));
vi.mock("$lib/stores/chat.svelte", () => ({
  getChat: () => ({
    loaded: true,
    prewarmForProject: () => Promise.resolve(),
    channelsForProject: (projectId: string) => channels.filter((channel) => channel.projectId === projectId),
    showArchivedChannelsInPickers: false,
    archivedChannelsForProject: () => [],
    loadArchivedChannelsForPicker: () => Promise.resolve(),
  }),
}));
vi.mock("$lib/stores/chat/sidebar-order.svelte", () => ({
  getChatSidebarOrder: () => ({
    sortChannels: (_projectId: string | null, list: readonly unknown[]) => [...list],
    isChannelStarred: () => false,
    orderForProject: () => "name-asc",
    orderOverride: () => null,
    setProjectOrder: vi.fn(),
  }),
}));
vi.mock("$lib/stores/notes.svelte", () => ({ getNotes: () => notes }));
vi.mock("$lib/stores/preferences.svelte", () => ({
  getPreferences: () => ({ projectsSidebarOrder: "name-asc", setProjectsSidebarOrder: vi.fn() }),
}));
vi.mock("$lib/chat/pane-events", () => ({ chatPaneEvents: () => paneEvents }));

function press(target: Element, key: string, init: KeyboardEventInit = {}): void {
  const code = /^[0-9]$/.test(key) ? `Digit${key}` : key.length === 1 ? `Key${key.toUpperCase()}` : key;
  target.dispatchEvent(new KeyboardEvent("keydown", { key, code, bubbles: true, cancelable: true, ...init }));
}

function button(label: string): HTMLButtonElement {
  const match = [...surface().querySelectorAll<HTMLButtonElement>("button")].find((candidate) => candidate.textContent?.trim() === label);
  if (!match) throw new Error(`Expected a ${label} button`);
  return match;
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

  it("lists a kind's projects with the first one highlighted for Enter", async () => {
    press(surface(), "4");
    await tick();
    await vi.waitFor(() => expect(document.activeElement).toBe(search()));
    press(search(), "Enter");
    await tick();
    expect(surface().textContent).toContain("Chat / Work");
    expect(tabs.openKind).not.toHaveBeenCalled();
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
    press(search(), "3");
    expect(tabs.openLocation).toHaveBeenCalledWith("project", { kind: "projects", projectId: "garden", item: null });
  });

  async function openEmptyNotesProject(): Promise<void> {
    press(surface(), "3");
    await tick();
    press(search(), "1");
    await tick();
    press(search(), "1");
    await tick();
    expect(surface().textContent).toContain("Notes / Work / Report");
  }

  it("creates a note from an empty Notes project and opens it in a new tab", async () => {
    await openEmptyNotesProject();
    expect(surface().textContent).toContain("New folder");
    button("New note").click();
    await vi.waitFor(() => expect(tabs.openLocation).toHaveBeenCalled());
    expect(notes.createPageInBackground).toHaveBeenCalledWith("", null, { projectId: "report", folderId: null });
    expect(tabs.openLocation).toHaveBeenCalledWith("item", { kind: "notes", projectId: "report", item: { type: "notes-page", id: "new-page" } });
    expect(onClose).toHaveBeenCalled();
  });

  it("creates a note with the new note shortcut on a pages level only", async () => {
    press(surface(), "2");
    await tick();
    press(search(), "n", { ctrlKey: true });
    await tick();
    expect(notes.createPageInBackground).not.toHaveBeenCalled();
    press(search(), "Escape");
    await tick();
    await openEmptyNotesProject();
    press(search(), "n", { ctrlKey: true });
    await vi.waitFor(() => expect(tabs.openLocation).toHaveBeenCalled());
    expect(notes.createPageInBackground).toHaveBeenCalledTimes(1);
  });

  it("highlights a new group in the list", async () => {
    press(surface(), "2");
    await tick();
    button("Group").click();
    await tick();
    const input = surface().querySelector<HTMLInputElement>("input:not([type=search])");
    if (!input) throw new Error("Expected the group name field");
    input.value = "Fresh";
    input.dispatchEvent(new InputEvent("input", { bubbles: true }));
    input.form?.requestSubmit();
    await vi.waitFor(() => expect(surface().querySelector("[data-highlighted]")?.textContent).toContain("Fresh"));
    expect(projects.addGroup).toHaveBeenCalledWith("Fresh", expect.any(String));
    expect(surface().querySelector("input:not([type=search])")).toBeNull();
  });

  it("cancels an open create form on Escape before going back a level", async () => {
    press(surface(), "2");
    await tick();
    button("Group").click();
    await tick();
    const input = surface().querySelector<HTMLInputElement>("input:not([type=search])");
    if (!input) throw new Error("Expected the group name field");
    press(input, "Escape");
    await tick();
    expect(surface().querySelector("input:not([type=search])")).toBeNull();
    expect(surface().textContent).toContain("Projects");
    press(search(), "Escape");
    await tick();
    expect(surface().querySelector("input[type=search]")).toBeNull();
  });

  it("opens a Chat tab at the project and asks it for a new channel", async () => {
    const requested = vi.fn();
    paneEvents.addEventListener("ganbaru:chat-new-channel", requested);
    press(surface(), "4");
    await tick();
    press(search(), "1");
    await tick();
    press(search(), "1");
    await tick();
    button("New channel").click();
    await vi.waitFor(() => expect(requested).toHaveBeenCalled());
    expect(onClose).toHaveBeenCalled();
    expect(tabs.openLocation).toHaveBeenCalledWith("project", { kind: "chat", projectId: "report", item: null });
    paneEvents.removeEventListener("ganbaru:chat-new-channel", requested);
  });
});
