import { describe, expect, it } from "vitest";
import {
  BOTTOM_SHEET_HALF_RATIO,
  bottomSheetDragHeight,
  bottomSheetHeights,
  bottomSheetReleaseVelocity,
  higherBottomSheetDetent,
  settleBottomSheet,
} from "./bottom-sheet";

describe("bottomSheetHeights", () => {
  it("keeps the header, summary, half, and full heights in order", () => {
    expect(bottomSheetHeights(800, 60, 180)).toEqual({
      dip: 60,
      peek: 180,
      half: Math.round(800 * BOTTOM_SHEET_HALF_RATIO),
      full: 800,
    });
  });

  it("raises half to the peek when the summary is taller than half the sheet", () => {
    const heights = bottomSheetHeights(300, 60, 220);
    expect(heights.half).toBe(220);
    expect(heights.peek).toBe(220);
  });

  it("clamps every detent inside a sheet shrunk by the keyboard", () => {
    expect(bottomSheetHeights(120, 60, 180)).toEqual({ dip: 60, peek: 120, half: 120, full: 120 });
  });

  it("never lets the peek fall below the header", () => {
    expect(bottomSheetHeights(800, 60, 20).peek).toBe(60);
  });

  it("returns zero heights before the sheet is measured", () => {
    expect(bottomSheetHeights(0, 60, 180)).toEqual({ dip: 0, peek: 0, half: 0, full: 0 });
  });
});

describe("bottomSheetDragHeight", () => {
  const heights = bottomSheetHeights(800, 60, 180);

  it("follows the finger up and down", () => {
    expect(bottomSheetDragHeight(180, -100, heights)).toBe(280);
    expect(bottomSheetDragHeight(180, 50, heights)).toBe(130);
  });

  it("stops at the full height and at the bottom edge", () => {
    expect(bottomSheetDragHeight(700, -400, heights)).toBe(800);
    expect(bottomSheetDragHeight(180, 400, heights)).toBe(0);
  });
});

describe("bottomSheetReleaseVelocity", () => {
  it("is positive for an upward flick", () => {
    expect(bottomSheetReleaseVelocity([
      { y: 500, time: 0 },
      { y: 450, time: 50 },
      { y: 400, time: 100 },
    ])).toBe(1);
  });

  it("ignores samples older than the velocity window", () => {
    expect(bottomSheetReleaseVelocity([
      { y: 900, time: 0 },
      { y: 400, time: 300 },
      { y: 420, time: 340 },
    ])).toBeCloseTo(-0.5);
  });

  it("is zero for a single sample or no movement in time", () => {
    expect(bottomSheetReleaseVelocity([])).toBe(0);
    expect(bottomSheetReleaseVelocity([{ y: 10, time: 5 }])).toBe(0);
  });
});

describe("settleBottomSheet", () => {
  const heights = bottomSheetHeights(800, 60, 180);

  it("rests on the nearest detent for a slow release", () => {
    expect(settleBottomSheet(200, 0, heights, true)).toBe("peek");
    expect(settleBottomSheet(420, 0, heights, true)).toBe("half");
    expect(settleBottomSheet(700, 0, heights, true)).toBe("full");
  });

  it("carries a flick toward the next detent", () => {
    expect(settleBottomSheet(220, 1.5, heights, true)).toBe("half");
    expect(settleBottomSheet(600, 2, heights, true)).toBe("full");
    expect(settleBottomSheet(600, -2, heights, true)).toBe("peek");
  });

  it("closes a sheet released well below the peek", () => {
    expect(settleBottomSheet(60, 0, heights, true)).toBe("close");
    expect(settleBottomSheet(170, -1, heights, true)).toBe("close");
  });

  it("returns to the peek instead of closing when closing is not allowed", () => {
    expect(settleBottomSheet(20, -1, heights, false)).toBe("peek");
  });
});

describe("higherBottomSheetDetent", () => {
  it("picks the more open detent", () => {
    expect(higherBottomSheetDetent("peek", "full")).toBe("full");
    expect(higherBottomSheetDetent("half", "peek")).toBe("half");
  });
});
