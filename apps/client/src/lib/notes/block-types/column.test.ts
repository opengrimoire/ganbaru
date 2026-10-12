import { describe, expect, it } from "vitest";
import {
  NOTES_COLUMN_MAX_COUNT,
  NOTES_COLUMN_MIN_COUNT,
  notesColumnCanAdd,
  notesColumnCanMove,
  notesColumnCanRemove,
  notesColumnGridTemplate,
  notesColumnResizeWidths,
  notesColumnWidths,
  notesColumnWithWidthRatio,
  planNotesColumnInsertion,
  planNotesColumnRemoval,
} from "./column";
import type { NotesColumnBlock } from "$lib/notes/types";

function columnBlock(id: string, widthRatio?: number): NotesColumnBlock {
  return {
    object: "block",
    id,
    parent: { type: "block_id", block_id: "columns-1" },
    created_time: "2026-01-01T00:00:00.000Z",
    last_edited_time: "2026-01-01T00:00:00.000Z",
    has_children: true,
    in_trash: false,
    type: "column",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    column: widthRatio === undefined ? {} : { width_ratio: widthRatio },
  };
}

describe("notes column helpers", () => {
  it("normalizes missing width ratios into an equal grid", () => {
    const widths = notesColumnWidths([
      columnBlock("left"),
      columnBlock("right"),
    ]);

    expect(widths).toEqual([0.5, 0.5]);
    expect(notesColumnGridTemplate([columnBlock("left"), columnBlock("right")])).toBe(
      "0.5fr 0.5fr",
    );
  });

  it("splits the selected column when inserting a new column", () => {
    const plan = planNotesColumnInsertion([
      columnBlock("left", 0.5),
      columnBlock("right", 0.5),
    ], 0);

    expect(plan).toEqual({
      insertIndex: 1,
      widths: [0.25, 0.25, 0.5],
    });
  });

  it("prevents adding beyond the practical local column limit", () => {
    expect(notesColumnCanAdd(NOTES_COLUMN_MAX_COUNT - 1)).toBe(true);
    expect(notesColumnCanAdd(NOTES_COLUMN_MAX_COUNT)).toBe(false);
  });

  it("moves removed column width to a neighbor and preserves the minimum count", () => {
    expect(notesColumnCanRemove(NOTES_COLUMN_MIN_COUNT)).toBe(false);
    expect(notesColumnCanRemove(NOTES_COLUMN_MIN_COUNT + 1)).toBe(true);

    const plan = planNotesColumnRemoval([
      columnBlock("left", 0.25),
      columnBlock("middle", 0.25),
      columnBlock("right", 0.5),
    ], "middle");

    expect(plan).toEqual({
      removeIndex: 1,
      targetIndex: 0,
      widths: [0.5, 0.5],
    });
  });

  it("plans reorder availability by column position", () => {
    const columns = [
      columnBlock("left", 0.25),
      columnBlock("middle", 0.25),
      columnBlock("right", 0.5),
    ];

    expect(notesColumnCanMove(columns, "left", "left")).toBe(false);
    expect(notesColumnCanMove(columns, "middle", "left")).toBe(true);
    expect(notesColumnCanMove(columns, "middle", "right")).toBe(true);
    expect(notesColumnCanMove(columns, "right", "right")).toBe(false);
  });

  it("resizes one column and distributes remaining space across siblings", () => {
    const widths = notesColumnResizeWidths([
      columnBlock("left", 0.25),
      columnBlock("middle", 0.25),
      columnBlock("right", 0.5),
    ], "right", 0.7);

    expect(widths).toEqual([0.15, 0.15, 0.7]);
  });

  it("preserves column payload shape when writing a width ratio", () => {
    const update = notesColumnWithWidthRatio(0.4);

    expect(update).toEqual({
      type: "column",
      column: { width_ratio: 0.4 },
    });
  });
});
