import { describe, expect, it } from "vitest";
import type { View } from "$lib/navigation";
import {
  activeWorkspaceTab,
  closeOtherWorkspaceTabs,
  closeWorkspaceTab,
  createWorkspaceTab,
  defaultWorkspaceTabsState,
  detachWorkspaceTab,
  insertWorkspaceTab,
  isWithinTabScope,
  kindLocation,
  MAX_CLOSED_WORKSPACE_TABS,
  MAX_WORKSPACE_TABS,
  moveWorkspaceTab,
  parseWorkspaceTabLocation,
  parseWorkspaceTabsState,
  relativeWorkspaceTabId,
  removeWorkspaceTab,
  reopenWorkspaceTab,
  serializeWorkspaceTabsState,
  tabMatchesTarget,
  updateWorkspaceTab,
  visibleWorkspaceTabs,
  workspaceTabIdAtPosition,
  type WorkspaceTab,
  type WorkspaceTabLocation,
  type WorkspaceTabsState,
} from "./model";

const ALL_KINDS: View[] = ["calendar", "projects", "notes", "chat"];

function idFactory(prefix = "new"): () => string {
  let next = 0;
  return () => `${prefix}-${++next}`;
}

function tab(id: string, kind: View, overrides: Partial<WorkspaceTab> = {}): WorkspaceTab {
  return { ...createWorkspaceTab(id, kindLocation(kind)), ...overrides };
}

function state(tabs: WorkspaceTab[], activeTabId = tabs[0].id): WorkspaceTabsState {
  return { tabs, activeTabId, closed: [] };
}

function ids(value: WorkspaceTabsState): string[] {
  return value.tabs.map((entry) => entry.id);
}

const projectA: WorkspaceTabLocation = { kind: "projects", projectId: "a", item: null };
const projectB: WorkspaceTabLocation = { kind: "projects", projectId: "b", item: null };
const pageOne: WorkspaceTabLocation = {
  kind: "notes",
  projectId: "a",
  item: { type: "notes-page", id: "page-1" },
};

describe("workspace tab defaults", () => {
  it("opens Calendar and Projects on first use", () => {
    const initial = defaultWorkspaceTabsState(ALL_KINDS, idFactory());
    expect(initial.tabs.map((entry) => entry.location.kind)).toEqual(["calendar", "projects"]);
    expect(initial.activeTabId).toBe(initial.tabs[0].id);
  });

  it("falls back to the first available kind when no default kind exists", () => {
    const initial = defaultWorkspaceTabsState(["notes", "chat"], idFactory());
    expect(initial.tabs.map((entry) => entry.location.kind)).toEqual(["notes"]);
  });
});

describe("workspace tab levels", () => {
  it("lowers levels the location cannot support and never locks a kind tab", () => {
    expect(createWorkspaceTab("t", kindLocation("projects"), { level: "project", locked: true }))
      .toMatchObject({ level: "kind", locked: false });
    expect(createWorkspaceTab("t", projectA, { level: "item", locked: true }))
      .toMatchObject({ level: "project", locked: true });
    expect(createWorkspaceTab("t", pageOne, { level: "item" })).toMatchObject({ level: "item" });
  });

  it("keeps unlocked tabs following any location in their kind", () => {
    const projectTab = createWorkspaceTab("t", projectA, { level: "project" });
    expect(isWithinTabScope(projectTab, projectB)).toBe(true);
    expect(isWithinTabScope(projectTab, kindLocation("calendar"))).toBe(false);
  });

  it("confines locked tabs to their project or item", () => {
    const projectTab = createWorkspaceTab("t", projectA, { level: "project", locked: true });
    expect(isWithinTabScope(projectTab, { ...projectA, item: { type: "project-view", id: "v" } })).toBe(true);
    expect(isWithinTabScope(projectTab, projectB)).toBe(false);

    const pageTab = createWorkspaceTab("p", pageOne, { level: "item", locked: true });
    expect(isWithinTabScope(pageTab, { ...pageOne, item: { type: "notes-page", id: "page-2" } })).toBe(false);
    expect(isWithinTabScope(pageTab, pageOne)).toBe(true);
  });

  it("matches open tabs by the level a new tab would use", () => {
    const projectTab = createWorkspaceTab("t", projectA, { level: "project" });
    expect(tabMatchesTarget(projectTab, "project", { ...projectA, item: { type: "project-view", id: "v" } }))
      .toBe(true);
    expect(tabMatchesTarget(projectTab, "project", projectB)).toBe(false);
    expect(tabMatchesTarget(projectTab, "kind", projectA)).toBe(false);
  });
});

describe("opening and updating workspace tabs", () => {
  it("inserts after the active tab and activates the new tab", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects")]);
    const next = insertWorkspaceTab(current, tab("c", "notes"), "after-active");
    expect(ids(next)).toEqual(["a", "c", "b"]);
    expect(next.activeTabId).toBe("c");
    expect(ids(insertWorkspaceTab(current, tab("c", "notes"), "end"))).toEqual(["a", "b", "c"]);
  });

  it("refuses new tabs at the tab limit", () => {
    const full = state(Array.from({ length: MAX_WORKSPACE_TABS }, (_, index) => tab(`t${index}`, "notes")));
    expect(insertWorkspaceTab(full, tab("extra", "chat"), "end")).toBe(full);
  });

  it("returns the same state when an update changes nothing", () => {
    const current = state([tab("a", "projects", { location: projectA, level: "project" })]);
    expect(updateWorkspaceTab(current, "a", { location: { ...projectA } })).toBe(current);
    expect(updateWorkspaceTab(current, "missing", { title: "x" })).toBe(current);
    expect(updateWorkspaceTab(current, "a", { location: projectB }).tabs[0].location).toEqual(projectB);
  });

  it("ignores a lock request on a kind tab", () => {
    const current = state([tab("a", "projects")]);
    expect(updateWorkspaceTab(current, "a", { locked: true })).toBe(current);
  });
});

describe("closing workspace tabs", () => {
  it("activates the right neighbor, else the left one", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes")], "b");
    expect(closeWorkspaceTab(current, "b")?.activeTabId).toBe("c");
    const last = state([tab("a", "calendar"), tab("b", "projects")], "b");
    expect(closeWorkspaceTab(last, "b")?.activeTabId).toBe("a");
    expect(closeWorkspaceTab(current, "a")?.activeTabId).toBe("b");
  });

  it("refuses to close the last visible tab even when detached tabs exist", () => {
    const current = state([tab("a", "calendar"), tab("b", "chat", { detached: true })]);
    expect(closeWorkspaceTab(current, "a")).toBeNull();
  });

  it("removes a detached tab with its final location", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects", { detached: true })]);
    const next = removeWorkspaceTab(current, "b", projectB);
    expect(ids(next)).toEqual(["a"]);
    expect(next.closed[0]).toMatchObject({ location: projectB, index: 1 });
  });

  it("keeps a bounded reopen history with the newest first", () => {
    let current = state(Array.from({ length: MAX_CLOSED_WORKSPACE_TABS + 3 }, (_, index) => tab(`t${index}`, "notes")));
    for (let index = 0; index < MAX_CLOSED_WORKSPACE_TABS + 2; index += 1) {
      current = closeWorkspaceTab(current, `t${index}`) ?? current;
    }
    expect(current.closed).toHaveLength(MAX_CLOSED_WORKSPACE_TABS);
    expect(current.closed[0].index).toBe(0);
  });

  it("closes other visible tabs and keeps detached ones", () => {
    const current = state([
      tab("a", "calendar"),
      tab("b", "projects"),
      tab("c", "chat", { detached: true }),
      tab("d", "notes"),
    ]);
    const next = closeOtherWorkspaceTabs(current, "b");
    expect(ids(next)).toEqual(["b", "c"]);
    expect(next.activeTabId).toBe("b");
    expect(next.closed.map((entry) => entry.location.kind)).toEqual(["notes", "calendar"]);
  });

  it("reopens closed tabs at their original positions, clamped to the current strip", () => {
    const current = closeOtherWorkspaceTabs(
      state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes"), tab("d", "chat")]),
      "a",
    );
    const createId = idFactory();
    const first = reopenWorkspaceTab(current, createId);
    expect(first?.tab.location.kind).toBe("chat");
    expect(first?.state.tabs.map((entry) => entry.location.kind)).toEqual(["calendar", "chat"]);
    const second = first && reopenWorkspaceTab(first.state, createId);
    expect(second?.state.tabs.map((entry) => entry.location.kind)).toEqual(["calendar", "chat", "notes"]);
    expect(second?.state.activeTabId).toBe(second?.tab.id);
    expect(second?.state.closed).toHaveLength(1);
  });

  it("reopens a chosen history entry and reports when nothing is left", () => {
    const current = closeOtherWorkspaceTabs(state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes")]), "a");
    expect(reopenWorkspaceTab(current, idFactory(), 1)?.tab.location.kind).toBe("projects");
    expect(reopenWorkspaceTab(state([tab("a", "calendar")]), idFactory())).toBeNull();
  });
});

describe("arranging workspace tabs", () => {
  it("moves a tab before or after a target", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes")]);
    expect(ids(moveWorkspaceTab(current, "a", "c", "after"))).toEqual(["b", "c", "a"]);
    expect(ids(moveWorkspaceTab(current, "c", "a", "before"))).toEqual(["c", "a", "b"]);
    expect(moveWorkspaceTab(current, "a", "b", "before")).toBe(current);
    expect(moveWorkspaceTab(current, "a", "missing", "before")).toBe(current);
  });

  it("cycles through visible tabs and skips detached ones", () => {
    const current = state([tab("a", "calendar"), tab("b", "chat", { detached: true }), tab("c", "notes")], "c");
    expect(relativeWorkspaceTabId(current, 1)).toBe("a");
    expect(relativeWorkspaceTabId(current, -1)).toBe("a");
  });

  it("selects tabs by position, with the ninth position meaning the last tab", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes")]);
    expect(workspaceTabIdAtPosition(current, 1)).toBe("b");
    expect(workspaceTabIdAtPosition(current, 4)).toBeNull();
    expect(workspaceTabIdAtPosition(current, 8)).toBe("c");
  });
});

describe("detached workspace tabs", () => {
  it("hides a detached tab and moves focus to a neighbor", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects")], "b");
    const next = detachWorkspaceTab(current, "b");
    expect(visibleWorkspaceTabs(next).map((entry) => entry.id)).toEqual(["a"]);
    expect(next.activeTabId).toBe("a");
    expect(activeWorkspaceTab(next).id).toBe("a");
  });

  it("keeps focus when an inactive tab detaches and refuses to detach the only visible tab", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects")], "a");
    expect(detachWorkspaceTab(current, "b").activeTabId).toBe("a");
    const single = state([tab("a", "calendar")]);
    expect(detachWorkspaceTab(single, "a")).toBe(single);
  });
});

describe("workspace tab persistence", () => {
  it("round-trips tabs without the runtime detached flag", () => {
    const current: WorkspaceTabsState = {
      tabs: [
        tab("a", "calendar"),
        createWorkspaceTab("b", pageOne, { level: "item", locked: true, title: "Page" }),
      ],
      activeTabId: "b",
      closed: [{ level: "project", locked: false, location: projectA, title: "A", index: 1 }],
    };
    current.tabs[1] = { ...current.tabs[1], detached: true };
    const parsed = parseWorkspaceTabsState(JSON.parse(serializeWorkspaceTabsState(current)), ALL_KINDS);
    expect(parsed?.tabs[1]).toEqual({ ...current.tabs[1], detached: false });
    expect(parsed?.activeTabId).toBe("b");
    expect(parsed?.closed).toEqual(current.closed);
  });

  it("drops duplicate, unavailable, and malformed tabs", () => {
    const parsed = parseWorkspaceTabsState({
      tabs: [
        { id: "a", level: "kind", locked: false, location: kindLocation("calendar") },
        { id: "a", level: "kind", locked: false, location: kindLocation("notes") },
        { id: "b", level: "kind", locked: false, location: kindLocation("chat") },
        { id: "c", level: "bogus", location: kindLocation("notes") },
        { id: "", level: "kind", location: kindLocation("notes") },
      ],
      activeTabId: "b",
    }, ["calendar", "projects", "notes"]);
    expect(parsed?.tabs.map((entry) => entry.id)).toEqual(["a"]);
    expect(parsed?.activeTabId).toBe("a");
    expect(parsed?.closed).toEqual([]);
  });

  it("lowers persisted levels that their location cannot support", () => {
    const parsed = parseWorkspaceTabsState({
      tabs: [{ id: "a", level: "item", locked: true, location: { kind: "projects", projectId: null, item: null } }],
      activeTabId: "a",
    }, ALL_KINDS);
    expect(parsed?.tabs[0]).toMatchObject({ level: "kind", locked: false });
  });

  it("returns null when no usable tab remains", () => {
    expect(parseWorkspaceTabsState({ tabs: [] }, ALL_KINDS)).toBeNull();
    expect(parseWorkspaceTabsState("tabs", ALL_KINDS)).toBeNull();
  });
});

describe("workspace tab locations", () => {
  it("rejects item types that do not belong to the kind", () => {
    expect(parseWorkspaceTabLocation({ kind: "chat", projectId: "a", item: { type: "notes-page", id: "p" } }))
      .toBeNull();
    expect(parseWorkspaceTabLocation({ kind: "notes", projectId: "a", item: { type: "notes-folder", id: "f" } }))
      .toEqual({ kind: "notes", projectId: "a", item: { type: "notes-folder", id: "f" } });
  });

  it("normalizes Calendar locations and rejects unknown kinds", () => {
    expect(parseWorkspaceTabLocation({ kind: "calendar", projectId: "a" })).toEqual(kindLocation("calendar"));
    expect(parseWorkspaceTabLocation({ kind: "diary" })).toBeNull();
    expect(parseWorkspaceTabLocation({ kind: "notes", projectId: 4 })).toBeNull();
    expect(parseWorkspaceTabLocation({ kind: "notes", projectId: "x".repeat(2_000) })).toBeNull();
  });
});
