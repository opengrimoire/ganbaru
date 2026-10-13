// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectSidebarActions } from "$lib/projects/sidebar";
import type { Project, ProjectHostedViewType, ProjectView, ProjectViewType } from "$lib/projects/types";
import { projectViewsFor } from "$lib/projects/views";
import { starredFirst } from "$lib/utils/sidebar-order";
import ProjectSidebarSection from "./ProjectSidebarSection.svelte";

const store = vi.hoisted(() => ({
  views: [] as ProjectView[],
  hostableViewTypes: [] as ProjectHostedViewType[],
  addView: vi.fn(),
  renameView: vi.fn(),
  reorderViews: vi.fn(),
  removeView: vi.fn(),
  starredProjectIds: [] as string[],
  starredViewIds: [] as string[],
  setProjectStarred: vi.fn(),
  setViewStarred: vi.fn(),
}));

vi.mock("$lib/stores/projects.svelte", () => ({
  getProjects: () => ({
    get views() { return store.views; },
    get hostableViewTypes() { return store.hostableViewTypes; },
    viewsForProject: (projectId: string) => sidebarViews(projectId),
    addView: store.addView,
    renameView: store.renameView,
    reorderViews: store.reorderViews,
    removeView: store.removeView,
    isProjectStarred: (projectId: string) => store.starredProjectIds.includes(projectId),
    isViewStarred: (viewId: string) => store.starredViewIds.includes(viewId),
    setProjectStarred: store.setProjectStarred,
    setViewStarred: store.setViewStarred,
  }),
}));

const project: Project = {
  id: "project-a",
  groupId: "group-a",
  name: "Launch",
  icon: "lucide:folder",
  color: 8,
  sortOrder: 0,
  status: "active",
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

const actions: ProjectSidebarActions = {
  identityLocked: false,
  canRename: true,
  canMoveToGroup: false,
  moveGroupIds: [],
  visibility: { label: "hide", status: "hidden" },
  archive: { label: "archive", status: "archived" },
};

/** Mirrors the store: the project's hostable views in saved order, starred views first. */
function sidebarViews(projectId: string): ProjectView[] {
  return starredFirst(projectViewsFor(store.views, projectId, store.hostableViewTypes), (entry) => store.starredViewIds.includes(entry.id));
}

function view(id: string, viewType: ProjectViewType, sortOrder: number, name?: string): ProjectView {
  return { id, projectId: project.id, viewType, sortOrder, name, createdAt: "", updatedAt: "" };
}

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

function render(overrides: { collapsed?: boolean; onSelectView?: (view: ProjectView) => void; onCollapsedChange?: (value: boolean) => void } = {}) {
  component = mount(ProjectSidebarSection, {
    target,
    props: {
      project,
      selected: true,
      actions,
      moveGroups: [],
      views: sidebarViews(project.id),
      activeViewId: "overview",
      collapsed: overrides.collapsed ?? false,
      onCollapsedChange: overrides.onCollapsedChange ?? (() => undefined),
      onSelectView: overrides.onSelectView ?? (() => undefined),
      onRename: () => Promise.resolve(),
      onMoveToGroup: () => Promise.resolve(),
      onSetStatus: () => Promise.resolve(),
    },
  });
  flushSync();
}

function button(label: string, root: ParentNode = document.body): HTMLButtonElement {
  const match = [...root.querySelectorAll<HTMLButtonElement>("button")]
    .find((candidate) => candidate.getAttribute("aria-label") === label || candidate.textContent?.trim() === label);
  if (!match) throw new Error(`No button labeled ${label}`);
  return match;
}

function viewRowLabels(): string[] {
  return [...target.querySelectorAll<HTMLElement>("[data-project-view-id]")].map((row) => row.getAttribute("aria-label") ?? "");
}

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
  store.views = [view("overview", "dashboard", 1000), view("list", "list", 2000, "Backlog"), view("notes", "notes", 3000)];
  store.hostableViewTypes = ["notes", "chat"];
  store.addView.mockReset();
  store.renameView.mockReset().mockResolvedValue(undefined);
  store.reorderViews.mockReset().mockResolvedValue(undefined);
  store.removeView.mockReset().mockResolvedValue(undefined);
  store.starredProjectIds = [];
  store.starredViewIds = [];
  store.setProjectStarred.mockReset();
  store.setViewStarred.mockReset();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

describe("ProjectSidebarSection", () => {
  it("lists the project's views under its heading with their names or type labels", () => {
    render();
    expect(viewRowLabels()).toEqual(["Overview", "Backlog", "Notes"]);
    expect(target.querySelector('[aria-current="page"]')?.textContent?.trim()).toBe("Overview");
  });

  it("hides the views while collapsed and expands from the heading", () => {
    const onCollapsedChange = vi.fn();
    render({ collapsed: true, onCollapsedChange });
    expect(viewRowLabels()).toEqual([]);
    button("Expand Launch").click();
    expect(onCollapsedChange).toHaveBeenCalledWith(false);
  });

  it("marks hosted views the project already has as unavailable and opens a newly added view", async () => {
    const onSelectView = vi.fn();
    render({ onSelectView });
    button("Add a view to Launch").click();
    flushSync();
    const menu = document.body.querySelector<HTMLElement>('[role="menu"]');
    expect(menu?.querySelector('[data-project-add-view="notes"]')?.getAttribute("aria-disabled")).toBe("true");
    expect(menu?.querySelector('[data-project-add-view="chat"]')?.getAttribute("aria-disabled")).toBeNull();

    (menu?.querySelector('[data-project-add-view="notes"]') as HTMLButtonElement).click();
    expect(store.addView).not.toHaveBeenCalled();

    store.addView.mockImplementation((projectId: string, viewType: ProjectViewType) => {
      store.views = [...store.views, { ...view("added", viewType, 4000), projectId }];
      return Promise.resolve("added");
    });
    (menu?.querySelector('[data-project-add-view="kanban"]') as HTMLButtonElement).click();
    await vi.waitFor(() => expect(onSelectView).toHaveBeenCalled());
    expect(store.addView).toHaveBeenCalledWith("project-a", "kanban");
    expect(onSelectView.mock.calls[0][0]).toMatchObject({ id: "added", viewType: "kanban" });
  });

  it("removes a view only after the confirmation", async () => {
    render();
    button("View actions: Backlog").click();
    flushSync();
    button("Remove view").click();
    flushSync();
    expect(store.removeView).not.toHaveBeenCalled();
    const confirm = [...document.body.querySelectorAll<HTMLButtonElement>("button")]
      .find((candidate) => candidate.textContent?.includes("Remove view (Enter)"));
    confirm?.click();
    await tick();
    expect(store.removeView).toHaveBeenCalledWith("list");
  });

  it("moves a view down with every view of the project in the new order", () => {
    render();
    button("View actions: Overview").click();
    flushSync();
    button("Move down").click();
    expect(store.reorderViews).toHaveBeenCalledWith("project-a", ["list", "overview", "notes"]);
  });

  it("lists starred views first and moves a view only among views with the same starred state", () => {
    store.starredViewIds = ["notes"];
    render();
    expect(viewRowLabels()).toEqual(["Notes", "Overview", "Backlog"]);

    button("View actions: Notes").click();
    flushSync();
    expect(button("Move down").getAttribute("aria-disabled")).toBe("true");
    button("Move down").click();
    expect(store.reorderViews).not.toHaveBeenCalled();
    button("View actions: Notes").click();
    flushSync();

    button("View actions: Backlog").click();
    flushSync();
    expect(button("Move up").getAttribute("aria-disabled")).toBeNull();
    button("Move up").click();
    expect(store.reorderViews).toHaveBeenCalledWith("project-a", ["list", "overview", "notes"]);
  });

  it("stars a view and the project from their menus", () => {
    store.starredProjectIds = ["project-a"];
    render();
    expect(target.querySelector(".section-heading .section-star")).not.toBeNull();
    expect(target.querySelector(".explorer-row-star")).toBeNull();

    button("View actions: Backlog").click();
    flushSync();
    button("Add to favorites").click();
    expect(store.setViewStarred).toHaveBeenCalledWith("list", true);

    button("Project actions: Launch").click();
    flushSync();
    button("Remove from favorites").click();
    expect(store.setProjectStarred).toHaveBeenCalledWith("project-a", false);
  });

  it("keeps an unnamed view on its type label when a rename repeats it, and saves a new name", async () => {
    render();
    button("View actions: Overview").click();
    flushSync();
    button("Rename view").click();
    await tick();
    const input = target.querySelector<HTMLInputElement>('input[aria-label="View name"]');
    expect(input?.value).toBe("Overview");
    input?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    flushSync();
    expect(store.renameView).not.toHaveBeenCalled();

    button("View actions: Overview").click();
    flushSync();
    button("Rename view").click();
    await tick();
    const renamed = target.querySelector<HTMLInputElement>('input[aria-label="View name"]');
    if (!renamed) throw new Error("Rename input missing");
    renamed.value = "Status";
    renamed.dispatchEvent(new Event("input", { bubbles: true }));
    renamed.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    flushSync();
    expect(store.renameView).toHaveBeenCalledWith("overview", "Status");
  });
});
