import { describe, expect, it } from "vitest";
import { notesFloatingPanelContentHeight, notesFloatingPanelPlacement } from "./floating-panel";

describe("floating panel content height", () => {
  it("includes borders when fitting a panel to its content", () => {
    expect(notesFloatingPanelContentHeight({ scrollHeight: 120, clientHeight: 120, offsetHeight: 122 })).toBe(122);
  });

  it("measures full content after a small viewport has constrained its current height", () => {
    expect(notesFloatingPanelContentHeight({ scrollHeight: 210, clientHeight: 98, offsetHeight: 100 })).toBe(212);
  });
});

describe("notesFloatingPanelPlacement", () => {
  it("opens from the title action and keeps its full size when space allows", () => {
    expect(notesFloatingPanelPlacement(
      { left: 240, right: 320, top: 120, bottom: 150 },
      { width: 1100, height: 800 },
      { width: 360, height: 400, align: "start" },
    )).toEqual({ left: 240, top: 156, width: 360, maxHeight: 400 });
  });

  it("moves above the action when the lower viewport has too little room", () => {
    expect(notesFloatingPanelPlacement(
      { left: 350, right: 440, top: 570, bottom: 600 },
      { width: 900, height: 650 },
      { width: 400, height: 480, align: "end" },
    )).toEqual({ left: 40, top: 84, width: 400, maxHeight: 480 });
  });

  it("fits a narrow viewport without losing the trigger alignment", () => {
    expect(notesFloatingPanelPlacement(
      { left: 245, right: 310, top: 100, bottom: 132 },
      { width: 320, height: 500 },
      { width: 400, height: 480, align: "end" },
    )).toEqual({ left: 8, top: 138, width: 304, maxHeight: 354 });
  });

  it("keeps the panel visible when the title action scrolls out of view", () => {
    expect(notesFloatingPanelPlacement(
      { left: 120, right: 180, top: -40, bottom: -10 },
      { width: 700, height: 500 },
      { width: 360, height: 400, align: "start" },
    )).toEqual({ left: 120, top: 8, width: 360, maxHeight: 400 });
  });
});
