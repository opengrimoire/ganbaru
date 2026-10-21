import { describe, expect, it } from "vitest";
import type { View } from "$lib/navigation";
import {
  activeWorkspaceTab,
  closeWorkspaceTab,
  createWorkspaceTab,
  DEFAULT_SPLIT_RATIO,
  defaultWorkspaceTabsState,
  insertWorkspaceTab,
  isWithinTabScope,
  kindLocation,
  MAX_CLOSED_WORKSPACE_TABS,
  MAX_WORKSPACE_TABS,
  MIN_SPLIT_RATIO,
  moveWorkspaceTab,
  normalizeWorkspaceSplits,
  parseClosedWorkspaceTabs,
  parseWorkspaceTabLocation,
  parseWorkspaceTabsState,
  relativeWorkspaceTabId,
  rememberClosedWorkspaceTabs,
  reopenWorkspaceTab,
  resizeWorkspaceSplit,
  savedWindowLabels,
  serializeClosedWorkspaceTabs,
  serializeWorkspaceTabsState,
  setWorkspaceSplitOrientation,
  showWorkspaceTabNeighbor,
  splitWorkspaceTabs,
  swapWorkspaceSplit,
  tabMatchesTarget,
  takeWorkspaceTab,
  unsplitWorkspaceTab,
  updateWorkspaceTab,
  workspaceTabIdAtPosition,
  secondaryScopeTabId,
  workspaceSplitPartnerId,
  workspaceTabLockLevel,
  type ClosedWorkspaceTab,
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
  return { tabs, activeTabId, closed: [], splits: [] };
}

/** A closed kind tab remembered at a strip position, most recent first when listed. */
function closedAt(kind: View, index: number): ClosedWorkspaceTab {
  return { level: "kind", locked: false, location: kindLocation(kind), title: null, projectTitle: null, index };
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
  it("opens only Calendar on first use", () => {
    const initial = defaultWorkspaceTabsState(ALL_KINDS, idFactory());
    expect(initial.tabs.map((entry) => entry.location.kind)).toEqual(["calendar"]);
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

  it("locks a kind tab to its project, else its item, and keeps narrower levels", () => {
    expect(workspaceTabLockLevel(createWorkspaceTab("t", { ...pageOne, projectId: "a" }))).toBe("project");
    expect(workspaceTabLockLevel(createWorkspaceTab("t", { ...pageOne, projectId: null }))).toBe("item");
    expect(workspaceTabLockLevel(createWorkspaceTab("t", kindLocation("notes")))).toBeNull();
    expect(workspaceTabLockLevel(createWorkspaceTab("t", pageOne, { level: "item" }))).toBe("item");
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

  it("inserts in the background after a given tab without changing the active tab", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes")]);
    const first = insertWorkspaceTab(current, tab("x", "chat"), "after-active", false);
    expect(ids(first)).toEqual(["a", "x", "b", "c"]);
    expect(first.activeTabId).toBe("a");
    const second = insertWorkspaceTab(first, tab("y", "chat"), { afterId: "x" }, false);
    expect(ids(second)).toEqual(["a", "x", "y", "b", "c"]);
    expect(second.activeTabId).toBe("a");
  });

  it("falls back to after the active tab when the anchor tab is gone", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects")]);
    expect(ids(insertWorkspaceTab(current, tab("x", "chat"), { afterId: "missing" }))).toEqual(["a", "x", "b"]);
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

  it("refuses to close the window's last tab", () => {
    expect(closeWorkspaceTab(state([tab("a", "calendar")]), "a")).toBeNull();
  });

  it("remembers the tabs of a closed window after the current strip", () => {
    const current = state([tab("a", "calendar"), tab("b", "notes")]);
    const next = rememberClosedWorkspaceTabs(current, [
      { ...tab("x", "projects"), location: projectB },
      tab("y", "chat"),
    ]);
    expect(ids(next)).toEqual(["a", "b"]);
    expect(next.closed.map((entry) => [entry.location.kind, entry.index])).toEqual([["chat", 2], ["projects", 2]]);
    expect(next.closed[1].location).toEqual(projectB);
    expect(rememberClosedWorkspaceTabs(current, [])).toBe(current);
  });

  it("keeps a bounded reopen history with the newest first", () => {
    let current = state(Array.from({ length: MAX_CLOSED_WORKSPACE_TABS + 3 }, (_, index) => tab(`t${index}`, "notes")));
    for (let index = 0; index < MAX_CLOSED_WORKSPACE_TABS + 2; index += 1) {
      current = closeWorkspaceTab(current, `t${index}`) ?? current;
    }
    expect(current.closed).toHaveLength(MAX_CLOSED_WORKSPACE_TABS);
    expect(current.closed[0].index).toBe(0);
  });

  it("reopens closed tabs at their original positions, clamped to the current strip", () => {
    const current = {
      ...state([tab("a", "calendar")]),
      closed: [closedAt("chat", 3), closedAt("notes", 2), closedAt("projects", 1)],
    };
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
    const current = { ...state([tab("a", "calendar")]), closed: [closedAt("notes", 2), closedAt("projects", 1)] };
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

  it("cycles through tabs in both directions", () => {
    const current = state([tab("a", "calendar"), tab("b", "chat"), tab("c", "notes")], "c");
    expect(relativeWorkspaceTabId(current, 1)).toBe("a");
    expect(relativeWorkspaceTabId(current, -1)).toBe("b");
  });

  it("selects tabs by position, with the ninth position meaning the last tab", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes")]);
    expect(workspaceTabIdAtPosition(current, 1)).toBe("b");
    expect(workspaceTabIdAtPosition(current, 4)).toBeNull();
    expect(workspaceTabIdAtPosition(current, 8)).toBe("c");
  });
});

describe("tabs leaving for another window", () => {
  it("removes the tab without remembering it and moves focus to a neighbor", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes")], "b");
    const next = takeWorkspaceTab(current, "b");
    expect(next && ids(next)).toEqual(["a", "c"]);
    expect(next?.activeTabId).toBe("c");
    expect(next && activeWorkspaceTab(next).id).toBe("c");
    expect(next?.closed).toEqual([]);
  });

  it("keeps focus when an inactive tab leaves and reports when nothing would be left", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects")], "a");
    expect(takeWorkspaceTab(current, "b")?.activeTabId).toBe("a");
    expect(takeWorkspaceTab(current, "missing")).toBe(current);
    expect(takeWorkspaceTab(state([tab("a", "calendar")]), "a")).toBeNull();
  });

  it("shows the next tab, else the previous one, while the active tab is pulled out, keeping its slot", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes")], "b");
    const middle = showWorkspaceTabNeighbor(current, "b");
    expect(ids(middle)).toEqual(["a", "b", "c"]);
    expect(middle.activeTabId).toBe("c");
    expect(showWorkspaceTabNeighbor({ ...current, activeTabId: "c" }, "c").activeTabId).toBe("b");
  });

  it("leaves the state alone for an inactive tab or one alone in its window", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects")], "a");
    expect(showWorkspaceTabNeighbor(current, "b")).toBe(current);
    const single = state([tab("a", "calendar")]);
    expect(showWorkspaceTabNeighbor(single, "a")).toBe(single);
  });
});

describe("workspace pane scopes", () => {
  const hostedNotes: WorkspaceTabLocation = { kind: "projects", projectId: "a", item: { type: "project-view", id: "notes-view" } };
  const paneView = (location: WorkspaceTabLocation): View => (
    location.item?.type === "project-view" && location.item.id === "notes-view" ? "notes" : location.kind
  );
  const first = tab("one", "notes", { location: pageOne });
  const second = tab("two", "notes", { location: pageOne });

  it("gives no tab secondary stores without a partner or when the shown tabs use different stores", () => {
    expect(secondaryScopeTabId(first, null, null, paneView)).toBeNull();
    expect(secondaryScopeTabId(first, tab("cal", "calendar"), "cal", paneView)).toBeNull();
  });

  it("gives the partner secondary stores, leaving the primary ones with the tab that showed first", () => {
    expect(secondaryScopeTabId(first, second, null, paneView)).toBe("two");
    expect(secondaryScopeTabId(first, second, "gone", paneView)).toBe("two");
  });

  it("keeps secondary stores on the tab that has them when the other pane takes focus", () => {
    expect(secondaryScopeTabId(second, first, "two", paneView)).toBe("two");
  });

  it("gives secondary stores to a project view hosting the view of the tab beside it", () => {
    const hosted = tab("hosted", "projects", { location: hostedNotes });
    expect(secondaryScopeTabId(first, hosted, null, paneView)).toBe("hosted");
  });

  it("gives the partner secondary stores when both views follow the selected project", () => {
    expect(secondaryScopeTabId(first, tab("tasks", "projects", { location: projectA }), null, paneView)).toBe("tasks");
    expect(secondaryScopeTabId(tab("talk", "chat"), first, null, paneView)).toBe("one");
  });
});

describe("workspace split views", () => {
  const strip = () => state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes"), tab("d", "chat")], "a");

  it("shows the added tab beside the current one, next to it in the strip, keeping the current tab active", () => {
    const next = splitWorkspaceTabs(strip(), "a", "c");
    expect(ids(next)).toEqual(["a", "c", "b", "d"]);
    expect(next.activeTabId).toBe("a");
    expect(next.splits).toEqual([{ tabIds: ["a", "c"], orientation: "side-by-side", ratio: DEFAULT_SPLIT_RATIO }]);
    expect(workspaceSplitPartnerId(next, "c")).toBe("a");
  });

  it("replaces the partner of a tab already split, keeping its side and layout", () => {
    const stacked = setWorkspaceSplitOrientation(resizeWorkspaceSplit(splitWorkspaceTabs(strip(), "b", "a"), "a", 0.3), "a", "stacked");
    expect(ids(stacked)).toEqual(["b", "a", "c", "d"]);
    const next = splitWorkspaceTabs(stacked, "a", "d");
    expect(ids(next)).toEqual(["b", "d", "a", "c"]);
    expect(next.splits).toEqual([{ tabIds: ["d", "a"], orientation: "stacked", ratio: 0.3 }]);
  });

  it("takes an added tab out of the split it was in", () => {
    const first = splitWorkspaceTabs(strip(), "a", "b");
    const next = splitWorkspaceTabs({ ...first, activeTabId: "c" }, "c", "b");
    expect(next.splits.map((split) => split.tabIds)).toEqual([["c", "b"]]);
    expect(ids(next)).toEqual(["a", "c", "b", "d"]);
  });

  it("refuses a tab paired with itself or a missing tab", () => {
    const current = strip();
    expect(splitWorkspaceTabs(current, "a", "a")).toBe(current);
    expect(splitWorkspaceTabs(current, "a", "missing")).toBe(current);
  });

  it("swaps the sides and the strip order, mirroring the ratio", () => {
    const split = resizeWorkspaceSplit(splitWorkspaceTabs(strip(), "a", "b"), "b", 0.3);
    const next = swapWorkspaceSplit(split, "a");
    expect(ids(next)).toEqual(["b", "a", "c", "d"]);
    expect(next.splits[0]).toMatchObject({ tabIds: ["b", "a"], ratio: 0.7 });
  });

  it("keeps the ratio within bounds and ignores tabs outside a split", () => {
    const split = splitWorkspaceTabs(strip(), "a", "b");
    expect(resizeWorkspaceSplit(split, "a", 0.01).splits[0].ratio).toBe(MIN_SPLIT_RATIO);
    expect(resizeWorkspaceSplit(split, "a", 0.99).splits[0].ratio).toBe(1 - MIN_SPLIT_RATIO);
    expect(resizeWorkspaceSplit(split, "a", Number.NaN).splits[0].ratio).toBe(DEFAULT_SPLIT_RATIO);
    expect(resizeWorkspaceSplit(split, "c", 0.3)).toBe(split);
    expect(swapWorkspaceSplit(split, "d")).toBe(split);
    expect(setWorkspaceSplitOrientation(split, "a", "side-by-side")).toBe(split);
  });

  it("ends a split without moving its tabs", () => {
    const next = unsplitWorkspaceTab(splitWorkspaceTabs(strip(), "a", "c"), "c");
    expect(next.splits).toEqual([]);
    expect(ids(next)).toEqual(["a", "c", "b", "d"]);
  });

  it("moves a split as one block and never drops a tab between its two tabs", () => {
    const split = splitWorkspaceTabs(strip(), "a", "b");
    expect(ids(moveWorkspaceTab(split, "b", "d", "after"))).toEqual(["c", "d", "a", "b"]);
    expect(ids(moveWorkspaceTab(split, "d", "a", "after"))).toEqual(["a", "b", "d", "c"]);
    expect(ids(moveWorkspaceTab(split, "d", "b", "before"))).toEqual(["d", "a", "b", "c"]);
    expect(moveWorkspaceTab(split, "a", "b", "after")).toBe(split);
  });

  it("opens new tabs after the whole split", () => {
    const split = splitWorkspaceTabs(strip(), "a", "b");
    expect(ids(insertWorkspaceTab(split, tab("e", "notes"), "after-active"))).toEqual(["a", "b", "e", "c", "d"]);
    expect(ids(insertWorkspaceTab(split, tab("e", "notes"), { afterId: "a" }))).toEqual(["a", "b", "e", "c", "d"]);
  });

  it("ends the split when one of its tabs closes, focusing the other one", () => {
    const split = splitWorkspaceTabs(strip(), "a", "b");
    const next = closeWorkspaceTab({ ...split, activeTabId: "b" }, "b");
    expect(next?.splits).toEqual([]);
    expect(next?.activeTabId).toBe("a");
    expect(next?.closed[0].location.kind).toBe("projects");
  });

  it("reopens a tab after a split instead of between its tabs", () => {
    const split = splitWorkspaceTabs(state([tab("a", "calendar"), tab("b", "projects")]), "a", "b");
    const reopened = reopenWorkspaceTab({ ...split, closed: [closedAt("notes", 1)] }, idFactory());
    expect(reopened && ids(reopened.state)).toEqual(["a", "b", "new-1"]);
    expect(reopened?.state.splits).toHaveLength(1);
  });

  it("drops splits that point at missing tabs or claim a tab twice, and rejoins separated ones", () => {
    const current = state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes")]);
    const normalized = normalizeWorkspaceSplits({
      ...current,
      splits: [
        { tabIds: ["a", "c"], orientation: "side-by-side", ratio: 0.5 },
        { tabIds: ["c", "b"], orientation: "side-by-side", ratio: 0.5 },
        { tabIds: ["b", "missing"], orientation: "side-by-side", ratio: 0.5 },
        { tabIds: ["b", "b"], orientation: "side-by-side", ratio: 0.5 },
      ],
    });
    expect(normalized.splits.map((split) => split.tabIds)).toEqual([["a", "c"]]);
    expect(ids(normalized)).toEqual(["a", "c", "b"]);
    expect(normalizeWorkspaceSplits(normalized)).toBe(normalized);
  });
});

describe("workspace tab persistence", () => {
  it("round-trips split views and drops malformed ones", () => {
    const split = setWorkspaceSplitOrientation(
      splitWorkspaceTabs(state([tab("a", "calendar"), tab("b", "projects"), tab("c", "notes")]), "a", "c"),
      "a",
      "stacked",
    );
    const raw = JSON.parse(serializeWorkspaceTabsState(split)) as Record<string, unknown>;
    expect(parseWorkspaceTabsState(raw, ALL_KINDS)?.splits).toEqual(split.splits);
    const malformed = parseWorkspaceTabsState({
      ...raw,
      splits: [
        { tabIds: ["a"], orientation: "stacked", ratio: 0.5 },
        { tabIds: ["a", "b"], orientation: "diagonal", ratio: 0.5 },
        { tabIds: ["b", "missing"], orientation: "stacked", ratio: 0.5 },
        { tabIds: ["b", "c"], orientation: "stacked", ratio: 3 },
      ],
    }, ALL_KINDS);
    expect(malformed?.splits).toEqual([{ tabIds: ["b", "c"], orientation: "stacked", ratio: 1 - MIN_SPLIT_RATIO }]);
    expect(parseWorkspaceTabsState({ ...raw, splits: "split" }, ALL_KINDS)?.splits).toEqual([]);
  });

  it("round-trips a window's tabs and active tab, leaving the shared reopen history out", () => {
    const current: WorkspaceTabsState = {
      tabs: [
        tab("a", "calendar"),
        createWorkspaceTab("b", pageOne, { level: "item", locked: true, title: "Page", projectTitle: "Alpha" }),
      ],
      activeTabId: "b",
      closed: [{ level: "project", locked: false, location: projectA, title: "A", projectTitle: "Alpha", index: 1 }],
      splits: [],
    };
    const parsed = parseWorkspaceTabsState(JSON.parse(serializeWorkspaceTabsState(current)), ALL_KINDS);
    expect(parsed?.tabs).toEqual(current.tabs);
    expect(parsed?.activeTabId).toBe("b");
    expect(parsed?.closed).toEqual([]);
  });

  it("round-trips the shared reopen history", () => {
    const closed: ClosedWorkspaceTab[] = [
      { level: "project", locked: true, location: projectA, title: "A", projectTitle: "Alpha", index: 1 },
      { level: "kind", locked: false, location: kindLocation("calendar"), title: null, projectTitle: null, index: 0 },
    ];
    expect(parseClosedWorkspaceTabs(JSON.parse(serializeClosedWorkspaceTabs(closed)), ALL_KINDS)).toEqual(closed);
  });

  it("keeps only valid, available entries of the reopen history, up to its limit", () => {
    const valid = { level: "kind", locked: false, location: kindLocation("calendar"), index: 0 };
    expect(parseClosedWorkspaceTabs([
      valid,
      { level: "kind", locked: false, location: kindLocation("chat"), index: 0 },
      { level: "kind", locked: false, location: kindLocation("notes"), index: -1 },
      "closed",
    ], ["calendar", "notes"])).toHaveLength(1);
    expect(parseClosedWorkspaceTabs(Array.from({ length: 20 }, () => valid), ALL_KINDS))
      .toHaveLength(MAX_CLOSED_WORKSPACE_TABS);
    expect(parseClosedWorkspaceTabs({ closed: [valid] }, ALL_KINDS)).toEqual([]);
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

describe("saved windows", () => {
  it("lists the windows a vault saved tabs for, without the main window or other vaults", () => {
    const keys = [
      "ganbaru.workspace-tabs.v1:vault-a",
      "ganbaru.workspace-tabs.v1:vault-a:view-1",
      "ganbaru.workspace-tabs.v1:vault-a:view-2",
      "ganbaru.workspace-tabs.v1:vault-ab:view-3",
      "ganbaru.workspace-tabs.v1:vault-b:view-4",
      "ganbaru.workspace-tabs.v1:vault-a:view-5:extra",
      "ganbaru.workspace-tabs.v1:vault-a:",
      "other-key",
    ];
    expect(savedWindowLabels(keys, "ganbaru.workspace-tabs.v1:vault-a:")).toEqual(["view-1", "view-2"]);
  });
});
