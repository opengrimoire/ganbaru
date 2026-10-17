// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { marked } from "marked";
import { notesClipboardContent } from "./export";
import { notesClipboardPasteHtml } from "./paste";
import { notesInlineClipboardRichText, planNotesRichHtmlPaste } from "$lib/notes/rich-text/paste";
import { applyBlockUpdate, blockEditableRichText, blockPlainText, createBlockWrite } from "$lib/notes/blocks/factory";
import { createTextRichText } from "$lib/notes/rich-text/core";
import type { NotesBlock, NotesBlockUpdate } from "$lib/notes/types";

/** Construct complete model blocks so export and paste can be exercised together. */
function block(type: NotesBlock["type"], text = "", id = "block"): NotesBlock {
  return { ...createBlockWrite(id, type, text), object: "block", parent: { type: "page_id", page_id: "page" },
    created_time: "", last_edited_time: "", has_children: false, in_trash: false,
    source_provider: null, source_object_id: null, source_last_edited_time: null } as NotesBlock;
}

/** Convert arbitrary clipboard representations through the actual paste planner. */
function pasted(text: string, html = "", currentText = "", start = 0, end = start) {
  let id = 0;
  const current = block("paragraph", currentText);
  const plan = planNotesRichHtmlPaste({ currentBlock: current, selectionStart: start, selectionEnd: end,
    html: notesClipboardPasteHtml(text, html), createId: () => `new-${++id}` });
  const updates: NotesBlockUpdate[] = plan ? [plan.currentUpdate, ...plan.appendedBlocks] : [];
  return { plan, blocks: updates.map((update) => applyBlockUpdate(current, update)) };
}

describe("Notes portable clipboard interoperability", () => {
  it.each(["Task [list] & <items>", ""])("copies a database named %j between text as a local block link", (title) => {
    const pageId = "10000000-0000-4000-8000-000000000001";
    const databaseId = "10000000-0000-4000-8000-000000000002";
    const database = { ...block("child_database", title, databaseId), parent: { type: "page_id", page_id: pageId } } as NotesBlock;
    const options = { pageId, unnamedDatabaseTitle: "New database" };
    const link = `#notes?page=${pageId}&block=${databaseId}`;
    const clipboard = notesClipboardContent([
      block("paragraph", "Before", "first"), database, block("paragraph", "After", "last"),
    ].map((block) => ({ block })), options);
    const template = document.createElement("template");
    template.innerHTML = clipboard.html;
    expect(template.content.querySelector("a")?.textContent).toBe(title || options.unnamedDatabaseTitle);
    expect(template.content.querySelector("a")?.getAttribute("href")).toBe(link);
    expect(template.content.querySelector("table")).toBeNull();
    expect(clipboard.plainText).toContain(link);
    for (const html of [clipboard.html, ""]) {
      const result = pasted(clipboard.plainText, html);
      expect(result.blocks.map((entry) => entry.type)).toEqual(["paragraph", "paragraph", "paragraph"]);
      expect(result.blocks.map(blockPlainText)).toEqual(["Before", title || options.unnamedDatabaseTitle, "After"]);
      expect(blockEditableRichText(result.blocks[1])[0].href).toBe(link);
      expect(result.plan?.copiedPageIds).toBeUndefined();
    }
    expect(notesClipboardContent([{ block: database, start: 1 }], options).plainText).toBe("");
    expect(notesClipboardContent([{ block: database, end: 0 }], options).plainText).toBe("");
  });

  it("resolves a database inside selected layout blocks and uses page context for omitted ancestors", () => {
    const pageId = "10000000-0000-4000-8000-000000000001";
    const databaseId = "10000000-0000-4000-8000-000000000002";
    const layout = { ...block("column", "", "layout"), parent: { type: "page_id", page_id: pageId } } as NotesBlock;
    const database = { ...block("child_database", "Tasks", databaseId), parent: { type: "block_id", block_id: layout.id } } as NotesBlock;
    const options = { pageId, unnamedDatabaseTitle: "New database" };
    const reference = `[Tasks](#notes?page=${pageId}&block=${databaseId})`;
    expect(notesClipboardContent([{ block: layout }, { block: database }], options).plainText).toBe(reference);
    expect(notesClipboardContent([{ block: database }], options).plainText).toBe(reference);
    expect(notesClipboardContent([{ block: database }]).plainText).toBe("Tasks");
  });

  it("keeps portable database links while planning an independent in-app database copy", () => {
    const pageId = "10000000-0000-4000-8000-000000000001";
    const databaseId = "10000000-0000-4000-8000-000000000002";
    const database = block("child_database", "Tasks", databaseId);
    database.parent = { type: "page_id", page_id: pageId };
    if (database.type !== "child_database") throw new Error("Expected database");
    database.child_database = { title: "Tasks", database_id: databaseId,
      data_source_id: "10000000-0000-4000-8000-000000000003", view_id: "10000000-0000-4000-8000-000000000004" };
    const clipboard = notesClipboardContent([{ block: database }], { pageId, unnamedDatabaseTitle: "New database" });
    expect(clipboard.plainText).toBe(`[Tasks](#notes?page=${pageId}&block=${databaseId})`);
    const template = document.createElement("template");
    template.innerHTML = clipboard.html;
    expect(template.content.querySelector("p")?.dataset.notesChildDatabaseId).toBe(databaseId);
    expect(template.content.querySelector("a")?.textContent).toBe("Tasks");
    expect(template.content.querySelector("table")).toBeNull();
    const result = pasted(clipboard.plainText, clipboard.html, "BeforeAfter", 6);
    expect(result.blocks.map((entry) => entry.type)).toEqual(["paragraph", "child_database", "paragraph"]);
    expect(result.blocks.map(blockPlainText)).toEqual(["Before", "Tasks", "After"]);
    expect(result.plan?.copiedDatabaseIds).toEqual({ "new-1": databaseId });
    const portable = pasted(clipboard.plainText);
    expect(portable.plan?.copiedDatabaseIds).toBeUndefined();
    expect(blockEditableRichText(portable.blocks[0])[0].href).toContain(`block=${databaseId}`);
  });

  it("preserves a copied database between surrounding paragraphs and ignores invalid database markers", () => {
    const databaseId = "10000000-0000-4000-8000-000000000002";
    const { plan, blocks } = pasted("Before\n\nTasks\n\nAfter",
      `<p>Before</p><p data-notes-child-database-id="${databaseId}"><a href="#notes?page=10000000-0000-4000-8000-000000000001&block=${databaseId}">Tasks</a></p><p>After</p>`);
    expect(blocks.map((entry) => entry.type)).toEqual(["paragraph", "child_database", "paragraph"]);
    expect(plan?.copiedDatabaseIds).toEqual({ "new-1": databaseId });
    const invalid = pasted("Tasks", '<p data-notes-child-database-id="invalid">Tasks</p>');
    expect(invalid.plan?.copiedDatabaseIds).toBeUndefined();
    expect(invalid.blocks.map((entry) => entry.type)).toEqual(["paragraph"]);
  });

  it("copies child notes as readable local references and pastes independent page copies between text", () => {
    const noteId = "10000000-0000-4000-8000-000000000001";
    const entries = [block("paragraph", "Before", "first"), block("child_page", "A [note]", noteId), block("paragraph", "After", "last")];
    const clipboard = notesClipboardContent(entries.map((block) => ({ block })));
    expect(clipboard.plainText).toBe(`Before\n\n[A \\[note\\]](#notes?page=${noteId})\n\nAfter`);
    const result = pasted(clipboard.plainText, clipboard.html);
    expect(result.blocks.map((block) => block.type)).toEqual(["paragraph", "child_page", "paragraph"]);
    expect(result.plan?.copiedPageIds).toEqual({ "new-1": noteId });
    expect(blockPlainText(result.blocks[1])).toBe("A [note]");
  });

  it("keeps text on both sides of a pasted note outside its title", () => {
    const note = block("child_page", "", "10000000-0000-4000-8000-000000000001");
    const clipboard = notesClipboardContent([{ block: note, start: 0, end: 1 }]);
    const { blocks, plan } = pasted(clipboard.plainText, clipboard.html, "BeforeAfter", 6);
    expect(blocks.map(blockPlainText)).toEqual(["Before", "", "After"]);
    expect(plan?.copiedPageIds).toEqual({ "new-1": note.id });
    expect(notesClipboardContent([{ block: note, start: 1 }]).plainText).toBe("");
    expect(notesClipboardContent([{ block: note, end: 0 }]).plainText).toBe("");
  });

  it("treats an invalid copied note identity as readable text", () => {
    const { plan, blocks } = pasted("Readable", '<p data-notes-child-page-id="invalid">Readable</p>');
    expect(plan?.copiedPageIds).toBeUndefined();
    expect(blocks.map(blockPlainText)).toEqual(["Readable"]);
  });

  it("pastes a plain local note reference as a page mention without duplicating the note", () => {
    const id = "10000000-0000-4000-8000-000000000001";
    const clipboard = notesClipboardContent([{ block: block("child_page", "Note", id) }]);
    const { plan, blocks } = pasted(clipboard.plainText);
    expect(plan?.copiedPageIds).toBeUndefined();
    expect(blockEditableRichText(blocks[0])[0]).toMatchObject({ type: "mention", mention: { type: "page", page: { id } } });
  });

  it("imports Notion aside text as a callout with heading and paragraph children", () => {
    const text = "Normal text\n\n<aside>\n💡\n\n# Title\n\nOne\n\nTwo\n\nThree\n\n</aside>\n\nNormal text";
    const { plan, blocks } = pasted(text);
    expect(blocks.map((item) => item.type)).toEqual([
      "paragraph", "callout", "heading_1", "paragraph", "paragraph", "paragraph", "paragraph",
    ]);
    expect(blocks.map(blockPlainText)).toEqual(["Normal text", "", "Title", "One", "Two", "Three", "Normal text"]);
    expect(plan?.blockDepths).toEqual([0, 0, 1, 1, 1, 1, 0]);
    expect(blocks[1]).toMatchObject({ callout: { icon: { type: "emoji", emoji: "💡" } } });
  });

  it("copies nested callouts as asides and restores their icons, colors, and structure", () => {
    const callout = block("callout", "", "callout");
    if (callout.type !== "callout") throw new Error("Expected callout");
    callout.callout.color = "blue_background";
    callout.callout.icon = { type: "emoji", emoji: "💡" };
    const heading = block("heading_1", "Title", "heading");
    heading.parent = { type: "block_id", block_id: callout.id };
    const nested = block("callout", "", "nested");
    nested.parent = { type: "block_id", block_id: callout.id };
    if (nested.type !== "callout") throw new Error("Expected nested callout");
    nested.callout.icon = { type: "emoji", emoji: "⚠️" };
    const body = block("paragraph", "Check this", "body");
    body.parent = { type: "block_id", block_id: nested.id };
    const content = notesClipboardContent([callout, heading, nested, body].map((item) => ({ block: item })));
    expect(content.plainText).toContain("<aside>\n💡\n\n# Title");
    expect(content.plainText).not.toContain("> # Title");
    expect(content.html).toContain("<aside data-notes-callout-icon-json=");
    const rich = pasted(content.plainText, content.html);
    expect(rich.blocks.map((item) => item.type)).toEqual(["callout", "heading_1", "callout", "paragraph"]);
    expect(rich.plan?.blockDepths).toEqual([0, 1, 1, 2]);
    expect(rich.blocks[0]).toMatchObject({ callout: { color: "blue_background", icon: { type: "emoji", emoji: "💡" } } });
    expect(rich.blocks[2]).toMatchObject({ callout: { icon: { type: "emoji", emoji: "⚠️" } } });
    const plain = pasted(content.plainText);
    expect(plain.blocks.map((item) => item.type)).toEqual(["callout", "heading_1", "callout", "paragraph"]);
    expect(plain.plan?.blockDepths).toEqual([0, 1, 1, 2]);
  });

  it("preserves a closed toggle through HTML and provides a readable plain-text list", () => {
    const toggle = block("toggle", "Details", "toggle");
    if (toggle.type !== "toggle") throw new Error("Expected toggle");
    toggle.toggle.ganbaru_open = false;
    const paragraph = block("paragraph", "Inside", "child");
    paragraph.parent = { type: "block_id", block_id: toggle.id };
    const bullet = block("bulleted_list_item", "Nested", "bullet");
    bullet.parent = { type: "block_id", block_id: toggle.id };
    const content = notesClipboardContent([toggle, paragraph, bullet, block("paragraph", "Outside", "after")]
      .map((block) => ({ block })));
    expect(content.plainText).toBe("- Details\n\n    Inside\n    \n    - Nested\n\nOutside");
    expect(content.html).toContain('<details open data-notes-toggle-open="false"><summary>Details</summary><p>Inside</p><ul><li>Nested</li></ul></details>');
    const { plan, blocks } = pasted(content.plainText, content.html);
    expect(blocks.map((block) => block.type)).toEqual(["toggle", "paragraph", "bulleted_list_item", "paragraph"]);
    expect(blocks.map(blockPlainText)).toEqual(["Details", "Inside", "Nested", "Outside"]);
    expect(plan?.blockDepths).toEqual([0, 1, 1, 0]);
    expect(blocks[0]).toMatchObject({ toggle: { ganbaru_open: false } });
    const plainOnly = pasted(content.plainText);
    expect(plainOnly.blocks.map(blockPlainText)).toEqual(["Details", "Inside", "Nested", "Outside"]);
    expect(plainOnly.blocks[0].type).toBe("bulleted_list_item");
  });

  it("converts a foldable Obsidian callout with an inline body into an open toggle", () => {
    const { plan, blocks } = pasted("> [!faq]+ Question\n> Answer\n>\n> - Next step");
    expect(blocks.map((block) => block.type)).toEqual(["toggle", "paragraph", "bulleted_list_item"]);
    expect(blocks.map(blockPlainText)).toEqual(["Question", "Answer", "Next step"]);
    expect(plan?.blockDepths).toEqual([0, 1, 1]);
    expect(blocks[0]).toMatchObject({ toggle: { ganbaru_open: true } });
  });

  it("uses matching foldable Markdown when rich HTML contains only a styled callout wrapper", () => {
    const text = "> [!note]- Details\n> Body";
    const html = '<div class="callout"><div class="callout-title">Details</div><div class="callout-content"><p>Body</p></div></div>';
    const { plan, blocks } = pasted(text, html);
    expect(blocks.map((block) => block.type)).toEqual(["toggle", "paragraph"]);
    expect(blocks.map(blockPlainText)).toEqual(["Details", "Body"]);
    expect(plan?.blockDepths).toEqual([0, 1]);
    expect(blocks[0]).toMatchObject({ toggle: { ganbaru_open: false } });
    expect(notesClipboardPasteHtml(text, "<p>Unrelated content</p>")).toBe("<p>Unrelated content</p>");
  });

  it("keeps surrounding paragraph text outside a pasted toggle", () => {
    const { plan, blocks } = pasted("", "<details><summary>Details</summary><p>Inside</p></details>",
      "Before after", 7);
    expect(blocks.map((block) => block.type)).toEqual(["paragraph", "toggle", "paragraph", "paragraph"]);
    expect(blocks.map(blockPlainText)).toEqual(["Before ", "Details", "Inside", "after"]);
    expect(plan?.blockDepths).toEqual([0, 0, 1, 0]);
  });

  it("focuses the title instead of a hidden child after pasting a closed toggle", () => {
    const { plan, blocks } = pasted("> [!note]- Closed\n>\n> Body");
    expect(blocks.map((block) => block.type)).toEqual(["toggle", "paragraph"]);
    expect(plan?.focusBlockId).toBe("block");
    expect(plan?.focusOffset).toBe("Closed".length);
  });

  it.each(["html", "markdown"])("preserves table cells, headers, inline styles, and insertion boundaries via %s", (format) => {
    const html = '<table><thead><tr><th>Name</th><th>Value</th></tr></thead><tbody><tr><td><strong>A</strong></td><td>B<br>C</td></tr></tbody></table>';
    const text = '| Name | Value |\n| --- | --- |\n| **A** | B<br>C |';
    const { plan, blocks } = pasted(text, format === "html" ? html : "", "Before after", 7);
    expect(blocks.map((block) => block.type)).toEqual(["paragraph", "table", "table_row", "table_row", "paragraph"]);
    expect(blockPlainText(blocks[0])).toBe("Before ");
    expect(blockPlainText(blocks.at(-1)!)).toBe("after");
    expect(plan?.blockDepths).toEqual([0, 0, 1, 1, 0]);
    expect(blocks[1]).toMatchObject({ table: { table_width: 2, has_column_header: true } });
    expect(blocks[3]).toMatchObject({ table_row: { cells: [
      [expect.objectContaining({ plain_text: "A", annotations: expect.objectContaining({ bold: true }) })],
      [expect.objectContaining({ plain_text: "B\nC" })],
    ] } });
  });

  it("keeps cell text and blank cells when flattening merged table cells", () => {
    const { blocks } = pasted("", '<table><tr><th colspan="2">Group</th></tr><tr><td rowspan="2">A</td><td>B</td></tr><tr><td>C</td></tr></table>');
    const rows = blocks.filter((block) => block.type === "table_row");
    expect(rows.map((row) => row.table_row.cells.map((cell) => cell.map((run) => run.plain_text).join("")))).toEqual([["Group", ""], ["A", "B"], ["", "C"]]);
  });

  it("preserves spaces, blank paragraphs, blank code, and Unicode in HTML round trips", () => {
    const entries = [block("paragraph", "two  spaces\tand 👩🏽‍💻 café", "a"), block("paragraph", "", "b"), block("code", "", "c"), block("paragraph", "end", "d")];
    const content = notesClipboardContent(entries.map((block) => ({ block })));
    const { blocks } = pasted(content.plainText, content.html);
    expect(blocks.map(blockPlainText)).toEqual(entries.map(blockPlainText));
    expect(blocks.map((block) => block.type)).toEqual(entries.map((block) => block.type));
  });

  it("preserves quote paragraph boundaries without adding trailing newlines", () => {
    expect(pasted("> First\n>\n> Second").blocks.map(blockPlainText)).toEqual(["First\nSecond"]);
  });

  it("retains relative links and image descriptions without inventing external URLs", () => {
    const { blocks } = pasted('[Local](folder/note.md) and ![Diagram](assets/diagram.png)');
    const runs = blocks.flatMap(blockEditableRichText);
    expect(runs.map((run) => run.plain_text).join("")).toContain("folder/note.md");
    expect(runs.map((run) => run.plain_text).join("")).toContain("assets/diagram.png");
    expect(runs.every((run) => run.href === null)).toBe(true);
  });

  it("preserves HTML image references as readable text without fetching media", () => {
    const { blocks } = pasted("", '<p>See <img src="https://example.com/picture.png" alt="Picture"> here</p>');
    expect(blocks.map(blockPlainText).join("\n")).toContain("Picture");
    expect(blocks.map(blockPlainText).join("\n")).toContain("https://example.com/picture.png");
  });

  it("exports headerless tables without promoting the first data row to a header", () => {
    const table = block("table");
    const row = block("table_row", "", "row");
    if (table.type !== "table" || row.type !== "table_row") throw new Error("Expected table");
    table.table.has_column_header = false;
    row.parent = { type: "block_id", block_id: table.id };
    row.table_row.cells = [[createTextRichText("A|B")], [createTextRichText("C")]];
    const content = notesClipboardContent([{ block: table }, { block: row }]);
    const template = document.createElement("template");
    template.innerHTML = marked.parse(content.plainText, { async: false });
    expect(template.content.querySelector("thead")?.textContent?.trim()).toBe("");
    expect(template.content.querySelector("tbody td")?.textContent).toBe("A|B");
  });
  it("retains inline styles when flattening formatted clipboard content into a table cell", () => {
    const runs = notesInlineClipboardRichText(notesClipboardPasteHtml("**First**\n\n*Second*"))!;
    expect(runs.map((run) => run.plain_text).join("")).toBe("First\nSecond");
    expect(runs[0].annotations.bold).toBe(true);
    expect(runs.at(-1)?.annotations.italic).toBe(true);
  });

  it("keeps tables separate when inserted at the start of an existing heading", () => {
    const current = block("heading_1", "Existing");
    let id = 0;
    const plan = planNotesRichHtmlPaste({ currentBlock: current, selectionStart: 0, selectionEnd: 0,
      html: "<table><tr><td>Cell</td></tr></table>", createId: () => String(++id) })!;
    expect(plan.currentUpdate.type).toBe("heading_1");
    expect(plan.appendedBlocks.map((write) => write.type)).toEqual(["table", "table_row", "paragraph"]);
    expect(blockPlainText(applyBlockUpdate(current, plan.appendedBlocks.at(-1)!))).toBe("Existing");
  });

  it("retains all table text when a clipboard exceeds the structural block limit", () => {
    const html = `<table>${Array.from({ length: 110 }, (_, index) => `<tr><td>Cell ${index}</td></tr>`).join("")}</table>`;
    const { blocks } = pasted("", html);
    expect(blocks.every((block) => block.type === "paragraph")).toBe(true);
    expect(blocks.map(blockPlainText).join("\n")).toContain("Cell 109");
  });

  it("keeps combined underline and emphasis semantic without Markdown inside HTML", () => {
    const source = block("paragraph", "Styled");
    const run = blockEditableRichText(source)[0];
    run.annotations.underline = true;
    run.annotations.bold = true;
    run.annotations.italic = true;
    const content = notesClipboardContent([{ block: source }]);
    expect(content.plainText).toBe("<u><em><strong>Styled</strong></em></u>");
    const [result] = pasted(content.plainText).blocks;
    expect(blockEditableRichText(result)[0].annotations).toMatchObject({
      underline: true, bold: true, italic: true,
    });
  });

});
