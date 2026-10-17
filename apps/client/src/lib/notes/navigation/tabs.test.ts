import { describe, expect, it } from "vitest";
import {
  NOTES_TAB_MAX_COUNT,
  createNotesTabLabelWrite,
  notesTabCanAdd,
  notesTabCanMove,
  notesTabCanRemove,
  notesTabIconOptions,
  notesTabIconText,
  notesTabLabelWithIcon,
  notesTabLabelWithText,
} from "./tabs";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesIcon, NotesParagraphBlock, NotesTabBlockItems } from "$lib/notes/types";

function paragraphBlock(id: string, label: string, icon?: NotesIcon | null): NotesParagraphBlock {
  return {
    object: "block",
    id,
    parent: { type: "block_id", block_id: "00000000-0000-4000-8000-000000000001" },
    created_time: "2026-01-01T00:00:00.000Z",
    last_edited_time: "2026-01-01T00:00:00.000Z",
    has_children: false,
    in_trash: false,
    type: "paragraph",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    paragraph: {
      rich_text: [createRichText(label)],
      color: "default",
      ...(icon === undefined ? {} : { icon }),
    },
  };
}

function tabItem(id: string, label: string): NotesTabBlockItems {
  return {
    label: paragraphBlock(id, label),
    items: [],
  };
}

describe("notes tab helpers", () => {
  it("limits local tab counts", () => {
    expect(notesTabCanAdd(0)).toBe(true);
    expect(notesTabCanAdd(NOTES_TAB_MAX_COUNT)).toBe(false);
    expect(notesTabCanRemove(1)).toBe(false);
    expect(notesTabCanRemove(2)).toBe(true);
  });

  it("plans tab movement by label row", () => {
    const tabs = [tabItem("a", "A"), tabItem("b", "B"), tabItem("c", "C")];

    expect(notesTabCanMove(tabs, "a", "left")).toBe(false);
    expect(notesTabCanMove(tabs, "a", "right")).toBe(true);
    expect(notesTabCanMove(tabs, "b", "left")).toBe(true);
    expect(notesTabCanMove(tabs, "c", "right")).toBe(false);
    expect(notesTabCanMove(tabs, "missing", "right")).toBe(false);
  });

  it("creates Notion-shaped tab label paragraph writes", () => {
    const icon: NotesIcon = { type: "icon", icon: { name: "star", color: "yellow" } };
    const write = createNotesTabLabelWrite("label", " Roadmap ", icon);

    expect(write).toEqual({
      id: "label",
      type: "paragraph",
      paragraph: {
        rich_text: [createRichText("Roadmap")],
        color: "default",
        icon,
      },
    });
  });

  it("preserves icons when renaming tab labels", () => {
    const icon: NotesIcon = { type: "emoji", emoji: "⭐" };
    const update = notesTabLabelWithText(paragraphBlock("label", "Old", icon), "New");

    expect(update).toEqual({
      type: "paragraph",
      paragraph: {
        rich_text: [createRichText("New")],
        color: "default",
        icon,
      },
    });
  });

  it("preserves label text when changing tab icons", () => {
    const icon: NotesIcon = { type: "emoji", emoji: "📌" };
    const update = notesTabLabelWithIcon(paragraphBlock("label", "Ideas"), icon);

    expect(update).toEqual({
      type: "paragraph",
      paragraph: {
        rich_text: [createRichText("Ideas")],
        color: "default",
        icon,
      },
    });
  });

  it("offers clearable emoji and native icon payloads", () => {
    const options = notesTabIconOptions();

    expect(options[0]).toEqual({ id: "none", label: "No icon", icon: null, text: "" });
    expect(options.some((option) => option.icon?.type === "emoji")).toBe(true);
    expect(options.some((option) => option.icon?.type === "icon")).toBe(true);
    expect(notesTabIconText({ type: "emoji", emoji: "✅" })).toBe("✅");
    expect(notesTabIconText(null)).toBeNull();
  });
});
