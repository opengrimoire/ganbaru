import { describe, expect, it } from "vitest";
import { createBlockWrite, createRichText } from "$lib/notes/blocks/factory";
import { flattenNotesBlockOutlines, notesBlockOutlineFromBlock } from "$lib/notes/blocks/outline";
import { notesCalloutLayers, notesCalloutIsOwnTextHidden, notesEmbeddedCalloutLayers } from "./callout-layout";
import type { NotesBlock, NotesBlockTreeItem, NotesParent } from "$lib/notes/types";

const pageId = "page";

function block(id: string, type: NotesBlock["type"], parent: NotesParent, color?: "blue_background"): NotesBlock {
  const write = createBlockWrite(id, type);
  if (write.type === "callout" && color) write.callout.color = color;
  return {
    ...write, object: "block", parent, created_time: "", last_edited_time: "",
    has_children: false, in_trash: false, source_provider: null,
    source_object_id: null, source_last_edited_time: null,
  } as NotesBlock;
}

describe("Notes callout surfaces", () => {
  it("hides only an empty callout label that has children", () => {
    const callout = block("callout", "callout", { type: "page_id", page_id: pageId });
    expect(notesCalloutIsOwnTextHidden(callout, 0)).toBe(false);
    expect(notesCalloutIsOwnTextHidden(callout, 1)).toBe(true);
    callout.has_children = true;
    expect(notesCalloutIsOwnTextHidden(callout, 0)).toBe(true);
    if (callout.type !== "callout") throw new Error("Expected callout");
    callout.callout.rich_text = [createRichText("Label")];
    expect(notesCalloutIsOwnTextHidden(callout, 1)).toBe(false);
  });

  it("keeps every nested descendant inside its callout and rounds each callout boundary", () => {
    const page: NotesParent = { type: "page_id", page_id: pageId };
    const blocks = [
      block("before", "paragraph", page),
      block("outer", "callout", page, "blue_background"),
      block("heading", "heading_1", { type: "block_id", block_id: "outer" }),
      block("inner", "callout", { type: "block_id", block_id: "outer" }),
      block("body", "paragraph", { type: "block_id", block_id: "inner" }),
      block("after", "paragraph", page),
    ];
    const byId = new Map(blocks.map((item) => [item.id, item]));
    const outlines = flattenNotesBlockOutlines(blocks.map((item, index) =>
      notesBlockOutlineFromBlock(item, pageId, index)), pageId, Object.fromEntries(byId));
    const layers = notesCalloutLayers(outlines, (id) => byId.get(id));
    expect(layers.has("before")).toBe(false);
    expect(layers.get("outer")).toEqual([{ id: "outer", color: "blue_background", first: true, last: false }]);
    expect(layers.get("heading")).toEqual([{ id: "outer", color: "blue_background", first: false, last: false }]);
    expect(layers.get("inner")).toEqual([
      { id: "outer", color: "blue_background", first: false, last: false },
      { id: "inner", color: "gray_background", first: true, last: false },
    ]);
    expect(layers.get("body")).toEqual([
      { id: "outer", color: "blue_background", first: false, last: true },
      { id: "inner", color: "gray_background", first: false, last: true },
    ]);
    expect(layers.has("after")).toBe(false);
  });

  it("groups callouts and their children in embedded column and tab rows", () => {
    const parent: NotesParent = { type: "block_id", block_id: "column" };
    const callout = block("callout", "callout", parent, "blue_background");
    const heading = block("heading", "heading_1", { type: "block_id", block_id: callout.id });
    const after = block("after", "paragraph", parent);
    const items: NotesBlockTreeItem[] = [callout, heading, after].map((entry, index) => ({
      block: entry,
      depth: index === 1 ? 1 : 0,
      parentId: entry.parent.type === "block_id" ? entry.parent.block_id : pageId,
      previousSiblingId: null,
      previousVisibleId: index > 0 ? [callout, heading, after][index - 1].id : null,
    }));
    expect(notesEmbeddedCalloutLayers(items).get("heading")).toEqual([
      { id: "callout", color: "blue_background", first: false, last: true },
    ]);
    expect(notesEmbeddedCalloutLayers(items).has("after")).toBe(false);
  });
});
