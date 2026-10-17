import { createNotesCompoundTestAdapter } from "./compound-edits.test-helpers";
import type { NotesCompoundEdit } from "$lib/api/notes/compound-edits";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyBlockUpdate, createBlockWrite } from "$lib/notes/blocks/factory";
import {
  buildNotesChildIdsByParent,
  flattenNotesBlockTree,
  type NotesTreeState,
} from "$lib/notes/blocks/tree";
import {
  createNotesUndoSnapshot,
  createNotesUndoSnapshotForBlocks,
} from "$lib/notes/history/undo-history";
import type {
  NotesBlock,
  NotesBlockUpdate,
  NotesBlockWrite,
  NotesParent,
} from "$lib/notes/types";
import {
  createNotesBlockActions,
  type NotesBlockActionsContext,
} from "./block-actions";
import { applyNotesPostMutationToTree } from "$lib/notes/post-mutation";
import { collectLoadedBlockSubtreeIds } from "$lib/notes/blocks/duplicate";

const notesApi = vi.hoisted(() => ({
  updateNotesBlock: vi.fn(),
  appendNotesBlockChildren: vi.fn(),
  moveNotesBlock: vi.fn(),
  trashNotesBlock: vi.fn(),
  trashNotesBlocks: vi.fn(),
}));

vi.mock("$lib/api/notes", () => notesApi);
let compoundAdapter = createNotesCompoundTestAdapter(notesApi);
vi.mock("$lib/api/notes/compound-edits", () => ({ applyNotesCompoundEdit: (request: NotesCompoundEdit) => compoundAdapter(request) }));

const pageId = "00000000-0000-4000-8000-000000000001";
const firstBlockId = "00000000-0000-4000-8000-000000000002";
const emptyBlockId = "00000000-0000-4000-8000-000000000003";
const parent: NotesParent = { type: "page_id", page_id: pageId };
const now = "2026-07-10T12:00:00.000Z";

function blockFromWrite(write: NotesBlockWrite): NotesBlock {
  if (write.type !== "paragraph") throw new Error("expected paragraph fixture");
  return {
    object: "block", edit_revision: "0".repeat(64),
    id: write.id,
    parent,
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    type: write.type,
    paragraph: write.paragraph,
  };
}

function paragraph(id: string, text: string): NotesBlock {
  return blockFromWrite(createBlockWrite(id, "paragraph", text));
}

describe("notes store block actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("keeps typing, formatting, insert, move, and delete reads at zero", async () => {
    const second = paragraph(emptyBlockId, "B");
    let state: NotesTreeState = {
      blocksById: { [firstBlockId]: paragraph(firstBlockId, "A"), [emptyBlockId]: second },
      childIdsByParentId: buildNotesChildIdsByParent([paragraph(firstBlockId, "A"), second]),
    };
    const loadPageTree = vi.fn(async () => undefined);
    const scheduleBlockSave = vi.fn();
    const saveBlockNow = vi.fn(async (blockId: string, update: NotesBlockUpdate) => {
      const block = state.blocksById[blockId];
      if (!block) return;
      state = {
        ...state,
        blocksById: { ...state.blocksById, [blockId]: applyBlockUpdate(block, update) },
      };
    });
    notesApi.appendNotesBlockChildren.mockImplementation(async (request: {
      parent: NotesParent;
      children: NotesBlockWrite[];
    }) => ({
      object: "list",
      type: "block",
      block: {},
      results: request.children.map((write) => ({
        ...blockFromWrite(write),
        parent: request.parent,
      })),
      next_cursor: null,
      has_more: false,
    }));
    notesApi.moveNotesBlock.mockImplementation(async (
      blockId: string,
      request: { parent: NotesParent },
    ) => ({ ...state.blocksById[blockId], parent: request.parent }));
    notesApi.trashNotesBlocks.mockResolvedValue({
      object: "list",
      type: "block",
      block: {},
      results: [],
      next_cursor: null,
      has_more: false,
    });
    const context: NotesBlockActionsContext = {
      readPageRootBlockIds: () => state.childIdsByParentId[pageId] ?? [],
      enqueueEditorMutation: (mutation) => mutation(),
      awaitSelectedPageReady: () => Promise.resolve(),
      readSelectedPageId: () => pageId,
      readBlocksById: () => state.blocksById,
      readChildIdsByParentId: () => Object.fromEntries(
        Object.entries(state.childIdsByParentId).map(([key, childIds]) => [key, [...childIds]]),
      ),
      treeState: () => state,
      outlineSubtreeIds: (rootIds) => rootIds.flatMap((id) => collectLoadedBlockSubtreeIds(state, id)),
      blockById: (blockId) => state.blocksById[blockId],
      flatBlockItemsForBlockContext: () => flattenNotesBlockTree(state, pageId),
      tableRowsForBlock: () => [],
      columnItemsForBlock: () => [],
      tabItemsForBlock: () => [],
      requestBlockFocus: () => undefined,
      createChildPageFromBlock: async () => undefined,
      createChildPageAfterBlock: async () => undefined,
      applyPostMutation: (result) => {
        state = applyNotesPostMutationToTree(state, result);
      },
      loadPageTree,
      refreshOpenLinks: async () => undefined,
      localApplyBlockUpdate: (blockId, update) => {
        const block = state.blocksById[blockId];
        if (!block) return;
        state = {
          ...state,
          blocksById: { ...state.blocksById, [blockId]: applyBlockUpdate(block, update) },
        };
      },
      localInsertBlockAfter: () => undefined,
      localInsertBlockBefore: () => undefined,
      localRemoveLeafBlock: () => false,
      saveBlockNow,
      scheduleBlockSave,
      flushBlockSave: async () => undefined,
      flushPendingBlockSaves: async () => undefined,
      createUndoSnapshot: (focusBlockId, extraBlocks = [], focusSelection = null) =>
        createNotesUndoSnapshot(pageId, state, focusBlockId, extraBlocks, focusSelection),
      createUndoSnapshotForBlocks: (focusBlockId, blockIds, extraBlocks = [], focusSelection = null) =>
        createNotesUndoSnapshotForBlocks(
          pageId,
          state,
          focusBlockId,
          blockIds,
          extraBlocks,
          focusSelection,
        ),
      recordUndo: () => undefined,
    };
    const actions = createNotesBlockActions(context);

    await actions.updateBlockText(firstBlockId, "Typed");
    await actions.updateBlockColor(firstBlockId, "blue");
    await actions.createSiblingAfter(firstBlockId);
    const insertedId = (state.childIdsByParentId[pageId] ?? []).find(
      (blockId) => blockId !== firstBlockId && blockId !== emptyBlockId,
    );
    expect(insertedId).toBeTruthy();
    await actions.moveBlockDown(firstBlockId);
    if (insertedId) await actions.deleteBlockSelection([insertedId]);

    expect(loadPageTree).toHaveBeenCalledTimes(0);
    expect(scheduleBlockSave).toHaveBeenCalledTimes(1);
    expect(saveBlockNow).toHaveBeenCalledTimes(0);
    expect(notesApi.updateNotesBlock).toHaveBeenCalledTimes(1);
    expect(notesApi.appendNotesBlockChildren).toHaveBeenCalledTimes(1);
    expect(notesApi.moveNotesBlock).toHaveBeenCalledTimes(1);
    expect(notesApi.trashNotesBlock).toHaveBeenCalledTimes(1);
    expect(notesApi.trashNotesBlocks).toHaveBeenCalledTimes(0);
  });
});
