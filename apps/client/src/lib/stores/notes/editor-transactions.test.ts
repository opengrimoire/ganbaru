// @vitest-environment jsdom

import { createNotesCompoundTestAdapter } from "./compound-edits.test-helpers";
import type { NotesCompoundEdit } from "$lib/api/notes/compound-edits";
import { afterEach, describe, expect, it, vi } from "vitest";
import { applyBlockUpdate, blockIndent, blockEditableRichText, blockPlainText, blockUpdateWithIndent, createBlockUpdate, createBlockWrite } from "$lib/notes/blocks/factory";
import { buildNotesChildIdsByParent, flattenNotesBlockTree, notesIndentationContextIds } from "$lib/notes/blocks/tree";
import { parseNotesBlock } from "$lib/notes/blocks/validation";
import { notesNumberedListOrdinals } from "$lib/notes/blocks/editor-ui";
import { notesBlockOutlineFromBlock, notesOutlineSubtreeIds } from "$lib/notes/blocks/outline";
import type { NotesAppendBlockChildrenRequest, NotesBlock, NotesBlockUpdate, NotesBlockWrite, NotesDatabaseCreateRequest, NotesParent } from "$lib/notes/types";
import { createNotesBlockActions } from "./block-actions";
import { createNotesBlockPersistence } from "./persistence";
import { NotesTreeProjectionController } from "./tree-projection.svelte";
import { createNotesUndoController } from "./undo";

const api = vi.hoisted(() => ({
  updateNotesBlock: vi.fn(), appendNotesBlockChildren: vi.fn(), trashNotesBlock: vi.fn(),
  moveNotesBlock: vi.fn(), saveNotesUndoState: vi.fn(), clearNotesUndoState: vi.fn(),
  createNotesDatabase: vi.fn(),
}));
vi.mock("$lib/api/notes", () => api);
let compoundAdapter = createNotesCompoundTestAdapter(api);
vi.mock("$lib/api/notes/compound-edits", () => ({ applyNotesCompoundEdit: (request: NotesCompoundEdit) => compoundAdapter(request) }));
const pageId = "00000000-0000-4000-8000-000000000001";
const firstId = "00000000-0000-4000-8000-000000000002";
const lastId = "00000000-0000-4000-8000-000000000003";
const parent: NotesParent = { type: "page_id", page_id: pageId };

/** Creates persisted block fixtures using the production payload factory. */
function fromWrite(write: NotesBlockWrite): NotesBlock {
  return {
    object: "block", edit_revision: "0".repeat(64), parent, created_time: "2026-09-24T00:00:00Z", last_edited_time: "2026-09-24T00:00:00Z",
    has_children: false, in_trash: false, source_provider: null, source_object_id: null, source_last_edited_time: null,
    ...write,
  } as NotesBlock;
}

/** Connects the real action, projection, persistence, and history controllers to delayed storage. */
function editor(
  initialBlocks?: NotesBlock[],
  prepareIndentation?: (ids: readonly string[], direction: "nest" | "outdent") => void | Promise<void>,
  prepareBlockDeletion?: (blockId: string) => void | Promise<void>,
) {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const first = fromWrite(createBlockWrite(firstId, "paragraph", "FirstSecond"));
  const last = fromWrite(createBlockWrite(lastId, "paragraph", "Last"));
  const blocks = initialBlocks ?? [first, last];
  const stored = new Map(blocks.map((block) => [block.id, block]));
  compoundAdapter = createNotesCompoundTestAdapter(api, () => [...stored.values()], (original) => {
    stored.clear();
    for (const block of original) stored.set(block.id, block);
  });
  api.createNotesDatabase.mockImplementation(async (request: NotesDatabaseCreateRequest) => {
    await gate;
    const source = request.replace_block_id ? stored.get(request.replace_block_id) : undefined;
    if (request.replace_block_id && !source) throw new Error("Missing source block");
    if (source && source.type !== "paragraph") throw new Error("Source typing must be saved before conversion");
    const created = fromWrite({
      id: request.id,
      type: "child_database",
      child_database: {
        title: request.title, database_id: request.id,
        data_source_id: request.data_source_id, view_id: request.view_id,
      },
    });
    stored.set(created.id, created);
    return { block: created };
  });
  api.updateNotesBlock.mockImplementation(async (id: string, update: NotesBlockUpdate) => {
    await gate;
    const block = stored.get(id);
    if (!block) throw new Error(`Missing block ${id}`);
    const saved = applyBlockUpdate(block, update);
    stored.set(id, saved);
    return saved;
  });
  api.appendNotesBlockChildren.mockImplementation(async (request: NotesAppendBlockChildrenRequest) => {
    await gate;
    const blocks = request.children.map((write) => ({ ...fromWrite(write), parent: request.parent }));
    for (const block of blocks) stored.set(block.id, block);
    return { results: blocks };
  });
  api.trashNotesBlock.mockImplementation(async (id: string, inTrash: boolean) => {
    await gate;
    const block = stored.get(id);
    if (!block) throw new Error(`Missing block ${id}`);
    stored.set(id, { ...block, in_trash: inTrash });
  });
  api.moveNotesBlock.mockImplementation(async (id: string, request: { parent: NotesParent }) => {
    await gate;
    const block = stored.get(id);
    if (!block) throw new Error(`Missing block ${id}`);
    const moved = { ...block, parent: request.parent };
    stored.set(id, moved);
    return moved;
  });
  const projection = new NotesTreeProjectionController({ readSelectedPageId: () => pageId });
  projection.blocksById = Object.fromEntries(blocks.map((block) => [block.id, block]));
  projection.childIdsByParentId = { [pageId]: [], ...buildNotesChildIdsByParent(blocks) };
  projection.syncHydratedOutlines(pageId);
  const focus = vi.fn();
  const restoreSelection = vi.fn();
  const error = vi.fn();
  const persistence = createNotesBlockPersistence({
    reconcileCanonicalBlocks: (blocks) => undo.reconcileCanonicalBlocks(blocks),
    readBlock: (id) => projection.blocksById[id], beforeSave: async () => undefined,
    replaceBlock: (block) => projection.replaceBlock(block), setLoadError: error, debounceMs: 250,
  });
  projection.setLocalChangeMarker(persistence.markBlockLocallyChanged);
  const undo = createNotesUndoController({
    applyPostMutation: (result) => projection.applyPostMutation(result),
    readCanonicalRevision: persistence.readCanonicalRevision,
    acknowledgeCanonicalBlocks: persistence.acknowledgeCanonicalBlocks,
    readSelectedPageId: () => pageId, readTreeState: () => projection.treeState(),
    loadPageTreeForUndo: async () => undefined, requestBlockFocus: focus,
    restoreDocumentSelection: restoreSelection,
    flushPendingMutations: persistence.flushPendingBlockSaves, enqueueEditorMutation: persistence.enqueueEditorMutation,
    applyLocalSnapshot: (target, source) => projection.applyLocalUndoSnapshot(target, source),
  });
  const actions = createNotesBlockActions({
    reconcileCanonicalBlocks: undo.reconcileCanonicalBlocks,
    reconcileCompoundUndo: undo.reconcileCompoundUndo,
    prepareIndentation,
    prepareBlockDeletion,
    readPageRootBlockIds: () => projection.blockOutlines.filter((outline) => outline.parent.type === "page_id" && outline.parent.page_id === pageId).map((outline) => outline.id),
    ...persistence, readSelectedPageId: () => pageId,
    readBlocksById: () => projection.blocksById, readChildIdsByParentId: () => projection.childIdsByParentId,
    treeState: () => projection.treeState(),
    outlineSubtreeIds: (ids) => notesOutlineSubtreeIds(projection.blockOutlines, pageId, ids),
    blockById: (id) => projection.blocksById[id],
    flatBlockItemsForBlockContext: () => flattenNotesBlockTree(projection.treeState(), pageId),
    tableRowsForBlock: () => [], columnItemsForBlock: () => [], tabItemsForBlock: () => [],
    requestBlockFocus: focus,
    createChildPageFromBlock: async () => undefined, createChildPageAfterBlock: async () => undefined,
    loadPageTree: async () => { throw new Error("Must retain local draft"); }, refreshOpenLinks: async () => undefined,
    applyPostMutation: (result) => {
      projection.applyPostMutation(result);
      projection.blockOutlines = projection.blockOutlines.filter((outline) => !result.removedBlockIds?.includes(outline.id));
      projection.syncHydratedOutlines(pageId);
    },
    localInsertBlockAfter: (block, after) => projection.insertBlockAfter(block, after),
    localInsertBlockBefore: (block, before) => projection.insertBlockBefore(block, before),
    localRemoveLeafBlock: (id) => projection.removeLeafBlock(id), awaitSelectedPageReady: async () => undefined,
    createUndoSnapshot: undo.snapshot, createUndoSnapshotForBlocks: undo.snapshotBlocks, recordUndo: undo.record,
  });
  return { actions, undo, projection, persistence, stored, release, focus, error, restoreSelection };
}

afterEach(() => { vi.clearAllMocks(); });

describe("Notes editing with delayed persistence", () => {
  it("replaces a selected database with a fresh paragraph and restores its original identity on undo", async () => {
    const sourceId = "00000000-0000-4000-8000-000000000004";
    const viewId = "00000000-0000-4000-8000-000000000005";
    const database = fromWrite({ id: firstId, type: "child_database", child_database: {
      title: "Tasks", database_id: firstId, data_source_id: sourceId, view_id: viewId,
    } });
    if (database.type !== "child_database") throw new Error("Expected a database fixture");
    const h = editor([database]);
    await h.actions.replaceDocumentRange([firstId], 0, 1, "Replacement");
    const replacementId = h.projection.childIdsByParentId[pageId][0];
    expect(replacementId).not.toBe(firstId);
    expect(h.projection.blocksById[firstId]).toBeUndefined();
    expect(blockPlainText(h.projection.blocksById[replacementId])).toBe("Replacement");
    expect(h.stored.get(firstId)?.type).toBe("child_database");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(api.trashNotesBlock).toHaveBeenCalledWith(firstId, true);
    expect(api.updateNotesBlock).not.toHaveBeenCalledWith(firstId, expect.objectContaining({ type: "paragraph" }));
    await h.undo.undo();
    await h.persistence.flushPendingBlockSaves();
    expect(h.projection.blocksById[firstId]).toMatchObject({ type: "child_database", child_database: database.child_database });
    expect(h.stored.get(firstId)?.in_trash).toBe(false);
    h.undo.dispose();
  });

  it("trashes a removed nested graph through its root after moving unselected children", async () => {
    const childId = "00000000-0000-4000-8000-000000000004";
    const nestedId = "00000000-0000-4000-8000-000000000005";
    const collapsed = fromWrite(createBlockWrite(lastId, "toggle", "Details"));
    if (collapsed.type !== "toggle") throw new Error("Expected toggle");
    collapsed.toggle.ganbaru_open = false;
    const child = { ...fromWrite(createBlockWrite(childId, "toggle", "Nested")), parent: { type: "block_id" as const, block_id: lastId } };
    const nested = { ...fromWrite(createBlockWrite(nestedId, "child_database", "Tasks")), parent: { type: "block_id" as const, block_id: childId } };
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", "Before")), collapsed, child, nested]);
    await h.actions.replaceDocumentRange([firstId, lastId], 0, Number.MAX_SAFE_INTEGER, "After");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(api.trashNotesBlock).toHaveBeenCalledExactlyOnceWith(lastId, true);
    expect(h.projection.blocksById[nestedId]).toBeUndefined();
    h.undo.dispose();
  });
  it("links partial document text immediately, preserves code and databases, and undoes it in one step", async () => {
    const codeId = "00000000-0000-4000-8000-000000000004";
    const databaseId = "00000000-0000-4000-8000-000000000005";
    const first = fromWrite(createBlockWrite(firstId, "paragraph", "FirstSecond"));
    const last = fromWrite(createBlockWrite(lastId, "paragraph", "Last"));
    const code = fromWrite(createBlockWrite(codeId, "code", "Code"));
    const database = fromWrite(createBlockWrite(databaseId, "child_database", "Tasks"));
    const h = editor([first, code, database, last]);
    const ids = [firstId, codeId, databaseId, lastId];
    const url = `#notes?page=${pageId}&block=${databaseId}`;
    const selection = { anchor: { blockId: lastId, offset: 2 }, focus: { blockId: firstId, offset: 5 } };
    await h.actions.linkDocumentRange(ids, 5, 2, url, selection);
    const firstText = blockEditableRichText(h.projection.blocksById[firstId]);
    const lastText = blockEditableRichText(h.projection.blocksById[lastId]);
    expect(firstText.map((item) => [item.plain_text, item.href])).toEqual([["First", null], ["Second", url]]);
    expect(lastText.map((item) => [item.plain_text, item.href])).toEqual([["La", url], ["st", null]]);
    expect(h.projection.blocksById[codeId]).toEqual(code);
    expect(h.projection.blocksById[databaseId]).toEqual(database);
    expect(blockEditableRichText(h.stored.get(firstId)!)[0].href).toBeNull();
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockEditableRichText(parseNotesBlock(h.stored.get(firstId)))[1].href).toBe(url);
    expect(api.updateNotesBlock.mock.calls.map(([id]) => id)).toEqual([firstId, lastId]);
    await h.undo.undo();
    expect(blockEditableRichText(h.projection.blocksById[firstId]).every((item) => item.href === null)).toBe(true);
    expect(blockEditableRichText(h.projection.blocksById[lastId]).every((item) => item.href === null)).toBe(true);
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, selection);
    await h.undo.redo();
    expect(blockEditableRichText(h.projection.blocksById[firstId])[1].href).toBe(url);
    await h.persistence.flushPendingBlockSaves();
  });

  it("reserves a slash database immediately, saves earlier typing first, and ignores repeated conversion and stale text", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", "/dat"))]);
    await h.actions.updateBlockText(firstId, "/datab");
    const creation = h.actions.convertBlock(firstId, "child_database", true);
    const reserved = h.projection.blocksById[firstId];
    expect(reserved).toMatchObject({ type: "child_database", child_database: { title: "" } });
    expect(h.actions.isDatabaseCreationPending(firstId)).toBe(true);
    expect(api.createNotesDatabase).not.toHaveBeenCalled();
    await h.actions.convertBlock(firstId, "child_database", true);
    await h.actions.convertBlock(firstId, "paragraph", true);
    await h.actions.updateBlockText(firstId, "/database");
    await h.actions.updateBlockRichText(firstId, []);
    h.release();
    await creation;
    await h.persistence.flushPendingBlockSaves();
    expect(api.createNotesDatabase).toHaveBeenCalledOnce();
    expect(api.createNotesDatabase).toHaveBeenCalledWith(expect.objectContaining({ title: "", replace_block_id: firstId }));
    expect(api.updateNotesBlock).toHaveBeenCalledOnce();
    if (reserved.type !== "child_database") throw new Error("Expected a reserved database block");
    expect(h.stored.get(firstId)).toMatchObject({ type: "child_database", child_database: reserved.child_database });
    expect(h.actions.isDatabaseCreationPending(firstId)).toBe(false);
    expect(h.focus).toHaveBeenLastCalledWith(firstId);
    expect(h.error).not.toHaveBeenCalled();
  });

  it("retains an ordinary paragraph title when converting it through the block menu", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", "Project tasks"))]);
    const creation = h.actions.convertBlock(firstId, "child_database");
    h.release();
    await creation;
    expect(h.stored.get(firstId)).toMatchObject({ child_database: { title: "Project tasks" } });
  });

  it("inserts an empty database placeholder before native creation completes without replacing its preceding text", async () => {
    const h = editor();
    const creation = h.actions.createSiblingAfter(firstId, { kind: "block", blockType: "child_database" });
    const ids = h.projection.childIdsByParentId[pageId];
    const databaseId = ids[1];
    expect(ids).toEqual([firstId, databaseId, lastId]);
    expect(h.projection.blocksById[databaseId]).toMatchObject({ type: "child_database", child_database: { title: "" } });
    expect(h.actions.isDatabaseCreationPending(databaseId)).toBe(true);
    h.release();
    await creation;
    expect(h.projection.childIdsByParentId[pageId]).toEqual(ids);
    expect(blockPlainText(h.stored.get(firstId)!)).toBe("FirstSecond");
    expect(h.stored.get(databaseId)).toMatchObject({ child_database: { title: "" } });
    expect(h.actions.isDatabaseCreationPending(databaseId)).toBe(false);
  });

  it("keeps failed database creation retryable with the same reserved identities", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", "/datab"))]);
    api.createNotesDatabase.mockRejectedValueOnce(new Error("Storage unavailable"));
    const creation = h.actions.convertBlock(firstId, "child_database", true);
    h.release();
    await expect(creation).rejects.toThrow("Storage unavailable");
    expect(h.actions.isDatabaseCreationPending(firstId)).toBe(true);
    expect(h.error).toHaveBeenLastCalledWith("Storage unavailable");
    await h.persistence.retryEditorMutations();
    expect(api.createNotesDatabase).toHaveBeenCalledTimes(2);
    expect(api.createNotesDatabase.mock.calls[1]).toEqual(api.createNotesDatabase.mock.calls[0]);
    expect(h.actions.isDatabaseCreationPending(firstId)).toBe(false);
    expect(h.stored.get(firstId)).toMatchObject({ child_database: { title: "" } });
    expect(h.error).toHaveBeenLastCalledWith(null);
  });

  it("releases a failed database reservation when its queued writes are abandoned", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", "/datab"))]);
    api.createNotesDatabase.mockRejectedValueOnce(new Error("Storage unavailable"));
    const creation = h.actions.convertBlock(firstId, "child_database", true);
    h.release();
    await expect(creation).rejects.toThrow("Storage unavailable");
    h.persistence.discardPendingEditorWrites();
    h.actions.discardPendingDatabaseCreations();
    expect(h.actions.isDatabaseCreationPending(firstId)).toBe(false);
    h.projection.replaceBlock(h.stored.get(firstId)!);
    await h.actions.convertBlock(firstId, "child_database", true);
    expect(api.createNotesDatabase).toHaveBeenCalledTimes(2);
    expect(h.actions.isDatabaseCreationPending(firstId)).toBe(false);
  });

  it.each(["previous", "next"] as const)("inserts a writable paragraph %s to an only note without changing its identity", async (direction) => {
    const h = editor([fromWrite(createBlockWrite(firstId, "child_page", "Child"))]);
    await h.actions.insertParagraphAdjacent(firstId, direction);
    const ids = h.projection.childIdsByParentId[pageId];
    const inserted = ids.find((id) => id !== firstId)!;
    expect(ids).toEqual(direction === "previous" ? [inserted, firstId] : [firstId, inserted]);
    await h.actions.updateBlockText(inserted, "Typed before saving");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockPlainText(h.stored.get(inserted)!)).toBe("Typed before saving");
    expect(h.stored.get(firstId)?.type).toBe("child_page");
    await h.undo.undo();
    await h.undo.undo();
    await h.persistence.flushPendingBlockSaves();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId]);
    expect(h.error).not.toHaveBeenCalled();
  });

  it("deletes an only note as a page and restores it through undo", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "child_page", "Child"))]);
    await h.actions.replaceDocumentRange([firstId], 0, 1, "");
    const [replacementId] = h.projection.childIdsByParentId[pageId];
    expect(replacementId).not.toBe(firstId);
    expect(h.projection.blocksById[replacementId].type).toBe("paragraph");
    expect(blockPlainText(h.projection.blocksById[replacementId])).toBe("");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(firstId)).toMatchObject({ type: "child_page", in_trash: true });
    expect(api.updateNotesBlock).not.toHaveBeenCalledWith(firstId, expect.objectContaining({ type: "paragraph" }));
    await h.undo.undo();
    await h.persistence.flushPendingBlockSaves();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId]);
    expect(h.stored.get(firstId)).toMatchObject({ type: "child_page", in_trash: false });
    expect(h.error).not.toHaveBeenCalled();
  });

  it("preserves a note excluded by a document range endpoint", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", "Before")), fromWrite(createBlockWrite(lastId, "child_page", "Child"))]);
    await h.actions.replaceDocumentRange([firstId, lastId], 3, 0, "!");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, lastId]);
    expect(blockPlainText(h.stored.get(firstId)!)).toBe("Bef!");
    expect(h.stored.get(lastId)).toMatchObject({ type: "child_page", in_trash: false });
    expect(h.error).not.toHaveBeenCalled();
  });

  it("retries a failed range replacement as one complete operation with its original identities", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "child_page", "Child")), fromWrite(createBlockWrite(lastId, "paragraph", "Last"))]);
    const persistTrash = api.trashNotesBlock.getMockImplementation()!;
    let fail = true;
    api.trashNotesBlock.mockImplementation(async (id: string, inTrash: boolean) => {
      if (id === lastId && fail) { fail = false; throw new Error("Temporary failure"); }
      return persistTrash(id, inTrash);
    });
    await h.actions.replaceDocumentRange([firstId, lastId], 0, 4, "Replacement");
    h.release();
    await expect(h.persistence.flushPendingBlockSaves()).rejects.toThrow("Temporary failure");
    expect(h.stored.get(firstId)?.in_trash).toBe(false);
    await h.persistence.retryEditorMutations();
    await h.persistence.flushPendingBlockSaves();
    expect(api.appendNotesBlockChildren).toHaveBeenCalledTimes(2);
    expect(api.moveNotesBlock).toHaveBeenCalledTimes(2);
    expect(h.stored.get(lastId)?.in_trash).toBe(true);
    const [replacement] = h.projection.childIdsByParentId[pageId];
    expect(blockPlainText(h.stored.get(replacement)!)).toBe("Replacement");
  });

  it.each(["-", "1."])("changes only the current %s list item's depth, preserving descendants and undo", async (marker) => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", ""))]);
    await h.actions.pastePlainTextIntoBlock(firstId, 0, 0, `${marker} ABC\n  ${marker} DEF\n    ${marker} Nested\n${marker} GHI`);
    const rows = () => flattenNotesBlockTree(h.projection.treeState(), pageId).map((row) => [blockPlainText(row.block), row.depth]);
    const original = rows();
    const caret = { start: 0, end: 0 };
    await h.actions.nestBlock(firstId, caret);
    expect(rows()).toEqual([["ABC", 1], ["DEF", 1], ["Nested", 2], ["GHI", 0]]);
    if (marker === "1.") {
      const ordinals = notesNumberedListOrdinals(h.projection.flatBlockOutlines.map(({ outline }) => ({
        id: outline.id, type: outline.type, indent: outline.ganbaru_indent,
        parentId: outline.parent.type === "page_id" ? outline.parent.page_id : outline.parent.block_id,
      })));
      expect(h.projection.flatBlockOutlines.map(({ outline }) => ordinals.get(outline.id))).toEqual([1, 2, 1, 1]);
    }
    await h.actions.nestBlock(firstId, caret);
    expect(rows()).toEqual([["ABC", 2], ["DEF", 1], ["Nested", 2], ["GHI", 0]]);
    await h.actions.outdentBlock(firstId, caret);
    expect(rows()).toEqual([["ABC", 1], ["DEF", 1], ["Nested", 2], ["GHI", 0]]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    for (const { block } of flattenNotesBlockTree(h.projection.treeState(), pageId)) {
      const saved = parseNotesBlock(h.stored.get(block.id));
      expect(saved.parent).toEqual(block.parent);
      expect(blockIndent(saved)).toBe(blockIndent(block));
    }
    await h.undo.undo();
    expect(rows()[0]).toEqual(["ABC", 2]);
    await h.undo.undo();
    await h.undo.undo();
    expect(rows()).toEqual(original);
    await h.undo.redo();
    expect(rows()).toEqual([["ABC", 1], ["DEF", 1], ["Nested", 2], ["GHI", 0]]);
    expect(h.focus).toHaveBeenLastCalledWith(firstId, caret);
    await h.persistence.flushPendingBlockSaves();
  });

  it("outdents a parent without changing the levels of its children or following nested siblings", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", ""))]);
    await h.actions.pastePlainTextIntoBlock(firstId, 0, 0, "- Start\n  - ABC\n    - DEF\n      - Deep\n  - Following\n- GHI");
    const current = h.projection.childIdsByParentId[firstId][0];
    await h.actions.outdentBlock(current);
    expect(flattenNotesBlockTree(h.projection.treeState(), pageId).map((row) => [blockPlainText(row.block), row.depth])).toEqual([
      ["Start", 0], ["ABC", 0], ["DEF", 2], ["Deep", 3], ["Following", 1], ["GHI", 0],
    ]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
  });

  it("removes several parent indentation levels while retaining the child's absolute depth", async () => {
    const block = applyBlockUpdate(fromWrite(createBlockWrite(firstId, "numbered_list_item", "ABC")),
      blockUpdateWithIndent(createBlockUpdate("numbered_list_item", "ABC"), 4));
    const h = editor([block, { ...fromWrite(createBlockWrite(lastId, "numbered_list_item", "DEF")), parent: { type: "block_id", block_id: firstId } }]);
    for (const level of [3, 2, 1, 0]) {
      await h.actions.outdentBlock(firstId);
      expect(h.projection.flatBlockOutlines.map((row) => row.depth)).toEqual([level, 5]);
    }
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockIndent(parseNotesBlock(h.stored.get(lastId)))).toBe(4);
  });

  it.each(["paragraph", "bulleted_list_item", "numbered_list_item", "to_do"] as const)("preserves embedded blocks when indenting and outdenting a %s", async (type) => {
    const childId = "00000000-0000-4000-8000-000000000004";
    const h = editor([
      fromWrite(createBlockWrite(firstId, "paragraph", "Before")),
      { ...fromWrite(createBlockWrite(lastId, type, "ABC")), parent: { type: "block_id", block_id: firstId } },
      { ...fromWrite(createBlockWrite(childId, "divider", "")), parent: { type: "block_id", block_id: lastId } },
    ]);
    await h.actions.outdentBlock(lastId);
    expect(h.projection.flatBlockOutlines.map((row) => row.depth)).toEqual([0, 0, 2]);
    await h.actions.nestBlock(lastId);
    expect(h.projection.flatBlockOutlines.map((row) => row.depth)).toEqual([0, 1, 2]);
    await h.actions.nestBlock(lastId);
    expect(h.projection.flatBlockOutlines.map((row) => row.depth)).toEqual([0, 2, 2]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(parseNotesBlock(h.stored.get(childId))).toMatchObject({ divider: { ganbaru_indent: 1 } });
  });

  it("loads an unmounted child before changing its parent and serializes repeated keys", async () => {
    let finishHydration!: () => void;
    const hydration = new Promise<void>((resolve) => { finishHydration = resolve; });
    const prepare = vi.fn((ids: readonly string[], direction: "nest" | "outdent") => {
      const required = notesIndentationContextIds(h.projection.blockOutlines, ids, direction);
      if (required.every((id) => h.projection.blocksById[id])) return;
      return hydration.then(() => {
        for (const id of required) if (!h.projection.blocksById[id]) h.projection.blocksById[id] = h.stored.get(id)!;
        h.projection.childIdsByParentId = buildNotesChildIdsByParent(h.projection.flatBlockOutlines
          .map(({ outline }) => h.projection.blocksById[outline.id]).filter((block): block is NotesBlock => block !== undefined));
      });
    });
    const h = editor([
      fromWrite(createBlockWrite(firstId, "bulleted_list_item", "ABC")),
      { ...fromWrite(createBlockWrite(lastId, "bulleted_list_item", "DEF")), parent: { type: "block_id", block_id: firstId } },
    ], prepare);
    delete h.projection.blocksById[lastId];
    h.projection.childIdsByParentId[firstId] = [];
    const first = h.actions.nestBlock(firstId);
    const second = h.actions.nestBlock(firstId);
    const third = h.actions.outdentBlock(firstId);
    expect(h.projection.flatBlockOutlines.map((row) => row.depth)).toEqual([0, 1]);
    finishHydration();
    await Promise.all([first, second, third]);
    expect(h.projection.flatBlockOutlines.map((row) => row.depth)).toEqual([1, 1]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockIndent(parseNotesBlock(h.stored.get(lastId)))).toBe(1);
  });

  it("changes every explicitly selected row once without including its unselected descendants", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", ""))]);
    await h.actions.pastePlainTextIntoBlock(firstId, 0, 0, "- ABC\n  - DEF\n    - Unselected\n- GHI");
    const child = h.projection.childIdsByParentId[firstId][0];
    const selection = { anchor: { blockId: firstId, offset: 0 }, focus: { blockId: child, offset: 3 } };
    await h.actions.indentBlockSelection([firstId, child], "nest", selection);
    expect(h.projection.flatBlockOutlines.map((row) => row.depth)).toEqual([1, 2, 2, 0]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    await h.undo.undo();
    expect(h.projection.flatBlockOutlines.map((row) => row.depth)).toEqual([0, 1, 2, 0]);
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, selection);
    await h.undo.redo();
    await h.actions.indentBlockSelection([firstId, child], "outdent", selection);
    expect(h.projection.flatBlockOutlines.map((row) => row.depth)).toEqual([0, 1, 2, 0]);
    await h.persistence.flushPendingBlockSaves();
  });

  it("keeps document order when outdenting an item before other nested siblings", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", ""))]);
    await h.actions.pastePlainTextIntoBlock(firstId, 0, 0, "- Parent\n  - First\n  - Second\n- Last");
    const firstChild = h.projection.childIdsByParentId[firstId][0];
    await h.actions.outdentBlock(firstChild, { start: 2, end: 2 });
    expect(flattenNotesBlockTree(h.projection.treeState(), pageId).map((row) => [blockPlainText(row.block), row.depth])).toEqual([
      ["Parent", 0], ["First", 0], ["Second", 1], ["Last", 0],
    ]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    await h.undo.undo();
    expect(flattenNotesBlockTree(h.projection.treeState(), pageId).map((row) => [blockPlainText(row.block), row.depth])).toEqual([
      ["Parent", 0], ["First", 1], ["Second", 1], ["Last", 0],
    ]);
  });

  it.each(["paragraph", "bulleted_list_item", "numbered_list_item", "to_do"] as const)("indents the first empty %s repeatedly and carries indentation through typing, Enter, and undo", async (type) => {
    const h = editor([fromWrite(createBlockWrite(firstId, type, ""))]);
    for (let level = 1; level <= 12; level += 1) {
      await h.actions.nestBlock(firstId, { start: 0, end: 0 });
      expect(blockIndent(h.projection.blocksById[firstId])).toBe(level);
      expect(h.projection.flatBlockOutlines[0].depth).toBe(level);
    }
    await h.actions.updateBlockText(firstId, "Keep this");
    expect(blockIndent(h.projection.blocksById[firstId])).toBe(12);
    await h.actions.splitTextBlockAtSelection(firstId, 9, 9);
    const second = h.projection.childIdsByParentId[pageId][1];
    expect(blockIndent(h.projection.blocksById[second])).toBe(12);
    for (let level = 11; level >= 0; level -= 1) {
      await h.actions.outdentBlock(second, { start: 0, end: 0 });
      expect(blockIndent(h.projection.blocksById[second])).toBe(level);
      expect(h.projection.flatBlockOutlines.find((item) => item.outline.id === second)?.depth).toBe(level);
    }
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockIndent(h.stored.get(firstId)!)).toBe(12);
    expect(blockIndent(h.stored.get(second)!)).toBe(0);
    await h.undo.undo();
    expect(blockIndent(h.projection.blocksById[second])).toBe(1);
    await h.undo.redo();
    expect(blockIndent(h.projection.blocksById[second])).toBe(0);
  });

  it("indents selected rows equally and restores the group with one undo", async () => {
    const h = editor();
    const range = { anchor: { blockId: firstId, offset: 0 }, focus: { blockId: lastId, offset: 4 } };
    await h.actions.indentBlockSelection([firstId, lastId], "nest", range);
    expect(h.projection.flatBlockOutlines.map((item) => item.depth)).toEqual([1, 1]);
    await h.actions.indentBlockSelection([firstId, lastId], "nest", range);
    expect(h.projection.flatBlockOutlines.map((item) => item.depth)).toEqual([2, 2]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    await h.undo.undo();
    expect(h.projection.flatBlockOutlines.map((item) => item.depth)).toEqual([1, 1]);
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, range);
    await h.actions.indentBlockSelection([firstId, lastId], "outdent", range);
    expect(h.projection.flatBlockOutlines.map((item) => item.depth)).toEqual([0, 0]);
    await h.persistence.flushPendingBlockSaves();
  });

  it("indents and outdents immediately, preserving the caret during rapid edits and undo", async () => {
    const h = editor();
    const caret = { start: 1, end: 3 };
    const indent = h.actions.nestBlock(lastId, caret);
    expect(h.projection.blocksById[lastId].parent).toEqual({ type: "block_id", block_id: firstId });
    expect(h.focus).toHaveBeenLastCalledWith(lastId, caret);
    await indent;
    await h.actions.outdentBlock(lastId, caret);
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, lastId]);
    await h.actions.nestBlock(lastId, caret);
    await h.actions.updateBlockText(lastId, "Latest draft");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(api.moveNotesBlock.mock.calls.map((call) => call[1].parent)).toEqual([
      { type: "block_id", block_id: firstId }, parent, { type: "block_id", block_id: firstId },
    ]);
    expect(blockPlainText(h.projection.blocksById[lastId])).toBe("Latest draft");
    expect(blockPlainText(h.stored.get(lastId)!)).toBe("Latest draft");
    await h.undo.undo();
    await h.undo.undo();
    expect(h.projection.blocksById[lastId].parent).toEqual(parent);
    expect(h.focus).toHaveBeenLastCalledWith(lastId, caret);
    await h.undo.redo();
    expect(h.projection.blocksById[lastId].parent).toEqual({ type: "block_id", block_id: firstId });
  });

  it("opens a collapsed parent while indenting and restores its state on undo", async () => {
    const toggle = fromWrite(createBlockWrite(firstId, "toggle", "Parent"));
    if (toggle.type !== "toggle") throw new Error("Expected toggle");
    toggle.toggle.ganbaru_open = false;
    const h = editor([toggle, fromWrite(createBlockWrite(lastId, "paragraph", "Child"))]);
    await h.actions.nestBlock(lastId, { start: 2, end: 2 });
    expect(flattenNotesBlockTree(h.projection.treeState(), pageId).map((row) => row.block.id)).toEqual([firstId, lastId]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    await h.undo.undo();
    expect(h.projection.blocksById[firstId]).toMatchObject({ toggle: { ganbaru_open: false } });
    expect(h.projection.blocksById[lastId].parent).toEqual(parent);
  });

  it("retains a failed indentation locally and retries subsequent moves in order", async () => {
    const h = editor();
    api.moveNotesBlock.mockRejectedValueOnce(new Error("Storage unavailable"));
    await h.actions.nestBlock(lastId);
    await h.actions.outdentBlock(lastId);
    h.release();
    await expect(h.persistence.flushPendingBlockSaves()).rejects.toThrow("Storage unavailable");
    expect(h.projection.blocksById[lastId].parent).toEqual(parent);
    await h.persistence.retryEditorMutations();
    expect(h.stored.get(lastId)?.parent).toEqual(parent);
  });

  it("leaves invalid indentation in place without moving focus or writing", async () => {
    const h = editor();
    await h.actions.outdentBlock(firstId);
    await h.actions.outdentBlock(lastId);
    expect(h.focus).not.toHaveBeenCalled();
    expect(api.moveNotesBlock).not.toHaveBeenCalled();
    h.release();
    await h.persistence.flushPendingBlockSaves();
  });

  it("retains annotated boundaries when pasting nested Markdown over a partial range", async () => {
    const first = fromWrite(createBlockWrite(firstId, "paragraph", "Before old"));
    const last = fromWrite(createBlockWrite(lastId, "paragraph", "old after"));
    if (first.type !== "paragraph" || last.type !== "paragraph") throw new Error("Expected paragraphs");
    first.paragraph.rich_text[0].annotations.bold = true;
    last.paragraph.rich_text[0].annotations.italic = true;
    const h = editor([first, last]);
    await h.actions.replaceDocumentRange([firstId, lastId], 7, 3, "Intro\n- Parent\n  - Child");
    const rows = flattenNotesBlockTree(h.projection.treeState(), pageId);
    expect(rows.map((row) => [blockPlainText(row.block), row.depth])).toEqual([
      ["Before Intro", 0], ["Parent", 0], ["Child after", 1],
    ]);
    expect(blockEditableRichText(rows[0].block)[0].annotations.bold).toBe(true);
    expect(blockEditableRichText(rows[2].block).at(-1)?.annotations.italic).toBe(true);
    h.release();
    await h.persistence.flushPendingBlockSaves();
  });

  it.each(["plain", "html", "selection", "plain selection"])("pastes nested lists with immediate structure and one undo entry (%s)", async (mode) => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", ""))]);
    const html = "<ul><li>Parent<ol><li>Child<ul><li>Grandchild</li></ul></li></ol></li><li>Sibling</li></ul>";
    const text = "- Parent\n  1. Child\n    - Grandchild\n- Sibling";
    if (mode === "plain") await h.actions.pastePlainTextIntoBlock(firstId, 0, 0, text);
    else if (mode === "html") await h.actions.pasteRichHtmlIntoBlock(firstId, 0, 0, html);
    else await h.actions.replaceDocumentRange([firstId], 0, 0, text, mode === "selection" ? html : undefined);
    const rows = flattenNotesBlockTree(h.projection.treeState(), pageId);
    expect(rows.map((row) => [blockPlainText(row.block), row.depth])).toEqual([
      ["Parent", 0], ["Child", 1], ["Grandchild", 2], ["Sibling", 0],
    ]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    for (const row of rows) expect(h.stored.get(row.block.id)?.parent).toEqual(row.block.parent);
    await h.undo.undo();
    expect(flattenNotesBlockTree(h.projection.treeState(), pageId).map((row) => blockPlainText(row.block))).toEqual([""]);
    await h.undo.redo();
    expect(flattenNotesBlockTree(h.projection.treeState(), pageId).map((row) => [blockPlainText(row.block), row.depth])).toEqual([
      ["Parent", 0], ["Child", 1], ["Grandchild", 2], ["Sibling", 0],
    ]);
  });

  it.each(["single", "selection"])("persists pasted table structure with undo and redo (%s)", async (mode) => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", "Before after"))]);
    const html = "<table><tr><th>Name</th><th>Value</th></tr><tr><td><b>A</b></td><td>B</td></tr></table>";
    if (mode === "single") await h.actions.pasteRichHtmlIntoBlock(firstId, 7, 7, html);
    else await h.actions.replaceDocumentRange([firstId], 7, 7, "", html);
    const visibleRows = flattenNotesBlockTree(h.projection.treeState(), pageId);
    const tableId = visibleRows[1].block.id;
    const rows = [visibleRows[0], visibleRows[1],
      ...(h.projection.childIdsByParentId[tableId] ?? []).map((id) => ({ block: h.projection.blocksById[id], depth: 1 })),
      visibleRows[2]];
    expect(rows.map((row) => [row.block.type, row.depth])).toEqual([
      ["paragraph", 0], ["table", 0], ["table_row", 1], ["table_row", 1], ["paragraph", 0],
    ]);
    expect(blockPlainText(rows[0].block)).toBe("Before ");
    expect(blockPlainText(rows.at(-1)!.block)).toBe("after");
    for (const row of rows) expect(parseNotesBlock(row.block)).toEqual(row.block);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    for (const row of rows) expect(h.stored.get(row.block.id)?.parent).toEqual(row.block.parent);
    await h.undo.undo();
    expect(flattenNotesBlockTree(h.projection.treeState(), pageId).map((row) => blockPlainText(row.block))).toEqual(["Before after"]);
    await h.undo.redo();
    expect(flattenNotesBlockTree(h.projection.treeState(), pageId).map((row) => row.block.type))
      .toEqual(visibleRows.map((row) => row.block.type));
    expect(h.projection.childIdsByParentId[tableId]).toHaveLength(2);
  });

  it("exits a list immediately while preserving rich text, descendants, numbering, and undo", async () => {
    const first = fromWrite(createBlockWrite(firstId, "numbered_list_item", "First"));
    const last = fromWrite(createBlockWrite(lastId, "numbered_list_item", "Second"));
    if (last.type !== "numbered_list_item") throw new Error("Expected numbered item");
    last.numbered_list_item.rich_text[0].annotations.bold = true;
    const h = editor([first, last]);
    const child = { ...fromWrite(createBlockWrite("child", "paragraph", "Nested")), parent: { type: "block_id" as const, block_id: lastId } };
    h.projection.insertBlockAfter(child, null);
    h.stored.set(child.id, child);
    const ordinals = () => notesNumberedListOrdinals(h.projection.flatBlockOutlines.map(({ outline }) => ({
      id: outline.id, type: outline.type,
      parentId: outline.parent.type === "page_id" ? outline.parent.page_id : outline.parent.block_id,
    })));
    expect(ordinals().get(lastId)).toBe(2);
    await h.actions.convertBlock(lastId, "paragraph", false, { start: 0, end: 0 });
    expect(h.projection.blocksById[lastId].type).toBe("paragraph");
    expect(blockEditableRichText(h.projection.blocksById[lastId])).toEqual(last.numbered_list_item.rich_text);
    expect(h.projection.childIdsByParentId[lastId]).toEqual([child.id]);
    expect(h.focus).toHaveBeenLastCalledWith(lastId, { start: 0, end: 0 });
    expect(ordinals().has(lastId)).toBe(false);
    await h.undo.undo();
    expect(h.projection.blocksById[lastId].type).toBe("numbered_list_item");
    expect(ordinals().get(lastId)).toBe(2);
    await h.undo.redo();
    expect(h.projection.blocksById[lastId].type).toBe("paragraph");
    expect(h.projection.childIdsByParentId[lastId]).toEqual([child.id]);
    await h.actions.updateBlockText(lastId, "Second edited");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockPlainText(h.stored.get(lastId)!)).toBe("Second edited");
    expect(h.stored.get(lastId)?.type).toBe("paragraph");
    expect(h.stored.get(child.id)?.parent).toEqual(child.parent);
  });

  it.each(["selection", "single"])("keeps the body writable after deleting its only database through %s deletion", async (mode) => {
    const database = fromWrite({ id: firstId, type: "child_database", child_database: { title: "Tasks", database_id: firstId } });
    const h = editor([database]);
    if (mode === "selection") await h.actions.deleteBlockSelection([firstId]);
    else await h.actions.deleteBlock(firstId);
    const [replacementId] = h.projection.childIdsByParentId[pageId];
    expect(replacementId).not.toBe(firstId);
    expect(h.projection.blocksById[replacementId].type).toBe("paragraph");
    expect(h.focus).toHaveBeenLastCalledWith(replacementId, { start: 0, end: 0 });
    expect(h.stored.has(replacementId)).toBe(false);
    await h.actions.updateBlockText(replacementId, "Still writable");
    expect(blockPlainText(h.projection.blocksById[replacementId])).toBe("Still writable");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(firstId)?.in_trash).toBe(true);
    expect(blockPlainText(h.stored.get(replacementId)!)).toBe("Still writable");
    expect(h.error).not.toHaveBeenCalled();
  });

  it("undoes and redoes complete block deletion together with its replacement paragraph", async () => {
    const h = editor();
    await h.actions.deleteBlockSelection([firstId, lastId]);
    const [replacementId] = h.projection.childIdsByParentId[pageId];
    expect(h.projection.childIdsByParentId[pageId]).toHaveLength(1);
    await h.undo.undo();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, lastId]);
    expect(h.projection.blocksById[replacementId]).toBeUndefined();
    await h.undo.redo();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([replacementId]);
    await h.actions.updateBlockText(replacementId, "After redo");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(firstId)?.in_trash).toBe(true);
    expect(h.stored.get(lastId)?.in_trash).toBe(true);
    expect(blockPlainText(h.stored.get(replacementId)!)).toBe("After redo");
  });

  it("repairs an empty body once without stealing title focus or overwriting immediate typing", async () => {
    const h = editor([]);
    expect(h.actions.ensurePageBody("another-page")).toBeNull();
    const id = h.actions.ensurePageBody(pageId)!;
    expect(h.actions.ensurePageBody(pageId)).toBe(id);
    expect(h.focus).not.toHaveBeenCalled();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([id]);
    await h.actions.updateBlockText(id, "Restored body");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(api.appendNotesBlockChildren).toHaveBeenCalledOnce();
    expect(blockPlainText(h.projection.blocksById[id])).toBe("Restored body");
    expect(blockPlainText(h.stored.get(id)!)).toBe("Restored body");
  });

  it("keeps a replacement editable after an append failure and retries before deleting originals", async () => {
    const h = editor();
    api.appendNotesBlockChildren.mockRejectedValueOnce(new Error("Storage unavailable"));
    await h.actions.deleteBlockSelection([firstId, lastId]);
    const [id] = h.projection.childIdsByParentId[pageId];
    await h.actions.updateBlockText(id, "Retained draft");
    h.release();
    await expect(h.persistence.flushPendingBlockSaves()).rejects.toThrow("Storage unavailable");
    expect(h.error).toHaveBeenCalledWith("Storage unavailable");
    expect(blockPlainText(h.projection.blocksById[id])).toBe("Retained draft");
    expect(h.stored.get(firstId)?.in_trash).toBe(false);
    await h.persistence.retryEditorMutations();
    expect(h.stored.get(firstId)?.in_trash).toBe(true);
    expect(blockPlainText(h.stored.get(id)!)).toBe("Retained draft");
    expect(h.projection.childIdsByParentId[pageId]).toEqual([id]);
  });

  it("clears the final text block immediately and accepts typing while storage is pending", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "paragraph", "Before"))]);
    await h.actions.deleteBlock(firstId);
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe("");
    expect(h.focus).toHaveBeenLastCalledWith(firstId, { start: 0, end: 0 });
    await h.actions.updateBlockText(firstId, "After");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockPlainText(h.stored.get(firstId)!)).toBe("After");
  });

  it("deletes hidden toggle descendants when a document range replaces the whole toggle", async () => {
    const toggle = fromWrite(createBlockWrite(firstId, "toggle", "Details"));
    if (toggle.type !== "toggle") throw new Error("Expected toggle");
    toggle.toggle.ganbaru_open = false;
    const childId = "00000000-0000-4000-8000-000000000004";
    const child = { ...fromWrite(createBlockWrite(childId, "paragraph", "Hidden")),
      parent: { type: "block_id" as const, block_id: firstId } };
    const h = editor([toggle, child, fromWrite(createBlockWrite(lastId, "paragraph", "After"))]);

    await h.actions.replaceDocumentRange([firstId, lastId], 0, Number.MAX_SAFE_INTEGER, "");
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId]);
    expect(h.projection.blocksById[firstId].type).toBe("paragraph");
    expect(h.projection.blocksById[childId]).toBeUndefined();
    expect(h.projection.blockOutlines.some((outline) => outline.id === childId)).toBe(false);

    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(childId)?.in_trash).toBe(true);
    expect(h.stored.get(lastId)?.in_trash).toBe(true);
    expect(h.error).not.toHaveBeenCalled();
  });

  it("clears a selected closed toggle and its body when it is the only visible row", async () => {
    const toggle = fromWrite(createBlockWrite(firstId, "toggle", "Details"));
    if (toggle.type !== "toggle") throw new Error("Expected toggle");
    toggle.toggle.ganbaru_open = false;
    const childId = "00000000-0000-4000-8000-000000000004";
    const child = { ...fromWrite(createBlockWrite(childId, "paragraph", "Hidden")),
      parent: { type: "block_id" as const, block_id: firstId } };
    const h = editor([toggle, child]);

    await h.actions.replaceDocumentRange([firstId], 0, Number.MAX_SAFE_INTEGER, "");
    expect(h.projection.blocksById[firstId].type).toBe("paragraph");
    expect(h.projection.blocksById[childId]).toBeUndefined();

    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(childId)?.in_trash).toBe(true);
    expect(h.error).not.toHaveBeenCalled();
  });

  it("serializes toggle updates before reparenting and deleting its row", async () => {
    const toggle = fromWrite(createBlockWrite(firstId, "toggle", "Details"));
    if (toggle.type !== "toggle") throw new Error("Expected toggle");
    toggle.toggle.ganbaru_open = false;
    const childId = "00000000-0000-4000-8000-000000000004";
    const child = { ...fromWrite(createBlockWrite(childId, "paragraph", "Hidden")),
      parent: { type: "block_id" as const, block_id: firstId } };
    const h = editor([fromWrite(createBlockWrite(lastId, "paragraph", "Before")), toggle, child]);
    let releaseUpdate!: () => void;
    const updateGate = new Promise<void>((resolve) => { releaseUpdate = resolve; });
    const events: string[] = [];
    api.updateNotesBlock.mockImplementation(async (id: string, update: NotesBlockUpdate) => {
      events.push("update started");
      await updateGate;
      const block = h.stored.get(id);
      if (!block || block.in_trash) throw new Error("notes block not found");
      const saved = applyBlockUpdate(block, update);
      h.stored.set(id, saved);
      events.push("update finished");
      return saved;
    });
    api.moveNotesBlock.mockImplementation(async (id: string, request: { parent: NotesParent }) => {
      events.push("move");
      const block = h.stored.get(id)!;
      const moved = { ...block, parent: request.parent };
      h.stored.set(id, moved);
      return moved;
    });
    api.trashNotesBlock.mockImplementation(async (id: string, inTrash: boolean) => {
      events.push("trash");
      const block = h.stored.get(id)!;
      h.stored.set(id, { ...block, in_trash: inTrash });
    });

    await h.actions.updateToggleOpen(firstId, true);
    const deletion = h.actions.deleteBlock(firstId);
    await Promise.resolve();
    expect(events).toEqual(["update started"]);
    releaseUpdate();
    await deletion;
    expect(events).toEqual(["update started", "update finished", "move", "trash"]);
    expect(h.stored.get(childId)?.parent).toEqual(parent);
    expect(h.stored.get(firstId)?.in_trash).toBe(true);
    expect(h.error).not.toHaveBeenCalled();
    h.release();
  });

  it("loads a closed toggle's children before deleting only its row", async () => {
    const toggle = fromWrite(createBlockWrite(firstId, "toggle", "Details"));
    if (toggle.type !== "toggle") throw new Error("Expected toggle");
    toggle.toggle.ganbaru_open = false;
    const childId = "00000000-0000-4000-8000-000000000004";
    const child = { ...fromWrite(createBlockWrite(childId, "paragraph", "Hidden")),
      parent: { type: "block_id" as const, block_id: firstId } };
    const prepare = vi.fn(async () => {
      h.projection.blocksById[childId] = child;
      h.projection.childIdsByParentId[firstId] = [childId];
    });
    const h = editor([fromWrite(createBlockWrite(lastId, "paragraph", "Before")), toggle, child], undefined, prepare);
    delete h.projection.blocksById[childId];
    h.projection.childIdsByParentId[firstId] = [];

    const deletion = h.actions.deleteBlock(firstId);
    await Promise.resolve();
    expect(prepare).toHaveBeenCalledExactlyOnceWith(firstId);
    h.release();
    await deletion;
    expect(h.stored.get(childId)?.parent).toEqual(parent);
    expect(h.stored.get(childId)?.in_trash).toBe(false);
    expect(h.stored.get(firstId)?.in_trash).toBe(true);
    expect(h.error).not.toHaveBeenCalled();
  });

  it("does not add an empty paragraph when only unloaded root outlines remain", async () => {
    const h = editor([]);
    h.projection.blockOutlines = [notesBlockOutlineFromBlock(fromWrite(createBlockWrite(firstId, "paragraph", "Unloaded")), pageId, 0)];
    expect(h.actions.ensurePageBody(pageId)).toBe(firstId);
    expect(h.projection.childIdsByParentId[pageId]).toEqual([]);
    expect(api.appendNotesBlockChildren).not.toHaveBeenCalled();
    h.release();
    await h.persistence.flushPendingBlockSaves();
  });

  it.each(["", "Replacement"])("restores select-all after replacing it with %j and clears the range on redo", async (text) => {
    const h = editor();
    const selection = { anchor: { blockId: firstId, offset: 0 }, focus: { blockId: lastId, offset: Number.MAX_SAFE_INTEGER } };
    await h.actions.replaceDocumentRange([firstId, lastId], 0, Number.MAX_SAFE_INTEGER, text, undefined, selection);
    await h.undo.undo();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, lastId]);
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, selection);
    await h.undo.redo();
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, null);
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe(text);
    await h.undo.undo();
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, selection);
    h.release();
    await h.persistence.flushPendingBlockSaves();
  });

  it("preserves a backward partial selection through replacement and formatting history", async () => {
    const h = editor();
    const selection = { anchor: { blockId: lastId, offset: 2 }, focus: { blockId: firstId, offset: 5 } };
    await h.actions.replaceDocumentRange([firstId, lastId], 5, 2, "X", undefined, selection);
    await h.undo.undo();
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, selection);
    await h.actions.formatDocumentRange([firstId, lastId], 5, 2, "bold", selection);
    await h.undo.undo();
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, selection);
    await h.undo.redo();
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, selection);
    h.release();
    await h.persistence.flushPendingBlockSaves();
  });

  it("preserves the unselected children of a removed boundary block", async () => {
    const h = editor();
    const child = { ...fromWrite(createBlockWrite("child", "paragraph", "Keep me")), parent: { type: "block_id" as const, block_id: lastId } };
    h.projection.insertBlockAfter(child, null);
    h.stored.set(child.id, child);
    await h.actions.replaceDocumentRange([firstId, lastId], 5, 2, "");
    expect(blockPlainText(h.projection.blocksById[child.id])).toBe("Keep me");
    expect(h.projection.blocksById[child.id].parent).toEqual(parent);
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, child.id]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(api.moveNotesBlock).toHaveBeenCalledWith(child.id, { parent, after: firstId, before: null });
    expect(api.moveNotesBlock.mock.invocationCallOrder[0]).toBeLessThan(api.trashNotesBlock.mock.invocationCallOrder[0]);
  });

  it("formats a partial document range as one undo step", async () => {
    const h = editor();
    await h.actions.formatDocumentRange([firstId, lastId], 5, 2, "bold");
    expect(blockEditableRichText(h.projection.blocksById[firstId]).map((run) => [run.plain_text, run.annotations.bold]))
      .toEqual([["First", false], ["Second", true]]);
    expect(blockEditableRichText(h.projection.blocksById[lastId]).map((run) => [run.plain_text, run.annotations.bold]))
      .toEqual([["La", true], ["st", false]]);
    await h.undo.undo();
    expect(blockEditableRichText(h.projection.blocksById[firstId]).every((run) => !run.annotations.bold)).toBe(true);
    expect(blockEditableRichText(h.projection.blocksById[lastId]).every((run) => !run.annotations.bold)).toBe(true);
    h.release();
    await h.persistence.flushPendingBlockSaves();
  });

  it.each([false, true])("leaves text at an exclusive endpoint unformatted (backward: %s)", async (backward) => {
    const h = editor();
    const start = { blockId: firstId, offset: 0 };
    const end = { blockId: lastId, offset: 0 };
    const selection = { anchor: backward ? end : start, focus: backward ? start : end };
    const last = blockEditableRichText(h.projection.blocksById[lastId]);
    await h.actions.formatDocumentRange([firstId, lastId], 0, 0, "bold", selection);
    expect(blockEditableRichText(h.projection.blocksById[firstId]).every((run) => run.annotations.bold)).toBe(true);
    expect(blockEditableRichText(h.projection.blocksById[lastId])).toEqual(last);
    await h.undo.undo();
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, selection);
    expect(blockEditableRichText(h.projection.blocksById[firstId]).every((run) => !run.annotations.bold)).toBe(true);
    h.release();
    await h.persistence.flushPendingBlockSaves();
  });

  it("replaces through the selected line break while preserving all text at the endpoint", async () => {
    const h = editor();
    const selection = { anchor: { blockId: firstId, offset: 0 }, focus: { blockId: lastId, offset: 0 } };
    await h.actions.replaceDocumentRange([firstId, lastId], 0, 0, "Pasted\n", undefined, selection);
    expect(flattenNotesBlockTree(h.projection.treeState(), pageId).map(({ block }) => blockPlainText(block))).toEqual(["Pasted", "Last"]);
    await h.undo.undo();
    expect(flattenNotesBlockTree(h.projection.treeState(), pageId).map(({ block }) => blockPlainText(block))).toEqual(["FirstSecond", "Last"]);
    expect(h.restoreSelection).toHaveBeenLastCalledWith(pageId, selection);
    h.release();
    await h.persistence.flushPendingBlockSaves();
  });

  it("pastes sanitized rich text over a cross-block range without losing the suffix", async () => {
    const h = editor();
    await h.actions.replaceDocumentRange([firstId, lastId], 5, 2, "Bold", "<p><strong>Bold</strong></p>");
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe("FirstBoldst");
    expect(blockEditableRichText(h.projection.blocksById[firstId]).find((run) => run.plain_text === "Bold")?.annotations.bold).toBe(true);
    h.release();
    await h.persistence.flushPendingBlockSaves();
  });

  it("replaces partial text across blocks immediately and restores both blocks with one undo", async () => {
    const h = editor();
    await h.actions.replaceDocumentRange([firstId, lastId], 5, 2, "X");
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe("FirstXst");
    expect(h.projection.blocksById[lastId]).toBeUndefined();
    await h.undo.undo();
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe("FirstSecond");
    expect(blockPlainText(h.projection.blocksById[lastId])).toBe("Last");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockPlainText(h.stored.get(firstId)!)).toBe("FirstSecond");
    expect(h.stored.get(lastId)?.in_trash).toBe(false);
  });

  it("replaces a document range with multiple lines and preserves the trailing text", async () => {
    const h = editor();
    await h.actions.replaceDocumentRange([firstId, lastId], 5, 2, "A\nB");
    const ids = h.projection.childIdsByParentId[pageId];
    expect(ids).toHaveLength(2);
    expect(ids.map((id) => blockPlainText(h.projection.blocksById[id]))).toEqual(["FirstA", "Bst"]);
    await h.actions.updateBlockText(ids[1], "B typed st");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockPlainText(h.stored.get(ids[1])!)).toBe("B typed st");
  });

  it("renders Enter in place immediately and retains subsequent typing after the append response", async () => {
    const h = editor();
    await h.actions.splitTextBlockAtSelection(firstId, 5, 5);
    const newId = h.projection.childIdsByParentId[pageId][1];
    expect(h.projection.flatBlockOutlines.map((item) => item.outline.id)).toEqual([firstId, newId, lastId]);
    await h.actions.updateBlockText(newId, "Second edited");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockPlainText(h.projection.blocksById[newId])).toBe("Second edited");
    expect(blockPlainText(h.stored.get(newId)!)).toBe("Second edited");
  });

  it("opens a collapsed toggle and writes the split title suffix as an editable child", async () => {
    const toggle = fromWrite(createBlockWrite(firstId, "toggle", "Title suffix"));
    if (toggle.type !== "toggle") throw new Error("Expected toggle");
    toggle.toggle.ganbaru_open = false;
    const h = editor([toggle, fromWrite(createBlockWrite(lastId, "paragraph", "After"))]);
    await h.actions.splitTextBlockAtSelection(firstId, 6, 6);
    const childId = h.projection.childIdsByParentId[firstId]?.[0];
    expect(childId).toBeTruthy();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, lastId]);
    expect(h.projection.flatBlockOutlines.map((item) => item.outline.id)).toEqual([firstId, childId, lastId]);
    expect(h.projection.blocksById[firstId]).toMatchObject({ toggle: { ganbaru_open: true } });
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe("Title ");
    expect(h.projection.blocksById[childId].parent).toEqual({ type: "block_id", block_id: firstId });
    expect(h.projection.blocksById[childId].type).toBe("paragraph");
    expect(blockPlainText(h.projection.blocksById[childId])).toBe("suffix");
    await h.undo.undo();
    expect(h.projection.childIdsByParentId[firstId] ?? []).toEqual([]);
    expect(h.projection.blocksById[firstId]).toMatchObject({ toggle: { ganbaru_open: false } });
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe("Title suffix");
    await h.undo.redo();
    expect(h.projection.childIdsByParentId[firstId]).toEqual([childId]);
    await h.actions.updateBlockText(childId, "Edited child");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(firstId)).toMatchObject({ toggle: { ganbaru_open: true } });
    expect(h.stored.get(childId)?.parent).toEqual({ type: "block_id", block_id: firstId });
    expect(blockPlainText(h.stored.get(childId)!)).toBe("Edited child");
    expect(h.error).not.toHaveBeenCalled();
  });

  it("keeps Enter-created rows inside a callout and exits on an empty child row", async () => {
    const callout = fromWrite(createBlockWrite(firstId, "callout", "Title body"));
    const h = editor([callout, fromWrite(createBlockWrite(lastId, "paragraph", "After"))]);
    await h.actions.splitTextBlockAtSelection(firstId, 6, 6);
    const childId = h.projection.childIdsByParentId[firstId]?.[0];
    expect(childId).toBeTruthy();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, lastId]);
    expect(h.projection.blocksById[childId]).toMatchObject({
      type: "paragraph", parent: { type: "block_id", block_id: firstId },
    });
    expect(blockPlainText(h.projection.blocksById[childId])).toBe("body");
    await h.actions.splitTextBlockAtSelection(childId, 4, 4);
    const emptyId = h.projection.childIdsByParentId[firstId]?.[1];
    expect(blockPlainText(h.projection.blocksById[emptyId])).toBe("");
    await h.actions.splitTextBlockAtSelection(emptyId, 0, 0);
    const followingId = h.projection.childIdsByParentId[firstId]?.[2];
    await h.actions.updateBlockText(followingId, "Still inside");
    await h.actions.outdentBlock(emptyId, { start: 0, end: 0 });
    expect(h.projection.childIdsByParentId[firstId]).toEqual([childId, followingId]);
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, emptyId, lastId]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(childId)?.parent).toEqual({ type: "block_id", block_id: firstId });
    expect(h.stored.get(followingId)?.parent).toEqual({ type: "block_id", block_id: firstId });
    expect(h.stored.get(emptyId)?.parent).toEqual(parent);
    expect(h.error).not.toHaveBeenCalled();
  });

  it("creates an ordinary paragraph before a callout when Enter starts its text", async () => {
    const callout = fromWrite(createBlockWrite(firstId, "callout", "Keep this"));
    const childId = crypto.randomUUID();
    const child = { ...fromWrite(createBlockWrite(childId, "heading_1", "Heading")),
      parent: { type: "block_id", block_id: firstId } as const };
    const h = editor([callout, child, fromWrite(createBlockWrite(lastId, "paragraph", "After"))]);
    await h.actions.splitTextBlockAtSelection(firstId, 0, 0);
    const previousId = h.projection.childIdsByParentId[pageId][0];
    expect(h.projection.childIdsByParentId[pageId]).toEqual([previousId, firstId, lastId]);
    expect(h.projection.blocksById[previousId].type).toBe("paragraph");
    expect(h.projection.childIdsByParentId[firstId]).toEqual([childId]);
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe("Keep this");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(previousId)?.parent).toEqual(parent);
    expect(h.stored.get(childId)?.parent).toEqual(child.parent);
  });

  it("creates a paragraph before an empty-label callout when Enter starts its first child", async () => {
    const callout = fromWrite(createBlockWrite(firstId, "callout"));
    const childId = crypto.randomUUID();
    const child = { ...fromWrite(createBlockWrite(childId, "heading_1", "Title")),
      parent: { type: "block_id", block_id: firstId } as const };
    const h = editor([callout, child, fromWrite(createBlockWrite(lastId, "paragraph", "After"))]);
    await h.actions.splitTextBlockAtSelection(childId, 0, 0);
    const previousId = h.projection.childIdsByParentId[pageId][0];
    expect(h.projection.childIdsByParentId[pageId]).toEqual([previousId, firstId, lastId]);
    expect(h.projection.blocksById[previousId].type).toBe("paragraph");
    expect(h.projection.childIdsByParentId[firstId]).toEqual([childId]);
    expect(blockPlainText(h.projection.blocksById[childId])).toBe("Title");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(previousId)?.parent).toEqual(parent);
    expect(h.stored.get(childId)?.parent).toEqual(child.parent);
    expect(h.error).not.toHaveBeenCalled();
  });

  it("changes a callout icon while preserving its text and child blocks", async () => {
    const callout = fromWrite(createBlockWrite(firstId, "callout", "Notice"));
    const childId = crypto.randomUUID();
    const child = { ...fromWrite(createBlockWrite(childId, "paragraph", "Details")),
      parent: { type: "block_id", block_id: firstId } as const };
    const h = editor([callout, child]);
    await h.actions.updateCalloutIcon(firstId, { type: "emoji", emoji: "⚠️" });
    await h.actions.updateBlockColor(firstId, "yellow_background");
    await h.actions.updateBlockColor(firstId, "red");
    expect(h.projection.blocksById[firstId]).toMatchObject({
      callout: { icon: { type: "emoji", emoji: "⚠️" }, color: "yellow_background" },
    });
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe("Notice");
    expect(h.projection.childIdsByParentId[firstId]).toEqual([childId]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(firstId)).toMatchObject({
      callout: { icon: { type: "emoji", emoji: "⚠️" }, color: "yellow_background" },
    });
  });

  it("inserts an empty sibling before a toggle when Enter is pressed at the start of its title", async () => {
    const precedingId = crypto.randomUUID();
    const childId = crypto.randomUUID();
    const preceding = fromWrite(createBlockWrite(precedingId, "paragraph", "Before"));
    const toggle = fromWrite(createBlockWrite(firstId, "toggle", "Example toggle"));
    if (toggle.type !== "toggle") throw new Error("Expected toggle");
    toggle.toggle.ganbaru_open = false;
    const child = { ...fromWrite(createBlockWrite(childId, "paragraph", "Existing child")),
      parent: { type: "block_id", block_id: firstId } as const };
    const following = fromWrite(createBlockWrite(lastId, "paragraph", "After"));
    const h = editor([preceding, toggle, child, following]);

    await h.actions.splitTextBlockAtSelection(firstId, 0, 0);
    const newId = h.projection.childIdsByParentId[pageId][1];
    expect(h.projection.childIdsByParentId[pageId]).toEqual([precedingId, newId, firstId, lastId]);
    expect(h.projection.flatBlockOutlines.map(({ outline }) => outline.id))
      .toEqual([precedingId, newId, firstId, lastId]);
    expect(h.projection.blocksById[newId]).toMatchObject({
      type: "toggle", parent, toggle: { ganbaru_open: true },
    });
    expect(blockPlainText(h.projection.blocksById[newId])).toBe("");
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe("Example toggle");
    expect(h.projection.blocksById[firstId]).toMatchObject({ toggle: { ganbaru_open: false } });
    expect(h.projection.childIdsByParentId[firstId]).toEqual([childId]);
    expect(h.focus).toHaveBeenLastCalledWith(newId, { start: 0, end: 0 });

    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(api.appendNotesBlockChildren).toHaveBeenCalledWith({
      parent, after: firstId, children: [expect.objectContaining({ id: newId, type: "toggle" })],
    });
    expect(api.moveNotesBlock).toHaveBeenCalledWith(newId, {
      parent, after: null, before: firstId,
    });
    expect(h.stored.get(childId)?.parent).toEqual(child.parent);
    expect(h.error).not.toHaveBeenCalled();

    await h.undo.undo();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([precedingId, firstId, lastId]);
    expect(h.projection.childIdsByParentId[firstId]).toEqual([childId]);
    await h.undo.redo();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([precedingId, newId, firstId, lastId]);
    expect(h.projection.childIdsByParentId[firstId]).toEqual([childId]);
  });

  it("inserts a toggle before a nested toggle without moving the nested toggle's children", async () => {
    const nestedId = crypto.randomUUID();
    const nestedChildId = crypto.randomUUID();
    const root = fromWrite(createBlockWrite(firstId, "toggle", "Parent"));
    const nested = { ...fromWrite(createBlockWrite(nestedId, "toggle", "Nested")),
      parent: { type: "block_id", block_id: firstId } as const };
    const nestedChild = { ...fromWrite(createBlockWrite(nestedChildId, "paragraph", "Body")),
      parent: { type: "block_id", block_id: nestedId } as const };
    const h = editor([root, nested, nestedChild]);

    await h.actions.splitTextBlockAtSelection(nestedId, 0, 0);
    const newId = h.projection.childIdsByParentId[firstId][0];
    expect(h.projection.childIdsByParentId[firstId]).toEqual([newId, nestedId]);
    expect(h.projection.childIdsByParentId[nestedId]).toEqual([nestedChildId]);
    expect(h.projection.blocksById[newId].parent).toEqual(nested.parent);
    expect(blockPlainText(h.projection.blocksById[nestedId])).toBe("Nested");
    await h.actions.updateBlockText(newId, "Earlier nested toggle");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(api.moveNotesBlock).toHaveBeenCalledWith(newId, {
      parent: nested.parent, after: null, before: nestedId,
    });
    expect(blockPlainText(h.stored.get(newId)!)).toBe("Earlier nested toggle");
    expect(h.stored.get(nestedChildId)?.parent).toEqual(nestedChild.parent);
    expect(h.error).not.toHaveBeenCalled();
  });

  it("retries the placement without appending a duplicate toggle", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "toggle", "Example"))]);
    api.moveNotesBlock.mockRejectedValueOnce(new Error("Placement unavailable"));

    await h.actions.splitTextBlockAtSelection(firstId, 0, 0);
    const newId = h.projection.childIdsByParentId[pageId][0];
    h.release();
    await expect(h.persistence.flushPendingBlockSaves()).rejects.toThrow("Placement unavailable");
    expect(api.appendNotesBlockChildren).toHaveBeenCalledTimes(1);
    expect(h.stored.has(newId)).toBe(false);

    await h.persistence.retryEditorMutations();
    expect(api.appendNotesBlockChildren).toHaveBeenCalledTimes(2);
    expect(api.moveNotesBlock).toHaveBeenCalledTimes(2);
    expect(h.projection.childIdsByParentId[pageId]).toEqual([newId, firstId]);
    expect(h.error).toHaveBeenLastCalledWith(null);
  });

  it("keeps unloaded preceding rows before a newly inserted toggle", async () => {
    const unloaded = fromWrite(createBlockWrite(crypto.randomUUID(), "paragraph", "Unloaded"));
    const toggle = fromWrite(createBlockWrite(firstId, "toggle", "Example"));
    const following = fromWrite(createBlockWrite(lastId, "paragraph", "After"));
    const h = editor([toggle, following]);
    h.projection.replaceOutlines([unloaded, toggle, following].map((block, index) => (
      notesBlockOutlineFromBlock(block, pageId, index)
    )), pageId);

    await h.actions.splitTextBlockAtSelection(firstId, 0, 0);
    const newId = h.projection.childIdsByParentId[pageId][0];
    expect(h.projection.flatBlockOutlines.map(({ outline }) => outline.id))
      .toEqual([unloaded.id, newId, firstId, lastId]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.error).not.toHaveBeenCalled();
  });

  it("adds an empty paragraph inside an empty toggle without creating another top-level toggle", async () => {
    const h = editor([fromWrite(createBlockWrite(firstId, "toggle", "")),
      fromWrite(createBlockWrite(lastId, "paragraph", "After"))]);
    await h.actions.splitTextBlockAtSelection(firstId, 0, 0);
    const childId = h.projection.childIdsByParentId[firstId]?.[0];
    expect(childId).toBeTruthy();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, lastId]);
    expect(h.projection.blocksById[childId]).toMatchObject({
      type: "paragraph", parent: { type: "block_id", block_id: firstId },
    });
    expect(h.focus).toHaveBeenLastCalledWith(childId, { start: 0, end: 0 });
    await h.actions.outdentBlock(childId);
    expect(h.projection.childIdsByParentId[firstId] ?? []).toEqual([]);
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, childId, lastId]);
    expect(h.projection.blocksById[childId].parent).toEqual(parent);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(childId)?.parent).toEqual(parent);
  });

  it("removes collapsed toggle descendants from rendered outlines without deleting them", async () => {
    const root = fromWrite(createBlockWrite(firstId, "toggle", "Details"));
    const child = { ...fromWrite(createBlockWrite("child", "paragraph", "Hidden")),
      parent: { type: "block_id", block_id: firstId } as const };
    const sibling = fromWrite(createBlockWrite(lastId, "paragraph", "After"));
    const h = editor([root, child, sibling]);
    const visibleIds = () => h.projection.flatBlockOutlines.map(({ outline }) => outline.id);
    expect(visibleIds()).toEqual([firstId, child.id, lastId]);

    await h.actions.updateToggleOpen(firstId, false);
    expect(visibleIds()).toEqual([firstId, lastId]);
    expect(h.projection.blockOutlines.map((outline) => outline.id)).toContain(child.id);
    expect(h.projection.childIdsByParentId[firstId]).toEqual([child.id]);

    await h.actions.updateToggleOpen(firstId, true);
    expect(visibleIds()).toEqual([firstId, child.id, lastId]);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.stored.get(child.id)?.parent).toEqual(child.parent);
    expect(h.error).not.toHaveBeenCalled();
  });

  it("hides toggle child placeholders once a closed parent body hydrates", () => {
    const root = fromWrite(createBlockWrite(firstId, "toggle", "Details"));
    if (root.type !== "toggle") throw new Error("Expected toggle");
    root.toggle.ganbaru_open = false;
    const child = { ...fromWrite(createBlockWrite("child", "paragraph", "Hidden")),
      parent: { type: "block_id", block_id: firstId } as const };
    const sibling = fromWrite(createBlockWrite(lastId, "paragraph", "After"));
    const projection = new NotesTreeProjectionController({ readSelectedPageId: () => pageId });
    projection.replaceOutlines([root, child, sibling].map((block, index) => (
      notesBlockOutlineFromBlock(block, pageId, index)
    )), pageId);
    expect(projection.flatBlockOutlines.map(({ outline }) => outline.id))
      .toEqual([firstId, child.id, lastId]);

    projection.replaceHydratedBlocks({ [firstId]: root }, { [pageId]: [firstId, lastId] });
    expect(projection.flatBlockOutlines.map(({ outline }) => outline.id))
      .toEqual([firstId, lastId]);
    expect(projection.blockOutlines.map((outline) => outline.id)).toContain(child.id);

    projection.replaceHydratedBlocks({}, {});
    expect(projection.flatBlockOutlines.map(({ outline }) => outline.id))
      .toEqual([firstId, lastId]);
    projection.replaceHydratedBlocks({ [firstId]: {
      ...root, toggle: { ...root.toggle, ganbaru_open: true },
    } }, { [pageId]: [firstId, lastId] });
    expect(projection.flatBlockOutlines.map(({ outline }) => outline.id))
      .toEqual([firstId, child.id, lastId]);
  });

  it("merges, splits, and undoes before storage responds without resurrecting stale text", async () => {
    const h = editor();
    await h.actions.splitTextBlockAtSelection(firstId, 5, 5);
    const newId = h.projection.childIdsByParentId[pageId][1];
    await h.actions.updateBlockText(newId, "Second edited");
    await h.actions.mergeBlockWithPrevious(newId);
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, lastId]);
    expect(blockPlainText(h.projection.blocksById[firstId])).toBe("FirstSecond edited");
    expect(h.focus).toHaveBeenLastCalledWith(firstId, { start: 5, end: 5 });
    await h.undo.undo();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, newId, lastId]);
    await h.actions.updateBlockText(newId, "After undo");
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(blockPlainText(h.stored.get(firstId)!)).toBe("First");
    expect(blockPlainText(h.stored.get(newId)!)).toBe("After undo");
    expect(h.stored.get(newId)?.in_trash).toBe(false);
    expect(blockPlainText(h.projection.blocksById[newId])).toBe("After undo");
  });

  it("preserves middle block order across grouped Enter undo and redo", async () => {
    const h = editor();
    await h.actions.splitTextBlockAtSelection(firstId, 5, 5);
    const second = h.projection.childIdsByParentId[pageId][1];
    await h.actions.splitTextBlockAtSelection(second, 3, 3);
    const expected = [...h.projection.childIdsByParentId[pageId]];
    await h.undo.undo();
    expect(h.projection.childIdsByParentId[pageId]).toEqual([firstId, lastId]);
    await h.undo.redo();
    expect(h.projection.childIdsByParentId[pageId]).toEqual(expected);
    expect(h.projection.flatBlockOutlines.map((item) => item.outline.id)).toEqual(expected);
    h.release();
    await h.persistence.flushPendingBlockSaves();
    expect(h.error).not.toHaveBeenCalled();
  });

  it("keeps unloaded siblings in the outline when inserting within a loaded window", () => {
    const h = editor();
    const unloaded = fromWrite(createBlockWrite(crypto.randomUUID(), "paragraph", "Unloaded"));
    h.projection.replaceOutlines([h.projection.blocksById[firstId], unloaded, h.projection.blocksById[lastId]]
      .map((block, index) => notesBlockOutlineFromBlock(block, pageId, index)), pageId);
    const inserted = fromWrite(createBlockWrite(crypto.randomUUID(), "paragraph", "New"));
    h.projection.insertBlockAfter(inserted, firstId);
    expect(h.projection.flatBlockOutlines.map((item) => item.outline.id)).toEqual([firstId, inserted.id, unloaded.id, lastId]);
    h.release();
  });

});
