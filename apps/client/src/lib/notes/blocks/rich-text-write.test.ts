import { describe, expect, it } from "vitest";
import { createRichText } from "./factory";
import { createBlockWriteFromRichText } from "./rich-text-write";
import { applyRichTextAnnotations, createLinkedTextRichText } from "$lib/notes/rich-text/core";

describe("notes rich text block writes", () => {
  it("creates paragraph writes without flattening formatting", () => {
    const richText = applyRichTextAnnotations(
      [createRichText("Important")],
      0,
      "Important".length,
      { bold: true, color: "yellow_background" },
    );
    const write = createBlockWriteFromRichText("block-1", "paragraph", richText, "blue");

    expect(write.type).toBe("paragraph");
    if (write.type !== "paragraph") throw new Error("expected paragraph write");
    expect(write.paragraph.color).toBe("blue");
    expect(write.paragraph.rich_text[0]).toMatchObject({
      type: "text",
      plain_text: "Important",
      annotations: { bold: true, color: "yellow_background" },
    });
  });

  it("creates unchecked to-do writes from split rich text", () => {
    const write = createBlockWriteFromRichText(
      "block-1",
      "to_do",
      [createLinkedTextRichText("Link", "https://example.com")],
      "green",
    );

    expect(write.type).toBe("to_do");
    if (write.type !== "to_do") throw new Error("expected to-do write");
    expect(write.to_do.checked).toBe(false);
    expect(write.to_do.color).toBe("green");
    expect(write.to_do.rich_text[0]).toMatchObject({
      type: "text",
      text: { link: { url: "https://example.com" } },
      href: "https://example.com",
    });
  });
});
