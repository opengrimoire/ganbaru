import { describe, expect, it } from "vitest";
import { viewportShift } from "./keep-in-viewport";

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
