import { createNotesCompoundPersistence, enqueueNotesCompoundEdit } from "./compound-edits";
import { reconcileNotesCompoundUndo } from "$lib/notes/history/undo-compound";
import type { NotesEditOperation } from "$lib/api/notes/compound-edits";
import {
  moveNotesBlock,
} from "$lib/api/notes";
import { collectLoadedBlockSubtreeIds } from "$lib/notes/blocks/duplicate";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import {
  createNotesColumnWrite,
  notesColumnCanAdd,
  notesColumnCanMove,
  notesColumnResizeWidths,
  notesColumnWithWidthRatio,
  planNotesColumnInsertion,
  planNotesColumnRemoval,
  type NotesColumnMoveDirection,
} from "$lib/notes/block-types/column";
import type {
  NotesBlock,
  NotesBlockUpdate,
  NotesColumnBlock,
  NotesColumnBlockItems,
} from "$lib/notes/types";
import type {
  NotesUndoKind,
  NotesUndoRecordOptions,
  NotesUndoSnapshot,
} from "$lib/notes/history/undo-history";
import type { NotesTreeState } from "$lib/notes/blocks/tree";
import {
  notesPostMoveResult,
  type NotesPostMutationResult,
} from "$lib/notes/post-mutation";

export interface NotesColumnActionsContext {
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  readSelectedPageId: () => string | null;
  readChildIdsByParentId: () => Record<string, string[]>;
  treeState: () => NotesTreeState;
  blockById: (blockId: string) => NotesBlock | undefined;
  columnItemsForBlock: (blockId: string) => NotesColumnBlockItems[];
  requestBlockFocus: (blockId: string | null) => void;
  applyPostMutation: (result: NotesPostMutationResult) => void;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  saveBlockNow: (blockId: string, update: NotesBlockUpdate) => Promise<void>;
  flushPendingBlockSaves: () => Promise<void>;
  createUndoSnapshot: (
    focusBlockId: string | null,
    extraBlocks?: readonly NotesBlock[],
  ) => NotesUndoSnapshot | null;
  recordUndo: (options: Omit<NotesUndoRecordOptions, "id">) => void;
}

export interface NotesColumnActions {
  addColumn: (columnListBlockId: string, afterColumnIndex: number) => Promise<void>;
  removeColumn: (columnListBlockId: string, columnBlockId: string) => Promise<void>;
  moveColumn: (
    columnListBlockId: string,
    columnBlockId: string,
    direction: NotesColumnMoveDirection,
  ) => Promise<void>;
  resizeColumn: (
    columnListBlockId: string,
    columnBlockId: string,
    widthRatio: number,
  ) => Promise<void>;
  moveBlockToColumn: (blockId: string, columnBlockId: string) => Promise<void>;
}

/**
 * Create Notes column layout mutation and UI action methods.
 */
export function createNotesColumnActions(
  context: NotesColumnActionsContext,
): NotesColumnActions {
  function undoSnapshot(
    focusBlockId: string | null,
    extraBlocks: readonly NotesBlock[] = [],
  ): NotesUndoSnapshot | null {
    return context.createUndoSnapshot(focusBlockId, extraBlocks);
  }

  function recordUndo(
    kind: NotesUndoKind,
    before: NotesUndoSnapshot | null,
    after: NotesUndoSnapshot | null,
    groupKey: string | null = null,
  ): void {
    context.recordUndo({ kind, before, after, groupKey });
  }

  function recordUndoAfter(
    kind: NotesUndoKind,
    before: NotesUndoSnapshot | null,
    focusBlockId: string | null,
    groupKey: string | null = null,
    extraBlocks: readonly NotesBlock[] = [],
  ): void {
    recordUndo(kind, before, undoSnapshot(focusBlockId, extraBlocks), groupKey);
  }

  function columnBlocksForList(columnListBlockId: string): NotesColumnBlock[] {
    return context.columnItemsForBlock(columnListBlockId).map((item) => item.column);
  }

  function activeChildBlockIds(parentBlockId: string): string[] {
    return (context.readChildIdsByParentId()[parentBlockId] ?? []).filter((blockId) => {
      const block = context.blockById(blockId);
      return block !== undefined && !block.in_trash;
    });
  }

  function widthOperations(columns: readonly NotesColumnBlock[], widths: readonly number[]): NotesEditOperation[] {
    return columns.flatMap((column, index) => widths[index] === undefined ? [] : [{ type: "update" as const, block_id: column.id, update: notesColumnWithWidthRatio(widths[index]) }]);
  }

  async function addColumn(columnListBlockId: string, afterColumnIndex: number): Promise<void> {
    const columnList = context.blockById(columnListBlockId);
    if (!context.readSelectedPageId() || !columnList || columnList.type !== "column_list") return;
    const columns = columnBlocksForList(columnListBlockId);
    if (!notesColumnCanAdd(columns.length)) return;
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(columnListBlockId);
    const operations: NotesEditOperation[] = [];
    let focusId: string;
    if (columns.length === 0) {
      // A partially loaded layout must be hydrated before inserting its initial columns.
      if (columnList.has_children) throw new Error("Notes columns are not loaded");
      const left = crypto.randomUUID(), right = crypto.randomUUID();
      focusId = crypto.randomUUID();
      operations.push(
        { type: "append", request: { parent: { type: "block_id", block_id: columnListBlockId }, after: null, children: [createNotesColumnWrite(left, 0.5), createNotesColumnWrite(right, 0.5)] } },
        { type: "append", request: { parent: { type: "block_id", block_id: left }, after: null, children: [createBlockWrite(focusId, "paragraph")] } },
        { type: "append", request: { parent: { type: "block_id", block_id: right }, after: null, children: [createBlockWrite(crypto.randomUUID(), "paragraph")] } },
      );
    } else {
      const plan = planNotesColumnInsertion(columns, afterColumnIndex);
      if (!plan) return;
      const id = crypto.randomUUID();
      focusId = crypto.randomUUID();
      operations.push(...widthOperations(columns, columns.map((_, index) => plan.widths[index < plan.insertIndex ? index : index + 1])));
      operations.push(
        { type: "append", request: { parent: { type: "block_id", block_id: columnListBlockId }, after: columns[Math.max(0, plan.insertIndex - 1)]?.id ?? columns.at(-1)?.id ?? null, children: [createNotesColumnWrite(id, plan.widths[plan.insertIndex] ?? 1)] } },
        { type: "append", request: { parent: { type: "block_id", block_id: id }, after: null, children: [createBlockWrite(focusId, "paragraph")] } },
      );
    }
    await enqueueNotesCompoundEdit(context, "column_layout", operations);
    context.requestBlockFocus(focusId);
    recordUndoAfter("create", before, focusId);
  }

  async function removeColumn(columnListBlockId: string, columnBlockId: string): Promise<void> {
    const columnList = context.blockById(columnListBlockId);
    if (!context.readSelectedPageId() || !columnList || columnList.type !== "column_list") return;
    const columns = columnBlocksForList(columnListBlockId);
    const plan = planNotesColumnRemoval(columns, columnBlockId);
    if (!plan) return;
    const removed = columns[plan.removeIndex], target = columns[plan.targetIndex];
    if (!removed || !target) return;
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(columnListBlockId);
    const targetIds = activeChildBlockIds(target.id);
    const persist = createNotesCompoundPersistence(context, "column_layout", [
      { type: "move_children", source_block_id: removed.id, parent: { type: "block_id", block_id: target.id }, after: targetIds.at(-1) ?? null },
      { type: "trash", block_id: removed.id, in_trash: true },
      ...widthOperations(columns.filter((column) => column.id !== removed.id), plan.widths),
    ], Object.values(context.treeState().blocksById));
    await context.enqueueEditorMutation(async () => {
      const result = await persist();
      context.applyPostMutation({ blocks: result.blocks.filter((block) => !block.in_trash), placements: result.placements, removedBlockIds: [removed.id] });
      const after = undoSnapshot(columnListBlockId);
      reconcileNotesCompoundUndo(before, after, result);
      context.recordUndo({ kind: "delete", before, after });
    });
    const focusId = activeChildBlockIds(target.id)[0] ?? columnListBlockId;
    context.requestBlockFocus(focusId);
  }

  async function moveColumn(
    columnListBlockId: string,
    columnBlockId: string,
    direction: NotesColumnMoveDirection,
  ): Promise<void> {
    const selectedPageId = context.readSelectedPageId();
    const columnList = context.blockById(columnListBlockId);
    if (!selectedPageId || !columnList || columnList.type !== "column_list") return;
    const columns = columnBlocksForList(columnListBlockId);
    if (!notesColumnCanMove(columns, columnBlockId, direction)) return;
    const columnIndex = columns.findIndex((column) => column.id === columnBlockId);
    const targetColumn = columns[direction === "left" ? columnIndex - 1 : columnIndex + 1];
    if (!targetColumn) return;
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(columnListBlockId);
    const moveRequest = {
      parent: { type: "block_id", block_id: columnListBlockId },
      after: direction === "right" ? targetColumn.id : null,
      before: direction === "left" ? targetColumn.id : null,
    } as const;
    context.applyPostMutation(notesPostMoveResult(
      await moveNotesBlock(columnBlockId, moveRequest),
      moveRequest,
    ));
    context.requestBlockFocus(columnListBlockId);
    recordUndoAfter("move", before, columnListBlockId);
  }

  async function resizeColumn(
    columnListBlockId: string,
    columnBlockId: string,
    widthRatio: number,
  ): Promise<void> {
    const columnList = context.blockById(columnListBlockId);
    if (!columnList || columnList.type !== "column_list") return;
    const columns = columnBlocksForList(columnListBlockId);
    const widths = notesColumnResizeWidths(columns, columnBlockId, widthRatio);
    if (!widths) return;
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(columnListBlockId);
    await enqueueNotesCompoundEdit(context, "column_layout", widthOperations(columns, widths));
    context.requestBlockFocus(columnListBlockId);
    recordUndoAfter("update", before, columnListBlockId, `update:${columnListBlockId}:columns`);
  }

  async function moveBlockToColumn(blockId: string, columnBlockId: string): Promise<void> {
    const selectedPageId = context.readSelectedPageId();
    const block = context.blockById(blockId);
    const column = context.blockById(columnBlockId);
    if (!selectedPageId || !block || !column || column.type !== "column") return;
    if (block.type === "column" || block.type === "table_row") return;
    if (collectLoadedBlockSubtreeIds(context.treeState(), blockId).includes(columnBlockId)) return;
    const originalChildIds = activeChildBlockIds(columnBlockId);
    if (
      block.parent.type === "block_id"
      && block.parent.block_id === columnBlockId
      && originalChildIds.at(-1) === blockId
    ) {
      return;
    }
    const childIds = originalChildIds.filter((childId) => childId !== blockId);
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(blockId);
    const moveRequest = {
      parent: { type: "block_id", block_id: columnBlockId },
      after: childIds.at(-1) ?? null,
      before: null,
    } as const;
    context.applyPostMutation(notesPostMoveResult(
      await moveNotesBlock(blockId, moveRequest),
      moveRequest,
    ));
    context.requestBlockFocus(blockId);
    recordUndoAfter("move", before, blockId);
  }

  return {
    addColumn,
    removeColumn,
    moveColumn,
    resizeColumn,
    moveBlockToColumn,
  };
}
