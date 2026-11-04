import { describe, expect, it } from "vitest";
import { growThenFadeStyle, type GrowThenFadeBox } from "./grow-then-fade";

const box: GrowThenFadeBox = { height: 36, paddingTop: 4, paddingBottom: 2, marginTop: 8, marginBottom: 0 };
const linear = (t: number) => t;

function parsed(style: string): Record<string, number> {
  return Object.fromEntries(style.split("; ").filter((part) => part.includes(":") && !part.startsWith("overflow")).map((part) => {
    const [name, value] = part.split(": ");
    return [name, parseFloat(value)];
  }));
}

describe("growThenFadeStyle", () => {
  it("starts closed and invisible and ends at the full box, fully visible", () => {
    expect(parsed(growThenFadeStyle(0, box, linear))).toEqual({ opacity: 0, height: 0, "padding-top": 0, "padding-bottom": 0, "margin-top": 0, "margin-bottom": 0 });
    expect(parsed(growThenFadeStyle(1, box, linear))).toEqual({ opacity: 1, height: 36, "padding-top": 4, "padding-bottom": 2, "margin-top": 8, "margin-bottom": 0 });
  });

  it("keeps the content hidden while most of the height grows", () => {
    const early = parsed(growThenFadeStyle(0.3, box, linear));
    expect(early.opacity).toBe(0);
    expect(early.height).toBeGreaterThan(0);
    expect(early.height).toBeLessThan(box.height);
  });

  it("fades the content in only once the height has nearly settled, and holds the full height while it does", () => {
    const fading = parsed(growThenFadeStyle(0.8, box, linear));
    expect(fading.height).toBe(box.height);
    expect(fading.opacity).toBeGreaterThan(0);
    expect(fading.opacity).toBeLessThan(1);
  });

  it("clips the content so it never spills while the height changes", () => {
    expect(growThenFadeStyle(0.5, box, linear)).toContain("overflow: hidden");
  });
});
