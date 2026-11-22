// @vitest-environment jsdom
import { createRawSnippet, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { listNotesDatabaseViews, listNotesDataSources, listNotesDataSourceTemplates } from "$lib/api/notes";
import { notesDatabaseSession } from "$lib/notes/database/session.svelte";
import type { NotesDatabaseView } from "$lib/notes/types";
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

/** Two saved table views over one source. */
function views(): NotesDatabaseView[] {
  return ["First", "Second"].map((name) => ({
    object: "view", id: name, parent: { type: "database_id", database_id: "database" },
    data_source_id: "source", name, type: "table", filter: null, sorts: [], url: null,
    configuration: { type: "table", table: {
      property_order: [], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page",
    } },
    created_time: "2026-09-29T00:00:00Z", last_edited_time: "2026-09-29T00:00:00Z",
    source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null,
  }));
}

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  notesDatabaseSession.clear();
  document.body.replaceChildren();
  vi.resetAllMocks();
});

/** Open the active view's tab menu and press its copy link item. */
async function copyActiveViewLink(): Promise<void> {
  document.querySelector<HTMLButtonElement>('button[aria-label="First"]')!.click();
  await tick();
  const copy = (): HTMLButtonElement | undefined => Array.from(document.querySelectorAll<HTMLButtonElement>("button"))
    .find((button) => button.textContent?.trim() === "Copy link to view");
  await vi.waitFor(() => expect(copy()).toBeDefined());
  copy()!.click();
}

describe("Notes database view action failures", () => {
  it("clears a view link copy failure once a retried copy succeeds", async () => {
    vi.mocked(listNotesDatabaseViews).mockResolvedValue(views());
    vi.mocked(listNotesDataSources).mockResolvedValue([]);
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    const onCopyViewLink = vi.fn<(viewId: string) => Promise<void>>()
      .mockRejectedValueOnce(new Error("clipboard denied"))
      .mockResolvedValueOnce(undefined);
    component = mount(NotesDatabaseViewSurface, { target: document.body, props: {
      dataSourceId: "source", databaseId: "database", initialViewId: "First",
      onReady: vi.fn(),
      reloadKeys: { table: 0, board: 0, gallery: 0, list: 0, calendar: 0, timeline: 0 },
      onSelectPage: vi.fn(), onEditProperties: vi.fn(), onCreateLinkedDatabaseView: vi.fn(),
      propertyEditor: createRawSnippet<[string]>(() => ({ render: () => "<div></div>" })), onLoadPropertyEditor: vi.fn(),
      onAddProperty: vi.fn(async () => {}), onCopyViewLink,
    } });
    await vi.waitFor(() => expect(document.querySelector("[data-session-view]")?.getAttribute("data-session-view")).toBe("First"));

    await copyActiveViewLink();
    await vi.waitFor(() => expect(document.body.textContent).toContain("The view link could not be copied."));

    await copyActiveViewLink();
    await vi.waitFor(() => expect(onCopyViewLink).toHaveBeenCalledTimes(2));
    await tick();
    expect(document.body.textContent).not.toContain("The view link could not be copied.");
  });
});
