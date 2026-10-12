import { describe, expect, it } from "vitest";
import { notesEditorScrollTopForTarget } from "./scroll";

describe("Notes editor scrolling", () => {
  it.each([
    { targetTop: 320, targetBottom: 345 },
    { targetTop: 340, targetBottom: 420 },
    { targetTop: -100, targetBottom: 600 },
  ])("keeps pointer focus stationary for visible, clipped, and tall rows: %o", (target) => {
    expect(notesEditorScrollTopForTarget({
      scrollTop: 100, maxScrollTop: 500, viewportTop: 50, viewportBottom: 350,
      ...target, padding: 16, alignment: "nearest", preventScroll: true,
    })).toBe(100);
  });

  it("scrolls down inside the editor viewport to reveal a new row", () => {
    expect(notesEditorScrollTopForTarget({
      scrollTop: 100,
      maxScrollTop: 500,
      viewportTop: 50,
      viewportBottom: 350,
      targetTop: 340,
      targetBottom: 380,
      padding: 16,
      alignment: "nearest",
    })).toBe(146);
  });

  it("keeps an already visible row stable", () => {
    expect(notesEditorScrollTopForTarget({
      scrollTop: 100,
      maxScrollTop: 500,
      viewportTop: 50,
      viewportBottom: 350,
      targetTop: 120,
      targetBottom: 160,
      padding: 16,
      alignment: "nearest",
    })).toBe(100);
  });

  it("centers navigation targets and clamps the result", () => {
    expect(notesEditorScrollTopForTarget({
      scrollTop: 480,
      maxScrollTop: 500,
      viewportTop: 50,
      viewportBottom: 350,
      targetTop: 600,
      targetBottom: 640,
      padding: 16,
      alignment: "center",
    })).toBe(500);
  });
});
