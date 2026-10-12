import { createNotesCompoundPersistence } from "$lib/stores/notes/compound-edits";
import { reconcileNotesCompoundUndo } from "$lib/notes/history/undo-compound";
import type { NotesPostMutationResult } from "$lib/notes/post-mutation";
import {
  createNotesTableRowWrite,
  notesTableCanAddColumn,
  notesTableCanAddRow,
  notesTableCanRemoveColumn,
  notesTableCanRemoveRow,
  notesTableRowWithCellRichText,
  notesTableRowWithCellText,
  notesTableRowWithInsertedColumn,
  notesTableRowWithRemovedColumn,
  notesTableVisibleWidth,
  notesTableWithWidth,
} from "$lib/notes/block-types/table";
import type {
  NotesAppendBlockChildrenRequest,
  NotesBlock,
  NotesBlockUpdate,
  NotesRichText,
  NotesTableRowBlock,
} from "$lib/notes/types";
import type { NotesUndoSnapshot, NotesUndoRecordOptions } from "$lib/notes/history/undo-history";

interface NotesTableBlockActionsContext {
  recordUndo: (options: Omit<NotesUndoRecordOptions, "id">) => void;
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  applyPostMutation: (result: NotesPostMutationResult) => void;
  readSelectedPageId: () => string | null;
  blockById: (blockId: string) => NotesBlock | undefined;
  tableRowsForBlock: (blockId: string) => NotesTableRowBlock[];
  requestBlockFocus: (blockId: string | null) => void;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  scheduleBlockSave: (blockId: string, update: NotesBlockUpdate) => void;
  saveBlockNow: (blockId: string, update: NotesBlockUpdate) => Promise<void>;
  flushPendingBlockSaves: () => Promise<void>;
  appendAndApply: (request: NotesAppendBlockChildrenRequest) => Promise<NotesBlock[]>;
  trashAndApply: (rootBlockIds: readonly string[]) => Promise<void>;
  replaceBlockWithUpdate: (blockId: string, update: NotesBlockUpdate) => Promise<void>;
  undoSnapshot: (focusBlockId: string | null) => NotesUndoSnapshot | null;
  recordUndoAfter: (
    kind: "create" | "delete" | "typing" | "update",
    before: NotesUndoSnapshot | null,
    focusBlockId: string | null,
    groupKey?: string | null,
  ) => void;
}

export interface NotesTableBlockActions {
  updateTableCell: (rowBlockId: string, columnIndex: number, text: string) => Promise<void>;
  updateTableCellRichText: (
    rowBlockId: string,
    columnIndex: number,
    richText: readonly NotesRichText[],
  ) => Promise<void>;
  addTableRow: (tableBlockId: string, afterRowIndex: number) => Promise<void>;
  removeTableRow: (tableBlockId: string, rowBlockId: string) => Promise<void>;
  addTableColumn: (tableBlockId: string, afterColumnIndex: number) => Promise<void>;
  removeTableColumn: (tableBlockId: string, columnIndex: number) => Promise<void>;
}

function tableInsertIndex(index: number, width: number): number {
  if (!Number.isFinite(index)) return width;
  return Math.max(0, Math.min(width, Math.trunc(index)));
}

function tableRemoveIndex(index: number, width: number): number {
  if (!Number.isFinite(index)) return Math.max(0, width - 1);
  return Math.max(0, Math.min(Math.max(0, width - 1), Math.trunc(index)));
}

/** Create row, cell, and column mutations for table blocks. */
export function createNotesTableBlockActions(
  context: NotesTableBlockActionsContext,
): NotesTableBlockActions {
  async function updateTableCell(
    rowBlockId: string,
    columnIndex: number,
    text: string,
  ): Promise<void> {
    const block = context.blockById(rowBlockId);
    if (!block || block.type !== "table_row") return;
    const before = context.undoSnapshot(rowBlockId);
    const update = notesTableRowWithCellText(block, columnIndex, text);
    context.localApplyBlockUpdate(rowBlockId, update);
    context.scheduleBlockSave(rowBlockId, update);
    context.recordUndoAfter("typing", before, rowBlockId, `typing:${rowBlockId}:${columnIndex}`);
  }

  async function updateTableCellRichText(
    rowBlockId: string,
    columnIndex: number,
    richText: readonly NotesRichText[],
  ): Promise<void> {
    const block = context.blockById(rowBlockId);
    if (!block || block.type !== "table_row") return;
    const before = context.undoSnapshot(rowBlockId);
    const update = notesTableRowWithCellRichText(block, columnIndex, richText);
    context.localApplyBlockUpdate(rowBlockId, update);
    context.scheduleBlockSave(rowBlockId, update);
    context.recordUndoAfter("typing", before, rowBlockId, `typing:${rowBlockId}:${columnIndex}`);
  }

  async function addTableRow(tableBlockId: string, afterRowIndex: number): Promise<void> {
    const table = context.blockById(tableBlockId);
    if (!context.readSelectedPageId() || !table || table.type !== "table") return;
    const rows = context.tableRowsForBlock(tableBlockId);
    if (!notesTableCanAddRow(rows.length)) return;
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(tableBlockId);
    const rowIndex = tableRemoveIndex(afterRowIndex, Math.max(1, rows.length));
    const after = rows[rowIndex]?.id ?? rows.at(-1)?.id ?? null;
    await context.appendAndApply({
      parent: { type: "block_id", block_id: tableBlockId },
      after,
      children: [createNotesTableRowWrite(crypto.randomUUID(), notesTableVisibleWidth(table, rows))],
    });
    context.requestBlockFocus(tableBlockId);
    context.recordUndoAfter("create", before, tableBlockId);
  }

  async function removeTableRow(tableBlockId: string, rowBlockId: string): Promise<void> {
    const table = context.blockById(tableBlockId);
    if (!context.readSelectedPageId() || !table || table.type !== "table") return;
    const rows = context.tableRowsForBlock(tableBlockId);
    if (!notesTableCanRemoveRow(rows.length) || !rows.some((row) => row.id === rowBlockId)) return;
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(tableBlockId);
    await context.trashAndApply([rowBlockId]);
    context.requestBlockFocus(tableBlockId);
    context.recordUndoAfter("delete", before, tableBlockId);
  }

  async function addTableColumn(tableBlockId: string, afterColumnIndex: number): Promise<void> {
    const table = context.blockById(tableBlockId);
    if (!context.readSelectedPageId() || !table || table.type !== "table") return;
    const rows = context.tableRowsForBlock(tableBlockId);
    const width = notesTableVisibleWidth(table, rows);
    if (!notesTableCanAddColumn(width)) return;
    const insertIndex = tableInsertIndex(afterColumnIndex + 1, width);
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(tableBlockId);
    const emptyRowId = crypto.randomUUID();
    const persist = createNotesCompoundPersistence(context, "table_columns", [{ type: "table_column", table_id: tableBlockId, index: insertIndex, insert: true, empty_row_id: emptyRowId }], rows);
    context.localApplyBlockUpdate(tableBlockId, notesTableWithWidth(table, width + 1));
    for (const row of rows) {
      const update = notesTableRowWithInsertedColumn(row, insertIndex, width);
      context.localApplyBlockUpdate(row.id, update);
    }
    await context.enqueueEditorMutation(async () => {
      const result = await persist();
      const created = result.blocks.find((block) => block.id === emptyRowId);
      if (created) context.applyPostMutation({ blocks: [created], placements: [{ blockId: created.id, parent: created.parent, after: null }], canonical: true });
      const after = context.undoSnapshot(tableBlockId);
      reconcileNotesCompoundUndo(before, after, result);
      context.recordUndo({ kind: "update", before, after });
    });
    context.requestBlockFocus(tableBlockId);
  }

  async function removeTableColumn(tableBlockId: string, columnIndex: number): Promise<void> {
    const table = context.blockById(tableBlockId);
    if (!context.readSelectedPageId() || !table || table.type !== "table") return;
    const rows = context.tableRowsForBlock(tableBlockId);
    const width = notesTableVisibleWidth(table, rows);
    if (!notesTableCanRemoveColumn(width)) return;
    const removeIndex = tableRemoveIndex(columnIndex, width);
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(tableBlockId);
    const persist = createNotesCompoundPersistence(context, "table_columns", [{ type: "table_column", table_id: tableBlockId, index: removeIndex, insert: false, empty_row_id: crypto.randomUUID() }], rows);
    context.localApplyBlockUpdate(tableBlockId, notesTableWithWidth(table, width - 1));
    for (const row of rows) {
      const update = notesTableRowWithRemovedColumn(row, removeIndex, width);
      context.localApplyBlockUpdate(row.id, update);
    }
    await context.enqueueEditorMutation(async () => {
      const result = await persist();
      const after = context.undoSnapshot(tableBlockId);
      reconcileNotesCompoundUndo(before, after, result);
      context.recordUndo({ kind: "update", before, after });
    });
    context.requestBlockFocus(tableBlockId);
  }

  return {
    updateTableCell,
    updateTableCellRichText,
    addTableRow,
    removeTableRow,
    addTableColumn,
    removeTableColumn,
  };
}
