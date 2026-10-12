import { describe, expect, it } from "vitest";
import { notesFocusedBlockEstimatedOffset } from "./virtualizer.svelte";

describe("Notes block virtual focus recovery", () => {
  it("does not estimate a scroll jump for pointer focus during a render update", () => {
    const items = [{ id: "first", estimatedHeight: 36 }, { id: "target", estimatedHeight: 48 }];
    expect(notesFocusedBlockEstimatedOffset(items, new Map(), "target", false, true)).toBeNull();
    expect(notesFocusedBlockEstimatedOffset(items, new Map(), "target", false)).toBe(36);
  });

  it("uses measured predecessors before retained-height estimates", () => {
    const items = [
      { id: "first", estimatedHeight: 36 },
      { id: "second", estimatedHeight: 48 },
      { id: "target", estimatedHeight: 36 },
    ];
    expect(notesFocusedBlockEstimatedOffset(items, new Map([["first", 52]]), "target", false))
      .toBe(100);
    expect(notesFocusedBlockEstimatedOffset(items, new Map(), "missing", false)).toBeNull();
  });

  it("leaves scrolling to the visible editor for a rendered focus target", () => {
    const items = [
      { id: "first", estimatedHeight: 36 },
      { id: "target", estimatedHeight: 48 },
    ];
    expect(notesFocusedBlockEstimatedOffset(items, new Map(), "target", true)).toBeNull();
    expect(notesFocusedBlockEstimatedOffset(items, new Map(), "target", false)).toBe(36);
  });
});
