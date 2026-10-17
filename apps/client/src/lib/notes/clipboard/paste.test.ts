// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { notesClipboardPasteHtml, readNotesClipboard } from "./paste";
import { planNotesRichHtmlPaste } from "$lib/notes/rich-text/paste";
import { createBlockWrite, blockEditableRichText, applyBlockUpdate } from "$lib/notes/blocks/factory";
import type { NotesBlock } from "$lib/notes/types";

/** Exercise format resolution and the actual paste planner together. */
function paste(text: string, html = "", type: NotesBlock["type"] = "paragraph", currentText = "") {
  const block = { ...createBlockWrite("target", type, currentText), object: "block", parent: { type: "page_id", page_id: "page" },
    created_time: "", last_edited_time: "", has_children: false, in_trash: false,
    source_provider: null, source_object_id: null, source_last_edited_time: null } as NotesBlock;
  let id = 0;
  const plan = planNotesRichHtmlPaste({ currentBlock: block, selectionStart: 0, selectionEnd: currentText.length,
    html: notesClipboardPasteHtml(text, html), createId: () => `new-${++id}` });
  return { plan, richText: plan ? blockEditableRichText(applyBlockUpdate(block, plan.currentUpdate)) : [] };
}

afterEach(() => { vi.unstubAllGlobals(); });

describe("Notes clipboard representation resolution", () => {
  it("reads paired representations from the same item for menu paste", async () => {
    vi.stubGlobal("navigator", { clipboard: {
      read: async () => [{ types: ["text/plain", "text/html"], getType: async (type: string) => ({ text: async () => type === "text/plain" ? "# Title" : "<h2>Title</h2>" }) }],
      readText: vi.fn(),
    } });
    const { plainText, html } = await readNotesClipboard();
    expect(notesClipboardPasteHtml(plainText, html)).toBe("<h1>Title</h1>");
    expect(navigator.clipboard.readText).not.toHaveBeenCalled();
  });
  it("uses Markdown heading levels when paired HTML shifts them, preserving HTML styling", () => {
    const { plan, richText } = paste("# Title\n\n## Subtitle\n\n### Detail", '<h2><strong>Title</strong></h2><h3>Subtitle</h3><h4>Detail</h4>');
    expect(plan?.currentUpdate.type).toBe("heading_1");
    expect(plan?.appendedBlocks.map((block) => block.type)).toEqual(["heading_2", "heading_3"]);
    expect(richText[0].annotations.bold).toBe(true);
  });

  it("does not promote unrelated HTML headings or interpret escaped and fenced hashes", () => {
    expect(paste("Subtitle", "<h2>Subtitle</h2>").plan?.currentUpdate.type).toBe("heading_2");
    expect(paste("# Unrelated", "<h2>Subtitle</h2>").plan?.currentUpdate.type).toBe("heading_2");
    expect(paste("\\# Literal", "<h2># Literal</h2>").plan?.currentUpdate.type).toBe("heading_2");
    expect(paste("```\n# Code\n```", "<h2># Code</h2>").plan?.currentUpdate.type).toBe("heading_2");
  });

  it.each([1, 2, 3, 4, 5, 6])("pastes Markdown heading level %i into empty and fully selected heading blocks", (level) => {
    const text = `${"#".repeat(level)} Title`;
    expect(paste(text).plan?.currentUpdate.type).toBe(`heading_${level}`);
    expect(paste(text, "", "heading_2").plan?.currentUpdate.type).toBe(`heading_${level}`);
    expect(paste(text, "", "heading_2", "Old heading").plan?.currentUpdate.type).toBe(`heading_${level}`);
  });

  it("parses inline Markdown, quotes, nested tasks, dividers, and variable-length code fences", () => {
    const text = "**Bold** and [link](https://example.com)\n\n> Quote\n\n- [x] Done\n  - Child\n\n---\n\n````ts\nconst code = '```';\n````";
    const { plan, richText } = paste(text);
    expect(richText[0].annotations.bold).toBe(true);
    expect(richText.at(-1)?.href).toBe("https://example.com/");
    expect(plan?.appendedBlocks.map((block) => block.type)).toEqual(["quote", "to_do", "bulleted_list_item", "divider", "code"]);
    expect(plan?.appendedBlocks[1]).toMatchObject({ to_do: { checked: true } });
    expect(plan?.blockDepths).toEqual([0, 0, 0, 1, 0, 0]);
    expect(plan?.appendedBlocks.at(-1)).toMatchObject({ code: { language: "ts" } });
  });

  it.each(["code", "code\n", "code\n\n"])("preserves code block whitespace without adding a trailing newline: %j", (text) => {
    const { richText } = paste(`\`\`\`\n${text}\n\`\`\``);
    expect(richText.map((item) => item.plain_text).join("")).toBe(text);
  });

  it("keeps literal HTML as text and strips unsafe links from Markdown", () => {
    const { richText } = paste('**Safe** <script>alert(1)</script> [bad](javascript:alert)');
    expect(richText.map((item) => item.plain_text).join("")).toContain("<script>alert(1)</script>");
    expect(richText.every((item) => item.href === null)).toBe(true);
  });

  it("converts Markdown tables into semantic HTML", () => {
    expect(notesClipboardPasteHtml("| A | B |\n| --- | --- |\n| X | Y |")).toContain("<td>X</td>");
  });
});
