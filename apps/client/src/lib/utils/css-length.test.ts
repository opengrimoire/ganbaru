// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";
import { cssLengthToPixels } from "./css-length";

describe("cssLengthToPixels", () => {
  afterEach(() => {
    document.documentElement.style.fontSize = "";
    document.body.replaceChildren();
  });

  it("reads pixels as they are and resolves rem against the root font size", () => {
    document.documentElement.style.fontSize = "20px";
    const context = document.createElement("div");
    document.body.append(context);
    expect(cssLengthToPixels(" 12px ", context, 0)).toBe(12);
    expect(cssLengthToPixels("1.5rem", context, 0)).toBe(30);
  });

  it("resolves em against the context font size", () => {
    const context = document.createElement("div");
    context.style.fontSize = "10px";
    document.body.append(context);
    expect(cssLengthToPixels("3em", context, 0)).toBe(30);
  });

  it("returns the fallback for values that are not plain lengths", () => {
    const context = document.createElement("div");
    expect(cssLengthToPixels("", context, 7)).toBe(7);
    expect(cssLengthToPixels("calc(2 * 1rem)", context, 7)).toBe(7);
  });
});
