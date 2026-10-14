import { describe, expect, it } from "vitest";
import {
  sortableDragBounds,
  sortableDragLanding,
  sortableDragShift,
  sortableDragTarget,
  type SortableSlot,
} from "./sortable-drag";

/** Three items with an 8px gap: a short row, a tall block, and another short row. */
const UNEVEN: SortableSlot[] = [
  { top: 0, height: 28 },
  { top: 36, height: 100 },
  { top: 144, height: 28 },
];

const EVEN: SortableSlot[] = [
  { top: 0, height: 28 },
  { top: 36, height: 28 },
  { top: 72, height: 28 },
  { top: 108, height: 28 },
];

describe("sortable drag", () => {
  it("keeps the dragged item between the first and last slots", () => {
    expect(sortableDragBounds(EVEN, 1)).toEqual({ min: -36, max: 72 });
    expect(sortableDragBounds(UNEVEN, 1)).toEqual({ min: -36, max: 36 });
    expect(sortableDragBounds(UNEVEN, 0)).toEqual({ min: 0, max: 144 });
    expect(sortableDragBounds([], 0)).toEqual({ min: 0, max: 0 });
  });

  it("passes an item once the leading edge crosses its center", () => {
    expect(sortableDragTarget(EVEN, 0, 0)).toBe(0);
    expect(sortableDragTarget(EVEN, 0, 22)).toBe(0);
    expect(sortableDragTarget(EVEN, 0, 23)).toBe(1);
    expect(sortableDragTarget(EVEN, 0, 108)).toBe(3);
    expect(sortableDragTarget(EVEN, 3, -22)).toBe(3);
    expect(sortableDragTarget(EVEN, 3, -23)).toBe(2);
    expect(sortableDragTarget(EVEN, 3, -500)).toBe(0);
  });

  it("measures crossings against each item's own height", () => {
    // The short first row reaches the tall block's center only after traveling past half of it.
    expect(sortableDragTarget(UNEVEN, 0, 57)).toBe(0);
    expect(sortableDragTarget(UNEVEN, 0, 59)).toBe(1);
    // The tall block passes the last row as soon as its bottom edge crosses that row's center.
    expect(sortableDragTarget(UNEVEN, 1, 22)).toBe(1);
    expect(sortableDragTarget(UNEVEN, 1, 23)).toBe(2);
    expect(sortableDragTarget(UNEVEN, 1, -21)).toBe(1);
    expect(sortableDragTarget(UNEVEN, 1, -23)).toBe(0);
  });

  it("slides the items between source and target by the dragged item's height and the gap", () => {
    expect([0, 1, 2, 3].map((index) => sortableDragShift(EVEN, index, 0, 2))).toEqual([0, -36, -36, 0]);
    expect([0, 1, 2, 3].map((index) => sortableDragShift(EVEN, index, 3, 1))).toEqual([0, 36, 36, 0]);
    expect([0, 1, 2, 3].map((index) => sortableDragShift(EVEN, index, 1, 1))).toEqual([0, 0, 0, 0]);
    expect([0, 1, 2].map((index) => sortableDragShift(UNEVEN, index, 1, 2))).toEqual([0, 0, -108]);
    expect([0, 1, 2].map((index) => sortableDragShift(UNEVEN, index, 0, 2))).toEqual([0, -36, -36]);
  });

  it("lands the dragged item where its new slot opens", () => {
    expect(sortableDragLanding(EVEN, 0, 2)).toBe(72);
    expect(sortableDragLanding(EVEN, 3, 1)).toBe(-72);
    expect(sortableDragLanding(EVEN, 2, 2)).toBe(0);
    // The tall block lands flush with the bottom of the list when moved past the last row.
    expect(sortableDragLanding(UNEVEN, 1, 2)).toBe(36);
    expect(sortableDragLanding(UNEVEN, 2, 0)).toBe(-144);
    expect(sortableDragLanding(UNEVEN, 0, 2)).toBe(144);
  });
});
