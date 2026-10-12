import { describe, expect, it } from "vitest";
import {
  createLinkedTextRichText,
  createPageMentionRichText,
  createTextRichText,
  richTextPlainText,
} from "./core";
import { planNotesMarkdownInlineShortcutConversion } from "./markdown";
import type { NotesRichText, NotesTextRichText } from "$lib/notes/types";

const pageId = "11111111-1111-4111-8111-111111111111";

function textItemWithContent(
  richText: readonly NotesRichText[],
  content: string,
): NotesTextRichText | undefined {
  return richText.find(
    (item): item is NotesTextRichText => item.type === "text" && item.plain_text === content,
  );
}

describe("notes markdown inline shortcuts", () => {
  it.each([
    {
      shortcut: "bold",
      source: "Make **bold**",
      expected: "Make bold",
      target: "bold",
      annotation: "bold",
    },
    {
      shortcut: "italic",
      source: "Make *italic*",
      expected: "Make italic",
      target: "italic",
      annotation: "italic",
    },
    {
      shortcut: "code",
      source: "Use `code`",
      expected: "Use code",
      target: "code",
      annotation: "code",
    },
    {
      shortcut: "strikethrough",
      source: "Drop ~strike~",
      expected: "Drop strike",
      target: "strike",
      annotation: "strikethrough",
    },
  ] as const)("converts $shortcut delimiters around typed text", (caseData) => {
    const plan = planNotesMarkdownInlineShortcutConversion(
      [createTextRichText(caseData.source)],
      caseData.source.length,
      caseData.source.length,
    );

    expect(plan?.shortcut).toBe(caseData.shortcut);
    expect(plan).not.toBeNull();
    if (!plan) return;
    expect(richTextPlainText(plan.richText)).toBe(caseData.expected);
    expect(plan.cursor).toBe(richTextPlainText(plan.richText).length);
    expect(textItemWithContent(plan.richText, caseData.target)?.annotations[caseData.annotation])
      .toBe(true);
  });

  it("preserves adjacent page mentions when converting text", () => {
    const source = [
      createPageMentionRichText(pageId, "Project", null),
      createTextRichText(" **bold**"),
    ];
    const plan = planNotesMarkdownInlineShortcutConversion(
      source,
      richTextPlainText(source).length,
      richTextPlainText(source).length,
    );

    expect(plan).not.toBeNull();
    if (!plan) return;
    expect(richTextPlainText(plan.richText)).toBe("Project bold");
    expect(plan.richText[0]).toMatchObject({
      type: "mention",
      mention: { type: "page", page: { id: pageId } },
      plain_text: "Project",
    });
    expect(textItemWithContent(plan.richText, "bold")?.annotations.bold).toBe(true);
  });

  it("preserves links inside converted text ranges", () => {
    const source = [
      createTextRichText("Read **"),
      createLinkedTextRichText("docs", "https://example.com/docs"),
      createTextRichText("**"),
    ];
    const plan = planNotesMarkdownInlineShortcutConversion(
      source,
      richTextPlainText(source).length,
      richTextPlainText(source).length,
    );

    expect(plan).not.toBeNull();
    if (!plan) return;
    const linkedText = textItemWithContent(plan.richText, "docs");
    expect(richTextPlainText(plan.richText)).toBe("Read docs");
    expect(linkedText?.annotations.bold).toBe(true);
    expect(linkedText?.text.link?.url).toBe("https://example.com/docs");
    expect(linkedText?.href).toBe("https://example.com/docs");
  });

  it("ignores incomplete bold syntax instead of converting italic", () => {
    const source = "Make **bold*";

    expect(
      planNotesMarkdownInlineShortcutConversion(
        [createTextRichText(source)],
        source.length,
        source.length,
      ),
    ).toBeNull();
  });

  it("ignores empty and whitespace-only shortcut content", () => {
    for (const source of ["****", "** **"]) {
      expect(
        planNotesMarkdownInlineShortcutConversion(
          [createTextRichText(source)],
          source.length,
          source.length,
        ),
      ).toBeNull();
    }
  });

  it("ignores selections and cursors that are not at a closing delimiter", () => {
    const source = "Make **bold**";

    expect(
      planNotesMarkdownInlineShortcutConversion([createTextRichText(source)], 0, 4),
    ).toBeNull();
    expect(
      planNotesMarkdownInlineShortcutConversion(
        [createTextRichText(source)],
        "Make **bold".length,
        "Make **bold".length,
      ),
    ).toBeNull();
  });

  it("ignores shortcut ranges that contain non-text rich objects", () => {
    const source = [
      createTextRichText("**"),
      createPageMentionRichText(pageId, "Project", null),
      createTextRichText("**"),
    ];

    expect(
      planNotesMarkdownInlineShortcutConversion(
        source,
        richTextPlainText(source).length,
        richTextPlainText(source).length,
      ),
    ).toBeNull();
  });

  it("ignores multi-line shortcut ranges", () => {
    const source = "**first\nsecond**";

    expect(
      planNotesMarkdownInlineShortcutConversion(
        [createTextRichText(source)],
        source.length,
        source.length,
      ),
    ).toBeNull();
  });
});
