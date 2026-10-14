import { describe, expect, it } from "vitest";
import { projectViewDragPlacement, projectViewDragRange, projectViewDragShift, projectViewDragTarget } from "./view-drag";

describe("project view drag", () => {
  it("keeps a drag inside the run of views sharing its starred state", () => {
    const starred = [true, true, false, false, false];
    expect(projectViewDragRange(starred, 0)).toEqual({ start: 0, end: 1 });
    expect(projectViewDragRange(starred, 1)).toEqual({ start: 0, end: 1 });
    expect(projectViewDragRange(starred, 3)).toEqual({ start: 2, end: 4 });
    expect(projectViewDragRange([false], 0)).toEqual({ start: 0, end: 0 });
    expect(projectViewDragRange([], 2)).toEqual({ start: 2, end: 2 });
  });

  it("picks the slot under the card center and clamps it to the allowed range", () => {
    const tops = [0, 28, 56, 84];
    const all = { start: 0, end: 3 };
    expect(projectViewDragTarget(tops, all, -40)).toBe(0);
    expect(projectViewDragTarget(tops, all, 27)).toBe(0);
    expect(projectViewDragTarget(tops, all, 28)).toBe(1);
    expect(projectViewDragTarget(tops, all, 70)).toBe(2);
    expect(projectViewDragTarget(tops, all, 500)).toBe(3);
    expect(projectViewDragTarget(tops, { start: 2, end: 3 }, 0)).toBe(2);
    expect(projectViewDragTarget(tops, { start: 0, end: 1 }, 90)).toBe(1);
  });

  it("slides only the rows between the source and the target into their neighbor's slot", () => {
    const tops = [0, 28, 56, 84];
    expect([0, 1, 2, 3].map((index) => projectViewDragShift(tops, index, 0, 2))).toEqual([0, -28, -28, 0]);
    expect([0, 1, 2, 3].map((index) => projectViewDragShift(tops, index, 3, 1))).toEqual([0, 28, 28, 0]);
    expect([0, 1, 2, 3].map((index) => projectViewDragShift(tops, index, 1, 1))).toEqual([0, 0, 0, 0]);
  });

  it("follows uneven slot spacing when sliding rows", () => {
    const tops = [0, 30, 50];
    expect(projectViewDragShift(tops, 1, 0, 2)).toBe(-30);
    expect(projectViewDragShift(tops, 2, 0, 2)).toBe(-20);
  });

  it("places a drop after the target row when moving down and before it when moving up", () => {
    expect(projectViewDragPlacement(0, 2)).toBe("after");
    expect(projectViewDragPlacement(3, 1)).toBe("before");
    expect(projectViewDragPlacement(2, 2)).toBeNull();
  });
});
