import { describe, expect, it } from "vitest";
import { createTextPayload } from "$lib/notes/blocks/factory";
import { buildNotesTableOfContents } from "./table-of-contents";
import type { NotesBlock, NotesBlockTreeItem, NotesBlockType } from "$lib/notes/types";

const now = "2026-06-30T12:30:00.000Z";

function block(id: string, type: NotesBlockType, text: string): NotesBlock {
  const base = {
    object: "block" as const,
    id,
    parent: { type: "page_id" as const, page_id: "page" },
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
  if (type === "heading_1") return { ...base, type, heading_1: createTextPayload(text) };
  if (type === "heading_2") return { ...base, type, heading_2: createTextPayload(text) };
  if (type === "heading_3") return { ...base, type, heading_3: createTextPayload(text) };
  if (type === "heading_4") return { ...base, type, heading_4: createTextPayload(text) };
  if (type === "table_of_contents") {
    return { ...base, type, table_of_contents: { color: "default" } };
  }
  return { ...base, type: "paragraph", paragraph: createTextPayload(text) };
}

function item(block: NotesBlock): NotesBlockTreeItem {
  return {
    block,
    depth: 0,
    parentId: "page",
    previousSiblingId: null,
    previousVisibleId: null,
  };
}

describe("notes table of contents", () => {
  it("builds a heading index in render order", () => {
    expect(
      buildNotesTableOfContents([
        item(block("toc", "table_of_contents", "")),
        item(block("intro", "heading_1", "Intro")),
        item(block("body", "paragraph", "Not indexed")),
        item(block("deep", "heading_3", "Details")),
        item(block("small", "heading_4", "Fine print")),
        item(block("empty", "heading_2", "")),
      ]),
    ).toEqual([
      { blockId: "intro", title: "Intro", level: 1 },
      { blockId: "deep", title: "Details", level: 3 },
      { blockId: "small", title: "Fine print", level: 4 },
    ]);
  });
});
