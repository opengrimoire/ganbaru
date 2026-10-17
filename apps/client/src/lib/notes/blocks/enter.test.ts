import { describe, expect, it } from "vitest";
import {
  notesEmptyEnterReturnsParagraph,
  notesEnterSiblingBlockType,
  notesEnterSplitsRichTextBlock,
} from "./enter";

describe("notes Enter block behavior", () => {
  it("splits rich text document blocks", () => {
    expect(notesEnterSplitsRichTextBlock("paragraph")).toBe(true);
    expect(notesEnterSplitsRichTextBlock("heading_1")).toBe(true);
    expect(notesEnterSplitsRichTextBlock("bulleted_list_item")).toBe(true);
    expect(notesEnterSplitsRichTextBlock("to_do")).toBe(true);
    expect(notesEnterSplitsRichTextBlock("callout")).toBe(true);
    expect(notesEnterSplitsRichTextBlock("code")).toBe(false);
    expect(notesEnterSplitsRichTextBlock("bookmark")).toBe(false);
  });

  it("chooses the sibling type created by Enter", () => {
    expect(notesEnterSiblingBlockType("paragraph")).toBe("paragraph");
    expect(notesEnterSiblingBlockType("bulleted_list_item")).toBe("bulleted_list_item");
    expect(notesEnterSiblingBlockType("numbered_list_item")).toBe("numbered_list_item");
    expect(notesEnterSiblingBlockType("to_do")).toBe("to_do");
    expect(notesEnterSiblingBlockType("toggle")).toBe("paragraph");
    expect(notesEnterSiblingBlockType("heading_2")).toBe("paragraph");
    expect(notesEnterSiblingBlockType("callout")).toBe("paragraph");
    expect(notesEnterSiblingBlockType("quote")).toBe("paragraph");
  });

  it("returns empty list-like blocks to paragraph on Enter", () => {
    expect(notesEmptyEnterReturnsParagraph("bulleted_list_item")).toBe(true);
    expect(notesEmptyEnterReturnsParagraph("numbered_list_item")).toBe(true);
    expect(notesEmptyEnterReturnsParagraph("to_do")).toBe(true);
    expect(notesEmptyEnterReturnsParagraph("toggle")).toBe(false);
    expect(notesEmptyEnterReturnsParagraph("callout")).toBe(false);
    expect(notesEmptyEnterReturnsParagraph("quote")).toBe(true);
    expect(notesEmptyEnterReturnsParagraph("paragraph")).toBe(false);
  });
});
