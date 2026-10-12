import { describe, expect, it } from "vitest";
import {
  notesAdjacentRenderedBlockId,
  notesBoundaryRenderedBlockId,
  notesCollapsedNavigationSelection,
} from "./navigation";

describe("notes block navigation", () => {
  it("finds adjacent rendered blocks", () => {
    expect(notesAdjacentRenderedBlockId(["a", "b", "c"], "b", "previous")).toBe("a");
    expect(notesAdjacentRenderedBlockId(["a", "b", "c"], "b", "next")).toBe("c");
    expect(notesAdjacentRenderedBlockId(["a", "b", "c"], "a", "previous")).toBeNull();
    expect(notesAdjacentRenderedBlockId(["a", "b", "c"], "missing", "next")).toBeNull();
  });

  it("finds document boundary blocks", () => {
    expect(notesBoundaryRenderedBlockId(["a", "b"], "first")).toBe("a");
    expect(notesBoundaryRenderedBlockId(["a", "b"], "last")).toBe("b");
    expect(notesBoundaryRenderedBlockId([], "first")).toBeNull();
  });

  it("builds collapsed selections for target blocks", () => {
    expect(notesCollapsedNavigationSelection(7)).toEqual({ start: 7, end: 7 });
  });
});
