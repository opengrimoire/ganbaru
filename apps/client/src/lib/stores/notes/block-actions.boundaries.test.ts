import { createNotesCompoundTestAdapter } from "./compound-edits.test-helpers";
import type { NotesCompoundEdit } from "$lib/api/notes/compound-edits";
import { describe, expect, it, vi } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import { createManagedMediaPayload } from "$lib/notes/block-types/media";
import { flattenNotesBlockTree, type NotesTreeState } from "$lib/notes/blocks/tree";
import { applyNotesPostMutationToTree } from "$lib/notes/post-mutation";
import type { NotesBlock, NotesBlockWrite, NotesParent } from "$lib/notes/types";
import { createNotesBlockPasteActions } from "$lib/stores/notes/block-actions/paste";
import { createNotesBlockDuplicationActions } from "$lib/stores/notes/block-actions/duplication";
import { createNotesBlockMovementActions } from "$lib/stores/notes/block-actions/movement";
import { createNotesMediaBlockActions } from "$lib/stores/notes/block-actions/media";
import { notesTreeStateWithoutLeafBlock } from "./block-tree";

const assetCache = vi.hoisted(() => ({ invalidateAssetUrl: vi.fn() }));
const notesApi = vi.hoisted(() => ({ trashNotesBlock: vi.fn(), updateNotesBlock: vi.fn(), appendNotesBlockChildren: vi.fn() }));

vi.mock("$lib/api/asset-url-cache", () => assetCache);
vi.mock("$lib/api/notes", () => notesApi);
let compoundAdapter = createNotesCompoundTestAdapter(notesApi);
vi.mock("$lib/api/notes/compound-edits", () => ({ applyNotesCompoundEdit: (request: NotesCompoundEdit) => compoundAdapter(request) }));

const pageId = "00000000-0000-4000-8000-000000000001";
const parent: NotesParent = { type: "page_id", page_id: pageId };

function paragraph(id: string, text: string): NotesBlock {
  const write = createBlockWrite(id, "paragraph", text);
  if (write.type !== "paragraph") throw new Error("expected paragraph fixture");
  return {
    object: "block", edit_revision: "0".repeat(64),
    id,
    parent,
    created_time: "2026-07-12T00:00:00Z",
    last_edited_time: "2026-07-12T00:00:00Z",
    has_children: false,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    type: "paragraph",
    paragraph: write.paragraph,
  };
}

describe("Notes block action boundaries", () => {
  it("keeps a deleted leaf removed when its pending insert completes later", async () => {
    notesApi.trashNotesBlock.mockReset().mockResolvedValue(undefined);
    const first = paragraph("first-block", "A");
    const deleted = paragraph("new-block", "");
    let state: NotesTreeState = {
      blocksById: { [first.id]: first, [deleted.id]: deleted },
      childIdsByParentId: { [pageId]: [first.id, deleted.id] },
    };
    let finishInsert!: () => void;
    const pendingInsert = new Promise<void>((resolve) => { finishInsert = resolve; });
    let pendingDelete: Promise<void> | null = null;
    const actions = createNotesBlockMovementActions({
      ensurePageBody: () => null,
      enqueueEditorMutation: async (mutation) => { await pendingInsert; await mutation(); },
      localApplyBlockUpdate: () => undefined,
      readSelectedPageId: () => pageId,
      treeState: () => state,
      blockById: (id) => state.blocksById[id],
      flatBlockItemsForBlockContext: () => flattenNotesBlockTree(state, pageId),
      requestBlockFocus: () => undefined,
      flushBlockSave: async () => undefined,
      flushPendingBlockSaves: async () => undefined,
      localRemoveLeafBlock: (id) => {
        const next = notesTreeStateWithoutLeafBlock(state, id);
        if (!next) return false;
        state = next;
        return true;
      },
      loadPageTree: async () => undefined,
      applyPostMutation: (result) => { state = applyNotesPostMutationToTree(state, result); },
      createUndoSnapshot: () => null,
      replaceBlockWithUpdate: async () => undefined,
      moveAndApply: async () => first,
      moveManyAndApply: async () => [],
      trashAndApply: async () => undefined,
      pendingOptimisticWrite: (id) => id === deleted.id ? pendingInsert : null,
      trackOptimisticBlockWrites: (_ids, persistence) => { pendingDelete = persistence; },
      undoSnapshot: () => null,
      recordUndo: () => undefined,
      recordUndoAfter: () => undefined,
    });

    await actions.deleteBlock(deleted.id);
    expect(state.blocksById[deleted.id]).toBeUndefined();
    finishInsert();
    if (!pendingDelete) throw new Error("delete persistence was not tracked");
    await pendingDelete;

    expect(notesApi.trashNotesBlock).toHaveBeenCalledWith(deleted.id, true);
    expect(state.blocksById[deleted.id]).toBeUndefined();
    expect(state.childIdsByParentId[pageId]).toEqual([first.id]);
  });

  it("reports failed paste persistence without reloading away the local draft", async () => {
    const block = paragraph("block-a", "Before");
    const events: string[] = [];
    let persistence: Promise<void> | null = null;
    notesApi.updateNotesBlock.mockImplementation(async () => { events.push("update"); });
    notesApi.appendNotesBlockChildren.mockImplementation(async () => { events.push("append"); throw new Error("append failed"); });
    const actions = createNotesBlockPasteActions({
      applyPostMutation: () => undefined,
      enqueueEditorMutation: (mutation) => mutation(),
      readSelectedPageId: () => pageId,
      readChildIdsByParentId: () => ({ [pageId]: [block.id] }),
      blockById: (id) => id === block.id ? block : undefined,
      localApplyBlockUpdate: () => undefined,
      localInsertBlockAfter: () => undefined,
      localInsertBlockBefore: () => undefined,
      requestBlockFocus: () => undefined,
      scheduleBlockSave: () => undefined,
      saveBlockNow: async () => undefined,
      flushBlockSave: async () => { events.push("flush"); },
      loadPageTree: async () => { events.push("reload"); },
      appendAndApply: async () => {
        events.push("append");
        throw new Error("append failed");
      },
      pendingOptimisticWrite: () => null,
      trackOptimisticBlockWrites: (_ids, next) => { persistence = next; },
      optimisticBlockFromWrite: (write: NotesBlockWrite, nextParent: NotesParent) => ({
        ...paragraph(write.id, ""),
        parent: nextParent,
      }),
      undoSnapshotForBlocks: () => null,
      recordUndo: () => undefined,
    });

    expect(await actions.pastePlainTextIntoBlock(block.id, 6, 6, "\nAfter")).toBe(true);
    if (!persistence) throw new Error("paste persistence was not tracked");
    await expect(persistence).rejects.toThrow("append failed");

    expect(events).toEqual(["update", "append"]);
  });

  it("does not focus or record duplication after the backend fails", async () => {
    const block = paragraph("block-a", "A");
    const requestBlockFocus = vi.fn();
    const recordUndoAfter = vi.fn();
    const actions = createNotesBlockDuplicationActions({
      readSelectedPageId: () => pageId,
      blockById: (id) => id === block.id ? block : undefined,
      treeState: () => ({ blocksById: { [block.id]: block }, childIdsByParentId: { [pageId]: [block.id] } }),
      outlineSubtreeIds: () => [block.id],
      flushBlockSave: async () => undefined,
      flushPendingBlockSaves: async () => undefined,
      requestBlockFocus,
      duplicateSubtreesAndApply: async () => { throw new Error("duplicate failed"); },
      undoSnapshot: () => null,
      recordUndoAfter,
    });

    await expect(actions.duplicateBlock(block.id)).rejects.toThrow("duplicate failed");
    expect(requestBlockFocus).not.toHaveBeenCalled();
    expect(recordUndoAfter).not.toHaveBeenCalled();
  });

  it("does not trash a parent when child reparenting fails", async () => {
    const parentBlock = {
      ...paragraph("parent-block", "Parent"),
      type: "toggle" as const,
      has_children: true,
      toggle: { rich_text: [], color: "default" as const },
    } satisfies NotesBlock;
    const child = {
      ...paragraph("child-block", "Child"),
      parent: { type: "block_id" as const, block_id: parentBlock.id },
    };
    const state: NotesTreeState = {
      blocksById: { [parentBlock.id]: parentBlock, [child.id]: child },
      childIdsByParentId: { [pageId]: [parentBlock.id], [parentBlock.id]: [child.id] },
    };
    const trashAndApply = vi.fn();
    compoundAdapter = createNotesCompoundTestAdapter({ ...notesApi, moveNotesBlock: async () => { throw new Error("move failed"); } }, () => Object.values(state.blocksById));
    const actions = createNotesBlockMovementActions({
      ensurePageBody: () => null,
      enqueueEditorMutation: (mutation) => mutation(),
      localApplyBlockUpdate: () => undefined,
      readSelectedPageId: () => pageId,
      treeState: () => state,
      blockById: (id) => state.blocksById[id],
      flatBlockItemsForBlockContext: () => flattenNotesBlockTree(state, pageId),
      requestBlockFocus: vi.fn(),
      flushBlockSave: async () => undefined,
      flushPendingBlockSaves: async () => undefined,
      localRemoveLeafBlock: () => false,
      loadPageTree: async () => undefined,
      applyPostMutation: () => undefined,
      createUndoSnapshot: () => null,
      replaceBlockWithUpdate: async () => undefined,
      moveAndApply: async () => { throw new Error("move failed"); },
      moveManyAndApply: async () => [],
      trashAndApply,
      pendingOptimisticWrite: () => null,
      trackOptimisticBlockWrites: () => undefined,
      undoSnapshot: () => null,
      recordUndo: () => undefined,
      recordUndoAfter: () => undefined,
    });

    await expect(actions.deleteBlock(parentBlock.id)).rejects.toThrow("move failed");
    expect(trashAndApply).not.toHaveBeenCalled();
  });

  it("does not focus or record a drop after the move fails", async () => {
    const first = paragraph("first-block", "First");
    const second = paragraph("second-block", "Second");
    const state: NotesTreeState = {
      blocksById: { [first.id]: first, [second.id]: second },
      childIdsByParentId: { [pageId]: [first.id, second.id] },
    };
    const requestBlockFocus = vi.fn();
    const recordUndoAfter = vi.fn();
    const actions = createNotesBlockMovementActions({
      ensurePageBody: () => null,
      enqueueEditorMutation: (mutation) => mutation(),
      localApplyBlockUpdate: () => undefined,
      readSelectedPageId: () => pageId,
      treeState: () => state,
      blockById: (id) => state.blocksById[id],
      flatBlockItemsForBlockContext: () => flattenNotesBlockTree(state, pageId),
      requestBlockFocus,
      flushBlockSave: async () => undefined,
      flushPendingBlockSaves: async () => undefined,
      localRemoveLeafBlock: () => false,
      loadPageTree: async () => undefined,
      applyPostMutation: () => undefined,
      createUndoSnapshot: () => null,
      replaceBlockWithUpdate: async () => undefined,
      moveAndApply: async () => { throw new Error("drop failed"); },
      moveManyAndApply: async () => [],
      trashAndApply: async () => undefined,
      pendingOptimisticWrite: () => null,
      trackOptimisticBlockWrites: () => undefined,
      undoSnapshot: () => null,
      recordUndo: () => undefined,
      recordUndoAfter,
    });

    await expect(actions.dropBlockOnBlock(first.id, second.id, "after")).rejects.toThrow(
      "drop failed",
    );
    expect(requestBlockFocus).not.toHaveBeenCalled();
    expect(recordUndoAfter).not.toHaveBeenCalled();
  });

  it("invalidates a replaced managed media asset", async () => {
    assetCache.invalidateAssetUrl.mockClear();
    const path = "notes/files/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png";
    const block: NotesBlock = {
      ...paragraph("image-block", ""),
      type: "image",
      image: createManagedMediaPayload({
        relativePath: path,
        originalName: "image.png",
        contentType: "image/png",
        byteSize: 42,
        sha256: "a".repeat(64),
        kind: "image",
      }),
    };
    const actions = createNotesMediaBlockActions({
      blockById: () => block,
      localApplyBlockUpdate: () => undefined,
      scheduleBlockSave: () => undefined,
      createUndoSnapshot: () => null,
      recordUndo: () => undefined,
    });

    await actions.updateMedia(block.id, "https://example.com/image.png", "");

    expect(assetCache.invalidateAssetUrl).toHaveBeenCalledWith("notes-file", path);
  });
});
