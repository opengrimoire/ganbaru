import { describe, expect, it, vi } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import type { NotesBlock, NotesParent } from "$lib/notes/types";
import { createNotesTemplateBlockActions } from "./template";

const pageId = "10000000-0000-4000-8000-000000000001";
const blockId = "10000000-0000-4000-8000-000000000002";
const parent: NotesParent = { type: "page_id", page_id: pageId };

describe("canonical template application", () => {
  it.each(["template", "button"] as const)("applies a %s whose children are not hydrated", async (type) => {
    const owner = { ...createBlockWrite(blockId, type, "Insert"), object: "block", parent,
      edit_revision: "1".repeat(64), created_time: "", last_edited_time: "", has_children: true,
      in_trash: false, source_provider: null, source_object_id: null, source_last_edited_time: null,
    } as NotesBlock;
    const duplicateSubtreesAndApply = vi.fn(async () => []);
    const actions = createNotesTemplateBlockActions({
      readSelectedPageId: () => pageId, readBlocksById: () => ({ [blockId]: owner }),
      readChildIdsByParentId: () => ({ [pageId]: [blockId], [blockId]: ["unloaded"] }),
      blockById: (id) => id === blockId ? owner : undefined,
      treeState: () => ({ blocksById: { [blockId]: owner }, childIdsByParentId: { [pageId]: [blockId], [blockId]: ["unloaded"] } }),
      flushPendingBlockSaves: async () => undefined, requestBlockFocus: () => undefined,
      localApplyBlockUpdate: () => undefined, scheduleBlockSave: () => undefined,
      appendAndApply: async () => [], duplicateSubtreesAndApply,
      undoSnapshot: () => null, recordUndoAfter: () => undefined,
    });
    if (type === "template") await actions.useTemplateBlock(blockId);
    else await actions.useButtonBlock(blockId);
    expect(duplicateSubtreesAndApply).toHaveBeenCalledWith(expect.objectContaining({
      sourceParentBlockId: blockId, rootBlockIds: [], sourceSubtreeBlockIds: [], parent,
    }));
  });
});
