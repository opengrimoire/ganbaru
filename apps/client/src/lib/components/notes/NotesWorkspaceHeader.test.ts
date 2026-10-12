// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NotesChildDatabaseBlock, NotesDatabaseView, NotesFolder, NotesNavigationDatabase, NotesPage } from "$lib/notes/types";
import { applyBlockUpdate } from "$lib/notes/blocks/factory";
import { createProvisionalNotesPage } from "$lib/notes/pages/creation";
import NotesWorkspaceHeader from "./NotesWorkspaceHeader.svelte";

const notesState = vi.hoisted(() => ({
  store: {
    viewMode: "pages" as const,
    allPages: [] as NotesPage[],
    get navigationPages(): NotesPage[] { return this.allPages; },
    linkResolutionPages: [] as NotesPage[],
    folders: [] as NotesFolder[],
    sidebarPageIdsWithChildren: [] as string[],
    navigationDatabases: [] as NotesNavigationDatabase[],
    selectedDatabaseBlockId: null as string | null,
    selectedDatabaseBlock: null as NotesChildDatabaseBlock | null,
    closeDatabase: vi.fn(),
    openDatabase: vi.fn(async () => true),
    loadNavigationChildren: vi.fn(async () => undefined),
    selectPage: vi.fn(async (_pageId: string) => undefined),
    pageTitleDraftForPage: vi.fn((_pageId: string) => null),
    createPage: vi.fn(async () => undefined),
  },
}));

const databaseViews = vi.hoisted(() => vi.fn<() => Promise<NotesDatabaseView[]>>());
vi.mock("$lib/api/notes", () => ({ listNotesDatabaseViews: databaseViews }));

vi.mock("$lib/stores/notes.svelte", async () => {
  const { SvelteMap } = await import("svelte/reactivity");
  const selection = new SvelteMap<string, NotesChildDatabaseBlock | null>([["database", null]]);
  Object.defineProperty(notesState.store, "selectedDatabaseBlock", {
    get: () => selection.get("database") ?? null,
    set: (block: NotesChildDatabaseBlock | null) => selection.set("database", block),
  });
  return { getNotes: () => notesState.store };
});

vi.mock("$lib/stores/viewport.svelte", () => ({
  getViewport: () => ({ width: 1200, height: 800 }),
}));

vi.mock("$lib/stores/projects.svelte", () => ({
  getProjects: () => ({
    customEmojis: [],
    visibleGroups: () => [group],
    projectsForGroup: () => [project],
    projectsForGroupIncludingInactive: () => [project],
    projectById: (id: string | null) => id === project.id ? project : undefined,
    groupById: (id: string | null) => id === group.id ? group : undefined,
  }),
}));

const group = {
  id: "group-1",
  name: "Routine",
  icon: "repeat",
  sortOrder: 0,
  collapsed: false,
  createdAt: "2026-07-26T12:00:00.000Z",
  updatedAt: "2026-07-26T12:00:00.000Z",
};

const project = {
  id: "project-1",
  groupId: group.id,
  name: "Learning",
  icon: "graduation-cap",
  sortOrder: 0,
  status: "active" as const,
  defaultEventName: null,
  defaultEventTimeMode: "timed" as const,
  defaultEventDurationMinutes: null,
  defaultPomodoroMode: "preset" as const,
  defaultIdleSettingsSource: "global" as const,
  defaultIdlePauseEnabled: true,
  defaultIdleThresholdMinutes: 5 as const,
  createdAt: "2026-07-26T12:00:00.000Z",
  updatedAt: "2026-07-26T12:00:00.000Z",
};

const folder: NotesFolder = {
  object: "folder",
  id: "folder-1",
  project_id: project.id,
  parent_folder_id: null,
  name: "Lessons",
  created_time: "2026-07-26T12:00:00.000Z",
  last_edited_time: "2026-07-26T12:00:00.000Z",
};

const page: NotesPage = {
  object: "page",
  id: "page-1",
  created_time: "2026-07-26T12:00:00.000Z",
  last_edited_time: "2026-07-26T12:00:00.000Z",
  parent: { type: "workspace", workspace: true },
  folder_id: folder.id,
  in_trash: false,
  archived: false,
  icon: null,
  cover: null,
  properties: { __ganbaru_project_id: project.id },
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};

describe("NotesWorkspaceHeader", () => {
  const mounted: { target: HTMLDivElement; component: ReturnType<typeof mount> }[] = [];

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    });
  });

  afterEach(async () => {
    while (mounted.length > 0) {
      const entry = mounted.pop();
      if (!entry) continue;
      await unmount(entry.component);
      entry.target.remove();
    }
    notesState.store.allPages = [];
    notesState.store.linkResolutionPages = [];
    notesState.store.folders = [];
    notesState.store.selectedDatabaseBlockId = null;
    notesState.store.selectedDatabaseBlock = null;
    notesState.store.navigationDatabases = [];
    const { notesDatabaseSession } = await import("$lib/notes/database/session.svelte");
    notesDatabaseSession.clear();
    vi.clearAllMocks();
    vi.unstubAllGlobals();
  });

  function setup(explorerCollapsed: boolean, selectedPage = page, pages = [selectedPage], mobileLayout = false): HTMLDivElement {
    notesState.store.allPages = pages;
    notesState.store.folders = [folder];
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(NotesWorkspaceHeader, {
      target,
      props: {
        mobileLayout,
        selectedProject: project,
        selectedGroup: group,
        selectedProjectId: project.id,
        selectedPage,
        explorerCollapsed,
        creationFolderId: folder.id,
        showInactiveProjects: false,
        onShowInactiveProjectsChange: vi.fn(),
        onProjectSelected: vi.fn(),
        onShowHome: vi.fn(),
        projectSettingsOpen: false,
        onToggleProjectSettings: vi.fn(),
      },
    });
    mounted.push({ target, component });
    return target;
  }

  it("places one navigator chevron after the final visible breadcrumb segment", () => {
    const expanded = setup(false);
    expect(expanded.querySelectorAll("[data-notes-context-chevron]")).toHaveLength(1);
    expect(expanded.querySelector("[data-notes-context-chevron]")?.closest("button")?.textContent).toContain("Learning");

    const collapsed = setup(true);
    expect(collapsed.querySelectorAll("[data-notes-context-chevron]")).toHaveLength(1);
    expect(collapsed.querySelector("[data-notes-context-chevron]")?.closest("button")?.textContent).toContain("Untitled");
  });

  it("shows group, project, main note, and sub-note in order with the sidebar collapsed", () => {
    const parent = createProvisionalNotesPage({
      id: "main", title: "Main note", first_block_id: "main-body", folder_id: null,
      parent: { type: "workspace", workspace: true }, properties: { __ganbaru_project_id: project.id },
    }).page;
    const child = createProvisionalNotesPage({
      id: "child", title: "Sub-note", first_block_id: "child-body", folder_id: null,
      parent: { type: "page_id", page_id: parent.id }, properties: parent.properties,
    }).page;
    const header = setup(true, child, [parent, child]);
    const labels = Array.from(header.querySelectorAll("button")).map((button) => button.textContent?.trim());
    expect(labels.filter((label) => ["Routine", "Learning", "Main note", "Sub-note"].includes(label ?? "")))
      .toEqual(["Routine", "Learning", "Main note", "Sub-note"]);
  });

  it.each([false, true])("shows the entire database owner path with an expanded explorer and mobile layout %s", (mobileLayout) => {
    const loaded = createProvisionalNotesPage({
      id: "owner", title: "Reading", first_block_id: "database", folder_id: folder.id,
      parent: { type: "workspace", workspace: true }, properties: { __ganbaru_project_id: project.id },
    });
    const database = applyBlockUpdate(loaded.blocks.results[0], {
      type: "child_database", child_database: { title: "Book list", database_id: "database" },
    });
    if (database.type !== "child_database") throw new Error("Expected a database fixture");
    notesState.store.selectedDatabaseBlockId = database.id;
    notesState.store.selectedDatabaseBlock = database;
    const header = setup(false, loaded.page, [loaded.page], mobileLayout);
    const text = header.textContent ?? "";
    const labels = ["Routine", "Learning", "Lessons", "Reading", "Book list"];
    const indices = labels.map((label) => text.indexOf(label));
    expect(indices.every((index) => index >= 0)).toBe(true);
    expect(indices).toEqual([...indices].sort((first, second) => first - second));
    expect(header.querySelector("[aria-current='page']")?.textContent?.trim()).toBe("Book list");
    header.querySelector<HTMLButtonElement>('button[aria-label="Reading"]')!.click();
    expect(notesState.store.closeDatabase).toHaveBeenCalledOnce();
    expect(notesState.store.selectPage).not.toHaveBeenCalled();
  });

  it("updates the existing breadcrumb from an acknowledged database rename", () => {
    const loaded = createProvisionalNotesPage({
      id: "owner", title: "Reading", first_block_id: "database", folder_id: folder.id,
      parent: { type: "workspace", workspace: true }, properties: { __ganbaru_project_id: project.id },
    });
    const database = applyBlockUpdate(loaded.blocks.results[0], {
      type: "child_database", child_database: { title: "Book list", database_id: "database" },
    });
    if (database.type !== "child_database") throw new Error("Expected a database fixture");
    notesState.store.selectedDatabaseBlockId = database.id;
    notesState.store.selectedDatabaseBlock = database;
    const header = setup(false, loaded.page, [loaded.page]);
    const renamed = { ...database, child_database: { ...database.child_database, title: "Reading queue" } };
    flushSync(() => { notesState.store.selectedDatabaseBlock = renamed; });
    expect(header.querySelector("[data-notes-database-breadcrumb]")?.textContent?.trim()).toBe("Reading queue");
  });

  it("branches a note into its databases and preselects a view from the nested hover panels", async () => {
    const owner = createProvisionalNotesPage({
      id: page.id, title: "Reading", first_block_id: "body", folder_id: folder.id,
      parent: page.parent, properties: page.properties,
    }).page;
    const database: NotesNavigationDatabase = { id: "database", page_id: owner.id, title: "Book list", data_source_id: "source" };
    notesState.store.navigationDatabases = [database];
    databaseViews.mockResolvedValue([{
      object: "view", id: "board", parent: { type: "database_id", database_id: database.id }, data_source_id: database.data_source_id,
      name: "By category", type: "board", filter: {}, sorts: [], configuration: null, url: null,
      created_time: "2026-09-30T00:00:00Z", last_edited_time: "2026-09-30T00:00:00Z",
      source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null,
    }]);
    const header = setup(true, owner);
    await tick();
    header.querySelector<HTMLButtonElement>('button[aria-label="Reading"]')!.dispatchEvent(new Event("pointerenter"));
    await vi.waitFor(() => expect(header.querySelector('[role="dialog"] button[aria-label="Reading"]')).not.toBeNull());
    expect(header.querySelectorAll('[role="dialog"] input')).toHaveLength(1);
    const noteRows = header.querySelectorAll<HTMLButtonElement>('button[aria-label="Reading"]');
    noteRows[noteRows.length - 1].dispatchEvent(new Event("pointerenter"));
    await vi.waitFor(() => expect(header.querySelector('button[aria-label="Book list"]')).not.toBeNull());
    expect(header.querySelectorAll('[role="dialog"] input')).toHaveLength(1);
    header.querySelector<HTMLButtonElement>('button[aria-label="Book list"]')!.dispatchEvent(new Event("pointerenter"));
    await vi.waitFor(() => expect(header.querySelector('button[aria-label="By category"]')).not.toBeNull());
    expect(header.querySelectorAll('[role="dialog"] input')).toHaveLength(1);
    expect(header.querySelectorAll('.project-picker-panel.fixed input')).toHaveLength(0);
    header.querySelector<HTMLButtonElement>('button[aria-label="By category"]')!.click();
    await vi.waitFor(() => expect(notesState.store.openDatabase).toHaveBeenCalledWith(
      { pageId: owner.id, blockId: database.id }, { followSource: false, viewId: "board" },
    ));
    expect(header.querySelector("[data-notes-database-breadcrumb]")).toBeNull();
    await vi.waitFor(() => expect(header.querySelector("[role='dialog']")).toBeNull());
  });

  it("keeps search in the directly opened project panel while cascading into notes", async () => {
    const owner = createProvisionalNotesPage({
      id: page.id, title: "Reading", first_block_id: "body", folder_id: null,
      parent: page.parent, properties: page.properties,
    }).page;
    const header = setup(true, owner);
    await tick();
    const projectTrigger = Array.from(header.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent?.trim() === "Learning");
    expect(projectTrigger).toBeDefined();
    projectTrigger!.dispatchEvent(new Event("pointerenter"));
    await vi.waitFor(() => expect(header.querySelector('[role="dialog"] input')).not.toBeNull());
    const projectRow = Array.from(header.querySelectorAll<HTMLButtonElement>('[role="dialog"] button'))
      .find((button) => button.textContent?.trim() === "Learning");
    expect(projectRow).toBeDefined();
    projectRow!.focus();
    await vi.waitFor(() => expect(header.querySelector('[role="dialog"] button[aria-label="Reading"]')).not.toBeNull());
    expect(header.querySelectorAll('[role="dialog"] input')).toHaveLength(1);
    expect(header.querySelector('.project-picker-panel.fixed input')).toBeNull();
  });

  it("opens saved views when hovering the database breadcrumb and has no creation button", async () => {
    const loaded = createProvisionalNotesPage({
      id: "owner", title: "Reading", first_block_id: "database", folder_id: folder.id,
      parent: { type: "workspace", workspace: true }, properties: { __ganbaru_project_id: project.id },
    });
    const database = applyBlockUpdate(loaded.blocks.results[0], {
      type: "child_database", child_database: { title: "Book list", database_id: "database", data_source_id: "source" },
    });
    if (database.type !== "child_database") throw new Error("Expected a database fixture");
    notesState.store.selectedDatabaseBlockId = database.id;
    notesState.store.selectedDatabaseBlock = database;
    notesState.store.navigationDatabases = [{ id: database.id, page_id: loaded.page.id, title: "Book list", data_source_id: "source" }];
    databaseViews.mockResolvedValue([{
      object: "view", id: "list", parent: { type: "database_id", database_id: database.id }, data_source_id: "source",
      name: "Reading queue", type: "list", filter: {}, sorts: [], configuration: null, url: null,
      created_time: "2026-09-30T00:00:00Z", last_edited_time: "2026-09-30T00:00:00Z",
      source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null,
    }]);
    const header = setup(false, loaded.page, [loaded.page]);
    await tick();
    header.querySelector<HTMLButtonElement>("[data-notes-database-breadcrumb]")!.dispatchEvent(new Event("pointerenter"));
    await vi.waitFor(() => expect(databaseViews).toHaveBeenCalledWith("database"));
    await vi.waitFor(() => expect(header.querySelector('button[aria-label="Reading queue"]')).not.toBeNull());
    expect(header.querySelectorAll('[role="dialog"] input')).toHaveLength(1);
    expect(header.querySelectorAll("[data-notes-context-chevron]")).toHaveLength(1);
    expect(header.querySelector('[data-workspace-breadcrumb-terminal-icon="plus"]')).toBeNull();
    expect(header.querySelector('button[aria-keyshortcuts="Control+N Meta+N"]')).toBeNull();
    expect(header.querySelector('button[aria-label="New folder"]')).toBeNull();
    expect(header.querySelector("[data-notes-database-breadcrumb]")?.getAttribute("aria-expanded")).toBe("true");
    header.querySelector<HTMLButtonElement>('button[aria-label="Reading queue"]')!.click();
    await vi.waitFor(() => expect(notesState.store.openDatabase).toHaveBeenCalledWith(
      { pageId: loaded.page.id, blockId: database.id }, { followSource: false, viewId: "list" },
    ));
    await vi.waitFor(() => expect(header.querySelector("[role='dialog']")).toBeNull());
    expect(header.querySelector("[data-notes-database-breadcrumb]")?.textContent?.trim()).toBe("Book list");
    expect(header.querySelector("[data-notes-workspace-header]")?.textContent).not.toContain("Reading queue");
  });
});
