import { describe, expect, it } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import { buildNotesChildIdsByParent, type NotesTreeState } from "$lib/notes/blocks/tree";
import { applyNotesPostMutationToTree, notesPostTrashResult } from "./post-mutation";
import type { NotesBlock, NotesParent } from "./types";

const pageId = "00000000-0000-4000-8000-000000000001";
const firstId = "00000000-0000-4000-8000-000000000002";
const secondId = "00000000-0000-4000-8000-000000000003";
const childId = "00000000-0000-4000-8000-000000000004";
const now = "2026-07-11T12:00:00.000Z";

function paragraph(id: string, parent: NotesParent, text = ""): NotesBlock {
  const write = createBlockWrite(id, "paragraph", text);
  if (write.type !== "paragraph") throw new Error("expected paragraph");
  return {
    object: "block",
    id,
    parent,
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    type: "paragraph",
    paragraph: write.paragraph,
  };
}

function state(blocks: readonly NotesBlock[]): NotesTreeState {
  return {
    blocksById: Object.fromEntries(blocks.map((block) => [block.id, block])),
    childIdsByParentId: buildNotesChildIdsByParent(blocks),
  };
}

describe("Notes post-mutation application", () => {
  it("applies authoritative updates, inserts, and moves without re-reading blocks", () => {
    const pageParent = { type: "page_id", page_id: pageId } as const;
    const first = paragraph(firstId, pageParent, "First");
    const second = paragraph(secondId, pageParent, "Second");
    const inserted = paragraph(childId, pageParent, "Inserted");
    const next = applyNotesPostMutationToTree(state([first, second]), {
      blocks: [inserted, { ...second, parent: { type: "block_id", block_id: firstId } }],
      placements: [
        { blockId: childId, parent: pageParent, after: firstId },
        {
          blockId: secondId,
          parent: { type: "block_id", block_id: firstId },
          after: null,
        },
      ],
    });

    expect(next.childIdsByParentId[pageId]).toEqual([firstId, childId]);
    expect(next.childIdsByParentId[firstId]).toEqual([secondId]);
    expect(next.blocksById[childId]).toEqual(inserted);
  });

  it("removes a loaded subtree for an authoritative trash result", () => {
    const root = paragraph(firstId, { type: "page_id", page_id: pageId });
    const child = paragraph(childId, { type: "block_id", block_id: firstId });
    const current = state([root, child]);
    const next = applyNotesPostMutationToTree(
      current,
      notesPostTrashResult(current, [firstId]),
    );
    expect(next.blocksById).toEqual({});
    expect(next.childIdsByParentId[pageId]).toEqual([]);
  });
});
