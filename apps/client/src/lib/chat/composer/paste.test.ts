import { describe, expect, it } from "vitest";
import {
  LARGE_PASTE_ATTACHMENT_BYTES,
  MAX_TEXT_SNIPPET_BYTES,
  classifyPastedText,
  pastedTextLineCount,
  utf8ByteLength,
} from "./paste";

describe("classifyPastedText", () => {
  it("keeps text one byte under the threshold inline and attaches text at the threshold", () => {
    expect(LARGE_PASTE_ATTACHMENT_BYTES).toBe(32768);
    expect(classifyPastedText("a".repeat(32767))).toBe("inline");
    expect(classifyPastedText("a".repeat(32768))).toBe("attachment");
  });

  it("measures UTF-8 bytes rather than UTF-16 code units", () => {
    const accented = "\u{E9}".repeat(16384);
    expect(accented.length).toBe(16384);
    expect(utf8ByteLength(accented)).toBe(32768);
    expect(classifyPastedText(accented)).toBe("attachment");
  });

  it("attaches text at the snippet bound and refuses text over it", () => {
    expect(classifyPastedText("a".repeat(MAX_TEXT_SNIPPET_BYTES))).toBe("attachment");
    expect(classifyPastedText("a".repeat(MAX_TEXT_SNIPPET_BYTES + 1))).toBe("too_large");
  });
});

describe("pastedTextLineCount", () => {
  it("counts lines without counting a final newline as a line", () => {
    expect(pastedTextLineCount("one")).toBe(1);
    expect(pastedTextLineCount("one\ntwo\n")).toBe(2);
    expect(pastedTextLineCount("one\n\nthree")).toBe(3);
    expect(pastedTextLineCount("")).toBe(1);
  });
});
