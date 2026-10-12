// @vitest-environment jsdom

import { createNotesCompoundTestAdapter } from "./compound-edits.test-helpers";
import type { NotesCompoundEdit } from "$lib/api/notes/compound-edits";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyBlockUpdate, blockEditableRichText, blockPlainText, blockWithRichText, createBlockWrite } from "$lib/notes/blocks/factory";
import { applyNotesPostMutationToTree, type NotesPostMutationResult } from "$lib/notes/post-mutation";
import { createNotesUndoSnapshot } from "$lib/notes/history/undo-history";
import type { NotesTreeState } from "$lib/notes/blocks/tree";
import type { NotesBlock, NotesBlockWrite, NotesChildDatabaseBlock, NotesCreatedDatabase, NotesDatabaseReference, NotesLinkedDatabaseCreateRequest, NotesParent, NotesRichText } from "$lib/notes/types";
import { createNotesDatabasePasteController } from "./database-paste.svelte";

const api = vi.hoisted(() => ({
  getNotesDatabaseReference: vi.fn<(id: string) => Promise<NotesDatabaseReference>>(),
  createNotesLinkedDatabaseView: vi.fn<(request: NotesLinkedDatabaseCreateRequest) => Promise<NotesCreatedDatabase>>(),
  appendNotesBlockChildren: vi.fn(), trashNotesBlock: vi.fn(), updateNotesBlock: vi.fn(),
}));
vi.mock("$lib/api/notes", () => api);
let compoundAdapter = createNotesCompoundTestAdapter(api);
vi.mock("$lib/api/notes/compound-edits", () => ({ applyNotesCompoundEdit: (request: NotesCompoundEdit) => compoundAdapter(request) }));

const PAGE = "10000000-0000-4000-8000-000000000001";
const SOURCE = "10000000-0000-4000-8000-000000000002";
const LINK = `#notes?page=${PAGE}&block=${SOURCE}`;
const REFERENCE: NotesDatabaseReference = { block_id: SOURCE, page_id: PAGE, source_block_id: SOURCE,
  source_page_id: PAGE, title: "Tasks", owned_data_source_count: 1, is_linked: false, editing_locked: false };

/** Build editor blocks without fetching a source schema or rows. */
function model(write: NotesBlockWrite, parent: NotesParent = { type: "page_id", page_id: PAGE }): NotesBlock {
  return { ...write, object: "block", edit_revision: "0".repeat(64), parent, created_time: "", last_edited_time: "", has_children: false, in_trash: false,
    source_provider: null, source_object_id: null, source_last_edited_time: null } as NotesBlock;
}

/** The controller uses only the returned block, not the loaded source schema. */
function created(request: NotesLinkedDatabaseCreateRequest): NotesCreatedDatabase {
  const block: NotesChildDatabaseBlock = { ...model(createBlockWrite(request.id, "child_database", "Tasks")),
    type: "child_database", child_database: { title: "Tasks", database_id: request.id, data_source_id: "10000000-0000-4000-8000-000000000005", view_id: request.view_id } };
  return { block } as NotesCreatedDatabase;
}

function harness(blocks: readonly NotesBlock[]) {
  let page: string | null = PAGE;
  let tree: NotesTreeState = { blocksById: Object.fromEntries(blocks.map((block) => [block.id, block])),
    childIdsByParentId: { [PAGE]: blocks.map((block) => block.id) } };
  compoundAdapter = createNotesCompoundTestAdapter(api, () => Object.values(tree.blocksById));
  let retained: (() => Promise<void>) | null = null;
  const recordUndo = vi.fn();
  const reconcileIdentity = vi.fn();
  const apply = (result: NotesPostMutationResult) => { tree = applyNotesPostMutationToTree(tree, result); };
  const updateLocal = (id: string, richText: readonly NotesRichText[]) => {
    const block = tree.blocksById[id];
    apply({ blocks: [applyBlockUpdate(block, blockWithRichText(block, richText))] });
  };
  const controller = createNotesDatabasePasteController({
    readPageId: () => page, blockById: (id) => tree.blocksById[id], apply, optimisticBlock: model, updateLocal,
    requestFocus: () => undefined,
    updateRichText: async (id, richText) => updateLocal(id, richText),
    enqueue: async (mutation) => { retained = mutation; await mutation(); retained = null; },
    retry: async () => { await retained?.(); retained = null; },
    snapshot: (focus) => createNotesUndoSnapshot(page, tree, focus), recordUndo, reconcileIdentity,
  });
  return { controller, apply, recordUndo, reconcileIdentity, read: () => tree,
    changePage: (id: string | null) => { page = id; } };
}

describe("Notes database paste choices", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    compoundAdapter = createNotesCompoundTestAdapter(api);
    api.getNotesDatabaseReference.mockResolvedValue(REFERENCE);
    api.createNotesLinkedDatabaseView.mockImplementation(async (request) => created(request));
    api.trashNotesBlock.mockResolvedValue(undefined);
    api.appendNotesBlockChildren.mockResolvedValue({ results: [] });
    api.updateNotesBlock.mockResolvedValue(undefined);
  });

  it("ignores a late reference read after the destination text or note changed", async () => {
    let resolve: (value: NotesDatabaseReference) => void = () => {};
    api.getNotesDatabaseReference.mockReturnValue(new Promise((ready) => { resolve = ready; }));
    const h = harness([model(createBlockWrite("text", "paragraph", LINK))]);
    const inspection = h.controller.inspectLink("text", 0, LINK.length, LINK);
    h.apply({ removedBlockIds: ["text"] });
    resolve(REFERENCE);
    await inspection;
    expect(h.controller.prompt).toBeNull();
    h.apply({ blocks: [model(createBlockWrite("text", "paragraph", LINK))] });
    const another = h.controller.inspectLink("text", 0, LINK.length, LINK);
    h.changePage("another-note");
    await another;
    expect(h.controller.prompt).toBeNull();
  });

  it("turns only the pasted URL into a formatted database mention", async () => {
    const block = model(createBlockWrite("text", "paragraph", `Before ${LINK} after`));
    blockEditableRichText(block)[0].annotations.bold = true;
    const h = harness([block]);
    await h.controller.inspectLink("text", 7, 7 + LINK.length, LINK);
    await h.controller.mention();
    const result = blockEditableRichText(h.read().blocksById.text);
    expect(result.map((item) => item.plain_text).join("")).toBe("Before Tasks after");
    expect(result.find((item) => item.type === "mention")).toMatchObject({
      mention: { type: "database", database: { id: SOURCE } }, href: LINK, annotations: { bold: true },
    });
    expect(api.createNotesLinkedDatabaseView).not.toHaveBeenCalled();
    expect(h.controller.prompt).toBeNull();
  });

  it("splits surrounding text immediately and creates a linked view in the correct document position", async () => {
    let resolve: (value: NotesCreatedDatabase) => void = () => {};
    api.createNotesLinkedDatabaseView.mockReturnValue(new Promise((ready) => { resolve = ready; }));
    const h = harness([model(createBlockWrite("text", "paragraph", `Before ${LINK} after`)), model(createBlockWrite("next", "paragraph", "Next"))]);
    await h.controller.inspectLink("text", 7, 7 + LINK.length, LINK);
    const action = h.controller.linkedView();
    const ids = h.read().childIdsByParentId[PAGE];
    const databaseId = ids[1];
    expect(ids.map((id) => blockPlainText(h.read().blocksById[id]))).toEqual(["Before ", "Tasks", " after", "Next"]);
    expect(h.controller.isCreating(databaseId)).toBe(true);
    await vi.waitFor(() => expect(api.createNotesLinkedDatabaseView).toHaveBeenCalledOnce());
    const request = api.createNotesLinkedDatabaseView.mock.calls[0][0];
    expect(request).toMatchObject({ id: databaseId, source_block_id: SOURCE, after_block_id: "text" });
    resolve(created(request));
    await action;
    expect(h.controller.isCreating(databaseId)).toBe(false);
    expect(h.reconcileIdentity).toHaveBeenCalledOnce();
    expect(api.appendNotesBlockChildren).toHaveBeenCalledWith(expect.objectContaining({ after: databaseId }));
    expect(api.trashNotesBlock).not.toHaveBeenCalled();
  });

  it("retries the complete failed replacement with the same linked shell identity", async () => {
    api.trashNotesBlock.mockRejectedValueOnce(new Error("Unavailable")).mockResolvedValue(undefined);
    const h = harness([model(createBlockWrite("before", "paragraph", "Before")), model(createBlockWrite("copy", "child_database", "Tasks")), model(createBlockWrite("after", "paragraph", "After"))]);
    h.controller.beginCopies({ copy: SOURCE }, false);
    await h.controller.linkedView();
    expect(h.controller.error).toBe("Unavailable");
    expect(h.read().childIdsByParentId[PAGE]).toEqual(["before", "copy", "after"]);
    await h.controller.linkedView();
    expect(api.createNotesLinkedDatabaseView).toHaveBeenCalledTimes(2);
    const databaseId = h.read().childIdsByParentId[PAGE][1];
    expect(databaseId).not.toBe("copy");
    expect(h.read().childIdsByParentId[PAGE]).toEqual(["before", databaseId, "after"]);
    expect(h.controller.error).toBeNull();
    expect(h.controller.prompt).toBeNull();
    expect(h.recordUndo).toHaveBeenCalledOnce();
  });

  it("releases pending linked views and copies when their queued writes are abandoned", async () => {
    api.createNotesLinkedDatabaseView.mockRejectedValueOnce(new Error("Unavailable"));
    const h = harness([model(createBlockWrite("text", "paragraph", LINK))]);
    await h.controller.inspectLink("text", 0, LINK.length, LINK);
    await h.controller.linkedView();
    expect(h.controller.error).toBe("Unavailable");
    const [databaseId] = h.read().childIdsByParentId[PAGE];
    h.controller.beginCopies({ copy: SOURCE });
    expect(h.controller.isCreating(databaseId)).toBe(true);
    expect(h.controller.isCreating("copy")).toBe(true);
    h.controller.discardPendingCreations();
    expect(h.controller.isCreating(databaseId)).toBe(false);
    expect(h.controller.isCreating("copy")).toBe(false);
  });
});
