import { describe, expect, it } from "vitest";
import { maskCode } from "./masked-code";

describe("maskCode", () => {
  it("keeps the scheme and shows four characters at each end of the code", () => {
    expect(maskCode("ganbaru-anchor:AbCdEfGhIjKlMnOp")).toBe("ganbaru-anchor:AbCd••••MnOp");
  });

  it("masks a code without a scheme from its first character", () => {
    expect(maskCode("ABCD-EFGH-IJKL-MNOP")).toBe("ABCD••••MNOP");
  });

  it("uses the same mark for any length so the mask hides how long the code is", () => {
    expect(maskCode("x".repeat(12))).toBe(maskCode("x".repeat(400)));
  });

  it("returns a code unchanged when masking would hide fewer characters than it shows", () => {
    expect(maskCode("ganbaru-anchor:abcdefghijk")).toBe("ganbaru-anchor:abcdefghijk");
    expect(maskCode("")).toBe("");
  });

  it("does not treat an uppercase or mid-code colon as a scheme", () => {
    expect(maskCode("ABCD:EFGHIJKLMNOP")).toBe("ABCD••••MNOP");
    expect(maskCode("abcd efgh:ijklmnop")).toBe("abcd••••mnop");
  });

  it("counts characters rather than UTF-16 units at the ends", () => {
    expect(maskCode("😀😀😀😀middle😀😀😀😀")).toBe("😀😀😀😀••••😀😀😀😀");
  });
});
