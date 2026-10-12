import { describe, expect, it } from "vitest";
import {
  beginEventPanelDrag,
  endEventPanelDrag,
  moveEventPanelDrag,
} from "./geometry-controller.svelte";
import { clampFloatingLeft, clampFloatingTop } from "./geometry";

describe("EventPanel pointer drag", () => {
  it("keeps the existing offset and stops movement after pointer cleanup", () => {
    const started = beginEventPanelDrag({ x: 50, y: 40 }, { x: 10, y: 5 });
    const moved = moveEventPanelDrag(started, { x: 80, y: 70 });
    expect(moved.offset).toEqual({ x: 40, y: 35 });

    const ended = endEventPanelDrag(moved);
    expect(ended.active).toBe(false);
    expect(moveEventPanelDrag(ended, { x: 100, y: 100 })).toBe(ended);
  });

  it("clamps a resized panel back inside the viewport margins", () => {
    expect(clampFloatingLeft(500, 320, 280, 8)).toBe(32);
    expect(clampFloatingLeft(-20, 320, 280, 8)).toBe(8);
    expect(clampFloatingTop(500, 240, 180, 40, 8)).toBe(52);
    expect(clampFloatingTop(-20, 240, 180, 40, 8)).toBe(40);
  });
});
