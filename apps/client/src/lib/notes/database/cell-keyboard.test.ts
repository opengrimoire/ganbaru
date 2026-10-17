import { describe, expect, it } from "vitest";
import { notesDatabaseCellKeepsKey, type NotesDatabaseCellCaret } from "./cell-keyboard";

function caret(value: string, start: number, end = start, multiline = false): NotesDatabaseCellCaret {
  return { multiline, value, selectionStart: start, selectionEnd: end };
}

describe("notesDatabaseCellKeepsKey", () => {
  it("never keeps keys from controls that are not text fields", () => {
    for (const key of ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Enter"]) {
      expect(notesDatabaseCellKeepsKey(key, false, null)).toBe(false);
    }
  });

  it("keeps horizontal arrows while the caret can still move", () => {
    expect(notesDatabaseCellKeepsKey("ArrowLeft", false, caret("abc", 1))).toBe(true);
    expect(notesDatabaseCellKeepsKey("ArrowRight", false, caret("abc", 1))).toBe(true);
  });

  it("releases horizontal arrows at either end of the text", () => {
    expect(notesDatabaseCellKeepsKey("ArrowLeft", false, caret("abc", 0))).toBe(false);
    expect(notesDatabaseCellKeepsKey("ArrowRight", false, caret("abc", 3))).toBe(false);
  });

  it("keeps an arrow while a selection reaches away from that end", () => {
    expect(notesDatabaseCellKeepsKey("ArrowLeft", false, caret("abc", 0, 2))).toBe(true);
    expect(notesDatabaseCellKeepsKey("ArrowRight", false, caret("abc", 1, 3))).toBe(true);
  });

  it("releases vertical arrows and Enter in single-line fields", () => {
    expect(notesDatabaseCellKeepsKey("ArrowUp", false, caret("abc", 1))).toBe(false);
    expect(notesDatabaseCellKeepsKey("ArrowDown", false, caret("abc", 1))).toBe(false);
    expect(notesDatabaseCellKeepsKey("Enter", true, caret("abc", 1))).toBe(false);
  });

  it("keeps vertical arrows inside wrapped text until the caret reaches an end", () => {
    expect(notesDatabaseCellKeepsKey("ArrowUp", false, caret("a\nb", 2, 2, true))).toBe(true);
    expect(notesDatabaseCellKeepsKey("ArrowUp", false, caret("a\nb", 0, 0, true))).toBe(false);
    expect(notesDatabaseCellKeepsKey("ArrowDown", false, caret("a\nb", 1, 1, true))).toBe(true);
    expect(notesDatabaseCellKeepsKey("ArrowDown", false, caret("a\nb", 3, 3, true))).toBe(false);
  });

  it("keeps Shift+Enter for a line break in wrapped text but not plain Enter", () => {
    expect(notesDatabaseCellKeepsKey("Enter", true, caret("a", 1, 1, true))).toBe(true);
    expect(notesDatabaseCellKeepsKey("Enter", false, caret("a", 1, 1, true))).toBe(false);
  });
});
