import { describe, expect, it } from "vitest";
import { createBlockWrite } from "./factory";
import {
  mergeRichTextForBackspace,
  notesBackspaceCanMergeBlockTypes,
  notesParentCanAcceptBlockType,
} from "./backspace";
import {
  applyRichTextAnnotations,
  createLinkedTextRichText,
  createPageMentionRichText,
  createTextRichText,
  richTextPlainText,
} from "$lib/notes/rich-text/core";
import type { NotesBlock, NotesBlockType, NotesBlockWrite, NotesParent } from "$lib/notes/types";

const now = "2026-06-30T09:00:00.000Z";

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
    case "column_list":
      return { ...base, type: write.type, column_list: write.column_list };
    case "table":
      return { ...base, type: write.type, table: write.table };
    case "tab":
      return { ...base, type: write.type, tab: write.tab };
    default:
      throw new Error(`unsupported test block type: ${write.type}`);
  }
}

function block(id: string, type: NotesBlockType = "paragraph"): NotesBlock {
  return blockFromWrite(createBlockWrite(id, type), { type: "page_id", page_id: "page" });
}

const textEditableTypes: readonly NotesBlockType[] = [
  "paragraph",
  "heading_1",
  "heading_2",
  "heading_3",
  "heading_4",
  "bulleted_list_item",
  "numbered_list_item",
  "to_do",
  "toggle",
  "callout",
  "quote",
  "template",
  "button",
  "code",
];

describe("notes Backspace behavior helpers", () => {
  it("allows merge planning for every text-editable block type", () => {
    for (const sourceType of textEditableTypes) {
      expect(notesBackspaceCanMergeBlockTypes(sourceType, "paragraph")).toBe(true);
    }
    expect(notesBackspaceCanMergeBlockTypes("paragraph", "divider")).toBe(false);
    expect(notesBackspaceCanMergeBlockTypes("divider", "paragraph")).toBe(false);
  });

  it("preserves rich text objects and compatible text spans across merge", () => {
    const [bold] = applyRichTextAnnotations(
      [createTextRichText("Bold")],
      0,
      "Bold".length,
      { bold: true },
    );
    if (!bold) throw new Error("expected bold rich text");
    const merged = mergeRichTextForBackspace(
      [
        createTextRichText("Read "),
        createLinkedTextRichText("docs", "https://example.com/docs"),
      ],
      [
        createTextRichText(" with "),
        createPageMentionRichText("11111111-1111-4111-8111-111111111111", "Project", null),
        createTextRichText(" "),
        bold,
      ],
    );

    expect(richTextPlainText(merged)).toBe("Read docs with Project Bold");
    expect(merged.some((item) => item.type === "mention")).toBe(true);
    expect(
      merged.some((item) =>
        item.type === "text" && item.text.link?.url === "https://example.com/docs"
      ),
    ).toBe(true);
    expect(merged.at(-1)).toMatchObject({ type: "text", annotations: { bold: true } });
  });

  it("checks structural parents before moving children", () => {
    expect(notesParentCanAcceptBlockType(null, "paragraph")).toBe(true);
    expect(notesParentCanAcceptBlockType(null, "table_row")).toBe(false);
    expect(notesParentCanAcceptBlockType(block("parent"), "paragraph")).toBe(true);
    expect(notesParentCanAcceptBlockType(block("columns", "column_list"), "column")).toBe(true);
    expect(notesParentCanAcceptBlockType(block("columns", "column_list"), "paragraph")).toBe(false);
    expect(notesParentCanAcceptBlockType(block("table", "table"), "table_row")).toBe(true);
    expect(notesParentCanAcceptBlockType(block("tab", "tab"), "paragraph")).toBe(true);
    expect(notesParentCanAcceptBlockType(block("tab", "tab"), "heading_1")).toBe(false);
  });
});
