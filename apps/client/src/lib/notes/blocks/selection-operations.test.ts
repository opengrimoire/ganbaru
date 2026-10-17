import { describe, expect, it } from "vitest";
import {
  createDuplicateBlocksRequest,
  notesSelectionPlainText,
  notesSelectionRootBlockIds,
  notesSelectionSubtreeIds,
  planNotesSelectionMoveWithinSiblings,
} from "./selection-operations";
import { buildNotesChildIdsByParent, type NotesTreeState } from "./tree";
import type { NotesBlock, NotesParent, NotesParagraphBlock } from "$lib/notes/types";

function pageParent(pageId: string): NotesParent {
  return { type: "page_id", page_id: pageId };
}

function blockParent(blockId: string): NotesParent {
  return { type: "block_id", block_id: blockId };
}

function paragraphBlock(id: string, parent: NotesParent, text: string): NotesParagraphBlock {
  return {
    object: "block",
    id,
    parent,
    created_time: "2026-01-01T00:00:00.000Z",
    last_edited_time: "2026-01-01T00:00:00.000Z",
    has_children: false,
    in_trash: false,
    type: "paragraph",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    paragraph: {
      rich_text: [
        {
          type: "text",
          text: { content: text, link: null },
          annotations: {
            bold: false,
            italic: false,
            strikethrough: false,
            underline: false,
            code: false,
            color: "default",
          },
          plain_text: text,
          href: null,
        },
      ],
      color: "default",
    },
  };
}

function tree(blocks: NotesBlock[]): NotesTreeState {
  return {
    blocksById: Object.fromEntries(blocks.map((block) => [block.id, block])),
    childIdsByParentId: buildNotesChildIdsByParent(blocks),
  };
}

describe("notes selection operation planning", () => {
  it("keeps selected roots and preserves loaded subtree order", () => {
    const state = tree([
      paragraphBlock("a", pageParent("page"), "Alpha"),
      paragraphBlock("b", blockParent("a"), "Beta"),
      paragraphBlock("c", pageParent("page"), "Gamma"),
    ]);

    expect(notesSelectionRootBlockIds(state, ["a", "b", "c"])).toEqual(["a", "c"]);
    expect(notesSelectionSubtreeIds(state, ["a", "c"])).toEqual(["a", "b", "c"]);
    expect(notesSelectionPlainText(state, ["a", "c"])).toBe("Alpha\nBeta\nGamma");
  });

  it("plans sibling group movement without moving across parents", () => {
    const state = tree([
      paragraphBlock("a", pageParent("page"), "A"),
      paragraphBlock("b", pageParent("page"), "B"),
      paragraphBlock("c", pageParent("page"), "C"),
      paragraphBlock("d", pageParent("page"), "D"),
      paragraphBlock("e", blockParent("d"), "E"),
    ]);

    expect(planNotesSelectionMoveWithinSiblings(state, ["b", "c"], "up")).toEqual({
      blockIds: ["b", "c"],
      parentId: "page",
      after: null,
      before: "a",
      focusBlockId: "b",
    });
    expect(planNotesSelectionMoveWithinSiblings(state, ["b", "c"], "down")).toEqual({
      blockIds: ["b", "c"],
      parentId: "page",
      after: "d",
      before: null,
      focusBlockId: "b",
    });
    expect(planNotesSelectionMoveWithinSiblings(state, ["c", "e"], "down")).toBeNull();
  });

  it("builds a duplicate request for every loaded source subtree id", () => {
    const state = tree([
      paragraphBlock("a", pageParent("page"), "Alpha"),
      paragraphBlock("b", blockParent("a"), "Beta"),
    ]);
    const ids = ["copy-a", "copy-b"];
    const request = createDuplicateBlocksRequest(
      state,
      ["a"],
      { parent: pageParent("page"), after: "a" },
      () => ids.shift() ?? "missing",
    );

    expect(request).toEqual({
      block_ids: ["a"],
      duplicated_block_ids: [
        { source_id: "a", duplicate_id: "copy-a" },
        { source_id: "b", duplicate_id: "copy-b" },
      ],
      parent: pageParent("page"),
      after: "a",
      before: null,
      include_trashed_sources: false,
    });
  });
});
