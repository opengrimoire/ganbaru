// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectHostedViewType, ProjectView, ProjectViewType } from "$lib/projects/types";
import { projectViewsFor } from "$lib/projects/views";
import ProjectMobileViewSwitcher from "./ProjectMobileViewSwitcher.svelte";

const store = vi.hoisted(() => ({
  views: [] as ProjectView[],
  hostableViewTypes: [] as ProjectHostedViewType[],
  activeViewId: null as string | null,
  addView: vi.fn(),
}));

vi.mock("$lib/stores/projects.svelte", () => {
  const getProjects = () => ({
    get views() { return store.views; },
    get hostableViewTypes() { return store.hostableViewTypes; },
    get activeViewId() { return store.activeViewId; },
    viewsForProject: (projectId: string) => projectViewsFor(store.views, projectId, store.hostableViewTypes),
    addView: store.addView,
  });
  return { getProjects, useProjects: getProjects };
});

const PROJECT_ID = "project-a";

function view(id: string, viewType: ProjectViewType, orderKey: string, name?: string): ProjectView {
  return { id, projectId: PROJECT_ID, viewType, orderKey, name, createdAt: "", updatedAt: "" };
}

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

function render(onSelectView: (view: ProjectView) => void = () => undefined): void {
  component = mount(ProjectMobileViewSwitcher, { target, props: { projectId: PROJECT_ID, onSelectView } });
  flushSync();
}

function switcher(): HTMLButtonElement {
  const match = target.querySelector<HTMLButtonElement>("[data-project-mobile-view-switcher]");
  if (!match) throw new Error("No view switcher");
  return match;
}

function addViewRow(): HTMLButtonElement {
  const match = document.body.querySelector<HTMLButtonElement>("[data-project-mobile-add-view]");
  if (!match) throw new Error("No add view row");
  return match;
}

function openMenu(): HTMLElement | null {
  return document.body.querySelector<HTMLElement>('[role="menu"]');
}

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
  store.views = [view("overview", "dashboard", "1000"), view("list", "list", "2000", "Backlog"), view("notes", "notes", "3000")];
  store.hostableViewTypes = ["notes", "chat"];
  store.activeViewId = "list";
  store.addView.mockReset();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

describe("ProjectMobileViewSwitcher", () => {
  it("names the open view and lists every view with the open one checked", () => {
    render();
    expect(switcher().textContent?.trim()).toBe("Backlog");

    switcher().click();
    flushSync();
    const items = [...(openMenu()?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]') ?? [])];
    expect(items.map((item) => item.textContent?.trim())).toEqual(["Overview", "Backlog", "Notes"]);
    expect(items.map((item) => item.getAttribute("aria-checked"))).toEqual(["false", "true", "false"]);
  });

  it("opens a chosen view and closes the menu, but leaves the open view alone", () => {
    const onSelectView = vi.fn();
    render(onSelectView);
    switcher().click();
    flushSync();
    const items = [...(openMenu()?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]') ?? [])];
    items[1].click();
    flushSync();
    expect(onSelectView).not.toHaveBeenCalled();
    expect(openMenu()).toBeNull();

    switcher().click();
    flushSync();
    openMenu()?.querySelector<HTMLButtonElement>('[role="menuitemradio"]')?.click();
    flushSync();
    expect(onSelectView).toHaveBeenCalledWith(expect.objectContaining({ id: "overview" }));
    expect(openMenu()).toBeNull();
  });

  it("closes the menu on a second press of its trigger", () => {
    render();
    switcher().click();
    flushSync();
    expect(switcher().getAttribute("aria-expanded")).toBe("true");
    switcher().click();
    flushSync();
    expect(openMenu()).toBeNull();
    expect(switcher().getAttribute("aria-expanded")).toBe("false");
  });

  it("expands the view types from the Add view row at the end of the same menu", () => {
    render();
    switcher().click();
    flushSync();
    const menu = openMenu();
    expect(menu?.lastElementChild?.lastElementChild).toBe(addViewRow());
    expect(menu?.querySelector("[data-project-add-view]")).toBeNull();

    addViewRow().click();
    flushSync();
    expect(addViewRow().getAttribute("aria-expanded")).toBe("true");
    expect(menu?.querySelector('[data-project-add-view="list"]')).not.toBeNull();

    addViewRow().click();
    flushSync();
    expect(menu?.querySelector("[data-project-add-view]")).toBeNull();
    expect(openMenu()).toBe(menu);
  });

  it("collapses the view types again when the menu reopens", () => {
    render();
    switcher().click();
    flushSync();
    addViewRow().click();
    flushSync();
    switcher().click();
    flushSync();
    switcher().click();
    flushSync();
    expect(addViewRow().getAttribute("aria-expanded")).toBe("false");
  });

  it("leaves out hosted views the project already has and opens a newly added view", async () => {
    const onSelectView = vi.fn();
    render(onSelectView);
    switcher().click();
    flushSync();
    addViewRow().click();
    flushSync();
    const menu = openMenu();
    expect(menu?.querySelector('[data-project-add-view="notes"]')).toBeNull();
    expect(menu?.querySelector('[data-project-add-view="chat"]')).not.toBeNull();

    store.addView.mockImplementation((projectId: string, viewType: ProjectViewType) => {
      store.views = [...store.views, { ...view("added", viewType, "4000"), projectId }];
      return Promise.resolve("added");
    });
    menu?.querySelector<HTMLButtonElement>('[data-project-add-view="kanban"]')?.click();
    await vi.waitFor(() => expect(onSelectView).toHaveBeenCalled());
    expect(store.addView).toHaveBeenCalledWith(PROJECT_ID, "kanban");
    expect(onSelectView.mock.calls[0][0]).toMatchObject({ id: "added", viewType: "kanban" });
    expect(openMenu()).toBeNull();
  });
});
