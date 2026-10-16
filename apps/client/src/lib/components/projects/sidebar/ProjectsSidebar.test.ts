// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Project, ProjectGroup } from "$lib/projects/types";
import type { SidebarSortOrder } from "$lib/utils/sidebar-order";
import ProjectsSidebar from "./ProjectsSidebar.svelte";

const store = vi.hoisted(() => ({
  order: "custom" as SidebarSortOrder,
  projects: [] as Project[],
  starredProjectIds: [] as string[],
  reorderProjectsInGroup: vi.fn(() => Promise.resolve()),
}));

const group: ProjectGroup = {
  id: "group-a",
  name: "Work",
  icon: "lucide:folder",
  sortOrder: 0,
  collapsed: false,
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

vi.mock("$lib/stores/preferences.svelte", () => ({
  getPreferences: () => ({ get projectsSidebarOrder() { return store.order; } }),
}));

vi.mock("$lib/stores/projects.svelte", () => {
  const getProjects = () => ({
    sidebarCollapsed: false,
    showInactiveProjects: false,
    selectedProjectId: "project-a",
    activeViewId: null,
    views: [],
    hostableViewTypes: [],
    selectedGroup: group,
    visibleGroups: () => [group],
    projectsForGroup: () => store.projects,
    projectsForGroupIncludingInactive: () => store.projects,
    viewsForProject: () => [],
    isSidebarProjectCollapsed: () => true,
    setSidebarProjectCollapsed: vi.fn(),
    isProjectStarred: (projectId: string) => store.starredProjectIds.includes(projectId),
    isViewStarred: () => false,
    reorderProjectsInGroup: store.reorderProjectsInGroup,
  });
  return { getProjects, useProjects: getProjects };
});

function project(id: string, name: string, sortOrder: number): Project {
  return {
    id,
    groupId: group.id,
    name,
    icon: "lucide:folder",
    color: 8,
    sortOrder,
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
}

const SECTION_HEIGHT = 30;
let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

function render(panelProps: { presentation: "panel"; onClose: () => void } | null = null): void {
  component = mount(ProjectsSidebar, {
    target,
    props: { onSelectView: () => undefined, onProjectSelected: () => undefined, ...panelProps },
  });
  flushSync();
}

function sectionItems(): HTMLElement[] {
  return [...target.querySelectorAll<HTMLElement>("[data-project-list] > [data-sortable-item]")];
}

function headingToggle(name: string): HTMLButtonElement {
  const section = target.querySelector<HTMLElement>(`section[aria-label="${name}"]`);
  const toggle = section?.querySelector<HTMLButtonElement>("button.section-toggle");
  if (!toggle) throw new Error(`No heading for ${name}`);
  return toggle;
}

/** Lays the project sections out in a column of equal sections starting at the top of the list. */
function stubSectionLayout(): void {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const index = sectionItems().indexOf(this);
    const top = index < 0 ? 0 : index * SECTION_HEIGHT;
    return DOMRect.fromRect({ x: 0, y: top, width: 200, height: index < 0 ? 0 : SECTION_HEIGHT });
  });
}

function pointer(element: EventTarget, type: string, clientY: number): void {
  element.dispatchEvent(new PointerEvent(type, { bubbles: true, button: 0, pointerId: 1, pointerType: "mouse", clientX: 20, clientY }));
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("reduce"), media: query, addEventListener: () => undefined, removeEventListener: () => undefined }));
  store.order = "custom";
  store.starredProjectIds = [];
  store.projects = [project("project-a", "Alpha", 0), project("project-b", "Beta", 1), project("project-c", "Gamma", 2)];
  store.reorderProjectsInGroup.mockClear();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  target.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  // A drop arms a one-task click suppression; running it keeps it from swallowing the next test's clicks.
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
});

describe("ProjectsSidebar", () => {
  it("drags a project by its name into a new place in the custom order", async () => {
    render();
    stubSectionLayout();

    pointer(headingToggle("Alpha"), "pointerdown", 10);
    pointer(window, "pointermove", 75);
    flushSync();
    pointer(window, "pointerup", 75);
    flushSync();
    await Promise.resolve();

    expect(store.reorderProjectsInGroup).toHaveBeenCalledWith("group-a", ["project-b", "project-c", "project-a"]);
  });

  it("keeps project headings in place outside the custom order", async () => {
    store.order = "name-asc";
    render();
    stubSectionLayout();

    pointer(headingToggle("Alpha"), "pointerdown", 10);
    pointer(window, "pointermove", 75);
    pointer(window, "pointerup", 75);
    flushSync();
    await Promise.resolve();

    expect(store.reorderProjectsInGroup).not.toHaveBeenCalled();
  });

  it("opens as a picker panel that closes from its header on the phone", () => {
    const onClose = vi.fn();
    render({ presentation: "panel", onClose });

    const panel = document.querySelector<HTMLElement>("[data-mobile-sidebar-panel]");
    const sidebar = panel?.querySelector<HTMLElement>("[data-projects-sidebar]");
    expect(panel?.closest('[role="dialog"]')?.getAttribute("aria-label")).toBe("Projects");
    expect(sidebar?.classList.contains("explorer-panel")).toBe(true);
    expect(sidebar?.classList.contains("explorer-dock")).toBe(false);
    expect(document.querySelector('button[aria-label="Expand sidebar"]')).toBeNull();
    expect(document.querySelector('button[aria-label="Collapse sidebar"]')).toBeNull();
    expect(panel?.querySelectorAll("[data-project-list] > [data-sortable-item]")).toHaveLength(3);

    panel?.querySelector<HTMLButtonElement>("[data-mobile-sidebar-panel-close]")?.click();
    flushSync();

    expect(onClose).toHaveBeenCalledOnce();
  });

  it("filters the panel list from the panel search field", () => {
    render({ presentation: "panel", onClose: vi.fn() });

    const panel = document.querySelector<HTMLElement>("[data-mobile-sidebar-panel]");
    const input = panel?.querySelector<HTMLInputElement>('input[aria-label="Search projects"]');
    if (!input) throw new Error("Panel search is missing");
    input.value = "gam";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    const names = [...(panel?.querySelectorAll<HTMLElement>("[data-project-list] section[aria-label]") ?? [])]
      .map((section) => section.getAttribute("aria-label"));
    expect(names).toEqual(["Gamma"]);
  });
});
