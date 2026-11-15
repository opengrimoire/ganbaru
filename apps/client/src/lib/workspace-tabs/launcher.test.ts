import { describe, expect, it } from "vitest";
import { buildProjectLists } from "$lib/projects/lists";
import type { ProjectGroup } from "$lib/projects/types";
import {
  tabLauncherCreateKinds,
  tabLauncherDigitsPick,
  tabLauncherHighlightIndex,
  tabLauncherKeyAction,
  tabLauncherKindDrills,
  tabLauncherLevel,
  tabLauncherProjectRows,
  tabLauncherRowNumbers,
  type TabLauncherKeyContext,
  type TabLauncherKeyInput,
  type TabLauncherRowShape,
  type TabLauncherStep,
} from "./launcher";

function group(id: string): ProjectGroup {
  return { id, name: id, icon: "lucide:folder", orderKey: "a0", collapsed: false, createdAt: "c", updatedAt: "u" };
}

function key(key: string, overrides: Partial<TabLauncherKeyInput> = {}): TabLauncherKeyInput {
  const digit = /^[0-9]$/.test(key) ? `Digit${key}` : key.length === 1 ? `Key${key.toUpperCase()}` : key;
  return { key, code: digit, isComposing: false, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...overrides };
}

const here: TabLauncherRowShape & { key: string } = { key: "here", numbered: false, canDrill: false };
const page: TabLauncherRowShape & { key: string } = { key: "page:a", numbered: true, canDrill: true };
const leaf: TabLauncherRowShape & { key: string } = { key: "page:b", numbered: true, canDrill: false };
const newFolder: TabLauncherRowShape & { key: string } = { key: "create:folder", numbered: false, canDrill: false, create: { primary: false } };
const newNote: TabLauncherRowShape & { key: string } = { key: "create:page", numbered: false, canDrill: false, create: { primary: true } };

function context(overrides: Partial<TabLauncherKeyContext> = {}): TabLauncherKeyContext {
  return { query: "", searchable: true, atKinds: false, rows: [here, page, leaf], highlight: 0, caretAtEnd: true, ...overrides };
}

describe("tabLauncherLevel", () => {
  const notes: TabLauncherStep = { type: "kind", kind: "notes", label: "Notes" };
  const work: TabLauncherStep = { type: "group", groupId: "work", label: "Work" };
  const report: TabLauncherStep = { type: "project", projectId: "report", label: "Report" };

  it("shows the kinds before a kind is picked", () => {
    expect(tabLauncherLevel([])).toEqual({ type: "kinds" });
  });

  it("shows a kind's projects, inside the last group picked", () => {
    expect(tabLauncherLevel([notes])).toEqual({ type: "projects", kind: "notes", groupId: null });
    expect(tabLauncherLevel([notes, work])).toEqual({ type: "projects", kind: "notes", groupId: "work" });
  });

  it("shows a project's channels for Chat and its root pages for Notes", () => {
    expect(tabLauncherLevel([{ type: "kind", kind: "chat", label: "Chat" }, work, report]))
      .toEqual({ type: "channels", projectId: "report" });
    expect(tabLauncherLevel([notes, work, report]))
      .toEqual({ type: "pages", projectId: "report", parent: { kind: "root" } });
  });

  it("shows the children of the last Notes page or folder picked", () => {
    const folder: TabLauncherStep = { type: "folder", folderId: "drafts", label: "Drafts" };
    const child: TabLauncherStep = { type: "page", pageId: "intro", label: "Intro" };
    expect(tabLauncherLevel([notes, report, folder])).toEqual({ type: "pages", projectId: "report", parent: { kind: "folder", id: "drafts" } });
    expect(tabLauncherLevel([notes, report, folder, child])).toEqual({ type: "pages", projectId: "report", parent: { kind: "page", id: "intro" } });
  });

  it("stops Projects at the project level", () => {
    expect(tabLauncherLevel([{ type: "kind", kind: "projects", label: "Projects" }, report]))
      .toEqual({ type: "projects", kind: "projects", groupId: null });
  });

  it("drills every kind but Calendar", () => {
    expect(tabLauncherKindDrills("calendar")).toBe(false);
    expect(["projects", "notes", "chat"].every((kind) => tabLauncherKindDrills(kind as "projects"))).toBe(true);
  });
});

describe("tabLauncherProjectRows", () => {
  const projectsByGroup = new Map<string | null, { name: string }[]>([
    ["work", [{ name: "Report" }, { name: "Roadmap" }]],
    ["empty", []],
    ["home", [{ name: "Garden" }]],
    [null, [{ name: "Loose report" }]],
  ]);
  const lists = buildProjectLists([group("work"), group("empty"), group("home")], (id) => projectsByGroup.get(id) ?? []);

  it("lists every group, empty ones included, then the projects outside every group", () => {
    const rows = tabLauncherProjectRows(lists, null, "");
    expect(rows.groups.map((candidate) => candidate.id)).toEqual(["work", "empty", "home"]);
    expect(rows.sections).toEqual([{ list: lists[3], labeled: false }]);
  });

  it("lists only the open group's projects", () => {
    const rows = tabLauncherProjectRows(lists, "home", "");
    expect(rows.groups).toEqual([]);
    expect(rows.sections.map((section) => section.list.projects)).toEqual([[{ name: "Garden" }]]);
  });

  it("searches every project, even inside a group, and labels each list", () => {
    const rows = tabLauncherProjectRows(lists, "home", " REPORT ");
    expect(rows.groups).toEqual([]);
    expect(rows.sections.map((section) => [section.list.key, section.labeled, section.list.projects.map((project) => project.name)]))
      .toEqual([["group:work", true, ["Report"]], ["ungrouped", true, ["Loose report"]]]);
  });

  it("lists nothing for a group that is gone", () => {
    expect(tabLauncherProjectRows(lists, "missing", "")).toEqual({ groups: [], sections: [] });
  });
});

describe("tabLauncherCreateKinds", () => {
  it("offers a group and a project at the top of a projects level, and only a project inside a group", () => {
    expect(tabLauncherCreateKinds({ type: "projects", kind: "notes", groupId: null }))
      .toEqual([{ kind: "group", primary: false }, { kind: "project", primary: true }]);
    expect(tabLauncherCreateKinds({ type: "projects", kind: "chat", groupId: "work" })).toEqual([{ kind: "project", primary: true }]);
  });

  it("offers a folder and a note outside a note, and only a note under one", () => {
    expect(tabLauncherCreateKinds({ type: "pages", projectId: "report", parent: { kind: "root" } }))
      .toEqual([{ kind: "folder", primary: false }, { kind: "page", primary: true }]);
    expect(tabLauncherCreateKinds({ type: "pages", projectId: "report", parent: { kind: "folder", id: "drafts" } }))
      .toEqual([{ kind: "folder", primary: false }, { kind: "page", primary: true }]);
    expect(tabLauncherCreateKinds({ type: "pages", projectId: "report", parent: { kind: "page", id: "intro" } }))
      .toEqual([{ kind: "page", primary: true }]);
  });

  it("offers a channel on a channels level and nothing on the kinds", () => {
    expect(tabLauncherCreateKinds({ type: "channels", projectId: "report" })).toEqual([{ kind: "channel", primary: true }]);
    expect(tabLauncherCreateKinds({ type: "kinds" })).toEqual([]);
  });
});

describe("tabLauncherRowNumbers", () => {
  it("numbers only numbered rows, from 1 up to 9", () => {
    const rows = [here, ...Array.from({ length: 10 }, () => leaf)];
    expect(tabLauncherRowNumbers(rows, true)).toEqual([null, 1, 2, 3, 4, 5, 6, 7, 8, 9, null]);
  });

  it("shows no numbers while digits type into the search", () => {
    expect(tabLauncherDigitsPick("re", true)).toBe(false);
    expect(tabLauncherDigitsPick("re", false)).toBe(true);
    expect(tabLauncherDigitsPick("", true)).toBe(true);
    expect(tabLauncherRowNumbers([here, page], false)).toEqual([null, null]);
  });

  it("never numbers create actions", () => {
    expect(tabLauncherRowNumbers([page, newFolder, newNote], true)).toEqual([1, null, null]);
  });
});

describe("tabLauncherHighlightIndex", () => {
  it("highlights the open here row by default and the first result while searching", () => {
    expect(tabLauncherHighlightIndex([here, page, leaf], null, "")).toBe(0);
    expect(tabLauncherHighlightIndex([here, page, leaf], null, "b")).toBe(1);
    expect(tabLauncherHighlightIndex([leaf], null, "b")).toBe(0);
  });

  it("keeps a remembered row while it is listed", () => {
    expect(tabLauncherHighlightIndex([here, page, leaf], "page:b", "")).toBe(2);
    expect(tabLauncherHighlightIndex([here, page], "page:b", "")).toBe(0);
  });

  it("highlights nothing when nothing is listed", () => {
    expect(tabLauncherHighlightIndex([], null, "")).toBe(-1);
  });

  it("highlights the primary create action of a level that lists nothing else", () => {
    expect(tabLauncherHighlightIndex([newFolder, newNote], null, "")).toBe(1);
    expect(tabLauncherHighlightIndex([page, newFolder, newNote], null, "")).toBe(0);
    expect(tabLauncherHighlightIndex([newFolder], null, "")).toBe(0);
    expect(tabLauncherHighlightIndex([newFolder, newNote], "create:folder", "")).toBe(0);
  });
});

describe("tabLauncherKeyAction", () => {
  it("clears a search, then goes back, then closes on Escape", () => {
    expect(tabLauncherKeyAction(key("Escape"), context({ query: "x" }))).toEqual({ type: "clear-search" });
    expect(tabLauncherKeyAction(key("Escape"), context())).toEqual({ type: "back" });
    expect(tabLauncherKeyAction(key("Escape"), context({ atKinds: true, searchable: false }))).toEqual({ type: "close" });
  });

  it("moves the highlight with wrapping arrows", () => {
    expect(tabLauncherKeyAction(key("ArrowDown"), context({ highlight: 2 }))).toEqual({ type: "highlight", index: 0 });
    expect(tabLauncherKeyAction(key("ArrowUp"), context({ highlight: 0 }))).toEqual({ type: "highlight", index: 2 });
    expect(tabLauncherKeyAction(key("ArrowDown"), context({ highlight: -1 }))).toEqual({ type: "highlight", index: 0 });
    expect(tabLauncherKeyAction(key("ArrowDown"), context({ rows: [] }))).toBeNull();
  });

  it("jumps to the ends with Home and End only while they would not move the search caret", () => {
    expect(tabLauncherKeyAction(key("End"), context())).toEqual({ type: "highlight", index: 2 });
    expect(tabLauncherKeyAction(key("Home"), context({ highlight: 2 }))).toEqual({ type: "highlight", index: 0 });
    expect(tabLauncherKeyAction(key("End"), context({ query: "x" }))).toBeNull();
    expect(tabLauncherKeyAction(key("End"), context({ query: "x", searchable: false }))).toEqual({ type: "highlight", index: 2 });
  });

  it("picks the highlighted row on Enter", () => {
    expect(tabLauncherKeyAction(key("Enter"), context({ highlight: 1 }))).toEqual({ type: "pick", index: 1 });
    expect(tabLauncherKeyAction(key("Enter"), context({ highlight: -1 }))).toBeNull();
  });

  it("picks a numbered row by digit only while the search is empty", () => {
    expect(tabLauncherKeyAction(key("1"), context())).toEqual({ type: "pick", index: 1 });
    expect(tabLauncherKeyAction(key("2"), context())).toEqual({ type: "pick", index: 2 });
    expect(tabLauncherKeyAction(key("1"), context({ query: "a" }))).toBeNull();
    expect(tabLauncherKeyAction(key("1", { ctrlKey: true }), context())).toBeNull();
  });

  it("lets a digit without a row type into the search", () => {
    expect(tabLauncherKeyAction(key("7"), context())).toBeNull();
  });

  it("picks kinds by digit, since the kinds level has no search", () => {
    const kinds = [leaf, leaf, leaf, leaf];
    expect(tabLauncherKeyAction(key("4"), context({ atKinds: true, searchable: false, rows: kinds }))).toEqual({ type: "pick", index: 3 });
  });

  it("leaves letters to the search", () => {
    expect(tabLauncherKeyAction(key("a"), context())).toBeNull();
  });

  it("drills a row with more inside on ArrowRight, unless the caret can still move right", () => {
    expect(tabLauncherKeyAction(key("ArrowRight"), context({ highlight: 1 }))).toEqual({ type: "drill", index: 1 });
    expect(tabLauncherKeyAction(key("ArrowRight"), context({ highlight: 2 }))).toBeNull();
    expect(tabLauncherKeyAction(key("ArrowRight"), context({ highlight: 1, query: "ab", caretAtEnd: false }))).toBeNull();
    expect(tabLauncherKeyAction(key("ArrowRight"), context({ highlight: 1, query: "ab" }))).toEqual({ type: "drill", index: 1 });
  });

  it("goes back on ArrowLeft or Backspace in an empty search below the kinds", () => {
    expect(tabLauncherKeyAction(key("Backspace"), context())).toEqual({ type: "back" });
    expect(tabLauncherKeyAction(key("ArrowLeft"), context())).toEqual({ type: "back" });
    expect(tabLauncherKeyAction(key("Backspace"), context({ query: "a" }))).toBeNull();
    expect(tabLauncherKeyAction(key("ArrowLeft"), context({ atKinds: true, searchable: false }))).toBeNull();
  });

  it("creates with the shortcut modifier and N only where the level allows it", () => {
    const create = key("n", { ctrlKey: true, metaKey: false });
    const appleCreate = key("n", { metaKey: true });
    const allowed = context({ createShortcut: true });
    const createOnThisPlatform = tabLauncherKeyAction(create, allowed) ?? tabLauncherKeyAction(appleCreate, allowed);
    expect(createOnThisPlatform).toEqual({ type: "create" });
    expect(tabLauncherKeyAction(create, context())).toBeNull();
    expect(tabLauncherKeyAction(key("n"), allowed)).toBeNull();
    expect(tabLauncherKeyAction(key("N", { ctrlKey: true, metaKey: true, shiftKey: true }), allowed)).toBeNull();
  });

  it("reaches create actions with the arrows, End, and Enter but never with a digit", () => {
    const rows = [page, leaf, newFolder, newNote];
    expect(tabLauncherKeyAction(key("ArrowDown"), context({ rows, highlight: 1 }))).toEqual({ type: "highlight", index: 2 });
    expect(tabLauncherKeyAction(key("ArrowDown"), context({ rows, highlight: 3 }))).toEqual({ type: "highlight", index: 0 });
    expect(tabLauncherKeyAction(key("End"), context({ rows }))).toEqual({ type: "highlight", index: 3 });
    expect(tabLauncherKeyAction(key("Enter"), context({ rows, highlight: 2 }))).toEqual({ type: "pick", index: 2 });
    expect(tabLauncherKeyAction(key("ArrowRight"), context({ rows, highlight: 3 }))).toBeNull();
    expect(tabLauncherKeyAction(key("3"), context({ rows }))).toBeNull();
  });

  it("ignores keys during composition", () => {
    expect(tabLauncherKeyAction(key("Enter", { isComposing: true }), context())).toBeNull();
  });
});
