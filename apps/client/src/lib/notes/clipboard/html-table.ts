import { NOTES_CLIPBOARD_MAX_BLOCKS } from "./blocks";
import type { NotesRichText } from "$lib/notes/types";

// Matches the canonical Notes table width constraint.
const MAX_CLIPBOARD_TABLE_COLUMNS = 100;

export interface NotesClipboardTable {
  width: number;
  hasColumnHeader: boolean;
  hasRowHeader: boolean;
  rows: NotesRichText[][][];
}

/** Expand a bounded HTML table into rectangular cells, keeping merged text once. */
export function readNotesClipboardTable(
  table: Element,
  readCell: (cell: Element) => NotesRichText[],
): NotesClipboardTable | null {
  const rows = Array.from(table.querySelectorAll("tr")).filter((row) => row.closest("table") === table);
  const cells = rows.map((row) => Array.from(row.children).filter((cell) => ["TD", "TH"].includes(cell.tagName)));
  if (!rows.length || rows.length + 1 > NOTES_CLIPBOARD_MAX_BLOCKS || table.querySelector("table")) return null;
  const grid: NotesRichText[][][] = rows.map(() => []);
  for (const [rowIndex, row] of cells.entries()) {
    let column = 0;
    for (const cell of row) {
      while (grid[rowIndex][column] !== undefined) column += 1;
      const span = Math.max(1, Number.parseInt(cell.getAttribute("colspan") ?? "1", 10) || 1);
      const rowSpanValue = Number.parseInt(cell.getAttribute("rowspan") ?? "1", 10);
      const rowSpan = rowSpanValue === 0 ? rows.length - rowIndex
        : Math.min(rows.length - rowIndex, Math.max(1, rowSpanValue || 1));
      if (column + span > MAX_CLIPBOARD_TABLE_COLUMNS) return null;
      for (let y = rowIndex; y < rowIndex + rowSpan; y += 1) {
        for (let x = column; x < column + span; x += 1) {
          if (grid[y][x] !== undefined) return null;
          grid[y][x] = y === rowIndex && x === column ? readCell(cell) : [];
        }
      }
      column += span;
    }
  }
  const width = Math.max(...grid.map((row) => row.length));
  if (!width) return null;
  const hasColumnHeader = cells[0].length > 0 && cells[0].every((cell) => cell.tagName === "TH");
  const bodyCells = hasColumnHeader ? cells.slice(1) : cells;
  return {
    width, hasColumnHeader,
    hasRowHeader: bodyCells.length > 0 && bodyCells.every((row) => row[0]?.tagName === "TH"),
    rows: grid.map((row) => Array.from({ length: width }, (_, index) => row[index] ?? [])),
  };
}
