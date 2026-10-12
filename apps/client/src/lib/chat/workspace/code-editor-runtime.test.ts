import { describe, expect, it } from "vitest";
import { chatLanguageDescription } from "./code-editor-runtime";

describe("chatLanguageDescription", () => {
  it.each([
    ["src/main.py", "Python"],
    ["src/component.tsx", "TSX"],
    ["src/component.svelte", "Svelte"],
    ["containers/Dockerfile", "Dockerfile"],
    ["styles/theme.scss", "SCSS"],
  ])("matches %s by its basename and extension", (relativePath, expectedName) => {
    expect(chatLanguageDescription(relativePath)?.name).toBe(expectedName);
  });

  it("supports Windows separators in provider paths", () => {
    expect(chatLanguageDescription("src\\component.svelte")?.name).toBe("Svelte");
  });

  it("falls back to plain text for an unknown extension", () => {
    expect(chatLanguageDescription("data/example.unknown-language")).toBeNull();
  });
});
