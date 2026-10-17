import { describe, expect, it } from "vitest";
import {
  chatComposerDocumentFromText,
  chatComposerDocumentVersioned,
  chatComposerMarkdown,
  chatComposerMarkdownOffset,
  chatComposerPlainText,
  chatComposerVisibleOffset,
  parseChatComposerDocument,
  replaceChatComposerText,
  toggleChatComposerMark,
  type ChatComposerDocument,
} from "./rich-text";

describe("Chat composer rich text", () => {
  it("serializes supported marks to provider-facing Markdown", () => {
    const document: ChatComposerDocument = {
      lines: [{
        runs: [
          { text: "Plain ", marks: [] },
          { text: "bold", marks: ["bold"] },
          { text: " and ", marks: [] },
          { text: "italic", marks: ["italic"] },
          { text: " together", marks: ["bold", "italic"] },
        ],
      }],
    };

    expect(chatComposerMarkdown(document)).toBe("Plain **bold** and *italic* ***together***");
    expect(chatComposerPlainText(document)).toBe("Plain bold and italic together");
  });

  it("keeps edge whitespace outside emphasis delimiters", () => {
    const document: ChatComposerDocument = {
      lines: [
        {
          runs: [
            { text: "Say", marks: [] },
            { text: " hello ", marks: ["bold"] },
            { text: "now", marks: [] },
          ],
        },
        {
          runs: [
            { text: "a", marks: [] },
            { text: "\t ", marks: ["italic"] },
            { text: "b", marks: [] },
            { text: "  ", marks: ["bold", "italic"] },
          ],
        },
        { runs: [{ text: " *both* ", marks: ["bold", "italic"] }] },
      ],
    };

    expect(chatComposerMarkdown(document)).toBe(
      "Say **hello** now\na\t b  \n ***\\*both\\**** ",
    );
    expect(chatComposerPlainText(document)).toBe("Say hello now\na\t b  \n *both* ");
  });

  it("uses asterisk emphasis so intraword and adjacent marks stay valid CommonMark", () => {
    const document: ChatComposerDocument = {
      lines: [
        {
          runs: [
            { text: "un", marks: [] },
            { text: "believ", marks: ["italic"] },
            { text: "able and re", marks: [] },
            { text: "do", marks: ["bold", "italic"] },
            { text: "ne", marks: [] },
          ],
        },
        {
          runs: [
            { text: "strong", marks: ["bold"] },
            { text: "soft", marks: ["italic"] },
            { text: "both", marks: ["bold", "italic"] },
          ],
        },
      ],
    };
    const markdown = chatComposerMarkdown(document);

    expect(markdown).toBe("un*believ*able and re***do***ne\n**strong***soft****both***");
    expect(parseChatComposerDocument(chatComposerDocumentVersioned(document), markdown)).toEqual(document);
    expect(chatComposerMarkdownOffset(document, 2)).toBe(3);
    expect(chatComposerMarkdownOffset(document, 8, "backward")).toBe(9);
    expect(chatComposerVisibleOffset(document, 9)).toBe(8);
  });

  it("maps offsets around whitespace moved outside emphasis delimiters", () => {
    const document: ChatComposerDocument = {
      lines: [{
        runs: [
          { text: "Say", marks: [] },
          { text: " hello ", marks: ["bold"] },
          { text: "now", marks: [] },
        ],
      }],
    };
    // Markdown: "Say **hello** now"; visible: "Say hello now".

    expect(chatComposerMarkdownOffset(document, 3)).toBe(3);
    expect(chatComposerMarkdownOffset(document, 4)).toBe(6);
    expect(chatComposerMarkdownOffset(document, 9)).toBe(13);
    expect(chatComposerMarkdownOffset(document, 10, "backward")).toBe(14);
    expect(chatComposerMarkdownOffset(document, 13, "backward")).toBe(17);
    expect(chatComposerVisibleOffset(document, 6)).toBe(4);
    expect(chatComposerVisibleOffset(document, 13)).toBe(9);
    expect(chatComposerVisibleOffset(document, 14)).toBe(10);

    const whitespaceOnly: ChatComposerDocument = {
      lines: [{ runs: [{ text: "a", marks: [] }, { text: "  ", marks: ["bold"] }] }],
    };
    expect(chatComposerMarkdownOffset(whitespaceOnly, 3, "backward")).toBe(3);
    expect(chatComposerMarkdownOffset(whitespaceOnly, 2)).toBe(2);
  });

  it("preserves empty and trailing lines during text replacement", () => {
    const initial = chatComposerDocumentFromText("Example");
    const withBreak = replaceChatComposerText(initial, { start: 7, end: 7 }, "\n").document;

    expect(chatComposerPlainText(withBreak)).toBe("Example\n");
    expect(withBreak.lines).toHaveLength(2);
    expect(withBreak.lines[1]?.runs).toEqual([]);

    const cleared = replaceChatComposerText(withBreak, { start: 0, end: 8 }, "").document;
    const typed = replaceChatComposerText(cleared, { start: 0, end: 0 }, "abcñ").document;
    expect(chatComposerPlainText(typed)).toBe("abcñ");
  });

  it("toggles marks across line boundaries without formatting the break", () => {
    const initial = chatComposerDocumentFromText("one\ntwo");
    const formatted = toggleChatComposerMark(initial, { start: 1, end: 6 }, "bold");

    expect(chatComposerMarkdown(formatted)).toBe("o**ne**\n**tw**o");
    expect(chatComposerPlainText(formatted)).toBe("one\ntwo");
    expect(chatComposerMarkdown(toggleChatComposerMark(formatted, { start: 1, end: 6 }, "bold")))
      .toBe("one\ntwo");
  });

  it("loads versioned rich content only when it matches the Markdown fallback", () => {
    const formatted = toggleChatComposerMark(
      chatComposerDocumentFromText("Important"),
      { start: 0, end: 9 },
      "bold",
    );
    const versioned = chatComposerDocumentVersioned(formatted);

    expect(parseChatComposerDocument(versioned, "**Important**")).toEqual(formatted);
    expect(parseChatComposerDocument(versioned, "Changed")).toEqual(chatComposerDocumentFromText("Changed"));
    expect(parseChatComposerDocument({
      schemaVersion: 1,
      value: { lines: [{ runs: [{ text: "Invalid\nline", marks: [] }] }] },
    }, "Fallback")).toEqual(chatComposerDocumentFromText("Fallback"));
    expect(parseChatComposerDocument({ schemaVersion: 99, value: {} }, "Fallback"))
      .toEqual(chatComposerDocumentFromText("Fallback"));
  });
});
