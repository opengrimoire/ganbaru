import { describe, expect, it } from "vitest";
import { contrastRatio } from "$lib/color/math";
import { mostContrastingQuickNoteText } from "./colors";

describe("mostContrastingQuickNoteText", () => {
  it("uses black on light and middle-luminance backgrounds", () => {
    expect(mostContrastingQuickNoteText("#ffffff")).toBe("#000000");
    expect(mostContrastingQuickNoteText("#808080")).toBe("#000000");
  });

  it("uses white on dark backgrounds", () => {
    expect(mostContrastingQuickNoteText("#000000")).toBe("#ffffff");
    expect(mostContrastingQuickNoteText("#303030")).toBe("#ffffff");
  });

  it("always returns the higher-contrast endpoint", () => {
    for (const background of ["#f2d5cf", "#8c6f63", "#3b5360", "#18231e"]) {
      const foreground = mostContrastingQuickNoteText(background);
      const alternative = foreground === "#000000" ? "#ffffff" : "#000000";
      expect(contrastRatio(background, foreground)).toBeGreaterThanOrEqual(
        contrastRatio(background, alternative),
      );
    }
  });
});
