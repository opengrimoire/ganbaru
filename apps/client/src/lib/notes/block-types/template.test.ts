import { describe, expect, it } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import { buildNotesChildIdsByParent, type NotesTreeState } from "$lib/notes/blocks/tree";
import { notesTemplateBlockStatus } from "./template";
import type { NotesBlock, NotesBlockType, NotesBlockWrite, NotesParent } from "$lib/notes/types";

const now = "2026-07-01T09:00:00.000Z";

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
    case "child_page":
      return { ...base, type: write.type, child_page: write.child_page };
    case "template":
      return { ...base, type: write.type, template: write.template };
    default:
      throw new Error(`Unsupported fixture block type: ${write.type}`);
  }
}

function block(
  id: string,
  parent: NotesParent,
  type: Extract<NotesBlockType, "paragraph" | "child_page" | "template"> = "paragraph",
): NotesBlock {
  return blockFromWrite(createBlockWrite(id, type, id), parent);
}

function state(blocks: NotesBlock[]): NotesTreeState {
  return {
    blocksById: Object.fromEntries(blocks.map((item) => [item.id, item])),
    childIdsByParentId: buildNotesChildIdsByParent(blocks),
  };
}

describe("notes template block status", () => {
  it("reports empty templates as unavailable", () => {
    const tree = state([block("template", { type: "page_id", page_id: "page" }, "template")]);

    expect(notesTemplateBlockStatus(tree, "template")).toEqual({
      childCount: 0,
      containsChildPage: false,
      canUse: false,
      useUnavailableReason: "empty",
    });
  });

  it("allows templates with active reusable child blocks", () => {
    const tree = state([
      block("template", { type: "page_id", page_id: "page" }, "template"),
      block("child", { type: "block_id", block_id: "template" }),
    ]);

    expect(notesTemplateBlockStatus(tree, "template")).toEqual({
      childCount: 1,
      containsChildPage: false,
      canUse: true,
      useUnavailableReason: null,
    });
  });

  it("ignores trashed direct children", () => {
    const trashed = block("trashed", { type: "block_id", block_id: "template" });
    trashed.in_trash = true;
    const tree = state([
      block("template", { type: "page_id", page_id: "page" }, "template"),
      trashed,
    ]);

    expect(notesTemplateBlockStatus(tree, "template").childCount).toBe(0);
  });

  it("blocks use when loaded reusable content contains a child page", () => {
    const tree = state([
      block("template", { type: "page_id", page_id: "page" }, "template"),
      block("section", { type: "block_id", block_id: "template" }),
      block("nested-page", { type: "block_id", block_id: "section" }, "child_page"),
    ]);

    expect(notesTemplateBlockStatus(tree, "template")).toEqual({
      childCount: 1,
      containsChildPage: true,
      canUse: false,
      useUnavailableReason: "child_page",
    });
  });
});
