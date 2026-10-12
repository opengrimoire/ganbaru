import { describe, expect, it } from "vitest";
import {
  planNotesBlockDrop,
  planNotesBlockPageDrop,
} from "./drag";
import { buildNotesChildIdsByParent, type NotesTreeState } from "./tree";
import { createBlockWrite } from "./factory";
import type { NotesBlock, NotesBlockType, NotesBlockWrite, NotesParent } from "$lib/notes/types";

const now = "2026-07-01T00:00:00.000Z";

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
    case "heading_1":
      return { ...base, type: write.type, heading_1: write.heading_1 };
    case "heading_2":
      return { ...base, type: write.type, heading_2: write.heading_2 };
    case "heading_3":
      return { ...base, type: write.type, heading_3: write.heading_3 };
    case "heading_4":
      return { ...base, type: write.type, heading_4: write.heading_4 };
    case "heading_5":
      return { ...base, type: write.type, heading_5: write.heading_5 };
    case "heading_6":
      return { ...base, type: write.type, heading_6: write.heading_6 };
    case "bulleted_list_item":
      return { ...base, type: write.type, bulleted_list_item: write.bulleted_list_item };
    case "numbered_list_item":
      return { ...base, type: write.type, numbered_list_item: write.numbered_list_item };
    case "to_do":
      return { ...base, type: write.type, to_do: write.to_do };
    case "toggle":
      return { ...base, type: write.type, toggle: write.toggle };
    case "callout":
      return { ...base, type: write.type, callout: write.callout };
    case "quote":
      return { ...base, type: write.type, quote: write.quote };
    case "child_page":
      return { ...base, type: write.type, child_page: write.child_page };
    case "child_database":
      return { ...base, type: write.type, child_database: write.child_database };
    case "breadcrumb":
      return { ...base, type: write.type, breadcrumb: write.breadcrumb };
    case "table_of_contents":
      return { ...base, type: write.type, table_of_contents: write.table_of_contents };
    case "column_list":
      return { ...base, type: write.type, column_list: write.column_list };
    case "column":
      return { ...base, type: write.type, column: write.column };
    case "table":
      return { ...base, type: write.type, table: write.table };
    case "table_row":
      return { ...base, type: write.type, table_row: write.table_row };
    case "tab":
      return { ...base, type: write.type, tab: write.tab };
    case "image":
      return { ...base, type: write.type, image: write.image };
    case "video":
      return { ...base, type: write.type, video: write.video };
    case "audio":
      return { ...base, type: write.type, audio: write.audio };
    case "file":
      return { ...base, type: write.type, file: write.file };
    case "pdf":
      return { ...base, type: write.type, pdf: write.pdf };
    case "bookmark":
      return { ...base, type: write.type, bookmark: write.bookmark };
    case "link_preview":
      return { ...base, type: write.type, link_preview: write.link_preview };
    case "synced_block":
      return { ...base, type: write.type, synced_block: write.synced_block };
    case "template":
      return { ...base, type: write.type, template: write.template };
    case "button":
      return { ...base, type: write.type, button: write.button };
    case "embed":
      return { ...base, type: write.type, embed: write.embed };
    case "equation":
      return { ...base, type: write.type, equation: write.equation };
    case "divider":
      return { ...base, type: write.type, divider: write.divider };
    case "code":
      return { ...base, type: write.type, code: write.code };
    case "unsupported":
      return { ...base, type: write.type, unsupported: write.unsupported };
  }
}

function pageParent(pageId: string): NotesParent {
  return { type: "page_id", page_id: pageId };
}

function blockParent(blockId: string): NotesParent {
  return { type: "block_id", block_id: blockId };
}

function block(
  id: string,
  parent: NotesParent,
  type: NotesBlockType = "paragraph",
): NotesBlock {
  return blockFromWrite(createBlockWrite(id, type, id), parent);
}

function tree(blocks: NotesBlock[]): NotesTreeState {
  return {
    blocksById: Object.fromEntries(blocks.map((candidate) => [candidate.id, candidate])),
    childIdsByParentId: buildNotesChildIdsByParent(blocks),
  };
}

describe("notes block drag planning", () => {
  it("plans reorder before and after blocks across valid parents", () => {
    const state = tree([
      block("a", pageParent("page")),
      block("b", pageParent("page")),
      block("c", blockParent("a")),
    ]);

    expect(planNotesBlockDrop(state, "b", "a", "before")).toEqual({
      blockId: "b",
      parentId: "page",
      after: null,
      before: "a",
      indicator: "before",
    });
    expect(planNotesBlockDrop(state, "b", "c", "after")).toEqual({
      blockId: "b",
      parentId: "a",
      after: "c",
      before: null,
      indicator: "after",
    });
  });

  it("plans nesting into parents that can accept the source block type", () => {
    const state = tree([
      block("parent", pageParent("page")),
      block("existing", blockParent("parent")),
      block("source", pageParent("page"), "to_do"),
    ]);

    expect(planNotesBlockDrop(state, "source", "parent", "inside")).toEqual({
      blockId: "source",
      parentId: "parent",
      after: "existing",
      before: null,
      indicator: "inside",
    });
  });

  it("plans outdent after the target parent block", () => {
    const state = tree([
      block("parent", pageParent("page")),
      block("target", blockParent("parent")),
      block("source", blockParent("target")),
    ]);

    expect(planNotesBlockDrop(state, "source", "target", "outdent")).toEqual({
      blockId: "source",
      parentId: "page",
      after: "parent",
      before: null,
      indicator: "outdent",
    });
  });

  it("rejects cycles, internal structural blocks, and invalid target parents", () => {
    const state = tree([
      block("parent", pageParent("page")),
      block("child", blockParent("parent")),
      block("table", pageParent("page"), "table"),
      block("row", blockParent("table"), "table_row"),
    ]);

    expect(planNotesBlockDrop(state, "parent", "child", "inside")).toBeNull();
    expect(planNotesBlockDrop(state, "row", "parent", "before")).toBeNull();
    expect(planNotesBlockDrop(state, "parent", "table", "inside")).toBeNull();
  });

  it("plans dropping a block on another page and rejects no-op or subtree pages", () => {
    const state = tree([
      block("source", pageParent("page")),
      block("nested-page", blockParent("source"), "child_page"),
    ]);

    expect(planNotesBlockPageDrop(state, "source", "other-page", "page")).toEqual({
      blockId: "source",
      pageId: "other-page",
    });
    expect(planNotesBlockPageDrop(state, "source", "page", "page")).toBeNull();
    expect(planNotesBlockPageDrop(state, "source", "nested-page", "page")).toBeNull();
  });
});
