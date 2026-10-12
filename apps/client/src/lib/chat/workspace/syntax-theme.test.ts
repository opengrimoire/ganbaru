import { describe, expect, it } from "vitest";
import { contrastRatio } from "$lib/color/math";
import { darkTheme, lightTheme } from "$lib/themes";
import { chatSyntaxStyle, deriveChatSyntaxPalette } from "./syntax-theme";

describe("deriveChatSyntaxPalette", () => {
  it.each([lightTheme, darkTheme])("keeps every syntax role readable for $id", (theme) => {
    const palette = deriveChatSyntaxPalette(theme);
    const roles = Object.entries(palette).filter(([name]) => name !== "background");
    for (const [name, color] of roles) {
      expect(contrastRatio(palette.background, color), name).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("maps the same semantic colors into CodeMirror and Pierre variables", () => {
    const palette = deriveChatSyntaxPalette(lightTheme);
    const style = chatSyntaxStyle(lightTheme);
    expect(style).toContain(`--chat-syntax-keyword:${palette.keyword}`);
    expect(style).toContain(`--diffs-token-keyword:${palette.keyword}`);
    expect(style).toContain(`--chat-syntax-string:${palette.string}`);
    expect(style).toContain(`--diffs-token-string:${palette.string}`);
  });
});
