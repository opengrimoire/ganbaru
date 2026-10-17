import { describe, expect, it } from "vitest";
import { composerScrollTopForCaret } from "./scroll";

describe("Chat composer scrolling", () => {
  it("reveals the complete caret line below the viewport", () => {
    expect(composerScrollTopForCaret(22, 176, 22, 264, 132)).toBe(66);
  });

  it("reveals the complete caret line above the viewport without snapping", () => {
    expect(composerScrollTopForCaret(88, 43, 22, 264, 132)).toBe(43);
  });

  it("reaches the true end of an uneven scroll range", () => {
    expect(composerScrollTopForCaret(0, 208, 22, 230, 132)).toBe(98);
  });
});
