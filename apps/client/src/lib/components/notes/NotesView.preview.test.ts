// @vitest-environment jsdom
import { createRawSnippet, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { applyBlockUpdate, blockPlainText, createBlockUpdate } from "$lib/notes/blocks/factory";
import { notesBlockOutlineFromBlock } from "$lib/notes/blocks/outline";
import { createProvisionalNotesPage } from "$lib/notes/pages/creation";
import { parseNotesLinkHash } from "$lib/notes/links/block-link";
import type { NotesBlockHydrationRequest, NotesBlockUpdate, NotesChildPageFromBlockCreate, NotesDataSourceTableView, NotesDatabaseCreateRequest, NotesDatabaseReference, NotesLocalUser, NotesPageCreate, NotesPageOpenResponse, NotesWorkspaceShell, NotesWorkspaceShellRequest } from "$lib/notes/types";
import type { NotesPageOpenMode } from "$lib/notes/pages/open-mode";
import type { NotesCompoundEdit, NotesCompoundEditResult } from "$lib/api/notes/compound-edits";
import type { NotesCreatedDatabase } from "$lib/notes/types";

const backend = vi.hoisted(() => ({
  pages: new Map<string, NotesPageOpenResponse>(),
  open: vi.fn(), createChild: vi.fn(), createPage: vi.fn(), saveBlock: vi.fn(),
  createDatabase: vi.fn<(request: NotesDatabaseCreateRequest) => Promise<NotesCreatedDatabase>>(), databaseViews: vi.fn(), databaseTable: vi.fn(),
  databaseReference: vi.fn(async (_blockId: string): Promise<NotesDatabaseReference> => { throw new Error("Missing database fixture"); }),
  renameDatabase: vi.fn(async (_databaseId: string, title: string) => title),
  mentionSources: vi.fn(async () => []),
  destinations: vi.fn(async () => ({ pages: [], next_page_cursor: null })),
  workspaceRequests: [] as NotesWorkspaceShellRequest[],
  otherProjectCursor: null as string | null,
  projectsLoaded: true,
  projectsReady: vi.fn(async (): Promise<void> => undefined),
}));

vi.mock("$lib/api/notes/compound-edits", () => ({
  applyNotesCompoundEdit: async (request: NotesCompoundEdit): Promise<NotesCompoundEditResult> => {
    const before = Object.keys(request.expected_blocks).map((id) => {
      const block = [...backend.pages.values()].flatMap((page) => page.blocks.results).find((block) => block.id === id);
      if (!block) throw new Error("Missing canonical block fixture");
      return block;
    });
    const blocks: NotesCompoundEditResult["blocks"] = [];
    const databases: NotesCreatedDatabase[] = [];
    for (const operation of request.operations) {
      if (operation.type === "create_database") {
        const created = await backend.createDatabase(operation.request);
        databases.push(created);
        blocks.push(created.block);
      } else if (operation.type === "update") {
        blocks.push(await backend.saveBlock(operation.block_id, operation.update));
      } else {
        throw new Error(`Unsupported preview fixture operation ${operation.type}`);
      }
    }
    return { operation_id: request.operation_id, page_id: request.page_id, blocks, databases, placements: [], before_blocks: before, before_placements: [] };
  },
}));

vi.mock("$lib/api/notes", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/notes")>(),
  openNotesPage: backend.open,
  createNotesChildPageFromBlock: backend.createChild,
  createNotesPage: backend.createPage,
  updateNotesBlock: backend.saveBlock,
  createNotesDatabase: backend.createDatabase,
  renameNotesDatabase: backend.renameDatabase,
  listNotesDatabaseViews: backend.databaseViews,
  getNotesDataSourceTableView: backend.databaseTable,
  getNotesDatabaseReference: backend.databaseReference,
  listNotesDataSourceTemplates: async () => [],
  loadNotesWorkspaceShell: async (request: NotesWorkspaceShellRequest): Promise<NotesWorkspaceShell> => {
    backend.workspaceRequests.push(request);
    const allPages = [...backend.pages.values()].map(({ page }) => page);
    const pages = allPages.filter((page) => page.properties.__ganbaru_project_id === request.project_id);
    return {
      pages, folders: [], navigation_pages: request.include_navigation_index ? allPages : [],
      navigation_folders: [], navigation_page_ids_with_children: [],
      navigation_databases: [],
      page_ids_with_children: [], missing_parent_page_ids: [], trashed_parent_page_ids: [],
      resolved_selected_page_id: null, total_page_count: pages.length, total_folder_count: 0,
      next_page_cursor: request.project_id === "other-project" && !request.page_cursor ? backend.otherProjectCursor : null,
      next_folder_cursor: null,
    };
  },
  getNotesPageBreadcrumb: async (id: string) => backend.pages.get(id)!.breadcrumb,
  hydrateNotesBlocks: async (request: NotesBlockHydrationRequest) => backend.pages.get(request.page_id)!.blocks.results.filter((block) => request.block_ids.includes(block.id)),
  listNotesBacklinks: async () => [], listNotesPageAliases: async () => [], listNotesUnresolvedLinks: async () => [],
  listNotesComments: async () => [], listNotesSuggestions: async () => [], listNotesDataSources: backend.mentionSources,
  getNotesLocalUser: async (): Promise<NotesLocalUser> => ({
    object: "user", id: "local", display_name: "Local",
    created_time: "2026-09-28T12:00:00Z", last_edited_time: "2026-09-28T12:00:00Z",
  }),
  saveNotesUndoState: async () => undefined,
  clearNotesUndoState: async () => undefined,
  loadNotesUndoState: async () => null,
  listNotesWorkingMarkdown: async () => ({ roots: [], unavailableWorkingFolderIds: [] }),
  listNotesDestinationCandidates: backend.destinations,
}));
vi.mock("$lib/stores/projects.svelte", async () => {
  const { SvelteMap } = await import("svelte/reactivity");
  const group = { id: "group", name: "Group", icon: "folder" };
  const otherGroup = { id: "other-group", name: "Other group", icon: "folder" };
  const project = { id: "project", groupId: group.id, name: "Project", icon: "folder", status: "active" };
  const otherProject = { id: "other-project", groupId: otherGroup.id, name: "Other project", icon: "folder", status: "active" };
  const selection = new SvelteMap([["projectId", project.id]]);
  const projects = [project, otherProject];
  const groups = [group, otherGroup];
  const store = {
    get loaded(): boolean { return backend.projectsLoaded; },
    get selectedProjectId(): string | null { return selection.get("projectId") ?? null; },
    set selectedProjectId(id: string | null) { if (id) selection.set("projectId", id); else selection.delete("projectId"); },
    get selectedProject(): typeof project | undefined { return projects.find((project) => project.id === store.selectedProjectId); },
    get selectedGroup(): typeof group | undefined { return groups.find((group) => group.id === store.selectedProject?.groupId); },
    projects, tasks: [], customEmojis: [], ensureLoaded: backend.projectsReady,
    projectById: (id: string) => projects.find((project) => project.id === id),
    groupById: (id: string) => groups.find((group) => group.id === id),
    visibleGroups: () => groups,
    projectsForGroup: (id: string) => projects.filter((project) => project.groupId === id),
    projectsForGroupIncludingInactive: (id: string) => projects.filter((project) => project.groupId === id),
    isProjectStarred: () => false,
  };
  return { getProjects: () => store, useProjects: () => store };
});
vi.mock("$lib/stores/calendar.svelte", () => ({ getCalendar: () => ({ loaded: true, sourceEvents: [] }) }));
vi.mock("$lib/stores/pomodoro.svelte", () => ({ getPomodoro: () => ({ activeRunId: null, formattedTime: "00:00" }) }));
vi.mock("$lib/vault/config", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/vault/config")>(), setConfigKey: vi.fn(),
}));
vi.mock("$lib/stores/viewport.svelte", () => ({ getViewport: () => ({ width: 1200, height: 900 }) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: "main" }) }));

/** Build the same page, initial body, and outline contract returned by native page loading. */
function page(id: string, title: string, parentId?: string, projectId = "project"): NotesPageOpenResponse {
  const loaded = createProvisionalNotesPage({
    id, title, first_block_id: `${id}-body`, folder_id: null,
    parent: parentId ? { type: "page_id", page_id: parentId } : { type: "workspace", workspace: true },
    properties: { __ganbaru_project_id: projectId },
  });
  loaded.blocks.results[0] = { ...applyBlockUpdate(loaded.blocks.results[0], createBlockUpdate("paragraph", `${title} content`)), edit_revision: "1".padStart(64, "0") };
  return {
    ...loaded,
    outlines: loaded.blocks.results.map((block, index) => notesBlockOutlineFromBlock(block, id, index)),
    breadcrumb: [
      ...(parentId ? [{ id: parentId, title: "Main note", current: false, status: "active" as const }] : []),
      { id, title, current: true, status: "active" },
    ],
  };
}

/** Return the initial empty table using the identities reserved by database creation. */
function emptyDatabaseTable(request: NotesDatabaseCreateRequest): NotesDataSourceTableView {
  const source = {
    source_provider: null, source_object_id: null, source_workspace_id: null,
    source_last_edited_time: null, created_time: "2026-09-30T00:00:00Z", last_edited_time: "2026-09-30T00:00:00Z",
  };
  return {
    data_source: {
      ...source, object: "data_source", id: request.data_source_id,
      parent: { type: "database_id", database_id: request.id },
      database_parent: { type: "page_id", page_id: "parent" }, title: request.title,
      title_rich_text: [], description: [], icon: null, in_trash: false,
      properties: { Name: { id: "title", name: "Name", type: "title", title: {} } },
    },
    view: {
      ...source, object: "view", id: request.view_id,
      parent: { type: "database_id", database_id: request.id }, data_source_id: request.data_source_id,
      name: "Table", type: "table", filter: null, sorts: [], url: null,
      configuration: { type: "table", table: {
        property_order: ["title"], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page",
      } },
    },
    rows: [], total_row_count: 0, has_more: false, next_cursor: null,
  };
}

/** Give a note an existing database without running a creation mutation. */
function databaseInPage(owner: NotesPageOpenResponse, title = "Planning") {
  const blockId = owner.blocks.results[0].id;
  const request: NotesDatabaseCreateRequest = {
    id: crypto.randomUUID(), data_source_id: crypto.randomUUID(), view_id: crypto.randomUUID(),
    parent: { type: "page_id", page_id: owner.page.id }, title,
  };
  const table = emptyDatabaseTable(request);
  const block = applyBlockUpdate(owner.blocks.results[0], {
    type: "child_database", child_database: {
      title, database_id: request.id, data_source_id: request.data_source_id, view_id: request.view_id,
    },
  });
  if (block.type !== "child_database") throw new Error("Expected a database fixture");
  owner.blocks.results[0] = block;
  owner.outlines = owner.blocks.results.map((item, index) => notesBlockOutlineFromBlock(item, owner.page.id, index));
  backend.databaseViews.mockResolvedValue([table.view]);
  backend.databaseTable.mockResolvedValue(table);
  return { blockId, block, table };
}

describe("Notes preview ownership", () => {
  let component: ReturnType<typeof mount> | undefined;
  afterEach(async () => {
    if (component) await unmount(component);
    component = undefined;
    const { getNotes } = await import("$lib/stores/notes.svelte");
    await getNotes().selectPage(null);
    document.body.replaceChildren();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
    backend.pages.clear();
    backend.workspaceRequests.length = 0;
    backend.otherProjectCursor = null;
    backend.projectsLoaded = true;
    backend.projectsReady.mockResolvedValue(undefined);
    backend.databaseReference.mockReset();
    window.history.replaceState(null, "", window.location.pathname);
  });

  async function setup(openMode: NotesPageOpenMode = "full", emptyChildWithCover = false, parentBodyId?: string, parentPageId = "parent") {
    const { getProjects } = await import("$lib/stores/projects.svelte");
    getProjects().selectedProjectId = "project";
    vi.stubGlobal("CSS", { escape: (value: string) => value });
    vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => window.setTimeout(() => callback(0), 0));
    vi.stubGlobal("cancelAnimationFrame", (id: number) => window.clearTimeout(id));
    const parent = page(parentPageId, "Main note");
    if (parentBodyId) parent.blocks.results[0] = { ...parent.blocks.results[0], id: parentBodyId };
    const child = page("child", "Sub-note", parent.page.id);
    if (emptyChildWithCover) {
      child.blocks.results[0] = applyBlockUpdate(child.blocks.results[0], createBlockUpdate("paragraph", ""));
      child.page.cover = { type: "design", design: { pattern: "solid", color: "default" } };
    }
    const sibling = page("sibling", "Second sub-note", parent.page.id);
    const other = page("other-note", "Other project note", undefined, "other-project");
    parent.blocks.results.push(...[child, sibling].map(({ page: childPage }) => applyBlockUpdate(
      { ...parent.blocks.results[0], id: childPage.id }, createBlockUpdate("child_page", childPage.id === child.page.id ? "Sub-note" : "Second sub-note"),
    )));
    parent.outlines = parent.blocks.results.map((block, index) => notesBlockOutlineFromBlock(block, parent.page.id, index));
    backend.pages.set(parent.page.id, parent);
    backend.pages.set(child.page.id, child);
    backend.pages.set(sibling.page.id, sibling);
    backend.pages.set(other.page.id, other);
    backend.open.mockImplementation(async (id: string) => backend.pages.get(id)!);
    backend.createPage.mockImplementation(async (request: NotesPageCreate) => {
      const parentId = request.parent.type === "page_id" ? request.parent.page_id : undefined;
      const created = page(request.id, request.title, parentId);
      backend.pages.set(created.page.id, created);
      const owner = parentId ? backend.pages.get(parentId) : undefined;
      if (owner) owner.blocks.results.push(applyBlockUpdate(
        { ...owner.blocks.results[0], id: created.page.id }, createBlockUpdate("child_page", request.title),
      ));
      return created;
    });
    backend.saveBlock.mockImplementation(async (id: string, update: NotesBlockUpdate) => {
      const owner = [...backend.pages.values()].find((page) => page.blocks.results.some((block) => block.id === id));
      if (!owner) throw new Error("Missing block");
      const index = owner.blocks.results.findIndex((block) => block.id === id);
      const next = { ...applyBlockUpdate(owner.blocks.results[index], update), edit_revision: "2".padStart(64, "0") };
      owner.blocks.results[index] = next;
      return next;
    });
    backend.databaseReference.mockImplementation(async (blockId: string) => {
      const owner = [...backend.pages.values()].find((loaded) => loaded.blocks.results.some((block) => block.id === blockId));
      const block = owner?.blocks.results.find((candidate) => candidate.id === blockId);
      if (!owner || block?.type !== "child_database" || block.in_trash || block.archived) throw new Error("Missing database fixture");
      return {
        block_id: blockId, page_id: owner.page.id, title: block.child_database.title,
        source_block_id: blockId, source_page_id: owner.page.id,
        is_linked: false, owned_data_source_count: 1, editing_locked: false,
      };
    });
    const { getNotes } = await import("$lib/stores/notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.selectPage(parent.page.id, { openMode });
    notes.explorerCollapsed = true;
    const { default: NotesView } = await import("./NotesView.svelte");
    component = mount(NotesView, { target: document.body });
    await tick(); await tick();
    return { notes, parent, child, sibling, other };
  }

  it("keeps one database placeholder across earlier saves, creation, and initial rows, then focuses an empty title", async () => {
    const { notes, parent } = await setup("full", false, crypto.randomUUID());
    const blockId = parent.blocks.results[0].id;
    let finishSave!: () => void;
    let finishCreation!: () => void;
    let finishRows!: () => void;
    const saving = new Promise<void>((resolve) => { finishSave = resolve; });
    const creating = new Promise<void>((resolve) => { finishCreation = resolve; });
    const rows = new Promise<void>((resolve) => { finishRows = resolve; });
    const saveBlock = backend.saveBlock.getMockImplementation()!;
    backend.saveBlock.mockImplementation(async (id: string, update: NotesBlockUpdate) => {
      await saving;
      return saveBlock(id, update);
    });
    backend.createDatabase.mockImplementation(async (request: NotesDatabaseCreateRequest) => {
      const table = emptyDatabaseTable(request);
      backend.databaseViews.mockResolvedValue([table.view]);
      backend.databaseTable.mockImplementation(async () => { await rows; return table; });
      await creating;
      const index = parent.blocks.results.findIndex((block) => block.id === blockId);
      const block = { ...applyBlockUpdate(parent.blocks.results[index], {
        type: "child_database",
        child_database: { title: request.title, database_id: request.id, data_source_id: request.data_source_id, view_id: request.view_id },
      }), edit_revision: "2".padStart(64, "0") };
      if (block.type !== "child_database") throw new Error("Expected a created database fixture");
      parent.blocks.results[index] = block;
      return {
        block, data_source: table.data_source, view: table.view,
        database: {
          object: "database", id: request.id, parent: block.parent, title: request.title,
          title_rich_text: [], description: [], icon: null, cover: null, in_trash: false,
          is_inline: true, data_sources: [], url: null, public_url: null,
          source_provider: null, source_object_id: null, source_workspace_id: null,
          source_last_edited_time: null, created_time: table.data_source.created_time,
          last_edited_time: table.data_source.last_edited_time,
        },
      };
    });
    await notes.updateBlockText(blockId, "/datab");
    const creation = notes.convertBlock(blockId, "child_database", true);
    try {
      await tick(); await tick();
      const skeleton = document.querySelector('[data-notes-skeleton="database"]')!;
      expect(skeleton).not.toBeNull();
      const shapes = skeleton.querySelector<HTMLElement>(".notes-skeleton-shapes")!;
      expect(backend.createDatabase).not.toHaveBeenCalled();
      expect(backend.databaseViews).not.toHaveBeenCalled();
      await vi.waitFor(() => expect(shapes.style.opacity).toBe("1"));
      finishSave();
      await vi.waitFor(() => expect(backend.createDatabase).toHaveBeenCalledOnce());
      expect(backend.databaseViews).not.toHaveBeenCalled();
      expect(document.querySelector('[data-notes-skeleton="database"]')).toBe(skeleton);
      finishCreation();
      await creation;
      await vi.waitFor(() => expect(backend.databaseTable).toHaveBeenCalled(), { timeout: 5_000 });
      expect(document.querySelector('[data-notes-skeleton="database"]')).toBe(skeleton);
      expect(skeleton.contains(shapes)).toBe(true);
      expect(shapes.style.opacity).toBe("1");
      finishRows();
      await vi.waitFor(() => expect(document.querySelector('[data-notes-skeleton="database"]')).toBeNull());
      const title = document.querySelector<HTMLInputElement>('input[aria-label="Database title"]')!;
      expect(title.value).toBe("");
      expect(title.placeholder).toBe("Database name");
      await vi.waitFor(() => expect(document.activeElement).toBe(title));
      const down = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
      title.dispatchEvent(down);
      expect(down.defaultPrevented).toBe(true);
      expect(notes.focusBlockId).toBe(parent.blocks.results[1].id);
      await tick();
      const childButton = document.querySelector<HTMLButtonElement>(`[data-notes-atomic-block='${parent.blocks.results[1].id}']`)!;
      childButton.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true, cancelable: true }));
      await vi.waitFor(() => expect(document.activeElement).toBe(title));
      const updateCount = backend.saveBlock.mock.calls.length;
      for (const name of ["Planning", ""]) {
        title.value = name;
        title.dispatchEvent(new Event("input", { bubbles: true }));
        title.blur();
        await vi.waitFor(() => expect(blockPlainText(notes.blockById(blockId)!)).toBe(name));
        expect(backend.renameDatabase).toHaveBeenLastCalledWith(blockId, name);
        expect(backend.saveBlock).toHaveBeenCalledTimes(updateCount);
        title.focus();
        await tick();
      }
      backend.renameDatabase.mockRejectedValueOnce(new Error("Rename failed"));
      title.value = "Unsaved title";
      title.dispatchEvent(new Event("input", { bubbles: true }));
      title.blur();
      await vi.waitFor(() => expect(document.body.textContent).toContain("Rename failed"));
      expect(blockPlainText(notes.blockById(blockId)!)).toBe("");
      expect(title.value).toBe("");
    } finally {
      finishSave(); finishCreation(); finishRows();
      await creation;
    }
  }, 15_000);

  it("updates the header and sidebar when the top-bar selector opens a note from another group", async () => {
    const { notes, other } = await setup();
    notes.explorerCollapsed = false;
    await tick();
    const namedButton = (name: string) => [...document.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.trim() === name);
    namedButton("Group")!.click();
    await vi.waitFor(() => expect(namedButton("Other group")).toBeDefined());
    namedButton("Other group")!.click();
    await vi.waitFor(() => expect(namedButton("Other project")).toBeDefined());
    namedButton("Other project")!.focus();
    await vi.waitFor(() => expect(namedButton("Other project note")).toBeDefined());
    namedButton("Other project note")!.click();
    await vi.waitFor(() => expect(notes.loadedPage?.id).toBe(other.page.id));
    const { getProjects } = await import("$lib/stores/projects.svelte");
    await vi.waitFor(() => expect(getProjects().selectedProjectId).toBe("other-project"));
    await tick();
    const header = document.querySelector("[data-notes-workspace-header]")!.textContent!;
    expect(header).toContain("Other group");
    expect(header).toContain("Other project");
    const explorer = document.querySelector("[data-notes-explorer]")!.textContent!;
    expect(explorer).toContain("Other project note");
    expect(explorer).not.toContain("Main note");
    expect(backend.workspaceRequests.at(-1)).toMatchObject({
      project_id: "other-project", selected_page_id: other.page.id, include_navigation_index: false,
    });
    expect(backend.open.mock.calls.filter(([id]) => id === other.page.id)).toHaveLength(1);
  }, 15_000);

  it("shows floating paste choices beside an atomic database row and keeps the copy when dismissed", async () => {
    const { notes, child } = await setup();
    const { blockId } = databaseInPage(child);
    await notes.selectPage(child.page.id);
    await vi.waitFor(() => expect(document.querySelector('input[aria-label="Database title"]')).not.toBeNull());
    notes.databasePaste.beginCopies({ [blockId]: crypto.randomUUID() }, false);
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"][aria-label="Paste as"]')).not.toBeNull());
    const panel = document.querySelector('[role="dialog"][aria-label="Paste as"]')!;
    expect(panel.closest(".notes-block-list")).toBeNull();
    const dismiss = [...panel.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Dismiss")!;
    dismiss.click();
    await tick();
    expect(document.querySelector('[role="dialog"][aria-label="Paste as"]')).toBeNull();
    expect(notes.blockById(blockId)?.type).toBe("child_database");
    expect(document.querySelector(`[data-notes-selectable-block-id="${blockId}"] input[aria-label="Database title"]`)).not.toBeNull();
  }, 15_000);

  it.each(["center", "side"] as const)("keeps a database collection menu inside its %s preview when another pane opens a database", async (mode) => {
    const { notes, parent, child } = await setup();
    const { blockId: parentDatabaseId } = databaseInPage(parent);
    await notes.selectPageLocally(null);
    await notes.selectPage(parent.page.id);
    await vi.waitFor(() => expect(backend.databaseTable).toHaveBeenCalled(), { timeout: 5_000 });
    const { blockId: childDatabaseId } = databaseInPage(child);
    await notes.selectPage(child.page.id, { openMode: mode });
    await vi.waitFor(() => expect(document.querySelector('[data-notes-page-peek] input[aria-label="Database title"]')).not.toBeNull());
    const peek = document.querySelector<HTMLElement>("[data-notes-page-peek]")!;
    const owner = peek.closest<HTMLElement>("[data-notes-pane]")!;
    const title = peek.querySelector<HTMLInputElement>('input[aria-label="Database title"]')!;
    await vi.waitFor(() => expect(document.activeElement).toBe(title));
    const trigger = peek.querySelector<HTMLButtonElement>(`[data-notes-selectable-block-id="${childDatabaseId}"] button[aria-label="More"]`)!;
    trigger.click();
    await vi.waitFor(() => expect(owner.querySelector('[data-app-floating-surface][aria-label="More"]')).not.toBeNull());
    const menu = owner.querySelector<HTMLElement>('[data-app-floating-surface][aria-label="More"]')!;
    await vi.waitFor(() => expect(menu.contains(document.activeElement)).toBe(true));
    expect(menu.parentElement).toBe(owner);
    expect(menu.closest("[data-notes-pane]")).toBe(owner);
    expect(owner.inert).toBe(false);

    const main = notes.editorPanes.find((pane) => pane.id === notes.mainPaneId)!;
    expect(await main.store.openDatabase(parentDatabaseId)).toBe(true);
    await vi.waitFor(() => {
      expect(owner.classList.contains("hidden")).toBe(true);
      expect(owner.inert).toBe(true);
    });
    const remainingMenus = [...document.querySelectorAll<HTMLElement>('[data-app-floating-surface][aria-label="More"]')];
    expect(remainingMenus.filter((surface) => !owner.contains(surface))).toEqual([]);
    if (menu.isConnected) {
      expect(menu.closest("[data-notes-pane]")).toBe(owner);
      expect(owner.contains(menu)).toBe(true);
    } else {
      expect(remainingMenus).toEqual([]);
    }
  }, 15_000);

  it.each(["center", "side"] as const)("retains both editor panes and cached rows while a %s preview opens its database fullwidth", async (mode) => {
    const { notes, parent, child } = await setup();
    const { blockId } = databaseInPage(child);
    await notes.selectPage(child.page.id, { openMode: mode });
    await vi.waitFor(() => expect(backend.databaseTable).toHaveBeenCalled(), { timeout: 5_000 });
    await tick(); await tick();
    const mainViewport = document.querySelector("[data-notes-main-page] [data-notes-editor-scroll]");
    const previewViewport = document.querySelector("[data-notes-page-peek] [data-notes-editor-scroll]");
    const paneIds = notes.editorPanes.map((pane) => pane.id);
    const rowReads = backend.databaseTable.mock.calls.length;
    const preview = notes.previewPane;
    const noteFocusBlockId = notes.focusBlockId;
    const noteFocusRequestId = notes.focusRequestId;
    expect(await notes.openDatabase(blockId)).toBe(true);
    await vi.waitFor(() => expect(document.querySelector("[data-notes-database-page] input[aria-label='Database title']")).not.toBeNull());
    expect(notes.editorPanes.map((pane) => pane.id)).toEqual(paneIds);
    expect(notes.previewPane).toBe(preview);
    expect(notes.pageOpenMode).toBe(mode);
    for (const element of document.querySelectorAll<HTMLElement>("[data-notes-pane]")) {
      const hidden = element.dataset.notesPane !== notes.activePaneId;
      expect(element.classList.contains("hidden")).toBe(hidden);
      expect(element.inert).toBe(hidden);
    }
    expect(document.querySelector("[data-notes-database-page]")?.closest("[data-notes-page-peek]")).toBeNull();
    expect(document.querySelector("[data-notes-main-page] [data-notes-editor-scroll]")).toBe(mainViewport);
    expect(document.querySelector(`[data-notes-pane='${notes.activePaneId}'] [data-notes-editor-scroll]`)).toBe(previewViewport);
    expect(backend.databaseTable).toHaveBeenCalledTimes(rowReads);
    expect(notes.focusBlockId).toBeNull();
    const title = document.querySelector<HTMLInputElement>("[data-notes-database-page] input[aria-label='Database title']")!;
    title.focus();
    expect(document.activeElement).toBe(title);
    expect(notes.focusRequestId).toBe(noteFocusRequestId);
    title.value = "Planning queue";
    title.dispatchEvent(new Event("input", { bubbles: true }));
    title.blur();
    await vi.waitFor(() => expect(document.querySelector("[data-notes-database-breadcrumb]")?.textContent).toContain("Planning queue"));
    const breadcrumb = document.querySelector<HTMLButtonElement>('[data-notes-workspace-header] button[aria-label="Sub-note"]')!;
    breadcrumb.click();
    await tick(); await tick();
    expect(notes.selectedDatabaseBlock).toBeNull();
    expect(notes.focusBlockId).toBe(noteFocusBlockId);
    expect(document.querySelector("[data-notes-database-page]")).toBeNull();
    expect(notes.previewPane).toBe(preview);
    expect(notes.pageOpenMode).toBe(mode);
    expect(document.querySelector("[data-notes-page-peek] [data-notes-editor-scroll]")).toBe(previewViewport);
    expect(backend.open.mock.calls.filter(([id]) => id === parent.page.id)).toHaveLength(1);
    expect(backend.open.mock.calls.filter(([id]) => id === child.page.id)).toHaveLength(1);
  }, 15_000);

  it("acknowledges a pending database rename after returning to the retained note", async () => {
    const { notes, parent } = await setup();
    const { blockId } = databaseInPage(parent);
    await notes.selectPageLocally(null);
    await notes.selectPage(parent.page.id);
    expect(await notes.openDatabase(blockId)).toBe(true);
    await vi.waitFor(() => expect(document.querySelector("[data-notes-database-page] input[aria-label='Database title']")).not.toBeNull());
    let finishRename!: (title: string) => void;
    backend.renameDatabase.mockImplementationOnce(() => new Promise<string>((resolve) => { finishRename = resolve; }));
    const title = document.querySelector<HTMLInputElement>("[data-notes-database-page] input[aria-label='Database title']")!;
    title.focus();
    title.value = "Planning queue";
    title.dispatchEvent(new Event("input", { bubbles: true }));
    title.blur();
    await vi.waitFor(() => expect(backend.renameDatabase).toHaveBeenCalled());
    notes.closeDatabase();
    await tick(); await tick();
    expect(document.querySelector("[data-notes-database-page]")).toBeNull();
    finishRename("Planning queue");
    await vi.waitFor(() => expect(blockPlainText(notes.blockById(blockId)!)).toBe("Planning queue"));
    const retainedTitle = document.querySelector<HTMLInputElement>("[data-notes-editor-container] input[aria-label='Database title']");
    await vi.waitFor(() => expect(retainedTitle?.value).toBe("Planning queue"));
  }, 15_000);

  it("retains the dedicated database behind a row peek and restores it when the peek closes", async () => {
    const { notes, parent, child } = await setup();
    const { blockId } = databaseInPage(parent);
    await notes.selectPageLocally(null);
    await notes.selectPage(parent.page.id);
    expect(await notes.openDatabase(blockId)).toBe(true);
    await vi.waitFor(() => expect(document.querySelector("[data-notes-database-page] input[aria-label='Database title']")).not.toBeNull());
    const databaseSurface = document.querySelector("[data-notes-database-page]");
    await notes.openPageContextually(child.page.id);
    await tick(); await tick();
    expect(document.querySelector("[data-notes-main-page] [data-notes-database-page]")).toBe(databaseSurface);
    expect(document.querySelector("[data-notes-page-peek] [data-notes-editor-scroll]")).not.toBeNull();
    await notes.closeContextualPage();
    await tick(); await tick();
    expect(notes.selectedDatabaseBlock?.id).toBe(blockId);
    expect(document.querySelector("[data-notes-database-page]")).toBe(databaseSurface);
    expect(document.querySelector("[data-notes-page-peek]")).toBeNull();
  }, 15_000);

  it("opens a database hash as a dedicated surface and returns to the note without a second page read", async () => {
    const { notes, parent } = await setup("full", false, crypto.randomUUID(), crypto.randomUUID());
    await notes.selectPage(null);
    const { blockId } = databaseInPage(parent);
    const hash = `#notes?page=${parent.page.id}&block=${blockId}`;
    window.history.replaceState(null, "", hash);
    const target = parseNotesLinkHash(hash);
    if (!target) throw new Error("Expected a Notes database link");
    const pageReads = backend.open.mock.calls.length;
    expect(await notes.openNotesLink(target)).toBe(true);
    await vi.waitFor(() => expect(document.querySelector("[data-notes-database-page] input[aria-label='Database title']")).not.toBeNull());
    expect(document.querySelector("[data-notes-editor-scroll]")).toBeNull();
    expect(backend.open).toHaveBeenCalledTimes(pageReads + 1);
    const header = document.querySelector("[data-notes-workspace-header]")!;
    expect(header.textContent).toContain("Planning");
    header.querySelector<HTMLButtonElement>('button[aria-label="Main note"]')!.click();
    await vi.waitFor(() => expect(document.querySelector("[data-notes-editor-scroll]")).not.toBeNull());
    expect(backend.open).toHaveBeenCalledTimes(pageReads + 1);
    expect(parseNotesLinkHash(window.location.hash)).toEqual({ pageId: parent.page.id });
  }, 15_000);

  it("settles pending owner writes before database navigation and retains the draft if saving fails", async () => {
    const { notes, parent } = await setup();
    const draft = { ...parent.blocks.results[0], id: crypto.randomUUID() };
    parent.blocks.results.push(draft);
    const { blockId } = databaseInPage(parent);
    await notes.selectPageLocally(null);
    await notes.selectPage(parent.page.id);
    const draftId = draft.id;
    await notes.updateBlockText(draftId, "Unsaved body");
    let finishSave!: () => void;
    const saving = new Promise<void>((resolve) => { finishSave = resolve; });
    const saveBlock = backend.saveBlock.getMockImplementation()!;
    backend.saveBlock.mockImplementation(async (id: string, update: NotesBlockUpdate) => {
      await saving;
      return saveBlock(id, update);
    });
    const opening = notes.openDatabase(blockId);
    await vi.waitFor(() => expect(backend.saveBlock).toHaveBeenCalled());
    expect(notes.selectedDatabaseBlockId).toBeNull();
    finishSave();
    expect(await opening).toBe(true);
    notes.closeDatabase();
    await notes.updateBlockText(draftId, "Retained body");
    backend.saveBlock.mockRejectedValueOnce(new Error("Save failed"));
    expect(await notes.openDatabase(blockId)).toBe(false);
    expect(notes.selectedDatabaseBlockId).toBeNull();
    expect(blockPlainText(notes.blockById(draftId)!)).toBe("Retained body");
    expect(notes.editorSaveError).toBe("Save failed");
    backend.saveBlock.mockImplementation(saveBlock);
    await notes.retryEditorMutations();
  });

  it("ignores late database metadata after the owner note has changed", async () => {
    const { notes, parent, child } = await setup();
    const { blockId } = databaseInPage(parent);
    const reference = await backend.databaseReference(blockId);
    let finishReference!: (value: NotesDatabaseReference) => void;
    backend.databaseReference.mockImplementationOnce(() => new Promise<NotesDatabaseReference>((resolve) => { finishReference = resolve; }));
    const opening = notes.openDatabase(blockId);
    await notes.selectPage(child.page.id, { openMode: "full" });
    finishReference(reference);
    expect(await opening).toBe(false);
    expect(notes.selectedPageId).toBe(child.page.id);
    expect(notes.selectedDatabaseBlockId).toBeNull();
  });

  it("redirects a linked database to its canonical owner while retaining the original note", async () => {
    const { notes, parent, other } = await setup();
    const { blockId } = databaseInPage(other, "Shared planning");
    backend.databaseReference.mockResolvedValueOnce({
      block_id: parent.blocks.results[0].id, page_id: parent.page.id, title: "Linked planning",
      source_block_id: blockId, source_page_id: other.page.id, is_linked: true, owned_data_source_count: 0, editing_locked: false,
    });
    const mainStore = notes.editorPanes[0].store;
    expect(await notes.openDatabase(parent.blocks.results[0].id)).toBe(true);
    expect(notes.editorPanes[0].store).toBe(mainStore);
    expect(mainStore.selectedPageId).toBe(parent.page.id);
    expect(notes.selectedPageId).toBe(other.page.id);
    expect(notes.selectedDatabaseBlock?.id).toBe(blockId);
    const { getProjects } = await import("$lib/stores/projects.svelte");
    expect(getProjects().selectedProjectId).toBe("other-project");
    await vi.waitFor(() => expect(document.querySelector("[data-notes-database-breadcrumb]")?.textContent).toContain("Shared planning"));
    notes.closeDatabase();
    expect(notes.selectedPageId).toBe(other.page.id);
    expect(notes.editorPanes[0].store).toBe(mainStore);
  });

  it("restores each pane's project context on focus and preview close without reopening either document", async () => {
    const { notes, parent, other } = await setup();
    const { getProjects } = await import("$lib/stores/projects.svelte");
    const mainViewport = document.querySelector("[data-notes-editor-scroll]");
    backend.otherProjectCursor = "other-project-next";
    await notes.selectPage(other.page.id, { openMode: "side" });
    expect(getProjects().selectedProjectId).toBe("other-project");
    await tick();
    await notes.loadMoreWorkspaceWindow();
    expect(backend.workspaceRequests.at(-1)).toMatchObject({ project_id: "other-project", page_cursor: "other-project-next" });
    const previewId = notes.activePaneId;
    notes.activatePane(notes.mainPaneId);
    expect(getProjects().selectedProjectId).toBe("project");
    notes.activatePane(previewId);
    expect(getProjects().selectedProjectId).toBe("other-project");
    await notes.closeContextualPage();
    await tick();
    expect(getProjects().selectedProjectId).toBe("project");
    expect(document.querySelector("[data-notes-editor-scroll]")).toBe(mainViewport);
    expect(backend.open.mock.calls.filter(([id]) => id === parent.page.id)).toHaveLength(1);
    expect(backend.open.mock.calls.filter(([id]) => id === other.page.id)).toHaveLength(1);
  });

  it("opens the selected project's first note beside the Projects sidebar instead of following an earlier project's note", async () => {
    const { notes, other } = await setup();
    const { getProjects } = await import("$lib/stores/projects.svelte");
    await unmount(component!);
    getProjects().selectedProjectId = "other-project";
    const { default: NotesView } = await import("./NotesView.svelte");
    const projectSidebar = createRawSnippet<[() => void]>(() => ({ render: () => "<aside></aside>" }));
    component = mount(NotesView, { target: document.body, props: { projectSidebar } });
    await tick();
    document.querySelector(`[data-notes-pane="${notes.mainPaneId}"]`)?.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(getProjects().selectedProjectId).toBe("other-project");
    await vi.waitFor(() => expect(notes.selectedPageId).toBe(other.page.id));
    expect(getProjects().selectedProjectId).toBe("other-project");
    expect(backend.workspaceRequests).toContainEqual(expect.objectContaining({
      project_id: "other-project", selected_page_id: null, include_navigation_index: true,
    }));
  });

  it("keeps a peek at another project's note open beside the Projects sidebar without changing the project", async () => {
    const { notes, parent, other } = await setup();
    const { getProjects } = await import("$lib/stores/projects.svelte");
    await unmount(component!);
    const { default: NotesView } = await import("./NotesView.svelte");
    const projectSidebar = createRawSnippet<[() => void]>(() => ({ render: () => "<aside></aside>" }));
    component = mount(NotesView, { target: document.body, props: { projectSidebar } });
    await tick();
    const shellReads = backend.workspaceRequests.length;
    await notes.selectPage(other.page.id, { openMode: "side" });
    notes.activatePane(notes.previewPane!.id);
    await tick(); await tick();
    expect(getProjects().selectedProjectId).toBe("project");
    expect(notes.previewPane?.store.selectedPageId).toBe(other.page.id);
    expect(notes.editorPanes[0]?.store.selectedPageId).toBe(parent.page.id);
    expect(backend.workspaceRequests).toHaveLength(shellReads);
  });

  it("keeps the current project when opening a note from another project fails", async () => {
    const { notes, other } = await setup();
    const { getProjects } = await import("$lib/stores/projects.svelte");
    const shellReads = backend.workspaceRequests.length;
    backend.open.mockRejectedValueOnce(new Error("Read failed"));
    await expect(notes.selectPage(other.page.id, { openMode: "full" })).rejects.toThrow("Read failed");
    expect(getProjects().selectedProjectId).toBe("project");
    expect(backend.workspaceRequests).toHaveLength(shellReads);
  });

  it("keeps an explicit project selection while reloading the workspace closes an old preview", async () => {
    const { notes, child } = await setup();
    const { getProjects } = await import("$lib/stores/projects.svelte");
    await notes.selectPage(child.page.id, { openMode: "side" });
    getProjects().selectedProjectId = "other-project";
    await notes.load();
    expect(notes.previewPane).toBeNull();
    expect(getProjects().selectedProjectId).toBe("other-project");
    expect(backend.workspaceRequests.at(-1)?.project_id).toBe("other-project");
  });

  it("does not restore an earlier note's project after project metadata finishes loading", async () => {
    const { notes, parent, other } = await setup();
    const { getProjects } = await import("$lib/stores/projects.svelte");
    backend.projectsLoaded = false;
    let finishProjects!: () => void;
    const readiness = new Promise<void>((resolve) => { finishProjects = resolve; });
    backend.projectsReady.mockReturnValue(readiness);
    await notes.selectPage(other.page.id, { openMode: "full" });
    await notes.selectPage(parent.page.id, { openMode: "full" });
    const shellReads = backend.workspaceRequests.length;
    backend.projectsLoaded = true;
    finishProjects();
    await readiness;
    await tick(); await tick();
    expect(notes.selectedPageId).toBe(parent.page.id);
    expect(getProjects().selectedProjectId).toBe("project");
    expect(backend.workspaceRequests).toHaveLength(shellReads);
  });

  it.each(["center", "side"] as const)("keeps the main editor mounted when a %s preview opens and closes, without reloading it", async (mode) => {
    const { notes, parent, child } = await setup();
    const mainViewport = document.querySelector<HTMLElement>("[data-notes-editor-scroll]")!;
    mainViewport.scrollTop = 240;
    mainViewport.dispatchEvent(new Event("scroll"));
    await notes.selectPage(child.page.id, { openMode: mode });
    await tick(); await tick();
    const background = document.querySelector("[data-notes-main-page]")!;
    expect((background as HTMLElement).inert).toBe(mode === "center");
    expect(background.querySelector("[data-notes-editor-scroll]")).toBe(mainViewport);
    expect(background.textContent).toContain("Main note content");
    expect(background.textContent).not.toContain("Sub-note content");
    expect(document.querySelector("[data-notes-page-peek]")?.textContent).toContain("Sub-note content");
    expect(backend.mentionSources).not.toHaveBeenCalled();
    expect(backend.destinations).not.toHaveBeenCalled();
    const path = document.querySelector("[data-notes-workspace-header]")!.textContent!;
    expect(path).toContain("Main note");
    expect(path.indexOf("Sub-note")).toBeGreaterThan(path.indexOf("Main note"));
    notes.focusBlock(child.blocks.results[0].id);
    await tick(); await tick();
    expect(background.contains(document.activeElement)).toBe(false);
    await notes.closeContextualPage();
    await tick(); await tick();
    expect(notes.selectedPageId).toBe(parent.page.id);
    expect(document.querySelector("[data-notes-page-peek]")).toBeNull();
    expect(document.querySelector<HTMLElement>("[data-notes-editor-scroll]")?.scrollTop).toBe(240);
    expect(document.querySelector("[data-notes-editor-scroll]")).toBe(mainViewport);
    expect(backend.open.mock.calls.filter(([id]) => id === parent.page.id)).toHaveLength(1);
    expect(document.querySelector("[data-notes-workspace-header]")?.textContent).not.toContain("Sub-note");
    expect(backend.saveBlock).not.toHaveBeenCalled();
  }, 15_000);

  it("converts the child row in its owning main editor and keeps that editor through further preview navigation", async () => {
    const { notes, parent } = await setup();
    const retained = notes.editorPanes[0].store;
    backend.createChild.mockImplementation(async (id: string, request: NotesChildPageFromBlockCreate) => {
      const created = page(id, request.title ?? "", parent.page.id);
      backend.pages.set(id, created);
      return created;
    });
    await notes.convertBlock(parent.blocks.results[0].id, "child_page", true);
    await tick(); await tick();
    expect(retained.selectedPageId).toBe(parent.page.id);
    expect(retained.blockById(parent.blocks.results[0].id)?.type).toBe("child_page");
    expect(document.querySelector("[data-notes-main-page] button[data-notes-atomic-block]")).not.toBeNull();
    await notes.selectPage("child", { openMode: "side" });
    await tick();
    expect(notes.editorPanes[0].store).toBe(retained);
    const previewViewport = document.querySelector("[data-notes-page-peek] [data-notes-editor-scroll]");
    await notes.showPaneAs(notes.activePaneId, "full");
    await tick();
    expect(notes.editorPanes).toHaveLength(1);
    expect(notes.editorPanes[0].store).not.toBe(retained);
    expect(document.querySelector("[data-notes-editor-scroll]")).toBe(previewViewport);
  });

  it.each(["center", "side"] as const)("retains a main note originally opened in %s mode when opening its child", async (mode) => {
    const { notes, parent, child } = await setup(mode);
    const mainViewport = document.querySelector("[data-notes-editor-scroll]");
    expect(notes.editorPanes).toHaveLength(1);
    await notes.openPageContextually(child.page.id);
    await tick(); await tick();
    expect(document.querySelector("[data-notes-main-page]")?.textContent).toContain("Main note content");
    expect(document.querySelector("[data-notes-page-peek]")?.textContent).toContain("Sub-note content");
    await notes.closeContextualPage();
    expect(notes.selectedPageId).toBe(parent.page.id);
    expect(notes.pageOpenMode).toBe("full");
    expect(document.querySelector("[data-notes-main-page] [data-notes-editor-scroll]")).toBe(mainViewport);
  });

  it("keeps the parent visible, shows a skeleton without a close button while loading, and allows closing a failed preview", async () => {
    const { notes, parent, child } = await setup();
    let rejectLoad!: (error: Error) => void;
    backend.open.mockImplementationOnce(() => new Promise<never>((_resolve, reject) => { rejectLoad = reject; }));
    const opening = notes.openPageContextually(child.page.id);
    const failure = expect(opening).rejects.toThrow("Read failed");
    await vi.waitFor(() => expect(document.querySelector("[data-notes-page-peek] [aria-busy='true']")).not.toBeNull());
    const pendingPreview = document.querySelector("[data-notes-page-peek]")!;
    expect(pendingPreview.querySelector('[data-notes-skeleton="page"]')).not.toBeNull();
    expect(pendingPreview.querySelector("button")).toBeNull();
    expect(pendingPreview.textContent).not.toContain("Loading");
    expect(document.querySelector("[data-notes-main-page]")?.textContent).toContain("Main note content");
    rejectLoad(new Error("Read failed"));
    await failure; await tick();
    expect(document.querySelector("[data-notes-page-peek] [role='alert']")?.textContent).toContain("Read failed");
    [...document.querySelectorAll<HTMLButtonElement>("[data-notes-page-peek] button")].find((button) => button.textContent?.trim() === "Close")!.click();
    await vi.waitFor(() => {
      expect(notes.selectedPageId).toBe(parent.page.id);
      expect(notes.loadedPage?.id).toBe(parent.page.id);
      expect(notes.primaryContentReady).toBe(true);
    });
  });

  it("opens an empty note through one page read and shows its known cover while pending", async () => {
    const { notes, child } = await setup("full", true);
    let resolveLoad!: (loaded: NotesPageOpenResponse) => void;
    backend.open.mockClear();
    backend.open.mockImplementationOnce(() => new Promise<NotesPageOpenResponse>((resolve) => { resolveLoad = resolve; }));
    const opening = notes.selectPage(child.page.id, { openMode: "full" });
    await vi.waitFor(() => expect(document.querySelector('[data-notes-skeleton="page"]')).not.toBeNull());
    const skeleton = document.querySelector('[data-notes-skeleton="page"]')!;
    expect(skeleton.querySelector("[data-notes-skeleton-cover]")).not.toBeNull();
    expect(skeleton.querySelector("button")).toBeNull();
    expect(skeleton.textContent).not.toContain("Loading");
    expect(document.querySelector("[data-notes-editor-scroll]")).toBeNull();
    expect(backend.open).toHaveBeenCalledExactlyOnceWith(child.page.id);
    resolveLoad(child);
    await opening; await tick();
    expect(document.querySelector('[data-notes-skeleton="page"]')).toBeNull();
    expect(document.querySelector("[data-notes-editor-scroll]")).not.toBeNull();
    expect(backend.open).toHaveBeenCalledTimes(1);
    expect(backend.mentionSources).not.toHaveBeenCalled();
    expect(backend.destinations).not.toHaveBeenCalled();
  });

  it("lets a failed main note retry its read without adding a close button", async () => {
    const { notes, child } = await setup();
    backend.open.mockClear();
    backend.open.mockRejectedValueOnce(new Error("Read failed"));
    await expect(notes.selectPage(child.page.id, { openMode: "full" })).rejects.toThrow("Read failed");
    await tick();
    const pane = document.querySelector("[data-notes-main-page]")!;
    expect(pane.querySelector("[role='alert']")?.textContent).toContain("Read failed");
    expect(pane.querySelector('[data-notes-skeleton="page"]')).toBeNull();
    expect([...pane.querySelectorAll<HTMLButtonElement>("button")].some((button) => button.textContent?.trim() === "Close")).toBe(false);
    expect(pane.querySelector("[data-notice-dismiss]")).toBeNull();
    const retry = pane.querySelector<HTMLButtonElement>('button[aria-label="Retry"]');
    expect(retry).not.toBeNull();
    retry!.click();
    await vi.waitFor(() => expect(pane.querySelector("[data-notes-editor-scroll]")).not.toBeNull());
    expect(notes.loadedPage?.id).toBe(child.page.id);
    expect(backend.open).toHaveBeenCalledTimes(2);
  });

  it("shows only the active note's ancestors while switching between two sibling previews and the main pane", async () => {
    const { notes, child, sibling } = await setup();
    const header = () => document.querySelector("[data-notes-workspace-header]")!.textContent!;
    expect(header()).toContain("Main note");
    expect(header()).not.toContain("Sub-note");
    expect(header()).not.toContain("Second sub-note");
    await notes.openPageContextually(child.page.id);
    await tick();
    expect(header()).toContain("Main note");
    expect(header()).toContain("Sub-note");
    expect(header()).not.toContain("Second sub-note");
    await notes.selectPage(sibling.page.id, { openMode: "side" });
    await tick();
    expect(header()).toContain("Main note");
    expect(header()).toContain("Second sub-note");
    expect(header()).not.toContain("Sub-note");
    document.querySelector<HTMLElement>("[data-notes-main-page] [contenteditable='true']")!.focus();
    await tick();
    expect(notes.activePaneId).toBe(notes.mainPaneId);
    expect(header()).not.toContain("Second sub-note");
    document.querySelector<HTMLElement>("[data-notes-page-peek] [contenteditable='true']")!.focus();
    await tick();
    expect(header()).toContain("Second sub-note");
    await notes.closeContextualPage();
    await tick();
    expect(header()).not.toContain("Second sub-note");
    expect(header()).not.toContain("Sub-note");
  });

  it("edits both side panes in their own documents and keeps undo history scoped to its editor", async () => {
    const { notes, parent, child } = await setup();
    const main = notes.editorPanes[0].store;
    await notes.selectPage(child.page.id, { openMode: "side" });
    await tick(); await tick();
    const preview = notes.previewPane!.store;
    const mainInput = document.querySelector<HTMLElement>("[data-notes-main-page] [contenteditable='true']")!;
    const childInput = document.querySelector<HTMLElement>("[data-notes-page-peek] [contenteditable='true']")!;
    for (const [input, text] of [[mainInput, "Edited main"], [childInput, "Edited child"]] as const) {
      input.focus();
      input.textContent = text;
      input.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
      await tick();
    }
    await notes.flushPendingWrites();
    expect(blockPlainText(main.blockById(parent.blocks.results[0].id)!)).toBe("Edited main");
    expect(blockPlainText(preview.blockById(child.blocks.results[0].id)!)).toBe("Edited child");
    expect(backend.saveBlock.mock.calls.map(([id]) => id)).toEqual(expect.arrayContaining(["parent-body", "child-body"]));
    expect(main.canUndoNotesEdit).toBe(true);
    expect(preview.canUndoNotesEdit).toBe(true);
    mainInput.focus();
    await notes.undoNotesEdit();
    await tick();
    expect(blockPlainText(main.blockById("parent-body")!)).toBe("Main note content");
    expect(blockPlainText(preview.blockById("child-body")!)).toBe("Edited child");
    await notes.closeContextualPage();
    await tick();
    expect(document.querySelector("[data-notes-main-page] [contenteditable='true']")).toBe(mainInput);
    expect(main.canRedoNotesEdit).toBe(true);
  });

  it("creates another child under the main page while a sibling preview is open", async () => {
    const { notes, parent, child } = await setup();
    const main = notes.editorPanes[0].store;
    await notes.selectPage(child.page.id, { openMode: "side" });
    await tick();
    const mainInput = document.querySelector<HTMLElement>("[data-notes-main-page] [contenteditable='true']")!;
    mainInput.focus();
    backend.createChild.mockImplementation(async (id: string, request: NotesChildPageFromBlockCreate) => {
      const created = page(id, request.title ?? "", parent.page.id);
      backend.pages.set(id, created);
      return created;
    });
    await main.convertBlock("parent-body", "child_page", true);
    await tick();
    expect(backend.createChild.mock.calls[0][0]).toBe("parent-body");
    expect(notes.loadedPage?.parent).toEqual({ type: "page_id", page_id: parent.page.id });
    expect(notes.editorPanes[0].store).toBe(main);
    expect(main.selectedPageId).toBe(parent.page.id);
    const header = document.querySelector("[data-notes-workspace-header]")!.textContent!;
    expect(header).toContain("Main note");
    expect(header).not.toContain("Sub-note");
    expect(header).not.toContain("Second sub-note");
  });

  it("keeps a preview draft open when closing cannot save it and leaves the main editor usable", async () => {
    const { notes, child } = await setup();
    const main = notes.editorPanes[0].store;
    await notes.selectPage(child.page.id, { openMode: "side" });
    const preview = notes.previewPane!;
    backend.saveBlock.mockRejectedValueOnce(new Error("Disk unavailable"));
    await preview.store.updateBlockText("child-body", "Unsaved child");
    await expect(notes.closeContextualPage()).rejects.toThrow("Disk unavailable");
    expect(notes.previewPane).toBe(preview);
    expect(blockPlainText(preview.store.blockById("child-body")!)).toBe("Unsaved child");
    await main.updateBlockText("parent-body", "Main remains editable");
    await main.flushPendingWrites();
    expect(main.editorSaveError).toBeNull();
    await preview.store.retryEditorMutations();
    await notes.closeContextualPage();
    expect(notes.previewPane).toBeNull();
  });

  it("inserts a new child row into the retained parent without reopening it", async () => {
    const { notes, parent } = await setup();
    const main = notes.editorPanes[0].store;
    await main.createSiblingAfter("parent-body", { kind: "block", blockType: "child_page" });
    const childId = notes.selectedPageId!;
    expect(main.blockById(childId)?.type).toBe("child_page");
    expect(main.childIdsByParentId[parent.page.id]).toEqual(["parent-body", childId, "child", "sibling"]);
    expect(backend.open.mock.calls.filter(([id]) => id === parent.page.id)).toHaveLength(1);
    await notes.closeContextualPage();
    await tick();
    expect(document.querySelectorAll("[data-notes-main-page] button[data-notes-atomic-block]")).toHaveLength(3);
  });

  it("returns to the mounted main editor when navigation explicitly opens it as a full page", async () => {
    const { notes, parent, child } = await setup();
    const viewport = document.querySelector("[data-notes-editor-scroll]");
    await notes.selectPage(child.page.id, { openMode: "side" });
    await notes.selectPage(parent.page.id, { openMode: "full" });
    await tick();
    expect(notes.previewPane).toBeNull();
    expect(notes.selectedPageId).toBe(parent.page.id);
    expect(document.querySelector("[data-notes-editor-scroll]")).toBe(viewport);
    expect(backend.open.mock.calls.filter(([id]) => id === parent.page.id)).toHaveLength(1);
  });
});
