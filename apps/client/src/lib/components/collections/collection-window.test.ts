import { describe, expect, it } from "vitest";
import { collectionWindow } from "./collection-window";

describe("collection card windows", () => {
  it("accounts for tall cards and the gaps surrounding virtual spacers", () => {
    const heights = [80, 240, 120, 60, 100];
    const range = collectionWindow(heights, 340, 130, 8, 0);
    expect(range).toEqual({ start: 2, end: 4, beforePx: 328, afterPx: 100 });
    const renderedHeight = range.beforePx + 8 + 120 + 8 + 60 + 8 + range.afterPx;
    expect(renderedHeight).toBe(heights.reduce((sum, height) => sum + height, 0) + 4 * 8);
  });

  it("keeps the final cards reachable when a filtered result shrinks below the current scroll", () => {
    expect(collectionWindow([100, 200], 3000, 300, 8, 1)).toEqual({ start: 0, end: 2, beforePx: 0, afterPx: 0 });
  });

  it("includes overscan without producing negative empty or hidden viewport ranges", () => {
    expect(collectionWindow([], 0, 0, 8, 3)).toEqual({ start: 0, end: 0, beforePx: 0, afterPx: 0 });
    expect(collectionWindow([100, 100, 100], -10, 0, 8, 1)).toEqual({ start: 0, end: 2, beforePx: 0, afterPx: 100 });
  });
});
