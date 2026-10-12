import { createColumnPayload } from "$lib/notes/blocks/factory";
import type {
  NotesBlockUpdate,
  NotesBlockWrite,
  NotesColumnBlock,
} from "$lib/notes/types";

export const NOTES_COLUMN_MIN_COUNT = 2;
export const NOTES_COLUMN_MAX_COUNT = 6;
export const NOTES_COLUMN_MIN_WIDTH_RATIO = 0.1;

export type NotesColumnMoveDirection = "left" | "right";

export interface NotesColumnInsertionPlan {
  insertIndex: number;
  widths: number[];
}

export interface NotesColumnRemovalPlan {
  removeIndex: number;
  targetIndex: number;
  widths: number[];
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function normalizeIndex(index: number, count: number): number {
  if (count <= 0 || !Number.isFinite(index)) return 0;
  return clamp(Math.trunc(index), 0, count - 1);
}

function roundWidth(value: number): number {
  return Math.round(value * 10000) / 10000;
}

function normalizedWidths(widths: readonly number[]): number[] {
  if (widths.length === 0) return [];
  const safeWidths = widths.map((width) =>
    Number.isFinite(width) && width > 0 ? width : 1,
  );
  const total = safeWidths.reduce((sum, width) => sum + width, 0);
  const normalized = safeWidths.map((width) => roundWidth(width / total));
  const roundedTotal = normalized.reduce((sum, width) => sum + width, 0);
  normalized[normalized.length - 1] = roundWidth(
    normalized[normalized.length - 1] + (1 - roundedTotal),
  );
  return normalized;
}

export function notesColumnWidths(
  columns: readonly NotesColumnBlock[],
): number[] {
  if (columns.length === 0) return [];
  return normalizedWidths(
    columns.map((column) => column.column.width_ratio ?? 1),
  );
}

export function notesColumnGridTemplate(
  columns: readonly NotesColumnBlock[],
): string {
  const widths = notesColumnWidths(columns);
  if (widths.length === 0) return "1fr";
  return widths.map((width) => `${width}fr`).join(" ");
}

export function notesColumnCanAdd(count: number): boolean {
  return count < NOTES_COLUMN_MAX_COUNT;
}

export function notesColumnCanRemove(count: number): boolean {
  return count > NOTES_COLUMN_MIN_COUNT;
}

export function notesColumnCanMove(
  columns: readonly NotesColumnBlock[],
  columnId: string,
  direction: NotesColumnMoveDirection,
): boolean {
  const index = columns.findIndex((column) => column.id === columnId);
  if (index < 0) return false;
  return direction === "left" ? index > 0 : index + 1 < columns.length;
}

export function notesColumnWithWidthRatio(widthRatio: number): NotesBlockUpdate {
  return {
    type: "column",
    column: createColumnPayload(widthRatio),
  };
}

export function createNotesColumnWrite(
  id: string,
  widthRatio: number,
): NotesBlockWrite {
  return {
    id,
    type: "column",
    column: createColumnPayload(widthRatio),
  };
}

export function planNotesColumnInsertion(
  columns: readonly NotesColumnBlock[],
  afterIndex: number,
): NotesColumnInsertionPlan | null {
  if (!notesColumnCanAdd(columns.length)) return null;
  if (columns.length === 0) {
    return { insertIndex: 0, widths: normalizedWidths([1, 1]) };
  }
  const widths = notesColumnWidths(columns);
  const safeAfterIndex = normalizeIndex(afterIndex, columns.length);
  const insertIndex = safeAfterIndex + 1;
  const splitWidth = widths[safeAfterIndex] / 2;
  const nextWidths = [
    ...widths.slice(0, safeAfterIndex),
    splitWidth,
    splitWidth,
    ...widths.slice(safeAfterIndex + 1),
  ];
  return { insertIndex, widths: normalizedWidths(nextWidths) };
}

export function planNotesColumnRemoval(
  columns: readonly NotesColumnBlock[],
  columnId: string,
): NotesColumnRemovalPlan | null {
  if (!notesColumnCanRemove(columns.length)) return null;
  const removeIndex = columns.findIndex((column) => column.id === columnId);
  if (removeIndex < 0) return null;
  const targetIndex = removeIndex > 0 ? removeIndex - 1 : 1;
  const widths = notesColumnWidths(columns);
  const removedWidth = widths[removeIndex] ?? 0;
  const nextWidths = widths.filter((_, index) => index !== removeIndex);
  const remainingTargetIndex = targetIndex > removeIndex ? targetIndex - 1 : targetIndex;
  nextWidths[remainingTargetIndex] = (nextWidths[remainingTargetIndex] ?? 0) + removedWidth;
  return {
    removeIndex,
    targetIndex,
    widths: normalizedWidths(nextWidths),
  };
}

export function notesColumnResizeWidths(
  columns: readonly NotesColumnBlock[],
  columnId: string,
  widthRatio: number,
): number[] | null {
  const index = columns.findIndex((column) => column.id === columnId);
  if (index < 0 || columns.length === 0) return null;
  if (columns.length === 1) return [1];
  const maxWidth = 1 - NOTES_COLUMN_MIN_WIDTH_RATIO * (columns.length - 1);
  const targetWidth = clamp(
    Number.isFinite(widthRatio) ? widthRatio : notesColumnWidths(columns)[index],
    NOTES_COLUMN_MIN_WIDTH_RATIO,
    maxWidth,
  );
  const remainingWidth = 1 - targetWidth;
  const otherWidth = remainingWidth / (columns.length - 1);
  return normalizedWidths(
    columns.map((_, columnIndex) => (columnIndex === index ? targetWidth : otherWidth)),
  );
}
