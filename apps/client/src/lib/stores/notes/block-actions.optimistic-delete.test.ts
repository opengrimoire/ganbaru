import { createNotesCompoundTestAdapter } from "./compound-edits.test-helpers";
import type { NotesCompoundEdit } from "$lib/api/notes/compound-edits";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import {
  buildNotesChildIdsByParent,
  flattenNotesBlockTree,
  type NotesTreeState,
} from "$lib/notes/blocks/tree";
import {
  createNotesUndoSnapshot,
  createNotesUndoSnapshotForBlocks,
  type NotesUndoRecordOptions,
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
import { notesTreeStateWithoutLeafBlock } from "./block-tree";
import { collectLoadedBlockSubtreeIds } from "$lib/notes/blocks/duplicate";

const notesApi = vi.hoisted(() => ({
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

  it("removes an empty leaf and transfers focus before persistence finishes", async () => {
    let resolveTrash!: () => void;
    const trashPromise = new Promise<void>((resolve) => {
      resolveTrash = resolve;
    });
    notesApi.trashNotesBlock.mockReturnValue(trashPromise);
    let state: NotesTreeState = {
      blocksById: {
        [firstBlockId]: paragraph(firstBlockId, "A"),
        [emptyBlockId]: paragraph(emptyBlockId, ""),
      },
      childIdsByParentId: buildNotesChildIdsByParent([
        paragraph(firstBlockId, "A"),
        paragraph(emptyBlockId, ""),
      ]),
    };
    const requestBlockFocus = vi.fn();
    const recordUndo = vi.fn<(options: Omit<NotesUndoRecordOptions, "id">) => void>();
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
      requestBlockFocus,
      createChildPageFromBlock: async () => undefined,
      createChildPageAfterBlock: async () => undefined,
      applyPostMutation: () => undefined,
      loadPageTree: async () => undefined,
      refreshOpenLinks: async () => undefined,
      localApplyBlockUpdate: (_blockId: string, _update: NotesBlockUpdate) => undefined,
      localInsertBlockAfter: () => undefined,
      localInsertBlockBefore: () => undefined,
      localRemoveLeafBlock: (blockId) => {
        const next = notesTreeStateWithoutLeafBlock(state, blockId);
        if (!next) return false;
        state = next;
        return true;
      },
      saveBlockNow: async () => undefined,
      scheduleBlockSave: () => undefined,
      flushBlockSave: async () => undefined,
      flushPendingBlockSaves: async () => undefined,
      createUndoSnapshot: (focusBlockId, extraBlocks = [], focusSelection = null) =>
        createNotesUndoSnapshot(pageId, state, focusBlockId, extraBlocks, focusSelection),
      createUndoSnapshotForBlocks: (
        focusBlockId,
        blockIds,
        extraBlocks = [],
        focusSelection = null,
      ) => createNotesUndoSnapshotForBlocks(
        pageId,
        state,
        focusBlockId,
        blockIds,
        extraBlocks,
        focusSelection,
      ),
      recordUndo,
    };
    const actions = createNotesBlockActions(context);

    await actions.deleteBlock(emptyBlockId);

    expect(state.blocksById[emptyBlockId]).toBeUndefined();
    expect(requestBlockFocus).toHaveBeenCalledWith(
      firstBlockId,
      { start: 1, end: 1 },
    );
    expect(recordUndo).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => expect(notesApi.trashNotesBlock).toHaveBeenCalledTimes(1));

    resolveTrash();
    await actions.flushOptimisticBlockWrites();
    expect(notesApi.trashNotesBlock).toHaveBeenCalledWith(emptyBlockId, true);
  });

});
