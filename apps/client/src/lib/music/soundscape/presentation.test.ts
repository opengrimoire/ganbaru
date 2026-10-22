import { describe, expect, it } from "vitest";
import { soundscapeIconStrokeWidth } from "./presentation";

describe("soundscapeIconStrokeWidth", () => {
  it("keeps the compact stroke for list-sized icons", () => {
    expect(soundscapeIconStrokeWidth(13)).toBe(1.5);
    expect(soundscapeIconStrokeWidth(16)).toBe(1.5);
  });

  it("thins large tile icons so their rendered line matches a 16 pixel icon", () => {
    const size = 32;
    const rendered = (stroke: number, pixels: number) => stroke * pixels / 24;
    expect(soundscapeIconStrokeWidth(size)).toBeCloseTo(0.75);
    expect(rendered(soundscapeIconStrokeWidth(size), size)).toBeCloseTo(rendered(1.5, 16));
  });

  it("falls back to the compact stroke for sizes that are not finite", () => {
    expect(soundscapeIconStrokeWidth(Number.NaN)).toBe(1.5);
    expect(soundscapeIconStrokeWidth(Number.POSITIVE_INFINITY)).toBe(1.5);
  });
});
