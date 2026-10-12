import {
  createEmptyTableRowPayload,
  createTableCell,
} from "$lib/notes/blocks/factory";
import { isNotesTabKey } from "$lib/notes/blocks/keyboard";
import {
  replacePlainTextPreservingRichText,
} from "$lib/notes/rich-text/core";
import type {
  NotesBlockUpdate,
  NotesBlockWrite,
  NotesRichText,
  NotesTableBlock,
  NotesTableRowBlock,
} from "$lib/notes/types";

export const NOTES_TABLE_MIN_WIDTH = 1;
export const NOTES_TABLE_MAX_WIDTH = 100;
export const NOTES_TABLE_MAX_ROWS = 100;

export interface NotesTableCellCoordinate {
  rowIndex: number;
  columnIndex: number;
}

export interface NotesTableCellNavigationInput extends NotesTableCellCoordinate {
  key: string;
  code?: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  rowCount: number;
  columnCount: number;
  selectionStart: number;
  selectionEnd: number;
  textLength: number;
}

export type NotesTableCellNavigationPlan =
  | { type: "none" }
  | {
    type: "focus_cell";
    rowIndex: number;
    columnIndex: number;
    preventDefault: true;
  };

function cloneRichTextArray(richText: readonly NotesRichText[]): NotesRichText[] {
  return richText.map((item) => structuredClone(item));
}

function normalizeNotesTableWidth(width: number): number {
  if (!Number.isFinite(width)) return NOTES_TABLE_MIN_WIDTH;
  return Math.max(
    NOTES_TABLE_MIN_WIDTH,
    Math.min(NOTES_TABLE_MAX_WIDTH, Math.trunc(width)),
  );
}

function normalizeColumnIndex(index: number, width: number): number {
  const safeWidth = normalizeNotesTableWidth(width);
  if (!Number.isFinite(index)) return 0;
  return Math.max(0, Math.min(safeWidth - 1, Math.trunc(index)));
}

function normalizeColumnInsertIndex(index: number, width: number): number {
  const safeWidth = normalizeNotesTableWidth(width);
  if (!Number.isFinite(index)) return safeWidth;
  return Math.max(0, Math.min(safeWidth, Math.trunc(index)));
}

function rowCellsAtWidth(
  row: NotesTableRowBlock,
  width: number,
): NotesRichText[][] {
  const safeWidth = normalizeNotesTableWidth(width);
  return Array.from({ length: safeWidth }, (_, index) =>
    cloneRichTextArray(row.table_row.cells[index] ?? []),
  );
}

export function notesTableVisibleWidth(
  table: NotesTableBlock,
  rows: readonly NotesTableRowBlock[],
): number {
  return normalizeNotesTableWidth(
    Math.max(
      table.table.table_width,
      ...rows.map((row) => row.table_row.cells.length),
    ),
  );
}

export function notesTableCanAddRow(rowCount: number): boolean {
  return rowCount < NOTES_TABLE_MAX_ROWS;
}

export function notesTableCanRemoveRow(rowCount: number): boolean {
  return rowCount > 1;
}

export function notesTableCanAddColumn(width: number): boolean {
  return normalizeNotesTableWidth(width) < NOTES_TABLE_MAX_WIDTH;
}

export function notesTableCanRemoveColumn(width: number): boolean {
  return normalizeNotesTableWidth(width) > NOTES_TABLE_MIN_WIDTH;
}

export function notesTableWithWidth(
  table: NotesTableBlock,
  width: number,
): NotesBlockUpdate {
  return {
    type: "table",
    table: {
      ...table.table,
      table_width: normalizeNotesTableWidth(width),
    },
  };
}

export function createNotesTableRowWrite(
  id: string,
  width: number,
): NotesBlockWrite {
  return {
    id,
    type: "table_row",
    table_row: createEmptyTableRowPayload(normalizeNotesTableWidth(width)),
  };
}

export function notesTableCellRichText(
  row: NotesTableRowBlock,
  columnIndex: number,
): NotesRichText[] {
  return cloneRichTextArray(row.table_row.cells[columnIndex] ?? []);
}

export function notesTableCellRichTextFromPlainTextEdit(
  existingCell: readonly NotesRichText[],
  plainText: string,
): NotesRichText[] {
  return replacePlainTextPreservingRichText(existingCell, plainText);
}

export function notesTableRowWithCellRichText(
  row: NotesTableRowBlock,
  columnIndex: number,
  richText: readonly NotesRichText[],
): NotesBlockUpdate {
  const width = Math.max(row.table_row.cells.length, columnIndex + 1);
  const cells = rowCellsAtWidth(row, width);
  cells[columnIndex] = cloneRichTextArray(richText);
  return {
    type: "table_row",
    table_row: { cells },
  };
}

export function notesTableRowWithCellText(
  row: NotesTableRowBlock,
  columnIndex: number,
  plainText: string,
): NotesBlockUpdate {
  return notesTableRowWithCellRichText(
    row,
    columnIndex,
    createTableCell(plainText),
  );
}

export function notesTableRowWithInsertedColumn(
  row: NotesTableRowBlock,
  insertIndex: number,
  width: number,
): NotesBlockUpdate {
  const safeWidth = notesTableVisibleWidth(
    {
      ...row,
      type: "table",
      table: {
        table_width: width,
        has_column_header: false,
        has_row_header: false,
      },
    } as NotesTableBlock,
    [row],
  );
  const cells = rowCellsAtWidth(row, safeWidth);
  cells.splice(normalizeColumnInsertIndex(insertIndex, safeWidth), 0, []);
  return {
    type: "table_row",
    table_row: { cells },
  };
}

export function notesTableRowWithRemovedColumn(
  row: NotesTableRowBlock,
  removeIndex: number,
  width: number,
): NotesBlockUpdate {
  const safeWidth = normalizeNotesTableWidth(Math.max(width, row.table_row.cells.length));
  if (!notesTableCanRemoveColumn(safeWidth)) {
    return {
      type: "table_row",
      table_row: { cells: rowCellsAtWidth(row, safeWidth) },
    };
  }
  const cells = rowCellsAtWidth(row, safeWidth);
  cells.splice(normalizeColumnIndex(removeIndex, safeWidth), 1);
  return {
    type: "table_row",
    table_row: { cells },
  };
}

function targetCell(
  input: NotesTableCellCoordinate & {
    rowCount: number;
    columnCount: number;
    direction: "next" | "previous" | "up" | "down";
  },
): NotesTableCellCoordinate | null {
  const rowCount = Math.max(0, Math.trunc(input.rowCount));
  const columnCount = Math.max(0, Math.trunc(input.columnCount));
  if (rowCount <= 0 || columnCount <= 0) return null;
  const rowIndex = Math.max(0, Math.min(rowCount - 1, Math.trunc(input.rowIndex)));
  const columnIndex = Math.max(0, Math.min(columnCount - 1, Math.trunc(input.columnIndex)));
  switch (input.direction) {
    case "next":
      if (columnIndex + 1 < columnCount) return { rowIndex, columnIndex: columnIndex + 1 };
      if (rowIndex + 1 < rowCount) return { rowIndex: rowIndex + 1, columnIndex: 0 };
      return null;
    case "previous":
      if (columnIndex > 0) return { rowIndex, columnIndex: columnIndex - 1 };
      if (rowIndex > 0) return { rowIndex: rowIndex - 1, columnIndex: columnCount - 1 };
      return null;
    case "up":
      return rowIndex > 0 ? { rowIndex: rowIndex - 1, columnIndex } : null;
    case "down":
      return rowIndex + 1 < rowCount ? { rowIndex: rowIndex + 1, columnIndex } : null;
  }
}

function navigationPlanForTarget(
  target: NotesTableCellCoordinate | null,
): NotesTableCellNavigationPlan {
  if (!target) return { type: "none" };
  return {
    type: "focus_cell",
    rowIndex: target.rowIndex,
    columnIndex: target.columnIndex,
    preventDefault: true,
  };
}

export function planNotesTableCellNavigation(
  input: NotesTableCellNavigationInput,
): NotesTableCellNavigationPlan {
  if (input.altKey || input.ctrlKey || input.metaKey) return { type: "none" };

  if (isNotesTabKey(input)) {
    return navigationPlanForTarget(
      targetCell({
        ...input,
        direction: input.shiftKey ? "previous" : "next",
      }),
    );
  }

  if (input.key === "Enter") {
    return navigationPlanForTarget(
      targetCell({
        ...input,
        direction: input.shiftKey ? "up" : "down",
      }),
    );
  }

  const collapsed = input.selectionStart === input.selectionEnd;
  if (
    input.key === "ArrowLeft"
    && collapsed
    && input.selectionStart <= 0
  ) {
    return navigationPlanForTarget(targetCell({ ...input, direction: "previous" }));
  }
  if (
    input.key === "ArrowRight"
    && collapsed
    && input.selectionEnd >= input.textLength
  ) {
    return navigationPlanForTarget(targetCell({ ...input, direction: "next" }));
  }
  if (input.key === "ArrowUp") {
    return navigationPlanForTarget(targetCell({ ...input, direction: "up" }));
  }
  if (input.key === "ArrowDown") {
    return navigationPlanForTarget(targetCell({ ...input, direction: "down" }));
  }

  return { type: "none" };
}
