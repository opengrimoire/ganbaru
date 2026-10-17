import { describe, expect, it } from "vitest";
import { musicVirtualWindow, revealMusicVirtualIndex } from "./virtual-window";

describe("music virtual window", () => {
  it("retains only a bounded dense window with a full scroll range", () => {
    const window = musicVirtualWindow({
      count: 10_000,
      scrollTop: 200_000,
      viewportHeight: 500,
      rowHeight: 56,
      overscan: 6,
    });
    expect(window.endIndex - window.startIndex).toBeLessThanOrEqual(22);
    expect(window.topSpacer + (window.endIndex - window.startIndex) * 56 + window.bottomSpacer)
      .toBe(window.totalHeight);
  });

  it("reveals active rows only when they leave the viewport", () => {
    expect(revealMusicVirtualIndex(3, 100, 200, 50)).toBe(100);
    expect(revealMusicVirtualIndex(1, 100, 200, 50)).toBe(50);
    expect(revealMusicVirtualIndex(10, 100, 200, 50)).toBe(350);
  });
});
