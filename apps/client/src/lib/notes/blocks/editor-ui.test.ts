import { describe, expect, it } from "vitest";
import type { NotesBlock, NotesBlockWrite } from "$lib/notes/types";
import { createBlockWrite } from "./factory";
import {
  isNotesPageBodyEmpty,
  notesBlockMarker,
  notesNumberedListOrdinals,
  notesRichTextEditorClass,
  notesRichTextPreviewClass,
  notesTextareaClass,
} from "./editor-ui";

describe("notes block editor UI helpers", () => {
  it("numbers explicit indentation levels independently without interrupting the outer list", () => {
    const rows = [0, 1, 1, 2, 1, 0].map((indent, index) => ({ id: String(index), parentId: "page", type: "numbered_list_item", indent }));
    expect([...notesNumberedListOrdinals(rows).values()]).toEqual([1, 1, 2, 1, 3, 2]);
  });

  it("keeps heading and code editor classes distinct", () => {
    expect(notesTextareaClass("heading_1")).toContain("notes-heading-1");
    expect(notesTextareaClass("heading_2")).toContain("notes-heading-2");
    expect(notesTextareaClass("heading_3")).toContain("notes-heading-3");
    expect(notesTextareaClass("heading_4")).toContain("notes-heading-4");
    expect(notesTextareaClass("heading_1")).toContain("leading-[1.3]");
    expect(notesTextareaClass("code")).toContain("font-mono");
  });

  it("adds preview-only affordances on top of text block classes", () => {
    expect(notesRichTextPreviewClass("paragraph")).toContain("cursor-text");
    expect(notesRichTextPreviewClass("paragraph")).toContain("whitespace-pre-wrap");
  });

  it("adds rich editor affordances on top of text block classes", () => {
    expect(notesRichTextEditorClass("paragraph")).toContain("notes-rich-text-editor");
    expect(notesRichTextEditorClass("paragraph")).toContain("break-words");
    expect(notesRichTextEditorClass("paragraph")).toContain("notes-editor-body-text");
    expect(notesRichTextEditorClass("paragraph")).toContain("leading-normal");
    expect(notesRichTextEditorClass("paragraph")).not.toContain("focus-visible:ring");
    expect(notesRichTextEditorClass("code")).toContain("font-mono");
  });

  it("keeps ordinary text flush with the page title while padding code backgrounds", () => {
    expect(notesTextareaClass("paragraph")).not.toContain("pl-1");
    expect(notesTextareaClass("paragraph")).not.toContain("px-1");
    expect(notesTextareaClass("code")).toContain("pl-1");
  });

  it("returns stable visible markers for list-like blocks", () => {
    expect(notesBlockMarker("bulleted_list_item")).toBe("•");
    expect(notesBlockMarker("numbered_list_item")).toBe("1.");
    expect(notesBlockMarker("paragraph")).toBe("");
    expect(notesBlockMarker("numbered_list_item", 12)).toBe("12.");
  });
});

describe("empty page body", () => {
  function item(write: NotesBlockWrite, hasChildren = false): { block: NotesBlock } {
    const base = {
      object: "block" as const, id: write.id, parent: { type: "page_id" as const, page_id: "page" },
      created_time: "2026-10-11T09:00:00.000Z", last_edited_time: "2026-10-11T09:00:00.000Z",
      has_children: hasChildren, in_trash: false, archived: false,
      source_provider: null, source_object_id: null, source_last_edited_time: null,
    };
    if (write.type === "paragraph") return { block: { ...base, type: write.type, paragraph: write.paragraph } };
    if (write.type === "heading_1") return { block: { ...base, type: write.type, heading_1: write.heading_1 } };
    throw new Error(`unsupported test block type: ${write.type}`);
  }

  it("treats a lone empty paragraph as an empty page", () => {
    expect(isNotesPageBodyEmpty([item(createBlockWrite("a"))])).toBe(true);
  });

  it("keeps the hint away once the page has text, structure, or more blocks", () => {
    expect(isNotesPageBodyEmpty([])).toBe(false);
    expect(isNotesPageBodyEmpty([item(createBlockWrite("a", "paragraph", " "))])).toBe(false);
    expect(isNotesPageBodyEmpty([item(createBlockWrite("a", "heading_1"))])).toBe(false);
    expect(isNotesPageBodyEmpty([item(createBlockWrite("a"), true)])).toBe(false);
    expect(isNotesPageBodyEmpty([item(createBlockWrite("a")), item(createBlockWrite("b"))])).toBe(false);
  });
});

describe("numbered list sequences", () => {
  const item = (id: string, parentId = "page", type = "numbered_list_item") => ({ id, parentId, type });

  it("counts sibling items across nested content and resets only on a different sibling type", () => {
    expect([...notesNumberedListOrdinals([
      item("a"), item("child-a", "a"), item("child-b", "a"),
      item("child-text", "a", "paragraph"), item("child-c", "a"), item("b"),
      item("break", "page", "bulleted_list_item"), item("c"), item("d"),
      item("other-column", "column"), item("other-tab", "tab"),
    ])]).toEqual([
      ["a", 1], ["child-a", 1], ["child-b", 2], ["child-c", 1], ["b", 2],
      ["c", 1], ["d", 2], ["other-column", 1], ["other-tab", 1],
    ]);
  });

  it("uses the full outline so a window starting mid-list retains its ordinal", () => {
    const outline = Array.from({ length: 120 }, (_, index) => item(String(index)));
    const numbers = notesNumberedListOrdinals(outline);
    expect(numbers.get("99")).toBe(100);
    expect(numbers.get("119")).toBe(120);
    expect([...notesNumberedListOrdinals([outline[2], outline[0]])]).toEqual([["2", 1], ["0", 2]]);
    expect(notesNumberedListOrdinals([]).size).toBe(0);
  });
});
