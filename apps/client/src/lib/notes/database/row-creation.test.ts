import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyNotesDataSourceTemplate, createNotesDataSourceRowPage, createNotesDataSourceSubitem, loadNotesPage, updateNotesDataSourceRowProperty } from "$lib/api/notes";
import { createNotesDatabaseRowCreation } from "./row-creation.svelte";
import { createProvisionalNotesPage } from "$lib/notes/pages/creation";
import type { NotesDatabaseTableColumn } from "./table";
import type { NotesPage } from "$lib/notes/types";

vi.mock("$lib/api/notes", () => ({
  applyNotesDataSourceTemplate: vi.fn(), createNotesDataSourceRowPage: vi.fn(),
  createNotesDataSourceSubitem: vi.fn(),
  loadNotesPage: vi.fn(), updateNotesDataSourceRowProperty: vi.fn(),
}));

const title: NotesDatabaseTableColumn = {
  id: "title", name: "Name", type: "title", hidden: false, width: 240, options: [],
  relationDataSourceId: null, buttonLabel: "", buttonRequiresConfirmation: false,
};

function loaded(id: string, name = "", sourceId = "source") {
  return createProvisionalNotesPage({ id, title: name, first_block_id: "block",
    parent: { type: "data_source_id", data_source_id: sourceId }, folder_id: null });
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(createNotesDataSourceRowPage).mockImplementation(async (_source, request) => loaded(request.id));
  vi.mocked(loadNotesPage).mockRejectedValue(new Error("Not found"));
});

describe("Notes database creation persistence", () => {
  it("creates a source-owned sub-item atomically and retains its queued title and ancestry on retry", async () => {
    vi.mocked(createNotesDataSourceSubitem).mockRejectedValueOnce(new Error("Cannot create sub-item"))
      .mockImplementation(async (_source, _parent, request) => loaded(request.id));
    vi.mocked(updateNotesDataSourceRowProperty).mockImplementation(async (_source, id, update) => loaded(id, String(update.value)).page);
    const controller = createNotesDatabaseRowCreation(async () => {});
    const id = controller.begin("source", "", [], "parent");
    controller.draft(id, "title", "Child draft");
    controller.submit(id, title, "Child draft");
    await vi.waitFor(() => expect(controller.errorFor(id)).toBe("Cannot create sub-item"));
    expect(controller.hierarchyFor("source", { parent: {
      parent_row_page_id: "ancestor", ancestor_row_page_ids: ["ancestor"], depth: 1, child_count: 0,
    } })[id]).toMatchObject({ parent_row_page_id: "parent", ancestor_row_page_ids: ["parent", "ancestor"], depth: 2 });
    expect(controller.rowsFor("source", [])[0].parent).toEqual({ type: "data_source_id", data_source_id: "source" });
    controller.retry(id);
    await vi.waitFor(() => expect(controller.isSaving("source")).toBe(false));
    expect(createNotesDataSourceSubitem).toHaveBeenLastCalledWith("source", "parent", expect.objectContaining({ id }));
    expect(createNotesDataSourceRowPage).not.toHaveBeenCalled();
    expect(updateNotesDataSourceRowProperty).toHaveBeenCalledWith("source", id, { property_id: "title", value: "Child draft" });
    expect(controller.errorFor(id)).toBeNull();
  });
  it("serializes queued titles and keeps a newer draft when an earlier save returns", async () => {
    let finish: (page: NotesPage) => void = () => {};
    vi.mocked(updateNotesDataSourceRowProperty).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    vi.mocked(updateNotesDataSourceRowProperty).mockImplementation(async (_source, id, update) => loaded(id, String(update.value)).page);
    const controller = createNotesDatabaseRowCreation(async () => {});
    const id = controller.begin("source");
    controller.draft(id, "title", "First title");
    controller.submit(id, title, "First title");
    await vi.waitFor(() => expect(updateNotesDataSourceRowProperty).toHaveBeenCalledTimes(1));
    controller.draft(id, "title", "Latest title");
    finish(loaded(id, "First title").page);
    await vi.waitFor(() => expect(controller.isSaving("source")).toBe(false));
    expect(controller.valueFor(controller.rowsFor("source", [])[0], title)).toBe("Latest title");
    expect(updateNotesDataSourceRowProperty).toHaveBeenCalledTimes(1);
    controller.submit(id, title, "Latest title");
    await vi.waitFor(() => expect(controller.isSaving("source")).toBe(false));
    expect(updateNotesDataSourceRowProperty).toHaveBeenLastCalledWith("source", id, { property_id: "title", value: "Latest title" });
    expect(controller.valueFor(controller.rowsFor("source", [])[0], title)).toBe("Latest title");
  });

  it("uses a reserved ID for templates and recovers a committed create after a failed response", async () => {
    vi.mocked(applyNotesDataSourceTemplate).mockRejectedValue(new Error("Response lost"));
    vi.mocked(loadNotesPage).mockImplementation(async (id) => loaded(id));
    const controller = createNotesDatabaseRowCreation(async () => {});
    const id = controller.begin("source", "template");
    await vi.waitFor(() => expect(controller.isSaving("source")).toBe(false));
    expect(applyNotesDataSourceTemplate).toHaveBeenCalledWith("source", "template", { id, title: "" });
    expect(controller.errorFor(id)).toBeNull();
    expect(controller.blocked(id)).toBe(false);
    const canonical = loaded(id).page;
    expect(controller.rowsFor("source", [canonical])).toHaveLength(1);
    controller.acceptWindow("source", id, controller.settledIds("source"));
    expect(controller.rowsFor("source", [])).toHaveLength(1);
    controller.acceptWindow("source", null, controller.settledIds("source"));
    expect(controller.rowsFor("source", [])).toHaveLength(0);
  });

  it("does not recover a page from another source and keeps failed or unsaved drafts through refresh", async () => {
    vi.mocked(createNotesDataSourceRowPage).mockRejectedValue(new Error("Cannot create"));
    vi.mocked(loadNotesPage).mockImplementation(async (id) => loaded(id, "", "other-source"));
    const controller = createNotesDatabaseRowCreation(async () => {});
    const id = controller.begin("source");
    controller.draft(id, "title", "Keep me");
    await vi.waitFor(() => expect(controller.errorFor(id)).toBe("Cannot create"));
    controller.acceptWindow("source", null, controller.settledIds("source"));
    expect(controller.rowsFor("source", [])).toHaveLength(1);
    expect(controller.valueFor(controller.rowsFor("source", [])[0], title)).toBe("Keep me");
    expect(controller.rowsFor("other-source", [])).toHaveLength(0);
  });

  it("keeps a row created while an older canonical refresh was in flight", async () => {
    const controller = createNotesDatabaseRowCreation(async () => {});
    const beforeCreate = controller.settledIds("source");
    controller.begin("source");
    await vi.waitFor(() => expect(controller.isSaving("source")).toBe(false));
    controller.acceptWindow("source", null, beforeCreate);
    expect(controller.rowsFor("source", [])).toHaveLength(1);
  });

  it("retries a failed title write without creating another page", async () => {
    vi.mocked(updateNotesDataSourceRowProperty)
      .mockRejectedValueOnce(new Error("Cannot save title"))
      .mockImplementation(async (_source, id, update) => loaded(id, String(update.value)).page);
    const controller = createNotesDatabaseRowCreation(async () => {});
    const id = controller.begin("source");
    controller.submit(id, title, "Keep my name");
    await vi.waitFor(() => expect(controller.errorFor(id)).toBe("Cannot save title"));
    controller.acceptWindow("source", null, controller.settledIds("source"));
    expect(controller.valueFor(controller.rowsFor("source", [])[0], title)).toBe("Keep my name");
    controller.retry(id);
    controller.retry(id);
    await vi.waitFor(() => expect(controller.isSaving("source")).toBe(false));
    expect(controller.errorFor(id)).toBeNull();
    expect(createNotesDataSourceRowPage).toHaveBeenCalledTimes(1);
    expect(updateNotesDataSourceRowProperty).toHaveBeenCalledTimes(2);
  });

  it("flushes a draft when its view closes while creation is still pending", async () => {
    let finish: () => void = () => {};
    vi.mocked(createNotesDataSourceRowPage).mockImplementation((_source, request) => new Promise((resolve) => { finish = () => resolve(loaded(request.id)); }));
    vi.mocked(updateNotesDataSourceRowProperty).mockImplementation(async (_source, id, update) => loaded(id, String(update.value)).page);
    const controller = createNotesDatabaseRowCreation(async () => {});
    const id = controller.begin("source");
    controller.draft(id, "title", "Closing title");
    controller.flush();
    expect(updateNotesDataSourceRowProperty).not.toHaveBeenCalled();
    finish();
    await vi.waitFor(() => expect(updateNotesDataSourceRowProperty).toHaveBeenCalledWith("source", id, { property_id: "title", value: "Closing title" }));
    expect(createNotesDataSourceRowPage).toHaveBeenCalledTimes(1);
  });
});
