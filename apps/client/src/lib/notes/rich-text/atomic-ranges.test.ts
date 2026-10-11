import { describe, expect, it } from "vitest";
import {
  normalizeNotesSelectionForAtomicRanges,
  notesAtomicTextRanges,
  snapNotesDirectionalSelection,
  snapNotesOffsetToAtomicEdge,
} from "./atomic-ranges";
import {
  createLocalObjectMentionRichText,
  createPageMentionRichText,
  createTextRichText,
} from "./core";

const PAGE_ID = "11111111-1111-4111-8111-111111111111";

describe("Notes atomic text ranges", () => {
  it("collects every mention span and skips plain text", () => {
    expect(notesAtomicTextRanges([
      createTextRichText("Eat "),
      createLocalObjectMentionRichText("project", "eating", "Eating"),
      createPageMentionRichText(PAGE_ID, "Plan"),
      createTextRichText(" today"),
    ])).toEqual([
      { start: 4, end: 10 },
      { start: 10, end: 14 },
    ]);
  });

  it("snaps a caret inside a token to its nearest edge", () => {
    const ranges = [{ start: 4, end: 10 }];
    expect(snapNotesOffsetToAtomicEdge(ranges, 5)).toBe(4);
    expect(snapNotesOffsetToAtomicEdge(ranges, 9)).toBe(10);
    expect(snapNotesOffsetToAtomicEdge(ranges, 7)).toBe(10);
    expect(snapNotesOffsetToAtomicEdge(ranges, 4)).toBe(4);
    expect(snapNotesOffsetToAtomicEdge(ranges, 2)).toBe(2);
  });

  it("grows a selection that cuts through tokens to cover them whole", () => {
    const ranges = [{ start: 4, end: 10 }, { start: 14, end: 20 }];
    expect(normalizeNotesSelectionForAtomicRanges(ranges, { start: 6, end: 16 })).toEqual({
      start: 4,
      end: 20,
    });
    expect(normalizeNotesSelectionForAtomicRanges(ranges, { start: 0, end: 4 })).toEqual({
      start: 0,
      end: 4,
    });
  });

  it("snaps each end of a directional selection outward without reversing it", () => {
    const ranges = [{ start: 4, end: 10 }, { start: 14, end: 20 }];
    expect(snapNotesDirectionalSelection(ranges, { anchor: 6, focus: 16 })).toEqual({ anchor: 4, focus: 20 });
    expect(snapNotesDirectionalSelection(ranges, { anchor: 16, focus: 6 })).toEqual({ anchor: 20, focus: 4 });
    expect(snapNotesDirectionalSelection(ranges, { anchor: 12, focus: 15 })).toEqual({ anchor: 12, focus: 20 });
    expect(snapNotesDirectionalSelection(ranges, { anchor: 5, focus: 5 })).toEqual({ anchor: 4, focus: 4 });
    expect(snapNotesDirectionalSelection(ranges, { anchor: 10, focus: 14 })).toEqual({ anchor: 10, focus: 14 });
  });
});
