// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import { renderOrganizationalMarkdown } from "$lib/chat/timeline/organizational-markdown";
import { toggleChatComposerLineFormat, type ChatComposerTextEdit } from "./line-formats";
import {
  chatComposerDocumentFromText,
  chatComposerDocumentVersioned,
  chatComposerMarkdown,
  parseChatComposerDocument,
  type ChatComposerDocument,
} from "./rich-text";

function rendered(markdown: string): DocumentFragment {
  const template = document.createElement("template");
  template.innerHTML = renderOrganizationalMarkdown(markdown, []).html;
  return template.content;
}

function applyEdits(text: string, edits: readonly ChatComposerTextEdit[]): string {
  return [...edits].reverse().reduce((value, edit) => value.slice(0, edit.start) + edit.text + value.slice(edit.end), text);
}

function roundTrips(document: ChatComposerDocument): string {
  const markdown = chatComposerMarkdown(document);
  expect(parseChatComposerDocument(chatComposerDocumentVersioned(document), markdown)).toEqual(document);
  return markdown;
}

describe("Chat composer formatting round trip", () => {
  it("renders every inline mark the composer stores", () => {
    const markdown = roundTrips({
      lines: [{
        runs: [
          { text: "bold", marks: ["bold"] },
          { text: " ", marks: [] },
          { text: "soft", marks: ["italic"] },
          { text: " ", marks: [] },
          { text: "gone", marks: ["strikethrough"] },
          { text: " ", marks: [] },
          { text: "a `tick`", marks: ["code"] },
          { text: " ", marks: [] },
          { text: "all", marks: ["bold", "italic", "strikethrough", "code"] },
        ],
      }],
    });
    const content = rendered(markdown);

    expect(content.querySelector("strong")?.textContent).toBe("bold");
    expect(content.querySelector("p > em")?.textContent).toBe("soft");
    expect(content.querySelector("p > del")?.textContent).toBe("gone");
    expect(content.querySelector("p > code")?.textContent).toBe("a `tick`");
    expect(content.querySelector("em > strong > del > code, strong > em > del > code")?.textContent).toBe("all");
  });

  it("keeps escaped punctuation in marked text literal", () => {
    const markdown = roundTrips({ lines: [{ runs: [{ text: "~a~ *b* `c`", marks: ["bold"] }] }] });

    expect(rendered(markdown).querySelector("strong")?.textContent).toBe("~a~ *b* `c`");
  });

  it("renders composer line formats as lists, quotes, and code blocks", () => {
    const bulleted = toggleChatComposerLineFormat("one\ntwo", { start: 0, end: 7 }, "bulletList");
    const numbered = toggleChatComposerLineFormat("one\ntwo", { start: 0, end: 7 }, "orderedList");
    const quoted = toggleChatComposerLineFormat("one\ntwo", { start: 0, end: 7 }, "blockquote");
    const code = toggleChatComposerLineFormat("say\nlet *x*", { start: 4, end: 4 }, "codeBlock");

    expect([...rendered(applyEdits("one\ntwo", bulleted.edits)).querySelectorAll("ul > li")].map((item) => item.textContent))
      .toEqual(["one", "two"]);
    expect(rendered(applyEdits("one\ntwo", numbered.edits)).querySelectorAll("ol > li")).toHaveLength(2);
    expect(rendered(applyEdits("one\ntwo", quoted.edits)).querySelector("blockquote")?.innerHTML).toContain("one<br>two");
    const codeText = applyEdits("say\nlet *x*", code.edits);
    expect(roundTrips(chatComposerDocumentFromText(codeText))).toBe("say\n```\nlet *x*\n```");
    expect(rendered(codeText).querySelector("pre > code")?.textContent).toBe("let *x*\n");
  });
});
