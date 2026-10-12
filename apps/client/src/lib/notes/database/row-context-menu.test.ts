import { describe, expect, it } from "vitest";
import {
  notesRowContextMenuGeometry,
  notesRowContextMenuStyle,
} from "./row-context-menu";

describe("Notes row context menu geometry", () => {
  it("uses the pointer position when the menu fits", () => {
    expect(notesRowContextMenuGeometry({
      clientX: 120,
      clientY: 80,
      viewportWidth: 800,
      viewportHeight: 600,
    })).toEqual({ left: 120, top: 80, width: 240, maxHeight: 330 });
  });

  it("clamps the menu to the right and bottom edges", () => {
    expect(notesRowContextMenuGeometry({
      clientX: 790,
      clientY: 590,
      viewportWidth: 800,
      viewportHeight: 600,
    })).toEqual({ left: 552, top: 262, width: 240, maxHeight: 330 });
  });

  it("shrinks within a compact viewport", () => {
    const geometry = notesRowContextMenuGeometry({
      clientX: 200,
      clientY: 120,
      viewportWidth: 180,
      viewportHeight: 180,
    });

    expect(geometry).toEqual({ left: 8, top: 8, width: 164, maxHeight: 164 });
    expect(notesRowContextMenuStyle(geometry))
      .toBe("left: 8px; top: 8px; width: 164px; max-height: 164px");
  });
});
