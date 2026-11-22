// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  getNotesDatabaseReference,
  getNotesDataSourceTableView,
  listNotesDatabaseViews,
  listNotesDataSources,
  listNotesDataSourceTemplates,
} from "$lib/api/notes";
import type { NotesChildDatabaseBlock as NotesChildDatabaseBlockDto, NotesDataSourceTableView, NotesDatabaseReference } from "$lib/notes/types";
import { notesDatabaseSession } from "$lib/notes/database/session.svelte";
import NotesChildDatabaseBlock from "./NotesChildDatabaseBlock.svelte";

vi.mock("$lib/api/notes", () => ({
  applyNotesDataSourcePropertyAction: vi.fn(), createNotesDataSource: vi.fn(), attachNotesDataSource: vi.fn(), setNotesDatabaseEditingLock: vi.fn(),
  getNotesDatabaseReference: vi.fn(), getNotesDataSourceSchema: vi.fn(), getNotesDataSourceTableView: vi.fn(),
  listNotesDatabaseViews: vi.fn(), listNotesDataSources: vi.fn(), listNotesDataSourceTemplates: vi.fn(),
  renameNotesDatabase: vi.fn(), updateNotesDataSourceSchema: vi.fn(), updateNotesDataSourceTableView: vi.fn(),
}));

/** A linked database block and its table, so the block offers to open its source database. */
function fixture(): { block: NotesChildDatabaseBlockDto; table: NotesDataSourceTableView; reference: NotesDatabaseReference } {
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
        icon: null, in_trash: false, properties: { Name: { id: "title", name: "Name", type: "title", title: {} } },
      },
      view: {
        ...source, object: "view", id: "view", parent: { type: "database_id", database_id: "database" },
        data_source_id: "source", name: "Table", type: "table", filter: null, sorts: [], url: null,
        configuration: { type: "table", table: {
          property_order: ["title"], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page",
        } },
      },
      rows: [], total_row_count: 0, has_more: false, next_cursor: null,
    },
    reference: {
      block_id: "block", page_id: "page", source_block_id: "source-block", source_page_id: "source-page", title: "Tasks",
      owned_data_source_count: 0, is_linked: true, editing_locked: false,
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

describe("Notes child database action failures", () => {
  it("clears an open failure once a retried open succeeds", async () => {
    const { block, table, reference } = fixture();
    vi.mocked(getNotesDatabaseReference).mockResolvedValue(reference);
    vi.mocked(getNotesDataSourceTableView).mockResolvedValue(table);
    vi.mocked(listNotesDatabaseViews).mockResolvedValue([table.view]);
    vi.mocked(listNotesDataSources).mockResolvedValue([{ ...table.data_source, location: [] }]);
    vi.mocked(listNotesDataSourceTemplates).mockResolvedValue([]);
    const onOpenDatabase = vi.fn<(blockId: string) => Promise<void>>()
      .mockRejectedValueOnce(new Error("open failed"))
      .mockResolvedValueOnce(undefined);
    component = mount(NotesChildDatabaseBlock, { target: document.body, props: {
      block, focusBlockId: null, focusRequestId: 0, onFocusBlock: vi.fn(), onKeydown: vi.fn(),
      onSelectPage: vi.fn(), onCreateLinkedDatabaseView: vi.fn(), onOpenDatabase,
    } });
    const openSource = (): HTMLButtonElement | null => document.querySelector<HTMLButtonElement>('button[aria-label="Open source database"]');
    await vi.waitFor(() => expect(openSource()).not.toBeNull(), { timeout: 5_000 });

    openSource()!.click();
    await vi.waitFor(() => expect(document.body.textContent).toContain("The database could not be opened."));

    openSource()!.click();
    await vi.waitFor(() => expect(onOpenDatabase).toHaveBeenCalledTimes(2));
    await tick();
    expect(onOpenDatabase).toHaveBeenLastCalledWith("source-block");
    expect(document.body.textContent).not.toContain("The database could not be opened.");
  });
});
