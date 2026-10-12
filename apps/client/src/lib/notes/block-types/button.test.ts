import { describe, expect, it } from "vitest";
import { createBlockWrite, createButtonPayload } from "$lib/notes/blocks/factory";
import { buildNotesChildIdsByParent, type NotesTreeState } from "$lib/notes/blocks/tree";
import {
  NOTES_BUTTON_UNSUPPORTED_ACTION_AVAILABILITY,
  NOTES_BUTTON_UNSUPPORTED_ACTIONS,
  notesButtonBlockStatus,
  notesButtonPrimaryInsertPosition,
  notesButtonWithIcon,
  notesButtonWithPrimaryInsertPosition,
} from "./button";
import { createNotesNativePageIcon } from "$lib/notes/pages/icon";
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
    case "button":
      return { ...base, type: write.type, button: write.button };
    default:
      throw new Error(`Unsupported fixture block type: ${write.type}`);
  }
}

function block(
  id: string,
  parent: NotesParent,
  type: Extract<NotesBlockType, "paragraph" | "child_page" | "button"> = "paragraph",
): NotesBlock {
  return blockFromWrite(createBlockWrite(id, type, id), parent);
}

function state(blocks: NotesBlock[]): NotesTreeState {
  return {
    blocksById: Object.fromEntries(blocks.map((item) => [item.id, item])),
    childIdsByParentId: buildNotesChildIdsByParent(blocks),
  };
}

describe("notes button block status", () => {
  it("reports empty buttons as unavailable", () => {
    const tree = state([block("button", { type: "page_id", page_id: "page" }, "button")]);

    expect(notesButtonBlockStatus(tree, "button")).toEqual({
      childCount: 0,
      containsChildPage: false,
      canUse: false,
      useUnavailableReason: "empty",
    });
  });

  it("allows buttons with active source child blocks", () => {
    const tree = state([
      block("button", { type: "page_id", page_id: "page" }, "button"),
      block("child", { type: "block_id", block_id: "button" }),
    ]);

    expect(notesButtonBlockStatus(tree, "button")).toEqual({
      childCount: 1,
      containsChildPage: false,
      canUse: true,
      useUnavailableReason: null,
    });
  });

  it("ignores trashed direct source children", () => {
    const trashed = block("trashed", { type: "block_id", block_id: "button" });
    trashed.in_trash = true;
    const tree = state([
      block("button", { type: "page_id", page_id: "page" }, "button"),
      trashed,
    ]);

    expect(notesButtonBlockStatus(tree, "button").childCount).toBe(0);
  });

  it("blocks use when loaded source content contains a child page", () => {
    const tree = state([
      block("button", { type: "page_id", page_id: "page" }, "button"),
      block("section", { type: "block_id", block_id: "button" }),
      block("nested-page", { type: "block_id", block_id: "section" }, "child_page"),
    ]);

    expect(notesButtonBlockStatus(tree, "button")).toEqual({
      childCount: 1,
      containsChildPage: true,
      canUse: false,
      useUnavailableReason: "child_page",
    });
  });
});

describe("notes button payload updates", () => {
  it("reads and updates the primary insert position", () => {
    const payload = createButtonPayload("Add agenda");

    expect(notesButtonPrimaryInsertPosition(payload)).toBe("below_button");

    const updated = notesButtonWithPrimaryInsertPosition(payload, "top_of_page");

    expect(updated.actions[0]).toEqual({
      type: "insert_blocks",
      source: "children",
      position: "top_of_page",
    });
    expect(notesButtonPrimaryInsertPosition(updated)).toBe("top_of_page");
    expect(notesButtonPrimaryInsertPosition(payload)).toBe("below_button");
  });

  it("updates the icon without changing label or actions", () => {
    const payload = createButtonPayload("Add agenda");
    const icon = createNotesNativePageIcon("star", "yellow");

    const updated = notesButtonWithIcon(payload, icon);

    expect(updated.icon).toEqual(icon);
    expect(updated.rich_text).toEqual(payload.rich_text);
    expect(updated.actions).toEqual(payload.actions);
    expect(updated).not.toBe(payload);
    expect(updated.actions).not.toBe(payload.actions);
  });

  it("keeps unsupported broad actions unavailable", () => {
    expect(NOTES_BUTTON_UNSUPPORTED_ACTIONS).toEqual([
      "database_edit",
      "webhook",
      "destructive_automation",
    ]);
    expect(NOTES_BUTTON_UNSUPPORTED_ACTIONS.every(
      (action) => NOTES_BUTTON_UNSUPPORTED_ACTION_AVAILABILITY[action] === false,
    )).toBe(true);
  });
});
