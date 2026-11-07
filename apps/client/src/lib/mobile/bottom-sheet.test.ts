import { describe, expect, it } from "vitest";
import {
  bottomSheetDragHeight,
  bottomSheetHeights,
  bottomSheetReleaseVelocity,
  higherBottomSheetDetent,
  revealScrollDelta,
  settleBottomSheet,
} from "./bottom-sheet";

describe("bottomSheetHeights", () => {
  it("keeps the header, summary, overview, and full heights in order", () => {
    expect(bottomSheetHeights(800, 60, 180, 480.4)).toEqual({ dip: 60, peek: 180, half: 480, full: 800 });
  });

  it("raises half to the peek when the overview ends above the summary", () => {
    const heights = bottomSheetHeights(800, 60, 220, 150);
    expect(heights.half).toBe(220);
    expect(heights.peek).toBe(220);
  });

  it("caps half at the full height when the overview is taller than the sheet", () => {
    expect(bottomSheetHeights(400, 60, 180, 520).half).toBe(400);
  });

  it("clamps every detent inside a sheet shrunk by the keyboard", () => {
    expect(bottomSheetHeights(120, 60, 180, 480)).toEqual({ dip: 60, peek: 120, half: 120, full: 120 });
  });

  it("never lets the peek fall below the header", () => {
    expect(bottomSheetHeights(800, 60, 20, 480).peek).toBe(60);
  });

  it("returns zero heights before the sheet is measured", () => {
    expect(bottomSheetHeights(0, 60, 180, 480)).toEqual({ dip: 0, peek: 0, half: 0, full: 0 });
  });
});

describe("bottomSheetDragHeight", () => {
  const heights = bottomSheetHeights(800, 60, 180, 440);

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
  const heights = bottomSheetHeights(800, 60, 180, 440);

  it("rests on the nearest detent for a slow release", () => {
    expect(settleBottomSheet(200, 0, heights, "close")).toBe("peek");
    expect(settleBottomSheet(420, 0, heights, "close")).toBe("half");
    expect(settleBottomSheet(700, 0, heights, "close")).toBe("full");
  });

  it("carries a flick toward the next detent", () => {
    expect(settleBottomSheet(220, 1.5, heights, "close")).toBe("half");
    expect(settleBottomSheet(600, 2, heights, "close")).toBe("full");
    expect(settleBottomSheet(600, -2, heights, "close")).toBe("peek");
  });

  it("closes a sheet released well below the peek", () => {
    expect(settleBottomSheet(60, 0, heights, "close")).toBe("close");
    expect(settleBottomSheet(170, -1, heights, "close")).toBe("close");
  });

  it("collapses instead of closing a sheet that lowers to its summary strip", () => {
    expect(settleBottomSheet(60, 0, heights, "collapse")).toBe("collapse");
    expect(settleBottomSheet(170, -1, heights, "collapse")).toBe("collapse");
    expect(settleBottomSheet(200, 0, heights, "collapse")).toBe("peek");
  });

  it("returns to the peek instead of closing when closing is not allowed", () => {
    expect(settleBottomSheet(20, -1, heights, "peek")).toBe("peek");
  });
});

describe("higherBottomSheetDetent", () => {
  it("picks the more open detent", () => {
    expect(higherBottomSheetDetent("peek", "full")).toBe("full");
    expect(higherBottomSheetDetent("half", "peek")).toBe("half");
  });
});

describe("revealScrollDelta", () => {
  const area = { top: 100, bottom: 500 };

  it("leaves a target that already fits", () => {
    expect(revealScrollDelta({ top: 200, bottom: 300 }, area, 16)).toBe(0);
  });

  it("scrolls a target hidden under the sheet up above it", () => {
    expect(revealScrollDelta({ top: 520, bottom: 600 }, area, 16)).toBe(116);
  });

  it("scrolls a target above the area back down", () => {
    expect(revealScrollDelta({ top: 40, bottom: 140 }, area, 16)).toBe(-76);
  });

  it("keeps the top of a target taller than the area visible", () => {
    expect(revealScrollDelta({ top: 300, bottom: 1000 }, area, 16)).toBe(184);
  });

  it("does nothing when the sheet leaves no room", () => {
    expect(revealScrollDelta({ top: 300, bottom: 400 }, { top: 100, bottom: 120 }, 16)).toBe(0);
  });
});
