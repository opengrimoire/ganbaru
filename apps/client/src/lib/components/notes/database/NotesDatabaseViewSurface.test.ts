// @vitest-environment jsdom
import { createRawSnippet, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { duplicateNotesDatabaseView, reorderNotesDatabaseViews, listNotesDatabaseViews, listNotesDataSourceTemplates, listNotesDataSources, setNotesDatabaseEditingLock, createNotesDataSource, attachNotesDataSource } from "$lib/api/notes";
import { databaseResource, MAX_DATABASE_SESSION_RESOURCES, notesDatabaseSession } from "$lib/notes/database/session.svelte";
import type { NotesDatabaseView, NotesDataSourcePropertyType, NotesDataSourceListItem } from "$lib/notes/types";
import type { NotesDatabaseSourceEditingScope } from "$lib/notes/database/data-source-schema";
import NotesDatabaseViewSurface from "./NotesDatabaseViewSurface.svelte";

vi.mock("$lib/api/notes", () => ({
  listNotesDatabaseViews: vi.fn(), listNotesDataSourceTemplates: vi.fn(async () => []),
  duplicateNotesDatabaseView: vi.fn(), reorderNotesDatabaseViews: vi.fn(),
  listNotesDataSources: vi.fn(async () => []), setNotesDatabaseEditingLock: vi.fn(), createNotesDataSource: vi.fn(), attachNotesDataSource: vi.fn(),
}));
vi.mock("$lib/components/notes/editor-component-registry", async () => {
  const { default: component } = await import("./NotesDatabaseViewSessionHarness.test.svelte");
  return {
    readNotesDatabaseView: () => ({ kind: "table", component }),
    loadNotesDatabaseView: async () => ({ kind: "table", component }),
    retryNotesDatabaseView: async () => ({ kind: "table", component }),
  };
});

function views(names = ["First", "Second"]): NotesDatabaseView[] {
  return names.map((name) => ({
    object: "view", id: name, parent: { type: "database_id", database_id: "database" },
    data_source_id: "source", name, type: "table", filter: null, sorts: [], url: null,
    configuration: { type: "table", table: {
      property_order: [], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page",
    } },
    created_time: "2026-09-29T00:00:00Z", last_edited_time: "2026-09-29T00:00:00Z",
    source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null,
  }));
}

/** Describe a local source independently of the shell that presents it. */
function source(id: string, title: string, databaseId: string, location: string[] = []): NotesDataSourceListItem {
  return { object: "data_source", id, title, title_rich_text: [], description: [], icon: null, in_trash: false,
    parent: { type: "database_id", database_id: databaseId }, database_parent: { type: "page_id", page_id: "page" },
    properties: { Name: { id: "title", name: "Name", type: "title", title: {} } },
    created_time: "2026-10-01T00:00:00Z", last_edited_time: "2026-10-01T00:00:00Z",
    source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null, location };
}

/** Serve saved views from a store that reorder requests rewrite, so reloads see the saved order. */
function persistViewOrder(saved: NotesDatabaseView[]): void {
  let current = saved;
  vi.mocked(listNotesDatabaseViews).mockImplementation(async () => current);
  vi.mocked(reorderNotesDatabaseViews).mockImplementation(async (_databaseId, ids) => {
    current = ids.map((id) => saved.find((view) => view.id === id)!);
    return current;
  });
}

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  notesDatabaseSession.clear();
  document.body.replaceChildren();
  vi.resetAllMocks();
});

function open(onReady = () => {}, options: {
  onAddProperty?: (type: NotesDataSourcePropertyType, name: string, icon: string | null, scope?: NotesDatabaseSourceEditingScope) => Promise<void>;
  onSavingChange?: (saving: boolean) => void;
  editingLocked?: boolean;
  onEditingLockChange?: (locked: boolean) => void;
} = {}): void {
  component = mount(NotesDatabaseViewSurface, { target: document.body, props: {
    dataSourceId: "source", databaseId: "database", initialViewId: "First",
    onReady,
    reloadKeys: { table: 0, board: 0, gallery: 0, list: 0, calendar: 0, timeline: 0 },
    onSelectPage: vi.fn(), onEditProperties: vi.fn(), onCreateLinkedDatabaseView: vi.fn(),
    propertyEditor: createRawSnippet<[string]>(() => ({ render: () => "<div></div>" })), onLoadPropertyEditor: vi.fn(),
    onAddProperty: options.onAddProperty ?? vi.fn(async () => {}),
    onSavingChange: options.onSavingChange,
    editingLocked: options.editingLocked,
    onEditingLockChange: options.onEditingLockChange,
  } });
}

function chooseSecond(): void {
  const button = [...document.querySelectorAll<HTMLButtonElement>("button")]
    .find((candidate) => candidate.textContent?.trim() === "Second");
  expect(button).toBeDefined();
  button!.click();
}

describe("Notes database view sessions", () => {
  it.each(["create", "attach"] as const)("selects the new table and its source after %s from the Source page", async (operation) => {
    let savedViews = views();
    let available = [source("source", "Tasks", "database"), source("foreign-source", "Workout", "other-database", ["Health", ""])];
    let resultView: NotesDatabaseView | null = null;
    vi.mocked(listNotesDatabaseViews).mockImplementation(async () => savedViews);
    vi.mocked(listNotesDataSources).mockImplementation(async () => available);
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    vi.mocked(createNotesDataSource).mockImplementation(async (request) => {
      const created = source(request.id, request.title, request.database_id);
      available = [...available, created];
      resultView = { ...savedViews[0], id: request.view_id, data_source_id: request.id, name: request.view_name };
      savedViews = [...savedViews, resultView];
      return { data_source: created, view: resultView };
    });
    vi.mocked(attachNotesDataSource).mockImplementation(async (request) => {
      resultView = { ...savedViews[0], id: request.view_id, data_source_id: request.data_source_id, name: request.view_name };
      savedViews = [...savedViews, resultView];
      return { data_source: available[1], view: resultView };
    });
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")).not.toBeNull());
    document.querySelector<HTMLButtonElement>('button[aria-label="View settings"]')!.click();
    await tick();
    document.querySelector<HTMLButtonElement>('button[aria-label="Source"]')!.click();
    await vi.waitFor(() => expect(document.querySelector<HTMLInputElement>('input[aria-label="Search or name a new source…"]')).not.toBeNull());
    if (operation === "create") {
      const name = document.querySelector<HTMLInputElement>('input[aria-label="Search or name a new source…"]')!;
      name.value = "Study";
      name.dispatchEvent(new Event("input", { bubbles: true }));
      await tick();
      Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.trim() === "Create “Study”")!.click();
      await vi.waitFor(() => expect(createNotesDataSource).toHaveBeenCalledWith(expect.objectContaining({ database_id: "database", title: "Study", view_name: "Table" })));
    } else {
      const workout = () => Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.includes("Workout"));
      await vi.waitFor(() => expect(workout()?.textContent).toContain("Health / Untitled"));
      workout()!.click();
      await vi.waitFor(() => expect(attachNotesDataSource).toHaveBeenCalledWith(expect.objectContaining({ data_source_id: "foreign-source", database_id: "database", view_name: "Table" })));
    }
    await vi.waitFor(() => {
      expect(resultView).not.toBeNull();
      expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe(resultView?.id);
      expect(document.querySelector("[data-session-view]")?.getAttribute("data-source-id")).toBe(resultView?.data_source_id);
    });
  });
  it("lists a few other sources with their locations, filters by location, and reveals the rest on Show more", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSources).mockResolvedValue([
      source("source", "Tasks", "database"),
      ...["One", "Two", "Three", "Four", "Five", "Six"].map((title) => source(title, title, `database-${title}`, ["Work", title === "Six" ? "Archive" : "Board"])),
    ]);
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")).not.toBeNull());
    document.querySelector<HTMLButtonElement>('button[aria-label="View settings"]')!.click();
    await tick();
    document.querySelector<HTMLButtonElement>('button[aria-label="Source"]')!.click();
    const buttonWith = (text: string) => Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.includes(text));
    await vi.waitFor(() => expect(buttonWith("Work / Board")).toBeDefined());
    expect(buttonWith("Six")).toBeUndefined();
    const query = document.querySelector<HTMLInputElement>('input[aria-label="Search or name a new source…"]')!;
    query.value = "archive";
    query.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(buttonWith("Work / Archive")?.textContent).toContain("Six");
    expect(buttonWith("Work / Board")).toBeUndefined();
    query.value = "";
    query.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    buttonWith("Show 1 more")!.click();
    await tick();
    expect(buttonWith("Six")).toBeDefined();
    expect(buttonWith("Show 1 more")).toBeUndefined();
  });

  it("opens view settings on the Source page from a view tab menu", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSources).mockResolvedValue([source("source", "Tasks", "database")]);
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    document.querySelector<HTMLButtonElement>('button[aria-label="First"]')!.click();
    await tick();
    const sourceItem = () => Array.from(document.querySelectorAll<HTMLButtonElement>("button.menu-item")).find((button) => button.textContent?.trim().startsWith("Source"));
    await vi.waitFor(() => expect(sourceItem()?.textContent).toContain("Tasks"));
    sourceItem()!.click();
    await vi.waitFor(() => expect(document.querySelector<HTMLInputElement>('input[aria-label="Search or name a new source…"]')).not.toBeNull());
    expect(document.querySelector('button[aria-label="View settings"]')?.getAttribute("aria-expanded")).toBe("true");
  });

  it("blocks toolbar switching during a layout write and resets local state only after a saved-view switch", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    let finishSave: () => void = () => {};
    open(() => {}, { onAddProperty: () => new Promise((resolve) => { finishSave = resolve; }) });
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    const firstLayout = document.querySelector("[data-session-view]");
    const draft = document.querySelector<HTMLInputElement>('input[aria-label="Layout draft"]')!;
    draft.value = "Local draft";
    draft.dispatchEvent(new Event("input", { bubbles: true }));
    document.querySelector<HTMLButtonElement>("[data-start-save]")!.click();
    await tick();
    const secondTab = document.querySelector<HTMLButtonElement>('button[aria-label="Second"]')!;
    expect(secondTab.disabled).toBe(true);
    chooseSecond();
    await tick();
    expect(document.querySelector("[data-session-view]")).toBe(firstLayout);
    expect(draft.value).toBe("Local draft");
    const newRow = Array.from(document.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent?.trim() === "New")!;
    expect(newRow.disabled).toBe(false);
    newRow.click();
    newRow.click();
    await tick();
    expect(firstLayout?.getAttribute("data-new-row-request")).toBe("2");
    finishSave();
    await vi.waitFor(() => expect(secondTab.disabled).toBe(false));
    chooseSecond();
    await tick();
    expect(document.querySelector("[data-session-view]")).not.toBe(firstLayout);
    expect(document.querySelector("[data-session-view]")?.getAttribute("data-layout-snapshot")).toBe("Second");
    expect(document.querySelector<HTMLInputElement>('input[aria-label="Layout draft"]')?.value).toBe("Second");
    expect(document.querySelector("[data-session-view]")?.getAttribute("data-new-row-request")).toBe("0");
    document.querySelector<HTMLButtonElement>('button[aria-label="First"]')!.click();
    await tick();
    expect(document.querySelector("[data-session-view]")?.getAttribute("data-new-row-request")).toBe("0");
  });

  it("uses the selected view's source for its layout, templates, and scoped property requests", async () => {
    const savedViews = views();
    savedViews[1] = { ...savedViews[1], data_source_id: "second-source" };
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(savedViews);
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    const addProperty = vi.fn(async (_type: NotesDataSourcePropertyType, _name: string, _icon: string | null, _scope?: NotesDatabaseSourceEditingScope) => {});
    open(() => {}, { onAddProperty: addProperty });
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-source-id")).toBe("source"));
    chooseSecond();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-source-id")).toBe("second-source"));
    document.querySelector<HTMLButtonElement>("[data-start-save]")!.click();
    await vi.waitFor(() => expect(addProperty).toHaveBeenCalledWith("rich_text", "Second", null, { dataSourceId: "second-source", databaseId: "database", viewId: "Second" }));
    const newOptions = document.querySelector<HTMLButtonElement>('button[aria-label="New page options"]')!;
    newOptions.click();
    await vi.waitFor(() => expect(listNotesDataSourceTemplates).toHaveBeenCalledWith("second-source"));
  });

  it("keeps saved-view selection and New available while the persisted layout is locked", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    vi.mocked(listNotesDataSources).mockResolvedValue([]);
    const changeLock = vi.fn<(locked: boolean) => void>();
    vi.mocked(setNotesDatabaseEditingLock).mockResolvedValue({ block_id: "database", page_id: "page", source_block_id: "database", source_page_id: "page", title: "Tasks", is_linked: false, owned_data_source_count: 1, editing_locked: false });
    open(() => {}, { editingLocked: true, onEditingLockChange: changeLock });
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-editing-locked")).toBe("true"));
    expect(document.querySelector<HTMLButtonElement>('button[aria-label="Add a new view"]')?.disabled).toBe(true);
    chooseSecond();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("Second"));
    const newRow = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.trim() === "New")!;
    expect(newRow.disabled).toBe(false);
    document.querySelector<HTMLButtonElement>('button[aria-label="View settings"]')!.click();
    await tick();
    const viewName = document.querySelector<HTMLInputElement>('input[aria-label="View name"]')!;
    expect(viewName.disabled).toBe(true);
    document.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Database layout lock"]')!.click();
    await vi.waitFor(() => expect(setNotesDatabaseEditingLock).toHaveBeenCalledWith("database", false));
    await vi.waitFor(() => expect(changeLock).toHaveBeenCalledWith(false));
  });

  it("isolates external preselection from old writes and ignores stale saving callbacks after a remount", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    let finishFirst: () => void = () => {};
    let finishSecond: () => void = () => {};
    const onSavingChange = vi.fn<(saving: boolean) => void>();
    open(() => {}, {
      onSavingChange,
      onAddProperty: (_type, viewId) => new Promise((resolve) => {
        if (viewId === "First") finishFirst = resolve;
        else finishSecond = resolve;
      }),
    });
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    const firstLayout = document.querySelector("[data-session-view]");
    document.querySelector<HTMLButtonElement>("[data-start-save]")!.click();
    await tick();
    notesDatabaseSession.selectView("database", "Second");
    await tick();
    const secondLayout = document.querySelector("[data-session-view]");
    expect(secondLayout).not.toBe(firstLayout);
    expect(secondLayout?.getAttribute("data-layout-snapshot")).toBe("Second");
    document.querySelector<HTMLButtonElement>("[data-start-save]")!.click();
    await tick();
    finishFirst();
    await tick();
    await tick();
    expect(document.querySelector("[data-session-view]")).toBe(secondLayout);
    expect(secondLayout?.getAttribute("data-layout-snapshot")).toBe("Second");
    expect(document.querySelector<HTMLButtonElement>('button[aria-label="First"]')?.disabled).toBe(true);
    expect(onSavingChange).toHaveBeenLastCalledWith(true);
    finishSecond();
    await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>('button[aria-label="First"]')?.disabled).toBe(false));
    expect(secondLayout?.getAttribute("data-layout-snapshot")).toBe("Saved Second");
    expect(onSavingChange).toHaveBeenLastCalledWith(false);
  });

  it("selects the newly copied saved view after an internal write while toolbar selection is disabled", async () => {
    const copied = { ...views()[0], id: "Copied", name: "Copied" };
    vi.mocked(listNotesDatabaseViews).mockResolvedValueOnce(views()).mockResolvedValue([...views(), copied]);
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    let finishCopy: (view: NotesDatabaseView) => void = () => {};
    vi.mocked(duplicateNotesDatabaseView).mockImplementation(() => new Promise((resolve) => { finishCopy = resolve; }));
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    document.querySelector<HTMLButtonElement>('button[aria-label="First"]')!.click();
    await tick();
    await tick();
    const duplicate = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button'))
      .find((button) => button.textContent?.trim() === "Duplicate view");
    expect(duplicate).toBeDefined();
    duplicate!.click();
    await vi.waitFor(() => expect(duplicateNotesDatabaseView).toHaveBeenCalledOnce());
    expect(document.querySelector<HTMLButtonElement>('button[aria-label="Second"]')?.disabled).toBe(true);
    finishCopy(copied);
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("Copied"));
    expect(notesDatabaseSession.recall("database")?.viewId).toBe("Copied");
  });

  it("keeps a mounted database's chosen view when unrelated presentation entries evict its cached preference", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    chooseSecond();
    await tick();
    for (let index = 0; index < MAX_DATABASE_SESSION_RESOURCES; index += 1) {
      notesDatabaseSession.remember(`other-${index}`, { viewId: null, scrollLeft: 0 });
    }
    expect(notesDatabaseSession.recall("database")).toBeUndefined();
    await tick();
    expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("Second");
    notesDatabaseSession.selectView("database", "First");
    await tick();
    expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First");
  });
  it("applies top-bar preselection to an already mounted database without reloading its metadata", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    notesDatabaseSession.selectView("database", "Second");
    await tick();
    expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("Second");
    expect(listNotesDatabaseViews).toHaveBeenCalledOnce();
  });

  it("falls back to a saved view when a preselected view has since been deleted", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    notesDatabaseSession.selectView("database", "Removed");
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    expect(notesDatabaseSession.recall("database")?.viewId).toBe("First");
  });
  it("reports readiness after rows are ready, without reporting metadata alone", async () => {
    const onReady = vi.fn();
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    open(onReady);
    await vi.waitFor(() => expect(document.querySelector("[data-finish-view]")).not.toBeNull());
    expect(onReady).not.toHaveBeenCalled();
    document.querySelector<HTMLButtonElement>("[data-finish-view]")!.click();
    expect(onReady).toHaveBeenCalledOnce();
  });

  it("restores the chosen saved view immediately without refetching metadata", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    chooseSecond();
    await tick();
    await unmount(component!);
    component = undefined;
    document.body.replaceChildren();
    open();
    await tick();
    expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("Second");
    expect(listNotesDatabaseViews).toHaveBeenCalledOnce();
    expect(listNotesDataSourceTemplates).toHaveBeenCalledOnce();
  });

  it("does not undo a view selection made while a metadata refresh is pending", async () => {
    notesDatabaseSession.write(databaseResource("views", "source", { databaseId: "database" }), views());
    notesDatabaseSession.write(databaseResource("templates", "source"), []);
    open();
    await tick();
    let finish: (value: NotesDatabaseView[]) => void = () => {};
    vi.mocked(listNotesDatabaseViews).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    notesDatabaseSession.invalidate();
    await vi.waitFor(() => expect(listNotesDatabaseViews).toHaveBeenCalledOnce());
    chooseSecond();
    await tick();
    finish(views());
    await tick();
    await tick();
    expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("Second");
  });

  it("lists overflow views in a text-only panel whose handles reorder them with the arrow keys", async () => {
    persistViewOrder(views(["First", "Second", "Third", "Fourth"]));
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    const trigger = document.querySelector<HTMLButtonElement>('button[aria-label="1 more..."]')!;
    expect(trigger.querySelector("svg")).toBeNull();
    trigger.click();
    await vi.waitFor(() => expect(document.activeElement?.getAttribute("aria-label")).toBe("Search for a view..."));
    for (const name of ["First", "Second", "Third", "Fourth"]) {
      expect(document.querySelector(`button[aria-label="Drag view: ${name}"]`)).not.toBeNull();
      expect(document.querySelector(`button[aria-label="${name} actions"]`)).not.toBeNull();
    }
    const handle = document.querySelector<HTMLButtonElement>('button[aria-label="Drag view: First"]')!;
    handle.focus();
    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await vi.waitFor(() => expect(reorderNotesDatabaseViews).toHaveBeenCalledWith("database", ["Second", "First", "Third", "Fourth"]));
    await vi.waitFor(() => expect(document.activeElement).toBe(handle));
    expect(handle.closest("[draggable]")?.previousElementSibling?.querySelector('[aria-label="Drag view: Second"]')).not.toBeNull();
  });

  it("filters overflow views by name and offers new views and data sources", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views(["First", "Second", "Third", "Fourth"]));
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")).not.toBeNull());
    document.querySelector<HTMLButtonElement>('button[aria-label="1 more..."]')!.click();
    await tick();
    const search = document.querySelector<HTMLInputElement>('input[aria-label="Search for a view..."]')!;
    search.value = "four";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(document.querySelector('button[aria-label="Drag view: Fourth"]')).not.toBeNull();
    expect(document.querySelector('button[aria-label="Drag view: First"]')).toBeNull();
    search.value = "missing";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(document.body.textContent).toContain("No matching views");
    const labels = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).map((button) => button.textContent?.trim());
    expect(labels).toContain("New view");
    expect(labels).toContain("New data source");
  });

  it("reorders toolbar tabs by dragging one onto another", async () => {
    persistViewOrder(views());
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    const tab = (name: string) => document.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)!.closest<HTMLElement>("[draggable]")!;
    expect(tab("First").getAttribute("draggable")).toBe("true");
    tab("First").dispatchEvent(new MouseEvent("dragstart", { bubbles: true }));
    tab("Second").dispatchEvent(new MouseEvent("dragover", { bubbles: true, cancelable: true }));
    await tick();
    tab("Second").dispatchEvent(new MouseEvent("drop", { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(reorderNotesDatabaseViews).toHaveBeenCalledWith("database", ["Second", "First"]));
    await vi.waitFor(() => expect(document.querySelector('button[aria-label="Second"]')?.compareDocumentPosition(document.querySelector('button[aria-label="First"]')!))
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING));
  });

  it("keeps tabs in place while the database layout is locked", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    open(() => {}, { editingLocked: true });
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")).not.toBeNull());
    const first = document.querySelector<HTMLButtonElement>('button[aria-label="First"]')!.closest<HTMLElement>("[draggable]")!;
    expect(first.getAttribute("draggable")).toBe("false");
    const start = new MouseEvent("dragstart", { bubbles: true, cancelable: true });
    first.dispatchEvent(start);
    expect(start.defaultPrevented).toBe(true);
  });

  it("opens a panel row's actions for that view without selecting it", async () => {
    const saved = views(["First", "Second", "Third", "Fourth"]);
    const copied = { ...saved[3], id: "Copied", name: "Copied" };
    vi.mocked(listNotesDatabaseViews).mockResolvedValueOnce(saved).mockResolvedValue([...saved, copied]);
    vi.mocked(duplicateNotesDatabaseView).mockResolvedValue(copied);
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));
    document.querySelector<HTMLButtonElement>('button[aria-label="1 more..."]')!.click();
    await tick();
    document.querySelector<HTMLButtonElement>('button[aria-label="Fourth actions"]')!.click();
    await tick();
    expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First");
    const duplicate = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.trim() === "Duplicate view")!;
    duplicate.click();
    await vi.waitFor(() => expect(duplicateNotesDatabaseView).toHaveBeenCalledWith(expect.objectContaining({ source_view_id: "Fourth", name: "Fourth copy" })));
  });

  it("keeps the panel's rows enabled while successive reorders save, then shows the latest stored order", async () => {
    let current = views(["First", "Second", "Third", "Fourth"]);
    const byId = new Map(current.map((view) => [view.id, view]));
    const finishes: Array<() => void> = [];
    vi.mocked(listNotesDatabaseViews).mockImplementation(async () => current);
    vi.mocked(reorderNotesDatabaseViews).mockImplementation((_databaseId, ids) => new Promise((resolve) => {
      finishes.push(() => {
        current = ids.map((id) => byId.get(id)!);
        resolve(current);
      });
    }));
    open();
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")).not.toBeNull());
    document.querySelector<HTMLButtonElement>('button[aria-label="1 more..."]')!.click();
    await vi.waitFor(() => expect(document.activeElement?.getAttribute("aria-label")).toBe("Search for a view..."));
    const handle = document.querySelector<HTMLButtonElement>('button[aria-label="Drag view: First"]')!;
    const rowOrder = () => Array.from(document.querySelectorAll<HTMLButtonElement>('button[aria-label^="Drag view: "]')).map((button) => button.getAttribute("aria-label")!.slice("Drag view: ".length));
    handle.focus();
    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await vi.waitFor(() => expect(document.activeElement).toBe(handle));
    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await vi.waitFor(() => expect(rowOrder()).toEqual(["Second", "Third", "First", "Fourth"]));
    const rowButtons = Array.from(document.querySelectorAll<HTMLButtonElement>('button[aria-label^="Drag view: "]')).flatMap((grip) => Array.from(grip.parentElement!.querySelectorAll("button")));
    expect(rowButtons.length).toBeGreaterThan(8);
    expect(rowButtons.filter((button) => button.disabled)).toEqual([]);
    finishes.shift()!();
    await tick();
    await tick();
    expect(rowOrder()).toEqual(["Second", "Third", "First", "Fourth"]);
    await vi.waitFor(() => expect(finishes).toHaveLength(1));
    finishes.shift()!();
    await vi.waitFor(() => expect(current.map((view) => view.id)).toEqual(["Second", "Third", "First", "Fourth"]));
    expect(reorderNotesDatabaseViews).toHaveBeenLastCalledWith("database", ["Second", "Third", "First", "Fourth"]);
    expect(rowOrder()).toEqual(["Second", "Third", "First", "Fourth"]);
  });
});
