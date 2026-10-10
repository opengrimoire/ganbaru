// @vitest-environment jsdom
import { createRawSnippet, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesDataSourcePropertyType, NotesDataSourceTableView } from "$lib/notes/types";
import type { NotesDatabasePropertyActionRequest } from "$lib/notes/database/data-source-schema";
import {
  createNotesDataSourceRowPage, getNotesDataSourceTableView, listNotesDataSourceTemplates, loadNotesPage,
  createNotesDataSourceSubitem,
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
  it("temporarily reveals a sub-item created under a collapsed locked parent without writing view settings", async () => {
    const parentId = "11111111-1111-4111-8111-111111111111";
    const initial = table(240);
    initial.rows = [createProvisionalNotesPage({ id: parentId, first_block_id: "block", title: "Parent", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page];
    initial.row_hierarchy = { [parentId]: { parent_row_page_id: null, ancestor_row_page_ids: [], depth: 0, child_count: 1 } };
    initial.view.configuration = { type: "table", table: { ...initial.view.configuration!.table as Record<string, unknown>, collapsed_row_ids: [parentId] } };
    vi.mocked(createNotesDataSourceSubitem).mockImplementation(() => new Promise(() => {}));
    await open(vi.fn(), initial, vi.fn(), true);
    expect(document.querySelector<HTMLButtonElement>('[aria-label="Expand sub-items of Parent"]')!.disabled).toBe(true);
    document.querySelector<HTMLButtonElement>(`[data-database-row-id="${parentId}"] button[aria-label="Row actions"]`)!.click();
    await tick(); await tick();
    action(document.querySelector('[role="dialog"][aria-label="Row actions"]')!, "Add sub-item").click();
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
    document.querySelector<HTMLButtonElement>('[aria-label="Collapse sub-items of Parent"]')!.click();
    await vi.waitFor(() => expect(inputs().map((input) => input.value)).toEqual(["Parent", "Other"]));
    expect(updateNotesDataSourceTableView).toHaveBeenLastCalledWith("source", expect.objectContaining({
      configuration: expect.objectContaining({ collapsed_row_ids: [parentId] }),
    }), { databaseId: null, viewId: null });
    inputs()[0].focus();
    inputs()[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(inputs()[1]);
  });

  it("adds a real sub-item through the row menu and focuses its optimistic title without creating a root row", async () => {
    const initial = table(240);
    const parent = createProvisionalNotesPage({ id: "parent", first_block_id: "block", title: "Parent", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page;
    initial.rows = [parent];
    initial.row_hierarchy = { parent: { parent_row_page_id: null, ancestor_row_page_ids: [], depth: 0, child_count: 0 } };
    vi.mocked(createNotesDataSourceSubitem).mockImplementation(() => new Promise(() => {}));
    await open(vi.fn(), initial);
    document.querySelector<HTMLButtonElement>('[data-database-row-id="parent"] button[aria-label="Row actions"]')!.click();
    await tick(); await tick();
    action(document.querySelector('[role="dialog"][aria-label="Row actions"]')!, "Add sub-item").click();
    await vi.waitFor(() => expect(createNotesDataSourceSubitem).toHaveBeenCalledOnce());
    const request = vi.mocked(createNotesDataSourceSubitem).mock.calls[0][2];
    const child = document.querySelector<HTMLElement>(`[data-database-row-id="${request.id}"]`)!;
    expect(child.dataset.databaseRowDepth).toBe("1");
    await vi.waitFor(() => expect(document.activeElement).toBe(child.querySelector('input[aria-label="Name"]')));
    expect(createNotesDataSourceRowPage).not.toHaveBeenCalled();
  });
});

describe("Notes table property actions", () => {
  it("changes an existing sort without duplicating it or resetting the saved view", async () => {
    const initial = configuredTable();
    vi.mocked(updateNotesDataSourceTableView).mockResolvedValue(initial);
    await open(vi.fn(), initial);
    action(await propertyMenu("Status"), "Sort descending").click();
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
      property_order: ["title", "done", "internal", "status"], hidden_property_ids: ["internal"],
    });
  });

  it("keeps the required title visible and edits its property in a submenu beside the column menu", async () => {
    const { onEditProperties, onLoadPropertyEditor } = await open(vi.fn(), configuredTable());
    const panel = await propertyMenu("Name");
    expect(panel.textContent).not.toContain("Move left");
    expect(panel.textContent).not.toContain("Move right");
    expect(panel.textContent).not.toContain("Hide");
    action(panel, "Edit property").click();
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"][aria-label="Edit property"] [data-property-editor]')?.textContent).toBe("Editing title"));
    expect(onLoadPropertyEditor).toHaveBeenCalledExactlyOnceWith("title");
    expect(panel.isConnected).toBe(true);
    expect(onEditProperties).not.toHaveBeenCalled();
    expect(updateNotesDataSourceTableView).not.toHaveBeenCalled();
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
