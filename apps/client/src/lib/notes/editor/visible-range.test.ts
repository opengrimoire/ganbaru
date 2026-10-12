import { describe, expect, it } from "vitest";
import {
  notesScrollAnchor,
  notesScrollOffsetForAnchor,
  notesVisibleRange,
  type NotesMeasuredRangeItem,
} from "./visible-range";

function items(count: number, height = 40): NotesMeasuredRangeItem[] {
  return Array.from({ length: count }, (_, index) => ({ id: `row-${index}`, estimatedHeight: height }));
}

describe("Notes visible range", () => {
  it("does not virtualize small collections", () => {
    expect(notesVisibleRange(items(20), new Map(), {
      viewportStart: 300,
      viewportEnd: 600,
      overscanPx: 100,
      minimumVirtualizedCount: 100,
    })).toMatchObject({ start: 0, end: 20, topHeight: 0, bottomHeight: 0 });
  });

  it("bounds retained rows to the viewport and overscan", () => {
    const range = notesVisibleRange(items(10_000), new Map(), {
      viewportStart: 4_000,
      viewportEnd: 4_600,
      overscanPx: 200,
      minimumVirtualizedCount: 100,
    });
    expect(range.start).toBe(95);
    expect(range.end).toBe(120);
    expect(range.topHeight + range.bottomHeight).toBe(399_000);
    expect(range.end - range.start).toBeLessThanOrEqual(25);
  });

  it("keeps the same anchor when preceding estimates are corrected", () => {
    const rows = items(200);
    const before = new Map<string, number>();
    const anchor = notesScrollAnchor(rows, before, 2_020);
    expect(anchor).toEqual({ id: "row-50", offset: 20 });
    const after = new Map<string, number>([["row-10", 80], ["row-20", 60]]);
    expect(notesScrollOffsetForAnchor(rows, after, anchor)).toBe(2_080);
  });
});
