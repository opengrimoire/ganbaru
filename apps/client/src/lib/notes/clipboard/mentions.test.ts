// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { notesClipboardContent } from "./export";
import { notesInlineClipboardRichText, planNotesRichHtmlPaste } from "$lib/notes/rich-text/paste";
import {
  createDatabaseMentionRichText,
  createDateMentionRichText,
  createDateMentionValue,
  createLocalObjectMentionRichText,
  createPageMentionRichText,
  createTextRichText,
  createUserMentionRichText,
  richTextPlainText,
} from "$lib/notes/rich-text/core";
import type { NotesParagraphBlock, NotesRichText } from "$lib/notes/types";

const PAGE_ID = "11111111-1111-4111-8111-111111111111";
const DATABASE_ID = "22222222-2222-4222-8222-222222222222";
const OTHER_DATABASE_ID = "33333333-3333-4333-8333-333333333333";
const BLOCK_ID = "44444444-4444-4444-8444-444444444444";
const USER_ID = "55555555-5555-4555-8555-555555555555";
const FIXTURE_TIME = "2026-09-30T09:00:00.000Z";
const PAGE_URL = `#notes?page=${PAGE_ID}`;
const DATABASE_URL = `${PAGE_URL}&block=${DATABASE_ID}`;

/** Exercise clipboard serialization with the complete canonical paragraph model. */
function paragraph(richText: NotesRichText[] = [createTextRichText("")]): NotesParagraphBlock {
  return {
    object: "block", id: BLOCK_ID, type: "paragraph", parent: { type: "page_id", page_id: PAGE_ID },
    created_time: FIXTURE_TIME, last_edited_time: FIXTURE_TIME, has_children: false, in_trash: false,
    source_provider: null, source_object_id: null, source_last_edited_time: null,
    paragraph: { rich_text: richText, color: "default" },
  };
}

/** Parse the public clipboard representation through sanitization and the normal inline reader. */
function pastedInline(html: string): NotesRichText[] {
  const result = notesInlineClipboardRichText(html);
  if (!result) throw new Error("Expected readable inline clipboard content");
  return result;
}

describe("inline Notes reference clipboard round trips", () => {
  it.each(["page", "database"] as const)("retains a catalog %s mention without a stored hyperlink between ordinary text", (type) => {
    const mention = type === "page" ? createPageMentionRichText(PAGE_ID, "Library") : createDatabaseMentionRichText(DATABASE_ID, "Library");
    const clipboard = notesClipboardContent([{ block: paragraph([createTextRichText("Before "), mention, createTextRichText(" after")]) }]);
    const result = pastedInline(clipboard.html);
    expect(result).toHaveLength(3);
    expect(result[1]).toMatchObject({ type: "mention", mention: mention.mention, plain_text: "Library", annotations: mention.annotations,
      href: type === "page" ? PAGE_URL : null });
    expect(richTextPlainText(result)).toBe("Before Library after");
    expect(clipboard.plainText).toBe("Before Library after");
    const template = document.createElement("template");
    template.innerHTML = clipboard.html;
    const reference = template.content.querySelector("[data-notes-reference-id]");
    expect(reference?.getAttribute("data-notes-reference-type")).toBe(type);
    expect(reference?.getAttribute("data-notes-reference-id")).toBe(type === "page" ? PAGE_ID : DATABASE_ID);
    expect(reference?.textContent).toBe("Library");
    expect(reference?.getAttribute("href")).toBe(type === "page" ? PAGE_URL : null);
  });

  it("preserves a database's known owner link and pastes it as inline content without duplicating its graph", () => {
    const mention = createDatabaseMentionRichText(DATABASE_ID, "Tasks");
    mention.href = DATABASE_URL;
    const clipboard = notesClipboardContent([{ block: paragraph([mention]) }]);
    expect(pastedInline(clipboard.html)).toEqual([mention]);
    const plan = planNotesRichHtmlPaste({ currentBlock: paragraph(), selectionStart: 0, selectionEnd: 0,
      html: clipboard.html, createId: () => OTHER_DATABASE_ID });
    expect(plan?.currentUpdate).toMatchObject({ type: "paragraph", paragraph: { rich_text: [mention] } });
    expect(plan?.appendedBlocks).toEqual([]);
    expect(plan?.copiedDatabaseIds).toBeUndefined();
    expect(plan?.copiedPageIds).toBeUndefined();
  });

  it("uses a page identity when its stored hyperlink is stale", () => {
    const mention = createPageMentionRichText(PAGE_ID, "Project", "https://example.com/old");
    const clipboard = notesClipboardContent([{ block: paragraph([mention]) }]);
    expect(pastedInline(clipboard.html)[0]).toMatchObject({ type: "mention", mention: mention.mention, href: PAGE_URL });
    expect(clipboard.html).not.toContain("https://example.com/old");
  });

  it.each(["page", "database"] as const)("retains all nested annotations and escaped label characters on a %s mention", (type) => {
    const title = 'Task <draft> & "review"';
    const mention = type === "page" ? createPageMentionRichText(PAGE_ID, title) : createDatabaseMentionRichText(DATABASE_ID, title);
    mention.annotations = { bold: true, italic: true, underline: true, strikethrough: true, code: true, color: "blue_background" };
    const clipboard = notesClipboardContent([{ block: paragraph([mention]) }]);
    expect(pastedInline(clipboard.html)).toEqual([{ ...mention, href: type === "page" ? PAGE_URL : null }]);
    const template = document.createElement("template");
    template.innerHTML = clipboard.html;
    expect(template.content.textContent).toBe(title);
    expect(template.content.querySelector("draft")).toBeNull();
  });

  it("preserves distinct annotation runs inside a reference instead of flattening its label", () => {
    const result = pastedInline(`<span data-notes-reference-type="database" data-notes-reference-id="${DATABASE_ID}"><strong>Bold</strong> regular <em>Italic</em></span>`);
    expect(result).toHaveLength(3);
    expect(result.map((item) => item.annotations.bold)).toEqual([true, false, false]);
    expect(result.map((item) => item.annotations.italic)).toEqual([false, false, true]);
    for (const item of result) expect(item).toMatchObject({ type: "mention", mention: { type: "database", database: { id: DATABASE_ID } }, href: null });
    expect(richTextPlainText(result)).toBe("Bold regular Italic");
  });

  it("retains line breaks within one mention and keeps adjacent mentions distinct", () => {
    const first = createDatabaseMentionRichText(DATABASE_ID, "Line\nNext");
    const second = createDatabaseMentionRichText(DATABASE_ID, "Second");
    const clipboard = notesClipboardContent([{ block: paragraph([first, second]) }]);
    expect(pastedInline(clipboard.html)).toEqual([first, second]);
  });

  it("reads unresolved renderer references before normalizing their hyperlink and preserves forwarded annotations", () => {
    const result = pastedInline(`<a href="#notes?block=${DATABASE_ID}" data-notes-reference-type="database" data-notes-reference-id="${DATABASE_ID}" data-notes-bold="true" data-notes-italic="true" data-notes-rich-text-color="purple_background">Tasks</a>`);
    expect(result).toHaveLength(1);
    expect(result[0]).toMatchObject({ type: "mention", mention: { type: "database", database: { id: DATABASE_ID } }, plain_text: "Tasks",
      href: null, annotations: { bold: true, italic: true, color: "purple_background" } });
  });

  it("preserves explicit whitespace in a selected reference label", () => {
    const result = pastedInline(`<span data-notes-reference-type="database" data-notes-reference-id="${DATABASE_ID}" style="white-space: pre-wrap">  Task\tlabel  </span>`);
    expect(result[0]).toMatchObject({ type: "mention", mention: { type: "database", database: { id: DATABASE_ID } }, plain_text: "  Task\tlabel  " });
  });

  it.each([
    'data-notes-reference-type="database"',
    `data-notes-reference-id="${DATABASE_ID}"`,
    `data-notes-reference-type="unknown" data-notes-reference-id="${DATABASE_ID}"`,
    'data-notes-reference-type="database" data-notes-reference-id="invalid"',
    'data-notes-reference-type="page" data-notes-reference-id=""',
  ])("keeps malformed reference metadata readable: %s", (attributes) => {
    const result = pastedInline(`<span ${attributes}>Readable label</span>`);
    expect(result).toHaveLength(1);
    expect(result[0]).toMatchObject({ type: "text", plain_text: "Readable label", href: null });
  });

  it("retains a valid ordinary hyperlink when its reference marker is invalid", () => {
    const result = pastedInline(`<a href="${PAGE_URL}" data-notes-reference-type="page" data-notes-reference-id="invalid">Readable label</a>`);
    expect(result[0]).toMatchObject({ type: "text", plain_text: "Readable label", href: PAGE_URL });
  });

  it("does not infer a database identity from an ownerless hyperlink without reference markers", () => {
    const result = pastedInline(`<a href="#notes?block=${DATABASE_ID}">Readable label</a>`);
    expect(result).toHaveLength(1);
    expect(result[0]).toMatchObject({ type: "text", plain_text: "Readable label", href: null });
  });

  it("drops a hyperlink to a different database while retaining the validated mention identity", () => {
    const result = pastedInline(`<a href="${PAGE_URL}&block=${OTHER_DATABASE_ID}" data-notes-reference-type="database" data-notes-reference-id="${DATABASE_ID}">Tasks</a>`);
    expect(result[0]).toMatchObject({ type: "mention", mention: { type: "database", database: { id: DATABASE_ID } }, plain_text: "Tasks", href: null });
  });

  it("sanitizes unsafe destinations without losing a valid reference's readable label", () => {
    const result = pastedInline(`<a href="javascript:alert(1)" data-notes-reference-type="database" data-notes-reference-id="${DATABASE_ID}">Tasks</a>`);
    expect(result[0]).toMatchObject({ type: "mention", mention: { type: "database", database: { id: DATABASE_ID } }, plain_text: "Tasks", href: null });
  });

  it.each(["page", "database"] as const)("exports an invalid %s mention identity as readable text", (type) => {
    const mention = type === "page" ? createPageMentionRichText("invalid", "Readable label") : createDatabaseMentionRichText("invalid", "Readable label");
    const clipboard = notesClipboardContent([{ block: paragraph([mention]) }]);
    expect(clipboard.html).not.toContain("data-notes-reference-id");
    expect(pastedInline(clipboard.html)[0]).toMatchObject({ type: "text", plain_text: "Readable label" });
  });

  it.each([
    ["project", createLocalObjectMentionRichText("project", "eating", "Eating")],
    ["task", createLocalObjectMentionRichText("project_task", "task-1", "Shop")],
    ["user", createUserMentionRichText(USER_ID, "Ana")],
    ["date", createDateMentionRichText(createDateMentionValue("2026-10-11", true), "October 11")],
  ] as const)("retains a %s mention identity between ordinary text", (_kind, mention) => {
    const clipboard = notesClipboardContent([{ block: paragraph([createTextRichText("See "), mention, createTextRichText(" soon")]) }]);
    expect(clipboard.plainText).toBe(`See ${mention.plain_text} soon`);
    expect(pastedInline(clipboard.html)[1]).toEqual(mention);
  });

  it.each([
    ["invalid JSON", "{"],
    ["an unknown object type", JSON.stringify({ type: "ganbaru_object", ganbaru_object: { type: "account", id: "x" } })],
    ["an empty object id", JSON.stringify({ type: "ganbaru_object", ganbaru_object: { type: "project", id: "" } })],
    ["a non-UUID user", JSON.stringify({ type: "user", user: { object: "user", id: "someone" } })],
    ["a page smuggled past reference markers", JSON.stringify({ type: "page", page: { id: PAGE_ID } })],
    ["an oversized payload", JSON.stringify({ type: "ganbaru_object", ganbaru_object: { type: "project", id: "x".repeat(4096) } })],
  ])("pastes a mention with %s as readable text", (_case, payload) => {
    const template = document.createElement("template");
    const span = document.createElement("span");
    span.setAttribute("data-notes-mention", payload);
    span.textContent = "Readable label";
    template.content.append(span);
    const result = pastedInline(template.innerHTML);
    expect(result).toHaveLength(1);
    expect(result[0]).toMatchObject({ type: "text", plain_text: "Readable label" });
  });
});
