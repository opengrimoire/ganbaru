import { describe, expect, it } from "vitest";
import { safeViewportRectFrom, viewportShift } from "./keep-in-viewport";

const phone = { left: 0, top: 0, right: 360, bottom: 640 };

describe("viewportShift", () => {
  it("leaves a panel that already fits where CSS placed it", () => {
    expect(viewportShift({ left: 20, top: 100, right: 300, bottom: 400 }, phone)).toEqual({ x: 0, y: 0 });
  });

  it("slides a panel that runs past the right edge back by the overflow plus the margin", () => {
    expect(viewportShift({ left: 170, top: 100, right: 450, bottom: 400 }, phone)).toEqual({ x: -98, y: 0 });
  });

  it("slides a panel above the top edge down and one past the left edge right", () => {
    expect(viewportShift({ left: -30, top: -50, right: 250, bottom: 200 }, phone)).toEqual({ x: 38, y: 58 });
  });

  it("aligns a panel wider than the viewport to the start edge", () => {
    expect(viewportShift({ left: 40, top: 0, right: 440, bottom: 100 }, { ...phone, right: 300 })).toEqual({ x: -32, y: 8 });
  });

  it("measures against an offset visual viewport", () => {
    const zoomed = { left: 100, top: 50, right: 300, bottom: 400 };
    expect(viewportShift({ left: 150, top: 60, right: 320, bottom: 200 }, zoomed)).toEqual({ x: -28, y: 0 });
  });
});

describe("safeViewportRectFrom", () => {
  const noInsets = { top: 0, right: 0, bottom: 0, left: 0 };
  const phoneViewport = {
    offsetLeft: 0,
    offsetTop: 0,
    width: 392,
    height: 851,
    layoutWidth: 392,
    layoutHeight: 851,
    insets: { top: 36, right: 0, bottom: 47, left: 0 },
    visibleBottom: 851,
  };

  it("leaves out the status bar and the system navigation", () => {
    expect(safeViewportRectFrom(phoneViewport)).toEqual({ left: 0, top: 36, right: 392, bottom: 804, width: 392, height: 768 });
  });

  it("ends above an open soft keyboard", () => {
    expect(safeViewportRectFrom({ ...phoneViewport, visibleBottom: 520 }).bottom).toBe(520);
  });

  it("leaves out a side cutout in landscape", () => {
    const landscape = { ...phoneViewport, width: 851, height: 392, layoutWidth: 851, layoutHeight: 392 };
    const rect = safeViewportRectFrom({ ...landscape, insets: { top: 0, right: 47, bottom: 0, left: 36 }, visibleBottom: 392 });
    expect(rect).toEqual({ left: 36, top: 0, right: 804, bottom: 392, width: 768, height: 392 });
  });

  it("keeps the whole visual viewport where nothing covers it", () => {
    const desktop = { ...phoneViewport, width: 1200, height: 800, layoutWidth: 1200, layoutHeight: 800, insets: noInsets, visibleBottom: 800 };
    expect(safeViewportRectFrom(desktop)).toEqual({ left: 0, top: 0, right: 1200, bottom: 800, width: 1200, height: 800 });
  });

  it("stays inside a zoomed visual viewport that already clears the bars", () => {
    const zoomed = { ...phoneViewport, offsetLeft: 50, offsetTop: 100, width: 200, height: 400, visibleBottom: 500 };
    expect(safeViewportRectFrom(zoomed)).toEqual({ left: 50, top: 100, right: 250, bottom: 500, width: 200, height: 400 });
  });

  it("never returns a negative size", () => {
    const tiny = { ...phoneViewport, height: 60, layoutHeight: 60, visibleBottom: 60 };
    expect(safeViewportRectFrom(tiny)).toMatchObject({ top: 36, bottom: 36, height: 0 });
  });
});
