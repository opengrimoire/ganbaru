// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectGroup } from "$lib/projects/types";
import ProjectsGroupList from "./ProjectsGroupList.svelte";

const store = vi.hoisted(() => ({
  groups: [] as ProjectGroup[],
  projectCounts: new Map<string, number>(),
  updateGroup: vi.fn((): Promise<void> => Promise.resolve()),
  removeGroup: vi.fn((): Promise<void> => Promise.resolve()),
}));

vi.mock("$lib/stores/projects.svelte", () => ({
  getProjects: () => ({
    loaded: true,
    loading: false,
    ensureLoaded: () => Promise.resolve(),
    visibleGroups: () => store.groups,
    projectsForGroupIncludingInactive: (groupId: string) =>
      Array.from({ length: store.projectCounts.get(groupId) ?? 0 }, (_, index) => ({ id: `${groupId}-${index}` })),
    updateGroup: store.updateGroup,
    removeGroup: store.removeGroup,
  }),
}));

function group(id: string, name: string): ProjectGroup {
  return {
    id,
    name,
    icon: "lucide:folder",
    orderKey: "0",
    collapsed: false,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

function render(): void {
  component = mount(ProjectsGroupList, { target });
  flushSync();
}

function buttonLabeled(label: string): HTMLButtonElement | undefined {
  return [...document.body.querySelectorAll<HTMLButtonElement>("button")]
    .find((button) => button.getAttribute("aria-label") === label || button.textContent?.trim().startsWith(label));
}

function nameInput(): HTMLInputElement {
  const input = target.querySelector<HTMLInputElement>("input");
  if (!input) throw new Error("No group name input");
  return input;
}

function typeName(value: string): void {
  const input = nameInput();
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

function press(key: string): void {
  nameInput().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  flushSync();
}

describe("Settings list of project groups", () => {
  const work = group("group-work", "Work");
  const routine = group("group-routine", "Routine");

  beforeEach(() => {
    target = document.createElement("div");
    document.body.append(target);
    store.groups = [routine, work];
    store.projectCounts = new Map([["group-routine", 12], ["group-work", 1]]);
    store.updateGroup.mockClear();
    store.removeGroup.mockClear();
  });

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    document.body.replaceChildren();
  });

  it("lists groups with their project counts and offers no rename or delete for the built-in Routine group", () => {
    render();

    const rows = [...target.querySelectorAll("li")];
    expect(rows[0]?.textContent).toContain("Built-in group");
    expect(rows[0]?.querySelector("button")).toBeNull();
    expect(rows[1]?.textContent).toContain("Work");
    expect(rows[1]?.textContent).toContain("1 project");
    expect(buttonLabeled("Rename Work")).toBeDefined();
    expect(buttonLabeled("Delete group Work")).toBeDefined();
  });

  it("renames a group with Enter and keeps the name when Escape cancels or the draft is blank", async () => {
    render();

    buttonLabeled("Rename Work")?.click();
    flushSync();
    typeName("Other");
    press("Escape");
    expect(target.querySelector("input")).toBeNull();

    buttonLabeled("Rename Work")?.click();
    flushSync();
    typeName("   ");
    press("Enter");
    expect(store.updateGroup).not.toHaveBeenCalled();

    buttonLabeled("Rename Work")?.click();
    flushSync();
    typeName("  Office  ");
    press("Enter");
    await Promise.resolve();
    expect(store.updateGroup).toHaveBeenCalledExactlyOnceWith(work, { name: "Office" });
  });

  it("deletes a group only after a confirmation that names how many projects leave it", async () => {
    store.projectCounts.set("group-work", 3);
    render();

    buttonLabeled("Delete group Work")?.click();
    flushSync();
    expect(store.removeGroup).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("keeps its 3 projects");

    buttonLabeled("Cancel")?.click();
    flushSync();
    expect(document.body.textContent).not.toContain("keeps its 3 projects");

    buttonLabeled("Delete group Work")?.click();
    flushSync();
    buttonLabeled("Delete group (Enter)")?.click();
    await Promise.resolve();
    expect(store.removeGroup).toHaveBeenCalledExactlyOnceWith(work);
  });

  it("reports a failed delete", async () => {
    store.removeGroup.mockRejectedValueOnce(new Error("locked"));
    render();

    buttonLabeled("Delete group Work")?.click();
    flushSync();
    buttonLabeled("Delete group (Enter)")?.click();
    await vi.waitFor(() => expect(target.textContent).toContain("Could not delete group."));
  });

  it("keeps a group busy until its own delete settles when another group's rename finishes first", async () => {
    const home = group("group-home", "Home");
    store.groups = [routine, work, home];
    let finishRename = (): void => {};
    store.updateGroup.mockReturnValueOnce(new Promise<void>((resolve) => {
      finishRename = resolve;
    }));
    store.removeGroup.mockReturnValueOnce(new Promise<void>(() => {}));
    render();

    buttonLabeled("Rename Work")?.click();
    flushSync();
    typeName("Office");
    press("Enter");
    buttonLabeled("Delete group Home")?.click();
    flushSync();
    buttonLabeled("Delete group (Enter)")?.click();
    flushSync();
    finishRename();
    await Promise.resolve();
    await Promise.resolve();
    flushSync();

    expect(buttonLabeled("Rename Work")?.disabled).toBe(false);
    expect(buttonLabeled("Delete group Home")?.disabled).toBe(true);
    expect(buttonLabeled("Rename Home")?.disabled).toBe(true);
  });

  it("says when there are no groups", () => {
    store.groups = [];
    render();

    expect(target.textContent).toContain("No groups");
  });
});
