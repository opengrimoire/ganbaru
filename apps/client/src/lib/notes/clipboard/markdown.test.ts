// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { marked } from "marked";
import { escapeNotesMarkdown, notesMarkdownFence, notesRichTextMarkdown } from "./markdown";
import { createTextRichText, createLinkedTextRichText } from "$lib/notes/rich-text/core";

/** Read Markdown as an independent consumer to verify its semantics. */
function rendered(source: string): DocumentFragment {
  const template = document.createElement("template");
  template.innerHTML = marked.parse(source, { async: false });
  return template.content;
}

describe("Notes Markdown clipboard serialization", () => {
  it.each(["# Literal", "---", "===", "1. Literal", "- Literal", "*literal* [not a link]", "<tag> &amp; & text", "a`b`c", "a_b_c"])("preserves literal syntax: %s", (text) => {
    expect(rendered(escapeNotesMarkdown(text)).textContent?.trim()).toBe(text);
  });

  it("serializes formatting and links as Markdown", () => {
    const bold = createTextRichText("bold");
    bold.annotations.bold = true;
    const emphasis = createTextRichText(" italic");
    emphasis.annotations.italic = true;
    const link = createLinkedTextRichText(" link", "https://example.com/a_(b)");
    const source = notesRichTextMarkdown([bold, emphasis, link]);
    expect(source).toBe("**bold** *italic* [link](<https://example.com/a_(b)>)");
    const html = rendered(source);
    expect(html.querySelector("strong")?.textContent).toBe("bold");
    expect(html.querySelector("em")?.textContent).toBe("italic");
    expect(html.querySelector("a")?.getAttribute("href")).toBe("https://example.com/a_(b)");
  });

  it("chooses safe fences for inline and block code containing backticks", () => {
    const code = createTextRichText("`value```");
    code.annotations.code = true;
    expect(rendered(notesRichTextMarkdown([code])).querySelector("code")?.textContent).toBe("`value```");
    expect(notesMarkdownFence("``` and ````")).toBe("`````");
  });

  it("does not introduce delimiters between adjacent runs with the same Markdown style", () => {
    const left = createTextRichText("one");
    const right = createTextRichText("two");
    left.annotations.bold = true;
    right.annotations.bold = true;
    right.annotations.color = "red";
    expect(rendered(notesRichTextMarkdown([left, right])).querySelector("strong")?.textContent).toBe("onetwo");
  });
});


describe("adjacent Markdown annotations", () => {
  const styles = ["bold", "italic", "strikethrough", "code"] as const;
  for (const leftStyle of styles) {
    for (const rightStyle of styles) {
      it(`preserves adjacent ${leftStyle} and ${rightStyle} text`, () => {
        const left = createTextRichText("first");
        const right = createTextRichText("second");
        left.annotations[leftStyle] = true;
        right.annotations[rightStyle] = true;
        const html = rendered(notesRichTextMarkdown([left, right]));
        expect(html.textContent?.trim()).toBe("firstsecond");
        const tags = { bold: "strong", italic: "em", strikethrough: "del", code: "code" };
        expect(html.querySelector(tags[leftStyle])?.textContent).toContain("first");
        expect(html.querySelector(tags[rightStyle])?.textContent).toContain("second");
      });
    }
  }
});


describe("overlapping emphasis boundaries", () => {
  for (let leftMask = 0; leftMask < 4; leftMask += 1) {
    for (let rightMask = 0; rightMask < 4; rightMask += 1) {
      it(`preserves emphasis masks ${leftMask} and ${rightMask}`, () => {
        const runs = [createTextRichText("first"), createTextRichText("second")];
        for (const [index, mask] of [leftMask, rightMask].entries()) {
          runs[index].annotations.bold = Boolean(mask & 1);
          runs[index].annotations.italic = Boolean(mask & 2);
        }
        const html = rendered(notesRichTextMarkdown(runs));
        expect(html.textContent?.trim()).toBe("firstsecond");
        for (const [index, mask] of [leftMask, rightMask].entries()) {
          for (const [bit, tag] of [[1, "strong"], [2, "em"]] as const) {
            expect(Array.from(html.querySelectorAll(tag)).some((element) => element.textContent?.includes(runs[index].plain_text)))
              .toBe(Boolean(mask & bit));
          }
        }
      });
    }
  }
});
