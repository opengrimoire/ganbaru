import { describe, expect, it } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import { buildNotesChildIdsByParent } from "$lib/notes/blocks/tree";
import type {
  NotesBlock,
  NotesBlockWrite,
  NotesParent,
} from "$lib/notes/types";
import {
  flatNotesBlockItems,
  flatNotesBlockItemsForContext,
  isOnlyNotesBlockInContext,
  notesTableRowsForBlock,
  notesTreeStateWithoutLeafBlock,
  previousNotesBlockType,
  type NotesBlockTreeSnapshot,
} from "./block-tree";

const now = "2026-06-30T09:00:00.000Z";

function blockFromWrite(write: NotesBlockWrite, parent: NotesParent): NotesBlock {
  const base = {
    object: "block" as const,
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
  };
  switch (write.type) {
    case "paragraph":
      return { ...base, type: write.type, paragraph: write.paragraph };
    case "table":
      return { ...base, type: write.type, table: write.table };
    case "table_row":
      return { ...base, type: write.type, table_row: write.table_row };
    default:
      throw new Error(`unsupported test block type: ${write.type}`);
  }
}

function block(id: string, parent: NotesParent, text = ""): NotesBlock {
  return blockFromWrite(createBlockWrite(id, "paragraph", text), parent);
}

function typedBlock(id: string, type: "table" | "table_row", parent: NotesParent): NotesBlock {
  return blockFromWrite(createBlockWrite(id, type), parent);
}

function snapshot(blocks: NotesBlock[], selectedPageId = "page"): NotesBlockTreeSnapshot {
  return {
    selectedPageId,
    blocksById: Object.fromEntries(blocks.map((item) => [item.id, item])),
    childIdsByParentId: buildNotesChildIdsByParent(blocks),
  };
}

describe("notes store block tree selectors", () => {
  it("flattens selected page blocks without store state", () => {
    const state = snapshot([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
      block("c", { type: "block_id", block_id: "b" }, "C"),
    ]);

    expect(flatNotesBlockItems(state).map((item) => [item.block.id, item.depth])).toEqual([
      ["a", 0],
      ["b", 0],
      ["c", 1],
    ]);
    expect(previousNotesBlockType(state, "b")).toBe("paragraph");
  });

  it("returns context and only-block status from the selected page snapshot", () => {
    const single = snapshot([block("a", { type: "page_id", page_id: "page" }, "A")]);
    const pair = snapshot([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);

    expect(flatNotesBlockItemsForContext(pair, "b").map((item) => item.block.id)).toEqual([
      "a",
      "b",
    ]);
    expect(isOnlyNotesBlockInContext(single, "a")).toBe(true);
    expect(isOnlyNotesBlockInContext(pair, "a")).toBe(false);
  });

  it("selects table row children for table surfaces", () => {
    const state = snapshot([
      typedBlock("table", "table", { type: "page_id", page_id: "page" }),
      typedBlock("row-a", "table_row", { type: "block_id", block_id: "table" }),
      typedBlock("row-b", "table_row", { type: "block_id", block_id: "table" }),
    ]);

    expect(notesTableRowsForBlock(state, "table").map((row) => row.id)).toEqual([
      "row-a",
      "row-b",
    ]);
  });

  it("removes a leaf block locally while preserving sibling order", () => {
    const current = snapshot([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }),
      block("c", { type: "page_id", page_id: "page" }, "C"),
    ]);

    const next = notesTreeStateWithoutLeafBlock(current, "b");

    expect(next?.blocksById.b).toBeUndefined();
    expect(next?.childIdsByParentId.page).toEqual(["a", "c"]);
  });

  it("keeps blocks with children on the transactional delete path", () => {
    const current = snapshot([
      block("parent", { type: "page_id", page_id: "page" }),
      block("child", { type: "block_id", block_id: "parent" }),
    ]);

    expect(notesTreeStateWithoutLeafBlock(current, "parent")).toBeNull();
  });
});
