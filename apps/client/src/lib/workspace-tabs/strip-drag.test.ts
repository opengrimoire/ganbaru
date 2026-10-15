import { describe, expect, it } from "vitest";
import {
  tabDragLeftBar,
  tabStripDropIndex,
  tabStripInsertIndex,
  tabStripInsertPosition,
  tabStripInsertTarget,
  tabStripMoveTarget,
  tabStripRight,
  tabStripVisualLeft,
  type TabStripSlot,
} from "./strip-drag";

const WIDTH = 100;
const GAP = 2;

function slots(...ids: string[]): TabStripSlot[] {
  return ids.map((id, index) => ({ id, left: 10 + index * (WIDTH + GAP), width: WIDTH }));
}

describe("tab strip drop index", () => {
  const strip = slots("a", "b", "c", "d");

  it("lands in the slot under the dragged tab's center", () => {
    expect(tabStripDropIndex(strip, 60)).toBe(0);
    expect(tabStripDropIndex(strip, 160)).toBe(1);
    expect(tabStripDropIndex(strip, 370)).toBe(3);
  });

  it("swaps with a neighbor only once the dragged tab covers half of it", () => {
    // Slot centers sit at 60 and 162, so the boundary between them is 111.
    expect(tabStripDropIndex(strip, 110)).toBe(0);
    expect(tabStripDropIndex(strip, 112)).toBe(1);
  });

  it("clamps to the ends when dragged past them", () => {
    expect(tabStripDropIndex(strip, -500)).toBe(0);
    expect(tabStripDropIndex(strip, 5_000)).toBe(3);
  });

  it("stays on the only slot of a single tab", () => {
    expect(tabStripDropIndex(slots("a"), 900)).toBe(0);
  });
});

describe("tab strip move target", () => {
  const ids = ["a", "b", "c", "d"];

  it("moves before the tab now in the target slot", () => {
    expect(tabStripMoveTarget(ids, "d", 1)).toEqual({ id: "b", placement: "before" });
    expect(tabStripMoveTarget(ids, "a", 2)).toEqual({ id: "d", placement: "before" });
  });

  it("moves after the last tab for the last slot", () => {
    expect(tabStripMoveTarget(ids, "a", 3)).toEqual({ id: "d", placement: "after" });
    expect(tabStripMoveTarget(ids, "b", 9)).toEqual({ id: "d", placement: "after" });
  });

  it("does nothing when the tab is already in the slot", () => {
    expect(tabStripMoveTarget(ids, "c", 2)).toBeNull();
    expect(tabStripMoveTarget(ids, "a", -3)).toBeNull();
  });

  it("does nothing for a tab outside the strip or a strip of one", () => {
    expect(tabStripMoveTarget(ids, "z", 0)).toBeNull();
    expect(tabStripMoveTarget(["a"], "a", 1)).toBeNull();
  });
});

describe("tab drag leaving the bar", () => {
  const bar = { top: 0, bottom: 36 };

  it("keeps reordering while the pointer stays near the bar", () => {
    expect(tabDragLeftBar(18, bar, 14)).toBe(false);
    expect(tabDragLeftBar(50, bar, 14)).toBe(false);
  });

  it("pulls the tab off once the pointer is past the margin", () => {
    expect(tabDragLeftBar(51, bar, 14)).toBe(true);
    expect(tabDragLeftBar(400, bar, 14)).toBe(true);
    expect(tabDragLeftBar(-15, bar, 14)).toBe(true);
  });
});

describe("dragged tab position", () => {
  it("follows the pointer from where the tab was grabbed", () => {
    expect(tabStripVisualLeft(150, 40, 100, 400)).toBe(110);
  });

  it("stops at either end of the strip", () => {
    expect(tabStripVisualLeft(10, 40, 100, 400)).toBe(0);
    expect(tabStripVisualLeft(900, 40, 100, 400)).toBe(300);
  });

  it("stays at the start when the strip is narrower than the tab", () => {
    expect(tabStripVisualLeft(900, 40, 100, 60)).toBe(0);
  });

  it("measures the strip by its rightmost tab", () => {
    expect(tabStripRight(slots("a", "b", "c"))).toBe(10 + 2 * (WIDTH + GAP) + WIDTH);
    expect(tabStripRight([])).toBe(0);
  });
});

describe("tab entering the strip from another window", () => {
  const strip = slots("a", "b", "c");

  it("takes the gap the pointer is in", () => {
    expect(tabStripInsertIndex(strip, 0)).toBe(0);
    expect(tabStripInsertIndex(strip, 10 + WIDTH / 2 + 1)).toBe(1);
    expect(tabStripInsertIndex(strip, 10 + WIDTH + GAP + 10)).toBe(1);
    expect(tabStripInsertIndex(strip, 1000)).toBe(3);
    expect(tabStripInsertIndex([], 50)).toBe(0);
  });

  it("lands before the tab at its index, or after the last one past the end", () => {
    const ids = ["a", "b", "c"];
    expect(tabStripInsertTarget(ids, 0)).toEqual({ id: "a", placement: "before" });
    expect(tabStripInsertTarget(ids, 2)).toEqual({ id: "c", placement: "before" });
    expect(tabStripInsertTarget(ids, 3)).toEqual({ id: "c", placement: "after" });
    expect(tabStripInsertTarget(ids, 9)).toEqual({ id: "c", placement: "after" });
    expect(tabStripInsertTarget(ids, -1)).toEqual({ id: "a", placement: "before" });
    expect(tabStripInsertTarget([], 0)).toBeNull();
  });

  it("maps a landing neighbor back to the index it names", () => {
    const ids = ["a", "b", "c"];
    for (let index = 0; index <= ids.length; index += 1) {
      expect(tabStripInsertPosition(ids, tabStripInsertTarget(ids, index))).toBe(index);
    }
    expect(tabStripInsertPosition(ids, null)).toBe(3);
    expect(tabStripInsertPosition(ids, { id: "gone", placement: "before" })).toBe(3);
  });
});
