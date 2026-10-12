import { describe, expect, it } from "vitest";
import {
  notesTextContextMenuPosition,
  notesTextContextMenuSelectionAtPoint,
  notesTextContextSubmenuPosition,
} from "./text-context-menu";

describe("Notes text context menu placement", () => {
  it("keeps a right-click menu inside the viewport", () => {
    expect(notesTextContextMenuPosition(
      { x: 790, y: 590 },
      { width: 800, height: 600 },
    )).toEqual({ left: 552, top: 152, width: 240, maxHeight: 440 });
  });

  it("fits the menu on a narrow viewport", () => {
    expect(notesTextContextMenuPosition(
      { x: 180, y: 220 },
      { width: 240, height: 300 },
    )).toEqual({ left: 8, top: 8, width: 224, maxHeight: 284 });
  });

  it("uses the rendered menu height to stay close to the click", () => {
    expect(notesTextContextMenuPosition(
      { x: 500, y: 500 },
      { width: 800, height: 600 },
      240,
    )).toEqual({ left: 500, top: 352, width: 240, maxHeight: 440 });
  });

  it("opens a submenu to the right when it fits", () => {
    expect(notesTextContextSubmenuPosition(
      { left: 100, right: 292, top: 80, bottom: 112 },
      { width: 168, height: 200 },
      { width: 800, height: 600 },
    )).toEqual({ x: 296, y: 80 });
  });

  it("flips a submenu left and clamps its height near the bottom", () => {
    expect(notesTextContextSubmenuPosition(
      { left: 600, right: 792, top: 550, bottom: 582 },
      { width: 168, height: 200 },
      { width: 800, height: 600 },
    )).toEqual({ x: 428, y: 392 });
  });

  it("preserves selected text only when right-clicking a selected fragment", () => {
    const rects = [
      { left: 100, right: 200, top: 100, bottom: 120 },
      { left: 100, right: 150, top: 125, bottom: 145 },
    ];
    expect(notesTextContextMenuSelectionAtPoint(rects, { x: 120, y: 132 })).toBe(true);
    expect(notesTextContextMenuSelectionAtPoint(rects, { x: 180, y: 132 })).toBe(false);
  });
});
