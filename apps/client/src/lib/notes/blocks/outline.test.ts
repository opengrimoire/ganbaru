import { describe, expect, it } from "vitest";
import { createBlockWrite } from "./factory";
import { buildNotesChildIdsByParent, flattenNotesBlockTree } from "./tree";
import { flattenNotesBlockOutlines, notesBlockOutlineFromBlock, notesOutlineSubtreeIds } from "./outline";
import type { NotesBlock, NotesToggleBlock } from "$lib/notes/types";

const pageId = "10000000-0000-4000-8000-000000000001";

function paragraph(id: string, parent: NotesBlock["parent"], hasChildren = false): NotesBlock {
  const write = createBlockWrite(id, "paragraph", id);
  if (write.type !== "paragraph") throw new Error("expected paragraph");
  return {
    object: "block",
    id,
    parent,
    created_time: "2026-01-01T00:00:00Z",
    last_edited_time: "2026-01-01T00:00:00Z",
    has_children: hasChildren,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    type: "paragraph",
    paragraph: write.paragraph,
  };
}

function toggle(id: string, parent: NotesBlock["parent"]): NotesToggleBlock {
  const write = createBlockWrite(id, "toggle", "Details");
  if (write.type !== "toggle") throw new Error("expected toggle");
  return {
    object: "block",
    id,
    parent,
    created_time: "2026-01-01T00:00:00Z",
    last_edited_time: "2026-01-01T00:00:00Z",
    has_children: true,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    type: "toggle",
    toggle: write.toggle,
  };
}

describe("Notes block outline", () => {
  it("matches hydrated tree order and depth for nested blocks", () => {
    const rootA = paragraph("10000000-0000-4000-8000-000000000002", { type: "page_id", page_id: pageId }, true);
    const child = paragraph("10000000-0000-4000-8000-000000000003", { type: "block_id", block_id: rootA.id });
    const rootB = paragraph("10000000-0000-4000-8000-000000000004", { type: "page_id", page_id: pageId });
    const blocks = [rootA, child, rootB];
    const outlines = [
      notesBlockOutlineFromBlock(rootB, pageId, 2_000),
      notesBlockOutlineFromBlock(child, pageId, 1_000),
      notesBlockOutlineFromBlock(rootA, pageId, 1_000),
    ];
    const outlineItems = flattenNotesBlockOutlines(outlines, pageId);
    const hydrated = flattenNotesBlockTree({
      blocksById: Object.fromEntries(blocks.map((block) => [block.id, block])),
      childIdsByParentId: buildNotesChildIdsByParent(blocks),
    }, pageId);
    expect(outlineItems.map((item) => [item.outline.id, item.depth])).toEqual(
      hydrated.map((item) => [item.block.id, item.depth]),
    );
  });

  it("omits every descendant of a closed toggle while retaining full outlines for subtree work", () => {
    const root = toggle("toggle", { type: "page_id", page_id: pageId });
    const child = paragraph("child", { type: "block_id", block_id: root.id }, true);
    const grandchild = paragraph("grandchild", { type: "block_id", block_id: child.id });
    const sibling = paragraph("sibling", { type: "page_id", page_id: pageId });
    const blocks = [root, child, grandchild, sibling];
    const outlines = blocks.map((block, index) => notesBlockOutlineFromBlock(block, pageId, index)).reverse();
    const ids = (hydrated: Readonly<Record<string, NotesBlock>>) => flattenNotesBlockOutlines(outlines, pageId, hydrated)
      .map(({ outline }) => outline.id);

    expect(ids({})).toEqual([root.id, child.id, grandchild.id, sibling.id]);
    expect(ids({ [root.id]: { ...root, toggle: { ...root.toggle, ganbaru_open: false } } }))
      .toEqual([root.id, sibling.id]);
    expect(notesOutlineSubtreeIds(outlines, pageId, [root.id]))
      .toEqual([root.id, child.id, grandchild.id]);
    expect(ids(Object.fromEntries(blocks.map((block) => [block.id, block]))))
      .toEqual([root.id, child.id, grandchild.id, sibling.id]);
  });
});
