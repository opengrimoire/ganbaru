// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyNotesDataSourcePropertyAction,
  getNotesDatabaseReference,
  getNotesDataSourceSchema,
  getNotesDataSourceTableView,
  listNotesDatabaseViews,
  listNotesDataSources,
  listNotesDataSourceTemplates,
  setNotesDatabaseEditingLock,
  updateNotesDataSourceSchema,
} from "$lib/api/notes";
import type { NotesChildDatabaseBlock as NotesChildDatabaseBlockDto, NotesDataSourceSchema, NotesDataSourceTableView, NotesDatabaseReference } from "$lib/notes/types";
import { notesDatabaseSession } from "$lib/notes/database/session.svelte";
import NotesChildDatabaseBlock from "./NotesChildDatabaseBlock.svelte";

vi.mock("$lib/api/notes", () => ({
  applyNotesDataSourcePropertyAction: vi.fn(), createNotesDataSource: vi.fn(), attachNotesDataSource: vi.fn(), setNotesDatabaseEditingLock: vi.fn(),
  getNotesDatabaseReference: vi.fn(), getNotesDataSourceSchema: vi.fn(), getNotesDataSourceTableView: vi.fn(),
  listNotesDatabaseViews: vi.fn(), listNotesDataSources: vi.fn(), listNotesDataSourceTemplates: vi.fn(),
  renameNotesDatabase: vi.fn(), updateNotesDataSourceSchema: vi.fn(), updateNotesDataSourceTableView: vi.fn(),
}));

/** Provide a real schema and table that can identify a property other than title. */
function fixture(): { block: NotesChildDatabaseBlockDto; table: NotesDataSourceTableView } {
  const source = {
    source_provider: null, source_object_id: null, source_workspace_id: null,
    source_last_edited_time: null, created_time: "2026-09-30T00:00:00Z", last_edited_time: "2026-09-30T00:00:00Z",
  };
  return {
    block: {
      object: "block", id: "block", parent: { type: "page_id", page_id: "page" },
      created_time: source.created_time, last_edited_time: source.last_edited_time,
      has_children: false, in_trash: false, archived: false,
      source_provider: null, source_object_id: null, source_last_edited_time: null,
      type: "child_database", child_database: { title: "Tasks", database_id: "database", data_source_id: "source", view_id: "view" },
    },
    table: {
      data_source: {
        ...source, object: "data_source", id: "source", parent: { type: "database_id", database_id: "database" },
        database_parent: { type: "page_id", page_id: "page" }, title: "Tasks", title_rich_text: [], description: [],
        icon: null, in_trash: false, properties: {
          Name: { id: "title", name: "Name", type: "title", title: {} },
          Priority: { id: "priority", name: "Priority", type: "number", number: { format: "number" } },
        },
      },
      view: {
        ...source, object: "view", id: "view", parent: { type: "database_id", database_id: "database" },
        data_source_id: "source", name: "Table", type: "table", filter: null, sorts: [], url: null,
        configuration: { type: "table", table: {
          property_order: ["title", "priority"], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page",
        } },
      },
      rows: [], total_row_count: 0, has_more: false, next_cursor: null,
    },
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

/** Mount the real database shell and table with read-only native boundaries mocked. */
async function open(referenceRead?: Promise<NotesDatabaseReference>): Promise<NotesDataSourceTableView> {
  const { block, table } = fixture();
  const reference: NotesDatabaseReference = {
    block_id: "block", page_id: "page", source_block_id: "block", source_page_id: "page", title: "Tasks",
    owned_data_source_count: 1, is_linked: false, editing_locked: false,
  };
  if (referenceRead) vi.mocked(getNotesDatabaseReference).mockReturnValue(referenceRead);
  else vi.mocked(getNotesDatabaseReference).mockResolvedValue(reference);
  vi.mocked(getNotesDataSourceTableView).mockResolvedValue(table);
  vi.mocked(listNotesDatabaseViews).mockResolvedValue([table.view]);
  vi.mocked(listNotesDataSources).mockResolvedValue([{ ...table.data_source, location: [] }]);
  vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
  component = mount(NotesChildDatabaseBlock, { target: document.body, props: {
    block, focusBlockId: null, focusRequestId: 0, onFocusBlock: vi.fn(), onKeydown: vi.fn(),
    onSelectPage: vi.fn(), onCreateLinkedDatabaseView: vi.fn(),
  } });
  // The first mount also compiles the lazily imported table renderer.
  await vi.waitFor(() => expect(document.querySelector('[role="columnheader"] button[aria-label="Name"]')).not.toBeNull(), { timeout: 5_000 });
  return table;
}

/** Open a column's Edit property submenu through the real header menu so selection crosses both component boundaries. */
async function editProperty(name: string): Promise<void> {
  document.querySelector<HTMLButtonElement>(`[role="columnheader"] button[aria-label="${name}"]`)!.click();
  await tick();
  await tick();
  const menu = document.querySelector<HTMLElement>(`[role="dialog"][aria-label="${name}"]`)!;
  const edit = Array.from(menu.querySelectorAll<HTMLButtonElement>("button"))
    .find((button) => button.textContent?.trim() === "Edit property");
  expect(edit).toBeDefined();
  edit!.click();
  await tick();
  await tick();
}

/** The Edit property submenu beside the column menu. */
function propertyEditor(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[role="dialog"][aria-label="Edit property"]');
}

/** Inspect the property description field, a schema draft that the column menu does not save on its own. */
function propertyDescriptionInput(): HTMLInputElement | null {
  const label = Array.from(propertyEditor()?.querySelectorAll<HTMLLabelElement>("label") ?? [])
    .find((candidate) => candidate.querySelector("span")?.textContent?.trim() === "Description");
  return label?.querySelector("input") ?? null;
}

/** Dismiss the column menu and its submenu while retaining the unsaved local draft. */
async function closePropertyEditor(): Promise<void> {
  document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
  await tick();
  expect(propertyEditor()).toBeNull();
}

/** Edit the existing property's local description without saving its schema. */
async function draftPriorityDescription(description: string): Promise<void> {
  await editProperty("Priority");
  await vi.waitFor(() => expect(propertyDescriptionInput()?.disabled).toBe(false));
  const input = propertyDescriptionInput()!;
  input.value = description;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await tick();
  await closePropertyEditor();
}

/** Name the open draft property header and choose its type from the panel below it. */
async function fillPropertyDraft(name: string, typeLabel: string): Promise<void> {
  await vi.waitFor(() => expect(document.activeElement).toBe(document.querySelector('[data-collection-property-draft] input[aria-label="Property name"]')));
  const draft = document.querySelector<HTMLElement>("[data-collection-property-draft]")!;
  const nameInput = draft.querySelector<HTMLInputElement>('input[aria-label="Property name"]')!;
  nameInput.value = name;
  nameInput.dispatchEvent(new Event("input", { bubbles: true }));
  await tick();
  const type = Array.from(draft.querySelectorAll<HTMLButtonElement>("[data-property-type]"))
    .find((button) => button.textContent?.trim() === typeLabel);
  expect(type).toBeDefined();
  type!.click();
  await tick();
}

/** Use the real table header add control to request a new property. */
async function addHeaderProperty(name: string, typeLabel = "Number"): Promise<void> {
  document.querySelector<HTMLButtonElement>('[role="columnheader"] button[aria-label="Add property"]')!.click();
  await fillPropertyDraft(name, typeLabel);
}

describe("Notes database property editor selection", () => {
  it("loads relation source choices when a quick addition cached schema before its first editor open", async () => {
    const initial = fixture().table;
    const canonical = { ...initial.data_source, properties: { ...initial.data_source.properties,
      Project: { id: "project", name: "Project", type: "relation", relation: { data_source_id: "foreign", type: "single_property", single_property: {} } },
    } };
    const foreign = { ...initial.data_source, id: "foreign", title: "Foreign projects" };
    vi.mocked(getNotesDataSourceSchema).mockResolvedValue({ data_source: canonical, view: initial.view });
    vi.mocked(updateNotesDataSourceSchema).mockImplementation(async (_source, update) => {
      const updated = { ...initial, data_source: { ...canonical, properties: update.properties } };
      vi.mocked(getNotesDataSourceTableView).mockResolvedValue(updated);
      return { data_source: updated.data_source, view: updated.view };
    });
    await open();
    vi.mocked(listNotesDataSources).mockResolvedValue([canonical, foreign].map((source) => ({ ...source, location: [] })));
    await addHeaderProperty("Estimate");
    await vi.waitFor(() => expect(document.querySelector('[role="columnheader"] button[aria-label="Project"]')).not.toBeNull());
    expect(listNotesDataSources).not.toHaveBeenCalled();
    await editProperty("Project");
    await vi.waitFor(() => expect(listNotesDataSources).toHaveBeenCalledOnce());
    expect(getNotesDataSourceSchema).toHaveBeenCalledOnce();
    await vi.waitFor(() => expect(propertyDescriptionInput()?.disabled).toBe(false));
    const choices = Array.from(propertyEditor()!.querySelectorAll<HTMLButtonElement>("button"));
    expect(choices.some((button) => button.textContent?.includes("Foreign projects"))).toBe(true);
  });
  it.each(["append", "insert"] as const)("loads foreign source metadata for a first %s rollup without opening the schema editor", async (operation) => {
    const initial = fixture().table;
    const canonical = { ...initial.data_source, properties: { ...initial.data_source.properties,
      Project: { id: "project", name: "Project", type: "relation", relation: { data_source_id: "foreign", type: "single_property", single_property: {} } },
    } };
    const foreign = { ...initial.data_source, id: "foreign", properties: { Budget: { id: "budget", name: "Budget", type: "number", number: { format: "number" } } } };
    vi.mocked(getNotesDataSourceSchema).mockResolvedValue({ data_source: canonical, view: initial.view });
    vi.mocked(updateNotesDataSourceSchema).mockImplementation(async (_source, update) => ({ data_source: { ...canonical, properties: update.properties }, view: initial.view }));
    vi.mocked(applyNotesDataSourcePropertyAction).mockImplementation(async (_source, _database, _view, action) => {
      if (action.type !== "insert") throw new Error("expected insertion");
      return { property_id: String(action.property.id), schema: { data_source: { ...canonical, properties: { ...canonical.properties, Total: action.property } }, view: initial.view } };
    });
    await open();
    vi.mocked(listNotesDataSources).mockResolvedValue([canonical, foreign].map((source) => ({ ...source, location: [] })));
    if (operation === "append") await addHeaderProperty("Total", "Rollup");
    else {
      document.querySelector<HTMLButtonElement>('[role="columnheader"] button[aria-label="Priority"]')!.click();
      await tick();
      await tick();
      Array.from(document.querySelectorAll<HTMLButtonElement>('[role="dialog"][aria-label="Priority"] button'))
        .find((button) => button.textContent?.trim() === "Insert right")!.click();
      await fillPropertyDraft("Total", "Rollup");
    }
    await vi.waitFor(() => expect(listNotesDataSources).toHaveBeenCalledOnce());
    if (operation === "append") await vi.waitFor(() => expect(updateNotesDataSourceSchema).toHaveBeenCalledWith("source", expect.objectContaining({ properties: expect.objectContaining({ Total: expect.objectContaining({ rollup: expect.objectContaining({ relation_property_id: "project", rollup_property_id: "budget" }) }) }) }), { dataSourceId: "source", databaseId: "database", viewId: "view" }));
    else await vi.waitFor(() => expect(applyNotesDataSourcePropertyAction).toHaveBeenCalledWith("source", "database", "view", expect.objectContaining({ type: "insert", property_id: "priority", side: "right", property: expect.objectContaining({ rollup: expect.objectContaining({ relation_property_id: "project", rollup_property_id: "budget" }) }) })));
    expect(document.querySelector('[role="dialog"][aria-label="Edit properties"]')).toBeNull();
  });
  it("refreshes a sibling's durable lock and keeps a newer toggle when an initial reference read finishes late", async () => {
    let finishRead: (value: NotesDatabaseReference) => void = () => {};
    const pendingRead = new Promise<NotesDatabaseReference>((resolve) => { finishRead = resolve; });
    const reference: NotesDatabaseReference = { block_id: "block", page_id: "page", source_block_id: "block", source_page_id: "page", title: "Tasks", is_linked: false, owned_data_source_count: 1, editing_locked: true };
    vi.mocked(setNotesDatabaseEditingLock).mockResolvedValue(reference);
    await open(pendingRead);
    document.querySelector<HTMLButtonElement>('button[aria-label="View settings"]')!.click();
    await tick();
    document.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Database layout lock"]')!.click();
    await vi.waitFor(() => expect(setNotesDatabaseEditingLock).toHaveBeenCalledWith("database", true));
    await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>('[role="columnheader"] button[aria-label="Priority"]')?.disabled).toBe(true));
    finishRead({ ...reference, editing_locked: false });
    await tick();
    await tick();
    expect(document.querySelector<HTMLButtonElement>('[role="columnheader"] button[aria-label="Priority"]')?.disabled).toBe(true);
    vi.mocked(getNotesDatabaseReference).mockResolvedValue({ ...reference, editing_locked: false });
    notesDatabaseSession.invalidate();
    await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>('[role="columnheader"] button[aria-label="Priority"]')?.disabled).toBe(false));
  });
  it("routes contextual empty duplication to the requesting table and preserves an unsaved schema draft", async () => {
    const initial = fixture().table;
    vi.mocked(getNotesDataSourceSchema).mockResolvedValue({ data_source: initial.data_source, view: initial.view });
    vi.mocked(applyNotesDataSourcePropertyAction).mockImplementation(async (_source, _database, _view, action) => {
      expect(action).toEqual({ type: "duplicate", property_id: "priority", name: "Priority (copy)" });
      const updated: NotesDataSourceTableView = {
        ...initial, data_source: { ...initial.data_source, properties: { ...initial.data_source.properties,
          "Priority (copy)": { id: "priority-copy", name: "Priority (copy)", type: "number", number: { format: "number" } },
        } },
      };
      vi.mocked(getNotesDataSourceTableView).mockResolvedValue(updated);
      return { property_id: "priority-copy", schema: { data_source: updated.data_source, view: updated.view } };
    });
    await open();
    await draftPriorityDescription("Draft priority");
    document.querySelector<HTMLButtonElement>('[role="columnheader"] button[aria-label="Priority"]')!.click();
    await tick();
    await tick();
    const duplicate = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="dialog"][aria-label="Priority"] button'))
      .find((button) => button.textContent?.trim() === "Duplicate property (empty)")!;
    duplicate.click();
    await vi.waitFor(() => expect(applyNotesDataSourcePropertyAction).toHaveBeenCalledWith("source", "database", "view", { type: "duplicate", property_id: "priority", name: "Priority (copy)" }));
    await vi.waitFor(() => expect(document.querySelector('[role="columnheader"] button[aria-label="Priority (copy)"]')).not.toBeNull());
    expect(updateNotesDataSourceSchema).not.toHaveBeenCalled();
    await editProperty("Priority");
    await vi.waitFor(() => expect(propertyDescriptionInput()?.value).toBe("Draft priority"));
  });

  it("retains independent unsaved drafts when editing properties from two attached sources", async () => {
    const initial = fixture().table;
    const second: NotesDataSourceTableView = {
      ...initial, data_source: { ...initial.data_source, id: "second-source" },
      view: { ...initial.view, id: "second-view", name: "Second table", data_source_id: "second-source" },
    };
    vi.mocked(getNotesDataSourceSchema).mockImplementation(async (sourceId) => {
      const selected = sourceId === "second-source" ? second : initial;
      return { data_source: selected.data_source, view: selected.view };
    });
    await open();
    vi.mocked(listNotesDatabaseViews).mockResolvedValue([initial.view, second.view]);
    vi.mocked(getNotesDataSourceTableView).mockImplementation(async (sourceId) => sourceId === "second-source" ? second : initial);
    notesDatabaseSession.invalidate();
    await draftPriorityDescription("First draft");
    await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>('button[aria-label="Second table"]')).not.toBeNull());
    document.querySelector<HTMLButtonElement>('button[aria-label="Second table"]')!.click();
    await vi.waitFor(() => expect(getNotesDataSourceTableView).toHaveBeenCalledWith("second-source", { databaseId: "database", viewId: "second-view" }));
    await draftPriorityDescription("Second draft");
    expect(getNotesDataSourceSchema).toHaveBeenCalledWith("second-source", { databaseId: "database", viewId: "second-view" });
    document.querySelector<HTMLButtonElement>('button[aria-label="Table"]')!.click();
    await vi.waitFor(() => expect(document.querySelector('[role="columnheader"] button[aria-label="Priority"]')).not.toBeNull());
    await editProperty("Priority");
    await vi.waitFor(() => expect(propertyDescriptionInput()?.value).toBe("First draft"));
    await closePropertyEditor();
    document.querySelector<HTMLButtonElement>('button[aria-label="Second table"]')!.click();
    await tick();
    await editProperty("Priority");
    await vi.waitFor(() => expect(propertyDescriptionInput()?.value).toBe("Second draft"));
    expect(updateNotesDataSourceSchema).not.toHaveBeenCalled();
  });

  it("ignores a delayed different-source schema result after reopening the cached source", async () => {
    const initial = fixture().table;
    const second: NotesDataSourceTableView = { ...initial,
      data_source: { ...initial.data_source, id: "second-source" },
      view: { ...initial.view, id: "second-view", name: "Second table", data_source_id: "second-source" },
    };
    let finishSecond: (schema: NotesDataSourceSchema) => void = () => {};
    vi.mocked(getNotesDataSourceSchema).mockImplementation((sourceId) => sourceId === "second-source"
      ? new Promise((resolve) => { finishSecond = resolve; })
      : Promise.resolve({ data_source: initial.data_source, view: initial.view }));
    await open();
    vi.mocked(listNotesDatabaseViews).mockResolvedValue([initial.view, second.view]);
    vi.mocked(getNotesDataSourceTableView).mockImplementation(async (sourceId) => sourceId === "second-source" ? second : initial);
    notesDatabaseSession.invalidate();
    await draftPriorityDescription("First draft");
    await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>('button[aria-label="Second table"]')).not.toBeNull());
    document.querySelector<HTMLButtonElement>('button[aria-label="Second table"]')!.click();
    await vi.waitFor(() => expect(getNotesDataSourceTableView).toHaveBeenCalledWith("second-source", { databaseId: "database", viewId: "second-view" }));
    await editProperty("Priority");
    await vi.waitFor(() => expect(getNotesDataSourceSchema).toHaveBeenCalledWith("second-source", { databaseId: "database", viewId: "second-view" }));
    expect(propertyDescriptionInput()).toBeNull();
    await closePropertyEditor();
    document.querySelector<HTMLButtonElement>('button[aria-label="Table"]')!.click();
    await tick();
    await editProperty("Priority");
    expect(propertyDescriptionInput()?.value).toBe("First draft");
    finishSecond({ data_source: second.data_source, view: second.view });
    await tick();
    await tick();
    expect(propertyDescriptionInput()?.value).toBe("First draft");
    expect(propertyDescriptionInput()?.disabled).toBe(false);
  });
  it("selects the requested header property after its initial schema read finishes", async () => {
    let finishSchema: (schema: NotesDataSourceSchema) => void = () => {};
    vi.mocked(getNotesDataSourceSchema).mockImplementation(() => new Promise((resolve) => { finishSchema = resolve; }));
    const table = await open();
    await editProperty("Priority");
    await vi.waitFor(() => expect(getNotesDataSourceSchema).toHaveBeenCalledOnce());
    expect(propertyDescriptionInput()).toBeNull();
    finishSchema({ data_source: table.data_source, view: table.view });
    await vi.waitFor(() => expect(propertyDescriptionInput()).not.toBeNull());
    expect(propertyEditor()!.querySelector('[data-schema-property-id="priority"]')?.contains(propertyDescriptionInput())).toBe(true);
    expect(document.querySelector('[role="dialog"][aria-label="Edit properties"]')).toBeNull();
  });

  it("changes the selected property on cached reopens without another schema read", async () => {
    const table = fixture().table;
    vi.mocked(getNotesDataSourceSchema).mockResolvedValue({ data_source: table.data_source, view: table.view });
    await open();
    await editProperty("Name");
    await vi.waitFor(() => expect(propertyEditor()?.querySelector('[data-schema-property-id="title"]')?.contains(propertyDescriptionInput())).toBe(true));
    await closePropertyEditor();
    await editProperty("Priority");
    await vi.waitFor(() => expect(propertyEditor()?.querySelector('[data-schema-property-id="priority"]')?.contains(propertyDescriptionInput())).toBe(true));
    expect(getNotesDataSourceSchema).toHaveBeenCalledOnce();
  });

  it("adds a canonical property without saving or discarding an unrelated editor draft", async () => {
    const initial = fixture().table;
    vi.mocked(getNotesDataSourceSchema).mockResolvedValue({ data_source: initial.data_source, view: initial.view });
    vi.mocked(updateNotesDataSourceSchema).mockImplementation(async (_sourceId, update) => {
      const table: NotesDataSourceTableView = {
        ...initial,
        data_source: { ...initial.data_source, properties: update.properties },
      };
      vi.mocked(getNotesDataSourceTableView).mockResolvedValue(table);
      return { data_source: table.data_source, view: table.view };
    });
    await open();
    await draftPriorityDescription("Draft priority");
    expect(updateNotesDataSourceSchema).not.toHaveBeenCalled();
    await addHeaderProperty("Estimate");
    await vi.waitFor(() => expect(updateNotesDataSourceSchema).toHaveBeenCalledOnce());
    const update = vi.mocked(updateNotesDataSourceSchema).mock.calls[0][1];
    expect(update.properties.Priority).toMatchObject({ id: "priority", name: "Priority", type: "number" });
    expect(update.properties.Priority).not.toMatchObject({ description: "Draft priority" });
    expect(update.properties.Estimate).toMatchObject({ name: "Estimate", type: "number" });
    await vi.waitFor(() => expect(document.querySelector('[role="columnheader"] button[aria-label="Estimate"]')).not.toBeNull());
    await editProperty("Priority");
    await vi.waitFor(() => expect(propertyDescriptionInput()?.value).toBe("Draft priority"));
    const save = Array.from(propertyEditor()!.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent?.trim() === "Save properties");
    expect(save?.disabled).toBe(false);
    await closePropertyEditor();
    await editProperty("Estimate");
    await vi.waitFor(() => expect(propertyEditor()?.querySelector<HTMLButtonElement>('button[aria-label="Type"]')?.textContent).toContain("Number"));
  });

  it("retains the unsaved editor draft when quick property creation fails", async () => {
    const initial = fixture().table;
    vi.mocked(getNotesDataSourceSchema).mockResolvedValue({ data_source: initial.data_source, view: initial.view });
    vi.mocked(updateNotesDataSourceSchema).mockRejectedValue(new Error("Cannot create property"));
    await open();
    await draftPriorityDescription("Draft priority");
    await addHeaderProperty("Estimate");
    await vi.waitFor(() => expect(document.querySelector('[data-collection-property-draft] [role="alert"]')?.textContent).toContain("Cannot create property"));
    expect(document.querySelector<HTMLInputElement>('[data-collection-property-draft] input[aria-label="Property name"]')?.value).toBe("Estimate");
    expect(updateNotesDataSourceSchema).toHaveBeenCalledOnce();
    await editProperty("Priority");
    await vi.waitFor(() => expect(propertyDescriptionInput()?.value).toBe("Draft priority"));
  });

  it("renames a property from its column menu and applies the canonical name to an unsaved editor draft", async () => {
    const initial = fixture().table;
    vi.mocked(getNotesDataSourceSchema).mockResolvedValue({ data_source: initial.data_source, view: initial.view });
    vi.mocked(updateNotesDataSourceSchema).mockImplementation(async (_sourceId, update) => {
      const table: NotesDataSourceTableView = { ...initial, data_source: { ...initial.data_source, properties: update.properties } };
      vi.mocked(getNotesDataSourceTableView).mockResolvedValue(table);
      return { data_source: table.data_source, view: table.view };
    });
    await open();
    await draftPriorityDescription("Draft priority");
    const name = await openColumnNameField("Name");
    name.value = "  Task  ";
    name.dispatchEvent(new Event("input", { bubbles: true }));
    name.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(updateNotesDataSourceSchema).toHaveBeenCalledOnce());
    const update = vi.mocked(updateNotesDataSourceSchema).mock.calls[0][1];
    expect(update.properties.Task).toMatchObject({ id: "title", type: "title" });
    expect(update.properties).not.toHaveProperty("Name");
    expect(update.properties.Priority).toMatchObject({ id: "priority", name: "Priority" });
    await vi.waitFor(() => expect(document.querySelector('[role="columnheader"] button[aria-label="Task"]')).not.toBeNull());
    await editProperty("Priority");
    await vi.waitFor(() => expect(propertyDescriptionInput()?.value).toBe("Draft priority"));
  });

  it("keeps a duplicate column rename in its field with an explicit error and leaves the schema unchanged", async () => {
    const initial = fixture().table;
    vi.mocked(getNotesDataSourceSchema).mockResolvedValue({ data_source: initial.data_source, view: initial.view });
    await open();
    const name = await openColumnNameField("Priority");
    name.value = "name";
    name.dispatchEvent(new Event("input", { bubbles: true }));
    name.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"][aria-label="Priority"] [role="alert"]')?.textContent)
      .toBe("Could not rename property: A property with that name already exists."));
    expect(name.value).toBe("name");
    expect(updateNotesDataSourceSchema).not.toHaveBeenCalled();
    expect(document.querySelector('[role="columnheader"] button[aria-label="Priority"]')).not.toBeNull();
  });
});

/** Open a column menu and return its focused property name field. */
async function openColumnNameField(name: string): Promise<HTMLInputElement> {
  document.querySelector<HTMLButtonElement>(`[role="columnheader"] button[aria-label="${name}"]`)!.click();
  await vi.waitFor(() => expect(document.activeElement).toBe(document.querySelector(`[role="dialog"][aria-label="${name}"] input[aria-label="Property name"]`)));
  return document.activeElement as HTMLInputElement;
}
