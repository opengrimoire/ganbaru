// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Project, ProjectGroup, ProjectLifecycleStatus } from "$lib/projects/types";
import ProjectsInactiveList from "./ProjectsInactiveList.svelte";

const store = vi.hoisted(() => ({
  projects: [] as Project[],
  setProjectStatus: vi.fn((): Promise<void> => Promise.resolve()),
}));

function group(id: string, name: string): ProjectGroup {
  return {
    id,
    name,
    icon: "lucide:folder",
    sortOrder: 0,
    collapsed: false,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

const groups = [group("group-a", "Work"), group("group-b", "Home")];

vi.mock("$lib/stores/projects.svelte", () => ({
  getProjects: () => ({
    loaded: true,
    loading: false,
    loadError: null,
    customEmojis: [],
    ensureLoaded: () => Promise.resolve(),
    visibleGroups: () => groups,
    projectsForGroupIncludingInactive: (groupId: string) => store.projects.filter((entry) => entry.groupId === groupId),
    setProjectStatus: store.setProjectStatus,
  }),
}));

function project(id: string, groupId: string, status: ProjectLifecycleStatus): Project {
  return {
    id,
    groupId,
    name: id,
    icon: "lucide:folder",
    color: 8,
    sortOrder: 0,
    status,
    defaultEventName: null,
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: 60,
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: "adaptive",
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: true,
    defaultIdleThresholdMinutes: 5,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

function render(): void {
  component = mount(ProjectsInactiveList, { target });
  flushSync();
}

function rowLabels(): string[] {
  return [...target.querySelectorAll("li")].map((row) => row.querySelector("button")?.getAttribute("aria-label") ?? "");
}

describe("Settings list of hidden and archived projects", () => {
  beforeEach(() => {
    target = document.createElement("div");
    document.body.append(target);
    store.setProjectStatus.mockClear();
  });

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    target.remove();
  });

  it("lists only inactive projects in group order with the action that fits each status", () => {
    store.projects = [
      project("Errands", "group-b", "archived"),
      project("Launch", "group-a", "active"),
      project("Draft", "group-a", "hidden"),
    ];
    render();

    expect(rowLabels()).toEqual(["Show Draft", "Restore Errands"]);
    expect(target.querySelector("li [role='img']")?.getAttribute("aria-label")).toBe("Hidden");
    expect([...target.querySelectorAll("li button")].map((button) => button.textContent?.trim())).toEqual(["Show", "Restore"]);
  });

  it("makes a project active again from its row", async () => {
    store.projects = [project("Draft", "group-a", "hidden")];
    render();

    target.querySelector<HTMLButtonElement>("li button")?.click();
    await Promise.resolve();

    expect(store.setProjectStatus).toHaveBeenCalledExactlyOnceWith(store.projects[0], "active");
  });

  it("reports a failed update with the project name", async () => {
    store.projects = [project("Draft", "group-a", "archived")];
    store.setProjectStatus.mockRejectedValueOnce(new Error("locked"));
    render();

    target.querySelector<HTMLButtonElement>("li button")?.click();
    await vi.waitFor(() => expect(target.textContent).toContain("Could not update Draft: locked"));
  });

  it("says when nothing is hidden or archived", () => {
    store.projects = [project("Launch", "group-a", "active")];
    render();

    expect(target.querySelector("ul")).toBeNull();
    expect(target.textContent).toContain("No hidden or archived projects");
  });
});
