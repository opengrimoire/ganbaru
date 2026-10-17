import { describe, expect, it } from "vitest";
import {
  notesNavigationDragCard,
  notesNavigationDragCardOrigin,
  notesNavigationSubtreeEnd,
} from "./drag-motion";

describe("Notes navigation drag motion", () => {
  const row = { left: 20, top: 100, width: 240, height: 32 };

  it("starts the card over its row with the pointer holding the grabbed spot", () => {
    const pointer = { x: 150, y: 110 };
    const card = notesNavigationDragCard(row, pointer, 320);
    expect(card).toEqual({ width: 240, height: 32, grab: { x: 130, y: 10 } });
    expect(notesNavigationDragCardOrigin(card, pointer)).toEqual({ x: 20, y: 100 });
    expect(notesNavigationDragCardOrigin(card, { x: 400, y: 50 })).toEqual({ x: 270, y: 40 });
  });

  it("keeps the grab point inside a card narrower than its row", () => {
    const card = notesNavigationDragCard(row, { x: 250, y: 140 }, 180);
    expect(card.width).toBe(180);
    expect(card.grab).toEqual({ x: 180, y: 32 });
  });

  it("clamps a pointer left of or above the row to the card's corner", () => {
    expect(notesNavigationDragCard(row, { x: 0, y: 90 }, 320).grab).toEqual({ x: 0, y: 0 });
  });

  it("finds the rows nested under a folder in a flattened tree", () => {
    const depths = [0, 1, 2, 1, 0, 1];
    expect(notesNavigationSubtreeEnd(depths, 0)).toBe(3);
    expect(notesNavigationSubtreeEnd(depths, 1)).toBe(2);
    expect(notesNavigationSubtreeEnd(depths, 2)).toBe(2);
    expect(notesNavigationSubtreeEnd(depths, 4)).toBe(5);
    expect(notesNavigationSubtreeEnd(depths, 9)).toBe(9);
  });
});
