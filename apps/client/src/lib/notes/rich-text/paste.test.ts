// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import {
  createLinkedTextRichText,
  createTextRichText,
  richTextPlainText,
} from "./core";
import { planNotesRichHtmlPaste } from "./paste";
import type {
  NotesBlock,
  NotesBlockUpdate,
  NotesParent,
  NotesRichText,
} from "$lib/notes/types";

const now = "2026-07-01T09:00:00.000Z";
const parent: NotesParent = { type: "page_id", page_id: "page-a" };

function paragraphBlock(richText: NotesRichText[] = [createTextRichText("")]): NotesBlock {
  return {
    object: "block",
    id: "block-a",
    parent,
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    type: "paragraph",
    paragraph: {
      rich_text: richText,
      color: "default",
    },
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

function codeBlock(): NotesBlock {
  return {
    object: "block",
    id: "code-a",
    parent,
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    type: "code",
    code: {
      rich_text: [createTextRichText("const value = 1;")],
      caption: [],
      language: "typescript",
    },
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

function currentRichText(update: NotesBlockUpdate): NotesRichText[] {
  if (update.type !== "paragraph") throw new Error("expected paragraph update");
  return update.paragraph.rich_text;
}

function nextId(): string {
  return crypto.randomUUID();
}

describe("notes rich text HTML paste planning", () => {
  it("preserves mixed nested lists without joining child text into its parent", () => {
    const plan = planNotesRichHtmlPaste({
      currentBlock: paragraphBlock(), selectionStart: 0, selectionEnd: 0,
      html: '<ul><li><p>Parent <strong>bold</strong></p><ol><li>Child<ul><li>Grandchild</li></ul></li><li>Child two</li></ol></li><li>Sibling</li></ul><p>After</p>',
      createId: nextId,
    });
    expect(plan?.blockDepths).toEqual([0, 1, 2, 1, 0, 0]);
    expect(plan?.currentUpdate).toMatchObject({ type: "bulleted_list_item", bulleted_list_item: {
      rich_text: [expect.objectContaining({ plain_text: "Parent " }), expect.objectContaining({ plain_text: "bold", annotations: expect.objectContaining({ bold: true }) })],
    } });
    expect(plan?.appendedBlocks.map((write) => write.type)).toEqual([
      "numbered_list_item", "bulleted_list_item", "numbered_list_item", "bulleted_list_item", "paragraph",
    ]);
    expect(plan?.appendedBlocks[0]).toMatchObject({ numbered_list_item: { rich_text: [expect.objectContaining({ plain_text: "Child" })] } });
  });

  it("keeps empty list parents and separate paragraphs inside an item", () => {
    const plan = planNotesRichHtmlPaste({
      currentBlock: paragraphBlock(), selectionStart: 0, selectionEnd: 0,
      html: '<ul><li><ul><li>Child</li></ul></li><li><p>First</p><p>Second</p></li></ul>', createId: nextId,
    });
    expect(plan?.blockDepths).toEqual([0, 1, 0]);
    expect(plan?.appendedBlocks[1]).toMatchObject({ bulleted_list_item: { rich_text: [expect.objectContaining({ plain_text: "First\nSecond" })] } });
  });

  it("preserves supported inline formatting and safe links", () => {
    const plan = planNotesRichHtmlPaste({
      currentBlock: paragraphBlock([createTextRichText("Start ")]),
      selectionStart: 6,
      selectionEnd: 6,
      html: [
        "<p>Hello ",
        "<strong>bold</strong> ",
        "<em>italic</em> ",
        "<u>under</u> ",
        "<s>gone</s> ",
        "<code>code</code> ",
        '<a href="https://example.com/docs">docs</a>',
        "</p>",
      ].join(""),
      createId: nextId,
    });

    if (!plan) throw new Error("expected rich HTML paste plan");
    const richText = currentRichText(plan.currentUpdate);
    expect(richTextPlainText(richText)).toBe("Start Hello bold italic under gone code docs");
    expect(richText.some((item) => item.type === "text" && item.plain_text === "bold" && item.annotations.bold))
      .toBe(true);
    expect(richText.some((item) => item.type === "text" && item.plain_text === "italic" && item.annotations.italic))
      .toBe(true);
    expect(richText.some((item) => item.type === "text" && item.plain_text === "under" && item.annotations.underline))
      .toBe(true);
    expect(richText.some((item) => item.type === "text" && item.plain_text === "gone" && item.annotations.strikethrough))
      .toBe(true);
    expect(richText.some((item) => item.type === "text" && item.plain_text === "code" && item.annotations.code))
      .toBe(true);
    expect(
      richText.some((item) =>
        item.type === "text"
        && item.plain_text === "docs"
        && item.text.link?.url === "https://example.com/docs"
      ),
    ).toBe(true);
    expect(plan.focusOffset).toBe("Start Hello bold italic under gone code docs".length);
  });

  it("splits rich paragraphs into current and appended sibling blocks", () => {
    const plan = planNotesRichHtmlPaste({
      currentBlock: paragraphBlock([createTextRichText("Before ")]),
      selectionStart: 7,
      selectionEnd: 7,
      html: "<p>First <strong>bold</strong></p><p>Second</p><p>Third</p>",
      createId: nextId,
    });

    if (!plan) throw new Error("expected rich HTML paste plan");
    expect(richTextPlainText(currentRichText(plan.currentUpdate)))
      .toBe("Before First bold");
    expect(plan.appendedBlocks).toHaveLength(2);
    const [second, third] = plan.appendedBlocks;
    expect(second?.type).toBe("paragraph");
    if (second?.type === "paragraph") {
      expect(richTextPlainText(second.paragraph.rich_text)).toBe("Second");
    }
    expect(third?.type).toBe("paragraph");
    if (third?.type === "paragraph") {
      expect(richTextPlainText(third.paragraph.rich_text)).toBe("Third");
    }
    expect(plan.focusBlockId).toBe(third?.id);
    expect(plan.focusOffset).toBe("Third".length);
  });

  it("moves selected suffix into the final pasted rich block", () => {
    const plan = planNotesRichHtmlPaste({
      currentBlock: paragraphBlock([createTextRichText("Hello "), createLinkedTextRichText("world", "https://example.com")]),
      selectionStart: 6,
      selectionEnd: 6,
      html: "<p>First</p><p><strong>Second</strong></p>",
      createId: nextId,
    });

    if (!plan) throw new Error("expected rich HTML paste plan");
    expect(richTextPlainText(currentRichText(plan.currentUpdate)))
      .toBe("Hello First");
    const appended = plan.appendedBlocks[0];
    expect(appended?.type).toBe("paragraph");
    if (appended?.type === "paragraph") {
      expect(richTextPlainText(appended.paragraph.rich_text)).toBe("Secondworld");
      expect(
        appended.paragraph.rich_text.some((item) =>
          item.type === "text"
          && item.plain_text === "Second"
          && item.annotations.bold
        ),
      ).toBe(true);
      expect(
        appended.paragraph.rich_text.some((item) =>
          item.type === "text"
          && item.plain_text === "world"
          && item.text.link?.url === "https://example.com"
        ),
      ).toBe(true);
    }
    expect(plan.focusOffset).toBe("Second".length);
  });

  it("rejects scripts and unsafe URLs while keeping safe text", () => {
    const plan = planNotesRichHtmlPaste({
      currentBlock: paragraphBlock(),
      selectionStart: 0,
      selectionEnd: 0,
      html: [
        '<script>alert("bad")</script>',
        '<p>Safe <a href="javascript:alert(1)">bad link</a>',
        '<img src="x" onerror="alert(1)">',
        '<a href="https://safe.example">safe link</a></p>',
      ].join(""),
      createId: nextId,
    });

    if (!plan) throw new Error("expected rich HTML paste plan");
    const richText = currentRichText(plan.currentUpdate);
    expect(richTextPlainText(richText)).toBe("Safe bad linkxsafe link");
    expect(richTextPlainText(richText)).not.toContain("alert");
    expect(
      richText.some((item) =>
        item.type === "text"
        && item.plain_text === "bad link"
        && item.text.link !== null
      ),
    ).toBe(false);
    expect(
      richText.some((item) =>
        item.type === "text"
        && item.plain_text === "safe link"
        && item.text.link?.url === "https://safe.example/"
      ),
    ).toBe(true);
  });

  it("preserves Notes copy metadata for links, annotations, and color", () => {
    const plan = planNotesRichHtmlPaste({
      currentBlock: paragraphBlock(),
      selectionStart: 0,
      selectionEnd: 0,
      html: [
        "<p>",
        '<span data-notes-bold="true" data-notes-underline="true" ',
        'data-notes-rich-text-color="blue_background" ',
        'data-notes-link-url="mailto:team@example.com">Team</span>',
        "</p>",
      ].join(""),
      createId: nextId,
    });

    if (!plan) throw new Error("expected rich HTML paste plan");
    const richText = currentRichText(plan.currentUpdate);
    expect(richText).toHaveLength(1);
    const item = richText[0];
    expect(item).toMatchObject({
      type: "text",
      plain_text: "Team",
      annotations: {
        bold: true,
        underline: true,
        color: "blue_background",
      },
      text: {
        link: { url: "mailto:team@example.com" },
      },
    });
  });

  it("leaves code blocks to the plain text paste path", () => {
    expect(
      planNotesRichHtmlPaste({
        currentBlock: codeBlock(),
        selectionStart: 0,
        selectionEnd: 0,
        html: "<p><strong>Code</strong></p>",
        createId: nextId,
      }),
    ).toBeNull();
  });
});
