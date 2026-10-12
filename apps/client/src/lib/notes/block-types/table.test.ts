import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import {
  createNotesTableRowWrite,
  notesTableCanRemoveColumn,
  notesTableCellRichTextFromPlainTextEdit,
  notesTableVisibleWidth,
  notesTableWithWidth,
  notesTableRowWithInsertedColumn,
  notesTableRowWithRemovedColumn,
  planNotesTableCellNavigation,
} from "./table";
import type {
  NotesRichText,
  NotesTableBlock,
  NotesTableRowBlock,
} from "$lib/notes/types";

function tableBlock(width = 2): NotesTableBlock {
  return {
    object: "block",
    id: "table-1",
    parent: { type: "page_id", page_id: "page-1" },
    created_time: "2026-01-01T00:00:00.000Z",
    last_edited_time: "2026-01-01T00:00:00.000Z",
    has_children: true,
    in_trash: false,
    type: "table",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    table: {
      table_width: width,
      has_column_header: true,
      has_row_header: true,
    },
  };
}

function rowBlock(id: string, cells: readonly string[]): NotesTableRowBlock {
  return {
    object: "block",
    id,
    parent: { type: "block_id", block_id: "table-1" },
    created_time: "2026-01-01T00:00:00.000Z",
    last_edited_time: "2026-01-01T00:00:00.000Z",
    has_children: false,
    in_trash: false,
    type: "table_row",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    table_row: {
      cells: cells.map((cell) => (cell ? [createRichText(cell)] : [])),
    },
  };
}

function boldCell(text: string): NotesRichText[] {
  const richText = createRichText(text);
  if (richText.type !== "text") throw new Error("Expected text rich text");
  return [
    {
      ...richText,
      annotations: {
        ...richText.annotations,
        bold: true,
      },
    },
  ];
}

describe("notes table helpers", () => {
  it("preserves table header flags when changing width", () => {
    const update = notesTableWithWidth(tableBlock(2), 3);

    expect(update.type).toBe("table");
    if (update.type !== "table") throw new Error("Expected table update");
    expect(update.table).toEqual({
      table_width: 3,
      has_column_header: true,
      has_row_header: true,
    });
  });

  it("uses row cell length when it exceeds the stored table width", () => {
    expect(notesTableVisibleWidth(tableBlock(2), [rowBlock("row-1", ["A", "B", "C"])])).toBe(3);
  });

  it("creates empty row writes at the visible table width", () => {
    const write = createNotesTableRowWrite("row-2", 3);

    expect(write.type).toBe("table_row");
    if (write.type !== "table_row") throw new Error("Expected table row write");
    expect(write.table_row.cells).toEqual([[], [], []]);
  });

  it("preserves rich text annotations when plain text edits keep the same label", () => {
    const richText = boldCell("Done");
    const edited = notesTableCellRichTextFromPlainTextEdit(richText, "Done");

    expect(edited).toEqual(richText);
  });

  it("inserts and removes columns while preserving neighboring rich text", () => {
    const row = rowBlock("row-1", ["A", "B"]);
    row.table_row.cells[0] = boldCell("A");
    const inserted = notesTableRowWithInsertedColumn(row, 1, 2);

    expect(inserted.type).toBe("table_row");
    if (inserted.type !== "table_row") throw new Error("Expected table row update");
    expect(inserted.table_row.cells.map((cell) => cell[0]?.plain_text ?? "")).toEqual([
      "A",
      "",
      "B",
    ]);
    expect(inserted.table_row.cells[0]?.[0]?.annotations.bold).toBe(true);

    const nextRow = { ...row, table_row: inserted.table_row };
    const removed = notesTableRowWithRemovedColumn(nextRow, 1, 3);
    expect(removed.type).toBe("table_row");
    if (removed.type !== "table_row") throw new Error("Expected table row update");
    expect(removed.table_row.cells.map((cell) => cell[0]?.plain_text ?? "")).toEqual([
      "A",
      "B",
    ]);
  });

  it("prevents removing the final column", () => {
    expect(notesTableCanRemoveColumn(1)).toBe(false);
    expect(notesTableCanRemoveColumn(2)).toBe(true);
  });

  it("plans keyboard navigation between table cells", () => {
    const base = {
      rowIndex: 0,
      columnIndex: 0,
      rowCount: 2,
      columnCount: 2,
      selectionStart: 0,
      selectionEnd: 0,
      textLength: 4,
      shiftKey: false,
      ctrlKey: false,
      metaKey: false,
      altKey: false,
    };

    expect(planNotesTableCellNavigation({ ...base, key: "Tab" })).toEqual({
      type: "focus_cell",
      rowIndex: 0,
      columnIndex: 1,
      preventDefault: true,
    });
    expect(planNotesTableCellNavigation({
      ...base, key: "Unidentified", code: "Tab", rowIndex: 1, shiftKey: true,
    })).toEqual({ type: "focus_cell", rowIndex: 0, columnIndex: 1, preventDefault: true });
    expect(
      planNotesTableCellNavigation({
        ...base,
        key: "Tab",
        columnIndex: 0,
        shiftKey: true,
      }),
    ).toEqual({ type: "none" });
    expect(planNotesTableCellNavigation({ ...base, key: "Enter" })).toEqual({
      type: "focus_cell",
      rowIndex: 1,
      columnIndex: 0,
      preventDefault: true,
    });
    expect(
      planNotesTableCellNavigation({
        ...base,
        key: "ArrowRight",
        selectionStart: 4,
        selectionEnd: 4,
      }),
    ).toEqual({
      type: "focus_cell",
      rowIndex: 0,
      columnIndex: 1,
      preventDefault: true,
    });
  });
});
