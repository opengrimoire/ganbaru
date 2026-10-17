import { describe, expect, it } from "vitest";
import {
  applyRichTextAnnotations,
  createLinkedTextRichText,
  createPageMentionRichText,
  createTextRichText,
  richTextPlainText,
} from "./core";
import { splitRichTextForBlock } from "./split";

const pageId = "11111111-1111-4111-8111-111111111111";

describe("notes rich text block splitting", () => {
  it("splits proxy-wrapped rich text from reactive state", () => {
    const item = new Proxy(createTextRichText("Hello world"), {});
    const split = splitRichTextForBlock([item], 5, 5);

    expect(richTextPlainText(split.before)).toBe("Hello");
    expect(richTextPlainText(split.after)).toBe(" world");
  });

  it("preserves formatting and rich objects around the split", () => {
    const [bold] = applyRichTextAnnotations(
      [createTextRichText("Important")],
      0,
      "Important".length,
      { bold: true },
    );
    if (!bold) throw new Error("expected bold rich text");
    const split = splitRichTextForBlock(
      [
        createTextRichText("Read "),
        createLinkedTextRichText("docs", "https://example.com/docs"),
        createTextRichText(" for "),
        createPageMentionRichText(pageId, "Project", null),
        createTextRichText(" "),
        bold,
      ],
      "Read docs for Project ".length,
      "Read docs for Project ".length,
    );

    expect(richTextPlainText(split.before)).toBe("Read docs for Project ");
    expect(richTextPlainText(split.after)).toBe("Important");
    expect(split.before.some((item) => item.type === "mention")).toBe(true);
    expect(
      split.before.some((item) =>
        item.type === "text" && item.text.link?.url === "https://example.com/docs"
      ),
    ).toBe(true);
    expect(split.after[0]).toMatchObject({ type: "text", annotations: { bold: true } });
  });

  it("uses a selected range as the removed boundary", () => {
    const split = splitRichTextForBlock([createTextRichText("Alpha selected omega")], 6, 14);

    expect(richTextPlainText(split.before)).toBe("Alpha ");
    expect(richTextPlainText(split.after)).toBe(" omega");
  });

  it("keeps non-text rich objects whole when the split lands inside their labels", () => {
    const mention = createPageMentionRichText(pageId, "Project", null);
    const splitBeforeMention = splitRichTextForBlock(
      [createTextRichText("See "), mention, createTextRichText(" soon")],
      "See Pro".length,
      "See Pro".length,
    );
    const splitAfterMention = splitRichTextForBlock(
      [createTextRichText("See "), mention, createTextRichText(" soon")],
      "See Proje".length,
      "See Proje".length,
    );

    expect(richTextPlainText(splitBeforeMention.before)).toBe("See ");
    expect(richTextPlainText(splitBeforeMention.after)).toBe("Project soon");
    expect(splitBeforeMention.after[0]).toEqual(mention);
    expect(richTextPlainText(splitAfterMention.before)).toBe("See Project");
    expect(splitAfterMention.before[1]).toEqual(mention);
    expect(richTextPlainText(splitAfterMention.after)).toBe(" soon");
  });
});
