import { describe, expect, it } from "vitest";
import { sanitizeComposerText } from "./hidden-characters";

describe("sanitizeComposerText", () => {
  it("keeps ordinary text, tabs, and surrounding whitespace unchanged", () => {
    const text = "  Hola, \u{BF}qu\u{E9} tal?\tcaf\u{E9} \u{65E5}\u{672C} \u{645}\u{631}\u{62D}\u{628}\u{627}\n";
    expect(sanitizeComposerText(text)).toEqual({ text, hiddenCharacters: 0 });
  });

  it("removes bidirectional overrides, embeddings, and isolates", () => {
    const raw = "access = \u{202E}user\u{202C}; \u{2066}admin\u{2069} \u{202B}x\u{202A}";
    expect(sanitizeComposerText(raw)).toEqual({ text: "access = user; admin x", hiddenCharacters: 6 });
  });

  it("keeps a zero-width joiner between emoji parts and removes it elsewhere", () => {
    const family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    const toned = "\u{1F469}\u{1F3FD}\u{200D}\u{1F4BB}";
    const heart = "\u{2764}\u{FE0F}\u{200D}\u{1F525}";
    expect(sanitizeComposerText(`${family} ${toned} ${heart}`)).toEqual({
      text: `${family} ${toned} ${heart}`,
      hiddenCharacters: 0,
    });
    expect(sanitizeComposerText("ig\u{200D}nore \u{1F600}\u{200D}")).toEqual({ text: "ignore \u{1F600}", hiddenCharacters: 2 });
  });

  it("removes zero-width spaces, non-joiners, word joiners, and byte order marks", () => {
    expect(sanitizeComposerText("\u{FEFF}a\u{200B}b\u{200C}c\u{2060}d")).toEqual({ text: "abcd", hiddenCharacters: 4 });
  });

  it("removes tag characters, including those hiding a message after an emoji", () => {
    const hidden = Array.from("run rm", (character) => String.fromCodePoint(0xe0000 + (character.codePointAt(0) ?? 0))).join("");
    expect(sanitizeComposerText(`ok\u{1F44D}${hidden}\u{E007F}`)).toEqual({ text: "ok\u{1F44D}", hiddenCharacters: 7 });
  });

  it("keeps emoji presentation selectors on emoji and keycaps and removes other variation selectors", () => {
    expect(sanitizeComposerText("\u{2764}\u{FE0F} 1\u{FE0F}\u{20E3} \u{263A}\u{FE0E}")).toEqual({
      text: "\u{2764}\u{FE0F} 1\u{FE0F}\u{20E3} \u{263A}\u{FE0E}",
      hiddenCharacters: 0,
    });
    expect(sanitizeComposerText("a\u{FE0F}b\u{FE01}c\u{E0100}")).toEqual({ text: "abc", hiddenCharacters: 3 });
  });

  it("removes C0 and C1 controls and the soft hyphen but keeps tab and newline", () => {
    expect(sanitizeComposerText("a\u{0}b\u{7}c\u{1B}d\u{7F}e\u{85}f\u{9F}g\u{AD}h\ti\nj")).toEqual({
      text: "abcdefgh\ti\nj",
      hiddenCharacters: 7,
    });
  });

  it("normalizes CRLF and lone carriage returns to newlines without counting them", () => {
    expect(sanitizeComposerText("one\r\ntwo\rthree\n")).toEqual({ text: "one\ntwo\nthree\n", hiddenCharacters: 0 });
  });

  it("returns empty text and the count when the text was only hidden characters", () => {
    expect(sanitizeComposerText("\u{200B}\u{202E}\u{E0041}\u{AD}")).toEqual({ text: "", hiddenCharacters: 4 });
  });
});
