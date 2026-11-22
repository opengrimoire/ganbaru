// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesDataSourceSchemaActionResult } from "$lib/notes/database/property-schema-actions";
import type { NotesDataSourceRowPageContext } from "$lib/notes/database/row-page-context";
import type { NotesDataSourceSchema, NotesPage } from "$lib/notes/types";
import NotesDatabaseRowPageProperties from "./NotesDatabaseRowPageProperties.svelte";

const api = vi.hoisted(() => ({
  getNotesDataSourceRowPageContext: vi.fn<(dataSourceId: string, pageId: string) => Promise<NotesDataSourceRowPageContext>>(),
  updateNotesDataSourceSchemaDrafts: vi.fn<() => Promise<NotesDataSourceSchemaActionResult>>(),
}));

vi.mock("$lib/api/notes", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/api/notes")>()),
  getNotesDataSourceRowPageContext: api.getNotesDataSourceRowPageContext,
  listNotesDataSources: vi.fn(async () => []),
}));

vi.mock("$lib/notes/database/property-schema-actions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/notes/database/property-schema-actions")>()),
  updateNotesDataSourceSchemaDrafts: api.updateNotesDataSourceSchemaDrafts,
}));

const dataSourceId = "21212121-2121-4121-8121-212121212121";
const databaseId = "22222222-2222-4222-8222-222222222222";
const rowId = "31313131-3131-4131-8131-313131313131";

const schema = {
  data_source: {
    object: "data_source",
    id: dataSourceId,
    parent: { type: "database_id", database_id: databaseId },
    database_parent: { type: "page_id", page_id: "11111111-1111-4111-8111-111111111111" },
    title: "Tasks",
    title_rich_text: [],
    description: [],
    icon: null,
    properties: {
      Name: { id: "title", name: "Name", description: "", type: "title", title: {} },
      Alpha: { id: "alpha", name: "Alpha", description: "", type: "rich_text", rich_text: {} },
      Beta: { id: "beta", name: "Beta", description: "", type: "rich_text", rich_text: {} },
      Gamma: { id: "gamma", name: "Gamma", description: "", type: "rich_text", rich_text: {} },
    },
    in_trash: false,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
    created_time: "2026-07-01T00:00:00.000Z",
    last_edited_time: "2026-07-01T00:00:00.000Z",
  },
  view: {
    object: "view",
    id: "44444444-4444-4444-8444-444444444444",
    parent: { type: "database_id", database_id: databaseId },
    data_source_id: dataSourceId,
    name: "Table",
    type: "table",
    filter: null,
    sorts: [],
    configuration: null,
    url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
    created_time: "2026-07-01T00:00:00.000Z",
    last_edited_time: "2026-07-01T00:00:00.000Z",
  },
} as unknown as NotesDataSourceSchema;

const row = {
  object: "page", id: rowId, created_time: "2026-07-01T00:00:00Z", last_edited_time: "2026-07-01T00:00:00Z",
  parent: { type: "data_source_id", data_source_id: dataSourceId }, folder_id: null, in_trash: false,
  icon: null, cover: null, properties: {},
  url: null, public_url: null, source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null,
} as unknown as NotesPage;

const context: NotesDataSourceRowPageContext = {
  schema,
  row,
  hierarchy: { parent_row_page_id: null, ancestor_row_page_ids: [], depth: 0, child_count: 0 },
  parent: null,
  children: [],
  editing_locked: false,
};

let component: ReturnType<typeof mount> | undefined;

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

/** A promise with its resolve function exposed, for a save that finishes when the test says so. */
function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

/** Let queued saves and reloads settle. */
async function settle(): Promise<void> {
  for (let step = 0; step < 6; step += 1) await tick();
}

/** The property ids in the order the row page shows them. */
function shownOrder(): string[] {
  return [...document.querySelectorAll<HTMLElement>("[data-row-page-property]")].map((element) => element.dataset.rowPageProperty ?? "");
}

/** Moves a property one step down from its reorder handle. */
function stepDown(propertyId: string): void {
  document.querySelector<HTMLElement>(`[data-row-page-grip="${propertyId}"]`)
    ?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
}

describe("Notes database row page properties", () => {
  it("keeps a later reorder on screen when an earlier one finishes first", async () => {
    vi.stubGlobal("CSS", { escape: (value: string) => value });
    api.getNotesDataSourceRowPageContext.mockResolvedValue(context);
    const first = deferred<NotesDataSourceSchemaActionResult>();
    const second = deferred<NotesDataSourceSchemaActionResult>();
    api.updateNotesDataSourceSchemaDrafts.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    component = mount(NotesDatabaseRowPageProperties, { target: document.body, props: { dataSourceId, pageId: rowId, onOpenPage: vi.fn() } });
    await settle();
    expect(shownOrder()).toEqual(["alpha", "beta", "gamma"]);

    stepDown("alpha");
    await settle();
    expect(shownOrder()).toEqual(["beta", "alpha", "gamma"]);
    stepDown("alpha");
    await settle();
    expect(shownOrder()).toEqual(["beta", "gamma", "alpha"]);

    first.resolve({ schema, changed: true });
    await settle();
    expect(shownOrder()).toEqual(["beta", "gamma", "alpha"]);

    second.resolve({ schema, changed: true });
    await settle();
    expect(shownOrder()).toEqual(["alpha", "beta", "gamma"]);
  });
});
