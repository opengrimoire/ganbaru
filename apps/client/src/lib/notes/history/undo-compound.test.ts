import { describe, expect, it } from "vitest";
import { createBlockWrite, blockPlainText } from "$lib/notes/blocks/factory";
import { createNotesUndoSnapshot } from "./undo-history";
import { buildNotesChildIdsByParent } from "$lib/notes/blocks/tree";
import { reconcileNotesCompoundUndo } from "./undo-compound";
import type { NotesBlock, NotesParent } from "$lib/notes/types";
import type { NotesCompoundEditResult } from "$lib/api/notes/compound-edits";

const pageId = "10000000-0000-4000-8000-000000000001";
const parent: NotesParent = { type: "page_id", page_id: pageId };
function block(id: string, text: string, owner = parent, revision = "1"): NotesBlock {
  return { ...createBlockWrite(id, "paragraph", text), object: "block", parent: owner, edit_revision: revision.repeat(64), created_time: "", last_edited_time: "", has_children: false, in_trash: false, source_provider: null, source_object_id: null, source_last_edited_time: null } as NotesBlock;
}
function snapshot(blocks: NotesBlock[]) {
  return createNotesUndoSnapshot(pageId, { blocksById: Object.fromEntries(blocks.map((row) => [row.id, row])), childIdsByParentId: buildNotesChildIdsByParent(blocks) }, blocks[0]?.id ?? null);
}
function result(before: NotesBlock[], after: NotesBlock[]): NotesCompoundEditResult {
  return { operation_id: "operation", page_id: pageId, databases: [], before_blocks: before, blocks: after,
    before_placements: before.map((row) => ({ blockId: row.id, parent: row.parent, after: null })),
    placements: after.filter((row) => !row.in_trash).map((row) => ({ blockId: row.id, parent: row.parent, after: null })),
  };
}

describe("canonical Notes undo preimages", () => {
  it("restores hidden moved content to its original parent without treating it as newly created", () => {
    const source = block("source", "Source");
    const target = block("target", "Target");
    const untouched = block("untouched", "Independent draft");
    const hidden = block("hidden", "Hidden", { type: "block_id", block_id: source.id });
    const moved = { ...hidden, parent: { type: "block_id" as const, block_id: target.id }, edit_revision: "2".repeat(64) };
    const before = snapshot([source, target, untouched]);
    const after = snapshot([target, untouched, moved]);
    reconcileNotesCompoundUndo(before, after, result([source, target, hidden], [{ ...source, in_trash: true, edit_revision: "2".repeat(64) }, target, moved]));
    expect(before?.childIdsByParentId.source).toEqual(["hidden"]);
    expect(after?.childIdsByParentId.target).toEqual(["hidden"]);
    expect(before?.blocks.find((row) => row.id === "hidden")?.edit_revision).toBe("2".repeat(64));
    expect(after?.blocks.some((row) => row.id === source.id)).toBe(false);
    expect(blockPlainText(before!.blocks.find((row) => row.id === untouched.id)!)).toBe("Independent draft");
  });

  it("includes both payload boundaries for an unloaded table row", () => {
    const table = { ...block("table", ""), type: "table" as const, table: { table_width: 2, has_column_header: false, has_row_header: false } };
    const hidden = { ...block("hidden", "", { type: "block_id", block_id: table.id }), type: "table_row" as const, table_row: { cells: [[], []] } };
    const edited = { ...table, edit_revision: "2".repeat(64), table: { ...table.table, table_width: 3 } };
    const editedHidden = { ...hidden, edit_revision: "2".repeat(64), table_row: { cells: [[], [], []] } };
    const before = snapshot([table]);
    const after = snapshot([edited]);
    reconcileNotesCompoundUndo(before, after, result([table, hidden], [edited, editedHidden]));
    const originalRow = before?.blocks.find((row) => row.id === hidden.id);
    const changedRow = after?.blocks.find((row) => row.id === hidden.id);
    expect(originalRow?.type === "table_row" && originalRow.table_row.cells).toHaveLength(2);
    expect(changedRow?.type === "table_row" && changedRow.table_row.cells).toHaveLength(3);
    expect(originalRow?.edit_revision).toBe("2".repeat(64));
  });
});
