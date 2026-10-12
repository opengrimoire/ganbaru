// @vitest-environment jsdom
import { createRawSnippet, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesDataSourcePropertyType, NotesDataSourceTableView } from "$lib/notes/types";
import type { NotesDatabasePropertyActionRequest } from "$lib/notes/database/data-source-schema";
import {
  createNotesDataSourceRowPage, getNotesDataSourceTableView, listNotesDataSourceTemplates, loadNotesPage,
  createNotesDataSourceSubitem, duplicateNotesPage, trashNotesPage,
  updateNotesDataSourceRowProperty, updateNotesDataSourceTableView,
} from "$lib/api/notes";
import { createProvisionalNotesPage } from "$lib/notes/pages/creation";
import NotesDatabaseTableView from "./NotesDatabaseTableView.svelte";
import { notesDatabaseSession } from "$lib/notes/database/session.svelte";

vi.mock("$lib/api/notes", () => ({
  getNotesDataSourceTableView: vi.fn(),
  listNotesDataSourceTemplates: vi.fn(async () => []),
  updateNotesDataSourceTableView: vi.fn(),
  createNotesDataSourceRowPage: vi.fn(),
  createNotesDataSourceSubitem: vi.fn(),
  duplicateNotesPage: vi.fn(),
  trashNotesPage: vi.fn(),
  updateNotesDataSourceRowParent: vi.fn(),
  updateNotesDataSourceRowProperty: vi.fn(),
  applyNotesDataSourceTemplate: vi.fn(),
  loadNotesPage: vi.fn(),
}));

/** Minimal canonical table response with a single resizable title column. */
function table(width: number): NotesDataSourceTableView {
  const source = {
    source_provider: null, source_object_id: null, source_workspace_id: null,
    source_last_edited_time: null, created_time: "2026-09-29T00:00:00Z", last_edited_time: "2026-09-29T00:00:00Z",
  };
  return {
    data_source: {
      ...source, object: "data_source", id: "source", parent: { type: "database_id", database_id: "database" },
      database_parent: { type: "page_id", page_id: "page" }, title: "Tasks", title_rich_text: [], description: [],
      icon: null, in_trash: false, properties: { Name: { id: "title", name: "Name", type: "title", title: {} } },
    },
    view: {
      ...source, object: "view", id: "view", parent: { type: "database_id", database_id: "database" },
      data_source_id: "source", name: "Table", type: "table", filter: null, sorts: [], url: null,
      configuration: { type: "table", table: {
        property_order: ["title"], hidden_property_ids: [], column_widths: { title: width }, row_open_mode: "full_page",
        group_property_id: null, group_order: [], collapsed_group_ids: [], hide_empty_groups: false,
        presentation: { frozen_property_id: null, columns: {} },
      } },
    },
    rows: [], total_row_count: 0, has_more: false, next_cursor: null,
  };
}

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  notesDatabaseSession.clear();
  document.body.replaceChildren();
  vi.resetAllMocks();
});

/** Dispatch pointer coordinates using jsdom's mouse event implementation. */
function pointer(target: EventTarget, type: string, x: number): void {
  const event = new MouseEvent(type, { clientX: x, button: 0, bubbles: true, cancelable: true });
  Object.defineProperty(event, "pointerId", { value: 1 });
  target.dispatchEvent(event);
}

async function open(
  onSavingChange = vi.fn<(saving: boolean) => void>(),
  initialTable = table(240),
  onEditProperties = vi.fn<() => void>(),
  editingLocked = false,
) {
  const onLoadPropertyEditor = vi.fn<(propertyId: string) => void>();
  const propertyEditor = createRawSnippet<[string]>((propertyId) => ({ render: () => `<p data-property-editor>Editing ${propertyId()}</p>` }));
  vi.mocked(getNotesDataSourceTableView).mockResolvedValue(initialTable);
  vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
  vi.mocked(loadNotesPage).mockRejectedValue(new Error("Page not found"));
  component = mount(NotesDatabaseTableView, { target: document.body, props: {
    dataSourceId: "source", onSavingChange, onSelectPage: vi.fn(), onAddProperty: vi.fn(), onEditProperties, propertyEditor, onLoadPropertyEditor, onCloseSettings: vi.fn(), editingLocked,
  } });
  await tick();
  await tick();
  await vi.waitFor(() => expect(document.querySelector(".collection-resize")).not.toBeNull());
  const handle = document.querySelector<HTMLButtonElement>(".collection-resize")!;
  handle.setPointerCapture = vi.fn();
  const row = document.querySelector<HTMLElement>('[role="row"]')!;
  return { handle, onSavingChange, onEditProperties, onLoadPropertyEditor, width: () => row.style.getPropertyValue("--collection-columns") };
}

/** Include a hidden property between visible properties to exercise saved presentation. */
function configuredTable(): NotesDataSourceTableView {
  const initial = table(240);
  return {
    ...initial,
    data_source: { ...initial.data_source, properties: {
      ...initial.data_source.properties,
      Status: { id: "status", name: "Status", type: "status", status: { options: [] } },
      Internal: { id: "internal", name: "Internal", type: "rich_text", rich_text: {} },
      Done: { id: "done", name: "Done", type: "checkbox", checkbox: {} },
    } },
    view: { ...initial.view,
      filter: { type: "and", filters: [{ property_id: "title", condition: "contains", value: "Active" }] },
      sorts: [{ property_id: "title", direction: "ascending" }, { property_id: "status", direction: "ascending" }],
      configuration: { type: "table", table: {
        property_order: ["title", "status", "internal", "done"], hidden_property_ids: ["internal"],
        column_widths: { title: 240, status: 160, internal: 180, done: 120 }, row_open_mode: "full_page",
        group_property_id: null, group_order: [], collapsed_group_ids: [], hide_empty_groups: false,
        presentation: { frozen_property_id: null, columns: {} },
      } },
    },
  };
}

/** The header control that opens a column's menu. */
function headerTrigger(name: string): HTMLButtonElement {
  const trigger = document.querySelector<HTMLButtonElement>(`[role="columnheader"] button[aria-label="${name}"]`);
  expect(trigger).not.toBeNull();
  return trigger!;
}

function headerCell(propertyId: string): HTMLElement {
  const cell = document.querySelector<HTMLElement>(`[role="columnheader"][data-collection-column-id="${propertyId}"]`);
  expect(cell).not.toBeNull();
  return cell!;
}

function headerOrder(): string[] {
  return Array.from(document.querySelectorAll("[role='columnheader'][data-collection-column-id]"), (cell) => cell.getAttribute("data-collection-column-id") ?? "");
}

/** Place the header cells side by side from x = 0 at the given widths, since jsdom has no layout. */
function layOutHeader(widths: Record<string, number>): void {
  const row = headerCell("title").closest<HTMLElement>('[role="row"]')!;
  let left = 0;
  for (const cell of row.querySelectorAll<HTMLElement>("[data-collection-column-id]")) {
    const rect = new DOMRect(left, 0, widths[cell.getAttribute("data-collection-column-id") ?? ""] ?? 0, 32);
    cell.getBoundingClientRect = () => rect;
    left += rect.width;
  }
  const rowRect = new DOMRect(0, 0, left, 32);
  row.getBoundingClientRect = () => rowRect;
}

/** Open a property menu through its real table header control. */
async function propertyMenu(name: string): Promise<HTMLElement> {
  document.querySelector<HTMLButtonElement>(`[role="columnheader"] button[aria-label="${name}"]`)!.click();
  await tick();
  await tick();
  const panel = document.querySelector<HTMLElement>(`[role="dialog"][aria-label="${name}"]`);
  expect(panel).not.toBeNull();
  return panel!;
}

/** Find an action by its visible label within one floating panel. */
function action(panel: ParentNode, label: string): HTMLButtonElement {
  const button = Array.from(panel.querySelectorAll<HTMLButtonElement>("button"))
    .find((candidate) => candidate.textContent?.trim() === label);
  expect(button).toBeDefined();
  return button!;
}

/** Open a submenu of a floating panel by its accessible name and return the submenu panel. */
async function submenu(panel: ParentNode, label: string): Promise<HTMLElement> {
  const trigger = panel.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  expect(trigger).not.toBeNull();
  trigger!.click();
  await tick();
  await tick();
  const opened = document.querySelector<HTMLElement>(`[role="dialog"][aria-label="${label}"]`);
  expect(opened).not.toBeNull();
  return opened!;
}

describe("Notes table sub-items", () => {
  it("uses distinct keyboard indexes for the same row rendered in multiple groups", async () => {
    const initial = table(240);
    const options = [{ id: "first", name: "First", color: "default" }, { id: "second", name: "Second", color: "default" }];
    initial.data_source.properties.Tags = { id: "tags", name: "Tags", type: "multi_select", multi_select: { options } };
    const both = createProvisionalNotesPage({ id: "both", first_block_id: "block-both", title: "Both", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page;
    const second = createProvisionalNotesPage({ id: "second-only", first_block_id: "block-second", title: "Second only", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page;
    both.properties.Tags = { id: "tags", type: "multi_select", multi_select: options };
    second.properties.Tags = { id: "tags", type: "multi_select", multi_select: [options[1]] };
    initial.rows = [second, both];
    initial.view.configuration = { type: "table", table: { ...initial.view.configuration!.table as Record<string, unknown>,
      property_order: ["title", "tags"], group_property_id: "tags", group_order: ["first", "second"], hide_empty_groups: true,
    } };
    await open(vi.fn(), initial);
    const inputs = Array.from(document.querySelectorAll<HTMLInputElement>('[data-table-cell="true"][data-column-index="0"]'));
    expect(inputs.map((input) => input.value)).toEqual(["Both", "Second only", "Both"]);
    expect(inputs.map((input) => input.dataset.rowIndex)).toEqual(["0", "1", "2"]);
    inputs[1].focus();
    inputs[1].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(inputs[2]);
    inputs[2].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(inputs[1]);
  });
  it("opens a collapsed parent of a locked view for the session and adds a sub-item without writing view settings", async () => {
    const parentId = "11111111-1111-4111-8111-111111111111";
    const initial = table(240);
    initial.rows = [createProvisionalNotesPage({ id: parentId, first_block_id: "block", title: "Parent", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page];
    initial.row_hierarchy = { [parentId]: { parent_row_page_id: null, ancestor_row_page_ids: [], depth: 0, child_count: 1 } };
    initial.view.configuration = { type: "table", table: { ...initial.view.configuration!.table as Record<string, unknown>, collapsed_row_ids: [parentId] } };
    vi.mocked(createNotesDataSourceSubitem).mockImplementation(() => new Promise(() => {}));
    await open(vi.fn(), initial, vi.fn(), true);
    expect(document.querySelector(`[data-database-subitem-add-row="${parentId}"]`)).toBeNull();
    document.querySelector<HTMLButtonElement>('button[aria-label="Expand Parent"]')!.click();
    await tick();
    expect(document.querySelector<HTMLButtonElement>('button[aria-label="Collapse Parent"]')!.disabled).toBe(false);
    document.querySelector<HTMLButtonElement>(`[data-database-subitem-add-row="${parentId}"] button`)!.click();
    await vi.waitFor(() => expect(createNotesDataSourceSubitem).toHaveBeenCalledOnce());
    const id = vi.mocked(createNotesDataSourceSubitem).mock.calls[0][2].id;
    const input = document.querySelector<HTMLInputElement>(`[data-database-row-id="${id}"] input[aria-label="Name"]`);
    expect(input).not.toBeNull();
    await vi.waitFor(() => expect(document.activeElement).toBe(input));
    expect(updateNotesDataSourceTableView).not.toHaveBeenCalled();
    expect(initial.view.configuration!.table).toMatchObject({ collapsed_row_ids: [parentId] });
  });
  it("renders children below their parent, navigates visible rows, and persists collapse per saved view", async () => {
    const parentId = "11111111-1111-4111-8111-111111111111";
    const childId = "22222222-2222-4222-8222-222222222222";
    const otherId = "33333333-3333-4333-8333-333333333333";
    const initial = table(240);
    initial.rows = [createProvisionalNotesPage({ id: childId, first_block_id: "block-child", title: "Child", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page,
      createProvisionalNotesPage({ id: parentId, first_block_id: "block-parent", title: "Parent", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page,
      createProvisionalNotesPage({ id: otherId, first_block_id: "block-other", title: "Other", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page];
    initial.row_hierarchy = {
      [parentId]: { parent_row_page_id: null, ancestor_row_page_ids: [], depth: 0, child_count: 1 },
      [childId]: { parent_row_page_id: parentId, ancestor_row_page_ids: [parentId], depth: 1, child_count: 0 },
      [otherId]: { parent_row_page_id: null, ancestor_row_page_ids: [], depth: 0, child_count: 0 },
    };
    vi.mocked(updateNotesDataSourceTableView).mockImplementation(async (_source, update) => ({ ...initial,
      view: { ...initial.view, configuration: { type: "table", table: { ...update.configuration } } },
    }));
    await open(vi.fn(), initial);
    const inputs = () => Array.from(document.querySelectorAll<HTMLInputElement>('[data-table-cell="true"][data-column-index="0"]'));
    expect(inputs().map((input) => input.value)).toEqual(["Parent", "Child", "Other"]);
    inputs()[0].focus();
    inputs()[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(inputs()[1]);
    expect(document.querySelector<HTMLButtonElement>('button[aria-label="Collapse Child"]')).toBeNull();
    document.querySelector<HTMLButtonElement>('button[aria-label="Collapse Parent"]')!.click();
    await vi.waitFor(() => expect(inputs().map((input) => input.value)).toEqual(["Parent", "Other"]));
    expect(updateNotesDataSourceTableView).toHaveBeenLastCalledWith("source", expect.objectContaining({
      configuration: expect.objectContaining({ collapsed_row_ids: [parentId] }),
    }), { databaseId: null, viewId: null });
    inputs()[0].focus();
    inputs()[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(inputs()[1]);
  });

  it("opens a row from the Open button inside its title cell", async () => {
    const initial = table(240);
    initial.rows = [createProvisionalNotesPage({ id: "row", first_block_id: "block", title: "Plan", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page];
    const onSelectPage = vi.fn<(pageId: string) => void>();
    vi.mocked(getNotesDataSourceTableView).mockResolvedValue(initial);
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    component = mount(NotesDatabaseTableView, { target: document.body, props: {
      dataSourceId: "source", onSelectPage, onAddProperty: vi.fn(), onEditProperties: vi.fn(), onCloseSettings: vi.fn(),
    } });
    await vi.waitFor(() => expect(document.querySelector('[data-database-row-id="row"]')).not.toBeNull());
    const row = document.querySelector<HTMLElement>('[data-database-row-id="row"]')!;
    expect(row.querySelectorAll('[role="cell"]')).toHaveLength(4);
    const openButton = row.querySelector<HTMLButtonElement>('button[aria-label="Open Plan"]')!;
    expect(openButton.closest('[role="cell"]')?.querySelector('input[aria-label="Name"]')).not.toBeNull();
    expect(openButton.textContent?.trim()).toBe("Open");
    openButton.click();
    expect(onSelectPage).toHaveBeenCalledExactlyOnceWith("row");
  });

  it("adds a real sub-item from an opened row's add row and focuses its optimistic title without creating a root row", async () => {
    const initial = table(240);
    const parent = createProvisionalNotesPage({ id: "parent", first_block_id: "block", title: "Parent", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page;
    initial.rows = [parent];
    initial.row_hierarchy = { parent: { parent_row_page_id: null, ancestor_row_page_ids: [], depth: 0, child_count: 0 } };
    vi.mocked(createNotesDataSourceSubitem).mockImplementation(() => new Promise(() => {}));
    await open(vi.fn(), initial);
    expect(document.querySelector('[data-database-subitem-add-row="parent"]')).toBeNull();
    document.querySelector<HTMLButtonElement>('[data-database-row-id="parent"] button[aria-label="Expand Parent"]')!.click();
    await tick();
    const addRow = document.querySelector<HTMLElement>('[data-database-subitem-add-row="parent"]')!;
    expect(addRow.textContent).toContain("Add sub-item");
    addRow.querySelector<HTMLButtonElement>("button")!.click();
    await vi.waitFor(() => expect(createNotesDataSourceSubitem).toHaveBeenCalledOnce());
    const request = vi.mocked(createNotesDataSourceSubitem).mock.calls[0][2];
    const child = document.querySelector<HTMLElement>(`[data-database-row-id="${request.id}"]`)!;
    expect(child.dataset.databaseRowDepth).toBe("1");
    await vi.waitFor(() => expect(document.activeElement).toBe(child.querySelector('input[aria-label="Name"]')));
    expect(createNotesDataSourceRowPage).not.toHaveBeenCalled();
  });

  it("acts on selected rows from the selection bar and copies a selected subtree once", async () => {
    const page = (id: string, title: string) => createProvisionalNotesPage({ id, first_block_id: `block-${id}`, title, parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page;
    const initial = table(240);
    initial.rows = [page("parent", "Parent"), page("child", "Child"), page("other", "Other")];
    initial.row_hierarchy = {
      parent: { parent_row_page_id: null, ancestor_row_page_ids: [], depth: 0, child_count: 1 },
      child: { parent_row_page_id: "parent", ancestor_row_page_ids: ["parent"], depth: 1, child_count: 0 },
      other: { parent_row_page_id: null, ancestor_row_page_ids: [], depth: 0, child_count: 0 },
    };
    vi.mocked(duplicateNotesPage).mockResolvedValue({ page: initial.rows[0] } as Awaited<ReturnType<typeof duplicateNotesPage>>);
    vi.mocked(trashNotesPage).mockResolvedValue(initial.rows[0]);
    await open(vi.fn(), initial);
    expect(document.querySelector('[role="toolbar"]')).toBeNull();
    document.querySelector<HTMLInputElement>('[aria-label="Select Parent"]')!.click();
    document.querySelector<HTMLInputElement>('[aria-label="Select Child"]')!.click();
    await tick();
    const bar = document.querySelector<HTMLElement>('[role="toolbar"]')!;
    expect(bar.textContent).toContain("2 selected");
    bar.querySelector<HTMLButtonElement>('button[aria-label="More actions"]')!.click();
    await tick(); await tick();
    const menu = document.querySelector('[role="dialog"][aria-label="More actions"]')!;
    expect(Array.from(menu.querySelectorAll("button")).some((button) => button.textContent?.trim() === "Open")).toBe(false);
    action(menu, "Duplicate").click();
    await vi.waitFor(() => expect(duplicateNotesPage).toHaveBeenCalledExactlyOnceWith("parent", {}));
    await vi.waitFor(() => expect(Array.from(bar.querySelectorAll("button")).find((button) => button.textContent?.trim() === "Move to trash")!.disabled).toBe(false));
    Array.from(bar.querySelectorAll("button")).find((button) => button.textContent?.trim() === "Move to trash")!.click();
    await vi.waitFor(() => expect(trashNotesPage).toHaveBeenCalledTimes(2));
    expect(vi.mocked(trashNotesPage).mock.calls.map(([id]) => id)).toEqual(["parent", "child"]);
    await vi.waitFor(() => expect(document.querySelector('[role="toolbar"]')).toBeNull());
  });
});

describe("Notes table property actions", () => {
  it("changes an existing sort without duplicating it or resetting the saved view", async () => {
    const initial = configuredTable();
    vi.mocked(updateNotesDataSourceTableView).mockResolvedValue(initial);
    await open(vi.fn(), initial);
    action(await submenu(await propertyMenu("Status"), "Sort"), "Sort descending").click();
    await vi.waitFor(() => expect(updateNotesDataSourceTableView).toHaveBeenCalledOnce());
    const update = vi.mocked(updateNotesDataSourceTableView).mock.calls[0][1];
    expect(update.sorts).toEqual([
      { property_id: "title", direction: "ascending" },
      { property_id: "status", direction: "descending" },
    ]);
    expect(update.filter).toEqual([{ property_id: "title", condition: "contains", value: "Active" }]);
    const original = initial.view.configuration!.table;
    if (typeof original !== "object" || original === null || Array.isArray(original)) throw new Error("Table fixture must include an object configuration");
    expect(update.configuration).toMatchObject(original);
  });

  it("moves visible properties around hidden properties and keeps title first", async () => {
    const initial = configuredTable();
    vi.mocked(updateNotesDataSourceTableView).mockResolvedValue(initial);
    await open(vi.fn(), initial);
    action(await propertyMenu("Done"), "Move left").click();
    await vi.waitFor(() => expect(updateNotesDataSourceTableView).toHaveBeenCalledOnce());
    expect(vi.mocked(updateNotesDataSourceTableView).mock.calls[0][1].configuration).toMatchObject({
      property_order: ["title", "done", "status", "internal"], hidden_property_ids: ["internal"],
    });
  });

  it("slides a dragged header over its neighbors and moves the columns only on release", async () => {
    const initial = configuredTable();
    const saved = configuredTable();
    saved.view.configuration = { type: "table", table: { ...(initial.view.configuration!.table as Record<string, unknown>), property_order: ["title", "done", "status", "internal"] } };
    let finishSave!: (table: NotesDataSourceTableView) => void;
    vi.mocked(updateNotesDataSourceTableView).mockImplementation(() => new Promise((resolve) => { finishSave = resolve; }));
    await open(vi.fn(), initial);
    layOutHeader({ title: 240, status: 160, done: 120 });
    pointer(headerTrigger("Done"), "pointerdown", 450);
    pointer(window, "pointermove", 440);
    await tick();
    expect(headerCell("done").hasAttribute("data-collection-column-dragged")).toBe(true);
    expect(headerCell("done").getAttribute("style")).toContain("translateX(-10px)");
    expect(headerCell("status").getAttribute("style") ?? "").not.toContain("translateX");
    pointer(window, "pointermove", 360);
    await tick();
    expect(headerCell("done").getAttribute("style")).toContain("translateX(-90px)");
    expect(headerCell("status").getAttribute("style")).toContain("translateX(120px)");
    expect(headerOrder()).toEqual(["title", "status", "done"]);
    expect(updateNotesDataSourceTableView).not.toHaveBeenCalled();
    pointer(window, "pointerup", 360);
    await tick();
    expect(headerOrder()).toEqual(["title", "done", "status"]);
    expect(headerCell("done").getAttribute("style") ?? "").not.toContain("translateX");
    expect(updateNotesDataSourceTableView).toHaveBeenCalledOnce();
    expect(vi.mocked(updateNotesDataSourceTableView).mock.calls[0][1].configuration).toMatchObject({
      property_order: ["title", "done", "status", "internal"], hidden_property_ids: ["internal"],
    });
    headerTrigger("Done").click();
    await tick();
    expect(document.querySelector('[role="dialog"][aria-label="Done"]')).toBeNull();
    finishSave(saved);
    await vi.waitFor(() => expect(headerOrder()).toEqual(["title", "done", "status"]));
  });

  it("restores the saved column order when a dragged order fails to save", async () => {
    const initial = configuredTable();
    vi.mocked(updateNotesDataSourceTableView).mockRejectedValue(new Error("Disk is full"));
    await open(vi.fn(), initial);
    layOutHeader({ title: 240, status: 160, done: 120 });
    pointer(headerTrigger("Done"), "pointerdown", 450);
    pointer(window, "pointermove", 360);
    pointer(window, "pointerup", 360);
    await vi.waitFor(() => expect(updateNotesDataSourceTableView).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(headerOrder()).toEqual(["title", "status", "done"]));
    expect(document.body.textContent).toContain("Disk is full");
  });

  it("drags no column while editing is locked", async () => {
    await open(vi.fn(), configuredTable(), vi.fn(), true);
    layOutHeader({ title: 240, status: 160, done: 120 });
    pointer(headerTrigger("Done"), "pointerdown", 450);
    pointer(window, "pointermove", 300);
    await tick();
    expect(headerCell("done").hasAttribute("data-collection-column-dragged")).toBe(false);
    pointer(window, "pointerup", 300);
    expect(updateNotesDataSourceTableView).not.toHaveBeenCalled();
  });

  it("keeps the title column in place", async () => {
    await open(vi.fn(), configuredTable());
    layOutHeader({ title: 240, status: 160, done: 120 });
    pointer(headerTrigger("Name"), "pointerdown", 100);
    pointer(window, "pointermove", 450);
    await tick();
    expect(headerCell("title").hasAttribute("data-collection-column-dragged")).toBe(false);
    pointer(window, "pointerup", 450);
    expect(updateNotesDataSourceTableView).not.toHaveBeenCalled();
  });

  it("opens a column menu with a right click", async () => {
    await open(vi.fn(), configuredTable());
    const menu = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2 });
    headerTrigger("Status").dispatchEvent(menu);
    await tick();
    await tick();
    expect(menu.defaultPrevented).toBe(true);
    expect(document.querySelector('[role="dialog"][aria-label="Status"]')).not.toBeNull();
  });

  it("edits a property's options in a submenu beside the column menu without saving the view", async () => {
    const { onEditProperties, onLoadPropertyEditor } = await open(vi.fn(), configuredTable());
    const panel = await propertyMenu("Status");
    action(panel, "Edit property").click();
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"][aria-label="Edit property"] [data-property-editor]')?.textContent).toBe("Editing status"));
    expect(onLoadPropertyEditor).toHaveBeenCalledExactlyOnceWith("status");
    expect(panel.isConnected).toBe(true);
    expect(onEditProperties).not.toHaveBeenCalled();
    expect(updateNotesDataSourceTableView).not.toHaveBeenCalled();
  });

  it("offers no Edit property for a type without settings", async () => {
    const { onLoadPropertyEditor } = await open(vi.fn(), configuredTable());
    const panel = await propertyMenu("Done");
    expect(panel.querySelector('button[aria-label="Edit property"]')).toBeNull();
    expect(onLoadPropertyEditor).not.toHaveBeenCalled();
  });

  it("keeps the date and time formats of a date column under Edit property", async () => {
    const initial = configuredTable();
    initial.data_source.properties.Due = { id: "due", name: "Due", type: "date", date: {} };
    const tableConfiguration = initial.view.configuration!.table as Record<string, unknown>;
    initial.view.configuration = { type: "table", table: { ...tableConfiguration, property_order: ["title", "status", "internal", "done", "due"], column_widths: { title: 240, status: 160, internal: 180, done: 120, due: 160 } } };
    vi.mocked(updateNotesDataSourceTableView).mockResolvedValue(initial);
    const { onLoadPropertyEditor } = await open(vi.fn(), initial);
    const panel = await propertyMenu("Due");
    expect(panel.querySelector('button[aria-label="Date format"]')).toBeNull();
    const editor = await submenu(panel, "Edit property");
    expect(editor.querySelector("[data-property-editor]")).toBeNull();
    expect(onLoadPropertyEditor).not.toHaveBeenCalled();
    action(await submenu(editor, "Date format"), "ISO date").click();
    await vi.waitFor(() => expect(updateNotesDataSourceTableView).toHaveBeenCalledOnce());
    expect(vi.mocked(updateNotesDataSourceTableView).mock.calls[0][1].configuration).toMatchObject({
      presentation: { columns: { due: { date_format: "iso" } } },
    });
    expect(editor.querySelector('button[aria-label="Time format"]')).not.toBeNull();
  });

  it("groups the view by a property from its column menu and removes the grouping again", async () => {
    const initial = configuredTable();
    vi.mocked(updateNotesDataSourceTableView).mockImplementation(async (_source, update) => ({ ...initial,
      view: { ...initial.view, configuration: { type: "table", table: { ...update.configuration } } },
    }));
    await open(vi.fn(), initial);
    expect((await propertyMenu("Done")).querySelector('button[aria-label="Group"]')).toBeNull();
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    action(await propertyMenu("Status"), "Group").click();
    await vi.waitFor(() => expect(updateNotesDataSourceTableView).toHaveBeenCalledOnce());
    expect(vi.mocked(updateNotesDataSourceTableView).mock.calls[0][1].configuration).toMatchObject({ group_property_id: "status" });
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"][aria-label="Status"]')).toBeNull());
    action(await propertyMenu("Status"), "Remove grouping").click();
    await vi.waitFor(() => expect(updateNotesDataSourceTableView).toHaveBeenCalledTimes(2));
    expect(vi.mocked(updateNotesDataSourceTableView).mock.calls[1][1].configuration).toMatchObject({ group_property_id: null });
  });

  it("loads the chosen property for its editor and hides only that view column", async () => {
    const initial = configuredTable();
    vi.mocked(updateNotesDataSourceTableView).mockResolvedValue(initial);
    const { onLoadPropertyEditor } = await open(vi.fn(), initial);
    const panel = await propertyMenu("Status");
    action(panel, "Edit property").click();
    await vi.waitFor(() => expect(onLoadPropertyEditor).toHaveBeenCalledExactlyOnceWith("status"));
    action(panel, "Hide").click();
    await vi.waitFor(() => expect(updateNotesDataSourceTableView).toHaveBeenCalledOnce());
    expect(vi.mocked(updateNotesDataSourceTableView).mock.calls[0][1].configuration.hidden_property_ids)
      .toEqual(["status", "internal"]);
  });

  it("adds a checkbox filter for the chosen property without replacing existing filters", async () => {
    const initial = configuredTable();
    vi.mocked(updateNotesDataSourceTableView).mockResolvedValue(initial);
    await open(vi.fn(), initial);
    const panel = await propertyMenu("Done");
    action(panel, "Filter").click();
    await tick();
    await tick();
    const filterPanel = document.querySelector<HTMLElement>('[role="dialog"][aria-label="Filter"]')!;
    action(filterPanel, "Add filter").click();
    await vi.waitFor(() => expect(updateNotesDataSourceTableView).toHaveBeenCalledOnce());
    expect(vi.mocked(updateNotesDataSourceTableView).mock.calls[0][1].filter).toEqual([
      { property_id: "title", condition: "contains", value: "Active" },
      { property_id: "done", condition: "checked", value: null },
    ]);
  });
});

describe("Notes table width handoff", () => {
  it("reveals ready rows before an unrelated template read finishes", async () => {
    const onReady = vi.fn();
    let finishTemplates!: (value: []) => void;
    vi.mocked(getNotesDataSourceTableView).mockResolvedValue(table(240));
    vi.mocked(listNotesDataSourceTemplates).mockImplementationOnce(() => new Promise((resolve) => { finishTemplates = resolve; }));
    component = mount(NotesDatabaseTableView, { target: document.body, props: {
      dataSourceId: "source", onReady, onSelectPage: vi.fn(), onAddProperty: vi.fn(), onEditProperties: vi.fn(), onCloseSettings: vi.fn(),
    } });
    await vi.waitFor(() => expect(document.querySelector(".collection-resize")).not.toBeNull());
    expect(onReady).toHaveBeenCalled();
    expect(document.querySelector('[data-notes-skeleton="table"]')).toBeNull();
    finishTemplates([]);
    await tick(); await tick();
    expect(getNotesDataSourceTableView).toHaveBeenCalledOnce();
    expect(listNotesDataSourceTemplates).toHaveBeenCalledOnce();
  });

  it("shows a table skeleton during the first row read and removes it as soon as rows arrive", async () => {
    let finish!: (view: NotesDataSourceTableView) => void;
    vi.mocked(getNotesDataSourceTableView).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    component = mount(NotesDatabaseTableView, { target: document.body, props: {
      dataSourceId: "source", onSelectPage: vi.fn(), onAddProperty: vi.fn(), onEditProperties: vi.fn(), onCloseSettings: vi.fn(),
    } });
    await vi.waitFor(() => expect(getNotesDataSourceTableView).toHaveBeenCalledOnce());
    expect(document.querySelector('[data-notes-skeleton="table"]')).not.toBeNull();
    expect(document.querySelector('[role="table"]')).toBeNull();
    finish(table(240));
    await vi.waitFor(() => expect(document.querySelector(".collection-resize")).not.toBeNull());
    expect(document.querySelector('[data-notes-skeleton="table"]')).toBeNull();
  });

  it("restores rows and horizontal position without querying again after a tab switch", async () => {
    await open();
    const viewport = document.querySelector<HTMLElement>(".overflow-x-auto")!;
    viewport.scrollLeft = 160;
    viewport.dispatchEvent(new Event("scroll"));
    await unmount(component!);
    component = undefined;
    document.body.replaceChildren();
    await open();
    expect(getNotesDataSourceTableView).toHaveBeenCalledOnce();
    expect(listNotesDataSourceTemplates).toHaveBeenCalledOnce();
    expect(document.querySelector<HTMLElement>(".overflow-x-auto")?.scrollLeft).toBe(160);
  });

  it("keeps the table visible during refresh and applies writes from another view", async () => {
    const { width } = await open();
    let finish: (view: NotesDataSourceTableView) => void = () => {};
    vi.mocked(getNotesDataSourceTableView).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    notesDatabaseSession.invalidate();
    await vi.waitFor(() => expect(getNotesDataSourceTableView).toHaveBeenCalledTimes(2));
    expect(document.querySelector('[data-notes-skeleton="table"]')).toBeNull();
    expect(width()).toContain("240px");
    expect(document.querySelector(".collection-resize")).not.toBeNull();
    finish(table(320));
    await vi.waitFor(() => expect(width()).toContain("320px"));
  });

  it("defers remote refresh while a cell is being edited so typing cannot be overwritten", async () => {
    const page = created({ id: "existing", first_block_id: "block", title: "Original" }).page;
    await open(vi.fn(), { ...table(240), rows: [page], total_row_count: 1 });
    const input = titleInput();
    input.focus();
    typeTitle(input, "My draft");
    notesDatabaseSession.invalidate();
    await tick();
    await tick();
    expect(input.value).toBe("My draft");
    expect(getNotesDataSourceTableView).toHaveBeenCalledOnce();
    vi.mocked(updateNotesDataSourceRowProperty).mockResolvedValue(page);
    input.blur();
    await vi.waitFor(() => expect(updateNotesDataSourceRowProperty).toHaveBeenCalledWith("source", "existing", {
      property_id: "title", value: "My draft",
    }));
    await vi.waitFor(() => expect(getNotesDataSourceTableView).toHaveBeenCalledTimes(2));
  });

  it("keeps the drag width through a delayed save and accepts its canonical response", async () => {
    let complete: (view: NotesDataSourceTableView) => void = () => {};
    vi.mocked(updateNotesDataSourceTableView).mockImplementation(() => new Promise((resolve) => { complete = resolve; }));
    const { handle, onSavingChange, width } = await open();
    pointer(handle, "pointerdown", 100);
    pointer(window, "pointermove", 180);
    await tick();
    expect(width()).toContain("320px");
    pointer(window, "pointerup", 180);
    await tick();
    expect(width()).toContain("320px");
    expect(updateNotesDataSourceTableView).toHaveBeenCalledTimes(1);
    expect(onSavingChange).toHaveBeenLastCalledWith(true);
    expect(document.body.textContent).not.toContain("Saving table");
    complete(table(316));
    await vi.waitFor(() => {
      expect(width()).toContain("316px");
      expect(onSavingChange).toHaveBeenLastCalledWith(false);
    });
  });

  it("restores the saved width after cancellation or a rejected keyboard resize", async () => {
    vi.mocked(updateNotesDataSourceTableView).mockRejectedValue(new Error("Cannot save width"));
    const { handle, width } = await open();
    pointer(handle, "pointerdown", 100);
    pointer(window, "pointermove", 180);
    pointer(window, "pointercancel", 180);
    await tick();
    expect(width()).toContain("240px");
    expect(updateNotesDataSourceTableView).not.toHaveBeenCalled();
    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    await tick();
    await tick();
    expect(width()).toContain("240px");
    await vi.waitFor(() => {
      expect(width()).toContain("240px");
      expect(document.querySelector('[role="alert"]')?.textContent).toContain("Cannot save width");
    });
  });
});

/** Canonical creation response for the row reserved by the table. */
function created(request: { id: string; first_block_id: string; title: string }) {
  return createProvisionalNotesPage({ ...request,
    parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null });
}

describe("saved grouped table presentation", () => {
  it("keeps frozen offsets aligned with the live resized tracks while a save is pending", async () => {
    const initial = configuredTable();
    initial.view.configuration = { type: "table", table: { ...initial.view.configuration!.table as Record<string, unknown>, presentation: { frozen_property_id: "status", columns: {} } } };
    vi.mocked(updateNotesDataSourceTableView).mockImplementation(() => new Promise(() => {}));
    const { handle } = await open(vi.fn(), initial);
    const statusHeader = document.querySelector<HTMLElement>('[role="columnheader"] button[aria-label="Status"]')!.closest<HTMLElement>('[role="columnheader"]')!;
    expect(statusHeader.style.left).toBe("240px");
    pointer(handle, "pointerdown", 200);
    pointer(window, "pointermove", 240);
    await tick();
    expect(statusHeader.style.left).toBe("280px");
    pointer(window, "pointerup", 240);
    await tick();
    expect(statusHeader.style.left).toBe("280px");
    expect(updateNotesDataSourceTableView).toHaveBeenCalledOnce();
  });

  it("allows wrapped read-only values to expand their row naturally", async () => {
    const initial = table(240);
    initial.data_source.properties.Summary = { id: "summary", name: "Summary", type: "formula", formula: { expression: "\"Summary\"" } };
    const page = created({ id: "row", first_block_id: "block", title: "Task" }).page;
    page.properties.Summary = { id: "summary", type: "formula", formula: { type: "string", string: "First line\nSecond line" } };
    initial.rows = [page];
    initial.view.configuration = { type: "table", table: { property_order: ["title", "summary"], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page", presentation: { columns: { summary: { wrap: true } } } } };
    await open(vi.fn(), initial);
    const cell = document.querySelector<HTMLElement>('[data-database-row-id="row"] button[data-column-index="1"]')!;
    expect(cell.textContent).toContain("First line\nSecond line");
    expect(cell.classList.contains("h-8")).toBe(false);
    expect(cell.classList.contains("min-h-8")).toBe(true);
    expect(cell.querySelector(".truncate")).toBeNull();
  });

  it("protects view settings while revealing a new row in a collapsed locked group", async () => {
    const initial = table(240);
    initial.data_source.properties.Status = { id: "status", name: "Status", type: "status", status: { options: [{ id: "todo", name: "To do", color: "blue" }] } };
    initial.group_counts = { todo: 3 };
    initial.view.configuration = { type: "table", table: {
      property_order: ["title", "status"], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page",
      group_property_id: "status", group_order: [], collapsed_group_ids: ["todo"], hide_empty_groups: true,
    } };
    vi.mocked(createNotesDataSourceRowPage).mockImplementation(() => new Promise(() => {}));
    const { handle } = await open(vi.fn(), initial, vi.fn(), true);
    expect(handle.disabled).toBe(true);
    expect(document.querySelector<HTMLButtonElement>('[role="columnheader"] button[aria-label="Name"]')!.disabled).toBe(true);
    const group = document.querySelector<HTMLElement>('[data-table-group-id="todo"]')!;
    expect(group.querySelector<HTMLButtonElement>("button[aria-expanded]")!.disabled).toBe(true);
    group.querySelector<HTMLButtonElement>('button[aria-label="New page"]')!.click();
    await vi.waitFor(() => expect(createNotesDataSourceRowPage).toHaveBeenCalledOnce());
    const id = vi.mocked(createNotesDataSourceRowPage).mock.calls[0][1].id;
    const input = document.querySelector<HTMLInputElement>(`[data-database-row-id="${id}"] input[aria-label="Name"]`);
    expect(input).not.toBeNull();
    await vi.waitFor(() => expect(document.activeElement).toBe(input));
    expect(updateNotesDataSourceTableView).not.toHaveBeenCalled();
  });

  it("shows unloaded group counts and complete-source calculations and saves collapse independently", async () => {
    const initial = table(240);
    initial.data_source.properties.Status = { id: "status", name: "Status", type: "status", status: { options: [
      { id: "todo", name: "To do", color: "blue" }, { id: "done", name: "Done", color: "green" },
    ] } };
    const page = created({ id: "row", first_block_id: "block", title: "Loaded row" }).page;
    page.properties.Status = { id: "status", type: "status", status: { id: "done", name: "Done", color: "green" } };
    initial.rows = [page]; initial.total_row_count = 8; initial.group_counts = { todo: 5, done: 3 };
    initial.calculations = { overall: { title: 8 }, groups: { todo: { title: 5 }, done: { title: 3 } } };
    initial.view.configuration = { type: "table", table: { property_order: ["title", "status"], hidden_property_ids: [], column_widths: { title: 240, status: 180 }, row_open_mode: "full_page",
      group_property_id: "status", group_order: ["todo", "done"], collapsed_group_ids: [], hide_empty_groups: true,
      presentation: { frozen_property_id: "title", columns: { title: { wrap: true, calculation: "count_all" } } },
    } };
    vi.mocked(updateNotesDataSourceTableView).mockResolvedValue(initial);
    await open(vi.fn(), initial);
    expect(document.querySelector('[data-table-group-id="todo"]')?.textContent).toContain("5");
    expect(document.querySelector('[data-table-group-id="done"]')?.textContent).toContain("3");
    expect(document.querySelector('[data-table-calculation-group="__all__"]')?.textContent).toContain("Count all 8");
    expect(document.querySelectorAll('[data-database-row-id]')).toHaveLength(1);
    expect(document.querySelector<HTMLTextAreaElement>('textarea[aria-label="Name"]')?.value).toBe("Loaded row");
    const frozenCell = document.querySelector<HTMLElement>('[data-database-row-id] .collection-cell');
    expect(frozenCell?.style.left).toBe("0px");
    document.querySelector<HTMLButtonElement>('[data-table-group-id="todo"] button[aria-expanded]')!.click();
    await vi.waitFor(() => expect(updateNotesDataSourceTableView).toHaveBeenCalledOnce());
    expect(vi.mocked(updateNotesDataSourceTableView).mock.calls[0][1].configuration).toMatchObject({
      collapsed_group_ids: ["todo"], group_property_id: "status", presentation: { frozen_property_id: "title", columns: { title: { wrap: true, calculation: "count_all" } } },
    });
  });

  it("applies typed conditional row colors without changing row values", async () => {
    const initial = table(240);
    initial.data_source.properties.Estimate = { id: "estimate", name: "Estimate", type: "number", number: { format: "number" } };
    const low = created({ id: "low", first_block_id: "low-block", title: "Low" }).page;
    const high = created({ id: "high", first_block_id: "high-block", title: "High" }).page;
    low.properties.Estimate = { id: "estimate", type: "number", number: 2 };
    high.properties.Estimate = { id: "estimate", type: "number", number: 12 };
    initial.rows = [low, high]; initial.total_row_count = 2;
    initial.view.configuration = { type: "table", table: { property_order: ["title", "estimate"], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page",
      presentation: { columns: {}, frozen_property_id: null, color_rules: [{ id: "large", property_id: null, color: "red", filters: [{ property_id: "estimate", condition: "greater_than", value: 10 }] }] },
    } };
    await open(vi.fn(), initial);
    expect(document.querySelector<HTMLElement>('[data-database-row-id="low"]')?.style.getPropertyValue("--notes-block-bg")).toBe("");
    expect(document.querySelector<HTMLElement>('[data-database-row-id="high"]')?.style.getPropertyValue("--notes-block-bg")).not.toBe("");
    expect(updateNotesDataSourceRowProperty).not.toHaveBeenCalled();
  });
});

function newPage(): HTMLButtonElement {
  return document.querySelector<HTMLButtonElement>("[data-database-new-row] button")!;
}

function titleInput(index = 0): HTMLInputElement {
  return document.querySelectorAll<HTMLInputElement>('[data-table-cell="true"]')[index]!;
}

function typeTitle(input: HTMLInputElement, title: string): void {
  input.value = title;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("immediate Notes table creation", () => {
  it("reserves editable blank rows before creation completes and keeps New page available", async () => {
    vi.mocked(createNotesDataSourceRowPage).mockImplementation(() => new Promise(() => {}));
    await open();
    const originalAddButton = newPage();
    const originalAddRow = originalAddButton.closest(".collection-row");
    originalAddButton.click();
    await tick();
    expect(originalAddButton.isConnected).toBe(false);
    expect(originalAddRow?.isConnected).toBe(false);
    expect(newPage()).not.toBe(originalAddButton);
    expect(titleInput().value).toBe("");
    expect(titleInput().disabled).toBe(false);
    expect(document.activeElement).toBe(titleInput());
    expect(newPage().disabled).toBe(false);
    expect(document.querySelector("form")).toBeNull();
    newPage().click();
    await tick();
    expect(document.querySelectorAll('[data-database-row-id]')).toHaveLength(2);
    expect(createNotesDataSourceRowPage).toHaveBeenCalledTimes(2);
    const requests = vi.mocked(createNotesDataSourceRowPage).mock.calls.map(([, request]) => request);
    expect(requests.every((request) => request.title === "")).toBe(true);
    expect(requests[0].id).not.toBe(requests[1].id);
    expect(getNotesDataSourceTableView).toHaveBeenCalledTimes(1);
  });

  it("keeps typing through creation and saves Enter on the last row without a draft-row flash", async () => {
    let finish: () => void = () => {};
    vi.mocked(createNotesDataSourceRowPage).mockImplementation((_source, request) => new Promise((resolve) => {
      finish = () => resolve(created(request));
    }));
    vi.mocked(updateNotesDataSourceRowProperty).mockImplementation(async (_source, id, update) => {
      const page = created({ id, first_block_id: "block", title: String(update.value) }).page;
      vi.mocked(getNotesDataSourceTableView).mockResolvedValue({ ...table(240), rows: [page], total_row_count: 1 });
      return page;
    });
    await open();
    newPage().click();
    await tick();
    const input = titleInput();
    const nextAddButton = newPage();
    typeTitle(input, "My new row");
    await tick();
    finish();
    await tick();
    await tick();
    expect(titleInput()).toBe(input);
    expect(input.value).toBe("My new row");
    expect(document.activeElement).toBe(input);
    expect(updateNotesDataSourceRowProperty).not.toHaveBeenCalled();
    expect(getNotesDataSourceTableView).toHaveBeenCalledTimes(1);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(updateNotesDataSourceRowProperty).toHaveBeenCalledWith("source", expect.any(String), {
      property_id: "title", value: "My new row",
    }));
    expect(document.querySelectorAll('[data-database-row-id]')).toHaveLength(1);
    expect(newPage()).not.toBeNull();
    await vi.waitFor(() => {
      expect(getNotesDataSourceTableView).toHaveBeenCalledTimes(2);
      expect(newPage().disabled).toBe(false);
    });
    expect(newPage()).toBe(nextAddButton);
    expect(document.activeElement).toBe(nextAddButton);
    expect(listNotesDataSourceTemplates).toHaveBeenCalledTimes(1);
  });

  it("queues a title submitted before creation finishes and retains it through failure and retry", async () => {
    let reject: (error: Error) => void = () => {};
    vi.mocked(createNotesDataSourceRowPage).mockImplementationOnce(() => new Promise((_resolve, fail) => { reject = fail; }));
    vi.mocked(createNotesDataSourceRowPage).mockImplementation(async (_source, request) => created(request));
    vi.mocked(updateNotesDataSourceRowProperty).mockImplementation(async (_source, id, update) => {
      const page = created({ id, first_block_id: "block", title: String(update.value) }).page;
      vi.mocked(getNotesDataSourceTableView).mockResolvedValue({ ...table(240), rows: [page], total_row_count: 1 });
      return page;
    });
    await open();
    newPage().click();
    await tick();
    const input = titleInput();
    typeTitle(input, "Keep this title");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    expect(updateNotesDataSourceRowProperty).not.toHaveBeenCalled();
    reject(new Error("Create failed"));
    await vi.waitFor(() => expect(document.querySelector('[role="alert"]')?.textContent).toContain("Create failed"));
    expect(titleInput().value).toBe("Keep this title");
    document.querySelector<HTMLButtonElement>('[role="alert"] button')!.click();
    await vi.waitFor(() => expect(updateNotesDataSourceRowProperty).toHaveBeenCalledWith("source", expect.any(String), {
      property_id: "title", value: "Keep this title",
    }));
    const calls = vi.mocked(createNotesDataSourceRowPage).mock.calls;
    expect(calls).toHaveLength(2);
    expect(calls[0][1]).toEqual(calls[1][1]);
    expect(document.querySelectorAll('[data-database-row-id]')).toHaveLength(1);
  });
});

describe("Notes table property drafts", () => {
  /** Mount the table with property callbacks the draft header reports to. */
  async function openWithPropertyCallbacks() {
    const onAddProperty = vi.fn<(type: NotesDataSourcePropertyType, name: string, icon: string | null) => Promise<void>>().mockResolvedValue(undefined);
    const onPropertyAction = vi.fn<(request: NotesDatabasePropertyActionRequest) => Promise<void>>().mockResolvedValue(undefined);
    vi.mocked(getNotesDataSourceTableView).mockResolvedValue(configuredTable());
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    component = mount(NotesDatabaseTableView, { target: document.body, props: {
      dataSourceId: "source", onSelectPage: vi.fn(), onAddProperty, onPropertyAction, onEditProperties: vi.fn(), onCloseSettings: vi.fn(),
    } });
    await vi.waitFor(() => expect(document.querySelector('[role="columnheader"] button[aria-label="Name"]')).not.toBeNull());
    return { onAddProperty, onPropertyAction };
  }

  /** Wait for the draft header to take focus, then name the property and choose its type. */
  async function fillDraft(name: string, type: NotesDataSourcePropertyType): Promise<HTMLElement> {
    await vi.waitFor(() => expect(document.activeElement).toBe(document.querySelector('[data-collection-property-draft] [aria-label="Property name"]')));
    const draft = document.querySelector<HTMLElement>("[data-collection-property-draft]")!;
    const input = draft.querySelector<HTMLInputElement>('[aria-label="Property name"]')!;
    input.value = name;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    draft.querySelector<HTMLButtonElement>(`[data-property-type="${type}"]`)!.click();
    return draft;
  }

  it("adds a named property from the header add button and returns focus to it", async () => {
    const { onAddProperty, onPropertyAction } = await openWithPropertyCallbacks();
    const add = document.querySelector<HTMLButtonElement>('button[aria-label="Add property"]')!;
    add.click();
    const draft = await fillDraft("Estimate", "number");
    await vi.waitFor(() => expect(onAddProperty).toHaveBeenCalledExactlyOnceWith("number", "Estimate", null));
    await vi.waitFor(() => expect(draft.isConnected).toBe(false));
    expect(document.activeElement).toBe(add);
    expect(onPropertyAction).not.toHaveBeenCalled();
  });

  it("keeps an insert draft open with the rejection and inserts beside the column on retry", async () => {
    const { onAddProperty, onPropertyAction } = await openWithPropertyCallbacks();
    onPropertyAction.mockRejectedValueOnce(new Error("Disk full"));
    action(await propertyMenu("Status"), "Insert left").click();
    const draft = await fillDraft("Owner", "people");
    await vi.waitFor(() => expect(draft.querySelector('[role="alert"]')?.textContent).toBe("Disk full"));
    expect(draft.isConnected).toBe(true);
    expect(document.querySelector('[role="dialog"][aria-label="Status"]')).toBeNull();
    draft.querySelector<HTMLButtonElement>('[data-property-type="people"]')!.click();
    await vi.waitFor(() => expect(draft.isConnected).toBe(false));
    expect(onPropertyAction).toHaveBeenLastCalledWith({ type: "insert", propertyId: "status", side: "left", propertyType: "people", name: "Owner", icon: null });
    expect(onAddProperty).not.toHaveBeenCalled();
  });

  it("keeps the title renameable without type, hide, move, duplicate, or delete actions", async () => {
    await openWithPropertyCallbacks();
    const panel = await propertyMenu("Name");
    expect(panel.querySelector('input[aria-label="Property name"]')).not.toBeNull();
    for (const label of ["Edit property", "Change type", "Hide", "Move left", "Move right", "Insert left", "Duplicate property (empty)", "Delete property"]) {
      expect(panel.querySelector(`button[aria-label="${label}"]`), label).toBeNull();
      expect(Array.from(panel.querySelectorAll("button")).some((button) => button.textContent?.trim() === label), label).toBe(false);
    }
    expect(action(panel, "Insert right")).toBeDefined();
  });

  it("changes a property type from the type list without offering the title type", async () => {
    const { onPropertyAction } = await openWithPropertyCallbacks();
    const types = await submenu(await propertyMenu("Status"), "Change type");
    expect(types.querySelector('[aria-pressed="true"]')?.textContent?.trim()).toBe("Status");
    expect(Array.from(types.querySelectorAll("button")).some((button) => button.textContent?.trim() === "Title")).toBe(false);
    action(types, "Number").click();
    await vi.waitFor(() => expect(onPropertyAction).toHaveBeenCalledExactlyOnceWith({ type: "changeType", propertyId: "status", propertyType: "number" }));
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"][aria-label="Status"]')).toBeNull());
  });

  it("deletes a property from its column menu and shows a rejection", async () => {
    const { onPropertyAction } = await openWithPropertyCallbacks();
    onPropertyAction.mockRejectedValueOnce(new Error("Disk full"));
    action(await propertyMenu("Status"), "Delete property").click();
    await vi.waitFor(() => expect(onPropertyAction).toHaveBeenCalledExactlyOnceWith({ type: "delete", propertyId: "status" }));
    await vi.waitFor(() => expect(document.body.textContent).toContain("Disk full"));
  });

  it("discards the draft on Escape without creating a property", async () => {
    const { onAddProperty } = await openWithPropertyCallbacks();
    const add = document.querySelector<HTMLButtonElement>('button[aria-label="Add property"]')!;
    add.click();
    await vi.waitFor(() => expect(document.activeElement).toBe(document.querySelector('[data-collection-property-draft] [aria-label="Property name"]')));
    document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(document.querySelector("[data-collection-property-draft]")).toBeNull());
    expect(document.activeElement).toBe(add);
    expect(onAddProperty).not.toHaveBeenCalled();
  });
});
